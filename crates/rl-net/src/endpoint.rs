//! The socket driver (networking spec §5.1, §5.4, §9.4): one UDP socket
//! shared by every peer's [`Link`], WebSocket signalling over TCP, per-peer
//! limits and an optional impairment stage.
//!
//! A host listens on TCP and UDP on the same port; a client connects to the
//! host's TCP address, sends its `Signal::Offer`, takes the answer, and both
//! links connect over UDP. Each side's only host candidate is the IP its
//! signalling connection uses with its UDP port (decision N16).
//! [`Endpoint::poll`] never blocks: it does what the sockets allow now.

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use rl_netcode::netsim::Profile;
use rl_netcode::{Channel, PeerId, Rng};
use rl_proto::{FINGERPRINT, PROTOCOL_VERSION, Refusal, Signal, decode_signal, encode_signal};

use crate::impair::Impair;
pub use crate::limits::Limits;
use crate::limits::PeerLimits;
use crate::link::{Ended, Link, LinkEvent, Transmit};
use crate::signal::SignalConn;

/// A client's only peer: the host.
pub const HOST: PeerId = 0;
/// The host's default TCP and UDP port (spec §5.1).
pub const DEFAULT_PORT: u16 = 23400;
/// At most this many bytes wait to be sent to one peer on one channel;
/// [`Endpoint::send`] drops a message beyond it (`PeerStats::sends_dropped`),
/// and a reliable one ends the link (`Why::Limits`).
pub const MAX_QUEUED: usize = 4 * 1024 * 1024;
/// A connection not set up within this time is dropped: the signalling, and
/// the link until it opens.
pub const SETUP_TIMEOUT: Duration = Duration::from_secs(10);
/// An open link that hears nothing for this long is dropped (`Why::Timeout`);
/// ICE keeps a live link's datagrams coming every few seconds.
pub const SILENCE_TIMEOUT: Duration = Duration::from_secs(10);
/// The client's TCP connect gives up after this long.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A host's signalling connection must deliver its offer within this time.
pub const OFFER_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a finished or refused signalling connection may take to close.
const LINGER: Duration = Duration::from_secs(1);
/// At most this many signalling connections wait for their offer; more are
/// closed as they come.
const MAX_INCOMING: usize = 64;
/// At most this many connections not open yet (waiting for their offer, or
/// a link connecting) per source IP; more are closed as they come.
pub const MAX_PER_IP: usize = 8;
/// At most this many TCP connections are taken in one poll.
const MAX_ACCEPTS_PER_POLL: usize = 64;
/// At most this many datagrams are taken in one poll.
const MAX_DATAGRAMS_PER_POLL: usize = 1024;

/// The host's admission check for a valid offer from the signalling
/// connection's remote address (bans by IP, Task 6/13). It runs after the
/// version and capacity checks.
pub type Gate = Box<dyn FnMut(&SocketAddr, &Signal) -> Result<(), Refusal> + Send>;

#[derive(Clone, Debug)]
pub struct EndpointConfig {
    /// The UDP port, and the host's TCP port too; 0 picks a free one
    /// ([`Endpoint::local_port`]).
    pub port: u16,
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
    /// Tests only (`test-hooks`): a client's offer names this IP in its
    /// candidates instead of its own (a client behind NAT, as the host sees
    /// it).
    #[cfg(feature = "test-hooks")]
    pub offer_ip_override: Option<IpAddr>,
    /// Tests only (`test-hooks`): a host's first peer id instead of 1 (a
    /// host that has seen many links).
    #[cfg(feature = "test-hooks")]
    pub first_peer_id: Option<PeerId>,
    /// Tests only (`test-hooks`): a host's answer names this address in
    /// its candidates instead of its own (a host pointing its joiners
    /// elsewhere).
    #[cfg(feature = "test-hooks")]
    pub answer_addr_override: Option<SocketAddr>,
}

impl Default for EndpointConfig {
    /// The default port, 32 players and 16 spectators (spec §7.2), the
    /// default limits, no impairment.
    fn default() -> Self {
        EndpointConfig {
            port: DEFAULT_PORT,
            max_peers: 48,
            limits: Limits::default(),
            impair: None,
            impair_seed: 0,
            #[cfg(feature = "test-hooks")]
            fingerprint_override: None,
            #[cfg(feature = "test-hooks")]
            offer_ip_override: None,
            #[cfg(feature = "test-hooks")]
            first_peer_id: None,
            #[cfg(feature = "test-hooks")]
            answer_addr_override: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetEvent {
    /// The link to `peer` is open; `addr` is the remote end of its
    /// signalling connection.
    Connected {
        peer: PeerId,
        addr: SocketAddr,
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
    /// Messages [`Endpoint::send`] dropped: over [`MAX_QUEUED`], or refused
    /// by the link (empty, too large). A reliable one ends the link.
    pub sends_dropped: u64,
    /// Datagrams the local socket did not take (a full send buffer).
    pub send_failures: u64,
}

struct Peer {
    /// Assigned when the link opens.
    id: Option<PeerId>,
    /// The remote end of the signalling connection.
    addr: SocketAddr,
    /// Our host candidate for this link: where its datagrams arrive.
    local: SocketAddr,
    link: Link,
    /// The signalling connection, until the link opens.
    ws: Option<SignalConn>,
    /// Client: the host's answer is taken. Host: always.
    answered: bool,
    /// The link must open by then.
    deadline: Instant,
    heard: Instant,
    /// The peer broke a limit with raw traffic; dropped at its next step.
    broke: Option<Why>,
    limits: PeerLimits,
    stats: PeerStats,
    impair: Option<Impair>,
}

/// A signalling connection waiting for its offer.
struct Incoming {
    conn: SignalConn,
    addr: SocketAddr,
    local: IpAddr,
    deadline: Instant,
}

enum Role {
    Host {
        listener: TcpListener,
        gate: Gate,
        next_id: PeerId,
        incoming: Vec<Incoming>,
    },
    Client {
        host: SocketAddr,
        /// The TCP connect running on its own thread, and when we started waiting.
        connecting: Option<(Receiver<io::Result<TcpStream>>, Option<Instant>)>,
    },
}

/// One end of online play: a host and its peers, or a client and its host.
pub struct Endpoint {
    cfg: EndpointConfig,
    udp: UdpSocket,
    port: u16,
    role: Role,
    /// Links not open yet.
    pending: Vec<Peer>,
    open: BTreeMap<PeerId, Peer>,
    /// Signalling connections closing.
    lingering: Vec<(SignalConn, Instant)>,
    /// Forks each link's impairment.
    rng: Rng,
    /// Datagrams no link accepted.
    stray: u64,
    /// The latest time `poll` was given.
    now: Instant,
}

fn bind_udp(port: u16) -> io::Result<UdpSocket> {
    let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?;
    udp.set_nonblocking(true)?;
    Ok(udp)
}

/// TCP and UDP on the same port; with port 0, a free one for both.
fn bind_both(port: u16) -> io::Result<(TcpListener, UdpSocket, u16)> {
    let mut tries = 0;
    loop {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))?;
        let port = listener.local_addr()?.port();
        match bind_udp(port) {
            Ok(udp) => {
                listener.set_nonblocking(true)?;
                return Ok((listener, udp, port));
            }
            Err(e) if tries < 32 && e.kind() == io::ErrorKind::AddrInUse => tries += 1,
            Err(e) => return Err(e),
        }
    }
}

fn send_datagram(udp: &UdpSocket, stats: &mut PeerStats, t: &Transmit) {
    // A full socket buffer loses the datagram, as a network would.
    if udp.send_to(&t.bytes, t.to).is_ok() {
        stats.datagrams_sent += 1;
        stats.bytes_sent += t.bytes.len() as u64;
    } else {
        stats.send_failures += 1;
    }
}

/// The IP address of an `a=candidate` line (`None` for any other line, or
/// an address that does not parse, e.g. an mDNS name).
fn candidate_ip(line: &str) -> Option<IpAddr> {
    let candidate = line.trim_end().strip_prefix("a=candidate:")?;
    candidate.split_ascii_whitespace().nth(4)?.parse().ok()
}

/// `sdp` without the `a=candidate` lines whose address is not `ip` (ruling
/// T4-A): the host never sends connectivity checks to an address the joiner
/// may not hold (reflection), and a joiner behind NAT still connects through
/// the peer-reflexive candidate its checks reveal.
fn keep_candidates_of(sdp: &str, ip: IpAddr) -> String {
    sdp.split_inclusive('\n')
        .filter(|line| !line.starts_with("a=candidate:") || candidate_ip(line) == Some(ip))
        .collect()
}

/// `sdp` with the address of every `a=candidate` line replaced by `ip`
/// (and the port by `port`): a client's view of the host's answer (ruling
/// F-G), and the test hooks `offer_ip_override`, `answer_addr_override`.
fn rewrite_candidates(sdp: &str, ip: IpAddr, port: Option<u16>) -> String {
    sdp.split_inclusive('\n')
        .map(|line| match line.strip_prefix("a=candidate:") {
            Some(candidate) => {
                let mut fields: Vec<String> = candidate.split(' ').map(str::to_owned).collect();
                if let Some(field) = fields.get_mut(4) {
                    *field = ip.to_string();
                }
                if let (Some(field), Some(port)) = (fields.get_mut(5), port) {
                    *field = port.to_string();
                }
                format!("a=candidate:{}", fields.join(" "))
            }
            None => line.to_owned(),
        })
        .collect()
}

/// Polls the peer's link and sends its datagrams (through the impairment).
fn pump(udp: &UdpSocket, peer: &mut Peer, now: Instant, events: &mut Vec<LinkEvent>) {
    let mut out = Vec::new();
    peer.link.poll(now, &mut out, events);
    if let Some(impair) = &mut peer.impair {
        for t in out.drain(..) {
            impair.push(now, t);
        }
        impair.due(now, &mut out);
    }
    for t in &out {
        send_datagram(udp, &mut peer.stats, t);
    }
}

/// Closes the peer's link and sends its last datagrams at once.
fn finish(udp: &UdpSocket, mut peer: Peer, now: Instant) {
    peer.link.close();
    let mut out = Vec::new();
    peer.link.poll(now, &mut out, &mut Vec::new());
    for t in &out {
        send_datagram(udp, &mut peer.stats, t);
    }
}

impl Endpoint {
    fn new(cfg: EndpointConfig, udp: UdpSocket, port: u16, role: Role) -> Endpoint {
        let rng = Rng::new(cfg.impair_seed);
        Endpoint {
            cfg,
            udp,
            port,
            role,
            pending: Vec::new(),
            open: BTreeMap::new(),
            lingering: Vec::new(),
            rng,
            stray: 0,
            now: Instant::now(),
        }
    }

    /// A host on `cfg.port` (TCP and UDP, all interfaces); `gate` admits or
    /// refuses each valid offer.
    pub fn host(cfg: EndpointConfig, gate: Gate) -> io::Result<Endpoint> {
        let (listener, udp, port) = bind_both(cfg.port)?;
        #[cfg(feature = "test-hooks")]
        let next_id = cfg.first_peer_id.unwrap_or(1).max(1);
        #[cfg(not(feature = "test-hooks"))]
        let next_id = 1;
        Ok(Endpoint::new(cfg, udp, port, Role::Host { listener, gate, next_id, incoming: Vec::new() }))
    }

    /// A client of the host whose signalling (TCP) address is `host`. The
    /// connection is made by `poll`: `Connected`, `Refused` or `Disconnected`
    /// follows.
    pub fn connect(cfg: EndpointConfig, host: SocketAddr) -> io::Result<Endpoint> {
        let udp = bind_udp(cfg.port)?;
        let port = udp.local_addr()?.port();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new().name("rl-net connect".into()).spawn(move || {
            let _ = tx.send(TcpStream::connect_timeout(&host, CONNECT_TIMEOUT));
        })?;
        Ok(Endpoint::new(cfg, udp, port, Role::Client { host, connecting: Some((rx, None)) }))
    }

    /// The UDP port (on a host, the TCP port too).
    pub fn local_port(&self) -> u16 {
        self.port
    }

    fn fingerprint(&self) -> u64 {
        #[cfg(feature = "test-hooks")]
        if let Some(f) = self.cfg.fingerprint_override {
            return f;
        }
        FINGERPRINT
    }

    fn is_client(&self) -> bool {
        matches!(self.role, Role::Client { .. })
    }

    /// Does what the sockets allow at `now` and appends what happened to
    /// `events`. Never blocks; call it often (every frame, and after sends).
    pub fn poll(&mut self, now: Instant, events: &mut Vec<NetEvent>) {
        self.now = self.now.max(now);
        let now = self.now;
        self.poll_signalling(now, events);
        self.receive(now);
        let pending = std::mem::take(&mut self.pending);
        let open = std::mem::take(&mut self.open);
        for peer in pending.into_iter().chain(open.into_values()) {
            if let Some(peer) = self.step(peer, now, events) {
                match peer.id {
                    Some(id) => {
                        self.open.insert(id, peer);
                    }
                    None => self.pending.push(peer),
                }
            }
        }
        self.lingering.retain_mut(|(conn, deadline)| {
            let _ = conn.poll();
            !conn.is_closed() && now < *deadline
        });
    }

    /// Queues `bytes` as one message to `peer`; it leaves with the next
    /// `poll`. A message to an unknown peer is ignored; one beyond
    /// [`MAX_QUEUED`] or refused by the link (empty, too large) is dropped
    /// and counted. On the reliable channel a dropped message would break
    /// the ordered stream silently, so it also ends the link: the next
    /// `poll` reports `Disconnected { why: Why::Unsent }` (callers keep
    /// their own backlog and feed it as [`Endpoint::buffered`] drains).
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
    /// byte limit: a state transfer the session expects (the sender's
    /// messages are not limited). It also lifts the raw limits by twice the
    /// transfer, which a sender of a state well above 3 MB needs too: the
    /// receiver's SACKs count.
    pub fn allow_burst(&mut self, peer: PeerId, bytes: usize) {
        if let Some(p) = self.open.get_mut(&peer) {
            p.limits.allow_burst(bytes);
        }
    }

    /// Closes the link to `peer`; the peer is told, no event follows here.
    pub fn close(&mut self, peer: PeerId) {
        if let Some(p) = self.open.remove(&peer) {
            finish(&self.udp, p, self.now);
        }
    }

    /// The open peers, by id.
    pub fn peers(&self) -> impl Iterator<Item = (PeerId, SocketAddr)> + '_ {
        self.open.iter().map(|(&id, p)| (id, p.addr))
    }

    /// What went through the socket for an open peer.
    pub fn stats(&self, peer: PeerId) -> Option<PeerStats> {
        self.open.get(&peer).map(|p| p.stats)
    }

    /// Datagrams no link accepted (dropped).
    pub fn stray_datagrams(&self) -> u64 {
        self.stray
    }

    fn new_peer(
        &mut self,
        addr: SocketAddr,
        local: SocketAddr,
        link: Link,
        ws: SignalConn,
        now: Instant,
    ) -> Peer {
        let impair = self.cfg.impair.map(|p| Impair::new(p, self.rng.fork()));
        Peer {
            id: None,
            addr,
            local,
            link,
            ws: Some(ws),
            answered: !self.is_client(),
            deadline: now + SETUP_TIMEOUT,
            heard: now,
            broke: None,
            limits: PeerLimits::new(&self.cfg.limits, now),
            stats: PeerStats::default(),
            impair,
        }
    }

    /// Host: accepts TCP connections and takes their offers. Client:
    /// finishes the TCP connect and sends the offer.
    fn poll_signalling(&mut self, now: Instant, events: &mut Vec<NetEvent>) {
        let max = self.cfg.limits.max_signal_bytes;
        match &mut self.role {
            Role::Host { listener, incoming, .. } => {
                for _ in 0..MAX_ACCEPTS_PER_POLL {
                    let Ok((stream, addr)) = listener.accept() else { break };
                    let ip = addr.ip();
                    let same_ip = incoming.iter().filter(|i| i.addr.ip() == ip).count()
                        + self.pending.iter().filter(|p| p.addr.ip() == ip).count();
                    if incoming.len() >= MAX_INCOMING
                        || same_ip >= MAX_PER_IP
                        || stream.set_nonblocking(true).is_err()
                    {
                        continue;
                    }
                    let _ = stream.set_nodelay(true);
                    let Ok(local) = stream.local_addr() else { continue };
                    let conn = SignalConn::accept(stream, max);
                    incoming.push(Incoming { conn, addr, local: local.ip(), deadline: now + OFFER_TIMEOUT });
                }
                let waiting = std::mem::take(incoming);
                let mut kept = Vec::new();
                for mut inc in waiting {
                    match inc.conn.poll() {
                        Some(frame) => self.on_offer(inc, &frame, now),
                        None if !inc.conn.is_closed() && now < inc.deadline => kept.push(inc),
                        None => {}
                    }
                }
                if let Role::Host { incoming, .. } = &mut self.role {
                    incoming.append(&mut kept);
                }
            }
            Role::Client { host, connecting } => {
                let host = *host;
                let Some((rx, started)) = connecting else { return };
                let started = *started.get_or_insert(now);
                let result = match rx.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) if now < started + CONNECT_TIMEOUT + LINGER => return,
                    Err(TryRecvError::Empty) => Err(io::ErrorKind::TimedOut.into()),
                    Err(TryRecvError::Disconnected) => Err(io::Error::other("the connect thread ended")),
                };
                *connecting = None;
                match result.and_then(|stream| self.offer(stream, host, now)) {
                    Ok(peer) => self.pending.push(peer),
                    Err(e) => {
                        let why =
                            if e.kind() == io::ErrorKind::TimedOut { Why::Timeout } else { Why::Closed };
                        events.push(NetEvent::Disconnected { peer: HOST, why });
                    }
                }
            }
        }
    }

    /// Client: the link and its offer over the new signalling connection.
    fn offer(&mut self, stream: TcpStream, host: SocketAddr, now: Instant) -> io::Result<Peer> {
        stream.set_nonblocking(true)?;
        let _ = stream.set_nodelay(true);
        let local = SocketAddr::new(stream.local_addr()?.ip(), self.port);
        #[cfg_attr(not(feature = "test-hooks"), allow(unused_mut))]
        let (link, mut sdp) = Link::offerer(&[local], now);
        #[cfg(feature = "test-hooks")]
        if let Some(ip) = self.cfg.offer_ip_override {
            sdp = rewrite_candidates(&sdp, ip, None);
        }
        let mut ws = SignalConn::connect(stream, host, self.cfg.limits.max_signal_bytes);
        ws.send(encode_signal(&Signal::Offer {
            protocol: PROTOCOL_VERSION,
            fingerprint: self.fingerprint(),
            sdp,
        }));
        Ok(self.new_peer(host, local, link, ws, now))
    }

    /// Host: checks an offer and answers or refuses it.
    fn on_offer(&mut self, inc: Incoming, frame: &[u8], now: Instant) {
        let Incoming { mut conn, addr, local, .. } = inc;
        let signal = match decode_signal(frame) {
            Ok(signal @ Signal::Offer { .. }) => signal,
            _ => return self.refuse(conn, Refusal::Malformed, now),
        };
        let Signal::Offer { protocol, fingerprint, sdp } = &signal else { return };
        if *protocol != PROTOCOL_VERSION || *fingerprint != self.fingerprint() {
            return self.refuse(conn, Refusal::Version, now);
        }
        if self.pending.len() + self.open.len() >= self.cfg.max_peers {
            return self.refuse(conn, Refusal::Full, now);
        }
        let Role::Host { gate, next_id, .. } = &mut self.role else { return };
        // Ids are never reused: refuse once they run out (4 billion links:
        // unreachable in practice).
        if u64::from(*next_id) + self.pending.len() as u64 >= u64::from(PeerId::MAX) {
            return self.refuse(conn, Refusal::Exhausted, now);
        }
        if let Err(why) = gate(&addr, &signal) {
            return self.refuse(conn, why, now);
        }
        // Only candidates on the signalling connection's IP (ruling T4-A).
        let sdp = keep_candidates_of(sdp, addr.ip());
        let local = SocketAddr::new(local, self.port);
        #[cfg_attr(not(feature = "test-hooks"), allow(unused_mut))]
        let Ok((link, mut answer)) = Link::answerer(&[local], &sdp, now) else {
            return self.refuse(conn, Refusal::Malformed, now);
        };
        #[cfg(feature = "test-hooks")]
        if let Some(a) = self.cfg.answer_addr_override {
            answer = rewrite_candidates(&answer, a.ip(), Some(a.port()));
        }
        conn.send(encode_signal(&Signal::Answer { sdp: answer }));
        let peer = self.new_peer(addr, local, link, conn, now);
        self.pending.push(peer);
    }

    fn refuse(&mut self, mut conn: SignalConn, why: Refusal, now: Instant) {
        conn.send(encode_signal(&Signal::Refused { reason: why }));
        self.linger(conn, now);
    }

    /// Closes `conn`, giving it [`LINGER`] to go; at most [`MAX_INCOMING`]
    /// linger, others are dropped at once (so under a flood of connections
    /// a refusal may not reach its joiner, who sees the connection close).
    fn linger(&mut self, mut conn: SignalConn, now: Instant) {
        conn.close();
        if self.lingering.len() < MAX_INCOMING {
            self.lingering.push((conn, now + LINGER));
        }
    }

    /// Hands each received datagram to the link that accepts it.
    fn receive(&mut self, now: Instant) {
        let mut buf = [0u8; 2048];
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            let (n, from) = match self.udp.recv_from(&mut buf) {
                Ok(got) => got,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // E.g. Windows reports an earlier datagram's ICMP error here.
                Err(_) => continue,
            };
            let bytes = &buf[..n];
            let peer = self
                .pending
                .iter_mut()
                .chain(self.open.values_mut())
                .find(|p| p.link.accepts(from, p.local, bytes));
            match peer {
                Some(p) if p.broke.is_none() => {
                    p.stats.datagrams_received += 1;
                    p.stats.bytes_received += n as u64;
                    if let Err(why) = p.limits.on_datagram(now, n) {
                        p.broke = Some(why);
                        continue;
                    }
                    p.heard = now;
                    p.link.receive(from, p.local, bytes, now);
                }
                Some(_) => {}
                None => self.stray += 1,
            }
        }
    }

    /// Drives one peer: its signalling, its link, its limits and timeouts.
    /// `None` when it is gone.
    fn step(&mut self, mut peer: Peer, now: Instant, events: &mut Vec<NetEvent>) -> Option<Peer> {
        if let Some(why) = peer.broke {
            return match peer.id {
                Some(id) => {
                    events.push(NetEvent::Disconnected { peer: id, why });
                    finish(&self.udp, peer, now);
                    None
                }
                None => self.fail_setup(peer, why, now, events),
            };
        }
        if let Some(ws) = &mut peer.ws {
            if let Some(frame) = ws.poll() {
                // After the offer (host) or the answer (client) nothing more
                // may come: one more frame is a violation.
                if peer.answered {
                    return self.fail_setup(peer, Why::Malformed, now, events);
                }
                match decode_signal(&frame) {
                    // Every candidate becomes the address the client dialled
                    // (ruling F-G): a host behind NAT (a port forward,
                    // Docker, a cloud VM) answers with its private address,
                    // and the host's TCP and UDP share the port, so a
                    // forward maps both alike; a host still cannot aim its
                    // joiners' checks at a third party (T4-A).
                    Ok(Signal::Answer { sdp })
                        if peer
                            .link
                            .accept_answer(&rewrite_candidates(&sdp, peer.addr.ip(), Some(peer.addr.port())))
                            .is_ok() =>
                    {
                        peer.answered = true
                    }
                    Ok(Signal::Refused { reason }) => {
                        events.push(NetEvent::Refused { why: reason });
                        return None;
                    }
                    _ => return self.fail_setup(peer, Why::Malformed, now, events),
                }
            }
            if ws.is_closed() {
                if !peer.answered {
                    return self.fail_setup(peer, Why::Closed, now, events);
                }
                peer.ws = None;
            }
        }
        let mut link_events = Vec::new();
        pump(&self.udp, &mut peer, now, &mut link_events);
        for event in link_events {
            match event {
                LinkEvent::Open => {
                    let id = match &mut self.role {
                        Role::Host { next_id, .. } => {
                            *next_id += 1;
                            *next_id - 1
                        }
                        Role::Client { .. } => HOST,
                    };
                    peer.id = Some(id);
                    events.push(NetEvent::Connected { peer: id, addr: peer.addr });
                    if let Some(ws) = peer.ws.take() {
                        self.linger(ws, now);
                    }
                }
                LinkEvent::Message { channel, bytes } => {
                    let Some(id) = peer.id else { continue };
                    match peer.limits.on_message(now, channel == Channel::Reliable, bytes.len()) {
                        Ok(()) => events.push(NetEvent::Message { peer: id, channel, bytes }),
                        Err(why) => {
                            events.push(NetEvent::Disconnected { peer: id, why });
                            finish(&self.udp, peer, now);
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
                        Some(id) => events.push(NetEvent::Disconnected { peer: id, why }),
                        None if self.is_client() => events.push(NetEvent::Disconnected { peer: HOST, why }),
                        None => {}
                    }
                    return None;
                }
            }
        }
        match peer.id {
            Some(id) if now.saturating_duration_since(peer.heard) >= SILENCE_TIMEOUT => {
                events.push(NetEvent::Disconnected { peer: id, why: Why::Timeout });
                finish(&self.udp, peer, now);
                None
            }
            None if now >= peer.deadline => self.fail_setup(peer, Why::Timeout, now, events),
            _ => Some(peer),
        }
    }

    /// A connection that did not come up: the client hears why, the host
    /// drops it quietly.
    fn fail_setup(&mut self, peer: Peer, why: Why, now: Instant, events: &mut Vec<NetEvent>) -> Option<Peer> {
        if self.is_client() {
            events.push(NetEvent::Disconnected { peer: HOST, why });
        }
        finish(&self.udp, peer, now);
        None
    }
}

impl Drop for Endpoint {
    /// Tells every peer the link is closed.
    fn drop(&mut self) {
        let now = self.now;
        for peer in
            std::mem::take(&mut self.pending).into_iter().chain(std::mem::take(&mut self.open).into_values())
        {
            finish(&self.udp, peer, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDP: &str = "v=0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n\
        a=candidate:1 1 udp 2130706431 192.168.1.5 50000 typ host\r\n\
        a=candidate:2 1 udp 2130706431 203.0.113.7 50000 typ host\r\n\
        a=candidate:3 1 udp 2130706431 x.local 50000 typ host\r\n\
        a=end-of-candidates\r\n";

    #[test]
    fn only_the_joiners_own_candidates_are_kept() {
        let kept = keep_candidates_of(SDP, "203.0.113.7".parse().unwrap());
        assert_eq!(
            kept,
            "v=0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n\
             a=candidate:2 1 udp 2130706431 203.0.113.7 50000 typ host\r\n\
             a=end-of-candidates\r\n"
        );
        let none = keep_candidates_of(SDP, "10.0.0.1".parse().unwrap());
        assert!(!none.contains("a=candidate"), "{none}");
    }

    #[test]
    fn every_candidate_is_rewritten() {
        let ip: IpAddr = "10.9.9.9".parse().unwrap();
        let rewritten = rewrite_candidates(SDP, ip, None);
        assert_eq!(rewritten.matches("a=candidate:").count(), 3);
        assert_eq!(keep_candidates_of(&rewritten, ip), rewritten);
        assert!(rewritten.contains("a=candidate:3 1 udp 2130706431 10.9.9.9 50000 typ host\r\n"));
    }

    /// A client sees every answer candidate as the address it dialled
    /// (ruling F-G), port included.
    #[test]
    fn an_answer_becomes_the_dialled_address() {
        let rewritten = rewrite_candidates(SDP, "198.51.100.4".parse().unwrap(), Some(23400));
        assert_eq!(rewritten.matches("a=candidate:").count(), 3);
        assert_eq!(rewritten.matches(" 198.51.100.4 23400 typ host\r\n").count(), 3, "{rewritten}");
        assert!(rewritten.ends_with("a=end-of-candidates\r\n"));
    }
}
