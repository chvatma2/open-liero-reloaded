//! How a client reaches a host (N46): directly at an address, else (or
//! when that never opens) through Nostr relays, and the texts of each
//! step, path and failure.
//!
//! **Through Nostr**, in [`JoinStep`] order:
//! 1. **Relays:** a [`Pool`] on the join's relays (the caller orders them:
//!    an invite's hints first, then its own, [`join_relays`]). With
//!    `turn_from_nostr` it collects TURN relay notes until 1 s after the
//!    first `EOSE` (at most 4 s after the start).
//! 2. **Gathering:** a signalled endpoint whose TURN servers are the
//!    caller's own (the first 2) and then announced ones in the join's
//!    region, at most 2 in all.
//! 3. **Asking:** once gathered, an [`Asker`] with the endpoint's offer.
//!    Its first event leaves once a relay sent `EOSE` for the answer's
//!    subscription (or 2 s passed), so the answer cannot outrun it; then
//!    every resend. Every event goes to every relay: no join depends on
//!    one relay (some refuse handshakes from keys they do not know).
//! 4. **Connecting:** the host's reply goes to the endpoint.
//!
//! The pool is dropped on a thread of its own (it waits up to 2 s for its
//! drivers), never on the caller's. The Nostr side reads the system's unix
//! time (event dates).

use std::io;
use std::net::SocketAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rl_discovery::{Asker, Pool, PoolConfig, PoolEvent, Region, TurnList};
use rl_net::endpoint::{Endpoint, EndpointConfig, TurnServerSpec, Why};
use rl_net::probe::NatBehaviour;
use rl_net::relayed::RelayState;
use rl_proto::{Path, PublicKey, Signal};

use crate::events::OnlineEvent;
use crate::outbox::Outbox;
use crate::reach::Relays;

/// At most this many relays carry a join (N46).
pub const MAX_JOIN_RELAYS: usize = 8;
/// TURN notes are collected this long after the first relay's `EOSE` …
const TURN_AFTER_EOSE: Duration = Duration::from_secs(1);
/// … and at most this long after the join's start.
const TURN_AT_MOST: Duration = Duration::from_secs(4);
/// The offer leaves at the latest this long after the answer's
/// subscription, without any relay's `EOSE`.
const EOSE_WAIT: Duration = Duration::from_secs(2);
/// At most this many TURN servers are allocated on (N48).
const MAX_TURN: usize = 2;
const TURN_SUB: &str = "turn";
const ANSWER_SUB: &str = "answer";

/// A join through Nostr: the host's key, the relays to ask on, where the
/// player is (for announced TURN relays), and whether to use them.
#[derive(Clone, Debug)]
pub struct NostrJoin {
    pub host: PublicKey,
    pub pool: PoolConfig,
    pub region: Region,
    /// Collect announced TURN relays (kind 31471) and allocate on the
    /// nearest when the player's own are fewer than 2.
    pub turn_from_nostr: bool,
    /// TURN notes at local or private addresses are used too (N62), and
    /// the endpoint accepts their relayed addresses.
    pub allow_local: bool,
}

/// How to reach a host: its address, its key on Nostr, or both (the
/// address first, Nostr if the link never opened).
#[derive(Clone, Debug)]
pub struct Join {
    pub direct: Option<SocketAddr>,
    pub nostr: Option<NostrJoin>,
}

impl Join {
    /// A typed address or a LAN game: direct only.
    pub fn direct(addr: SocketAddr) -> Join {
        Join { direct: Some(addr), nostr: None }
    }
}

/// Where a join stands, for the player's waiting text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinStep {
    Direct,
    Relays,
    Gathering,
    Asking,
    Connecting,
}

impl std::fmt::Display for JoinStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            JoinStep::Direct => "Reaching the game directly…",
            JoinStep::Relays => "Connecting to the relays…",
            JoinStep::Gathering => "Looking for paths through your router…",
            JoinStep::Asking => "Asking the game through the relays…",
            JoinStep::Connecting => "Connecting…",
        })
    }
}

/// Every relay failed before the host answered.
pub const NO_RELAYS: &str =
    "could not reach any Nostr relay: check your Internet connection, or the relays in Options → Online";
/// The host's answer did not come within `ANSWER_DEADLINE` (FR12b-3: a
/// clock off by more than a minute on either PC fails so too).
pub const NO_REPLY: &str = "the game did not answer through the relays (it may have closed, its relays may differ \
     from yours, or one of the two PCs' clocks is off by more than a minute)";
/// Every relay that could be reached refused this join's handshake events
/// (some take events only from keys they know, NIP-01's `blocked:` or
/// `restricted:`): not the host's doing.
pub const RELAYS_REFUSED: &str = "the relays refused this join's handshake (some take events only from keys they know): try other relays in \
     Options → Online";
/// Every relay that could be reached refused this join's handshake events,
/// one at least as `invalid:` (NIP-01): an event dated too far from the
/// relay's clock, or expired at once, is the likeliest (final review
/// B-M2: not web of trust).
pub const RELAYS_INVALID: &str = "the relays refused this join's handshake as invalid: is this PC's clock right? \
     (set it automatically)";
/// The host's endpoint had no room for another signalled setup
/// (`Refused { Busy }` through Nostr: every pending setup is live).
pub const BUSY: &str = "the game is busy with other joins: try again in a moment";

/// The link's setup timed out after the host's answer: no path found.
/// `port` is the host's first candidate's, `relayed` the TURN relay used.
pub fn no_path(port: u16, relayed: Option<SocketAddr>) -> String {
    match relayed {
        None => format!(
            "no network path to the game: hole punching failed and no TURN relay could be used. The host can open \
             port {port} (TCP and UDP) on their router (UPnP, or port forwarding) or use a dedicated server; anyone \
             can run a TURN relay (rl-server --relay)"
        ),
        Some(addr) => format!(
            "no network path to the game, not even through the TURN relay {addr}: try again, or ask the host to \
             open port {port} on their router"
        ),
    }
}

/// How a client's link reaches its host, in words (`remote`: the host's
/// end, or for `Relayed` the TURN relay's address, FR12b-4).
pub fn path_text(path: Path, remote: SocketAddr) -> String {
    match path {
        Path::Direct => format!("connected directly to {remote}"),
        Path::MappedPort => format!("connected through the host's open port {remote}"),
        Path::HolePunched => "connected by hole punching through both routers".into(),
        Path::Relayed => format!("connected through the TURN relay {remote}"),
    }
}

/// A member's path in a word or two (the lobby's tags, the server's log).
pub fn path_tag(path: Path) -> &'static str {
    match path {
        Path::Direct => "direct",
        Path::MappedPort => "open port",
        Path::HolePunched => "punched",
        Path::Relayed => "relayed",
    }
}

/// The NAT behaviour in words (the reach line's and `rl-server --nat-check`'s): "no NAT", "a NAT that keeps ports (hole punching works)", "symmetric NAT", "not known yet".
pub fn nat_text(nat: NatBehaviour) -> &'static str {
    match nat {
        NatBehaviour::Unknown => "not known yet",
        NatBehaviour::Open => "no NAT",
        NatBehaviour::Preserving => "a NAT that keeps ports (hole punching works)",
        NatBehaviour::Symmetric => "symmetric NAT",
    }
}

/// A join's relays (N46, ruling PF12b-3): the invite's `hints` first, then
/// the player's `own`, each once, at most [`MAX_JOIN_RELAYS`].
pub fn join_relays(hints: &[String], own: &[String]) -> Vec<String> {
    let mut relays: Vec<String> = Vec::new();
    for r in hints.iter().chain(own) {
        if relays.len() < MAX_JOIN_RELAYS && !relays.contains(r) {
            relays.push(r.clone());
        }
    }
    relays
}

/// The system's unix time in seconds (Nostr's event dates).
pub(crate) fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn rand() -> [u8; 32] {
    rl_net::random_bytes::<32>().unwrap_or([0; 32])
}

/// Drops `pool` on a thread of its own (it waits up to 2 s for its
/// drivers; ruling R10-10). If no thread can start, the closure, and the
/// pool with it, is dropped here.
fn drop_pool(pool: Pool) {
    let _ = std::thread::Builder::new().name("rl-online pool drop".into()).spawn(move || drop(pool));
}

/// The port of the first `a=candidate` line of `sdp` (0 without one).
fn first_candidate_port(sdp: &str) -> u16 {
    sdp.lines()
        .find_map(|l| l.trim_end().strip_prefix("a=candidate:"))
        .and_then(|c| c.split_ascii_whitespace().nth(5))
        .and_then(|p| p.parse().ok())
        .unwrap_or(0)
}

/// A relay's verdict on the join's handshake events.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    None,
    Accepted,
    Refused,
    /// Refused as `invalid:` (B-M2: a clock off, likeliest).
    Invalid,
}

/// A refusal by policy: NIP-01's machine-readable prefix (`blocked:`,
/// `restricted:`, `invalid:` …), not the pool's own "no answer within …"
/// for a slow relay, and not a transient `rate-limited:` or `error:` (the
/// resends get their chance).
fn refusal(message: &str) -> bool {
    message.split_once(':').is_some_and(|(prefix, _)| {
        !prefix.is_empty()
            && !matches!(prefix, "duplicate" | "rate-limited" | "error")
            && prefix.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
    })
}

/// The Nostr side of a join, from the pool to the host's reply.
struct Nostr {
    join: NostrJoin,
    pool: Option<Pool>,
    relays: Relays,
    /// When the join through Nostr began (its first poll).
    started: Option<Instant>,
    /// Collecting TURN notes, and the first `EOSE` of their subscription.
    turn: Option<(TurnList, Option<Instant>)>,
    /// TURN servers to allocate on, once the notes are in.
    turn_servers: Option<Vec<TurnServerSpec>>,
    endpoint_made: bool,
    asker: Option<Asker>,
    /// When the answer's subscription was made, and whether a relay sent
    /// its `EOSE`.
    asked: Option<Instant>,
    answer_eose: bool,
    /// The ids of the offer events published.
    published: Vec<[u8; 32]>,
    verdicts: Vec<Verdict>,
    /// The host's first candidate's port, once its answer is taken.
    answered: Option<u16>,
    /// The TURN relay an allocation was made on.
    relayed: Option<SocketAddr>,
    /// Seconds added to the system's clock for the handshake's dates (a
    /// test hook's skewed clock; 0).
    clock: i64,
}

impl Nostr {
    fn start(mut join: NostrJoin) -> Nostr {
        join.pool.relays = join_relays(&join.pool.relays, &[]);
        let n = join.pool.relays.len();
        let mut pool = Pool::start(join.pool.clone());
        if join.turn_from_nostr {
            pool.subscribe(TURN_SUB, vec![TurnList::filter(unix_now())]);
        }
        let turn = join.turn_from_nostr.then(|| {
            let mut list = TurnList::new();
            list.allow_local(join.allow_local);
            (list, None)
        });
        Nostr {
            join,
            pool: Some(pool),
            relays: Relays::new(n),
            started: None,
            turn,
            turn_servers: None,
            endpoint_made: false,
            asker: None,
            asked: None,
            answer_eose: false,
            published: Vec::new(),
            verdicts: vec![Verdict::None; n],
            answered: None,
            relayed: None,
            clock: 0,
        }
    }

    /// The unix time the handshake's events are dated by.
    fn unix(&self) -> u64 {
        unix_now().saturating_add_signed(self.clock)
    }

    /// Every relay either refused the join's events or could never be
    /// reached, and one refused.
    fn all_refused(&self) -> bool {
        let refused = |i: usize| matches!(self.verdicts[i], Verdict::Refused | Verdict::Invalid);
        (0..self.verdicts.len()).any(refused)
            && (0..self.verdicts.len()).all(|i| refused(i) || self.relays.unreachable(i))
    }

    fn poll(
        &mut self,
        now: Instant,
        cfg: &EndpointConfig,
        outbox: &mut Outbox,
        events: &mut Vec<OnlineEvent>,
    ) -> Result<(), String> {
        let started = *self.started.get_or_insert(now);
        let unix = self.unix();
        let mut got = Vec::new();
        if let Some(pool) = &mut self.pool {
            pool.poll(&mut got);
        }
        for e in got {
            match e {
                PoolEvent::Status { relay, status } => self.relays.status(relay, &status),
                PoolEvent::Eose { sub, .. } if sub == TURN_SUB => {
                    if let Some((_, eose)) = &mut self.turn {
                        eose.get_or_insert(now);
                    }
                }
                PoolEvent::Eose { sub, .. } if sub == ANSWER_SUB => self.answer_eose = true,
                PoolEvent::Event { event, .. } => {
                    if let Some((list, _)) = &mut self.turn {
                        list.offer(&event, unix);
                    }
                    let reply = match &mut self.asker {
                        Some(asker) if self.answered.is_none() => asker.take(&event),
                        _ => None,
                    };
                    if let (Some(reply), Some(ep)) = (reply, outbox.endpoint_mut()) {
                        ep.take_reply(&reply.signal, reply.mapped, now);
                        // A refusal ends the join with the endpoint's
                        // `Refused` (next poll): no step to report.
                        let port = match &reply.signal {
                            Signal::Answer { sdp } => {
                                events.push(OnlineEvent::Joining(JoinStep::Connecting));
                                first_candidate_port(sdp)
                            }
                            _ => 0,
                        };
                        self.answered = Some(port);
                    }
                }
                PoolEvent::Ok { relay, id, accepted, message } if self.published.contains(&id) => {
                    if let Some(v) = self.verdicts.get_mut(relay) {
                        if accepted {
                            *v = Verdict::Accepted;
                        } else if *v == Verdict::None && refusal(&message) {
                            *v = if message.starts_with("invalid:") {
                                Verdict::Invalid
                            } else {
                                Verdict::Refused
                            };
                        }
                    }
                }
                PoolEvent::Eose { .. } | PoolEvent::Ok { .. } => {}
            }
        }
        if self.answered.is_some() {
            if self.relayed.is_none()
                && let Some(ep) = outbox.endpoint()
            {
                self.relayed = ep
                    .reach()
                    .relays
                    .iter()
                    .find(|(_, s)| matches!(s, RelayState::Allocated(_)))
                    .map(|(a, _)| *a);
            }
            return Ok(());
        }
        if self.relays.none_reachable() {
            return Err(NO_RELAYS.into());
        }
        if self.all_refused() {
            let invalid = self.verdicts.contains(&Verdict::Invalid);
            return Err(if invalid { RELAYS_INVALID } else { RELAYS_REFUSED }.into());
        }
        // 1. The TURN notes, then the servers to allocate on.
        if let Some((list, eose)) = &self.turn {
            let done = eose.is_some_and(|t| now >= t + TURN_AFTER_EOSE) || now >= started + TURN_AT_MOST;
            if !done {
                return Ok(());
            }
            let own: Vec<TurnServerSpec> = cfg.ice.turn.iter().take(MAX_TURN).cloned().collect();
            let picks = list.pick(self.join.region, MAX_TURN.saturating_sub(own.len()));
            let mut servers = own;
            servers.extend(picks.into_iter().map(|p| TurnServerSpec {
                addr: p.addr.to_string(),
                user: p.user,
                pass: p.pass,
            }));
            self.turn_servers = Some(servers);
            self.turn = None;
            if let Some(pool) = &mut self.pool {
                pool.unsubscribe(TURN_SUB);
            }
        }
        // 2. The signalled endpoint gathers.
        if !self.endpoint_made {
            let mut c = cfg.clone();
            c.ice.turn = match self.turn_servers.take() {
                Some(servers) => servers,
                None => cfg.ice.turn.iter().take(MAX_TURN).cloned().collect(),
            };
            c.ice.allow_local |= self.join.allow_local;
            let ep = Endpoint::signalled(c)
                .map_err(|e| format!("this game could not open its network socket: {e}"))?;
            outbox.set_endpoint(ep);
            self.endpoint_made = true;
            events.push(OnlineEvent::Joining(JoinStep::Gathering));
        }
        // 3. Asking, once gathered.
        if self.asker.is_none() {
            let Some(offer) = outbox.endpoint_mut().filter(|ep| ep.gathered()).and_then(|ep| ep.offer(now))
            else {
                return Ok(());
            };
            let asker = Asker::new(self.join.host, offer, now, unix, &mut || rand());
            if let Some(pool) = &mut self.pool {
                pool.subscribe(ANSWER_SUB, vec![asker.filter(unix)]);
            }
            self.asker = Some(asker);
            self.asked = Some(now);
            events.push(OnlineEvent::Joining(JoinStep::Asking));
        }
        let (Some(asker), Some(asked)) = (&mut self.asker, self.asked) else { return Ok(()) };
        if asker.expired(now) {
            return Err(NO_REPLY.into());
        }
        if (self.answer_eose || now >= asked + EOSE_WAIT)
            && let Some(e) = asker.poll(now, unix, &mut || rand())
            && let Some(pool) = &mut self.pool
        {
            self.published.push(e.id);
            pool.publish(e);
        }
        Ok(())
    }
}

impl Drop for Nostr {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            drop_pool(pool);
        }
    }
}

enum Stage {
    /// The direct attempt's endpoint is in the outbox.
    Direct,
    Nostr(Box<Nostr>),
}

/// A client's join until its link opens: the direct attempt, then Nostr.
pub(crate) struct Joiner {
    cfg: EndpointConfig,
    nostr: Option<NostrJoin>,
    stage: Stage,
    /// The handshake's clock offset (`set_clock`, a test hook; 0).
    clock: i64,
}

/// Why a join's link was lost before it opened.
pub(crate) enum Lost {
    /// The direct attempt failed: joining through Nostr now.
    FellBack,
    /// The join ends with this text.
    Text(String),
    /// The join ends with the endpoint's usual text.
    Usual,
}

impl Joiner {
    /// Starts `join` with the client's endpoint `cfg`: the direct
    /// attempt's endpoint (to put in the outbox), or Nostr's pool; the
    /// first step to report.
    pub(crate) fn start(join: Join, cfg: EndpointConfig) -> io::Result<(Joiner, Option<Endpoint>, JoinStep)> {
        match join {
            Join { direct: Some(addr), nostr } => {
                let ep = Endpoint::connect(cfg.clone(), addr)?;
                Ok((Joiner { cfg, nostr, stage: Stage::Direct, clock: 0 }, Some(ep), JoinStep::Direct))
            }
            Join { direct: None, nostr: Some(n) } => {
                let stage = Stage::Nostr(Box::new(Nostr::start(n)));
                Ok((Joiner { cfg, nostr: None, stage, clock: 0 }, None, JoinStep::Relays))
            }
            Join { direct: None, nostr: None } => {
                Err(io::Error::new(io::ErrorKind::InvalidInput, "a join needs an address or a host key"))
            }
        }
    }

    /// Dates the handshake's events `secs` off the system's clock (tests:
    /// a joiner whose clock is wrong).
    #[cfg(feature = "test-hooks")]
    pub(crate) fn set_clock(&mut self, secs: i64) {
        self.clock = secs;
        if let Stage::Nostr(n) = &mut self.stage {
            n.clock = secs;
        }
    }

    /// Through Nostr (not a direct attempt).
    pub(crate) fn through_nostr(&self) -> bool {
        matches!(self.stage, Stage::Nostr(_))
    }

    /// Drives the join through Nostr; `Err` ends it with the text.
    pub(crate) fn poll(
        &mut self,
        now: Instant,
        outbox: &mut Outbox,
        events: &mut Vec<OnlineEvent>,
    ) -> Result<(), String> {
        match &mut self.stage {
            Stage::Direct => Ok(()),
            Stage::Nostr(n) => n.poll(now, &self.cfg, outbox, events),
        }
    }

    /// The link was lost before it opened (`Disconnected { HOST, why }`).
    pub(crate) fn lost(&mut self, why: Why, outbox: &mut Outbox, events: &mut Vec<OnlineEvent>) -> Lost {
        match &self.stage {
            Stage::Direct => match self.nostr.take() {
                Some(n) => {
                    // The direct endpoint goes (a client's has no mapping).
                    drop(outbox.take_endpoint());
                    let mut nostr = Nostr::start(n);
                    nostr.clock = self.clock;
                    self.stage = Stage::Nostr(Box::new(nostr));
                    events.push(OnlineEvent::Joining(JoinStep::Relays));
                    Lost::FellBack
                }
                None => Lost::Usual,
            },
            Stage::Nostr(n) => match (n.answered, why) {
                (Some(port), Why::Timeout | Why::Closed) => Lost::Text(no_path(port, n.relayed)),
                _ => Lost::Usual,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_as_written() {
        let texts: Vec<String> =
            [JoinStep::Direct, JoinStep::Relays, JoinStep::Gathering, JoinStep::Asking, JoinStep::Connecting]
                .iter()
                .map(ToString::to_string)
                .collect();
        assert_eq!(
            texts,
            [
                "Reaching the game directly…",
                "Connecting to the relays…",
                "Looking for paths through your router…",
                "Asking the game through the relays…",
                "Connecting…"
            ]
        );
    }

    #[test]
    fn paths_and_failures_read_as_written() {
        let at: SocketAddr = "203.0.113.7:23400".parse().unwrap();
        assert_eq!(path_text(Path::Direct, at), "connected directly to 203.0.113.7:23400");
        assert_eq!(
            path_text(Path::MappedPort, at),
            "connected through the host's open port 203.0.113.7:23400"
        );
        assert_eq!(path_text(Path::HolePunched, at), "connected by hole punching through both routers");
        assert_eq!(path_text(Path::Relayed, at), "connected through the TURN relay 203.0.113.7:23400");
        let tags: Vec<&str> = [Path::Direct, Path::MappedPort, Path::HolePunched, Path::Relayed]
            .into_iter()
            .map(path_tag)
            .collect();
        assert_eq!(tags, ["direct", "open port", "punched", "relayed"]);
        let nats: Vec<&str> =
            [NatBehaviour::Open, NatBehaviour::Preserving, NatBehaviour::Symmetric, NatBehaviour::Unknown]
                .into_iter()
                .map(nat_text)
                .collect();
        assert_eq!(
            nats,
            ["no NAT", "a NAT that keeps ports (hole punching works)", "symmetric NAT", "not known yet"]
        );
        assert_eq!(
            no_path(23400, None),
            "no network path to the game: hole punching failed and no TURN relay could be used. The host can open \
             port 23400 (TCP and UDP) on their router (UPnP, or port forwarding) or use a dedicated server; anyone \
             can run a TURN relay (rl-server --relay)"
        );
        assert_eq!(
            no_path(23400, Some("198.51.100.9:3478".parse().unwrap())),
            "no network path to the game, not even through the TURN relay 198.51.100.9:3478: try again, or ask the \
             host to open port 23400 on their router"
        );
        assert_eq!(
            NO_RELAYS,
            "could not reach any Nostr relay: check your Internet connection, or the relays in Options → Online"
        );
        assert_eq!(
            NO_REPLY,
            "the game did not answer through the relays (it may have closed, its relays may differ from yours, or \
             one of the two PCs' clocks is off by more than a minute)"
        );
    }

    /// PF12b-3: the invite's hints first, then the player's own, each once,
    /// at most 8.
    #[test]
    fn a_joins_relays_put_the_hints_first() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let hints = s(&["wss://b", "wss://a"]);
        let own =
            s(&["wss://a", "wss://c", "wss://d", "wss://e", "wss://f", "wss://g", "wss://h", "wss://i"]);
        assert_eq!(
            join_relays(&hints, &own),
            s(&["wss://b", "wss://a", "wss://c", "wss://d", "wss://e", "wss://f", "wss://g", "wss://h"])
        );
        assert_eq!(join_relays(&[], &s(&["wss://x", "wss://x"])), s(&["wss://x"]));
    }

    #[test]
    fn refusals_are_told_from_a_slow_relay() {
        assert!(refusal("blocked: kind 21470 not accepted"));
        assert!(refusal("restricted: not in web of trust"));
        assert!(refusal("invalid: bad signature"));
        assert!(!refusal("no answer within 5 s"));
        assert!(!refusal("duplicate: already have it"));
        assert!(!refusal("rate-limited: slow down"), "transient: the resends get their chance");
        assert!(!refusal("error: could not save"));
        assert!(!refusal(""));
    }

    #[test]
    fn the_answers_first_candidate_names_the_port() {
        let sdp = "v=0\r\na=ice-ufrag:x\r\na=candidate:1 1 udp 2130706431 192.168.1.5 23401 typ host\r\n\
                   a=candidate:2 1 udp 1694498815 203.0.113.7 40000 typ srflx\r\n";
        assert_eq!(first_candidate_port(sdp), 23401);
        assert_eq!(first_candidate_port("v=0\r\n"), 0);
        assert_eq!(first_candidate_port("a=candidate:1 1 udp 1 1.2.3.4 notaport typ host\r\n"), 0);
    }
}
