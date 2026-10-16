//! STUN: `public_ip` against local responders (nothing touches the public
//! internet); the codec's own tests are olr-proto's.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use olr_discovery::stun::response;
use olr_discovery::{StunResponder, binding_request, parse_binding_response, public_ip};

fn sa(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// The codec moved to `olr_proto::stun` (its tests too); these names
/// re-export it.
#[test]
fn the_codec_is_re_exported() {
    let t = [7u8; 12];
    assert_eq!(binding_request(t), olr_proto::stun::binding_request(t));
    let r = response(&t, sa("198.51.100.7:5000"), true);
    assert_eq!(parse_binding_response(&r, &t), Some(sa("198.51.100.7:5000")));
    assert!(olr_discovery::list::is_global("8.8.8.8".parse().unwrap()));
}

#[test]
fn asks_the_next_server_when_one_is_silent() {
    let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let responder = StunResponder::start(sa("192.0.2.77:1234"));
    let servers = vec![silent.local_addr().unwrap().to_string(), responder.server()];
    let start = Instant::now();
    let ip = public_ip(&servers, Duration::from_secs(5));
    assert_eq!(ip, Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 77))));
    assert!(start.elapsed() >= Duration::from_millis(900), "two tries of 500 ms on the silent server");
    assert_eq!(responder.answered(), 1);
}

#[test]
fn no_stun_answer_leaves_no_address() {
    // Without the deadline the two silent servers alone take 2 s (two
    // tries of 500 ms each); the bound leaves room for a loaded machine.
    let (a, b) = (UdpSocket::bind("127.0.0.1:0").unwrap(), UdpSocket::bind("127.0.0.1:0").unwrap());
    let servers = vec![
        a.local_addr().unwrap().to_string(),
        b.local_addr().unwrap().to_string(),
        "not a server".to_string(),
    ];
    let start = Instant::now();
    assert_eq!(public_ip(&servers, Duration::from_millis(300)), None, "the deadline cuts it short");
    assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());
    assert_eq!(public_ip(&[], Duration::from_secs(1)), None);
}

#[test]
fn the_deadline_is_checked_before_each_lookup() {
    // A passed deadline resolves no further name (a lookup may block): with
    // none left at all, nobody is asked.
    let responder = StunResponder::start(sa("192.0.2.5:1"));
    assert_eq!(public_ip(&[responder.server()], Duration::ZERO), None);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(responder.answered(), 0);
}

#[test]
fn forged_and_stray_replies_are_ignored() {
    // The server first gets a reply with the right transaction sent from
    // another port (forged), then one with a wrong transaction from its own
    // port, then the real answer.
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let forger = UdpSocket::bind("127.0.0.1:0").unwrap();
    server.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let addr = server.local_addr().unwrap();
    let fake = std::thread::spawn(move || {
        let mut buf = [0u8; 100];
        let (len, from) = server.recv_from(&mut buf).unwrap();
        assert_eq!(len, 20);
        let mut t = [0u8; 12];
        t.copy_from_slice(&buf[8..20]);
        forger.send_to(&response(&t, sa("198.51.100.66:1"), true), from).unwrap();
        let mut wrong = t;
        wrong[0] ^= 1;
        server.send_to(&response(&wrong, sa("198.51.100.67:1"), true), from).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        server.send_to(&response(&t, sa("203.0.113.5:1"), true), from).unwrap();
    });
    let ip = public_ip(&[addr.to_string()], Duration::from_secs(3));
    fake.join().unwrap();
    assert_eq!(ip, Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5))));
}

/// `olr-cli stun-server`'s mode: the answer is the request's observed
/// source (address and port), on the address it was bound to.
#[test]
fn a_reflecting_responder_answers_with_the_source() {
    let responder = StunResponder::reflecting(sa("127.0.0.1:0")).unwrap();
    let to = responder.local_addr();
    assert_eq!(to.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_ne!(to.port(), 0);
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let t = [3u8; 12];
    client.send_to(&binding_request(t), to).unwrap();
    let mut buf = [0u8; 200];
    let (len, from) = client.recv_from(&mut buf).unwrap();
    assert_eq!(from, to);
    assert_eq!(parse_binding_response(&buf[..len], &t), Some(client.local_addr().unwrap()));
    assert_eq!(responder.answered(), 1);
    // A port taken already is an error, not a panic.
    assert!(StunResponder::reflecting(to).is_err());
}
