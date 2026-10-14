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
use rl_net::portmap::fake::{Description, FakeGateway, Misbehaviour};
use rl_net::portmap::pmp::{parse_pcp, parse_pmp};
use rl_net::portmap::upnp;
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
/// no `poll` blocks: each returns within 50 ms (a loaded machine may preempt
/// a thread for a few ms; `settle_strictly` holds `poll` to 5 ms).
fn settle(mapper: &mut PortMapper, within: Duration) -> MappingState {
    settle_within(mapper, within, Duration::from_millis(50))
}

fn settle_strictly(mapper: &mut PortMapper, within: Duration) -> MappingState {
    settle_within(mapper, within, Duration::from_millis(5))
}

fn settle_within(mapper: &mut PortMapper, within: Duration, max_poll: Duration) -> MappingState {
    let end = Instant::now() + within;
    while Instant::now() < end {
        let t = Instant::now();
        let changed = mapper.poll();
        assert!(t.elapsed() < max_poll, "poll blocked for {:?}", t.elapsed());
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
        let state = settle_strictly(&mut mapper, Duration::from_secs(5));
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

    #[test]
    fn renew_silent() {
        // A renewal burst lost on the way (review m1): the mapping lives on
        // and is renewed again before it runs out, never reported dropped.
        let ssdp = silent();
        let fake = FakeGateway::start(false, true, false, EXTERNAL, Misbehaviour::RenewSilent);
        let mut req = request(&fake, 23600, true);
        req.ssdp = ssdp.local_addr().unwrap();
        req.lifetime = Some(Duration::from_secs(8));
        let mut mapper = PortMapper::start(req);
        mapped(&settle(&mut mapper, Duration::from_secs(5)));
        // Renewed at 4 s (silent until about 5.75 s), again at about 6.9 s.
        let end = Instant::now() + Duration::from_millis(7500);
        while Instant::now() < end {
            assert_eq!(mapper.poll(), None, "the mapping was reported lost");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fake.mappings().len(), 2);
    }

    #[test]
    fn tcp_refused() {
        // A router that refuses TCP still maps UDP alone (review m7).
        let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::TcpRefused);
        let mut mapper = PortMapper::start(upnp_only(&fake, 23610));
        let state = settle(&mut mapper, Duration::from_secs(5));
        assert_eq!(mapped(&state), (Method::Upnp, SocketAddrV4::new(EXTERNAL, 23610), false, true));
        assert_eq!(fake.mappings(), vec![(Method::Upnp, true, 23610, SocketAddrV4::new(LOCALHOST, 23610))]);
    }

    /// A UPnP-only request at `fake` (no PCP/NAT-PMP gateway).
    fn upnp_only(fake: &FakeGateway, port: u16) -> MapRequest {
        let mut req = request(fake, port, true);
        req.gateway = None;
        req
    }

    #[test]
    fn soap_drip() {
        // `AddPortMapping`'s answer drips a byte every 100 ms: the client
        // gives up at its deadline (4 s) and closes the connection; nothing
        // lingers.
        let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::SoapDrip);
        let start = Instant::now();
        let mut mapper = PortMapper::start(upnp_only(&fake, 23540));
        let state = settle(&mut mapper, Duration::from_secs(5));
        assert!(failed(&state).contains("UPnP"), "{state:?}");
        assert!(start.elapsed() < Duration::from_secs(5));
        let dropped = Instant::now();
        drop(mapper);
        assert!(dropped.elapsed() <= Duration::from_millis(1100), "drop took {:?}", dropped.elapsed());
        while fake.dripping() > 0 && dropped.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fake.dripping(), 0, "a dripping answer is still being read");
    }

    #[test]
    fn huge_body() {
        // An endless description: the client stops reading at 64 KiB.
        let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::HugeBody);
        let mut mapper = PortMapper::start(upnp_only(&fake, 23550));
        let state = settle(&mut mapper, Duration::from_secs(5));
        assert!(failed(&state).contains("UPnP"), "{state:?}");
        drop(mapper);
        std::thread::sleep(Duration::from_millis(200));
        assert!(fake.streamed() < 32 << 20, "the client read {} bytes", fake.streamed());
    }

    #[test]
    fn deep_nesting() {
        // 6000 levels of nesting (42 KB) around a real device: no crash.
        let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::DeepNesting);
        let mut mapper = PortMapper::start(upnp_only(&fake, 23560));
        let (method, ..) = mapped(&settle(&mut mapper, Duration::from_secs(5)));
        assert_eq!(method, Method::Upnp);
    }

    #[test]
    fn foreign_location() {
        // An SSDP answer naming another host than its sender is ignored.
        let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::ForeignLocation);
        let mut mapper = PortMapper::start(upnp_only(&fake, 23570));
        let state = settle(&mut mapper, Duration::from_secs(5));
        assert!(failed(&state).contains("UPnP"), "{state:?}");
        assert_eq!(fake.http_requests(), 0);
    }

    #[test]
    fn slow_delete() {
        // Each deletion's answer comes 250 ms late: the mapper's drop
        // waits for both (TCP and UDP), within `DROP_WAIT`.
        let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::SlowDelete);
        let mut mapper = PortMapper::start(upnp_only(&fake, 23710));
        mapped(&settle(&mut mapper, Duration::from_secs(5)));
        assert_eq!(fake.mappings().len(), 2);
        let dropped = Instant::now();
        drop(mapper);
        let took = dropped.elapsed();
        assert!(
            took >= Duration::from_millis(500) && took <= Duration::from_millis(1100),
            "drop took {took:?}"
        );
        assert_eq!(fake.mappings(), vec![]);
    }
}

#[test]
fn upnp_reads_real_router_descriptions() {
    for (i, d) in [Description::WanIp, Description::WanPpp, Description::UrlBase].into_iter().enumerate() {
        for chunked in [false, true] {
            let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::Honest);
            fake.serve_description(d);
            fake.chunked(chunked);
            let port = 23580 + 2 * i as u16 + u16::from(chunked);
            let mut req = request(&fake, port, true);
            req.gateway = None;
            let mut mapper = PortMapper::start(req);
            let state = settle(&mut mapper, Duration::from_secs(5));
            assert_eq!(
                state,
                MappingState::Mapped {
                    method: Method::Upnp,
                    external: SocketAddrV4::new(EXTERNAL, port),
                    tcp: true,
                    usable: true
                },
                "{d:?}, chunked {chunked}"
            );
            assert_eq!(fake.mappings().len(), 2, "{d:?}, chunked {chunked}");
        }
    }
}

/// The child of `upnp_ignores_http_proxy`: maps through a fake with
/// `HTTP_PROXY` (and its kin) set to a proxy that must never be used.
#[test]
#[ignore = "run by upnp_ignores_http_proxy with a proxy in the environment"]
fn upnp_ignores_http_proxy_child() {
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::Honest);
    let mut req = request(&fake, 23590, true);
    req.gateway = None;
    let mut mapper = PortMapper::start(req);
    mapped(&settle(&mut mapper, Duration::from_secs(5)));
    println!("{PROXY_CHILD_MAPPED}");
}

/// What the child prints once it has mapped (review n5: a filter matching
/// no test would pass vacuously).
const PROXY_CHILD_MAPPED: &str = "proxy child: mapped through the fake";

#[test]
fn upnp_ignores_http_proxy() {
    // The environment is per process: the child test runs in a process of
    // its own with the proxy variables set.
    let proxy = std::net::TcpListener::bind((LOCALHOST, 0)).unwrap();
    let url = format!("http://{}", proxy.local_addr().unwrap());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "portmap::upnp_ignores_http_proxy_child",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("HTTP_PROXY", &url)
        .env("http_proxy", &url)
        .env("ALL_PROXY", &url)
        .env("all_proxy", &url)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    proxy.set_nonblocking(true).unwrap();
    assert!(proxy.accept().is_err(), "the client went through the proxy");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the child failed to map through the fake:\n{stdout}");
    assert!(stdout.contains(PROXY_CHILD_MAPPED), "the child test did not run:\n{stdout}");
    assert!(stdout.contains("1 passed"), "the child test did not run:\n{stdout}");
}

/// Two UPnP devices answering one client's search (review n2): a broken one
/// on 127.0.0.3, which answers SSDP at once but whose HTTP accepts and
/// never answers, and `fake`, whose answer is relayed a moment later from
/// 127.0.0.1. Aim `MapRequest::ssdp` at `addr`.
struct TwoDevices {
    addr: std::net::SocketAddr,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    _silent_http: std::net::TcpListener,
}

impl TwoDevices {
    fn start(fake: &FakeGateway) -> TwoDevices {
        use std::sync::atomic::{AtomicBool, Ordering};
        let broken = Ipv4Addr::new(127, 0, 0, 3);
        let silent_http = std::net::TcpListener::bind((broken, 0)).unwrap();
        let answer = format!(
            "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
             LOCATION: http://{}/desc.xml\r\n\r\n",
            silent_http.local_addr().unwrap()
        );
        let front = UdpSocket::bind((broken, 0)).unwrap();
        let relay = UdpSocket::bind((LOCALHOST, 0)).unwrap();
        front.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
        relay.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
        let addr = front.local_addr().unwrap();
        let fake_ssdp = fake.ssdp();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn({
            let stop = stop.clone();
            move || {
                let mut client = None;
                let mut buf = [0u8; 1500];
                while !stop.load(Ordering::Relaxed) {
                    if let Ok((n, from)) = front.recv_from(&mut buf) {
                        client = Some(from);
                        let _ = front.send_to(answer.as_bytes(), from);
                        let _ = relay.send_to(&buf[..n], fake_ssdp);
                    }
                    if let (Ok((n, from)), Some(client)) = (relay.recv_from(&mut buf), client)
                        && from == fake_ssdp
                    {
                        let _ = relay.send_to(&buf[..n], client);
                    }
                }
            }
        });
        TwoDevices { addr, stop, thread: Some(thread), _silent_http: silent_http }
    }
}

impl Drop for TwoDevices {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[test]
fn a_stalled_device_does_not_hide_the_router() {
    // The broken device answers first and stalls its HTTP past the search
    // window; the router's answer, queued meanwhile, is still tried.
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::Honest);
    let devices = TwoDevices::start(&fake);
    let mut req = request(&fake, 23620, true);
    req.gateway = None;
    req.ssdp = devices.addr;
    let mut mapper = PortMapper::start(req);
    let state = settle(&mut mapper, Duration::from_secs(5));
    assert_eq!(mapped(&state).0, Method::Upnp, "{state:?}");
}

#[test]
fn a_panicking_winner_is_reported() {
    // A bug in the winning thread (review n1) ends the mapping it held.
    let ssdp = silent();
    let fake = FakeGateway::start(false, true, false, EXTERNAL, Misbehaviour::Honest);
    let mut req = request(&fake, 23630, true);
    req.ssdp = ssdp.local_addr().unwrap();
    req.lifetime = Some(Duration::from_secs(2));
    rl_net::portmap::panic_at_renewal(23630);
    let mut mapper = PortMapper::start(req);
    mapped(&settle(&mut mapper, Duration::from_secs(5)));
    let state = settle(&mut mapper, Duration::from_secs(3));
    assert!(failed(&state).contains("panicked"), "{state:?}");
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

/// Ruling PM-2 (Task 8 review): one request maps several ports with one
/// discovery. The protocol that maps `local` maps each extra port too, UDP
/// with the external port equal to the local one; a port the router would
/// move (here held by another client) fails alone and is never shifted.
/// `close` deletes them all and says the router confirmed it.
#[test]
fn extra_ports_are_mapped_exactly_by_the_winner() {
    let ssdp = silent();
    for (upnp, pmp, pcp, port) in
        [(true, false, false, 23520), (false, true, false, 23530), (false, false, true, 23540)]
    {
        let fake = FakeGateway::start(upnp, pmp, pcp, EXTERNAL, Misbehaviour::Honest);
        fake.hold(port + 2);
        let mut req = request(&fake, port, false);
        if upnp {
            req.gateway = None;
        } else {
            req.ssdp = ssdp.local_addr().unwrap();
        }
        req.exact = true;
        req.extra = vec![port + 1, port + 2, port + 3];
        let mut mapper = PortMapper::start(req);
        let (method, external, ..) = mapped(&settle(&mut mapper, Duration::from_secs(5)));
        assert_eq!(external.port(), port);
        let begun = Instant::now();
        while mapper.extras().iter().any(|(_, s)| *s == MappingState::Trying) {
            assert!(begun.elapsed() < Duration::from_secs(5), "{:?}", mapper.extras());
            mapper.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        let extras = mapper.extras().to_vec();
        assert_eq!(extras.iter().map(|e| e.0).collect::<Vec<_>>(), vec![port + 1, port + 2, port + 3]);
        for (p, s) in &extras {
            if *p == port + 2 {
                failed(s);
            } else {
                let (m, ext, tcp, usable) = mapped(s);
                assert_eq!((m, ext, tcp, usable), (method, SocketAddrV4::new(EXTERNAL, *p), false, true));
            }
        }
        let held: Vec<(bool, u16, u16)> = fake.mappings().iter().map(|m| (m.1, m.2, m.3.port())).collect();
        assert_eq!(held, vec![(true, port, port), (true, port + 1, port + 1), (true, port + 3, port + 3)]);
        assert!(mapper.close(), "the router confirmed every deletion");
        assert_eq!(fake.mappings(), vec![], "upnp {upnp} pmp {pmp} pcp {pcp}");
    }
}

/// `close` returns once the router confirmed the deletions: a router that
/// removes a mapping only `LATE` after the request (`LateUpnp`) has none
/// left when it returns. A primary that fails fails its extras too.
#[test]
fn close_waits_for_the_routers_deletion() {
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::LateUpnp);
    let mut req = request(&fake, 23550, true);
    req.gateway = None;
    let mut mapper = PortMapper::start(req);
    mapped(&settle(&mut mapper, Duration::from_secs(5)));
    assert_eq!(fake.mappings().len(), 2);
    let begun = Instant::now();
    assert!(mapper.close());
    assert!(begun.elapsed() >= rl_net::portmap::fake::LATE, "{:?}", begun.elapsed());
    assert_eq!(fake.mappings(), vec![], "gone when close returns");

    let ssdp = silent();
    let mut req = MapRequest::new(SocketAddrV4::new(LOCALHOST, 23560), false, None);
    req.ssdp = ssdp.local_addr().unwrap();
    req.search = Duration::from_millis(30);
    req.extra = vec![23561];
    let mut mapper = PortMapper::start(req);
    failed(&settle(&mut mapper, Duration::from_secs(6)));
    mapper.poll();
    assert!(matches!(mapper.extras(), [(23561, MappingState::Failed(_))]), "{:?}", mapper.extras());
    assert!(mapper.close(), "nothing to delete");
}

/// Ruling RH-1: a mapper of a port another mapper of this process is
/// still deleting (a router that deletes `LATE` after the request,
/// `LateDelete`, and adds at once) waits for that deletion before it asks:
/// the new mapping is there once the old deletion took effect.
#[test]
fn a_new_mapper_waits_for_the_ports_deletion() {
    let fake = FakeGateway::start(true, false, false, EXTERNAL, Misbehaviour::LateDelete);
    let mut req = request(&fake, 23570, true);
    req.gateway = None;
    let mut old = PortMapper::start(req.clone());
    mapped(&settle(&mut old, Duration::from_secs(5)));
    let closing = std::thread::spawn(move || old.close());
    std::thread::sleep(Duration::from_millis(50));
    let begun = Instant::now();
    let mut new = PortMapper::start(req);
    mapped(&settle(&mut new, Duration::from_secs(5)));
    assert!(begun.elapsed() >= Duration::from_millis(200), "waited for the deletion: {:?}", begun.elapsed());
    assert!(closing.join().unwrap(), "the old deletion confirmed");
    std::thread::sleep(rl_net::portmap::fake::LATE);
    assert_eq!(fake.mappings().len(), 2, "the new TCP and UDP mapping stay");
    assert!(new.close());
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
/// SSDP/HTTP head naming the fake's HTTP server (`$HTTP`) over junk, a
/// truncated all-in-one device description and SOAP answer (never a whole
/// one: it ends with the `AddPortMappingResponse` truncation always cuts),
/// deep nesting, a body over 64 KiB, runs of `<`, `<a`, attributes or
/// entities, or malformed chunked framing.
fn upnp_answer() -> impl Strategy<Value = Vec<u8>> {
    const DOC: &str = "<?xml version=\"1.0\"?><root><device><serviceList><service>\
        <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
        <SCPDURL>/scpd.xml</SCPDURL><controlURL>/ctl</controlURL></service></serviceList></device>\
        <s:Body><u:GetExternalIPAddressResponse><NewExternalIPAddress>203.0.113.7</NewExternalIPAddress>\
        </u:GetExternalIPAddressResponse><u:AddPortMappingResponse/>";
    let location = prop_oneof![
        Just("http://$HTTP/desc.xml".to_string()),
        Just("http://localhost/desc.xml".to_string()),
        Just("http://$HTTP/other.xml".to_string()),
        Just("http://$HTTP".to_string()),
        "[ -~]{0,40}",
    ];
    let truncated = || any::<Index>().prop_map(|cut| DOC.as_bytes()[..cut.index(DOC.len())].to_vec());
    let body = prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..400),
        truncated(),
        (0usize..9000, truncated()).prop_map(|(depth, doc)| {
            let mut b = "<a>".repeat(depth).into_bytes();
            b.extend(doc);
            b
        }),
        (65_000usize..100_000).prop_map(|n| vec![b'x'; n]),
        // Shapes that make a naive tag scanner quadratic (review IN-1).
        (0usize..60_000, prop_oneof![Just("<"), Just("<a"), Just(" a=1"), Just("</"), Just("&#x")])
            .prop_map(|(n, unit)| format!("<service{}", unit.repeat(n / unit.len())).into_bytes()),
    ];
    let chunk = (
        prop_oneof![Just("5".to_string()), Just("FFFFFFFFF".to_string()), "[0-9a-fA-FxZ;= -]{0,10}"],
        proptest::collection::vec(any::<u8>(), 0..12),
        prop_oneof![Just("\r\n"), Just("\n"), Just("")],
    );
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
        proptest::collection::vec(chunk, 0..8).prop_map(|chunks| {
            let mut answer =
                b"HTTP/1.1 200 OK\r\nLOCATION: http://$HTTP/desc.xml\r\nTransfer-Encoding: chunked\r\n\r\n"
                    .to_vec();
            for (size, data, end) in chunks {
                answer.extend(size.into_bytes());
                answer.extend(b"\r\n");
                answer.extend(data);
                answer.extend(end.as_bytes());
            }
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
    fn upnp_parsers_never_panic(bytes in upnp_answer(), eof in any::<bool>()) {
        let from = std::net::SocketAddr::from((LOCALHOST, 1900));
        let _ = upnp::parse_ssdp(&bytes, from);
        let _ = upnp::parse_http(&bytes, eof);
        let text = String::from_utf8_lossy(&bytes);
        let location = upnp::Location { addr: SocketAddrV4::new(LOCALHOST, 80), path: "/d/desc.xml".into() };
        let _ = upnp::control_urls(&text, &location);
        let _ = upnp::soap_result(200, &text, "AddPortMapping");
        let _ = upnp::soap_result(500, &text, "GetExternalIPAddress");
        let _ = upnp::element_text(&text, "a");
    }

    #[test]
    fn upnp_answers_never_panic(answer in upnp_answer()) {
        let fake = FakeGateway::serving(answer);
        let mut req = request(&fake, 23530, true);
        req.gateway = None;
        // A failed device leaves the client listening for others until the
        // search window ends: shortened, so 256 cases stay quick.
        req.search = Duration::from_millis(30);
        let mut mapper = PortMapper::start(req);
        let state = settle(&mut mapper, Duration::from_secs(6));
        prop_assert!(matches!(&state, MappingState::Failed(why) if !why.contains("panicked")), "{:?}", state);
        prop_assert_eq!(rl_net::portmap::worker_panics(), 0);
    }
}

/// Ruling FR12b-2 (final review A-I2): a router whose external address
/// changes (an ISP renumbering it) keeps the mapping; each renewal reads
/// the address again (PCP from its answer, NAT-PMP by asking, UPnP by
/// `GetExternalIPAddress`), and the mapper reports it, the extra ports too.
#[test]
fn a_renewal_reports_a_changed_external_address() {
    let ssdp = silent();
    let moved = Ipv4Addr::new(198, 51, 100, 9);
    for (i, (upnp, pmp, pcp, method)) in [
        (true, false, false, Method::Upnp),
        (false, true, false, Method::NatPmp),
        (false, false, true, Method::Pcp),
    ]
    .into_iter()
    .enumerate()
    {
        let port = 23640 + 10 * i as u16;
        let fake = FakeGateway::start(upnp, pmp, pcp, EXTERNAL, Misbehaviour::Honest);
        let mut req = request(&fake, port, true);
        if upnp {
            req.gateway = None;
        } else {
            req.ssdp = ssdp.local_addr().unwrap();
        }
        req.extra = vec![port + 5];
        req.lifetime = Some(Duration::from_secs(2));
        let mut mapper = PortMapper::start(req);
        let state = settle(&mut mapper, Duration::from_secs(5));
        assert_eq!(mapped(&state), (method, SocketAddrV4::new(EXTERNAL, port), true, true), "{method}");
        fake.set_external(moved);
        let end = Instant::now() + Duration::from_secs(4);
        let mut now_at = None;
        while Instant::now() < end && now_at.is_none() {
            if let Some(s) = mapper.poll() {
                now_at = Some(mapped(&s).1);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(now_at, Some(SocketAddrV4::new(moved, port)), "{method}: the renewal's address");
        let extra = &mapper.extras()[0].1;
        assert_eq!(mapped(extra).1, SocketAddrV4::new(moved, port + 5), "{method}: the extra port");
    }
}
