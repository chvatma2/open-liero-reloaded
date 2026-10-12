//! A link never panics on what a peer sends (spec §5.4): offers, answers
//! and datagrams, garbage or a real one damaged; a forged datagram does not
//! end an open link.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::{Config, TestRunner};
use rl_net::link::{Link, LinkError, LinkEvent};
use rl_netcode::Channel;

fn a() -> SocketAddr {
    "10.0.0.1:5000".parse().unwrap()
}

fn b() -> SocketAddr {
    "10.0.0.2:6000".parse().unwrap()
}

fn config() -> Config {
    Config { cases: 256, failure_persistence: None, ..Config::default() }
}

/// `text` with the byte at `at` replaced by `byte` and cut at `cut`.
fn damage(text: &str, at: Index, byte: u8, cut: Index) -> String {
    let mut bytes = text.as_bytes().to_vec();
    let at = at.index(bytes.len());
    bytes[at] = byte;
    bytes.truncate(cut.index(bytes.len() + 1));
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Polls both links until both are open (virtual time, 1 ms steps, no delay).
fn connect(x: &mut Link, y: &mut Link, now: &mut Instant) {
    for _ in 0..5000 {
        let (mut out, mut events) = (Vec::new(), Vec::new());
        x.poll(*now, &mut out, &mut events);
        for t in out.drain(..) {
            y.receive(t.from, t.to, &t.bytes, *now);
        }
        y.poll(*now, &mut out, &mut events);
        for t in out.drain(..) {
            x.receive(t.from, t.to, &t.bytes, *now);
        }
        if x.is_open() && y.is_open() {
            return;
        }
        *now += Duration::from_millis(1);
    }
    panic!("the links did not connect");
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn an_answerer_survives_any_offer(at in any::<Index>(), byte in any::<u8>(), cut in any::<Index>(), junk in ".{0,300}") {
        let now = Instant::now();
        let (_, offer) = Link::offerer(&[a()], now);
        let _ = Link::answerer(&[b()], &damage(&offer, at, byte, cut), now);
        let _ = Link::answerer(&[b()], &junk, now);
    }

    #[test]
    fn an_offerer_survives_any_answer(at in any::<Index>(), byte in any::<u8>(), cut in any::<Index>(), junk in ".{0,300}") {
        let now = Instant::now();
        let (mut x, offer) = Link::offerer(&[a()], now);
        let (_, answer) = Link::answerer(&[b()], &offer, now).unwrap();
        if x.accept_answer(&damage(&answer, at, byte, cut)).is_ok() {
            let (mut out, mut events) = (Vec::new(), Vec::new());
            x.poll(now + Duration::from_secs(1), &mut out, &mut events);
        }
        let (mut x, _) = Link::offerer(&[a()], now);
        let _ = x.accept_answer(&junk);
    }
}

/// An open link takes any datagram from its peer's address, STUN, DTLS and
/// RTP-looking ones included (the first byte picks the demultiplexing),
/// and stays open: without the keys a forged datagram is dropped.
#[test]
fn an_open_link_survives_any_datagram() {
    let mut now = Instant::now();
    let (mut x, offer) = Link::offerer(&[a()], now);
    let (mut y, answer) = Link::answerer(&[b()], &offer, now).unwrap();
    x.accept_answer(&answer).unwrap();
    connect(&mut x, &mut y, &mut now);
    let y = RefCell::new(y);
    let first = prop_oneof![0u8..2, 20u8..64, 128u8..192, any::<u8>()];
    let datagram = (first, prop::collection::vec(any::<u8>(), 0..1500)).prop_map(|(f, mut rest)| {
        rest.insert(0, f);
        rest
    });
    TestRunner::new(config())
        .run(&datagram, |bytes| {
            let mut y = y.borrow_mut();
            let _ = y.accepts(a(), b(), &bytes);
            y.receive(a(), b(), &bytes, now);
            let (mut out, mut events) = (Vec::new(), Vec::new());
            y.poll(now, &mut out, &mut events);
            y.send(Channel::Reliable, &bytes).unwrap();
            prop_assert!(events.iter().all(|e| !matches!(e, LinkEvent::Closed { .. })), "{events:?}");
            prop_assert!(y.is_open(), "a forged datagram does not end the link");
            Ok(())
        })
        .unwrap();
}

/// A link takes any datagram from an address that is not its peer's, while
/// still connecting as well as once open, and stays usable: such datagrams
/// are not accepted (`accepts` says so) and do not end the link.
#[test]
fn a_link_survives_datagrams_from_strangers_and_while_connecting() {
    let stranger: SocketAddr = "10.0.0.9:7000".parse().unwrap();
    let mut now = Instant::now();
    let (mut x, offer) = Link::offerer(&[a()], now);
    let (mut y, answer) = Link::answerer(&[b()], &offer, now).unwrap();
    let first = prop_oneof![0u8..2, 20u8..64, 128u8..192, any::<u8>()];
    let datagram = (first, prop::collection::vec(any::<u8>(), 0..1500)).prop_map(|(f, mut rest)| {
        rest.insert(0, f);
        rest
    });
    // Still connecting: the offerer has not taken the answer yet.
    let x_cell = RefCell::new(&mut x);
    TestRunner::new(config())
        .run(&(datagram.clone(), any::<bool>()), |(bytes, from_peer)| {
            let mut x = x_cell.borrow_mut();
            let from = if from_peer { b() } else { stranger };
            let _ = x.accepts(from, a(), &bytes);
            x.receive(from, a(), &bytes, now);
            let (mut out, mut events) = (Vec::new(), Vec::new());
            x.poll(now, &mut out, &mut events);
            prop_assert!(events.iter().all(|e| !matches!(e, LinkEvent::Closed { .. })), "{events:?}");
            Ok(())
        })
        .unwrap();
    x.accept_answer(&answer).unwrap();
    connect(&mut x, &mut y, &mut now);
    let y = RefCell::new(y);
    TestRunner::new(config())
        .run(&datagram, |bytes| {
            let mut y = y.borrow_mut();
            prop_assert!(!y.accepts(stranger, b(), &bytes), "a stranger's datagram is not accepted");
            y.receive(stranger, b(), &bytes, now);
            let (mut out, mut events) = (Vec::new(), Vec::new());
            y.poll(now, &mut out, &mut events);
            prop_assert!(events.iter().all(|e| !matches!(e, LinkEvent::Closed { .. })), "{events:?}");
            prop_assert!(y.is_open(), "a stranger's datagram does not end the link");
            Ok(())
        })
        .unwrap();
}

/// An empty message is refused on either channel: str0m would count it
/// without sending a chunk, and the reliable channel would stall behind it.
#[test]
fn an_empty_send_is_harmless() {
    let mut now = Instant::now();
    let (mut x, offer) = Link::offerer(&[a()], now);
    let (mut y, answer) = Link::answerer(&[b()], &offer, now).unwrap();
    x.accept_answer(&answer).unwrap();
    connect(&mut x, &mut y, &mut now);
    for channel in [Channel::Unreliable, Channel::Reliable] {
        assert_eq!(x.send(channel, &[]), Err(LinkError::Empty));
    }
    x.send(Channel::Reliable, b"after").unwrap();
    let mut got = Vec::new();
    for _ in 0..200 {
        let mut out = Vec::new();
        x.poll(now, &mut out, &mut got);
        for t in out.drain(..) {
            y.receive(t.from, t.to, &t.bytes, now);
        }
        y.poll(now, &mut out, &mut got);
        for t in out.drain(..) {
            x.receive(t.from, t.to, &t.bytes, now);
        }
        now += Duration::from_millis(1);
    }
    assert!(x.is_open() && y.is_open(), "{got:?}");
    assert!(got.iter().all(|e| !matches!(e, LinkEvent::Closed { .. })), "{got:?}");
    assert!(
        got.iter().any(|e| matches!(e, LinkEvent::Message { bytes, .. } if bytes == b"after")),
        "the link still carries messages: {got:?}"
    );
}
