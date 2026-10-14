//! The sans-IO endpoint (networking spec §5.1, §5.4, §9.4): every peer's
//! [`Link`], its limits, impairment, ids and deadlines, without sockets or
//! clocks. The caller hands it the datagrams it receives and the time, and
//! sends the [`Datagram`]s [`Core::poll`] hands out; signalling (carrying
//! the SDP strings) is the caller's too. [`crate::endpoint::Endpoint`] is
//! its socket driver; tests drive it on a virtual wire.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Instant;

use rl_netcode::netsim::Profile;
use rl_netcode::{Channel, PeerId, Rng};
use rl_proto::{FINGERPRINT, PROTOCOL_VERSION, Refusal, Signal};

use crate::endpoint::{HOST, MAX_QUEUED, NetEvent, PeerStats, SETUP_TIMEOUT, SILENCE_TIMEOUT, Why};
use crate::impair::Impair;
use crate::limits::{Limits, PeerLimits};
use crate::link::{Ended, Link, LinkError, LinkEvent, Transmit};

/// One connection attempt, from its offer (or answer) until its link opens
/// or fails; never reused.
pub type SetupId = u64;

#[derive(Clone, Debug)]
pub struct CoreConfig {
    /// A host numbers its peers 1, 2, 3, …; a client's only peer is `HOST`.
    pub host: bool,
    /// The host's most peers, open or connecting.
    pub max_peers: usize,
    pub limits: Limits,
    /// Every outgoing datagram passes this profile's network (spec §9.4).
    pub impair: Option<Profile>,
    pub impair_seed: u64,
    /// Tests only (`test-hooks`): the simulation fingerprint to send and
    /// expect instead of `rl_proto::FINGERPRINT`.
    #[cfg(feature = "test-hooks")]
    pub fingerprint_override: Option<u64>,
    /// Tests only (`test-hooks`): a host's first peer id instead of 1 (a
    /// host that has seen many links).
    #[cfg(feature = "test-hooks")]
    pub first_peer_id: Option<PeerId>,
}

impl CoreConfig {
    /// A host with `EndpointConfig`'s defaults: 32 players and 16
    /// spectators (spec §7.2), the default limits, no impairment.
    pub fn host() -> CoreConfig {
        CoreConfig {
            host: true,
            max_peers: 48,
            limits: Limits::default(),
            impair: None,
            impair_seed: 0,
            #[cfg(feature = "test-hooks")]
            fingerprint_override: None,
            #[cfg(feature = "test-hooks")]
            first_peer_id: None,
        }
    }

    /// A client with `EndpointConfig`'s defaults.
    pub fn client() -> CoreConfig {
        CoreConfig { host: false, ..CoreConfig::host() }
    }
}

/// One datagram to send from the core's socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Datagram {
    pub to: SocketAddr,
    pub bytes: Vec<u8>,
    /// The open peer it comes from (`None` while its link was connecting):
    /// a quick way to its stats.
    pub(crate) peer: Option<PeerId>,
    /// The setup it comes from: [`Core::sent`] counts it in the peer's
    /// stats, connecting or open.
    pub(crate) setup: SetupId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreEvent {
    /// What the endpoint reports (`Connected`, `Message`, `Disconnected`).
    Net(NetEvent),
    /// Setup `setup` opened as `peer` (the driver lingers its signalling
    /// connection).
    Opened { setup: SetupId, peer: PeerId },
    /// Setup `setup` did not open (`Timeout` past `SETUP_TIMEOUT`, `Closed`
    /// by the link).
    SetupFailed { setup: SetupId, why: Why },
}

struct Peer {
    setup: SetupId,
    /// Assigned when the link opens.
    id: Option<PeerId>,
    /// The peer's address as the caller names it: the remote end of the
    /// signalling connection.
    addr: SocketAddr,
    /// Our host candidate for this link: where its datagrams arrive.
    local: SocketAddr,
    link: Link,
    /// The link must open by then.
    deadline: Instant,
    heard: Instant,
    /// The peer broke a limit with raw traffic, or a reliable message to it
    /// could not be queued; dropped at its next step.
    broke: Option<Why>,
    limits: PeerLimits,
    stats: PeerStats,
    impair: Option<Impair>,
}

/// One end of online play without sockets: a host and its peers, or a
/// client and its host.
pub struct Core {
    cfg: CoreConfig,
    /// Links not open yet.
    pending: Vec<Peer>,
    open: BTreeMap<PeerId, Peer>,
    /// The host's next peer id.
    next_id: PeerId,
    next_setup: SetupId,
    /// Last datagrams of links the caller cancelled, sent by the next poll.
    outbox: Vec<Datagram>,
    /// Forks each link's impairment.
    rng: Rng,
    /// Datagrams no link accepted.
    stray: u64,
    /// The latest time `poll` was given.
    now: Instant,
}

/// The link's transmits as datagrams of `peer`.
fn datagrams(peer: &Peer, out: &mut Vec<Datagram>, transmits: impl IntoIterator<Item = Transmit>) {
    out.extend(transmits.into_iter().map(|t| Datagram {
        to: t.to,
        bytes: t.bytes,
        peer: peer.id,
        setup: peer.setup,
    }));
}

/// Polls the peer's link and hands out its datagrams (through the
/// impairment).
fn pump(peer: &mut Peer, now: Instant, out: &mut Vec<Datagram>, events: &mut Vec<LinkEvent>) {
    let mut transmits = Vec::new();
    peer.link.poll(now, &mut transmits, events);
    if let Some(impair) = &mut peer.impair {
        for t in transmits.drain(..) {
            impair.push(now, t);
        }
        impair.due(now, &mut transmits);
    }
    datagrams(peer, out, transmits);
}

/// Closes the peer's link and hands out its last datagrams at once.
fn finish(mut peer: Peer, now: Instant, out: &mut Vec<Datagram>) {
    peer.link.close();
    let mut transmits = Vec::new();
    peer.link.poll(now, &mut transmits, &mut Vec::new());
    datagrams(&peer, out, transmits);
}

impl Core {
    pub fn new(cfg: CoreConfig, now: Instant) -> Core {
        #[cfg(feature = "test-hooks")]
        let next_id = cfg.first_peer_id.unwrap_or(1).max(1);
        #[cfg(not(feature = "test-hooks"))]
        let next_id = 1;
        let rng = Rng::new(cfg.impair_seed);
        Core {
            cfg,
            pending: Vec::new(),
            open: BTreeMap::new(),
            next_id,
            next_setup: 0,
            outbox: Vec::new(),
            rng,
            stray: 0,
            now,
        }
    }

    /// The simulation fingerprint offers carry.
    pub(crate) fn fingerprint(&self) -> u64 {
        #[cfg(feature = "test-hooks")]
        if let Some(f) = self.cfg.fingerprint_override {
            return f;
        }
        FINGERPRINT
    }

    /// The version, fingerprint, capacity and id checks of an offer (not
    /// the gate), in that order: `Version`, `Full` (counting connecting
    /// links), `Exhausted`; anything but an offer is `Malformed`.
    pub fn admit(&self, offer: &Signal) -> Result<(), Refusal> {
        let Signal::Offer { protocol, fingerprint, .. } = offer else { return Err(Refusal::Malformed) };
        if *protocol != PROTOCOL_VERSION || *fingerprint != self.fingerprint() {
            return Err(Refusal::Version);
        }
        if self.pending.len() + self.open.len() >= self.cfg.max_peers {
            return Err(Refusal::Full);
        }
        // Ids are never reused: refuse once they run out (4 billion links:
        // unreachable in practice).
        if u64::from(self.next_id) + self.pending.len() as u64 >= u64::from(PeerId::MAX) {
            return Err(Refusal::Exhausted);
        }
        Ok(())
    }

    /// Client: an offerer link from host candidates `local` to the peer at
    /// `addr`; the SDP offer to carry to it.
    pub fn offer(&mut self, local: &[SocketAddr], addr: SocketAddr, now: Instant) -> (SetupId, String) {
        let (link, sdp) = Link::offerer(local, now);
        (self.add(addr, local, link, now), sdp)
    }

    /// Client: the peer's answer to setup `setup`'s offer.
    pub fn accept_answer(&mut self, setup: SetupId, answer: &str) -> Result<(), LinkError> {
        match self.pending.iter_mut().find(|p| p.setup == setup) {
            Some(p) => p.link.accept_answer(answer),
            None => Err(LinkError::Closed),
        }
    }

    /// Host: an answerer link for an admitted offer's SDP from the peer at
    /// `addr`; the SDP answer to carry back (`Malformed` if the offer does
    /// not parse).
    pub fn answer(
        &mut self,
        local: &[SocketAddr],
        offer_sdp: &str,
        addr: SocketAddr,
        now: Instant,
    ) -> Result<(SetupId, String), Refusal> {
        let (link, answer) = Link::answerer(local, offer_sdp, now).map_err(|_| Refusal::Malformed)?;
        Ok((self.add(addr, local, link, now), answer))
    }

    fn add(&mut self, addr: SocketAddr, local: &[SocketAddr], link: Link, now: Instant) -> SetupId {
        let setup = self.next_setup;
        self.next_setup += 1;
        let impair = self.cfg.impair.map(|p| Impair::new(p, self.rng.fork()));
        self.pending.push(Peer {
            setup,
            id: None,
            addr,
            local: local.first().copied().unwrap_or((Ipv4Addr::UNSPECIFIED, 0).into()),
            link,
            deadline: now + SETUP_TIMEOUT,
            heard: now,
            broke: None,
            limits: PeerLimits::new(&self.cfg.limits, now),
            stats: PeerStats::default(),
            impair,
        });
        setup
    }

    /// Has connecting setup `setup` broken a limit (it fails at the next
    /// poll, before its signalling is looked at)?
    pub(crate) fn broke(&self, setup: SetupId) -> bool {
        self.pending.iter().any(|p| p.setup == setup && p.broke.is_some())
    }

    /// The signalling of `setup` failed: drop it (a client reports
    /// `Disconnected { HOST, why }`). Its last datagrams leave with the
    /// next poll.
    pub fn cancel(&mut self, setup: SetupId, why: Why, events: &mut Vec<CoreEvent>) {
        if let Some(i) = self.pending.iter().position(|p| p.setup == setup) {
            let peer = self.pending.remove(i);
            if !self.cfg.host {
                events.push(CoreEvent::Net(NetEvent::Disconnected { peer: HOST, why }));
            }
            let mut out = std::mem::take(&mut self.outbox);
            finish(peer, self.now, &mut out);
            self.outbox = out;
        }
    }

    /// Hands a received datagram to the link that accepts it (others are
    /// counted, [`Core::stray_datagrams`]).
    pub fn receive(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) {
        let peer = self
            .pending
            .iter_mut()
            .chain(self.open.values_mut())
            .find(|p| p.link.accepts(from, p.local, bytes));
        match peer {
            Some(p) if p.broke.is_none() => {
                p.stats.datagrams_received += 1;
                p.stats.bytes_received += bytes.len() as u64;
                if let Err(why) = p.limits.on_datagram(now, bytes.len()) {
                    p.broke = Some(why);
                    return;
                }
                p.heard = now;
                p.link.receive(from, p.local, bytes, now);
            }
            Some(_) => {}
            None => self.stray += 1,
        }
    }

    /// Steps every link at `now`: appends the datagrams to send to `out`
    /// and what happened to `events`.
    pub fn poll(&mut self, now: Instant, out: &mut Vec<Datagram>, events: &mut Vec<CoreEvent>) {
        self.now = self.now.max(now);
        let now = self.now;
        out.append(&mut self.outbox);
        let pending = std::mem::take(&mut self.pending);
        let open = std::mem::take(&mut self.open);
        for peer in pending.into_iter().chain(open.into_values()) {
            if let Some(peer) = self.step(peer, now, out, events) {
                match peer.id {
                    Some(id) => {
                        self.open.insert(id, peer);
                    }
                    None => self.pending.push(peer),
                }
            }
        }
    }

    /// Counts a datagram of [`Core::poll`] in its peer's stats: sent, or
    /// refused by the socket.
    pub(crate) fn sent(&mut self, datagram: &Datagram, ok: bool) {
        let setup = datagram.setup;
        let peer = match datagram.peer.and_then(|id| self.open.get_mut(&id)) {
            Some(p) => Some(p),
            // Connecting when it was sent (it may have opened since).
            None => self.pending.iter_mut().chain(self.open.values_mut()).find(|p| p.setup == setup),
        };
        let Some(p) = peer.filter(|p| p.setup == setup) else { return };
        if ok {
            p.stats.datagrams_sent += 1;
            p.stats.bytes_sent += datagram.bytes.len() as u64;
        } else {
            p.stats.send_failures += 1;
        }
    }

    /// Drives one peer: its link, its limits and timeouts. `None` when it
    /// is gone.
    fn step(
        &mut self,
        mut peer: Peer,
        now: Instant,
        out: &mut Vec<Datagram>,
        events: &mut Vec<CoreEvent>,
    ) -> Option<Peer> {
        if let Some(why) = peer.broke {
            match peer.id {
                Some(id) => {
                    events.push(CoreEvent::Net(NetEvent::Disconnected { peer: id, why }));
                    finish(peer, now, out);
                }
                None => self.fail_setup(peer, why, now, out, events),
            }
            return None;
        }
        let mut link_events = Vec::new();
        pump(&mut peer, now, out, &mut link_events);
        for event in link_events {
            match event {
                LinkEvent::Open => {
                    let id = if self.cfg.host {
                        self.next_id += 1;
                        self.next_id - 1
                    } else {
                        HOST
                    };
                    peer.id = Some(id);
                    events.push(CoreEvent::Net(NetEvent::Connected { peer: id, addr: peer.addr }));
                    events.push(CoreEvent::Opened { setup: peer.setup, peer: id });
                }
                LinkEvent::Message { channel, bytes } => {
                    let Some(id) = peer.id else { continue };
                    match peer.limits.on_message(now, channel == Channel::Reliable, bytes.len()) {
                        Ok(()) => events.push(CoreEvent::Net(NetEvent::Message { peer: id, channel, bytes })),
                        Err(why) => {
                            events.push(CoreEvent::Net(NetEvent::Disconnected { peer: id, why }));
                            finish(peer, now, out);
                            return None;
                        }
                    }
                }
                LinkEvent::Closed { why } => {
                    let why = match why {
                        Ended::Lost => Why::Timeout,
                        Ended::Peer | Ended::Failed(_) => Why::Closed,
                    };
                    match peer.id {
                        Some(id) => events.push(CoreEvent::Net(NetEvent::Disconnected { peer: id, why })),
                        None => {
                            if !self.cfg.host {
                                events.push(CoreEvent::Net(NetEvent::Disconnected { peer: HOST, why }));
                            }
                            events.push(CoreEvent::SetupFailed { setup: peer.setup, why });
                        }
                    }
                    return None;
                }
            }
        }
        match peer.id {
            Some(id) if now.saturating_duration_since(peer.heard) >= SILENCE_TIMEOUT => {
                events.push(CoreEvent::Net(NetEvent::Disconnected { peer: id, why: Why::Timeout }));
                finish(peer, now, out);
                None
            }
            None if now >= peer.deadline => {
                self.fail_setup(peer, Why::Timeout, now, out, events);
                None
            }
            _ => Some(peer),
        }
    }

    /// A connection that did not come up: the client hears why, the host
    /// drops it quietly.
    fn fail_setup(
        &mut self,
        peer: Peer,
        why: Why,
        now: Instant,
        out: &mut Vec<Datagram>,
        events: &mut Vec<CoreEvent>,
    ) {
        if !self.cfg.host {
            events.push(CoreEvent::Net(NetEvent::Disconnected { peer: HOST, why }));
        }
        events.push(CoreEvent::SetupFailed { setup: peer.setup, why });
        finish(peer, now, out);
    }

    /// Queues `bytes` as one message to `peer`; it leaves with the next
    /// `poll`. A message to an unknown peer is ignored; one beyond
    /// [`MAX_QUEUED`] or refused by the link (empty, too large) is dropped
    /// and counted. On the reliable channel a dropped message would break
    /// the ordered stream silently, so it also ends the link: the next
    /// `poll` reports `Disconnected { why: Why::Unsent }`.
    pub fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        let Some(p) = self.open.get_mut(&peer) else { return };
        if p.broke.is_some()
            || p.link.buffered(channel) + bytes.len() > MAX_QUEUED
            || p.link.send(channel, bytes).is_err()
        {
            p.stats.sends_dropped += 1;
            if channel == Channel::Reliable {
                p.broke.get_or_insert(Why::Unsent);
            }
        }
    }

    /// The bytes waiting to leave for `peer` on `channel` (sent and not yet
    /// acknowledged, on the reliable channel).
    pub fn buffered(&self, peer: PeerId, channel: Channel) -> usize {
        self.open.get(&peer).map_or(0, |p| p.link.buffered(channel))
    }

    /// Lets the next `bytes` of reliable messages from `peer` through the
    /// byte limit, and lifts the raw limits by twice the transfer.
    pub fn allow_burst(&mut self, peer: PeerId, bytes: usize) {
        if let Some(p) = self.open.get_mut(&peer) {
            p.limits.allow_burst(bytes);
        }
    }

    /// Closes the link to `peer` and hands out its last datagrams; the peer
    /// is told, no event follows.
    pub fn close(&mut self, peer: PeerId, out: &mut Vec<Datagram>) {
        if let Some(p) = self.open.remove(&peer) {
            finish(p, self.now, out);
        }
    }

    /// Closes every link, connecting or open, and hands out their last
    /// datagrams (the driver's drop).
    pub(crate) fn close_all(&mut self, out: &mut Vec<Datagram>) {
        let now = self.now;
        out.append(&mut self.outbox);
        for peer in
            std::mem::take(&mut self.pending).into_iter().chain(std::mem::take(&mut self.open).into_values())
        {
            finish(peer, now, out);
        }
    }

    /// The open peers, by id, with their addresses.
    pub fn peers(&self) -> impl Iterator<Item = (PeerId, SocketAddr)> + '_ {
        self.open.iter().map(|(&id, p)| (id, p.addr))
    }

    /// The DTLS fingerprints of an open peer's link, offer side's then
    /// answer side's (`Link::dtls_fingerprints`): the same at both ends.
    pub fn dtls_fingerprints(&self, peer: PeerId) -> Option<([u8; 32], [u8; 32])> {
        self.open.get(&peer).and_then(|p| p.link.dtls_fingerprints())
    }

    /// What went through the socket for an open peer.
    pub fn stats(&self, peer: PeerId) -> Option<PeerStats> {
        self.open.get(&peer).map(|p| p.stats)
    }

    /// Datagrams no link accepted (dropped).
    pub fn stray_datagrams(&self) -> u64 {
        self.stray
    }

    /// The links not open yet.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}
