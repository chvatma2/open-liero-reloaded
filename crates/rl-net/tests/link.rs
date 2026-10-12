//! One WebRTC peer connection (spec §2.1): both channels, in memory on a
//! virtual clock and over real localhost sockets.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use rl_net::link::{Link, LinkError, LinkEvent, MAX_MESSAGE, Side, Transmit};
use rl_netcode::Channel;

/// Two links over an in-memory wire with a fixed one-way delay, on a virtual clock.
struct Wire {
    now: Instant,
    a: Link,
    b: Link,
    in_flight: Vec<(Instant, bool /* to b */, Transmit)>,
    got_a: Vec<LinkEvent>,
    got_b: Vec<LinkEvent>,
}

const A: &str = "10.0.0.1:5000";
const B: &str = "10.0.0.2:6000";

fn pair() -> Wire {
    let now = Instant::now();
    let (a, offer) = Link::offerer(&[A.parse().unwrap()], now);
    let (b, answer) = Link::answerer(&[B.parse().unwrap()], &offer, now).unwrap();
    let mut w = Wire { now, a, b, in_flight: vec![], got_a: vec![], got_b: vec![] };
    w.a.accept_answer(&answer).unwrap();
    w
}

impl Wire {
    /// Runs both links until `until`, delivering each datagram `delay` after it was sent.
    fn run(&mut self, until: Instant, delay: Duration) {
        while self.now < until {
            let mut out = Vec::new();
            let ta = self.a.poll(self.now, &mut out, &mut self.got_a);
            for t in out.drain(..) {
                self.in_flight.push((self.now + delay, true, t));
            }
            let tb = self.b.poll(self.now, &mut out, &mut self.got_b);
            for t in out.drain(..) {
                self.in_flight.push((self.now + delay, false, t));
            }
            let next_wire = self.in_flight.iter().map(|f| f.0).min();
            let next = [Some(ta), Some(tb), next_wire, Some(until)].into_iter().flatten().min().unwrap();
            self.now = next.max(self.now + Duration::from_micros(100));
            let (due, rest): (Vec<_>, Vec<_>) = self.in_flight.drain(..).partition(|f| f.0 <= self.now);
            self.in_flight = rest;
            for (_, to_b, t) in due {
                if to_b {
                    assert!(self.b.accepts(t.from, t.to, &t.bytes), "b accepts a's datagrams");
                    self.b.receive(t.from, t.to, &t.bytes, self.now)
                } else {
                    assert!(self.a.accepts(t.from, t.to, &t.bytes), "a accepts b's datagrams");
                    self.a.receive(t.from, t.to, &t.bytes, self.now)
                }
            }
        }
    }
}

#[test]
fn links_connect_and_carry_both_channels_on_a_virtual_clock() {
    let wall = Instant::now();
    let delay = Duration::from_millis(20);
    let mut w = pair();
    let t0 = w.now;
    w.run(t0 + Duration::from_secs(2), delay);
    assert!(w.a.is_open() && w.b.is_open(), "connected within 2 s of virtual time");
    assert!(w.got_a.iter().any(|e| matches!(e, LinkEvent::Open)), "a reports Open");
    assert!(w.got_b.iter().any(|e| matches!(e, LinkEvent::Open)), "b reports Open");
    for i in 0..1000u32 {
        w.a.send(Channel::Unreliable, &i.to_le_bytes()).unwrap();
        w.a.send(Channel::Reliable, &[i as u8; 1000]).unwrap();
        let t = w.now + Duration::from_millis(10);
        w.run(t, delay);
    }
    let t = w.now + Duration::from_secs(20);
    w.run(t, delay);
    let unreliable = w
        .got_b
        .iter()
        .filter(|e| matches!(e, LinkEvent::Message { channel: Channel::Unreliable, .. }))
        .count();
    let reliable: Vec<_> = w
        .got_b
        .iter()
        .filter_map(|e| match e {
            LinkEvent::Message { channel: Channel::Reliable, bytes } => Some(bytes[0]),
            _ => None,
        })
        .collect();
    assert_eq!(unreliable, 1000);
    assert_eq!(reliable, (0..1000u32).map(|i| i as u8).collect::<Vec<_>>());
    assert!(w.now - t0 >= Duration::from_secs(30), "covers at least 30 s of virtual time");
    assert!(!w.got_a.iter().chain(&w.got_b).any(|e| matches!(e, LinkEvent::Closed { .. })), "no link closed");
    let wall = wall.elapsed();
    eprintln!("virtual {:?} in wall {wall:?}", w.now - t0);
    assert!(wall < Duration::from_secs(2), "virtual time, not wall time");
}

#[test]
fn links_refuse_early_oversized_and_late_sends_and_report_a_close() {
    let delay = Duration::from_millis(20);
    let mut w = pair();
    assert_eq!((w.a.side(), w.b.side()), (Side::Offerer, Side::Answerer));
    assert_eq!(w.a.send(Channel::Reliable, b"early"), Err(LinkError::NotOpen));
    assert!(matches!(w.a.accept_answer("v=0"), Err(LinkError::Sdp(_))), "a second answer");
    assert!(matches!(w.b.accept_answer("v=0"), Err(LinkError::Sdp(_))), "an answer to an answerer");
    let t = w.now + Duration::from_secs(2);
    w.run(t, delay);
    assert!(w.a.is_open() && w.b.is_open());
    assert_eq!(w.a.send(Channel::Reliable, &vec![0; MAX_MESSAGE + 1]), Err(LinkError::TooLarge));
    w.a.send(Channel::Reliable, &vec![7; MAX_MESSAGE]).unwrap();
    w.b.close();
    assert!(!w.b.is_open());
    assert_eq!(w.b.send(Channel::Unreliable, b"late"), Err(LinkError::Closed));
    let t = w.now + Duration::from_secs(2);
    w.run(t, delay);
    let closed = w.got_a.iter().filter(|e| matches!(e, LinkEvent::Closed { .. })).count();
    assert_eq!(closed, 1, "a reports the peer's close once: {:?}", w.got_a.last());
    assert!(!w.a.is_open());
    assert!(!w.got_b.iter().any(|e| matches!(e, LinkEvent::Closed { .. })), "no event after a local close");
}

/// Peers that poll irregularly (a loaded machine: each wakes after 0.2 to
/// 20.2 ms, datagrams wait for it) still send about one datagram per input
/// message: no FORWARD-TSN/SACK storm, which floods real sockets and loses
/// inputs (seen under `cargo xtask check`).
#[test]
fn irregular_polling_does_not_multiply_the_inputs_traffic() {
    let mut seed = 0x1234_5678_u64;
    let mut sleep = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        Duration::from_micros(200 + seed % 20_000)
    };
    let mut w = pair();
    let t = w.now + Duration::from_secs(2);
    w.run(t, Duration::from_millis(1));
    assert!(w.a.is_open() && w.b.is_open());
    let (mut wake, mut inbox, mut datagrams) = ([w.now; 2], [Vec::<Transmit>::new(), Vec::new()], [0; 2]);
    let (mut sent, mut next_send) = (0u32, w.now);
    let end = w.now + Duration::from_secs(10);
    while w.now < end {
        for side in 0..2 {
            if w.now < wake[side] {
                continue;
            }
            let now = w.now;
            let (link, events) = if side == 0 { (&mut w.a, &mut w.got_a) } else { (&mut w.b, &mut w.got_b) };
            for t in std::mem::take(&mut inbox[side]) {
                link.receive(t.from, t.to, &t.bytes, now);
            }
            while side == 0 && sent < 5000 && next_send <= now {
                link.send(Channel::Unreliable, &[sent as u8; 64]).unwrap();
                sent += 1;
                next_send += Duration::from_millis(1);
            }
            let mut out = Vec::new();
            link.poll(now, &mut out, events);
            datagrams[side] += out.len();
            inbox[1 - side].extend(out);
            wake[side] = now + sleep();
        }
        w.now += Duration::from_micros(100);
    }
    let got = w
        .got_b
        .iter()
        .filter(|e| matches!(e, LinkEvent::Message { channel: Channel::Unreliable, .. }))
        .count();
    assert_eq!(got, 5000);
    assert!(
        datagrams[0] < 2 * 5000 && datagrams[1] < 5000,
        "datagrams sent (a, b): {datagrams:?} for 5000 messages"
    );
}

fn sockets() -> (UdpSocket, UdpSocket) {
    let a = UdpSocket::bind("127.0.0.1:0").unwrap();
    let b = UdpSocket::bind("127.0.0.1:0").unwrap();
    a.set_nonblocking(true).unwrap();
    b.set_nonblocking(true).unwrap();
    (a, b)
}

fn pump(sock: &UdpSocket, link: &mut Link, events: &mut Vec<LinkEvent>) {
    let local = sock.local_addr().unwrap();
    let mut buf = [0u8; 2048];
    while let Ok((n, from)) = sock.recv_from(&mut buf) {
        link.receive(from, local, &buf[..n], Instant::now());
    }
    let mut out = Vec::new();
    link.poll(Instant::now(), &mut out, events);
    for t in out {
        let _ = sock.send_to(&t.bytes, t.to);
    }
}

/// Two links over real localhost sockets, connected (within 1 s).
struct Real {
    sa: UdpSocket,
    sb: UdpSocket,
    a: Link,
    b: Link,
    ea: Vec<LinkEvent>,
    eb: Vec<LinkEvent>,
}

impl Real {
    fn connect() -> Real {
        let (sa, sb) = sockets();
        let (mut a, offer) = Link::offerer(&[sa.local_addr().unwrap()], Instant::now());
        let (mut b, answer) = Link::answerer(&[sb.local_addr().unwrap()], &offer, Instant::now()).unwrap();
        a.accept_answer(&answer).unwrap();
        let (mut ea, mut eb) = (Vec::new(), Vec::new());
        let start = Instant::now();
        while !(a.is_open() && b.is_open()) {
            pump(&sa, &mut a, &mut ea);
            pump(&sb, &mut b, &mut eb);
            assert!(start.elapsed() < Duration::from_secs(1), "connects within 1 s");
            std::thread::sleep(Duration::from_millis(1));
        }
        eprintln!("connected in {:?}", start.elapsed());
        Real { sa, sb, a, b, ea, eb }
    }

    fn pump(&mut self) {
        pump(&self.sa, &mut self.a, &mut self.ea);
        pump(&self.sb, &mut self.b, &mut self.eb);
    }
}

#[test]
fn links_carry_three_megabytes_over_real_sockets() {
    let mut r = Real::connect();
    let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let mut sent = 0;
    let mut got = Vec::new();
    let mut peak = 0;
    let start = Instant::now();
    while got.len() < payload.len() {
        while sent < payload.len() && r.a.buffered(Channel::Reliable) < 256 * 1024 {
            let end = (sent + 32 * 1024).min(payload.len());
            r.a.send(Channel::Reliable, &payload[sent..end]).unwrap();
            sent = end;
        }
        peak = peak.max(r.a.buffered(Channel::Reliable));
        r.pump();
        for e in r.eb.drain(..) {
            if let LinkEvent::Message { channel: Channel::Reliable, bytes } = e {
                got.extend(bytes);
            }
        }
        assert!(start.elapsed() < Duration::from_secs(5), "3 MB within 5 s");
    }
    eprintln!("3 MB in {:?}, peak buffered {peak}", start.elapsed());
    assert_eq!(got, payload);
}

/// Unreliable 64-byte datagrams at 1 ms intervals all arrive over loopback,
/// none twice.
#[test]
fn unreliable_channel_carries_its_datagrams_over_real_sockets() {
    // 10 000 (criterion 2, about 10 s) in `cargo xtask full`, 2 000 in `check`.
    let full = std::env::var_os("RL_FULL_TESTS").is_some();
    let count: u32 = if full { 10_000 } else { 2_000 };
    let mut r = Real::connect();
    let mut seen = vec![false; count as usize];
    let (mut sent, mut got, mut out_of_order, mut last) = (0u32, 0u32, 0u32, None);
    let start = Instant::now();
    let mut next = start;
    while got < count {
        while sent < count && Instant::now() >= next {
            let mut msg = [0u8; 64];
            msg[..4].copy_from_slice(&sent.to_le_bytes());
            r.a.send(Channel::Unreliable, &msg).unwrap();
            sent += 1;
            next += Duration::from_millis(1);
        }
        r.pump();
        for e in r.eb.drain(..) {
            if let LinkEvent::Message { channel: Channel::Unreliable, bytes } = e {
                assert_eq!(bytes.len(), 64);
                let i = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                assert!(!std::mem::replace(&mut seen[i as usize], true), "datagram {i} arrived twice");
                if last.is_some_and(|l| i < l) {
                    out_of_order += 1;
                }
                last = Some(i);
                got += 1;
            }
        }
        assert!(start.elapsed() < Duration::from_secs(30), "{got} of {count} datagrams within 30 s");
        std::thread::sleep(Duration::from_micros(200));
    }
    eprintln!("{count} datagrams in {:?}, {out_of_order} out of order", start.elapsed());
}
