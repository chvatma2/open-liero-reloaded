//! Port mapping (N51) against `FakeGateway`s on 127.0.0.1: UPnP-IGD, PCP and
//! NAT-PMP, a router that lies, stalls or forgets (review focus 2), and
//! hostile answers. No test sends to 239.255.255.250 or to a real gateway:
//! every request names a fake's SSDP and PCP/NAT-PMP addresses, or a silent
//! 127.0.0.1 socket.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::Config;
use rl_net::portmap::fake::{FakeGateway, Misbehaviour};
use rl_net::portmap::pmp::{parse_pcp, parse_pmp};
use rl_net::portmap::{MapRequest, MappingState, Method, PortMapper};

const LOCALHOST: Ipv4Addr = Ipv4Addr::LOCALHOST;
const EXTERNAL: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 7);

/// A 127.0.0.1 UDP socket that never answers: an SSDP target where no UPnP
/// device listens. Held open, so no other test's socket can take its port.
fn silent() -> UdpSocket {
    UdpSocket::bind((LOCALHOST, 0)).unwrap()
}

/// A request aimed at `fake` (its SSDP and PCP/NAT-PMP sockets), the game's
/// port `port` on 127.0.0.1, the gateway 127.0.0.1.
fn request(fake: &FakeGateway, port: u16, tcp: bool) -> MapRequest {
    let mut req = MapRequest::new(SocketAddrV4::new(LOCALHOST, port), tcp, Some(LOCALHOST));
    req.ssdp = fake.ssdp();
    req.pmp_port = fake.pmp().port();
    req.allow_local = true;
    req
}

/// Polls until the state leaves `Trying` (or `within` passes), checking that
/// no `poll` blocks: each returns within 5 ms.
fn settle(mapper: &mut PortMapper, within: Duration) -> MappingState {
    let end = Instant::now() + within;
    while Instant::now() < end {
        let t = Instant::now();
        let changed = mapper.poll();
        assert!(t.elapsed() < Duration::from_millis(5), "poll blocked for {:?}", t.elapsed());
        if let Some(state) = changed
            && state != MappingState::Trying
        {
            return state;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    mapper.state().clone()
}

fn mapped(state: &MappingState) -> (Method, SocketAddrV4, bool, bool) {
    match state {
        MappingState::Mapped { method, external, tcp, usable } => (*method, *external, *tcp, *usable),
        other => panic!("not mapped: {other:?}"),
    }
}

fn failed(state: &MappingState) -> &str {
    match state {
        MappingState::Failed(why) => why,
        other => panic!("not failed: {other:?}"),
    }
}

#[test]
fn upnp_maps_tcp_and_udp_on_one_port() {
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::Honest);
    let mut req = request(&fake, 23410, true);
    req.gateway = None;
    let mut mapper = PortMapper::start(req);
    let state = settle(&mut mapper, Duration::from_secs(5));
    assert_eq!(
        state,
        MappingState::Mapped {
            method: Method::Upnp,
            external: SocketAddrV4::new(EXTERNAL, 23410),
            tcp: true,
            usable: true
        }
    );
    let internal = SocketAddrV4::new(LOCALHOST, 23410);
    let maps = fake.mappings();
    assert!(maps.contains(&(Method::Upnp, true, 23410, internal)), "{maps:?}");
    assert!(maps.contains(&(Method::Upnp, false, 23410, internal)), "{maps:?}");
    assert_eq!(maps.len(), 2, "{maps:?}");
}

#[test]
fn pcp_maps_first_and_nat_pmp_is_the_fallback() {
    let ssdp = silent();
    for (pcp, method) in [(true, Method::Pcp), (false, Method::NatPmp)] {
        let fake = FakeGateway::start(false, !pcp, pcp, EXTERNAL, Misbehaviour::Honest);
        let mut req = request(&fake, 23420, true);
        req.ssdp = ssdp.local_addr().unwrap();
        let mut mapper = PortMapper::start(req);
        let state = settle(&mut mapper, Duration::from_secs(5));
        assert_eq!(mapped(&state), (method, SocketAddrV4::new(EXTERNAL, 23420), true, true));
        let internal = SocketAddrV4::new(LOCALHOST, 23420);
        assert_eq!(fake.mappings(), vec![(method, false, 23420, internal), (method, true, 23420, internal)]);
    }
}

/// Review focus 2: a router that lies, stalls or forgets.
mod fake_gateway_misbehaviours_are_reported {
    use super::*;

    #[test]
    fn http_silent() {
        let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::HttpSilent);
        let mut req = request(&fake, 23430, true);
        req.gateway = None;
        let start = Instant::now();
        let mut mapper = PortMapper::start(req);
        let state = settle(&mut mapper, Duration::from_secs(5));
        assert!(failed(&state).contains("UPnP"), "{state:?}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn other_udp_port() {
        let ssdp = silent();
        let fake = FakeGateway::start(false, true, false, EXTERNAL, Misbehaviour::OtherUdpPort);
        let mut req = request(&fake, 23440, true);
        req.ssdp = ssdp.local_addr().unwrap();
        let mut mapper = PortMapper::start(req);
        let (method, external, tcp, _) = mapped(&settle(&mut mapper, Duration::from_secs(5)));
        assert_eq!((method, tcp), (Method::NatPmp, true));
        let maps = fake.mappings();
        assert_eq!(maps.len(), 2, "{maps:?}");
        assert!(maps.iter().all(|m| m.2 == external.port()), "{maps:?} for {external}");
    }

    #[test]
    fn pcp_refuses() {
        let ssdp = silent();
        let fake = FakeGateway::start(false, true, true, EXTERNAL, Misbehaviour::PcpRefuses);
        let mut req = request(&fake, 23450, false);
        req.ssdp = ssdp.local_addr().unwrap();
        let mut mapper = PortMapper::start(req);
        let state = settle(&mut mapper, Duration::from_secs(5));
        assert_eq!(mapped(&state), (Method::NatPmp, SocketAddrV4::new(EXTERNAL, 23450), false, true));
    }

    #[test]
    fn private_external() {
        let private = Ipv4Addr::new(10, 0, 0, 1);
        let fake = FakeGateway::start(true, true, true, private, Misbehaviour::PrivateExternal);
        let mut req = request(&fake, 23460, true);
        req.allow_local = false;
        let mut mapper = PortMapper::start(req);
        let (_, external, _, usable) = mapped(&settle(&mut mapper, Duration::from_secs(5)));
        assert_eq!((*external.ip(), usable), (private, false));
    }

    #[test]
    fn renew_refused() {
        let ssdp = silent();
        for (upnp, pcp, port) in [(true, false, 23470), (false, true, 23471)] {
            let fake = FakeGateway::start(upnp, false, pcp, EXTERNAL, Misbehaviour::RenewRefused);
            let mut req = request(&fake, port, true);
            req.lifetime = Some(Duration::from_secs(2));
            if upnp {
                req.gateway = None;
            } else {
                req.ssdp = ssdp.local_addr().unwrap();
            }
            let mut mapper = PortMapper::start(req);
            mapped(&settle(&mut mapper, Duration::from_secs(5)));
            let state = settle(&mut mapper, Duration::from_secs(4));
            assert!(failed(&state).starts_with("the router dropped the mapping"), "{state:?}");
        }
    }

    #[test]
    fn garbage() {
        let fake = FakeGateway::start(true, true, true, EXTERNAL, Misbehaviour::Garbage);
        let mut mapper = PortMapper::start(request(&fake, 23480, true));
        let state = settle(&mut mapper, Duration::from_secs(6));
        failed(&state);
    }
}

#[test]
fn an_exact_request_never_shifts() {
    let fake = FakeGateway::start(true, true, true, EXTERNAL, Misbehaviour::Honest);
    fake.hold(23490);
    let mut req = request(&fake, 23490, false);
    req.exact = true;
    let mut mapper = PortMapper::start(req);
    let state = settle(&mut mapper, Duration::from_secs(6));
    let why = failed(&state);
    for method in ["PCP", "NAT-PMP", "UPnP"] {
        assert!(why.contains(method), "{why}");
    }
    assert_eq!(fake.mappings(), vec![]);

    // Without `exact`, the next port is taken.
    let mut mapper = PortMapper::start(request(&fake, 23490, true));
    let (_, external, tcp, _) = mapped(&settle(&mut mapper, Duration::from_secs(5)));
    assert_eq!((external.port(), tcp), (23491, true));
}

#[test]
fn mappings_are_deleted_on_drop() {
    let ssdp = silent();
    for (upnp, pmp, pcp, port) in
        [(true, false, false, 23500), (false, true, false, 23501), (false, false, true, 23502)]
    {
        let fake = FakeGateway::start(upnp, pmp, pcp, EXTERNAL, Misbehaviour::Honest);
        let mut req = request(&fake, port, true);
        if upnp {
            req.gateway = None;
        } else {
            req.ssdp = ssdp.local_addr().unwrap();
        }
        let mut mapper = PortMapper::start(req);
        mapped(&settle(&mut mapper, Duration::from_secs(5)));
        assert_eq!(fake.mappings().len(), 2);
        let dropped = Instant::now();
        drop(mapper);
        assert!(dropped.elapsed() <= Duration::from_millis(1100), "drop took {:?}", dropped.elapsed());
        while !fake.mappings().is_empty() && dropped.elapsed() < Duration::from_millis(1500) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fake.mappings(), vec![], "upnp {upnp} pmp {pmp} pcp {pcp}");
    }
}

#[test]
fn no_gateway_and_no_upnp_fails_quickly() {
    let ssdp = silent();
    let mut req = MapRequest::new(SocketAddrV4::new(LOCALHOST, 23510), true, None);
    req.ssdp = ssdp.local_addr().unwrap();
    let start = Instant::now();
    let mut mapper = PortMapper::start(req);
    let state = settle(&mut mapper, Duration::from_secs(5));
    let why = failed(&state);
    for method in ["PCP", "NAT-PMP", "UPnP"] {
        assert!(why.contains(method), "{why}");
    }
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_test_build_never_reaches_a_real_router() {
    // The defaults aim at the real SSDP group and a LAN gateway: a build with
    // `test-hooks` refuses them before any datagram leaves.
    let req = MapRequest::new(SocketAddrV4::new(LOCALHOST, 23520), true, Some(Ipv4Addr::new(192, 168, 1, 1)));
    let mut mapper = PortMapper::start(req);
    let state = settle(&mut mapper, Duration::from_millis(100));
    assert!(failed(&state).contains("FakeGateway"), "{state:?}");
    let req = MapRequest::new(SocketAddrV4::new(LOCALHOST, 23521), true, None);
    assert!(matches!(PortMapper::start(req).state(), MappingState::Failed(_)));
}

fn config() -> Config {
    Config { cases: 256, failure_persistence: None, ..Config::default() }
}

/// A NAT-PMP or PCP answer: a real one damaged, or junk.
fn pmp_answer() -> impl Strategy<Value = Vec<u8>> {
    let pmp_map = vec![0u8, 129, 0, 0, 0, 0, 0, 9, 0x5b, 0x68, 0x5b, 0x69, 0, 0, 0x1c, 0x20];
    let mut pcp = vec![2u8, 0x81, 0, 0, 0, 0, 0x1c, 0x20, 0, 0, 0, 9];
    pcp.extend([0u8; 12]);
    pcp.extend([7u8; 12]);
    pcp.extend([17, 0, 0, 0, 0x5b, 0x68, 0x5b, 0x68]);
    pcp.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 203, 0, 113, 7]);
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..1200),
        (prop_oneof![Just(pmp_map), Just(pcp)], any::<Index>(), any::<u8>(), any::<Index>()).prop_map(
            |(mut b, at, byte, cut)| {
                let i = at.index(b.len());
                b[i] = byte;
                b.truncate(cut.index(b.len() + 1));
                b
            }
        ),
    ]
}

/// What a hostile UPnP device answers to SSDP and HTTP alike: junk, or an
/// SSDP/HTTP head naming the fake's HTTP server (`$HTTP`) over junk or a
/// truncated device description (never a whole valid one).
fn upnp_answer() -> impl Strategy<Value = Vec<u8>> {
    const DOC: &str = "<?xml version=\"1.0\"?><root><device><serviceList><service>\
        <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
        <SCPDURL>/scpd.xml</SCPDURL><controlURL>/ctl</controlURL></service></serviceList></device>\
        <actionList><action><name>AddPortMapping</name><argumentList><argument><name>NewExternalPort</name>\
        <direction>in</direction></argument></argumentList></action></actionList>\
        <s:Body><u:GetExternalIPAddressResponse><NewExternalIPAddress>203.0.113.7</NewExternalIPAddress>\
        </u:GetExternalIPAddressResponse><u:AddPortMappingResponse/></s:Body></root>";
    let location = prop_oneof![
        Just("http://$HTTP/desc.xml".to_string()),
        Just("http://localhost/desc.xml".to_string()),
        Just("http://[::1]/x".to_string()),
        Just("http://$HTTP".to_string()),
        "[ -~]{0,40}",
    ];
    let body = prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..400),
        any::<Index>().prop_map(|cut| DOC.as_bytes()[..cut.index(DOC.len())].to_vec()),
    ];
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..600),
        (location, body, any::<bool>(), 0usize..2000).prop_map(|(location, body, honest, length)| {
            let length = if honest { body.len() } else { length };
            let mut answer =
                format!("HTTP/1.1 200 OK\r\nLOCATION: {location}\r\nContent-Length: {length}\r\n\r\n")
                    .into_bytes();
            answer.extend(body);
            answer
        }),
    ]
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn pmp_and_pcp_responses_never_panic(bytes in pmp_answer(), nonce in any::<[u8; 12]>()) {
        let _ = parse_pmp(&bytes);
        let _ = parse_pcp(&bytes, &nonce);
        let _ = parse_pcp(&bytes, &[7u8; 12]);
    }

    #[test]
    fn upnp_answers_never_panic(answer in upnp_answer()) {
        let fake = FakeGateway::serving(answer);
        let mut req = request(&fake, 23530, true);
        req.gateway = None;
        let mut mapper = PortMapper::start(req);
        let state = settle(&mut mapper, Duration::from_secs(6));
        prop_assert!(matches!(state, MappingState::Failed(_)), "{:?}", state);
    }
}
