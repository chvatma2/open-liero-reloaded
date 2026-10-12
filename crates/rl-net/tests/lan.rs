//! LAN announcements over loopback, ephemeral ports, every wait bounded.

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use rl_net::lan::{Announcer, LanGame, Listener, broadcast_targets};
use rl_proto::{LAN_PORT, LanAnnounce, encode_lan};

fn announce(name: &str, port: u16) -> LanAnnounce {
    LanAnnounce {
        fingerprint: 7,
        name: name.to_string(),
        port,
        players: 2,
        max_players: 8,
        spectators: 0,
        mod_id: "olx-classic".to_string(),
        level_id: "olx-level-dirt-level".to_string(),
        password: false,
        playing: false,
    }
}

fn listener() -> (Listener, SocketAddr) {
    let l = Listener::new(0).unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], l.local_port()));
    (l, addr)
}

/// Polls until `done` holds or ten seconds pass (a generous bound for a
/// loaded machine; a passing test returns as soon as `done` holds).
fn poll_until(l: &mut Listener, now: Instant, done: impl Fn(&[LanGame]) -> bool) -> Vec<LanGame> {
    poll_within(l, now, Duration::from_secs(10), done)
}

/// Polls until `done` holds or `wait` passes.
fn poll_within(
    l: &mut Listener,
    now: Instant,
    wait: Duration,
    done: impl Fn(&[LanGame]) -> bool,
) -> Vec<LanGame> {
    let end = Instant::now() + wait;
    loop {
        let games = l.poll(now).to_vec();
        if done(&games) || Instant::now() > end {
            return games;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn game_appears_with_announced_fields_and_source_ip() {
    let (mut l, target) = listener();
    let mut a = Announcer::new(target).unwrap();
    let t0 = Instant::now();
    let what = announce("Office", 23400);
    a.tick(t0, &what);
    let games = poll_until(&mut l, t0, |g| !g.is_empty());
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].info, what);
    assert_eq!(games[0].addr, "127.0.0.1:23400".parse().unwrap());
    assert_eq!(games[0].seen, t0);
}

#[test]
fn announcements_are_at_most_every_two_seconds() {
    let raw = UdpSocket::bind("127.0.0.1:0").unwrap();
    raw.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    let mut a = Announcer::new(raw.local_addr().unwrap()).unwrap();
    let t0 = Instant::now();
    let what = announce("Office", 23400);
    a.tick(t0, &what);
    a.tick(t0 + Duration::from_millis(1999), &what);
    let mut buf = [0u8; 2048];
    assert!(raw.recv_from(&mut buf).is_ok());
    assert!(raw.recv_from(&mut buf).is_err(), "second datagram within 2 s");
    a.tick(t0 + Duration::from_secs(2), &what);
    let (n, _) = raw.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..n], &encode_lan(&what)[..]);
}

#[test]
fn game_expires_six_seconds_after_the_last_announcement() {
    let (mut l, target) = listener();
    let mut a = Announcer::new(target).unwrap();
    let t0 = Instant::now();
    a.tick(t0, &announce("Office", 23400));
    assert_eq!(poll_until(&mut l, t0, |g| !g.is_empty()).len(), 1);
    assert_eq!(l.poll(t0 + Duration::from_millis(5999)).len(), 1);
    assert!(l.poll(t0 + Duration::from_secs(6)).is_empty());
}

#[test]
fn a_new_announcement_refreshes_and_updates() {
    let (mut l, target) = listener();
    let mut a = Announcer::new(target).unwrap();
    let t0 = Instant::now();
    a.tick(t0, &announce("Office", 23400));
    poll_until(&mut l, t0, |g| !g.is_empty());
    let t1 = t0 + Duration::from_secs(4);
    let mut what = announce("Office", 23400);
    what.players = 5;
    a.tick(t1, &what);
    let games = poll_until(&mut l, t1, |g| g.first().is_some_and(|g| g.seen == t1));
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].info.players, 5);
    assert_eq!(l.poll(t0 + Duration::from_secs(9)).len(), 1);
}

#[test]
fn garbage_is_ignored() {
    let (mut l, target) = listener();
    let raw = UdpSocket::bind("127.0.0.1:0").unwrap();
    let t0 = Instant::now();
    raw.send_to(&[], target).unwrap();
    raw.send_to(&[0xff; 40], target).unwrap();
    raw.send_to(&vec![0u8; 5000], target).unwrap();
    let mut bad = announce("Office", 23400);
    bad.name = "a\nb".into();
    raw.send_to(&encode_lan(&bad), target).unwrap();
    let mut a = Announcer::new(target).unwrap();
    a.tick(t0, &announce("Good", 1));
    let games = poll_until(&mut l, t0, |g| !g.is_empty());
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].info.name, "Good");
}

fn sock(ip: [u8; 4]) -> UdpSocket {
    UdpSocket::bind((std::net::Ipv4Addr::from(ip), 0)).unwrap()
}

#[test]
fn at_most_64_games_sorted_by_name() {
    let (mut l, target) = listener();
    // Linux loopback accepts the whole 127/8: 65 distinct source IPs.
    let socks: Vec<UdpSocket> = (0..65u8).map(|i| sock([127, 0, 1, i + 1])).collect();
    for (i, s) in socks.iter().enumerate() {
        s.send_to(&encode_lan(&announce(&format!("g{i:02}"), 1000 + i as u16)), target).unwrap();
    }
    let t0 = Instant::now();
    let games = poll_until(&mut l, t0, |g| g.len() >= 64);
    assert_eq!(games.len(), 64);
    std::thread::sleep(Duration::from_millis(100));
    let games = l.poll(t0).to_vec();
    assert_eq!(games.len(), 64);
    let names: Vec<_> = games.iter().map(|g| g.info.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "sorted by name");
}

#[test]
fn rotating_ports_from_one_ip_cannot_hide_a_real_host() {
    let (mut l, target) = listener();
    for i in 0..65u16 {
        let s = sock([127, 0, 0, 1]);
        s.send_to(&encode_lan(&announce(&format!("evil{i:02}"), 2000 + i)), target).unwrap();
    }
    let real = sock([127, 0, 0, 2]);
    real.send_to(&encode_lan(&announce("Real", 23400)), target).unwrap();
    let t0 = Instant::now();
    let games = poll_until(&mut l, t0, |g| g.iter().any(|g| g.info.name == "Real"));
    assert!(games.iter().any(|g| g.info.name == "Real"));
    assert!(games.iter().filter(|g| g.addr.ip().is_loopback() && g.info.name != "Real").count() <= 4);
}

#[test]
fn one_ip_holds_at_most_four_games() {
    let (mut l, target) = listener();
    for i in 0..8u16 {
        let s = sock([127, 0, 0, 1]);
        s.send_to(&encode_lan(&announce(&format!("g{i}"), 3000 + i)), target).unwrap();
    }
    let t0 = Instant::now();
    poll_until(&mut l, t0, |g| g.len() >= 4);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(l.poll(t0).len(), 4);
}

#[test]
fn ten_datagrams_in_a_second_are_admitted_the_eleventh_is_not() {
    let (mut l, target) = listener();
    // An address of its own: strays from other tests come from 127.0.0.1 and
    // would count in this source IP's rate window.
    let raw = sock([127, 0, 2, 1]);
    for _ in 0..9 {
        raw.send_to(&[9], target).unwrap();
    }
    raw.send_to(&encode_lan(&announce("tenth", 1)), target).unwrap();
    raw.send_to(&encode_lan(&announce("eleventh", 1)), target).unwrap();
    let t0 = Instant::now();
    let games = poll_until(&mut l, t0, |g| !g.is_empty());
    std::thread::sleep(Duration::from_millis(100));
    let games2 = l.poll(t0).to_vec();
    assert_eq!(games.len(), 1);
    assert_eq!(games2.len(), 1);
    assert_eq!(games2[0].info.name, "tenth");
}

/// Time is virtual: every `poll` takes the instant the test chooses, so the
/// ten seconds cost nothing and no scheduling delay can move a datagram into
/// another rate window. Delivery is awaited, not slept for: the flood is
/// valid announcements from one socket (loopback keeps their order), and the
/// tenth, the last admitted, shows when every one before it has been read.
#[test]
fn a_flooding_source_is_ignored_for_ten_seconds() {
    let (mut l, target) = listener();
    let raw = sock([127, 0, 2, 2]); // its own IP: see the test above
    let t0 = Instant::now();
    for i in 0..12 {
        raw.send_to(&encode_lan(&announce(&format!("flood{i:02}"), 1)), target).unwrap();
    }
    // One source address holds one entry: the last admitted datagram (the
    // 10th) is the entry's name once the flood has been read.
    let games = poll_until(&mut l, t0, |g| g.iter().any(|g| g.info.name == "flood09"));
    assert!(games.iter().any(|g| g.info.name == "flood09"), "the first ten datagrams arrive: {games:?}");
    // The 11th and 12th were sent before the 10th was read, so they are in
    // the socket (or read and ignored): `flood10`, `flood11` never show.
    let games = poll_within(&mut l, t0, Duration::from_millis(200), |_| false);
    assert!(games.iter().all(|g| g.info.name != "flood10" && g.info.name != "flood11"), "{games:?}");

    raw.send_to(&encode_lan(&announce("second", 1)), target).unwrap();
    let t1 = t0 + Duration::from_secs(5);
    let games =
        poll_within(&mut l, t1, Duration::from_millis(200), |g| g.iter().any(|g| g.info.name == "second"));
    assert!(games.iter().all(|g| g.info.name != "second"), "ignored at 5 s: {games:?}");

    raw.send_to(&encode_lan(&announce("third", 1)), target).unwrap();
    let t2 = t0 + Duration::from_secs(11);
    let games = poll_until(&mut l, t2, |g| g.iter().any(|g| g.info.name == "third"));
    assert_eq!(games.len(), 1, "{games:?}");
    assert_eq!(games[0].info.name, "third");
}

#[test]
fn broadcast_targets_include_the_limited_broadcast() {
    let t = broadcast_targets();
    assert!(t.contains(&SocketAddr::from(([255, 255, 255, 255], LAN_PORT))));
}
