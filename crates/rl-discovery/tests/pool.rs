//! The relay pool against the in-process test relay: publishing and
//! receiving over several relays, resubscription, misbehaving and
//! unreachable relays, `wss://`, flooding, silent and dripping relays,
//! unanswered publishes and shutdown.

use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use rl_discovery::*;
use rl_lobby::Identity;
use tungstenite::Message;

fn alice() -> Identity {
    Identity::from_secret([11; 32]).unwrap()
}

fn note(n: u8, content: &str) -> Event {
    Event::sign(
        &alice(),
        1_790_000_000 + u64::from(n),
        1,
        vec![vec!["t".into(), "pool".into()]],
        content.into(),
        [n; 32],
    )
}

fn kind1() -> Vec<Filter> {
    vec![Filter { kinds: vec![1], ..Filter::default() }]
}

fn config(relays: Vec<String>) -> PoolConfig {
    PoolConfig { relays, extra_roots: Vec::new(), max_frame: MAX_FRAME }
}

/// A pool and everything it has reported so far.
struct Watch {
    pool: Pool,
    seen: Vec<PoolEvent>,
}

impl Watch {
    fn new(cfg: PoolConfig) -> Watch {
        Watch { pool: Pool::start(cfg), seen: Vec::new() }
    }

    fn with(cfg: PoolConfig, timings: Timings) -> Watch {
        Watch { pool: Pool::start_with(cfg, timings), seen: Vec::new() }
    }

    /// Polls until `done` holds (true) or `timeout` passes (false). Every
    /// poll must return at once.
    fn until(&mut self, timeout: Duration, done: impl Fn(&[PoolEvent]) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let t = Instant::now();
            self.pool.poll(&mut self.seen);
            assert!(t.elapsed() < Duration::from_millis(200), "poll blocked for {:?}", t.elapsed());
            if done(&self.seen) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn events(&self, id: [u8; 32]) -> usize {
        self.seen.iter().filter(|e| matches!(e, PoolEvent::Event { event, .. } if event.id == id)).count()
    }
}

fn eose(seen: &[PoolEvent], relay: usize, sub: &str) -> usize {
    seen.iter()
        .filter(|e| matches!(e, PoolEvent::Eose { relay: r, sub: s } if *r == relay && s == sub))
        .count()
}

fn ok(seen: &[PoolEvent], relay: usize, id: [u8; 32]) -> bool {
    seen.iter().any(
        |e| matches!(e, PoolEvent::Ok { relay: r, id: i, accepted: true, .. } if *r == relay && *i == id),
    )
}

fn statuses(seen: &[PoolEvent], relay: usize) -> Vec<RelayStatus> {
    seen.iter()
        .filter_map(|e| match e {
            PoolEvent::Status { relay: r, status } if *r == relay => Some(status.clone()),
            _ => None,
        })
        .collect()
}

/// The relay was connected, dropped, and connected again.
fn dropped_and_retried(seen: &[PoolEvent], relay: usize) -> bool {
    let s = statuses(seen, relay);
    s.iter()
        .position(|s| matches!(s, RelayStatus::Dropped(_)))
        .is_some_and(|i| s[..i].contains(&RelayStatus::Connected) && s[i..].contains(&RelayStatus::Connected))
}

fn drop_reason(seen: &[PoolEvent], relay: usize) -> String {
    statuses(seen, relay)
        .into_iter()
        .find_map(|s| match s {
            RelayStatus::Dropped(r) => Some(r),
            _ => None,
        })
        .unwrap_or_default()
}

const LONG: Duration = Duration::from_secs(10);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn has_status(seen: &[PoolEvent], relay: usize, f: impl Fn(&RelayStatus) -> bool) -> bool {
    statuses(seen, relay).iter().any(f)
}

#[test]
fn publishes_and_receives_across_two_relays() {
    let (a, b) = (TestRelay::start(), TestRelay::start());
    // Stored before the pool connects, on both relays.
    let old = note(1, "stored");
    let mut publisher = Watch::new(config(vec![a.url(), b.url()]));
    publisher.pool.publish(old.clone());
    assert!(publisher.until(LONG, |s| ok(s, 0, old.id) && ok(s, 1, old.id)));

    let mut w = Watch::new(config(vec![a.url(), b.url()]));
    w.pool.subscribe("s", kind1());
    assert!(w.until(LONG, |s| eose(s, 0, "s") == 1 && eose(s, 1, "s") == 1));
    assert_eq!(w.events(old.id), 1, "both relays served it; the pool reports it once");

    // Published by the pool itself: accepted by both, echoed by both live
    // subscriptions, reported once.
    let new = note(2, "live");
    w.pool.publish(new.clone());
    assert!(w.until(LONG, |s| ok(s, 0, new.id) && ok(s, 1, new.id)));
    w.until(Duration::from_millis(300), |_| false);
    assert_eq!(w.events(new.id), 1);
    assert!(a.events().contains(&new) && b.events().contains(&new));
    let st = w.pool.statuses();
    assert_eq!(st, vec![(a.url(), RelayStatus::Connected), (b.url(), RelayStatus::Connected)]);
    assert_eq!(w.pool.dropped_events(), 0);
}

#[test]
fn resubscribes_after_a_reconnect() {
    let relay = TestRelay::start();
    let mut w = Watch::new(config(vec![relay.url()]));
    w.pool.subscribe("s", kind1());
    assert!(w.until(LONG, |s| eose(s, 0, "s") == 1));

    // The relay closes the connection at our next frame.
    relay.set_misbehaviour(Misbehaviour::DropAfter(1));
    w.pool.publish(note(1, "lost"));
    assert!(w.until(LONG, |s| statuses(s, 0).iter().any(|s| matches!(s, RelayStatus::Dropped(_)))));
    relay.set_misbehaviour(Misbehaviour::Honest);

    // Reconnected, the subscription is sent again without being asked.
    assert!(w.until(LONG, |s| eose(s, 0, "s") == 2));
    assert!(dropped_and_retried(&w.seen, 0));
    let later = note(2, "after the reconnect");
    let mut publisher = Watch::new(config(vec![relay.url()]));
    publisher.pool.publish(later.clone());
    assert!(w.until(LONG, |s| {
        s.iter().any(|e| matches!(e, PoolEvent::Event { event, .. } if event.id == later.id))
    }));
    assert_eq!(w.events(later.id), 1);
}

#[test]
fn a_misbehaving_relay_is_dropped_and_retried() {
    // Garbage: a frame that is not a Nostr message at all.
    let garbage = TestRelay::start();
    garbage.set_misbehaviour(Misbehaviour::Garbage);
    // NeverEose: no EOSE within the EOSE window (2 s here, 10 s by default).
    let silent = TestRelay::start();
    silent.set_misbehaviour(Misbehaviour::NeverEose);
    // An oversized frame: a stored event larger than the pool's `max_frame`.
    let big = TestRelay::start();
    let huge = note(3, &"x".repeat(2048));
    let mut publisher = Watch::new(config(vec![big.url()]));
    publisher.pool.publish(huge.clone());
    assert!(publisher.until(LONG, |s| ok(s, 0, huge.id)));

    let mut cfg = config(vec![garbage.url(), silent.url(), big.url()]);
    cfg.max_frame = 1024;
    let mut w = Watch::with(cfg, Timings { eose: Duration::from_secs(2), ..Timings::default() });
    w.pool.subscribe("s", kind1());
    assert!(w.until(Duration::from_secs(20), |s| (0..3).all(|r| dropped_and_retried(s, r))), "{:?}", w.seen);
    assert!(drop_reason(&w.seen, 0).contains("not a Nostr message"), "{}", drop_reason(&w.seen, 0));
    assert!(drop_reason(&w.seen, 1).contains("EOSE"), "{}", drop_reason(&w.seen, 1));
    assert!(drop_reason(&w.seen, 2).contains("too large"), "{}", drop_reason(&w.seen, 2));
    assert_eq!(w.events(huge.id), 0);
}

/// A relay that dies in the middle of a frame (half a frame, then the
/// connection closes) is dropped and retried, and nothing of the frame
/// arrives; over `ws://` and `wss://`.
#[test]
fn a_relay_dying_mid_frame_is_dropped_and_retried() {
    let plain = TestRelay::start();
    plain.set_misbehaviour(Misbehaviour::HalfFrame);
    let (tls, root) = TestRelay::start_tls();
    tls.set_misbehaviour(Misbehaviour::HalfFrame);
    let mut cfg = config(vec![plain.url(), tls.url()]);
    cfg.extra_roots = vec![root];
    let mut w = Watch::new(cfg);
    w.pool.subscribe("s", kind1());
    assert!(w.until(Duration::from_secs(20), |s| (0..2).all(|r| dropped_and_retried(s, r))), "{:?}", w.seen);
    assert!(!w.seen.iter().any(|e| matches!(e, PoolEvent::Event { .. })));
}

#[test]
fn an_unreachable_relay_never_blocks_the_caller() {
    // A listener that never accepts: the TCP connect succeeds (backlog),
    // the handshake is never answered. Held for the whole test, so no other
    // test can take the port.
    let mute = TcpListener::bind("127.0.0.1:0").unwrap();
    let mute_port = mute.local_addr().unwrap().port();
    // TEST-NET-1 (RFC 5737): never routed, so the connect hangs or fails.
    let relays = vec![format!("ws://127.0.0.1:{mute_port}"), "ws://192.0.2.1:9".to_string()];
    let t = Instant::now();
    let mut w = Watch::new(config(relays));
    w.pool.subscribe("s", kind1());
    w.pool.publish(note(1, "nobody hears this"));
    assert!(t.elapsed() < Duration::from_millis(100), "start, subscribe and publish took {:?}", t.elapsed());
    let failed = |s: &[PoolEvent], r| statuses(s, r).iter().any(|s| matches!(s, RelayStatus::Failed(_)));
    assert!(w.until(Duration::from_secs(12), |s| failed(s, 0) && failed(s, 1)), "{:?}", w.seen);
    assert!(has_status(
        &w.seen,
        0,
        |s| matches!(s, RelayStatus::Failed(r) if r.contains("handshake timed out"))
    ));
    let t = Instant::now();
    drop(w);
    assert!(t.elapsed() < Duration::from_millis(2500), "drop took {:?}", t.elapsed());
    drop(mute);

    // Dropped in the middle of a hanging connect: bounded all the same.
    let w = Watch::new(config(vec!["ws://192.0.2.1:9".to_string()]));
    thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    drop(w);
    assert!(t.elapsed() < Duration::from_millis(2500), "drop took {:?}", t.elapsed());
}

#[test]
fn wss_with_a_test_root() {
    let (relay, root) = TestRelay::start_tls();
    let mut cfg = config(vec![relay.url()]);
    cfg.extra_roots = vec![root];
    let mut w = Watch::new(cfg);
    w.pool.subscribe("s", kind1());
    let e = note(1, "over TLS");
    w.pool.publish(e.clone());
    assert!(w.until(LONG, |s| ok(s, 0, e.id) && eose(s, 0, "s") == 1), "{:?}", w.seen);
    assert!(
        w.until(LONG, |s| s.iter().any(|x| matches!(x, PoolEvent::Event { event, .. } if event.id == e.id)))
    );

    // Without the root, the certificate is refused.
    let mut untrusting = Watch::new(config(vec![relay.url()]));
    assert!(untrusting.until(LONG, |s| statuses(s, 0).iter().any(|s| matches!(s, RelayStatus::Failed(_)))));
    assert!(!statuses(&untrusting.seen, 0).contains(&RelayStatus::Connected));
}

/// A relay that floods: 3000 tiny text frames as soon as it is connected.
fn flooding_relay() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = flood(stream);
    });
    (port, h)
}

fn flood(stream: TcpStream) -> tungstenite::Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut ws = tungstenite::accept(stream).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(e) => e,
        tungstenite::HandshakeError::Interrupted(_) => std::io::Error::from(ErrorKind::WouldBlock).into(),
    })?;
    for _ in 0..3000 {
        ws.send(Message::text(r#"["NOTICE","flood"]"#))?;
    }
    thread::sleep(Duration::from_millis(500));
    Ok(())
}

#[test]
fn a_flooding_relay_is_dropped() {
    let (port, h) = flooding_relay();
    let mut w = Watch::new(config(vec![format!("ws://127.0.0.1:{port}")]));
    assert!(w.until(LONG, |s| !drop_reason(s, 0).is_empty()), "{:?}", w.seen);
    assert!(drop_reason(&w.seen, 0).contains("frames/s"), "{}", drop_reason(&w.seen, 0));
    drop(w);
    h.join().unwrap();
}

#[test]
fn dropping_the_pool_sends_what_is_queued_and_closes() {
    let relay = TestRelay::start();
    let mut w = Watch::new(config(vec![relay.url()]));
    assert!(w.until(LONG, |s| statuses(s, 0).contains(&RelayStatus::Connected)));
    // A closing note published just before the pool goes away still goes out.
    let last = note(9, "closed");
    w.pool.publish(last.clone());
    let t = Instant::now();
    drop(w);
    assert!(t.elapsed() < Duration::from_millis(2500), "drop took {:?}", t.elapsed());
    let deadline = Instant::now() + Duration::from_secs(2);
    while !relay.events().contains(&last) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(relay.events().contains(&last));
}

/// Liveness timings short enough for a test.
fn quick_ping() -> Timings {
    Timings { idle_ping: ms(500), ping_answer: ms(500), ..Timings::default() }
}

#[test]
fn a_silent_relay_is_dropped_after_an_unanswered_ping() {
    let (silent, idle) = (TestRelay::start(), TestRelay::start());
    let mut w = Watch::with(config(vec![silent.url(), idle.url()]), quick_ping());
    assert!(w.until(LONG, |s| statuses(s, 0).contains(&RelayStatus::Connected)
        && statuses(s, 1).contains(&RelayStatus::Connected)));
    silent.set_misbehaviour(Misbehaviour::Silent);
    let t = Instant::now();
    assert!(w.until(LONG, |s| !drop_reason(s, 0).is_empty()), "{:?}", w.seen);
    assert_eq!(drop_reason(&w.seen, 0), "no answer to ping");
    // 500 ms idle, then 500 ms for the answer.
    assert!(t.elapsed() >= ms(900) && t.elapsed() < ms(3000), "{:?}", t.elapsed());
    silent.set_misbehaviour(Misbehaviour::Honest);
    assert!(w.until(LONG, |s| dropped_and_retried(s, 0)));
    // An idle relay that answers pings stays connected all along.
    assert_eq!(statuses(&w.seen, 1), vec![RelayStatus::Connecting, RelayStatus::Connected]);
}

#[test]
fn a_dripping_relay_is_dropped_within_its_deadlines() {
    // In the handshake: the 1 s handshake deadline holds (tungstenite's
    // own attack check would only trip after 64 reads, 2.6 s here).
    let early = TestRelay::start();
    early.set_misbehaviour(Misbehaviour::Drip);
    // On an established connection: a frame that never completes is no
    // answer to a ping.
    let late = TestRelay::start();
    let t0 = Instant::now();
    let mut w =
        Watch::with(config(vec![early.url(), late.url()]), Timings { handshake: ms(1000), ..quick_ping() });
    assert!(w.until(LONG, |s| has_status(s, 0, |s| matches!(s, RelayStatus::Failed(_)))));
    assert!(t0.elapsed() < ms(2000), "{:?}", t0.elapsed());
    assert!(
        has_status(&w.seen, 0, |s| matches!(s, RelayStatus::Failed(r) if r == "handshake timed out")),
        "{:?}",
        statuses(&w.seen, 0)
    );
    assert!(!statuses(&w.seen, 0).contains(&RelayStatus::Connected));

    assert!(w.until(LONG, |s| statuses(s, 1).contains(&RelayStatus::Connected)));
    late.set_misbehaviour(Misbehaviour::Drip);
    let t = Instant::now();
    assert!(w.until(LONG, |s| !drop_reason(s, 1).is_empty()), "{:?}", w.seen);
    assert_eq!(drop_reason(&w.seen, 1), "no answer to ping");
    assert!(t.elapsed() < ms(3000), "{:?}", t.elapsed());
}

#[test]
fn dropping_the_pool_is_bounded_while_a_relay_drips() {
    // One relay drips its handshake, the other a frame on an established
    // connection; the drivers still notice the stop at once.
    let (early, late) = (TestRelay::start(), TestRelay::start());
    early.set_misbehaviour(Misbehaviour::Drip);
    let mut w = Watch::new(config(vec![early.url(), late.url()]));
    assert!(w.until(LONG, |s| statuses(s, 1).contains(&RelayStatus::Connected)));
    w.pool.subscribe("s", kind1());
    assert!(w.until(LONG, |s| eose(s, 1, "s") == 1));
    late.set_misbehaviour(Misbehaviour::Drip);
    // A frame of the late relay is now half-read.
    w.until(ms(300), |_| false);
    let t = Instant::now();
    drop(w);
    // Well inside the 2 s bound: the threads were joined, not detached.
    assert!(t.elapsed() < ms(1000), "drop took {:?}", t.elapsed());
}

#[test]
fn an_unanswered_publish_times_out_and_keeps_the_connection() {
    let relay = TestRelay::start();
    let mut w = Watch::with(config(vec![relay.url()]), Timings { ok: ms(1000), ..Timings::default() });
    assert!(w.until(LONG, |s| statuses(s, 0).contains(&RelayStatus::Connected)));
    // The relay answers 1.5 s late: after the 1 s OK window.
    relay.set_misbehaviour(Misbehaviour::Slow(ms(1500)));
    let e = note(1, "slow");
    w.pool.publish(e.clone());
    let oks = |s: &[PoolEvent]| -> Vec<(bool, String)> {
        s.iter()
            .filter_map(|x| match x {
                PoolEvent::Ok { relay: 0, id, accepted, message } if *id == e.id => {
                    Some((*accepted, message.clone()))
                }
                _ => None,
            })
            .collect()
    };
    assert!(w.until(LONG, |s| !oks(s).is_empty()));
    assert_eq!(oks(&w.seen), vec![(false, "no answer within 1 s".to_string())]);
    // The late answer is not reported, and the connection stays.
    w.until(ms(1500), |_| false);
    assert_eq!(oks(&w.seen).len(), 1);
    assert_eq!(statuses(&w.seen, 0), vec![RelayStatus::Connecting, RelayStatus::Connected]);
    assert!(relay.events().contains(&e));
}

/// 256 events of 120 KiB: more than loopback's socket buffers hold.
fn flood_of_big_notes() -> Vec<Event> {
    let body = "y".repeat(120 << 10);
    (0..=255u8).map(|n| note(n, &body)).collect()
}

#[test]
fn a_stalled_send_drops_the_connection_and_never_holds_up_a_drop() {
    let (a, b) = (TestRelay::start(), TestRelay::start());
    let mut wa = Watch::new(config(vec![a.url()]));
    let mut wb = Watch::new(config(vec![b.url()]));
    assert!(wa.until(LONG, |s| statuses(s, 0).contains(&RelayStatus::Connected)));
    assert!(wb.until(LONG, |s| statuses(s, 0).contains(&RelayStatus::Connected)));
    // Both relays stop reading; the pools send 30 MiB.
    a.set_misbehaviour(Misbehaviour::Silent);
    b.set_misbehaviour(Misbehaviour::Silent);
    let notes = flood_of_big_notes();
    let t = Instant::now();
    for e in &notes {
        wa.pool.publish(e.clone());
        wb.pool.publish(e.clone());
    }
    // The sends of one turn share a 5 s deadline.
    assert!(wa.until(LONG, |s| !drop_reason(s, 0).is_empty()), "{:?}", statuses(&wa.seen, 0));
    assert_eq!(drop_reason(&wa.seen, 0), "send timed out");
    assert!(t.elapsed() >= ms(4500) && t.elapsed() < ms(8000), "{:?}", t.elapsed());
    // B is still stuck in its sends: dropping it is bounded all the same.
    let t = Instant::now();
    drop(wb);
    assert!(t.elapsed() < ms(1500), "drop took {:?}", t.elapsed());
}

#[test]
fn a_relay_that_reads_slowly_gets_every_frame_intact() {
    // 64 events of 120 KiB (7.7 MB) per relay, read 1 KiB at a time with
    // pauses: the pool's socket buffers fill, and its writes are partial
    // and retried. Every frame must still arrive whole: the relay verifies
    // each event (id and signature) before it accepts it.
    let plain = TestRelay::start();
    let (tls, root) = TestRelay::start_tls();
    plain.set_misbehaviour(Misbehaviour::SlowReads);
    tls.set_misbehaviour(Misbehaviour::SlowReads);
    let mut cfg = config(vec![plain.url(), tls.url()]);
    cfg.extra_roots = vec![root];
    let mut w = Watch::new(cfg);
    assert!(w.until(LONG, |s| (0..2).all(|r| statuses(s, r).contains(&RelayStatus::Connected))));
    let body = "z".repeat(120 << 10);
    let notes: Vec<Event> = (0..64u8).map(|n| note(n, &body)).collect();
    for e in &notes {
        w.pool.publish(e.clone());
    }
    let all_ok = |s: &[PoolEvent]| (0..2).all(|r| notes.iter().all(|e| ok(s, r, e.id)));
    assert!(
        w.until(Duration::from_secs(20), all_ok),
        "{:?} {:?}",
        statuses(&w.seen, 0),
        statuses(&w.seen, 1)
    );
    for relay in [&plain, &tls] {
        let stored = relay.events();
        assert_eq!(stored.len(), notes.len());
        assert!(stored.iter().all(|e| e.verify() && notes.contains(e)));
    }
    for r in 0..2 {
        assert_eq!(statuses(&w.seen, r), vec![RelayStatus::Connecting, RelayStatus::Connected]);
    }
}
