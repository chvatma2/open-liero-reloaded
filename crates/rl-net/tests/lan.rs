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

/// Polls until `done` holds or two seconds pass.
fn poll_until(l: &mut Listener, now: Instant, done: impl Fn(&[LanGame]) -> bool) -> Vec<LanGame> {
    let end = Instant::now() + Duration::from_secs(2);
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

#[test]
fn at_most_64_games() {
    let (mut l, target) = listener();
    let socks: Vec<UdpSocket> = (0..65).map(|_| UdpSocket::bind("127.0.0.1:0").unwrap()).collect();
    for (i, s) in socks.iter().enumerate() {
        s.send_to(&encode_lan(&announce(&format!("g{i:02}"), 1000 + i as u16)), target).unwrap();
    }
    let t0 = Instant::now();
    let games = poll_until(&mut l, t0, |g| g.len() >= 64);
    assert_eq!(games.len(), 64);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(l.poll(t0).len(), 64);
    let names: Vec<_> = games.iter().map(|g| g.info.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "sorted by name");
}

#[test]
fn a_flooding_source_is_ignored_for_ten_seconds() {
    let (mut l, target) = listener();
    let raw = UdpSocket::bind("127.0.0.1:0").unwrap();
    let t0 = Instant::now();
    raw.send_to(&encode_lan(&announce("first", 1)), target).unwrap();
    for _ in 0..12 {
        raw.send_to(&[1, 2, 3], target).unwrap();
    }
    std::thread::sleep(Duration::from_millis(100));
    l.poll(t0);
    raw.send_to(&encode_lan(&announce("second", 1)), target).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let t1 = t0 + Duration::from_secs(5);
    assert!(l.poll(t1).iter().all(|g| g.info.name != "second"));
    raw.send_to(&encode_lan(&announce("third", 1)), target).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let t2 = t0 + Duration::from_secs(11);
    let games = poll_until(&mut l, t2, |g| !g.is_empty());
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].info.name, "third");
}

#[test]
fn broadcast_targets_include_the_limited_broadcast() {
    let t = broadcast_targets();
    assert!(t.contains(&SocketAddr::from(([255, 255, 255, 255], LAN_PORT))));
}
