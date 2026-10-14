//! TURN relays (decision N52, spec §5.3): the sans-IO relay core
//! ([`TurnCore`], wrapping `turn_server_proto`'s UDP server) with its
//! limits per client IP, and its socket driver ([`TurnServer`], a thread
//! with the listening socket and one UDP socket per allocation).
//!
//! A volunteer relay must not become a free general proxy: every client
//! IP gets a few allocations and about 32 KiB/s each way. Over a limit the
//! datagram is dropped and counted, never the client (a relay has no
//! session to drop). A relay also answers STUN binding requests (itself:
//! turn-server-proto 0.7.2 addresses its binding answer to its own
//! listening address).

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rl_proto::stun::{binding_request_transaction, binding_response};
use turn_server_proto::api::{SocketAllocateError, Transmit, TurnServerApi, TurnServerPollRet};
use turn_server_proto::server::TurnServer as Proto;
use turn_server_proto::stun::Instant as ProtoInstant;
use turn_server_proto::types::prelude::DelayedTransmitBuild;
use turn_server_proto::types::{AddressFamily, TransportType};

/// The realm of every rust-liero relay (N52: the credentials are public).
pub const REALM: &str = "rust-liero";

/// At most this many client IPs are tracked; a new one evicts the one
/// idle longest (holding no allocation, when one exists).
const MAX_IPS: usize = 4096;
/// A token bucket holds this many seconds of its rate.
const BURST_SECS: u128 = 2;
const NANOS: u128 = 1_000_000_000;
/// One library poll loop's bound (it returns one action at a time).
const MAX_ACTIONS: usize = 1024;

/// What a relay allows (N52).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnLimits {
    /// Allocations in all.
    pub capacity: usize,
    /// Allocations per client IP.
    pub per_ip_allocations: usize,
    /// Relayed payload bytes per second per client IP, each direction
    /// (a bucket holding 2 s).
    pub per_ip_bytes_per_s: u32,
    /// STUN/TURN requests per second per client IP (a bucket holding 2 s).
    pub per_ip_requests_per_s: u32,
}

impl Default for TurnLimits {
    /// 32 allocations, 4 per IP, 32 KiB/s each way, 20 requests/s (N52).
    fn default() -> TurnLimits {
        TurnLimits {
            capacity: 32,
            per_ip_allocations: 4,
            per_ip_bytes_per_s: 32 * 1024,
            per_ip_requests_per_s: 20,
        }
    }
}

impl TurnLimits {
    /// For tests that share 127.0.0.1: 256 allocations, 64 per IP, 64 MiB/s, 10 000 requests/s.
    pub fn generous() -> TurnLimits {
        TurnLimits {
            capacity: 256,
            per_ip_allocations: 64,
            per_ip_bytes_per_s: 64 * 1024 * 1024,
            per_ip_requests_per_s: 10_000,
        }
    }
}

/// One datagram for the driver to send from its port `from_port` (the
/// listening port or a relay port).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnOut {
    pub from_port: u16,
    pub to: SocketAddr,
    pub bytes: Vec<u8>,
}

/// The library asked for a relay socket for `client`: the driver picks the port and calls `allocated`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationRequest {
    pub client: SocketAddr,
    pub(crate) id: u64,
}

/// A token bucket of `rate` per second holding [`BURST_SECS`], in
/// billionths of a token; it starts full.
#[derive(Clone, Copy, Debug)]
struct Bucket {
    nanos: u128,
    at: Instant,
}

impl Bucket {
    fn full(rate: u32, now: Instant) -> Bucket {
        Bucket { nanos: BURST_SECS * u128::from(rate) * NANOS, at: now }
    }

    /// Takes `cost` tokens if there are that many.
    fn take(&mut self, rate: u32, cost: u64, now: Instant) -> bool {
        let cap = BURST_SECS * u128::from(rate) * NANOS;
        let elapsed = now.saturating_duration_since(self.at).as_nanos();
        self.at = self.at.max(now);
        self.nanos = self.nanos.saturating_add(elapsed.saturating_mul(u128::from(rate))).min(cap);
        let need = u128::from(cost) * NANOS;
        if self.nanos >= need {
            self.nanos -= need;
            true
        } else {
            false
        }
    }
}

/// What one client IP used.
struct IpState {
    /// Allocations, waiting for a port or bound.
    allocations: usize,
    /// Relayed payload: client to peers, and peers to the client.
    up: Bucket,
    down: Bucket,
    requests: Bucket,
    last: Instant,
}

/// An allocation the library asked for, waiting for the driver's port.
struct Asked {
    transport: TransportType,
    listen: SocketAddr,
    client: SocketAddr,
    allocation_transport: TransportType,
    family: AddressFamily,
}

/// The relay without sockets: `turn_server_proto`'s UDP server and the
/// limits per client IP (N52).
pub struct TurnCore {
    proto: Proto,
    listen: SocketAddr,
    public: IpAddr,
    limits: TurnLimits,
    /// The library's time zero.
    epoch: Instant,
    ips: BTreeMap<IpAddr, IpState>,
    /// Admitted allocations not yet handed to the driver, and handed ones
    /// waiting for its `allocated`.
    requests: Vec<AllocationRequest>,
    asked: BTreeMap<u64, Asked>,
    next_id: u64,
    /// Relay ports and their client's IP.
    relays: BTreeMap<u16, IpAddr>,
    released: Vec<u16>,
    out: Vec<TurnOut>,
    dropped: u64,
    relayed_bytes: u64,
}

/// Whether a datagram to the listening port is relayed data (ChannelData,
/// or a STUN Send indication), not a request.
fn is_data(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(|b| b & 0xc0 == 0x40) || bytes.get(..2) == Some(&[0x00, 0x16])
}

impl TurnCore {
    /// Listens on `listen`; relayed addresses are `public` with the port the driver allocated.
    pub fn new(
        listen: SocketAddr,
        public: IpAddr,
        user: &str,
        pass: &str,
        limits: TurnLimits,
        now: Instant,
    ) -> TurnCore {
        let mut proto = Proto::new(TransportType::Udp, listen, REALM.to_owned());
        proto.add_user(user.to_owned(), pass.to_owned());
        TurnCore {
            proto,
            listen,
            public,
            limits,
            epoch: now,
            ips: BTreeMap::new(),
            requests: Vec::new(),
            asked: BTreeMap::new(),
            next_id: 0,
            relays: BTreeMap::new(),
            released: Vec::new(),
            out: Vec::new(),
            dropped: 0,
            relayed_bytes: 0,
        }
    }

    /// The library's time for `now`.
    fn time(&self, now: Instant) -> ProtoInstant {
        let nanos = now.saturating_duration_since(self.epoch).as_nanos();
        ProtoInstant::from_nanos(i64::try_from(nanos).unwrap_or(i64::MAX))
    }

    /// The state of client IP `ip`, made (and the table kept bounded) if new.
    fn ip(&mut self, ip: IpAddr, now: Instant) -> &mut IpState {
        if !self.ips.contains_key(&ip) && self.ips.len() >= MAX_IPS {
            let idle = |s: &IpState| (s.allocations > 0, s.last);
            if let Some(oldest) = self.ips.iter().min_by_key(|(_, s)| idle(s)).map(|(&k, _)| k) {
                self.ips.remove(&oldest);
            }
        }
        let l = self.limits;
        let state = self.ips.entry(ip).or_insert_with(|| IpState {
            allocations: 0,
            up: Bucket::full(l.per_ip_bytes_per_s, now),
            down: Bucket::full(l.per_ip_bytes_per_s, now),
            requests: Bucket::full(l.per_ip_requests_per_s, now),
            last: now,
        });
        state.last = state.last.max(now);
        state
    }

    /// A datagram to the listening port.
    pub fn receive(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) {
        let limits = self.limits;
        let data = is_data(bytes);
        let state = self.ip(from.ip(), now);
        if !data && !state.requests.take(limits.per_ip_requests_per_s, 1, now) {
            self.dropped += 1;
            return;
        }
        if let Some(transaction) = binding_request_transaction(bytes) {
            // The library's answer would go to the listening address itself.
            let bytes = binding_response(&transaction, from, true);
            self.out.push(TurnOut { from_port: self.listen.port(), to: from, bytes });
            return;
        }
        let t = self.time(now);
        if let Some(build) = self.proto.recv(Transmit::new(bytes, TransportType::Udp, from, self.listen), t) {
            if build.from != self.listen {
                // Relayed to a peer: the client's upward bytes.
                let payload = build.data.len() as u64;
                let state = self.ip(from.ip(), now);
                if !state.up.take(limits.per_ip_bytes_per_s, payload, now) {
                    self.dropped += 1;
                    return;
                }
                self.relayed_bytes += payload;
            }
            let tx = build.build();
            self.out.push(TurnOut { from_port: tx.from.port(), to: tx.to, bytes: tx.data });
        }
        self.drain(now);
    }

    /// A datagram to relay port `port` from a peer.
    pub fn receive_relayed(&mut self, port: u16, from: SocketAddr, bytes: &[u8], now: Instant) {
        let Some(&owner) = self.relays.get(&port) else { return };
        let limits = self.limits;
        let state = self.ip(owner, now);
        if !state.down.take(limits.per_ip_bytes_per_s, bytes.len() as u64, now) {
            self.dropped += 1;
            return;
        }
        let t = self.time(now);
        let relay = SocketAddr::new(self.public, port);
        if let Some(build) = self.proto.recv(Transmit::new(bytes, TransportType::Udp, from, relay), t) {
            self.relayed_bytes += bytes.len() as u64;
            let tx = build.build();
            self.out.push(TurnOut { from_port: tx.from.port(), to: tx.to, bytes: tx.data });
        }
    }

    /// Steps the library at `now` (expiries, allocations ending) and hands
    /// out the datagrams to send.
    pub fn poll(&mut self, now: Instant, out: &mut Vec<TurnOut>) {
        self.drain(now);
        out.append(&mut self.out);
    }

    /// Runs the library's actions and takes its datagrams.
    fn drain(&mut self, now: Instant) {
        let t = self.time(now);
        for _ in 0..MAX_ACTIONS {
            match self.proto.poll(t) {
                TurnServerPollRet::WaitUntil(_) => break,
                TurnServerPollRet::AllocateSocket {
                    transport,
                    listen_addr,
                    client_addr,
                    allocation_transport,
                    family,
                } => {
                    let asked = Asked {
                        transport,
                        listen: listen_addr,
                        client: client_addr,
                        allocation_transport,
                        family,
                    };
                    self.admit(asked, now);
                }
                TurnServerPollRet::SocketClose { listen_addr, .. } => self.close(listen_addr.port()),
                // A UDP server makes no TCP connections.
                TurnServerPollRet::TcpConnect { .. } | TurnServerPollRet::TcpClose { .. } => {}
            }
        }
        while let Some(tx) = self.proto.poll_transmit(t) {
            self.out.push(TurnOut { from_port: tx.from.port(), to: tx.to, bytes: tx.data });
        }
    }

    /// An allocation the library asks for: refused when the relay or the
    /// client's IP is full (the library answers 508), else handed to the
    /// driver.
    fn admit(&mut self, asked: Asked, now: Instant) {
        let family = if self.public.is_ipv4() { AddressFamily::IPV4 } else { AddressFamily::IPV6 };
        let refusal = if asked.family != family || asked.allocation_transport != TransportType::Udp {
            Some(SocketAllocateError::AddressFamilyNotSupported)
        } else if self.relays.len() + self.asked.len() >= self.limits.capacity
            || self.ip(asked.client.ip(), now).allocations >= self.limits.per_ip_allocations
        {
            Some(SocketAllocateError::InsufficientCapacity)
        } else {
            None
        };
        if let Some(why) = refusal {
            let t = self.time(now);
            self.proto.allocated_socket(
                asked.transport,
                asked.listen,
                asked.client,
                asked.allocation_transport,
                asked.family,
                Err(why),
                t,
            );
            return;
        }
        self.ip(asked.client.ip(), now).allocations += 1;
        let id = self.next_id;
        self.next_id += 1;
        self.requests.push(AllocationRequest { client: asked.client, id });
        self.asked.insert(id, asked);
    }

    /// An allocation ended: its port goes back to the driver.
    fn close(&mut self, port: u16) {
        if let Some(ip) = self.relays.remove(&port) {
            if let Some(s) = self.ips.get_mut(&ip) {
                s.allocations = s.allocations.saturating_sub(1);
            }
            self.released.push(port);
        }
    }

    /// Allocations the limits admitted, waiting for a port (refused ones never appear: TurnCore answers them).
    pub fn take_allocation_requests(&mut self) -> Vec<AllocationRequest> {
        std::mem::take(&mut self.requests)
    }

    /// The driver's answer: `Some(port)` bound, `None` no port free (the client gets 508).
    pub fn allocated(&mut self, req: AllocationRequest, port: Option<u16>, now: Instant) {
        let Some(asked) = self.asked.remove(&req.id) else { return };
        // A port already relaying is no answer (a driver's mistake).
        let port = port.filter(|p| !self.relays.contains_key(p) && *p != self.listen.port());
        let result = match port {
            Some(p) => {
                self.relays.insert(p, asked.client.ip());
                Ok(SocketAddr::new(self.public, p))
            }
            None => {
                if let Some(s) = self.ips.get_mut(&asked.client.ip()) {
                    s.allocations = s.allocations.saturating_sub(1);
                }
                Err(SocketAllocateError::InsufficientCapacity)
            }
        };
        let t = self.time(now);
        self.proto.allocated_socket(
            asked.transport,
            asked.listen,
            asked.client,
            asked.allocation_transport,
            asked.family,
            result,
            t,
        );
        self.drain(now);
    }

    /// Relay ports whose allocation ended since the last call (the driver closes them).
    pub fn released(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.released)
    }

    /// Allocations bound to a port.
    pub fn allocations(&self) -> usize {
        self.relays.len()
    }

    /// Datagrams over a limit.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Payload bytes relayed, both directions.
    pub fn relayed_bytes(&self) -> u64 {
        self.relayed_bytes
    }
}

/// A [`TurnServer`]'s setup.
#[derive(Clone, Debug)]
pub struct TurnServerConfig {
    /// Port 0: an ephemeral port (`local_addr` tells).
    pub listen: SocketAddr,
    pub public: IpAddr,
    /// Relay ports; `0..=0`: an OS-assigned ephemeral port per allocation (tests).
    pub ports: RangeInclusive<u16>,
    pub user: String,
    pub pass: String,
    pub limits: TurnLimits,
}

/// What a running relay did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TurnStats {
    pub allocations: usize,
    pub dropped: u64,
    pub relayed_bytes: u64,
}

/// A relay on real sockets: a thread with the listening socket and one
/// UDP socket per allocation, driving a [`TurnCore`]. Dropping it stops
/// the thread (within about a millisecond's sleep) and joins it.
pub struct TurnServer {
    local: SocketAddr,
    stop: Arc<AtomicBool>,
    stats: Arc<Mutex<TurnStats>>,
    thread: Option<JoinHandle<()>>,
}

/// At most this many datagrams are read from one socket per turn of the loop.
const READS_PER_TURN: usize = 256;
/// The loop sleeps this long when a turn found nothing to do.
const IDLE_SLEEP: Duration = Duration::from_millis(1);

/// A relay socket for an allocation: a free port of `ports` on `ip`, or
/// an OS-assigned one for `0..=0`.
fn bind_relay(
    ip: IpAddr,
    ports: &RangeInclusive<u16>,
    taken: &BTreeMap<u16, UdpSocket>,
) -> Option<(u16, UdpSocket)> {
    let socket = if *ports == (0..=0) {
        UdpSocket::bind((ip, 0)).ok()?
    } else {
        ports
            .clone()
            .filter(|p| *p != 0 && !taken.contains_key(p))
            .find_map(|p| UdpSocket::bind((ip, p)).ok())?
    };
    socket.set_nonblocking(true).ok()?;
    let port = socket.local_addr().ok()?.port();
    (!taken.contains_key(&port)).then_some((port, socket))
}

impl TurnServer {
    pub fn start(cfg: TurnServerConfig) -> io::Result<TurnServer> {
        let listen = UdpSocket::bind(cfg.listen)?;
        listen.set_nonblocking(true)?;
        let local = listen.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Mutex::new(TurnStats::default()));
        let core = TurnCore::new(local, cfg.public, &cfg.user, &cfg.pass, cfg.limits, Instant::now());
        let (thread_stop, thread_stats) = (stop.clone(), stats.clone());
        let thread = std::thread::Builder::new()
            .name("turn-server".into())
            .spawn(move || run(core, listen, cfg.ports, thread_stop, thread_stats))?;
        Ok(TurnServer { local, stop, stats, thread: Some(thread) })
    }

    /// The listening address (the bound port when `listen`'s was 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    pub fn stats(&self) -> TurnStats {
        self.stats.lock().map(|s| *s).unwrap_or_default()
    }
}

impl Drop for TurnServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The relay's thread: reads every socket, steps the core, binds and
/// closes relay sockets, sends.
fn run(
    mut core: TurnCore,
    listen: UdpSocket,
    ports: RangeInclusive<u16>,
    stop: Arc<AtomicBool>,
    stats: Arc<Mutex<TurnStats>>,
) {
    let listen_port = core.listen.port();
    let ip = match core.listen.ip() {
        ip if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        ip => ip,
    };
    let mut relays: BTreeMap<u16, UdpSocket> = BTreeMap::new();
    let mut buf = vec![0u8; 65_536];
    let mut out = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let mut busy = false;
        for _ in 0..READS_PER_TURN {
            let Ok((n, from)) = listen.recv_from(&mut buf) else { break };
            core.receive(from, &buf[..n], now);
            busy = true;
        }
        for (&port, socket) in &relays {
            for _ in 0..READS_PER_TURN {
                let Ok((n, from)) = socket.recv_from(&mut buf) else { break };
                core.receive_relayed(port, from, &buf[..n], now);
                busy = true;
            }
        }
        for req in core.take_allocation_requests() {
            let bound = bind_relay(ip, &ports, &relays);
            let port = bound.map(|(port, socket)| {
                relays.insert(port, socket);
                port
            });
            core.allocated(req, port, now);
        }
        core.poll(now, &mut out);
        for o in out.drain(..) {
            let socket = if o.from_port == listen_port { Some(&listen) } else { relays.get(&o.from_port) };
            if let Some(socket) = socket {
                let _ = socket.send_to(&o.bytes, o.to);
            }
            busy = true;
        }
        for port in core.released() {
            relays.remove(&port);
        }
        if let Ok(mut s) = stats.lock() {
            *s = TurnStats {
                allocations: core.allocations(),
                dropped: core.dropped(),
                relayed_bytes: core.relayed_bytes(),
            };
        }
        if !busy {
            std::thread::sleep(IDLE_SLEEP);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turn_client_proto::api::{TurnClientApi, TurnConfig, TurnEvent, TurnRecvRet};
    use turn_client_proto::types::TurnCredentials;
    use turn_client_proto::udp::TurnClientUdp;

    fn a(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn a_bucket_holds_two_seconds_and_refills() {
        let t0 = Instant::now();
        let mut b = Bucket::full(10, t0);
        assert!(b.take(10, 20, t0), "two seconds' worth at once");
        assert!(!b.take(10, 1, t0));
        assert!(!b.take(10, 6, t0 + Duration::from_millis(500)), "five refilled");
        assert!(b.take(10, 5, t0 + Duration::from_millis(500)));
        // Never above two seconds' worth.
        assert!(!b.take(10, 21, t0 + Duration::from_secs(100)));
        assert!(b.take(10, 20, t0 + Duration::from_secs(100)));
    }

    #[test]
    fn the_ip_table_is_bounded_and_keeps_allocating_ips() {
        let t0 = Instant::now();
        let mut core = TurnCore::new(
            a("198.51.100.9:3478"),
            a("198.51.100.9:0").ip(),
            "u",
            "p",
            TurnLimits::default(),
            t0,
        );
        core.ip(a("10.0.0.1:1").ip(), t0).allocations = 1;
        for i in 0..MAX_IPS as u32 + 10 {
            let ip = IpAddr::V4(Ipv4Addr::from(0x0b00_0000 + i));
            core.ip(ip, t0 + Duration::from_millis(u64::from(i) + 1));
        }
        assert_eq!(core.ips.len(), MAX_IPS);
        assert!(core.ips.contains_key(&a("10.0.0.1:1").ip()), "an IP holding an allocation stays");
        assert!(!core.ips.contains_key(&IpAddr::V4(Ipv4Addr::from(0x0b00_0000))), "the oldest idle one went");
    }

    /// A sans-IO client and a core on a wire without delay: the client
    /// allocates, the driver's port answers, and data relays both ways.
    #[test]
    fn a_client_allocates_in_memory_and_its_ip_is_counted() {
        let t0 = Instant::now();
        let server = a("198.51.100.9:3478");
        let (client_addr, peer) = (a("203.0.113.20:50000"), a("203.0.113.30:40000"));
        let mut core = TurnCore::new(server, server.ip(), "u", "p", TurnLimits::default(), t0);
        let mut client =
            TurnClientUdp::allocate(client_addr, server, TurnConfig::new(TurnCredentials::new("u", "p")));
        let mut relayed = None;
        let mut out = Vec::new();
        for ms in 0..2000u64 {
            let now = t0 + Duration::from_millis(ms);
            let t = ProtoInstant::from_nanos(i64::try_from(ms * 1_000_000).unwrap());
            let _ = client.poll(t);
            while let Some(tx) = client.poll_transmit(t) {
                core.receive(client_addr, tx.data.as_ref(), now);
            }
            for req in core.take_allocation_requests() {
                assert_eq!(req.client, client_addr);
                core.allocated(req, Some(49160), now);
            }
            core.poll(now, &mut out);
            for o in out.drain(..) {
                if o.to == client_addr {
                    let ret = client
                        .recv(Transmit::new(o.bytes.as_slice(), TransportType::Udp, server, client_addr), t);
                    if let TurnRecvRet::PeerData(d) = ret {
                        assert_eq!((d.peer, d.data()), (peer, &b"down"[..]));
                        relayed = relayed.map(|(r, _)| (r, true));
                    }
                } else {
                    assert_eq!((o.from_port, o.to, o.bytes.as_slice()), (49160, peer, &b"up"[..]));
                    core.receive_relayed(49160, peer, b"down", now);
                }
            }
            while let Some(e) = client.poll_event() {
                match e {
                    TurnEvent::AllocationCreated(_, addr) => {
                        relayed = Some((addr, false));
                        client.create_permission(TransportType::Udp, peer.ip(), t).unwrap();
                    }
                    TurnEvent::PermissionCreated(..) => {
                        let tx =
                            client.send_to(TransportType::Udp, peer, b"up".as_slice(), t).unwrap().unwrap();
                        let tx = tx.build();
                        core.receive(client_addr, &tx.data, now);
                    }
                    _ => {}
                }
            }
            if relayed.is_some_and(|(_, down)| down) {
                break;
            }
        }
        assert_eq!(relayed, Some((a("198.51.100.9:49160"), true)));
        assert_eq!((core.allocations(), core.ip(client_addr.ip(), t0).allocations), (1, 1));
        assert_eq!(core.relayed_bytes(), 6);
    }
}
