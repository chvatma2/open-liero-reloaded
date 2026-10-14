//! TURN relays (N52): a `TurnServer` on 127.0.0.1 (port 0, OS-assigned
//! relay ports, generous limits unless a test is about the limits) driven
//! by plain UDP sockets through `turn_client_proto`, and the relay core
//! fuzzed.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::test_runner::Config;
use rl_net::candidates::TurnSpec;
use rl_net::core::{Core, CoreConfig};
use rl_net::relayed::RelayState;
use rl_net::turn::{MAX_SESSION, RelayPorts, TurnCore, TurnLimits, TurnOut, TurnServer, TurnServerConfig};
use turn_client_proto::api::{Transmit, TurnClientApi, TurnConfig, TurnEvent, TurnRecvRet};
use turn_client_proto::stun::Instant as TurnInstant;
use turn_client_proto::types::prelude::DelayedTransmitBuild;
use turn_client_proto::types::transmit::DelayedMessage;
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
        ports: RelayPorts::Ephemeral,
        user: USER.into(),
        pass: PASS.into(),
        limits,
        allow_local: true,
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

#[test]
fn a_relay_listens_on_ipv4_only() {
    let cfg = TurnServerConfig {
        listen: "[::1]:0".parse().unwrap(),
        public: local(),
        ports: RelayPorts::Ephemeral,
        user: USER.into(),
        pass: PASS.into(),
        limits: TurnLimits::generous(),
        allow_local: true,
    };
    let err = TurnServer::start(cfg).err().expect("refused");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// A `TurnCore` and a sans-IO `turn_client_proto` client on an in-memory
/// wire, a 1 ms virtual clock; relay ports from 49160 up.
struct Wire {
    core: TurnCore,
    client: TurnClientUdp,
    server: SocketAddr,
    me: SocketAddr,
    t0: Instant,
    ms: u64,
    /// Virtual milliseconds per step.
    tick: u64,
    next_port: u16,
    events: Vec<TurnEvent>,
    /// The client's datagrams, and data relayed to the client and to peers.
    sent: Vec<Vec<u8>>,
    received: Vec<(SocketAddr, Vec<u8>)>,
    to_peers: Vec<TurnOut>,
}

impl Wire {
    fn new(server: &str, me: &str, limits: TurnLimits, allow_local: bool) -> Wire {
        let (server, me) = (a(server), a(me));
        let t0 = Instant::now();
        let mut core = TurnCore::new(server, server.ip(), USER, PASS, limits, t0);
        core.set_allow_local(allow_local);
        let client = TurnClientUdp::allocate(me, server, TurnConfig::new(TurnCredentials::new(USER, PASS)));
        Wire {
            core,
            client,
            server,
            me,
            t0,
            ms: 0,
            tick: 1,
            next_port: 49160,
            events: Vec::new(),
            sent: Vec::new(),
            received: Vec::new(),
            to_peers: Vec::new(),
        }
    }

    fn now(&self) -> Instant {
        self.t0 + Duration::from_millis(self.ms)
    }

    fn t(&self) -> TurnInstant {
        TurnInstant::from_nanos(self.ms as i64 * 1_000_000)
    }

    /// The client's datagram to the core now.
    fn deliver(&mut self, bytes: Vec<u8>) {
        let now = self.now();
        self.core.receive(self.me, &bytes, now);
        self.sent.push(bytes);
    }

    fn step(&mut self) {
        self.ms += self.tick;
        let (now, t) = (self.now(), self.t());
        let _ = self.client.poll(t);
        while let Some(tx) = self.client.poll_transmit(t) {
            self.deliver(tx.data.as_ref().to_vec());
        }
        for req in self.core.take_allocation_requests() {
            self.core.allocated(req, Some(self.next_port), now);
            self.next_port += 1;
        }
        let mut out = Vec::new();
        self.core.poll(now, &mut out);
        for o in out {
            if o.to == self.me {
                let ret = self
                    .client
                    .recv(Transmit::new(o.bytes.as_slice(), TransportType::Udp, self.server, self.me), t);
                if let TurnRecvRet::PeerData(d) = ret {
                    self.received.push((d.peer, d.data().to_vec()));
                }
            } else {
                self.to_peers.push(o);
            }
        }
        while let Some(e) = self.client.poll_event() {
            self.events.push(e);
        }
    }

    /// Steps until `done` (at most 5 virtual seconds).
    fn until(&mut self, mut done: impl FnMut(&Wire) -> bool) -> bool {
        for _ in 0..5000 {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    fn allocate(&mut self) -> SocketAddr {
        assert!(self.until(|w| w.events.iter().any(|e| matches!(e, TurnEvent::AllocationCreated(..)))));
        self.client.relayed_addresses().next().unwrap().1
    }

    /// Asks for a permission to `ip`: whether the relay granted it.
    fn permit(&mut self, ip: &str) -> bool {
        let ip: IpAddr = ip.parse().unwrap();
        self.client.create_permission(TransportType::Udp, ip, self.t()).unwrap();
        assert!(self.until(|w| w.events.iter().any(|e| matches!(e,
            TurnEvent::PermissionCreated(_, i) | TurnEvent::PermissionCreateFailed(_, i) if *i == ip))));
        self.events.iter().any(|e| matches!(e, TurnEvent::PermissionCreated(_, i) if *i == ip))
    }

    /// A Send indication to `peer`, built by hand (the client sends only to permitted peers).
    fn send_indication(&mut self, peer: SocketAddr, bytes: &[u8]) {
        let msg = DelayedMessage::for_server(peer, bytes).build();
        self.deliver(msg);
    }
}

/// Task 8 (the Task 3 review's carry): an allocation ends `max_session`
/// after it was made, however often its client refreshes it, so a few IPs
/// cannot hold a relay's capacity forever. The default is
/// `MAX_SESSION` (6 h). On a virtual clock, one step a second.
#[test]
fn an_allocation_ends_at_the_session_cap() {
    assert_eq!(TurnLimits::default().max_session, MAX_SESSION);
    assert_eq!(MAX_SESSION, Duration::from_secs(6 * 3600));
    assert_eq!(TurnLimits::generous().max_session, MAX_SESSION);
    let cap = Duration::from_secs(1800);
    let limits = TurnLimits { max_session: cap, ..TurnLimits::generous() };
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", limits, false);
    let relayed = w.allocate();
    let made = w.now();
    w.tick = 1000;
    // Refreshed past its 600 s lifetime, up to the cap.
    while w.now() + Duration::from_secs(2) < made + cap {
        w.step();
        assert_eq!(w.core.allocations(), 1, "alive at {:?}", w.now() - made);
    }
    assert!(w.core.released().is_empty());
    while w.now() <= made + cap {
        w.step();
    }
    assert_eq!(w.core.released(), vec![relayed.port()], "ended at the cap");
    assert_eq!(w.core.allocations(), 0);
}

#[test]
fn a_public_relay_refuses_local_peers() {
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", TurnLimits::generous(), false);
    w.allocate();
    for ip in ["127.0.0.1", "192.168.1.5", "169.254.1.1", "224.0.0.1", "10.1.2.3"] {
        assert!(!w.permit(ip), "{ip} refused");
    }
    assert!(w.permit("8.8.4.4"), "a global peer is permitted");
    // A channel to a local peer is refused too, and a Send indication to one dropped.
    let lan = a("192.168.1.5:9");
    w.client.bind_channel(TransportType::Udp, lan, w.t()).unwrap();
    assert!(
        w.until(|w| w.events.iter().any(|e| matches!(e, TurnEvent::ChannelCreateFailed(_, p) if *p == lan)))
    );
    w.send_indication(lan, b"to the LAN");
    w.send_indication(a("127.0.0.53:53"), b"to loopback");
    w.step();
    assert!(w.to_peers.is_empty(), "nothing reaches a local address: {:?}", w.to_peers);
}

/// Ruling TR-5 (amends TR-1; final review B-I3): a relay reaches a host
/// behind its own public IP (on its LAN, or beside it on the same machine:
/// the router hairpins), but never its own sockets there (its listening
/// port, its relay ports: a loop), nor at another of its own addresses
/// (a multi-homed relay, final review A-M3). In memory, on global
/// addresses, without `allow_local`: no datagram leaves the test.
#[test]
fn a_relay_reaches_hosts_at_its_public_ip_but_never_its_own_ports() {
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", TurnLimits::generous(), false);
    w.core.set_own_ips(vec!["11.0.0.10".parse().unwrap()]);
    let relayed = w.allocate();
    assert!(w.permit("11.0.0.9"), "a host behind the relay's own public IP");
    assert!(w.permit("11.0.0.10"), "the relay's other address");
    let host = a("11.0.0.9:23400");
    for own in [a("11.0.0.9:3478"), relayed, a("11.0.0.10:3478"), a(&format!("11.0.0.10:{}", relayed.port()))]
    {
        w.send_indication(own, b"to itself");
        w.client.bind_channel(TransportType::Udp, own, w.t()).unwrap();
        assert!(
            w.until(|w| w
                .events
                .iter()
                .any(|e| matches!(e, TurnEvent::ChannelCreateFailed(_, p) if *p == own))),
            "a channel to {own} refused"
        );
    }
    w.send_indication(host, b"to the host");
    w.send_indication(a("11.0.0.10:23400"), b"to the other address");
    w.step();
    let sent: Vec<_> = w.to_peers.iter().map(|o| (o.to, o.bytes.clone())).collect();
    assert_eq!(
        sent,
        [(host, b"to the host".to_vec()), (a("11.0.0.10:23400"), b"to the other address".to_vec())]
    );
    // The host's answer comes back; a datagram from the relay's own port is a loop, dropped.
    let now = w.now();
    w.core.receive_relayed(relayed.port(), a("11.0.0.9:3478"), b"a loop", now);
    w.core.receive_relayed(relayed.port(), host, b"from the host", now);
    w.step();
    assert_eq!(w.received, vec![(host, b"from the host".to_vec())]);
}

#[test]
fn a_stranger_cannot_drain_a_players_down_budget() {
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", TurnLimits::default(), false);
    let relayed = w.allocate();
    assert!(w.permit("8.8.4.4"));
    let now = w.now();
    for _ in 0..200 {
        w.core.receive_relayed(relayed.port(), a("9.9.9.9:1"), &[1u8; 1024], now);
    }
    w.core.receive_relayed(relayed.port(), a("8.8.4.4:5000"), b"from the peer", now);
    w.step();
    assert_eq!(w.received, vec![(a("8.8.4.4:5000"), b"from the peer".to_vec())]);
    assert_eq!(w.core.dropped(), 0, "a stranger's datagrams are not over a limit, just not relayed");
}

#[test]
fn an_allocation_holds_at_most_16_permissions() {
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", TurnLimits::generous(), false);
    w.allocate();
    for i in 1..=16 {
        assert!(w.permit(&format!("8.8.8.{i}")), "permission {i}");
    }
    assert!(!w.permit("8.8.8.17"), "a 17th is refused");
}

#[test]
fn spoofed_sources_and_oversized_requests_cost_nothing() {
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", TurnLimits::generous(), false);
    w.step();
    let initial = w.sent[0].clone();
    let now = w.now();
    let mut out = Vec::new();
    for i in 0..10_000u32 {
        let from = SocketAddr::new(IpAddr::V4((0x0b00_0000 + i).into()), 40000);
        w.core.receive(from, &initial, now);
        w.core.poll(now, &mut out);
    }
    let answered = out.len();
    assert!(answered > 0, "the first ones are answered (401)");
    assert!(w.core.tracked_ips() <= 4096, "{} IPs tracked", w.core.tracked_ips());
    assert_eq!(w.core.allocations(), 0);
    // A request over 1500 bytes is dropped unread.
    let mut big = initial.clone();
    let pad = 1600usize;
    big.extend_from_slice(&0x8022u16.to_be_bytes());
    big.extend_from_slice(&(pad as u16).to_be_bytes());
    big.resize(big.len() + pad, b'x');
    let len = (big.len() - 20) as u16;
    big[2..4].copy_from_slice(&len.to_be_bytes());
    let dropped = w.core.dropped();
    w.core.receive(a("11.0.0.21:1"), &big, now);
    w.core.poll(now, &mut out);
    assert_eq!(out.len(), answered, "no answer");
    assert_eq!(w.core.dropped(), dropped + 1);
}

/// A client `Core` at `base` allocating on a relay that claims `relayed_ip`
/// and hands out port `port`; the relay's state once settled.
fn relay_claiming(base: &str, relayed_ip: &str, port: u16) -> (RelayState, Vec<SocketAddr>) {
    let (base, server) = (a(base), a("11.0.0.9:3478"));
    let t0 = Instant::now();
    let mut turn = TurnCore::new(server, relayed_ip.parse().unwrap(), USER, PASS, TurnLimits::generous(), t0);
    turn.set_allow_local(true);
    let mut core = Core::new(CoreConfig::client(), t0);
    core.set_base(base, vec![base]);
    core.allocate(&[TurnSpec { addr: server, user: USER.into(), pass: PASS.into() }], t0);
    for ms in 0..3000u64 {
        let now = t0 + Duration::from_millis(ms);
        let mut out = Vec::new();
        core.poll(now, &mut out, &mut Vec::new());
        for d in out {
            turn.receive(base, &d.bytes, now);
        }
        for req in turn.take_allocation_requests() {
            turn.allocated(req, Some(port), now);
        }
        let mut outs = Vec::new();
        turn.poll(now, &mut outs);
        for o in outs {
            core.receive(server, &o.bytes, now);
        }
        if core.relays()[0].1 != RelayState::Allocating {
            break;
        }
    }
    (core.relays()[0].1.clone(), core.local().relayed)
}

#[test]
fn a_relay_naming_our_own_or_a_local_address_is_refused() {
    let (state, relayed) = relay_claiming("11.0.0.20:50000", "11.0.0.20", 50000);
    assert!(matches!(state, RelayState::Failed(_)), "our base as the relayed address: {state:?}");
    assert!(relayed.is_empty());
    let (state, _) = relay_claiming("11.0.0.20:50000", "10.0.0.9", 49160);
    assert!(matches!(state, RelayState::Failed(_)), "a private relayed address: {state:?}");
    let (state, relayed) = relay_claiming("11.0.0.20:50000", "11.0.0.9", 49160);
    assert_eq!(state, RelayState::Allocated(a("11.0.0.9:49160")), "an honest relay");
    assert_eq!(relayed, vec![a("11.0.0.9:49160")]);
}

/// A core with one allocation for the client at 203.0.113.20:50000 (local
/// addresses allowed), a permission and a channel to 203.0.113.30:40000,
/// data sent both ways; the client's datagrams.
fn allocated_core() -> (TurnCore, Vec<Vec<u8>>) {
    let mut w = Wire::new("198.51.100.9:3478", "203.0.113.20:50000", TurnLimits::generous(), true);
    w.allocate();
    let peer = a("203.0.113.30:40000");
    assert!(w.permit("203.0.113.30"));
    w.send_indication(peer, b"unbound");
    w.client.bind_channel(TransportType::Udp, peer, w.t()).unwrap();
    assert!(w.until(|w| w.events.iter().any(|e| matches!(e, TurnEvent::ChannelCreated(..)))));
    let tx = w.client.send_to(TransportType::Udp, peer, b"bound".as_slice(), w.t()).unwrap().unwrap().build();
    w.deliver(tx.data);
    w.step();
    assert_eq!(w.to_peers.len(), 2, "both relayed");
    assert_eq!(w.core.allocations(), 1);
    (w.core, w.sent)
}

/// Final review A, declined item 3, judged a finding: a TURN relay is
/// anyone's (announced on Nostr), so its answers are untrusted input to
/// a joiner's `Core` and turn-client-proto. A `Core` allocating on a relay
/// whose every answer is damaged (a byte changed, cut, junk appended) or
/// replaced by junk never panics.
/// How many answers reached the client.
fn hostile_relay(edits: &[(u8, prop::sample::Index, u8, Vec<u8>)]) -> usize {
    let (base, server) = (a("11.0.0.20:50000"), a("11.0.0.9:3478"));
    let t0 = Instant::now();
    let mut turn = TurnCore::new(server, server.ip(), USER, PASS, TurnLimits::generous(), t0);
    let mut core = Core::new(CoreConfig::client(), t0);
    core.set_base(base, vec![base]);
    core.allocate(&[TurnSpec { addr: server, user: USER.into(), pass: PASS.into() }], t0);
    let mut edits = edits.iter().cycle();
    let mut answers = 0;
    for ms in (0..3000u64).step_by(5) {
        let now = t0 + Duration::from_millis(ms);
        let mut out = Vec::new();
        core.poll(now, &mut out, &mut Vec::new());
        for d in out {
            turn.receive(base, &d.bytes, now);
        }
        for req in turn.take_allocation_requests() {
            turn.allocated(req, Some(49160), now);
        }
        let mut outs = Vec::new();
        turn.poll(now, &mut outs);
        for o in outs {
            let mut bytes = o.bytes;
            if let Some((kind, at, byte, junk)) = edits.next() {
                match kind % 4 {
                    0 => {
                        let i = at.index(bytes.len().max(1)).min(bytes.len().saturating_sub(1));
                        if let Some(b) = bytes.get_mut(i) {
                            *b = *byte;
                        }
                    }
                    1 => bytes.truncate(at.index(bytes.len() + 1)),
                    2 => bytes.extend_from_slice(junk),
                    _ => bytes = junk.clone(),
                }
            }
            core.receive(server, &bytes, now);
            answers += 1;
        }
    }
    let _ = core.relays();
    answers
}

proptest! {
    #![proptest_config(Config { cases: 256, failure_persistence: None, ..Config::default() })]

    #[test]
    fn a_hostile_relays_answers_never_panic_the_client(
        edits in proptest::collection::vec(
            (any::<u8>(), any::<prop::sample::Index>(), any::<u8>(), proptest::collection::vec(any::<u8>(), 0..200)),
            1..8,
        ),
    ) {
        prop_assert!(hostile_relay(&edits) >= 1, "the relay answered");
    }

    /// Garbage, and the client's own datagrams (requests, a Send
    /// indication, ChannelData) damaged, to the listening port and the
    /// relay port, from the client, a peer and strangers.
    #[test]
    fn arbitrary_datagrams_never_panic_the_server(
        inputs in proptest::collection::vec(
            (0u8..4, any::<prop::sample::Index>(), any::<prop::sample::Index>(), any::<u8>(), proptest::collection::vec(any::<u8>(), 0..300)),
            1..24,
        ),
    ) {
        let (mut core, sent) = allocated_core();
        let t0 = Instant::now();
        let client = a("203.0.113.20:50000");
        let peer = a("203.0.113.30:40000");
        let stranger = a("192.0.2.77:1234");
        let mut out = Vec::new();
        for (i, (kind, which, at, byte, junk)) in inputs.into_iter().enumerate() {
            let now = t0 + Duration::from_millis(6000 + i as u64 * 7);
            // A real datagram with one byte changed (and junk appended), or junk alone.
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
                _ => core.receive(client, &bytes, now),
            }
            core.poll(now, &mut out);
            for req in core.take_allocation_requests() {
                core.allocated(req, Some(49200 + (i as u16)), now);
            }
            core.released();
        }
        // Past every lifetime: the allocations end without a panic.
        core.poll(t0 + Duration::from_secs(4000), &mut out);
        prop_assert_eq!(core.allocations(), 0);
        let _ = core.released();
    }
}

/// A request of `method` (header only, or with `attrs`), not signed.
fn raw_request(method: u16, attrs: &[u8]) -> Vec<u8> {
    let mut v = method.to_be_bytes().to_vec();
    v.extend_from_slice(&(attrs.len() as u16).to_be_bytes());
    v.extend_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
    v.extend_from_slice(&[5; 12]);
    v.extend_from_slice(attrs);
    v
}

/// REQUESTED-TRANSPORT UDP.
const UDP: [u8; 8] = [0x00, 0x19, 0x00, 0x04, 17, 0, 0, 0];

/// The answers `core` sends for `request` from `from` now.
fn answers(core: &mut TurnCore, from: SocketAddr, request: &[u8], now: Instant) -> Vec<TurnOut> {
    core.receive(from, request, now);
    let mut out = Vec::new();
    core.poll(now, &mut out);
    out
}

#[test]
fn only_an_allocate_gets_an_unauthenticated_answer() {
    let t0 = Instant::now();
    let mut core =
        TurnCore::new(a("11.0.0.9:3478"), "11.0.0.9".parse().unwrap(), USER, PASS, TurnLimits::default(), t0);
    let from = a("11.0.0.20:50000");
    for method in [0x0004, 0x0008, 0x0009] {
        assert!(answers(&mut core, from, &raw_request(method, &[]), t0).is_empty(), "method {method:#x}");
    }
    assert!(
        answers(&mut core, from, &raw_request(0x0003, &[]), t0).is_empty(),
        "an Allocate without a transport"
    );
    assert_eq!(answers(&mut core, from, &raw_request(0x0003, &UDP), t0).len(), 1, "a 401");
}

#[test]
fn unauthenticated_answers_amplify_little() {
    let t0 = Instant::now();
    let mut core =
        TurnCore::new(a("11.0.0.9:3478"), "11.0.0.9".parse().unwrap(), USER, PASS, TurnLimits::default(), t0);
    let from = a("11.0.0.20:50000");
    // The smallest requests that get an answer, and turn-client-proto's first Allocate.
    let binding = rl_proto::stun::binding_request([1; 12]);
    let allocate = raw_request(0x0003, &UDP);
    let mut client = Wire::new("11.0.0.9:3478", "11.0.0.21:50000", TurnLimits::default(), false);
    client.step();
    let first = client.sent[0].clone();
    for (request, most) in [(&binding, 1.6), (&allocate, 2.3), (&first, 1.6)] {
        let out = answers(&mut core, from, request, t0);
        assert_eq!(out.len(), 1);
        let factor = out[0].bytes.len() as f64 / request.len() as f64;
        assert!(factor <= most, "{} B answered with {} B ({factor:.2}x)", request.len(), out[0].bytes.len());
    }
}

#[test]
fn the_relay_caps_unauthenticated_answers_overall() {
    let t0 = Instant::now();
    let mut core =
        TurnCore::new(a("11.0.0.9:3478"), "11.0.0.9".parse().unwrap(), USER, PASS, TurnLimits::default(), t0);
    let binding = rl_proto::stun::binding_request([1; 12]);
    let allocate = raw_request(0x0003, &UDP);
    let mut answered = 0;
    for i in 0..10_000u32 {
        let from = SocketAddr::new(IpAddr::V4((0x0b00_0000 + i).into()), 40000);
        let request = if i % 2 == 0 { &binding } else { &allocate };
        answered += answers(&mut core, from, request, t0).iter().map(|o| o.bytes.len()).sum::<usize>();
    }
    assert!(answered <= 2 * 64 * 1024, "{answered} B of answers from many sources at once");
    assert!(answered > 64 * 1024, "the burst is answered");
}

#[test]
fn spoofed_requests_cannot_starve_a_clients_signed_requests() {
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", TurnLimits::default(), false);
    w.allocate();
    // A spoofer floods unauthenticated Allocates "from" the client's IP,
    // 5 a millisecond, while the client asks for a permission.
    let ip: IpAddr = "8.8.4.4".parse().unwrap();
    let mut port = 1000u16;
    let now = w.now();
    for _ in 0..100 {
        port += 1;
        w.core.receive(SocketAddr::new(w.me.ip(), port), &raw_request(0x0003, &UDP), now);
    }
    w.client.create_permission(TransportType::Udp, ip, w.t()).unwrap();
    for _ in 0..3000 {
        for _ in 0..5 {
            port = port.wrapping_add(1);
            // At the instant of the coming step, so the spoofer is first.
            let now = w.now() + Duration::from_millis(1);
            w.core.receive(SocketAddr::new(w.me.ip(), port), &raw_request(0x0003, &UDP), now);
        }
        w.step();
        if w.events.iter().any(|e| matches!(e, TurnEvent::PermissionCreated(..))) {
            return;
        }
    }
    panic!("the client's signed request never passed");
}

/// The driver's readers drop datagrams over 4 KiB before queueing them
/// (not testable over this machine's loopback, which drops UDP datagrams
/// over 1472 B); the core drops and counts them too.
#[test]
fn oversized_datagrams_are_dropped_and_counted() {
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", TurnLimits::generous(), false);
    let relayed = w.allocate();
    assert!(w.permit("8.8.4.4"));
    let now = w.now();
    let dropped = w.core.dropped();
    // A Send indication's type, so not a request.
    let mut big = vec![0u8; 5000];
    big[1] = 0x16;
    w.core.receive(w.me, &big, now);
    w.core.receive_relayed(relayed.port(), a("8.8.4.4:5000"), &[1u8; 5000], now);
    w.step();
    assert_eq!(w.core.dropped(), dropped + 2);
    assert!(w.received.is_empty() && w.to_peers.is_empty());
}

/// N6/N7: past nonce expiry (fresh for 10-20 minutes), a live client's
/// signed renewals carry an aged but genuine nonce: they are proven, so a
/// spoofer draining the client IP's unproven bucket and the relay-wide
/// unproven budget cannot starve them. 32 virtual minutes, past the
/// allocation's refresh (at 29 of its 30).
#[test]
fn a_live_client_survives_a_spoofer_past_nonce_expiry() {
    let mut w = Wire::new("11.0.0.9:3478", "11.0.0.20:50000", TurnLimits::default(), false);
    let relayed = w.allocate();
    let peer = a("8.8.4.4:5000");
    w.client.bind_channel(TransportType::Udp, peer, w.t()).unwrap();
    assert!(w.until(|w| w.events.iter().any(|e| matches!(e, TurnEvent::ChannelCreated(..)))));
    w.tick = 10;
    let allocate = raw_request(0x0003, &UDP);
    let mut spoof = 0u32;
    while w.ms < 32 * 60 * 1000 {
        // At the coming step's instant, so the spoofer comes first: one
        // forged Allocate from the client's IP, and sixteen from strangers
        // (above the relay-wide budget's ~10 answers per 10 ms).
        let now = w.now() + Duration::from_millis(w.tick);
        spoof = spoof.wrapping_add(1);
        w.core.receive(SocketAddr::new(w.me.ip(), 1024 + (spoof % 60_000) as u16), &allocate, now);
        for _ in 0..16 {
            spoof = spoof.wrapping_add(1);
            let from = SocketAddr::new(IpAddr::V4((0x0c00_0000 + spoof % 100_000).into()), 40000);
            w.core.receive(from, &allocate, now);
        }
        w.step();
    }
    assert!(w.core.dropped() > 100_000, "the spoofer was busy: {}", w.core.dropped());
    assert_eq!(w.core.allocations(), 1, "the allocation lives");
    assert_eq!(w.client.relayed_addresses().count(), 1, "the client kept it");
    // Its permission and channel still relay both ways.
    let now = w.now();
    w.core.receive_relayed(relayed.port(), peer, b"still here", now);
    let tx =
        w.client.send_to(TransportType::Udp, peer, b"me too".as_slice(), w.t()).unwrap().unwrap().build();
    w.deliver(tx.data);
    w.step();
    assert!(w.received.contains(&(peer, b"still here".to_vec())), "down");
    assert!(w.to_peers.iter().any(|o| o.to == peer && o.bytes == b"me too"), "up");
}
