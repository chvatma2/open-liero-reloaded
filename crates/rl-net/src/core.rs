//! The sans-IO endpoint (networking spec §5.1, §5.4, §9.4): every peer's
//! [`Link`], its limits, impairment, ids and deadlines, without sockets or
//! clocks. The caller hands it the datagrams it receives and the time, and
//! sends the [`Datagram`]s [`Core::poll`] hands out; signalling (carrying
//! the SDP strings) is the caller's too. [`crate::endpoint::Endpoint`] is
//! its socket driver; tests drive it on a virtual wire and behind virtual
//! NATs (`natsim`).
//!
//! Two kinds of setup share the socket: TCP-signalled ones (`offer`,
//! `answer`: the host candidate is the signalling connection's local IP,
//! decision N16) and signalled ones (`offer_ice`, `answer_ice`: every
//! local candidate of [`Core::local`], decisions N48-N50), whose
//! datagrams are all taken as arriving at the base. STUN rounds on the
//! same socket learn the server-reflexive candidate ([`Core::stun`]).

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use rl_netcode::netsim::Profile;
use rl_netcode::{Channel, PeerId, Rng};
use rl_proto::stun::binding_request_transaction;
use rl_proto::{FINGERPRINT, PROTOCOL_VERSION, Path, Refusal, Signal};

use crate::candidates::{LocalCandidates, RemoteCandidates, classify, remote_candidates, sanitize};
use crate::impair::Impair;
use crate::limits::{Limits, PeerLimits};
use crate::link::{Ended, Link, LinkCandidates, LinkError, LinkEvent, Transmit};
use crate::probe::{NatBehaviour, StunProbe};

/// A client's only peer: the host.
pub const HOST: PeerId = 0;
/// At most this many bytes wait to be sent to one peer on one channel;
/// [`Core::send`] drops a message beyond it (`PeerStats::sends_dropped`),
/// and a reliable one ends the link (`Why::Unsent`).
pub const MAX_QUEUED: usize = 4 * 1024 * 1024;
/// A connection not set up within this time is dropped: the signalling, and
/// the link until it opens.
pub const SETUP_TIMEOUT: Duration = Duration::from_secs(10);
/// An open link that hears nothing for this long is dropped (`Why::Timeout`);
/// ICE keeps a live link's datagrams coming every few seconds.
pub const SILENCE_TIMEOUT: Duration = Duration::from_secs(10);
/// A signalled setup must open within this (TCP-signalled ones keep SETUP_TIMEOUT, 10 s).
pub const SIGNALLED_SETUP_TIMEOUT: Duration = Duration::from_secs(15);
/// At most this many signalled setups connect at once; when full, PF12b-2 evicts a silent one.
pub const MAX_SIGNALLED_PENDING: usize = 8;
/// A signalled link that opened before str0m's stats named its selected
/// pair waits at most this long for them before `Connected` (stats come
/// every second).
const CONNECTED_WAIT: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetEvent {
    /// The link to `peer` is open. `addr` is the remote end of its
    /// signalling connection (a signalled link: the remote address of its
    /// selected candidate pair, else the caller's hint). `path` is how it
    /// goes (N49): the selected pair's class when the link opened, or at
    /// str0m's first stats at most 1.5 s later (`HolePunched` if none
    /// came); always `Direct` for a TCP-signalled link.
    Connected {
        peer: PeerId,
        addr: SocketAddr,
        path: Path,
    },
    Message {
        peer: PeerId,
        channel: Channel,
        bytes: Vec<u8>,
    },
    /// The peer is gone. On a client, `HOST` disconnected without a
    /// `Connected` first means the connection never came up.
    Disconnected {
        peer: PeerId,
        why: Why,
    },
    /// Client side: the host refused us.
    Refused {
        why: Refusal,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The peer closed the connection, or it could not be made.
    Closed,
    /// Nothing heard for [`SILENCE_TIMEOUT`], ICE lost the connection, or
    /// the setup took longer than [`SETUP_TIMEOUT`].
    Timeout,
    /// The peer sent faster than its [`Limits`].
    Limits,
    /// The peer sent a message too large for its channel, or bad signalling.
    Malformed,
    /// Not reported by the endpoint: the session layers' refusal (a kick or
    /// a ban).
    Refused,
    /// A reliable message to the peer could not be queued (beyond
    /// [`MAX_QUEUED`], or refused by the link): our own failure, the
    /// ordered stream cannot go on.
    Unsent,
}

/// What went through the socket for one peer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeerStats {
    pub datagrams_sent: u64,
    pub bytes_sent: u64,
    pub datagrams_received: u64,
    pub bytes_received: u64,
    /// Messages [`Core::send`] (`Endpoint::send`) dropped: over [`MAX_QUEUED`], or refused
    /// by the link (empty, too large). A reliable one ends the link.
    pub sends_dropped: u64,
    /// Datagrams the local socket did not take (a full send buffer).
    pub send_failures: u64,
}

/// One connection attempt, from its offer (or answer) until its link opens
/// or fails; never reused.
pub type SetupId = u64;

/// What a [`Core`] is: a host or a client, its capacity, the limits it
/// holds its peers to and the impairment of its sending.
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
    /// Seeds the STUN transaction ids (the endpoint takes it from the
    /// system's randomness, so an off-path party cannot guess them).
    pub stun_seed: u64,
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
    /// A host with the defaults (`EndpointConfig::default` takes them
    /// from here): 32 players and 16 spectators (spec §7.2), the default
    /// limits, no impairment.
    pub fn host() -> CoreConfig {
        CoreConfig {
            host: true,
            max_peers: 48,
            limits: Limits::default(),
            impair: None,
            impair_seed: 0,
            stun_seed: 0,
            #[cfg(feature = "test-hooks")]
            fingerprint_override: None,
            #[cfg(feature = "test-hooks")]
            first_peer_id: None,
        }
    }

    /// A client with the defaults.
    pub fn client() -> CoreConfig {
        CoreConfig { host: false, ..CoreConfig::host() }
    }
}

/// One datagram to send from the core's socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Datagram {
    pub to: SocketAddr,
    pub bytes: Vec<u8>,
    /// The setup it comes from: [`Core::sent`] counts it in that peer's
    /// stats, whether its link was connecting or open (a link opening
    /// hands out datagrams before it has a peer id).
    pub(crate) setup: SetupId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreEvent {
    /// What the endpoint reports (`Connected`, `Message`, `Disconnected`).
    Net(NetEvent),
    /// Setup `setup` opened as `peer` (the driver lingers its signalling
    /// connection).
    Opened { setup: SetupId, peer: PeerId },
    /// Setup `setup` did not open: `Timeout` past `SETUP_TIMEOUT` (or ICE
    /// lost it, or a full pending table evicted it, PF12b-2), `Closed` by
    /// the link, or the limit a connecting peer broke (`Limits`). Not
    /// reported for the caller's own [`Core::cancel`].
    SetupFailed { setup: SetupId, why: Why },
}

struct Peer {
    setup: SetupId,
    /// Assigned when the link opens.
    id: Option<PeerId>,
    /// The peer's address as the caller names it: the remote end of the
    /// signalling connection (signalled: the hint, then the selected
    /// pair's remote address once `Connected`).
    addr: SocketAddr,
    /// Our host candidate for this link: where its datagrams arrive (a
    /// signalled link's arrive at the base).
    local: SocketAddr,
    /// Set up by `offer_ice` / `answer_ice` (N48), not over TCP.
    signalled: bool,
    /// Its peer sent an ICE connectivity check while connecting (PF12b-2).
    checked: bool,
    /// The peer's candidates as its SDP names them, for `classify`.
    theirs: RemoteCandidates,
    /// When the link opened.
    opened: Option<Instant>,
    /// `Connected` was reported; until then its events wait in `held`.
    reported: bool,
    held: Vec<NetEvent>,
    /// The selected pair last classified, and its path.
    pair: Option<(SocketAddr, SocketAddr)>,
    path: Option<Path>,
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
    /// Events of calls that take no event list (`answer_ice`'s eviction),
    /// reported by the next poll.
    events: Vec<CoreEvent>,
    /// Where every signalled link's datagrams are taken as arriving.
    base: Option<SocketAddr>,
    /// The host candidates signalled links offer, `base` first.
    hosts: Vec<SocketAddr>,
    /// The router mapping's external address.
    mapped: Option<SocketAddr>,
    probe: StunProbe,
    /// Forks each link's impairment.
    rng: Rng,
    /// Datagrams no link accepted.
    stray: u64,
    /// The latest time `poll` was given.
    now: Instant,
}

/// The first of a TCP-signalled link's host candidates: where its
/// datagrams are taken as arriving.
fn first(local: &[SocketAddr]) -> SocketAddr {
    local.first().copied().unwrap_or((Ipv4Addr::UNSPECIFIED, 0).into())
}

/// Hands out `event` about an open peer, or keeps it until `Connected`.
fn emit(peer: &mut Peer, events: &mut Vec<CoreEvent>, event: NetEvent) {
    if peer.reported {
        events.push(CoreEvent::Net(event));
    } else {
        peer.held.push(event);
    }
}

/// The link's transmits as datagrams of `peer`.
fn datagrams(peer: &Peer, out: &mut Vec<Datagram>, transmits: impl IntoIterator<Item = Transmit>) {
    out.extend(transmits.into_iter().map(|t| Datagram { to: t.to, bytes: t.bytes, setup: peer.setup }));
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
        let probe = StunProbe::new(cfg.stun_seed);
        Core {
            cfg,
            pending: Vec::new(),
            open: BTreeMap::new(),
            next_id,
            next_setup: 0,
            outbox: Vec::new(),
            events: Vec::new(),
            base: None,
            hosts: Vec::new(),
            mapped: None,
            probe,
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
        (self.add(addr, first(local), link, now, None), sdp)
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
        Ok((self.add(addr, first(local), link, now, None), answer))
    }

    /// The socket's candidates: `base` is where every datagram is taken as arriving; `hosts` (base first) are offered.
    pub fn set_base(&mut self, base: SocketAddr, hosts: Vec<SocketAddr>) {
        self.base = Some(base);
        self.hosts = vec![base];
        for h in hosts {
            if !self.hosts.contains(&h) {
                self.hosts.push(h);
            }
        }
    }

    /// STUN servers asked from this socket now and every STUN_EVERY (two answers give NatBehaviour).
    pub fn stun(&mut self, servers: Vec<SocketAddr>, now: Instant) {
        self.probe.start(servers, now);
    }

    /// The router mapping's external address (a server-reflexive candidate
    /// at it), or `None`.
    pub fn set_mapped(&mut self, mapped: Option<SocketAddr>) {
        self.mapped = mapped;
    }

    /// Our candidates now; `srflx` is the latest STUN answer (a changed
    /// router address is offered from the next round on).
    pub fn local(&self) -> LocalCandidates {
        LocalCandidates {
            base: self.base,
            host: self.hosts.clone(),
            srflx: self.probe.srflx(),
            mapped: self.mapped,
            relayed: Vec::new(),
        }
    }

    pub fn nat(&self) -> NatBehaviour {
        self.probe.nat(self.base)
    }

    /// STUN answered (or STUN_WAIT passed) — and, from Task 3, TURN too.
    pub fn gathered(&self, now: Instant) -> bool {
        self.probe.gathered(now)
    }

    /// What a signalled link offers: the host candidates, and the
    /// server-reflexive and mapped addresses on the base (unless equal to
    /// it).
    fn link_candidates(&self) -> LinkCandidates {
        let mut c = LinkCandidates { host: self.hosts.clone(), ..LinkCandidates::default() };
        if let Some(base) = self.base {
            for addr in [self.probe.srflx(), self.mapped].into_iter().flatten() {
                if addr != base && addr.is_ipv4() && !c.srflx.iter().any(|&(a, _)| a == addr) {
                    c.srflx.push((addr, base));
                }
            }
        }
        c
    }

    /// Client, signalled: an offerer link with every local candidate.
    pub fn offer_ice(&mut self, peer_hint: SocketAddr, now: Instant) -> (SetupId, String) {
        let (link, sdp) = Link::offerer_with(&self.link_candidates(), now);
        let base = self.base.unwrap_or((Ipv4Addr::UNSPECIFIED, 0).into());
        (self.add(peer_hint, base, link, now, Some(RemoteCandidates::default())), sdp)
    }

    /// Client: the host's answer and its mapped hint (no F-G rewrite).
    /// The answer is sanitized as an offer is (N50).
    pub fn accept_answer_ice(
        &mut self,
        setup: SetupId,
        answer: &str,
        mapped_hint: Option<SocketAddr>,
    ) -> Result<(), LinkError> {
        let answer = sanitize(answer);
        let Some(p) = self.pending.iter_mut().find(|p| p.setup == setup) else {
            return Err(LinkError::Closed);
        };
        p.link.accept_answer(&answer)?;
        p.theirs = remote_candidates(&answer, mapped_hint);
        Ok(())
    }

    /// Host, signalled: admit, sanitize, an answerer link with every local candidate; Answer or Refused.
    /// With MAX_SIGNALLED_PENDING setups pending, the oldest one that has received no ICE connectivity
    /// check (STUN binding request) from its peer is closed to make room (PF12b-2); if every one has, Refused { Busy }.
    /// The evicted setup's `SetupFailed { why: Timeout }` comes with the next poll.
    pub fn answer_ice(&mut self, offer: &Signal, mapped_hint: Option<SocketAddr>, now: Instant) -> Signal {
        let refused = |reason| Signal::Refused { reason };
        if let Err(why) = self.admit(offer) {
            return refused(why);
        }
        let Signal::Offer { sdp, .. } = offer else { return refused(Refusal::Malformed) };
        let signalled = self.pending.iter().filter(|p| p.signalled).count();
        let victim = if signalled >= MAX_SIGNALLED_PENDING {
            // Setup ids grow: the smallest is the oldest.
            let silent = self.pending.iter().filter(|p| p.signalled && !p.checked).min_by_key(|p| p.setup);
            match silent {
                Some(p) => Some(p.setup),
                None => return refused(Refusal::Busy),
            }
        } else {
            None
        };
        let sdp = sanitize(sdp);
        let Ok((link, answer)) = Link::answerer_with(&self.link_candidates(), &sdp, now) else {
            return refused(Refusal::Malformed);
        };
        if let Some(i) = victim.and_then(|v| self.pending.iter().position(|p| p.setup == v)) {
            let peer = self.pending.remove(i);
            let (mut out, mut events) = (std::mem::take(&mut self.outbox), std::mem::take(&mut self.events));
            self.fail_setup(peer, Why::Timeout, now, &mut out, &mut events);
            (self.outbox, self.events) = (out, events);
        }
        let base = self.base.unwrap_or((Ipv4Addr::UNSPECIFIED, 0).into());
        let addr = (Ipv4Addr::UNSPECIFIED, 0).into();
        self.add(addr, base, link, now, Some(remote_candidates(&sdp, mapped_hint)));
        Signal::Answer { sdp: answer }
    }

    /// The path of an open (reported) peer's link: its selected pair's
    /// class, re-classified as the pair changes (`Direct` for a
    /// TCP-signalled link).
    pub fn path(&self, peer: PeerId) -> Option<Path> {
        self.open.get(&peer).filter(|p| p.reported).and_then(|p| p.path)
    }

    /// A connecting link; `theirs` is `Some` for a signalled one.
    fn add(
        &mut self,
        addr: SocketAddr,
        local: SocketAddr,
        link: Link,
        now: Instant,
        theirs: Option<RemoteCandidates>,
    ) -> SetupId {
        let setup = self.next_setup;
        self.next_setup += 1;
        let impair = self.cfg.impair.map(|p| Impair::new(p, self.rng.fork()));
        let signalled = theirs.is_some();
        self.pending.push(Peer {
            setup,
            id: None,
            addr,
            local,
            signalled,
            checked: false,
            theirs: theirs.unwrap_or_default(),
            opened: None,
            reported: false,
            held: Vec::new(),
            pair: None,
            path: None,
            link,
            deadline: now + if signalled { SIGNALLED_SETUP_TIMEOUT } else { SETUP_TIMEOUT },
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

    /// Hands a received datagram to the STUN probe when it answers one of
    /// its requests, else to the link that accepts it, as arriving at the
    /// base (a TCP-signalled link: at its own host candidate, N16). Others
    /// are counted, [`Core::stray_datagrams`].
    pub fn receive(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) {
        if self.probe.receive(from, bytes, now) {
            return;
        }
        let base = self.base;
        let to = |p: &Peer| if p.signalled { base.unwrap_or(p.local) } else { p.local };
        let peer = self
            .pending
            .iter_mut()
            .chain(self.open.values_mut())
            .find(|p| p.link.accepts(from, to(p), bytes));
        match peer {
            Some(p) if p.broke.is_none() => {
                p.stats.datagrams_received += 1;
                p.stats.bytes_received += bytes.len() as u64;
                if let Err(why) = p.limits.on_datagram(now, bytes.len()) {
                    p.broke = Some(why);
                    return;
                }
                p.heard = now;
                if p.id.is_none() && binding_request_transaction(bytes).is_some() {
                    p.checked = true;
                }
                let to = to(p);
                p.link.receive(from, to, bytes, now);
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
        events.append(&mut self.events);
        self.probe.poll(now, out);
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

    /// The caller reports each datagram of [`Core::poll`] and
    /// [`Core::close`]: `ok` if its socket took it, else refused (a full
    /// send buffer). The peer's [`PeerStats`] count it (`datagrams_sent`
    /// and `bytes_sent`, or `send_failures`); a datagram of a link gone
    /// since is ignored. Without these reports the stats count no sends.
    pub fn sent(&mut self, datagram: &Datagram, ok: bool) {
        let setup = datagram.setup;
        // At most `max_peers` links: a scan is cheap next to a `send_to`.
        let peer = self.pending.iter_mut().chain(self.open.values_mut()).find(|p| p.setup == setup);
        let Some(p) = peer else { return };
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
                    self.report(&mut peer, events);
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
                    peer.opened = Some(now);
                    // The driver lingers the signalling at once; a
                    // signalled link's `Connected` waits for its path.
                    events.push(CoreEvent::Opened { setup: peer.setup, peer: id });
                    if !peer.signalled {
                        self.report(&mut peer, events);
                    }
                }
                LinkEvent::Message { channel, bytes } => {
                    let Some(id) = peer.id else { continue };
                    match peer.limits.on_message(now, channel == Channel::Reliable, bytes.len()) {
                        Ok(()) => emit(&mut peer, events, NetEvent::Message { peer: id, channel, bytes }),
                        Err(why) => {
                            self.report(&mut peer, events);
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
                        Some(id) => {
                            self.report(&mut peer, events);
                            events.push(CoreEvent::Net(NetEvent::Disconnected { peer: id, why }));
                        }
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
        if let Some(opened) = peer.opened {
            let pair = peer.link.selected_pair();
            if !peer.reported && (pair.is_some() || now >= opened + CONNECTED_WAIT) {
                self.report(&mut peer, events);
            } else if peer.reported && peer.signalled && pair.is_some() && pair != peer.pair {
                // ICE moved to another pair: the path follows.
                peer.pair = pair;
                peer.path = Some(self.classify(&peer));
            }
        }
        match peer.id {
            Some(id) if now.saturating_duration_since(peer.heard) >= SILENCE_TIMEOUT => {
                self.report(&mut peer, events);
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

    /// The path of a peer's link: its selected pair's class (N49), or
    /// `HolePunched` without one; `Direct` for a TCP-signalled link.
    fn classify(&self, peer: &Peer) -> Path {
        match (peer.signalled, peer.pair) {
            (false, _) => Path::Direct,
            (true, Some((local, remote))) => classify(local, remote, &self.local(), &peer.theirs),
            (true, None) => Path::HolePunched,
        }
    }

    /// Reports an opened peer `Connected` (once) with its path, then the
    /// events that waited for it.
    fn report(&self, peer: &mut Peer, events: &mut Vec<CoreEvent>) {
        let Some(id) = peer.id else { return };
        if peer.reported {
            return;
        }
        peer.reported = true;
        peer.pair = peer.link.selected_pair();
        let path = self.classify(peer);
        peer.path = Some(path);
        if let (true, Some((_, remote))) = (peer.signalled, peer.pair) {
            peer.addr = remote;
        }
        events.push(CoreEvent::Net(NetEvent::Connected { peer: id, addr: peer.addr, path }));
        events.extend(peer.held.drain(..).map(CoreEvent::Net));
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

    /// The open peers reported `Connected`, by id, with their addresses.
    pub fn peers(&self) -> impl Iterator<Item = (PeerId, SocketAddr)> + '_ {
        self.open.iter().filter(|(_, p)| p.reported).map(|(&id, p)| (id, p.addr))
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
