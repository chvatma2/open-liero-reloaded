//! STUN on the game socket (decision N48): binding requests from the one
//! UDP socket every link shares, so the server-reflexive address they
//! learn is the one peers can reach, and the router's mapping stays alive
//! between rounds. Sans-IO: [`crate::core::Core`] hands the requests out
//! with its datagrams and gives the answers back.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rl_netcode::Rng;
use rl_proto::stun::{binding_request, parse_binding_response};

use crate::core::{Datagram, SetupId};

/// What two STUN servers' answers say about the router (N48).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NatBehaviour {
    #[default]
    Unknown,
    /// srflx == base: no NAT
    Open,
    /// the same mapped port toward two servers
    Preserving,
    /// different mapped ports: symmetric NAT
    Symmetric,
}

/// A STUN round every this long: the answers stay current and the
/// router's mapping (often 30 s or more idle) stays alive.
pub const STUN_EVERY: Duration = Duration::from_secs(25);
/// Gathering waits at most this long for the first round's answers.
pub const STUN_WAIT: Duration = Duration::from_secs(1);
/// An unanswered request is sent once more after this long.
const RETRY: Duration = Duration::from_millis(500);
/// At most this many servers are asked.
const MAX_SERVERS: usize = 4;
/// The setup of the probe's datagrams: no link's (`Core::sent` ignores them).
pub(crate) const PROBE_SETUP: SetupId = SetupId::MAX;

/// One request of a round.
struct Ask {
    server: SocketAddr,
    transaction: [u8; 12],
    answer: Option<SocketAddr>,
}

struct Round {
    start: Instant,
    asks: Vec<Ask>,
    /// The requests went out (`sent`), and again (`retried`).
    sent: bool,
    retried: bool,
    /// The round's end was looked at (its answers taken).
    closed: bool,
}

impl Round {
    fn answered(&self) -> impl Iterator<Item = (SocketAddr, SocketAddr)> + '_ {
        self.asks.iter().filter_map(|a| a.answer.map(|m| (a.server, m)))
    }

    fn complete(&self, now: Instant) -> bool {
        self.asks.iter().all(|a| a.answer.is_some()) || now >= self.start + STUN_WAIT
    }
}

/// The STUN rounds of one socket.
pub(crate) struct StunProbe {
    servers: Vec<SocketAddr>,
    round: Option<Round>,
    /// When the next round starts.
    next: Option<Instant>,
    /// The first round, for `gathered`.
    first: Option<Instant>,
    first_done: bool,
    /// The server whose answers are `srflx`: the first that answered (or,
    /// when it goes silent for a round, the first of the list that
    /// answered then).
    primary: Option<SocketAddr>,
    srflx: Option<SocketAddr>,
    /// From two answers of one round (`Open` is judged against the base).
    nat: NatBehaviour,
    rng: Rng,
}

impl StunProbe {
    pub(crate) fn new(seed: u64) -> StunProbe {
        StunProbe {
            servers: Vec::new(),
            round: None,
            next: None,
            first: None,
            // Nothing asked, nothing to wait for.
            first_done: true,
            primary: None,
            srflx: None,
            nat: NatBehaviour::Unknown,
            rng: Rng::new(seed),
        }
    }

    /// Asks `servers` (IPv4, the first few, duplicates dropped) now and
    /// every [`STUN_EVERY`]; an empty list stops the rounds.
    pub(crate) fn start(&mut self, servers: Vec<SocketAddr>, now: Instant) {
        let mut kept: Vec<SocketAddr> = Vec::new();
        for s in servers.into_iter().filter(SocketAddr::is_ipv4) {
            if !kept.contains(&s) && kept.len() < MAX_SERVERS {
                kept.push(s);
            }
        }
        self.servers = kept;
        self.round = None;
        self.next = (!self.servers.is_empty()).then_some(now);
        self.first = self.next;
        self.first_done = self.servers.is_empty();
    }

    /// The first round is over: every server answered, or [`STUN_WAIT`]
    /// passed (true when no STUN was asked for).
    pub(crate) fn gathered(&self, now: Instant) -> bool {
        self.first_done || self.first.is_some_and(|t| now >= t + STUN_WAIT)
    }

    pub(crate) fn srflx(&self) -> Option<SocketAddr> {
        self.srflx
    }

    pub(crate) fn nat(&self, base: Option<SocketAddr>) -> NatBehaviour {
        match (self.srflx, base) {
            (Some(s), Some(b)) if s == b => NatBehaviour::Open,
            _ => self.nat,
        }
    }

    /// Starts rounds and (re)sends their requests, appended to `out`.
    pub(crate) fn poll(&mut self, now: Instant, out: &mut Vec<Datagram>) {
        if let Some(round) = &self.round
            && !round.closed
            && round.complete(now)
        {
            self.close_round();
        }
        if self.next.is_some_and(|t| now >= t) {
            let asks = self
                .servers
                .iter()
                .map(|&server| {
                    let mut transaction = [0u8; 12];
                    transaction[..8].copy_from_slice(&self.rng.next_u64().to_le_bytes());
                    transaction[8..].copy_from_slice(&self.rng.next_u64().to_le_bytes()[..4]);
                    Ask { server, transaction, answer: None }
                })
                .collect();
            self.round = Some(Round { start: now, asks, sent: false, retried: false, closed: false });
            self.next = Some(now + STUN_EVERY);
        }
        let Some(round) = &mut self.round else { return };
        let resend = !round.retried && now >= round.start + RETRY;
        if round.sent && !resend {
            return;
        }
        for ask in round.asks.iter().filter(|a| a.answer.is_none()) {
            out.push(Datagram {
                to: ask.server,
                bytes: binding_request(ask.transaction),
                setup: PROBE_SETUP,
            });
        }
        round.retried |= round.sent;
        round.sent = true;
    }

    /// Takes a datagram from a server asked that answers an outstanding
    /// request of the current round; `false` for anything else (it goes to
    /// the links).
    pub(crate) fn receive(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) -> bool {
        let Some(round) = &mut self.round else { return false };
        let Some(ask) = round.asks.iter_mut().find(|a| a.server == from) else { return false };
        let Some(mapped) = parse_binding_response(bytes, &ask.transaction) else { return false };
        if ask.answer.is_some() {
            // A duplicate (the retry's answer): consumed.
            return true;
        }
        ask.answer = Some(mapped);
        let primary = *self.primary.get_or_insert(from);
        if primary == from {
            self.srflx = Some(mapped);
        }
        let mut answers = round.answered().map(|(_, m)| m);
        if let (Some(a), Some(b)) = (answers.next(), answers.next()) {
            self.nat = if a == b { NatBehaviour::Preserving } else { NatBehaviour::Symmetric };
        }
        drop(answers);
        if round.complete(now) && !round.closed {
            self.close_round();
        }
        true
    }

    /// The round is over: the first round gathered; a silent primary gives
    /// way to the first server of the list that answered.
    fn close_round(&mut self) {
        let Some(round) = &mut self.round else { return };
        round.closed = true;
        let primary_answered = round.answered().any(|(s, _)| Some(s) == self.primary);
        if !primary_answered && let Some((server, mapped)) = round.answered().next() {
            self.primary = Some(server);
            self.srflx = Some(mapped);
        }
        self.first_done = true;
    }
}
