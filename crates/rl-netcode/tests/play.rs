//! What real play needs of a session (plan 10): leaving cleanly, one `Late`
//! per datagram, the lagging client's own warning, stalls counted per
//! episode, and repairs timed from the client's load. A host and one client
//! over a link the test controls (`common::Pair`), or a client alone.

use crate::common;

use common::{CLIENT, Fate, HOST, Pair};
use rl_netcode::harness::Factory;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{Channel, Config, Driver, Event, Role, Seat, Session, TARGET_MARGIN, ToySim};
use rl_proto::{Control, Datagram, HostInputs, Stamp, decode_control, encode_control, encode_datagram};

fn client() -> (Session<ToySim>, Box<dyn Driver<ToySim>>) {
    let s = Session::new(Config::new(3, 1), Role::Client { host: HOST, slot: 2 }, ToySim::new(3), 0);
    (s, ToyFactory { slots: 3, seed: 1 }.driver(CLIENT, Some(Seat::Player(2))))
}

/// The host's datagram with every slot's input `0` for ticks `first..first + n`.
fn frames(first: u32, n: u32) -> Vec<u8> {
    frames_of(first, n, 0, 0)
}

/// The host's datagram with every slot's input `v` for ticks `first..first
/// + n`, encoded against `base` at `first - 1`.
fn frames_of(first: u32, n: u32, v: u8, base: u8) -> Vec<u8> {
    let h = HostInputs {
        stamp: Stamp::default(),
        tick: first + n,
        your_ack: 0,
        margin: TARGET_MARGIN as i16,
        first,
        frames: rl_proto::encode_frames(first, n, 3, &|_, _| Some(v), &|_| Some(base)),
        gone: vec![],
    };
    encode_datagram(&Datagram::Host(h))
}

#[test]
fn a_client_that_leaves_is_dropped_at_once() {
    let mut p = Pair::new(3, 2, 20, 1);
    while p.host.current_tick() < 300 {
        p.step();
    }
    let before = p.client_sent.len();
    p.client.leave();
    p.transmit_client();
    let left_ms = p.now_ms;
    // Until the host hears of it: where the client's inputs end.
    let mut known = p.host.input_log().known_upto(2);
    while !p.host_events.iter().any(|e| e.1 == Event::PeerLeft { peer: CLIENT }) {
        assert!(p.now_ms < left_ms + 1_000, "the host never heard the Bye");
        known = p.host.input_log().known_upto(2);
        p.step();
    }
    // From the next tick on, the slot is `Keys(0)` without waiting for the fill window.
    let current = p.host.current_tick();
    assert!(p.host.input_log().known_upto(2) >= current, "filled at once");
    p.run_ms(12_000);
    let log = p.host.input_log();
    assert!(known > 250 && (0..known).any(|t| log.get(t, 2) != Some(0)), "the client played");
    for t in known..p.host.frontier() {
        assert_eq!(log.get(t, 2), Some(0), "tick {t}");
    }
    let lefts = p.host_events.iter().filter(|e| matches!(e.1, Event::PeerLeft { .. })).count();
    assert_eq!(lefts, 1);
    assert!(p.host_events.iter().all(|e| !matches!(e.1, Event::PeerTimedOut { .. })), "no timeout");
    // The client sent its Bye, then nothing.
    let after = &p.client_sent[before..];
    assert_eq!(after.len(), 1, "{} messages after leaving", after.len());
    assert_eq!(after[0].1, Channel::Reliable);
    assert_eq!(decode_control(&after[0].2), Ok(Control::Bye));
    assert_eq!(p.host.stats().fills, 0, "the host never waited for the fill window");
}

#[test]
fn a_host_that_leaves_says_so() {
    let mut p = Pair::new(3, 2, 20, 1);
    p.run_ms(1_000);
    p.host.leave();
    p.run_ms(12_000);
    let lefts: Vec<_> = p.client_events.iter().filter(|e| matches!(e.1, Event::PeerLeft { .. })).collect();
    assert_eq!(lefts.len(), 1);
    assert_eq!(lefts[0].1, Event::PeerLeft { peer: HOST });
    assert!(p.client_events.iter().all(|e| !matches!(e.1, Event::PeerTimedOut { .. })));
}

#[test]
fn late_answers_are_one_per_datagram() {
    // The client's uplink is cut for 1 s: the host fills its slot, then
    // answers the inputs that arrive for filled ticks.
    let mut p = Pair::new(3, 2, 20, 2);
    p.up = Box::new(|ms, _, _| if (2_000..3_000).contains(&ms) { Fate::Drop } else { Fate::Pass });
    p.run_ms(6_000);
    assert!(p.host.stats().fills > 50);
    let most = p.lates_per_datagram.iter().copied().max().unwrap();
    assert!(most <= 1, "one datagram got {most} Late answers");
    let lates: Vec<(u32, u32)> = p
        .client_events
        .iter()
        .filter_map(|e| match e.1 {
            Event::Late { from, to } => Some((from, to)),
            _ => None,
        })
        .collect();
    assert!(!lates.is_empty());
    assert!(lates.iter().all(|&(from, to)| from < to), "{lates:?}");
    let answered: usize = p.lates_per_datagram.iter().sum();
    assert_eq!(answered, lates.len());
}

/// The `Lagging` events a client raised, with the time in ms.
fn lagging(events: &[(u64, Event)]) -> Vec<(u64, bool)> {
    events
        .iter()
        .filter_map(|e| match e.1 {
            Event::Lagging(on) => Some((e.0, on)),
            _ => None,
        })
        .collect()
}

/// A client told every 100 ms, until `until_ms`, that ten of its ticks
/// were filled; its events with the time in ms, over `ms`.
fn told_late(until_ms: u64, ms: u64) -> Vec<(u64, Event)> {
    let (mut c, mut drv) = client();
    let mut events = Vec::new();
    for now in 1..=ms {
        if now <= until_ms && now.is_multiple_of(100) {
            let to = (now / 10) as u32;
            let late = encode_control(&Control::Late { from: to - 10, to });
            c.receive(HOST, Channel::Reliable, &late, now * 1000);
        }
        c.advance(now * 1000, drv.as_mut());
        events.extend(c.drain_events().into_iter().map(|e| (now, e)));
    }
    events
}

#[test]
fn a_client_late_for_three_seconds_is_lagging() {
    // 300 ticks filled within 4 s: lagging; 2 s without a `Late`: no more.
    let events = told_late(5_000, 8_000);
    assert_eq!(lagging(&events), vec![(3_000, true), (7_000, false)]);
    // 250 ticks are not enough.
    assert_eq!(lagging(&told_late(2_500, 8_000)), vec![]);
}

#[test]
fn a_stall_counts_once_per_episode() {
    // No host: the client predicts `MAX_PREDICTION` ticks and waits.
    let (mut c, mut drv) = client();
    for ms in 1..=1_000u64 {
        c.advance(ms * 1000, drv.as_mut());
    }
    assert_eq!(c.stats().stalls, 1);
    // The host confirms 40 ticks: it runs on, then waits again.
    c.receive(HOST, Channel::Unreliable, &frames(0, 40), 1_000_000);
    for ms in 1_001..=2_000u64 {
        c.advance(ms * 1000, drv.as_mut());
    }
    assert_eq!(c.stats().stalls, 2);
    let stalled: Vec<Event> =
        c.drain_events().into_iter().filter(|e| matches!(e, Event::Stalled { .. })).collect();
    assert_eq!(stalled, vec![Event::Stalled { tick: 28 }, Event::Stalled { tick: 68 }]);
}

/// A pair whose client desyncs at the host's tick 500, with `down` deciding
/// the host's reliable messages from its first `StateBegin` on (the
/// time it went out, the message).
fn desynced(down: impl FnMut(u64, u64, &Control) -> Fate + 'static) -> Pair {
    let mut p = Pair::new(3, 2, 15, 3);
    let mut down = down;
    let mut state_at = None;
    p.down = Box::new(move |ms, ch, bytes| {
        if ch != Channel::Reliable {
            return Fate::Pass;
        }
        let Ok(c) = decode_control(bytes) else { return Fate::Pass };
        if matches!(c, Control::StateBegin { .. }) && state_at.is_none() {
            state_at = Some(ms);
        }
        match state_at {
            Some(at) => down(ms, at, &c),
            None => Fate::Pass,
        }
    });
    while p.host.current_tick() < 500 {
        p.step();
    }
    p.corrupt_client();
    p
}

fn host_count(p: &Pair, f: impl Fn(&Event) -> bool) -> usize {
    p.host_events.iter().filter(|e| f(&e.1)).count()
}

#[test]
fn a_repair_waits_for_the_load() {
    // The state takes 5 s to arrive (a multi-MB state over a slow link):
    // the client's reports differ all along, yet it is not sent again.
    let mut p = desynced(|ms, at, _| if ms < at + 5_000 { Fate::Hold } else { Fate::Pass });
    p.run_ms(10_000);
    assert_eq!(host_count(&p, |e| matches!(e, Event::Desync { .. })), 1);
    assert_eq!(host_count(&p, |e| matches!(e, Event::StateSent { .. })), 1);
    assert_eq!(host_count(&p, |e| matches!(e, Event::RepairFailed { .. })), 0);
    let loaded = p
        .client_events
        .iter()
        .find_map(|e| match e.1 {
            Event::StateLoaded { tick } => Some(tick),
            _ => None,
        })
        .expect("the state loaded");
    // Repaired: every checksum after the state agrees.
    let after = |v: &[(u32, u64)]| v.iter().filter(|(t, _)| *t > loaded).copied().collect::<Vec<_>>();
    let (h, c) = (after(p.host.checksums()), after(p.client.checksums()));
    assert!(c.len() > 50, "{} checksums after the state", c.len());
    assert_eq!(&h[..c.len()], &c[..]);
}

#[test]
fn a_state_never_loaded_fails_the_repair_after_a_minute() {
    let mut p = desynced(|_, _, _| Fate::Drop);
    let mut failed = None;
    while p.now_ms < 70_000 && failed.is_none() {
        p.step();
        failed = p.host_events.iter().find(|e| matches!(e.1, Event::RepairFailed { .. })).map(|e| e.0);
    }
    let sent = p.host_events.iter().find(|e| matches!(e.1, Event::StateSent { .. })).unwrap().0;
    let failed = failed.expect("the repair failed");
    assert!((sent + 60_000..sent + 60_100).contains(&failed), "sent at {sent} ms, failed at {failed} ms");
    assert_eq!(host_count(&p, |e| matches!(e, Event::Desync { .. })), 1);
    assert_eq!(host_count(&p, |e| matches!(e, Event::StateSent { .. })), 1);
}

#[test]
fn the_host_measures_each_peers_round_trip() {
    let mut p = Pair::new(3, 2, 20, 1);
    assert_eq!(p.host.peer_rtt_ms(CLIENT), None, "unmeasured at first");
    p.run_ms(2_000);
    let rtt = p.host.peer_rtt_ms(CLIENT).expect("measured");
    assert!((38..=60).contains(&rtt), "a 40 ms round trip measured as {rtt} ms");
    assert_eq!(p.host.stats().rtt_ms, Some(rtt), "the largest of one");
    assert_eq!(p.host.peer_rtt_ms(7), None, "not a peer");
}

/// Ruling SK-4 (net-soak's `typical-11s6c0v-seed3117434416`): a peer back
/// from an outage gets its backlog one `MAX_FRAMES` datagram a tick, each
/// chained to the one before (its frames start where that one's end). A
/// datagram overtaking its predecessor (jitter) began past what the peer
/// knew and was dropped, and so was every later one: the chain stood until
/// the host's resend a round trip later, the peer stalled at the
/// prediction limit meanwhile. A datagram ahead of what we know waits for
/// the one before it.
#[test]
fn a_backlog_datagram_that_overtakes_its_predecessor_is_kept() {
    let (mut c, _) = client();
    c.receive(HOST, Channel::Unreliable, &frames(128, 64), 1_000_000);
    c.receive(HOST, Channel::Unreliable, &frames(64, 64), 1_001_000);
    assert_eq!(c.frontier(), 0, "nothing before tick 64 yet");
    c.receive(HOST, Channel::Unreliable, &frames(0, 64), 1_002_000);
    assert_eq!(c.frontier(), 192, "the overtaking datagrams follow the one they waited for");
    assert_eq!(c.stats().malformed, 0);
}

/// Final review A-M2: a datagram with no frames (a new host's collection
/// keepalive, or one with nothing new) starting past the frontier is not
/// kept: it took one of the `AHEAD_KEPT` places and sorted before the real
/// backlog datagrams, pushing them out.
#[test]
fn a_datagram_without_frames_is_never_kept_ahead() {
    let (mut c, _) = client();
    // Sixteen frame-less datagrams ahead, then the backlog's second datagram.
    for k in 0..16 {
        c.receive(HOST, Channel::Unreliable, &frames(65 + k, 0), 1_000_000 + u64::from(k));
    }
    c.receive(HOST, Channel::Unreliable, &frames(128, 64), 1_100_000);
    c.receive(HOST, Channel::Unreliable, &frames(64, 64), 1_101_000);
    c.receive(HOST, Channel::Unreliable, &frames(0, 64), 1_102_000);
    assert_eq!(c.frontier(), 192, "the backlog's datagrams were kept, not the empty ones");
}

/// Ruling FR12c-2 (final review A-I1): frames kept ahead of the frontier
/// are the old host's. rl-online's new host has the old one's peer id
/// (`HOST`), so frames kept across the rehost were taken as the new host's
/// once the frontier reached them: the old host's values made final, then
/// the new host's own `Inconsistent`. A suspension and a rehost drop them;
/// a rehost also drops what was queued for the old host.
#[test]
fn frames_kept_ahead_of_the_old_host_are_dropped_at_a_rehost() {
    let (mut c, mut d) = client();
    c.receive(HOST, Channel::Unreliable, &frames(0, 10), 1_000_000);
    // A datagram for ticks 20..30 (value 7) overtook the one for 10..20.
    c.receive(HOST, Channel::Unreliable, &frames_of(20, 10, 7, 7), 1_001_000);
    assert_eq!(c.frontier(), 10);
    c.suspend();
    // Its keepalive, queued for the old host.
    c.advance(1_300_000, d.as_mut());
    c.rehost(HOST, 2_000_000);
    let mut sent = 0;
    c.transmit(&mut |_, _, _| sent += 1);
    assert_eq!(sent, 0, "nothing queued for the old host goes to the new one");
    // The new host resumes at 20 with ticks 10..20 (value 1) ...
    let resume = Control::Resume {
        tick: 20,
        at_ms: 0,
        first: 10,
        frames: rl_proto::encode_frames(10, 10, 3, &|_, _| Some(1), &|_| Some(0)),
        gone: vec![],
    };
    c.receive(HOST, Channel::Reliable, &encode_control(&resume), 2_100_000);
    assert_eq!(c.frontier(), 20);
    // ... and its first datagram has no frames: nothing past 20 is final.
    c.receive(HOST, Channel::Unreliable, &frames_of(20, 0, 0, 1), 2_200_000);
    assert_eq!(c.final_input(25, 0), None, "the old host's kept frames are not the new host's");
    // Its own frames for 20..30 (value 2) are taken, and agree.
    c.receive(HOST, Channel::Unreliable, &frames_of(20, 10, 2, 1), 2_210_000);
    assert_eq!(c.final_input(25, 0), Some(rl_sim::Keys(2)));
    let events = c.drain_events();
    assert!(!events.iter().any(|e| matches!(e, Event::Inconsistent { .. })), "{events:?}");
}
