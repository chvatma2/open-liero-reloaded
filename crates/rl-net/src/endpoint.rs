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
use rl_netcode::{Channel, PeerId};
use rl_proto::{PROTOCOL_VERSION, Refusal, Signal, decode_signal, encode_signal};

use crate::core::{Core, CoreConfig, CoreEvent, Datagram, SetupId};
pub use crate::core::{HOST, MAX_QUEUED, NetEvent, PeerStats, SETUP_TIMEOUT, SILENCE_TIMEOUT, Why};
pub use crate::limits::Limits;
use crate::signal::SignalConn;

/// The host's default TCP and UDP port (spec §5.1).
pub const DEFAULT_PORT: u16 = 23400;
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
    /// The default port and [`CoreConfig::host`]'s defaults: 32 players and
    /// 16 spectators (spec §7.2), the default limits, no impairment.
    fn default() -> Self {
        // The core's defaults, written once (`CoreConfig::host`).
        let core = CoreConfig::host();
        EndpointConfig {
            port: DEFAULT_PORT,
            max_peers: core.max_peers,
            limits: core.limits,
            impair: core.impair,
            impair_seed: core.impair_seed,
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

/// The signalling side of one of the core's setups, until its link opens
/// or fails.
struct Setup {
    /// The signalling connection's remote address.
    addr: SocketAddr,
    /// The signalling connection, until the link opens (or it closes after
    /// the answer).
    ws: Option<SignalConn>,
    /// Client: the host's answer is taken. Host: always.
    answered: bool,
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
        incoming: Vec<Incoming>,
    },
    Client {
        host: SocketAddr,
        /// The TCP connect running on its own thread, and when we started waiting.
        connecting: Option<(Receiver<io::Result<TcpStream>>, Option<Instant>)>,
    },
}

/// One end of online play: a host and its peers, or a client and its host.
/// The socket driver of a [`Core`].
pub struct Endpoint {
    cfg: EndpointConfig,
    udp: UdpSocket,
    port: u16,
    role: Role,
    /// The links, their limits, ids and deadlines.
    core: Core,
    /// The signalling of the core's setups not open yet.
    setups: BTreeMap<SetupId, Setup>,
    /// Signalling connections closing.
    lingering: Vec<(SignalConn, Instant)>,
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

/// Sends the core's datagrams and counts them in their peers' stats.
fn send_datagrams(udp: &UdpSocket, core: &mut Core, out: &mut Vec<Datagram>) {
    for d in out.drain(..) {
        // A full socket buffer loses the datagram, as a network would.
        let ok = udp.send_to(&d.bytes, d.to).is_ok();
        core.sent(&d, ok);
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

/// The STUN transaction ids' seed: the system's randomness (std's
/// randomly keyed hasher if it fails).
fn stun_seed() -> u64 {
    crate::random_bytes::<8>().map(u64::from_le_bytes).unwrap_or_else(|_| {
        use std::hash::{BuildHasher, RandomState};
        RandomState::new().hash_one(0u8)
    })
}

/// The core's configuration of an endpoint's.
fn core_config(cfg: &EndpointConfig, host: bool) -> CoreConfig {
    CoreConfig {
        host,
        max_peers: cfg.max_peers,
        limits: cfg.limits,
        impair: cfg.impair,
        impair_seed: cfg.impair_seed,
        stun_seed: stun_seed(),
        allow_local: false,
        #[cfg(feature = "test-hooks")]
        fingerprint_override: cfg.fingerprint_override,
        #[cfg(feature = "test-hooks")]
        first_peer_id: cfg.first_peer_id,
        #[cfg(feature = "test-hooks")]
        suppress_stats: false,
    }
}

impl Endpoint {
    fn new(cfg: EndpointConfig, udp: UdpSocket, port: u16, role: Role) -> Endpoint {
        let now = Instant::now();
        let core = Core::new(core_config(&cfg, matches!(role, Role::Host { .. })), now);
        Endpoint { cfg, udp, port, role, core, setups: BTreeMap::new(), lingering: Vec::new(), now }
    }

    /// A host on `cfg.port` (TCP and UDP, all interfaces); `gate` admits or
    /// refuses each valid offer.
    pub fn host(cfg: EndpointConfig, gate: Gate) -> io::Result<Endpoint> {
        let (listener, udp, port) = bind_both(cfg.port)?;
        Ok(Endpoint::new(cfg, udp, port, Role::Host { listener, gate, incoming: Vec::new() }))
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

    /// Does what the sockets allow at `now` and appends what happened to
    /// `events`. Never blocks; call it often (every frame, and after sends).
    pub fn poll(&mut self, now: Instant, events: &mut Vec<NetEvent>) {
        self.now = self.now.max(now);
        let now = self.now;
        let mut core_events = Vec::new();
        self.poll_signalling(now, events);
        self.receive(now);
        self.poll_setups(events, &mut core_events);
        let mut out = Vec::new();
        self.core.poll(now, &mut out, &mut core_events);
        send_datagrams(&self.udp, &mut self.core, &mut out);
        self.on_core_events(core_events, now, events);
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
        self.core.send(peer, channel, bytes);
    }

    /// The bytes waiting to leave for `peer` on `channel` (sent and not yet
    /// acknowledged, on the reliable channel).
    pub fn buffered(&self, peer: PeerId, channel: Channel) -> usize {
        self.core.buffered(peer, channel)
    }

    /// Lets the next `bytes` of reliable messages from `peer` through the
    /// byte limit: a state transfer the session expects (the sender's
    /// messages are not limited). It also lifts the raw limits by twice the
    /// transfer, which a sender of a state well above 3 MB needs too: the
    /// receiver's SACKs count.
    pub fn allow_burst(&mut self, peer: PeerId, bytes: usize) {
        self.core.allow_burst(peer, bytes);
    }

    /// Closes the link to `peer`; the peer is told, no event follows here.
    pub fn close(&mut self, peer: PeerId) {
        let mut out = Vec::new();
        self.core.close(peer, &mut out);
        send_datagrams(&self.udp, &mut self.core, &mut out);
    }

    /// The open peers, by id.
    pub fn peers(&self) -> impl Iterator<Item = (PeerId, SocketAddr)> + '_ {
        self.core.peers()
    }

    /// The DTLS fingerprints of an open peer's link, offer side's then
    /// answer side's (`Link::dtls_fingerprints`): the same at both ends.
    pub fn dtls_fingerprints(&self, peer: PeerId) -> Option<([u8; 32], [u8; 32])> {
        self.core.dtls_fingerprints(peer)
    }

    /// What went through the socket for an open peer.
    pub fn stats(&self, peer: PeerId) -> Option<PeerStats> {
        self.core.stats(peer)
    }

    /// Datagrams no link accepted (dropped).
    pub fn stray_datagrams(&self) -> u64 {
        self.core.stray_datagrams()
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
                    // The setups are the core's links not open yet.
                    let same_ip = incoming.iter().filter(|i| i.addr.ip() == ip).count()
                        + self.setups.values().filter(|s| s.addr.ip() == ip).count();
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
                if let Err(e) = result.and_then(|stream| self.offer(stream, host, now)) {
                    let why = if e.kind() == io::ErrorKind::TimedOut { Why::Timeout } else { Why::Closed };
                    events.push(NetEvent::Disconnected { peer: HOST, why });
                }
            }
        }
    }

    /// Client: the link and its offer over the new signalling connection.
    fn offer(&mut self, stream: TcpStream, host: SocketAddr, now: Instant) -> io::Result<()> {
        stream.set_nonblocking(true)?;
        let _ = stream.set_nodelay(true);
        let local = SocketAddr::new(stream.local_addr()?.ip(), self.port);
        #[cfg_attr(not(feature = "test-hooks"), allow(unused_mut))]
        let (setup, mut sdp) = self.core.offer(&[local], host, now);
        #[cfg(feature = "test-hooks")]
        if let Some(ip) = self.cfg.offer_ip_override {
            sdp = rewrite_candidates(&sdp, ip, None);
        }
        let mut ws = SignalConn::connect(stream, host, self.cfg.limits.max_signal_bytes);
        ws.send(encode_signal(&Signal::Offer {
            protocol: PROTOCOL_VERSION,
            fingerprint: self.core.fingerprint(),
            sdp,
        }));
        self.setups.insert(setup, Setup { addr: host, ws: Some(ws), answered: false });
        Ok(())
    }

    /// Host: checks an offer and answers or refuses it.
    fn on_offer(&mut self, inc: Incoming, frame: &[u8], now: Instant) {
        let Incoming { mut conn, addr, local, .. } = inc;
        let signal = match decode_signal(frame) {
            Ok(signal @ Signal::Offer { .. }) => signal,
            _ => return self.refuse(conn, Refusal::Malformed, now),
        };
        if let Err(why) = self.core.admit(&signal) {
            return self.refuse(conn, why, now);
        }
        let Role::Host { gate, .. } = &mut self.role else { return };
        if let Err(why) = gate(&addr, &signal) {
            return self.refuse(conn, why, now);
        }
        let Signal::Offer { sdp, .. } = &signal else { return };
        // Only candidates on the signalling connection's IP (ruling T4-A).
        let sdp = keep_candidates_of(sdp, addr.ip());
        let local = SocketAddr::new(local, self.port);
        #[cfg_attr(not(feature = "test-hooks"), allow(unused_mut))]
        let (setup, mut answer) = match self.core.answer(&[local], &sdp, addr, now) {
            Ok(answered) => answered,
            Err(why) => return self.refuse(conn, why, now),
        };
        #[cfg(feature = "test-hooks")]
        if let Some(a) = self.cfg.answer_addr_override {
            answer = rewrite_candidates(&answer, a.ip(), Some(a.port()));
        }
        conn.send(encode_signal(&Signal::Answer { sdp: answer }));
        self.setups.insert(setup, Setup { addr, ws: Some(conn), answered: true });
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

    /// Hands each received datagram to the core.
    fn receive(&mut self, now: Instant) {
        let mut buf = [0u8; 2048];
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            let (n, from) = match self.udp.recv_from(&mut buf) {
                Ok(got) => got,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // E.g. Windows reports an earlier datagram's ICMP error here.
                Err(_) => continue,
            };
            self.core.receive(from, &buf[..n], now);
        }
    }

    /// Drives the signalling of the setups not open yet: the client takes
    /// the answer or a refusal; one more frame, or a connection closed
    /// before the answer, cancels the setup.
    fn poll_setups(&mut self, events: &mut Vec<NetEvent>, core_events: &mut Vec<CoreEvent>) {
        let setups = std::mem::take(&mut self.setups);
        for (id, mut setup) in setups {
            // A setup that broke a limit fails in the core's poll, its
            // signalling untouched.
            if !self.core.broke(id) && !self.poll_setup(id, &mut setup, events, core_events) {
                continue;
            }
            self.setups.insert(id, setup);
        }
    }

    /// One setup's signalling; `false` when the setup is gone.
    fn poll_setup(
        &mut self,
        id: SetupId,
        setup: &mut Setup,
        events: &mut Vec<NetEvent>,
        core_events: &mut Vec<CoreEvent>,
    ) -> bool {
        let Some(ws) = &mut setup.ws else { return true };
        if let Some(frame) = ws.poll() {
            // After the offer (host) or the answer (client) nothing more
            // may come: one more frame is a violation.
            if setup.answered {
                self.core.cancel(id, Why::Malformed, core_events);
                return false;
            }
            match decode_signal(&frame) {
                // Every candidate becomes the address the client dialled
                // (ruling F-G): a host behind NAT (a port forward,
                // Docker, a cloud VM) answers with its private address,
                // and the host's TCP and UDP share the port, so a
                // forward maps both alike; a host still cannot aim its
                // joiners' checks at a third party (T4-A).
                Ok(Signal::Answer { sdp })
                    if self
                        .core
                        .accept_answer(
                            id,
                            &rewrite_candidates(&sdp, setup.addr.ip(), Some(setup.addr.port())),
                        )
                        .is_ok() =>
                {
                    setup.answered = true
                }
                Ok(Signal::Refused { reason }) => {
                    events.push(NetEvent::Refused { why: reason });
                    // Refused is all the client hears.
                    self.core.cancel(id, Why::Refused, &mut Vec::new());
                    return false;
                }
                _ => {
                    self.core.cancel(id, Why::Malformed, core_events);
                    return false;
                }
            }
        }
        if ws.is_closed() {
            if !setup.answered {
                self.core.cancel(id, Why::Closed, core_events);
                return false;
            }
            setup.ws = None;
        }
        true
    }

    /// Turns the core's events into the endpoint's: an opened setup's
    /// signalling lingers, a failed one's is dropped.
    fn on_core_events(&mut self, core_events: Vec<CoreEvent>, now: Instant, events: &mut Vec<NetEvent>) {
        for event in core_events {
            match event {
                CoreEvent::Net(event) => events.push(event),
                CoreEvent::Opened { setup, .. } => {
                    if let Some(ws) = self.setups.remove(&setup).and_then(|s| s.ws) {
                        self.linger(ws, now);
                    }
                }
                CoreEvent::SetupFailed { setup, .. } => {
                    self.setups.remove(&setup);
                }
            }
        }
    }
}

impl Drop for Endpoint {
    /// Tells every peer the link is closed.
    fn drop(&mut self) {
        let mut out = Vec::new();
        self.core.close_all(&mut out);
        send_datagrams(&self.udp, &mut self.core, &mut out);
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
