//! The endpoint's ICE on real 127.0.0.1 sockets (plan 12b, N48-N51,
//! PF12b-2): a signalled client and a host connect directly or through a
//! local `TurnServer`, STUN on the game socket learns the reflexive
//! address, a host maps its port on a `FakeGateway`, the signalled pending
//! table stays bounded, the TCP path is unchanged and gathering ends by its
//! deadline. Every endpoint has `base_override: 127.0.0.1`; nothing reaches
//! a real router or a public server: every datagram goes to 127.0.0.0/8
//! (test rule NT-1), so a fake router's external address is 127.0.0.2.
//! Every wait is bounded.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use rl_net::endpoint::{
    Endpoint, EndpointConfig, GATHER_DEADLINE, HOST, IceConfig, MAX_SIGNALLED_PENDING, NetEvent,
    TurnServerSpec,
};
use rl_net::link::{Link, LinkCandidates};
use rl_net::portmap::DROP_WAIT;
use rl_net::portmap::MappingState;
use rl_net::portmap::fake::{FakeGateway, Misbehaviour};
use rl_net::probe::NatBehaviour;
use rl_net::turn::{RelayPorts, TurnLimits, TurnServer, TurnServerConfig};
use rl_netcode::{Channel, PeerId};
use rl_proto::{FINGERPRINT, PROTOCOL_VERSION, Path, Signal};

const LOCALHOST: Ipv4Addr = Ipv4Addr::LOCALHOST;
const USER: &str = "relay-user";
const PASS: &str = "relay-pass";
/// The fake router's external address: a loopback alias (NT-1), not
/// global, so the mapping is usable only with `allow_local`, and not the
/// base (127.0.0.1), so it is a candidate of its own.
const EXTERNAL: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

/// Every endpoint's ICE: the base on 127.0.0.1, local relays allowed.
fn ice() -> IceConfig {
    IceConfig { base_override: Some(LOCALHOST), allow_local: true, ..IceConfig::default() }
}

fn config(ice: IceConfig) -> EndpointConfig {
    EndpointConfig { port: 0, ice, ..EndpointConfig::default() }
}

fn host(ice: IceConfig) -> Endpoint {
    Endpoint::host(config(ice), Box::new(|_, _| Ok(()))).expect("a host on port 0")
}

fn signalled(ice: IceConfig) -> Endpoint {
    Endpoint::signalled(config(ice)).expect("a signalled client on port 0")
}

/// A TURN server on 127.0.0.1 (port 0, OS-assigned relay ports, generous
/// limits, local peers allowed); it also answers STUN bindings.
fn turn_server() -> TurnServer {
    TurnServer::start(TurnServerConfig {
        listen: (LOCALHOST, 0).into(),
        public: LOCALHOST.into(),
        ports: RelayPorts::Ephemeral,
        user: USER.into(),
        pass: PASS.into(),
        limits: TurnLimits::generous(),
        allow_local: true,
    })
    .expect("a relay on 127.0.0.1")
}

fn turn_spec(server: &TurnServer) -> TurnServerSpec {
    TurnServerSpec { addr: server.local_addr().to_string(), user: USER.into(), pass: PASS.into() }
}

/// A 127.0.0.1 socket that never answers (held open, so no other test
/// takes its port).
fn silent() -> UdpSocket {
    UdpSocket::bind((LOCALHOST, 0)).expect("a silent socket")
}

/// An endpoint and what it reported.
struct Side {
    ep: Endpoint,
    events: Vec<NetEvent>,
}

impl Side {
    fn new(ep: Endpoint) -> Side {
        Side { ep, events: Vec::new() }
    }

    fn poll(&mut self) {
        self.ep.poll(Instant::now(), &mut self.events);
    }

    /// The first `Connected`: peer, address and path.
    fn connected(&self) -> Option<(PeerId, SocketAddr, Path)> {
        self.events.iter().find_map(|e| match e {
            NetEvent::Connected { peer, addr, path } => Some((*peer, *addr, *path)),
            _ => None,
        })
    }

    /// The reliable messages received, in order.
    fn reliable(&self) -> Vec<Vec<u8>> {
        self.events
            .iter()
            .filter_map(|e| match e {
                NetEvent::Message { channel: Channel::Reliable, bytes, .. } => Some(bytes.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Polls `sides` until `done` holds; panics after `limit` with `what`.
fn run_until(
    sides: &mut [&mut Side],
    limit: Duration,
    what: &str,
    mut done: impl FnMut(&[&mut Side]) -> bool,
) {
    let start = Instant::now();
    while !done(sides) {
        assert!(start.elapsed() < limit, "{what} within {limit:?}");
        for side in sides.iter_mut() {
            side.poll();
        }
        std::thread::sleep(Duration::from_micros(300));
    }
}

/// The client gathers, offers; the host answers; the client takes the
/// reply (the host's mapped hint with it).
fn signal(host: &mut Side, client: &mut Side) {
    run_until(&mut [&mut *host, &mut *client], GATHER_DEADLINE + Duration::from_secs(1), "gathering", |s| {
        s[1].ep.gathered()
    });
    let now = Instant::now();
    let offer = client.ep.offer(now).expect("an offer once gathered");
    assert!(matches!(offer, Signal::Offer { .. }), "{offer:?}");
    assert_eq!(client.ep.offer(now), Some(offer.clone()), "the same offer every time");
    let answer = host.ep.answer_signalled(&offer, None, now);
    assert!(matches!(answer, Signal::Answer { .. }), "{answer:?}");
    client.ep.take_reply(&answer, host.ep.mapped(), now);
}

/// Waits for both sides' `Connected`; returns the client's peer id at the
/// host.
fn wait_connected(host: &mut Side, client: &mut Side) -> PeerId {
    run_until(&mut [&mut *host, &mut *client], Duration::from_secs(10), "both sides connect", |s| {
        s[0].connected().is_some() && s[1].connected().is_some()
    });
    host.connected().unwrap().0
}

/// A reliable message each way.
fn exchange(host: &mut Side, client: &mut Side, peer: PeerId) {
    client.ep.send(HOST, Channel::Reliable, b"hello host");
    host.ep.send(peer, Channel::Reliable, b"hello client");
    run_until(&mut [&mut *host, &mut *client], Duration::from_secs(5), "a message each way", |s| {
        s[0].reliable().contains(&b"hello host".to_vec())
            && s[1].reliable().contains(&b"hello client".to_vec())
    });
}

#[test]
fn a_signalled_client_connects_to_a_host() {
    let mut host = Side::new(host(ice()));
    let mut client = Side::new(signalled(ice()));
    signal(&mut host, &mut client);
    let peer = wait_connected(&mut host, &mut client);

    let (_, host_addr, host_path) = host.connected().unwrap();
    let (client_peer, client_addr, client_path) = client.connected().unwrap();
    assert_eq!(client_peer, HOST);
    assert_eq!(host_path, Path::Direct);
    assert_eq!(client_path, Path::Direct);
    // `Connected`'s address is the selected pair's remote end.
    assert_eq!(host_addr, SocketAddr::from((LOCALHOST, client.ep.local_port())));
    assert_eq!(client_addr, SocketAddr::from((LOCALHOST, host.ep.local_port())));
    assert_eq!(host.ep.path(peer), Some(Path::Direct));
    assert_eq!(client.ep.path(HOST), Some(Path::Direct));
    exchange(&mut host, &mut client, peer);
}

#[test]
fn a_signalled_client_connects_through_a_local_turn_server() {
    let relay = turn_server();
    let mut host = Side::new(host(ice()));
    let mut client =
        Side::new(signalled(IceConfig { turn: vec![turn_spec(&relay)], relay_only: true, ..ice() }));
    signal(&mut host, &mut client);
    let peer = wait_connected(&mut host, &mut client);
    assert_eq!(client.connected().unwrap().2, Path::Relayed);
    assert_eq!(host.connected().unwrap().2, Path::Relayed);
    assert_eq!(client.ep.path(HOST), Some(Path::Relayed));
    assert!(
        matches!(client.ep.reach().relays.as_slice(), [(addr, rl_net::relayed::RelayState::Allocated(_))] if *addr == relay.local_addr()),
        "{:?}",
        client.ep.reach().relays
    );

    let messages: Vec<Vec<u8>> = (0..1000u32).map(|i| i.to_le_bytes().repeat(16)).collect();
    for m in &messages {
        client.ep.send(HOST, Channel::Reliable, m);
    }
    host.ep.send(peer, Channel::Reliable, b"hello client");
    run_until(
        &mut [&mut host, &mut client],
        Duration::from_secs(15),
        "1000 messages through the relay",
        |s| s[0].reliable().len() >= 1000 && !s[1].reliable().is_empty(),
    );
    assert_eq!(host.reliable(), messages, "every message, in order");
    assert_eq!(client.reliable(), vec![b"hello client".to_vec()]);
    assert!(relay.stats().relayed_bytes > 0, "{:?}", relay.stats());
}

#[test]
fn stun_on_the_game_socket_learns_the_reflexive_address() {
    let stun = turn_server();
    let mut side = Side::new(signalled(IceConfig { stun: vec![stun.local_addr().to_string()], ..ice() }));
    run_until(&mut [&mut side], Duration::from_secs(5), "a STUN answer", |s| s[0].ep.reach().srflx.is_some());
    let reach = side.ep.reach();
    let own = SocketAddr::from((LOCALHOST, side.ep.local_port()));
    assert_eq!(reach.srflx, Some(own));
    assert_eq!(reach.base, Some(own));
    assert_eq!(reach.nat, NatBehaviour::Open);
    assert_eq!(reach.mapping, MappingState::Off, "a client maps no port");
    assert!(reach.relays.is_empty());
    run_until(&mut [&mut side], Duration::from_secs(1), "gathered", |s| s[0].ep.gathered());

    // A host asks the same way.
    let mut host = Side::new(host(IceConfig { stun: vec![stun.local_addr().to_string()], ..ice() }));
    run_until(&mut [&mut host], Duration::from_secs(5), "a host's STUN answer", |s| {
        s[0].ep.reach().srflx.is_some()
    });
    assert_eq!(host.ep.reach().srflx, Some(SocketAddr::from((LOCALHOST, host.ep.local_port()))));
}

/// The fake's PCP/NAT-PMP address, for the `gateway` hook.
fn pmp(fake: &FakeGateway) -> SocketAddrV4 {
    match fake.pmp() {
        SocketAddr::V4(a) => a,
        other => panic!("the fake listens on IPv4: {other}"),
    }
}

/// A host mapping its port on `fake` (UPnP through the hooks), once mapped.
fn mapped_host(fake: &FakeGateway) -> Side {
    let mut host = Side::new(host(IceConfig {
        port_mapping: true,
        gateway: Some(pmp(fake)),
        ssdp: Some(fake.ssdp()),
        ..ice()
    }));
    run_until(&mut [&mut host], Duration::from_secs(6), "the mapping", |s| {
        matches!(s[0].ep.reach().mapping, MappingState::Mapped { .. })
    });
    host
}

#[test]
fn a_host_maps_its_port_on_a_fake_gateway() {
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::Honest);
    let host = mapped_host(&fake);
    let port = host.ep.local_port();
    match host.ep.reach().mapping {
        MappingState::Mapped { usable, tcp, external, .. } => {
            assert!(usable);
            assert!(tcp, "TCP and UDP on one port");
            assert_eq!(external, SocketAddrV4::new(EXTERNAL, port));
        }
        other => panic!("not mapped: {other:?}"),
    }
    assert_eq!(host.ep.mapped(), Some(SocketAddr::from((EXTERNAL, port))));
}

/// N49: a TCP join through the host's mapped port (the joiner dials the
/// external address) is `MappedPort` at the host, `Direct` at the joiner.
#[test]
fn a_tcp_join_through_the_mapping_is_mapped_port() {
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::Honest);
    let mut host = mapped_host(&fake);
    let external = host.ep.mapped().expect("a usable mapping");
    let mut client = Side::new(Endpoint::connect(config(ice()), external).unwrap());
    let peer = wait_connected(&mut host, &mut client);
    assert_eq!(client.connected().unwrap(), (HOST, external, Path::Direct));
    assert_eq!(host.connected().unwrap().2, Path::MappedPort);
    assert_eq!(host.ep.path(peer), Some(Path::MappedPort));
    exchange(&mut host, &mut client, peer);
}

/// Dropping a mapping host never blocks its caller, though the router
/// answers each deletion 250 ms late; the mapping still goes within
/// `DROP_WAIT`.
#[test]
fn a_host_drops_its_mapping_without_blocking() {
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::SlowDelete);
    let host = mapped_host(&fake);
    assert_eq!(fake.mappings().len(), 2);
    let dropping = Instant::now();
    drop(host);
    assert!(dropping.elapsed() < Duration::from_millis(200), "the drop blocked for {:?}", dropping.elapsed());
    while !fake.mappings().is_empty() && dropping.elapsed() < DROP_WAIT + Duration::from_millis(200) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fake.mappings(), vec![], "deleted within DROP_WAIT");
}

/// N51's exit: `shutdown` returns at once, and its `wait` returns once the
/// router deleted the mapping (bounded by its deadline).
#[test]
fn shutdown_deletes_the_mapping_before_wait_returns() {
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::SlowDelete);
    let host = mapped_host(&fake);
    assert_eq!(fake.mappings().len(), 2);
    let started = Instant::now();
    let mut shutdown = host.ep.shutdown();
    assert!(started.elapsed() < Duration::from_millis(200), "shutdown blocked for {:?}", started.elapsed());
    assert_eq!(shutdown.poll(), None, "the router is still asked: poll never waits");
    assert!(shutdown.wait(Instant::now() + DROP_WAIT + Duration::from_millis(200)), "deleted in time");
    assert_eq!(fake.mappings(), vec![]);
    assert!(started.elapsed() >= Duration::from_millis(500), "waited for both deletions");

    // An endpoint without a mapping shuts down at once.
    let plain = host_plain();
    let started = Instant::now();
    let mut shutdown = plain.shutdown();
    assert_eq!(shutdown.poll(), Some(true), "nothing to delete");
    assert!(shutdown.wait(Instant::now() + DROP_WAIT));
    assert!(started.elapsed() < Duration::from_millis(200));
}

fn host_plain() -> Endpoint {
    host(ice())
}

/// The SDP of an offerer `Link` that is never pumped: a joiner that never
/// sends a connectivity check.
fn silent_offer(at: SocketAddr) -> Signal {
    let (_link, sdp) =
        Link::offerer_with(&LinkCandidates { host: vec![at], ..LinkCandidates::default() }, Instant::now());
    Signal::Offer { protocol: PROTOCOL_VERSION, fingerprint: FINGERPRINT, sdp }
}

#[test]
fn the_signalled_pending_table_is_bounded() {
    let mut host = Side::new(host(ice()));
    let quiet = silent();
    let at = quiet.local_addr().unwrap();
    for _ in 0..MAX_SIGNALLED_PENDING {
        let answer = host.ep.answer_signalled(&silent_offer(at), None, Instant::now());
        assert!(matches!(answer, Signal::Answer { .. }), "{answer:?}");
    }
    // The ninth, a live joiner: a silent setup is evicted for it (PF12b-2).
    let mut client = Side::new(signalled(ice()));
    signal(&mut host, &mut client);
    wait_connected(&mut host, &mut client);
    assert_eq!(client.connected().unwrap().2, Path::Direct);
}

#[test]
fn the_direct_path_is_unchanged() {
    let mut host = Side::new(host(ice()));
    let addr = SocketAddr::from((LOCALHOST, host.ep.local_port()));
    let mut client = Side::new(Endpoint::connect(config(ice()), addr).unwrap());
    let peer = wait_connected(&mut host, &mut client);
    assert_eq!(client.connected().unwrap(), (HOST, addr, Path::Direct));
    assert_eq!(host.connected().unwrap().2, Path::Direct);
    assert_eq!(client.ep.path(HOST), Some(Path::Direct));
    assert_eq!(host.ep.path(peer), Some(Path::Direct));
    exchange(&mut host, &mut client, peer);
}

#[test]
fn gathering_ends_by_the_deadline() {
    let (stun, turn) = (silent(), silent());
    let mut side = Side::new(signalled(IceConfig {
        stun: vec![stun.local_addr().unwrap().to_string()],
        turn: vec![TurnServerSpec {
            addr: turn.local_addr().unwrap().to_string(),
            user: USER.into(),
            pass: PASS.into(),
        }],
        ..ice()
    }));
    // From the endpoint's making: binding its socket alone can take 100 ms
    // on a loaded machine.
    let start = Instant::now();
    run_until(&mut [&mut side], Duration::from_millis(500), "half a second", |_| {
        start.elapsed() >= Duration::from_millis(500)
    });
    assert!(!side.ep.gathered(), "nothing answered yet");
    assert_eq!(side.ep.offer(Instant::now()), None, "no offer before gathering ends");
    run_until(&mut [&mut side], GATHER_DEADLINE + Duration::from_millis(100), "gathered", |s| {
        s[0].ep.gathered()
    });
    assert!(start.elapsed() <= GATHER_DEADLINE + Duration::from_millis(100), "{:?}", start.elapsed());
    assert!(side.ep.offer(Instant::now()).is_some());
}
