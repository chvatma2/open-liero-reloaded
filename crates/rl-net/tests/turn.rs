//! TURN relays (N52): a `TurnServer` on 127.0.0.1 (port 0, OS-assigned
//! relay ports, generous limits unless a test is about the limits) driven
//! by plain UDP sockets through `turn_client_proto`, and the relay core
//! fuzzed.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::test_runner::Config;
use rl_net::turn::{TurnCore, TurnLimits, TurnServer, TurnServerConfig};
use turn_client_proto::api::{Transmit, TurnClientApi, TurnConfig, TurnEvent, TurnRecvRet};
use turn_client_proto::stun::Instant as TurnInstant;
use turn_client_proto::types::{TransportType, TurnCredentials};
use turn_client_proto::udp::TurnClientUdp;

const USER: &str = "relay-user";
const PASS: &str = "relay-pass";

fn local() -> IpAddr {
    "127.0.0.1".parse().unwrap()
}

fn server(limits: TurnLimits) -> TurnServer {
    TurnServer::start(TurnServerConfig {
        listen: (local(), 0).into(),
        public: local(),
        ports: 0..=0,
        user: USER.into(),
        pass: PASS.into(),
        limits,
    })
    .expect("a relay on 127.0.0.1")
}

/// A TURN client on its own UDP socket.
struct Client {
    socket: UdpSocket,
    local: SocketAddr,
    turn: TurnClientUdp,
    epoch: Instant,
    relayed: Option<SocketAddr>,
    failed: bool,
    permitted: Vec<IpAddr>,
    channels: Vec<SocketAddr>,
    received: Vec<(SocketAddr, Vec<u8>)>,
}

impl Client {
    fn new(server: SocketAddr) -> Client {
        let socket = UdpSocket::bind((local(), 0)).unwrap();
        socket.set_nonblocking(true).unwrap();
        let local = socket.local_addr().unwrap();
        let turn = TurnClientUdp::allocate(local, server, TurnConfig::new(TurnCredentials::new(USER, PASS)));
        Client {
            socket,
            local,
            turn,
            epoch: Instant::now(),
            relayed: None,
            failed: false,
            permitted: Vec::new(),
            channels: Vec::new(),
            received: Vec::new(),
        }
    }

    fn t(&self) -> TurnInstant {
        TurnInstant::from_nanos(self.epoch.elapsed().as_nanos() as i64)
    }

    /// One turn: the client's timers and requests, the socket's datagrams, its events.
    fn step(&mut self) {
        let t = self.t();
        let _ = self.turn.poll(t);
        while let Some(tx) = self.turn.poll_transmit(t) {
            let _ = self.socket.send_to(tx.data.as_ref(), tx.to);
        }
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = self.socket.recv_from(&mut buf) {
            let ret = self.turn.recv(Transmit::new(&buf[..n], TransportType::Udp, from, self.local), t);
            if let TurnRecvRet::PeerData(d) = ret {
                self.received.push((d.peer, d.data().to_vec()));
            }
        }
        while let Some(e) = self.turn.poll_event() {
            match e {
                TurnEvent::AllocationCreated(_, addr) => self.relayed = Some(addr),
                TurnEvent::AllocationCreateFailed(_) => self.failed = true,
                TurnEvent::PermissionCreated(_, ip) => self.permitted.push(ip),
                TurnEvent::ChannelCreated(_, peer) => self.channels.push(peer),
                _ => {}
            }
        }
    }

    /// Steps until `done` (true) or 5 s (false).
    fn until(&mut self, mut done: impl FnMut(&Client) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            self.step();
            if done(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    /// The relayed address, or `None` when the relay refused.
    fn allocate(&mut self) -> Option<SocketAddr> {
        assert!(self.until(|c| c.relayed.is_some() || c.failed), "the relay answered");
        self.relayed
    }

    fn permit(&mut self, ip: IpAddr) {
        self.turn.create_permission(TransportType::Udp, ip, self.t()).unwrap();
        assert!(self.until(|c| c.permitted.contains(&ip)), "permission for {ip}");
    }

    fn send(&mut self, to: SocketAddr, bytes: &[u8]) {
        let tx = self.turn.send_to(TransportType::Udp, to, bytes, self.t()).unwrap().unwrap().build();
        self.socket.send_to(&tx.data, tx.to).unwrap();
    }
}

/// A plain peer socket.
fn peer() -> UdpSocket {
    let socket = UdpSocket::bind((local(), 0)).unwrap();
    socket.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    socket
}

/// Waits for one datagram on `peer` (at most 2 s).
fn recv(peer: &UdpSocket) -> Option<(SocketAddr, Vec<u8>)> {
    let end = Instant::now() + Duration::from_secs(2);
    let mut buf = [0u8; 2048];
    while Instant::now() < end {
        if let Ok((n, from)) = peer.recv_from(&mut buf) {
            return Some((from, buf[..n].to_vec()));
        }
    }
    None
}

/// `client` relays `up` to `peer`, which answers `down` to the relayed address.
fn relays_both_ways(client: &mut Client, peer: &UdpSocket, up: &[u8], down: &[u8]) {
    let relayed = client.relayed.unwrap();
    let peer_addr = peer.local_addr().unwrap();
    client.send(peer_addr, up);
    assert_eq!(recv(peer), Some((relayed, up.to_vec())), "up through the relay");
    peer.send_to(down, relayed).unwrap();
    assert!(client.until(|c| c.received.contains(&(peer_addr, down.to_vec()))), "down through the relay");
}

#[test]
fn a_client_allocates_and_relays_both_ways() {
    let server = server(TurnLimits::generous());
    let mut client = Client::new(server.local_addr());
    let relayed = client.allocate().expect("allocated");
    assert_eq!(relayed.ip(), local());
    assert_ne!(relayed.port(), server.local_addr().port());
    let peer = peer();
    client.permit(peer.local_addr().unwrap().ip());
    relays_both_ways(&mut client, &peer, b"up", b"down");
    // Through a channel (4-byte ChannelData headers) too.
    let peer_addr = peer.local_addr().unwrap();
    client.turn.bind_channel(TransportType::Udp, peer_addr, client.t()).unwrap();
    assert!(client.until(|c| c.channels.contains(&peer_addr)), "channel bound");
    relays_both_ways(&mut client, &peer, b"up again", b"down again");
    assert_eq!(server.stats().allocations, 1);
    assert!(server.stats().relayed_bytes >= 24);
}

#[test]
fn turn_limits_hold_per_ip() {
    let server = server(TurnLimits::default());
    let mut clients: Vec<Client> = (0..5).map(|_| Client::new(server.local_addr())).collect();
    for (i, c) in clients.iter_mut().enumerate().take(4) {
        assert!(c.allocate().is_some(), "allocation {i} of 4");
    }
    assert_eq!(clients[4].allocate(), None, "a fifth allocation from one IP is refused");
    assert_eq!(server.stats().allocations, 4);

    // 200 KiB pushed within 1 s through one allocation.
    let peer = peer();
    let client = &mut clients[0];
    client.permit(peer.local_addr().unwrap().ip());
    let chunk = [7u8; 1024];
    let start = Instant::now();
    for i in 0..200u32 {
        let at = start + Duration::from_micros(2500) * i;
        if let Some(wait) = at.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        client.send(peer.local_addr().unwrap(), &chunk);
    }
    assert!(start.elapsed() < Duration::from_secs(1), "pushed within 1 s");
    let mut delivered = 0;
    let mut buf = [0u8; 2048];
    let end = Instant::now() + Duration::from_millis(500);
    while Instant::now() < end {
        if let Ok((n, _)) = peer.recv_from(&mut buf) {
            delivered += n;
        }
    }
    assert!(delivered > 0, "some got through");
    assert!(delivered <= 2 * 32 * 1024 + 32 * 1024, "{delivered} bytes delivered");
    assert!(server.stats().dropped > 0);
}

#[test]
fn a_full_relay_refuses_new_allocations() {
    let server = server(TurnLimits { capacity: 2, ..TurnLimits::default() });
    let (mut a, mut b, mut c) = (
        Client::new(server.local_addr()),
        Client::new(server.local_addr()),
        Client::new(server.local_addr()),
    );
    assert!(a.allocate().is_some() && b.allocate().is_some());
    assert_eq!(c.allocate(), None, "a third client is refused");
    let peer = peer();
    for (client, n) in [(&mut a, 1u8), (&mut b, 2)] {
        client.permit(peer.local_addr().unwrap().ip());
        relays_both_ways(client, &peer, &[n; 4], &[n + 10; 4]);
    }
}

#[test]
fn binding_requests_are_answered() {
    let server = server(TurnLimits::generous());
    let socket = peer();
    let transaction = [9u8; 12];
    socket.send_to(&rl_proto::stun::binding_request(transaction), server.local_addr()).unwrap();
    let (from, bytes) = recv(&socket).expect("an answer");
    assert_eq!(from, server.local_addr());
    assert_eq!(
        rl_proto::stun::parse_binding_response(&bytes, &transaction),
        Some(socket.local_addr().unwrap())
    );
}

/// A core at 198.51.100.9:3478 with one allocation (relay port 49160) for
/// a client at 203.0.113.20:50000, made in memory; the client's datagrams.
fn allocated_core() -> (TurnCore, Vec<Vec<u8>>) {
    let t0 = Instant::now();
    let server: SocketAddr = "198.51.100.9:3478".parse().unwrap();
    let client_addr: SocketAddr = "203.0.113.20:50000".parse().unwrap();
    let mut core = TurnCore::new(server, server.ip(), USER, PASS, TurnLimits::generous(), t0);
    let mut client =
        TurnClientUdp::allocate(client_addr, server, TurnConfig::new(TurnCredentials::new(USER, PASS)));
    let mut sent = Vec::new();
    let mut out = Vec::new();
    for ms in 0..500u64 {
        let now = t0 + Duration::from_millis(ms);
        let t = TurnInstant::from_nanos(ms as i64 * 1_000_000);
        let _ = client.poll(t);
        while let Some(tx) = client.poll_transmit(t) {
            sent.push(tx.data.as_ref().to_vec());
            core.receive(client_addr, tx.data.as_ref(), now);
        }
        for req in core.take_allocation_requests() {
            core.allocated(req, Some(49160), now);
        }
        core.poll(now, &mut out);
        for o in out.drain(..) {
            let _ =
                client.recv(Transmit::new(o.bytes.as_slice(), TransportType::Udp, server, client_addr), t);
        }
        while let Some(e) = client.poll_event() {
            if let TurnEvent::AllocationCreated(..) = e {
                let peer: SocketAddr = "203.0.113.30:40000".parse().unwrap();
                client.create_permission(TransportType::Udp, peer.ip(), t).unwrap();
                client.bind_channel(TransportType::Udp, peer, t).unwrap();
            }
        }
    }
    assert_eq!(core.allocations(), 1);
    (core, sent)
}

proptest! {
    #![proptest_config(Config { cases: 256, failure_persistence: None, ..Config::default() })]

    /// Garbage, and the client's own datagrams damaged, to the listening
    /// port and the relay port, from the client, a peer and strangers.
    #[test]
    fn arbitrary_datagrams_never_panic_the_server(
        inputs in proptest::collection::vec(
            (0u8..4, any::<prop::sample::Index>(), any::<prop::sample::Index>(), any::<u8>(), proptest::collection::vec(any::<u8>(), 0..300)),
            1..24,
        ),
    ) {
        let (mut core, sent) = allocated_core();
        let t0 = Instant::now();
        let client: SocketAddr = "203.0.113.20:50000".parse().unwrap();
        let peer: SocketAddr = "203.0.113.30:40000".parse().unwrap();
        let stranger: SocketAddr = "192.0.2.77:1234".parse().unwrap();
        let mut out = Vec::new();
        for (i, (kind, which, at, byte, junk)) in inputs.into_iter().enumerate() {
            let now = t0 + Duration::from_millis(600 + i as u64 * 7);
            // A real datagram with one byte changed and junk appended, or junk alone.
            let mut bytes = sent[which.index(sent.len())].clone();
            let at = at.index(bytes.len());
            bytes[at] = byte;
            match kind {
                0 => {
                    bytes.extend_from_slice(&junk);
                    core.receive(client, &bytes, now);
                }
                1 => core.receive(stranger, &junk, now),
                2 => core.receive_relayed(49160, peer, &junk, now),
                _ => core.receive_relayed(49161, stranger, &bytes, now),
            }
            core.poll(now, &mut out);
            for req in core.take_allocation_requests() {
                core.allocated(req, Some(49200 + (i as u16)), now);
            }
            core.released();
        }
        // Past every lifetime: the allocations end without a panic.
        core.poll(t0 + Duration::from_secs(4000), &mut out);
        let _ = core.released();
    }
}
