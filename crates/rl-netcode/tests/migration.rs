//! Host migration in the session (spec §3.7): three sessions over a wire the
//! test controls, in 1 ms steps. Who takes over and who rejoins whom is the
//! drivers' part (the harness, rl-online); here the test does it by hand.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use rl_netcode::harness::Factory;
use rl_netcode::migration::{HOST_SILENT_US, fit_report};
use rl_netcode::toy::ToyFactory;
use rl_netcode::{
    Channel, Config, Driver, Event, MAX_PREDICTION, PeerId, Role, Seat, Session, ToySim, input_delay,
};
use rl_proto::{
    Control, Datagram, Frame, InputsReport, MAX_CONTROL, MAX_REPORT_FRAMES, MAX_REPORT_OWN, decode_control,
    decode_datagram, encode_control, encode_frames,
};
use rl_sim::Keys;

const H: PeerId = 0; // the first host: slot 0 its player, slot 1 its bot
const A: PeerId = 1; // slot 2: the first successor
const B: PeerId = 2; // slot 3
const SLOTS: u8 = 4;

struct Mesh {
    s: BTreeMap<PeerId, Session<ToySim>>,
    d: BTreeMap<PeerId, Box<dyn Driver<ToySim>>>,
    /// Not alive: no advance, nothing received (a crash).
    alive: BTreeMap<PeerId, bool>,
    /// Paused: no advance; what arrives waits (a frozen game whose socket fills).
    paused: BTreeSet<PeerId>,
    inbox: BTreeMap<PeerId, Vec<(PeerId, Channel, Vec<u8>)>>,
    one_way_ms: u64,
    /// `(from, to)` → from this ms on, that direction loses everything.
    cut: BTreeMap<(PeerId, PeerId), u64>,
    now_ms: u64,
    wire: Vec<(u64, PeerId, PeerId, Channel, Vec<u8>)>,
    /// Every message sent (not cut), with the time: (ms, from, to, channel, bytes).
    sent: Vec<(u64, PeerId, PeerId, Channel, Vec<u8>)>,
    events: BTreeMap<PeerId, Vec<(u64, Event)>>,
}

impl Mesh {
    fn new(one_way_ms: u64, seed: u64) -> Mesh {
        let mut f = ToyFactory { slots: SLOTS, seed };
        let delay = input_delay(2 * one_way_ms as u32);
        let sim = || ToySim::new(SLOTS as usize);
        let host = Role::Host { peers: vec![(A, Seat::Player(2)), (B, Seat::Player(3))] };
        let mut s = BTreeMap::new();
        s.insert(H, Session::new(Config::new(SLOTS, 0), host, sim(), 0));
        s.insert(A, Session::new(Config::new(SLOTS, delay), Role::Client { host: H, slot: 2 }, sim(), 0));
        s.insert(B, Session::new(Config::new(SLOTS, delay), Role::Client { host: H, slot: 3 }, sim(), 0));
        let mut d = BTreeMap::new();
        d.insert(H, f.driver(H, None));
        d.insert(A, f.driver(A, Some(Seat::Player(2))));
        d.insert(B, f.driver(B, Some(Seat::Player(3))));
        Mesh {
            s,
            d,
            alive: [(H, true), (A, true), (B, true)].into(),
            paused: BTreeSet::new(),
            inbox: BTreeMap::new(),
            one_way_ms,
            cut: BTreeMap::new(),
            now_ms: 0,
            wire: Vec::new(),
            sent: Vec::new(),
            events: BTreeMap::new(),
        }
    }

    fn alive(&self, id: PeerId) -> bool {
        self.alive.get(&id).copied().unwrap_or(false)
    }

    fn step(&mut self) {
        self.now_ms += 1;
        let (now, us) = (self.now_ms, self.now_ms * 1000);
        let (due, later): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.wire).into_iter().partition(|m| m.0 <= now);
        self.wire = later;
        for (_, from, to, ch, bytes) in due {
            if self.paused.contains(&to) {
                self.inbox.entry(to).or_default().push((from, ch, bytes));
            } else if self.alive(to) {
                self.s.get_mut(&to).unwrap().receive(from, ch, &bytes, us);
            }
        }
        let ids: Vec<PeerId> = self.s.keys().copied().collect();
        for id in ids {
            if !self.alive(id) || self.paused.contains(&id) {
                continue;
            }
            for (from, ch, bytes) in self.inbox.remove(&id).unwrap_or_default() {
                self.s.get_mut(&id).unwrap().receive(from, ch, &bytes, us);
            }
            let s = self.s.get_mut(&id).unwrap();
            s.advance(us, self.d.get_mut(&id).unwrap().as_mut());
            let mut out = Vec::new();
            s.transmit(&mut |to, ch, b| out.push((to, ch, b)));
            for e in s.drain_events() {
                self.events.entry(id).or_default().push((now, e));
            }
            for (to, ch, b) in out {
                if self.cut.get(&(id, to)).is_some_and(|&from| now >= from) {
                    continue;
                }
                self.sent.push((now, id, to, ch, b.clone()));
                self.wire.push((now + self.one_way_ms, id, to, ch, b));
            }
        }
    }

    fn run_ms(&mut self, ms: u64) {
        for _ in 0..ms {
            self.step();
        }
    }

    fn until(&mut self, limit_ms: u64, what: &str, done: impl Fn(&Mesh) -> bool) {
        let end = self.now_ms + limit_ms;
        while !done(self) {
            assert!(self.now_ms < end, "{what} within {limit_ms} ms");
            self.step();
        }
    }

    fn first(&self, id: PeerId, f: impl Fn(&Event) -> bool) -> Option<u64> {
        self.events.get(&id)?.iter().find(|(_, e)| f(e)).map(|(t, _)| *t)
    }

    fn count(&self, id: PeerId, f: impl Fn(&Event) -> bool) -> usize {
        self.events.get(&id).map_or(0, |v| v.iter().filter(|(_, e)| f(e)).count())
    }

    fn known(&self, id: PeerId) -> Vec<u32> {
        (0..SLOTS).map(|s| self.s[&id].input_log().known_upto(s)).collect()
    }

    /// A takes over (slot 1 is the bot it now plays) and B rejoins it, as the drivers do.
    fn migrate_to_a(&mut self) {
        let us = self.now_ms * 1000;
        for id in [A, B] {
            self.s.get_mut(&id).unwrap().suspend();
        }
        self.s.get_mut(&A).unwrap().take_over(&[1], us);
        self.s.get_mut(&B).unwrap().rehost(A, us);
        assert!(self.s.get_mut(&A).unwrap().rejoined(B, Seat::Player(3), us));
    }
}

#[test]
fn a_silent_host_is_reported_after_two_seconds_and_heard_again() {
    let mut m = Mesh::new(20, 1);
    m.run_ms(3_000);
    m.alive.insert(H, false);
    let gone = m.now_ms;
    m.run_ms(2_500);
    let silent_ms = HOST_SILENT_US / 1000;
    for id in [A, B] {
        let at = m.first(id, |e| *e == Event::HostSilent).expect("HostSilent");
        // Its last datagram left at `gone` at the latest and arrived one way later.
        assert!(at >= gone + silent_ms && at <= gone + m.one_way_ms + silent_ms + 20, "peer {id} at {at}");
        assert_eq!(m.count(id, |e| *e == Event::HostSilent), 1, "once per silence");
        assert_eq!(m.count(id, |e| matches!(e, Event::PeerTimedOut { .. })), 0, "not yet 10 s");
    }
    m.alive.insert(H, true); // a frozen host whose clock ran: it simply sends again
    m.run_ms(500);
    for id in [A, B] {
        assert_eq!(m.count(id, |e| *e == Event::HostHeard), 1, "peer {id} heard it again");
    }
}

#[test]
fn a_peers_own_sleep_is_not_its_hosts_silence() {
    let mut m = Mesh::new(20, 2);
    m.run_ms(2_000);
    // B's game stops for 3 s and everything the host sent meanwhile is lost (its socket overflowed).
    m.paused.insert(B);
    m.cut.insert((H, B), m.now_ms);
    m.run_ms(3_000);
    m.cut.remove(&(H, B));
    m.inbox.remove(&B);
    m.paused.remove(&B);
    m.run_ms(1_000);
    assert_eq!(m.count(B, |e| *e == Event::HostSilent), 0, "the host was never silent while B ran");
}

#[test]
fn a_suspended_session_runs_no_tick_sends_no_inputs_and_never_stalls() {
    let mut m = Mesh::new(20, 3);
    m.run_ms(1_000);
    m.s.get_mut(&B).unwrap().suspend();
    assert!(m.s[&B].suspended());
    let (tick, from_ms) = (m.s[&B].current_tick(), m.now_ms);
    m.run_ms(1_000);
    assert_eq!(m.s[&B].current_tick(), tick);
    // Only frame-less keepalives: about 4 a second, none carrying a key.
    let datagrams: Vec<&Vec<u8>> = m
        .sent
        .iter()
        .filter(|(t, from, _, ch, _)| *t > from_ms && *from == B && *ch == Channel::Unreliable)
        .map(|(.., b)| b)
        .collect();
    assert!((3..=5).contains(&datagrams.len()), "{} datagrams in 1 s", datagrams.len());
    for b in datagrams {
        match decode_datagram(b, SLOTS as usize) {
            Ok(Datagram::Client(c)) => assert!(c.keys.is_empty(), "no input frames"),
            other => panic!("not a keepalive: {other:?}"),
        }
    }
    assert_eq!(m.count(B, |e| matches!(e, Event::Stalled { .. })), 0);
    m.s.get_mut(&B).unwrap().unsuspend(m.now_ms * 1000);
    m.run_ms(200);
    assert!(m.s[&B].current_tick() > tick, "it runs again");
}

#[test]
fn the_successor_takes_what_a_peer_knows_and_both_converge() {
    let mut m = Mesh::new(10, 4);
    m.run_ms(3_800);
    // The host's last 200 ms reach B only: B knows more than A.
    m.cut.insert((H, A), m.now_ms);
    m.run_ms(200);
    m.alive.insert(H, false);
    m.until(2_500, "A and B find the host silent", |m| {
        [A, B].iter().all(|&id| m.first(id, |e| *e == Event::HostSilent).is_some())
    });
    let (a_knew, b_knew) = (m.known(A), m.known(B));
    assert!(
        (0..SLOTS as usize).any(|s| b_knew[s] > a_knew[s] + 10),
        "B knows more: {b_knew:?}, A {a_knew:?}"
    );
    let b_values: Vec<(u32, u8, Option<Keys>)> = (0..SLOTS)
        .flat_map(|s| (a_knew[s as usize]..b_knew[s as usize]).map(move |t| (t, s)))
        .map(|(t, s)| (t, s, m.s[&B].final_input(t, s)))
        .collect();
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    for &(t, s, v) in &b_values {
        assert_eq!(m.s[&A].final_input(t, s), v, "slot {s} tick {t}: B's final value taken");
    }
    let r = m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    m.until(1_000, "B resumes", |m| m.first(B, |e| matches!(e, Event::Resumed { .. })).is_some());
    assert!(m.first(B, |e| *e == Event::Resumed { tick: r }).is_some(), "B resumes at the new host's tick");
    m.run_ms(5_000);
    let (a, b) = (&m.s[&A], &m.s[&B]);
    let upto = a.frontier().min(b.frontier());
    assert!(upto > r + 400, "the match went on: {upto} past {r}");
    for t in 0..upto {
        assert_eq!(a.input_log().row(t), b.input_log().row(t), "tick {t}");
    }
    // The old host's player stands still from its known prefix (N6); its bot plays on at A.
    let from = a_knew[0].max(b_knew[0]);
    assert!((from..upto).all(|t| a.final_input(t, 0) == Some(Keys(0))));
    assert!((r..upto).any(|t| a.final_input(t, 1) != Some(Keys(0))), "the bot plays at the new host");
    // Confirmed checksums agree, and nobody saw a desync, a conflict or a malformed message.
    let sums: BTreeMap<u32, u64> = a.checksums().iter().copied().collect();
    let common: Vec<&(u32, u64)> = b.checksums().iter().filter(|(t, _)| sums.contains_key(t)).collect();
    assert!(common.len() > 40, "{} common checksums", common.len());
    for (t, h) in common {
        assert_eq!(sums[t], *h, "checksum of tick {t}");
    }
    for id in [A, B] {
        let bad = |e: &Event| {
            matches!(e, Event::Desync { .. } | Event::Inconsistent { .. } | Event::Malformed { .. })
        };
        assert_eq!(m.count(id, bad), 0, "peer {id}");
    }
    assert_eq!(a.stats().migrations, 1);
    assert_eq!(b.stats().migrations, 1);
    assert_eq!(a.stats().report_conflicts, 0);
}

#[test]
fn a_collecting_new_host_never_looks_silent() {
    let mut m = Mesh::new(10, 9);
    m.run_ms(2_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    // A waits 3 s more before it resumes (a dead expected peer holds its collection open).
    m.run_ms(3_000);
    assert_eq!(
        m.count(B, |e| *e == Event::HostSilent),
        1,
        "the new host's keepalives keep B from a second migration"
    );
    m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    m.until(1_000, "B resumes", |m| m.first(B, |e| matches!(e, Event::Resumed { .. })).is_some());
}

#[test]
fn a_worst_case_report_is_trimmed_to_fit() {
    // 32 slots, every slot changing every tick, 3000 ticks: about 120 KB untrimmed.
    let value = |t: u32, s: usize| Some(((t as usize + s) % 2) as u8 + 1);
    let upto = 1_000 + MAX_REPORT_FRAMES as u32;
    let report =
        fit_report(1_000, 1_000, upto, 32, &value, &|_| Some(0), 1_000, vec![1; MAX_REPORT_OWN + 10]);
    let bytes = encode_control(&Control::Report(report.clone()));
    assert!(bytes.len() <= MAX_CONTROL, "{} bytes", bytes.len());
    assert!(
        report.frames.len() >= 100 && report.frames.len() < MAX_REPORT_FRAMES,
        "kept {} ticks",
        report.frames.len()
    );
    assert_eq!(report.own.len(), MAX_REPORT_OWN);
    let Ok(Control::Report(back)) = decode_control(&bytes) else { panic!("it decodes") };
    let mut got = Vec::new();
    rl_proto::decode_frames(back.first, &back.frames, 32, &|_| Some(0), &mut |t, s, v| got.push((t, s, v)))
        .unwrap();
    assert_eq!(got.first(), Some(&(1_000, 0, value(1_000, 0).unwrap())), "the earliest ticks are kept");
    assert!(got.iter().all(|&(t, s, v)| Some(v) == value(t, s)));
    // A short report is not trimmed.
    let short = fit_report(0, 1_000, 1_010, 32, &value, &|_| Some(0), 1_000, vec![]);
    assert_eq!(short.frames.len(), 10);
}

#[test]
fn a_far_resume_is_ignored() {
    let mut m = Mesh::new(10, 10);
    m.run_ms(1_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    let us = m.now_ms * 1000;
    let far = m.s[&B].current_tick() + 6_001;
    m.s.get_mut(&B).unwrap().receive(
        A,
        Channel::Reliable,
        &encode_control(&Control::Resume { tick: far }),
        us,
    );
    assert!(m.s[&B].suspended(), "a Resume 6001 ticks ahead is ignored");
}

#[test]
fn a_rejoined_player_is_not_corrected_for_inputs_it_reported() {
    let mut m = Mesh::new(30, 5);
    m.run_ms(4_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    let before = m.s[&B].stats().own_corrections;
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    m.run_ms(3_000);
    assert_eq!(m.s[&B].stats().own_corrections, before, "its pressed keys became final as pressed");
}

#[test]
fn a_conflicting_report_is_ignored_and_counted() {
    let mut m = Mesh::new(10, 6);
    m.run_ms(3_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(A, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    let us = m.now_ms * 1000;
    // A forged spectator rejoins and claims another value for slot 0 at tick 100.
    assert!(m.s.get_mut(&A).unwrap().rejoined(9, Seat::Spectator, us));
    let real = m.s[&A].final_input(100, 0).unwrap().0;
    let base: Vec<Option<u8>> = (0..SLOTS).map(|s| m.s[&A].final_input(99, s).map(|k| k.0)).collect();
    let frames = encode_frames(100, 1, SLOTS as usize, &|_, s| (s == 0).then_some(real ^ 0x10), &|s| base[s]);
    let report = InputsReport { ack: 0, first: 100, frames, own_first: 0, own: vec![] };
    m.s.get_mut(&A).unwrap().receive(9, Channel::Reliable, &encode_control(&Control::Report(report)), us);
    assert_eq!(m.s[&A].final_input(100, 0), Some(Keys(real)), "the first value stays");
    assert_eq!(m.s[&A].stats().report_conflicts, 1);
    assert_eq!(m.count(A, |e| matches!(e, Event::Malformed { .. })), 0);
}

#[test]
fn migration_messages_from_a_non_host_are_ignored() {
    let mut m = Mesh::new(10, 7);
    m.run_ms(1_000);
    let us = m.now_ms * 1000;
    // B, a client of a live host: A's Collect and Resume are not its host's.
    let sent = m.s[&B].stats().datagrams_out;
    for c in [Control::Collect { known: vec![0; SLOTS as usize] }, Control::Resume { tick: 5 }] {
        m.s.get_mut(&B).unwrap().receive(A, Channel::Reliable, &encode_control(&c), us);
    }
    let mut out = Vec::new();
    m.s.get_mut(&B).unwrap().transmit(&mut |to, ch, b| out.push((to, ch, b)));
    assert!(out.iter().all(|(to, ..)| *to == H), "nothing to A");
    assert_eq!(m.s[&B].stats().datagrams_out, sent);
    assert!(!m.s[&B].suspended());
    // The host: a report from a peer that never rejoined changes nothing.
    let row = m.s[&H].input_log().row(50);
    let frames = encode_frames(50, 1, SLOTS as usize, &|_, _| Some(0x7f), &|_| Some(0));
    let report = InputsReport { ack: 0, first: 50, frames, own_first: 0, own: vec![] };
    m.s.get_mut(&H).unwrap().receive(B, Channel::Reliable, &encode_control(&Control::Report(report)), us);
    assert_eq!(m.s[&H].input_log().row(50), row);
    assert_eq!(m.s[&H].stats().report_conflicts, 0);
    for id in [H, B] {
        assert_eq!(m.count(id, |e| matches!(e, Event::Malformed { .. })), 0);
    }
}

#[test]
fn a_new_host_stalled_past_an_absent_players_prefix_runs_on() {
    // Every peer stalls `MAX_PREDICTION` ticks past the dead host's last
    // frames: the new host resumes that far past the old host's player,
    // who never comes back.
    let mut m = Mesh::new(30, 5);
    m.run_ms(4_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    let frontier = m.s[&A].frontier();
    let r = m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    assert_eq!(r - frontier, MAX_PREDICTION, "resumed at the prediction limit");
    m.run_ms(1_000);
    for id in [A, B] {
        assert!(m.s[&id].frontier() > r + 50, "peer {id} confirms on: {} past {r}", m.s[&id].frontier());
    }
}

#[test]
fn a_spectator_reports_or_takes_over() {
    const S: PeerId = 3;
    for spectator_hosts in [false, true] {
        let mut m = Mesh::new(10, 14);
        let host =
            Role::Host { peers: vec![(A, Seat::Player(2)), (B, Seat::Player(3)), (S, Seat::Spectator)] };
        m.s.insert(H, Session::new(Config::new(SLOTS, 0), host, ToySim::new(SLOTS as usize), 0));
        m.s.insert(
            S,
            Session::new(Config::new(SLOTS, 0), Role::Spectator { host: H }, ToySim::new(SLOTS as usize), 0),
        );
        m.d.insert(S, ToyFactory { slots: SLOTS, seed: 14 }.driver(S, Some(Seat::Spectator)));
        m.alive.insert(S, true);
        m.run_ms(2_000);
        m.alive.insert(H, false);
        m.until(2_500, "silence", |m| {
            [A, B, S].iter().all(|&id| m.first(id, |e| *e == Event::HostSilent).is_some())
        });
        let us = m.now_ms * 1000;
        let (new, others) = if spectator_hosts {
            (S, [(A, Seat::Player(2)), (B, Seat::Player(3))])
        } else {
            (A, [(B, Seat::Player(3)), (S, Seat::Spectator)])
        };
        for id in [A, B, S] {
            m.s.get_mut(&id).unwrap().suspend();
        }
        m.s.get_mut(&new).unwrap().take_over(&[1], us);
        for (id, seat) in others {
            m.s.get_mut(&id).unwrap().rehost(new, us);
            assert!(m.s.get_mut(&new).unwrap().rejoined(id, seat, us));
        }
        m.until(1_000, "the reports", |m| m.s[&new].awaiting_reports().is_empty());
        let r = m.s.get_mut(&new).unwrap().resume(m.now_ms * 1000);
        m.run_ms(2_000);
        let upto = [A, B, S].iter().map(|id| m.s[id].frontier()).min().unwrap();
        assert!(upto > r + 100, "spectator hosts {spectator_hosts}: the match went on: {upto} past {r}");
        for id in [A, B, S] {
            for t in 0..upto {
                assert_eq!(m.s[&id].input_log().row(t), m.s[&new].input_log().row(t), "peer {id} tick {t}");
            }
            let bad = |e: &Event| {
                matches!(e, Event::Desync { .. } | Event::Inconsistent { .. } | Event::Malformed { .. })
            };
            assert_eq!(m.count(id, bad), 0, "peer {id}");
            assert_eq!(m.s[&id].stats().migrations, 1, "peer {id}");
        }
        // A spectator presses nothing: hosting, it plays the bot alone.
        assert!((r..upto).any(|t| m.s[&new].final_input(t, 1) != Some(Keys(0))), "the bot plays");
    }
}

/// Remembers every key its peer pressed: `(tick, slot)` → key, and the
/// tick being simulated when it was pressed (`at`).
struct Recording {
    inner: Box<dyn Driver<ToySim>>,
    pressed: std::rc::Rc<std::cell::RefCell<BTreeMap<(u32, u8), u8>>>,
    at: std::rc::Rc<std::cell::RefCell<BTreeMap<(u32, u8), u32>>>,
}

impl Driver<ToySim> for Recording {
    fn local_inputs(&mut self, sim: &ToySim, tick: u32, slots: &[u8], out: &mut [Keys]) {
        self.inner.local_inputs(sim, tick, slots, out);
        let (mut pressed, mut at) = (self.pressed.borrow_mut(), self.at.borrow_mut());
        for (&s, k) in slots.iter().zip(out.iter()) {
            pressed.insert((tick, s), k.0);
            at.insert((tick, s), sim.tick_count());
        }
    }
}

#[test]
fn the_resume_fills_bots_with_their_last_keys_and_its_own_slot_with_its_pressed_keys() {
    let mut m = Mesh::new(30, 12);
    let (pressed, at) = (std::rc::Rc::default(), std::rc::Rc::default());
    let inner = m.d.remove(&A).unwrap();
    let recording = Recording { inner, pressed: std::rc::Rc::clone(&pressed), at: std::rc::Rc::clone(&at) };
    m.d.insert(A, Box::new(recording));
    m.run_ms(4_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    let known = m.known(A);
    let r = m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    assert!(known[1] + 10 < r && known[2] + 10 < r, "ticks to fill: {known:?} below {r}");
    let last = m.s[&A].final_input(known[1] - 1, 1);
    assert!((known[1]..r).all(|t| m.s[&A].final_input(t, 1) == last), "the bot holds its keys");
    for t in known[2]..r {
        assert_eq!(
            m.s[&A].final_input(t, 2).map(|k| k.0),
            pressed.borrow().get(&(t, 2)).copied(),
            "tick {t}"
        );
    }
    // After the resume its own slot keeps the match's input delay (N2): the
    // keys pressed for a tick, `d` ticks ahead, are its final input.
    m.run_ms(2_000);
    let upto = m.s[&A].frontier();
    assert!(upto > r + 150, "the match went on: {upto} past {r}");
    let pressed = pressed.borrow();
    let finals: Vec<Option<u8>> = (r..upto).map(|t| m.s[&A].final_input(t, 2).map(|k| k.0)).collect();
    let keys: Vec<Option<u8>> = (r..upto).map(|t| pressed.get(&(t, 2)).copied()).collect();
    assert_eq!(finals, keys, "ticks {r}..{upto}");
    assert!(keys.windows(2).any(|w| w[0] != w[1]), "its keys change");
    let d = m.s[&A].config().input_delay as u32;
    assert!(d > 0);
    let at = at.borrow();
    assert!((r + d..upto).all(|t| at.get(&(t, 2)) == Some(&(t - d))), "pressed {d} ticks ahead");
}

#[test]
fn a_report_after_the_resume_is_taken() {
    let mut m = Mesh::new(30, 15);
    m.run_ms(4_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    // A resumes before B's report: it arrives late, own inputs and all.
    let us = m.now_ms * 1000;
    m.s.get_mut(&A).unwrap().resume(us);
    assert_eq!(m.s[&A].awaiting_reports(), vec![B]);
    let from = m.s[&A].input_log().known_upto(3);
    let own = vec![0x21, 0x22, 0x23];
    let report = InputsReport { ack: 0, first: 0, frames: vec![], own_first: from, own: own.clone() };
    m.s.get_mut(&A).unwrap().receive(B, Channel::Reliable, &encode_control(&Control::Report(report)), us);
    assert!(m.s[&A].awaiting_reports().is_empty());
    let got: Vec<Option<Keys>> = (from..from + 3).map(|t| m.s[&A].final_input(t, 3)).collect();
    assert_eq!(
        got,
        own.into_iter().map(|k| Some(Keys(k))).collect::<Vec<_>>(),
        "they extend B's slot at once"
    );
}

#[test]
fn a_silent_hosts_bye_is_no_return() {
    let mut m = Mesh::new(10, 16);
    m.run_ms(1_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(A, |e| *e == Event::HostSilent).is_some());
    let us = m.now_ms * 1000;
    m.s.get_mut(&A).unwrap().receive(H, Channel::Reliable, &encode_control(&Control::Bye), us);
    assert_eq!(m.s.get_mut(&A).unwrap().drain_events(), vec![Event::PeerLeft { peer: H }]);
}

#[test]
fn a_rejoined_peer_gets_what_it_lacks_from_its_reports_ack() {
    let mut m = Mesh::new(10, 13);
    m.run_ms(3_800);
    // The host's last 200 ms reach A only: B knows less than its new host.
    m.cut.insert((H, B), m.now_ms);
    m.run_ms(200);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    let b_frontier = m.s[&B].frontier();
    assert!(b_frontier + 10 < m.s[&A].frontier(), "B lacks some: {b_frontier} of {}", m.s[&A].frontier());
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    // The keepalives measure the round trip: a short one alone would start
    // the frames at the latest change, not at what B lacks.
    m.run_ms(600);
    assert!(m.s[&A].peer_rtt_ms(B).is_some_and(|r| r <= 100));
    let at = m.now_ms;
    m.s.get_mut(&A).unwrap().resume(at * 1000);
    m.run_ms(50);
    let first = m
        .sent
        .iter()
        .filter(|(t, from, to, ch, _)| *t >= at && *from == A && *to == B && *ch == Channel::Unreliable)
        .find_map(|(.., b)| match decode_datagram(b, SLOTS as usize) {
            Ok(Datagram::Host(h)) if !h.frames.is_empty() => Some(h.first),
            _ => None,
        });
    assert_eq!(first, Some(b_frontier), "the first frames start at its ack");
}

/// The decoder cannot check a report's `known` bits, or a `Collect`'s length,
/// against the match's slots: the session refuses both.
#[test]
fn a_report_or_collect_beyond_the_match_slots_is_refused() {
    let mut m = Mesh::new(10, 11);
    m.run_ms(2_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(A, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    let us = m.now_ms * 1000;
    assert!(m.s.get_mut(&A).unwrap().rejoined(9, Seat::Spectator, us));
    // Slot 3's next value, which alone would extend its prefix, and slot 4 of 4.
    let before = m.known(A);
    let first = before[3];
    let frames = vec![Frame { known: 0b1_1000, changed: 0b1_1000, keys: vec![0x55, 0x66] }];
    let report = InputsReport { ack: 0, first, frames, own_first: 0, own: vec![] };
    m.s.get_mut(&A).unwrap().receive(9, Channel::Reliable, &encode_control(&Control::Report(report)), us);
    assert_eq!(m.known(A), before, "nothing taken from it");
    assert_eq!(m.s[&A].final_input(first, 3), None);
    assert_eq!(m.s.get_mut(&A).unwrap().drain_events(), vec![Event::Malformed { peer: 9 }]);
    assert!(!m.s[&A].awaiting_reports().contains(&9), "it reported: no wait for another");
    // A Collect for five slots of four is no question B can answer.
    let collect = Control::Collect { known: vec![0; SLOTS as usize + 1] };
    m.s.get_mut(&B).unwrap().receive(A, Channel::Reliable, &encode_control(&collect), us);
    let mut out = Vec::new();
    m.s.get_mut(&B).unwrap().transmit(&mut |to, ch, b| out.push((to, ch, b)));
    assert!(out.iter().all(|(_, ch, _)| *ch != Channel::Reliable), "no report");
    assert_eq!(m.s.get_mut(&B).unwrap().drain_events(), vec![Event::Malformed { peer: A }]);
}

fn control() -> impl Strategy<Value = Control> {
    let edge = prop_oneof![Just(0u32), Just(1), Just(99), Just(u32::MAX - 1), Just(u32::MAX), 0u32..5_000];
    let frame = (any::<u32>(), any::<u32>(), prop::collection::vec(any::<u8>(), 0..33))
        .prop_map(|(known, changed, keys)| Frame { known, changed, keys });
    prop_oneof![
        prop::collection::vec(edge.clone(), 0..40).prop_map(|known| Control::Collect { known }),
        (
            edge.clone(),
            edge.clone(),
            prop::collection::vec(frame, 0..80),
            edge.clone(),
            prop::collection::vec(any::<u8>(), 0..300)
        )
            .prop_map(|(ack, first, frames, own_first, own)| Control::Report(InputsReport {
                ack,
                first,
                frames,
                own_first,
                own
            })),
        edge.prop_map(|tick| Control::Resume { tick }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn migration_controls_never_panic(
        to_host in prop::collection::vec(control(), 1..12),
        to_client in prop::collection::vec(control(), 1..12),
    ) {
        let mut m = Mesh::new(10, 8);
        m.run_ms(600);
        m.alive.insert(H, false);
        m.run_ms(2_100);
        m.migrate_to_a();
        let us = m.now_ms * 1000;
        let confirmed = m.s[&A].frontier();
        let rows: Vec<_> = (0..confirmed).map(|t| m.s[&A].input_log().row(t)).collect();
        for c in &to_host {
            m.s.get_mut(&A).unwrap().receive(B, Channel::Reliable, &encode_control(c), us);
        }
        for c in &to_client {
            m.s.get_mut(&B).unwrap().receive(A, Channel::Reliable, &encode_control(c), us);
        }
        m.run_ms(300);
        for t in 0..confirmed {
            prop_assert_eq!(m.s[&A].input_log().row(t), rows[t as usize], "tick {}", t);
        }
    }
}

// The harness's migrations (Task 3): the drivers' part, played over the
// network model, every rule in virtual time.

use rl_netcode::harness::{
    self, Exit, ExitHow, HOST, MigrationPlan, Outage, PeerPlan, ProfileChange, Report, Scenario,
};
use rl_netcode::migration::{GIVE_UP_US, STEP_DOWN_US, WINDOW_US};
use rl_netcode::netsim::Profile;

fn plan(id: u32, seat: Seat, profile: Profile) -> PeerPlan {
    PeerPlan {
        id,
        seat,
        profile,
        clock_ppm: 0,
        input_delay: if matches!(seat, Seat::Player(_)) { input_delay(profile.rtt_ms()) } else { 0 },
        frame_us: 6_944,
        join_tick: None,
        freeze: None,
    }
}

/// A host (slot 0 its player, slot 1 its bot), `clients` players and one spectator, migration on.
fn mscenario(seed: u64, profile: Profile, clients: u8) -> Scenario {
    let mut peers: Vec<PeerPlan> =
        (0..clients).map(|c| plan(1 + c as u32, Seat::Player(2 + c), profile)).collect();
    peers.push(plan(100, Seat::Spectator, profile));
    Scenario {
        seed,
        slots: clients + 2,
        peers,
        ticks: 1_500,
        settle_ticks: 200,
        outages: vec![],
        expect_no_stalls: false,
        corrupt: vec![],
        host_freeze: None,
        exits: vec![],
        migration: Some(MigrationPlan { host_human: Some(0), slow_joins: vec![] }),
        profile_changes: vec![],
    }
}

fn run_checked(sc: &Scenario) -> Report {
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(sc, &mut f).unwrap_or_else(|e| panic!("seed {}: {e:?}", sc.seed));
    harness::verdict(sc, &r, &mut f)
        .unwrap_or_else(|e| panic!("seed {} {:?}: {e:?}", sc.seed, sc.peers[0].profile.name));
    r
}

#[test]
fn a_crashed_or_leaving_host_is_replaced_in_every_profile() {
    for profile in Profile::ALL {
        for seed in 1..=12 {
            for how in [ExitHow::Crash, ExitHow::Leave] {
                let mut sc = mscenario(seed, profile, 3);
                sc.exits.push(Exit { peer: HOST, tick: 300 + (seed as u32 * 37) % 400, how });
                let r = run_checked(&sc);
                assert_eq!(r.hosts, vec![HOST, 1], "seed {seed} {how:?}");
                assert_eq!(r.migrations.len(), 1);
                let m = &r.migrations[0];
                let took = m.resumed_us - m.silent_us;
                let fast = matches!(profile.name, "lan" | "good" | "typical");
                assert!(
                    took <= if fast { 3_000_000 } else { WINDOW_US },
                    "seed {seed} {how:?} {}: {took} µs",
                    profile.name
                );
                assert!(r.host_lost.is_empty() && !r.exited.contains(&1));
            }
        }
    }
}

#[test]
fn a_leaving_host_is_followed_without_the_two_second_wait() {
    let mut sc = mscenario(3, Profile::GOOD, 2);
    sc.exits.push(Exit { peer: HOST, tick: 500, how: ExitHow::Leave });
    let r = run_checked(&sc);
    let left_us = 500 * 10_000;
    assert!(r.migrations[0].silent_us < left_us + 200_000, "{:?}", r.migrations[0]);
}

#[test]
fn the_second_successor_takes_over_when_the_first_is_dead() {
    let mut sc = mscenario(5, Profile::TYPICAL, 3);
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 400, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 2]);
    let m = &r.migrations[0];
    assert!(m.resumed_us >= m.silent_us + WINDOW_US, "the first window passed first");
}

#[test]
fn nobody_reachable_is_host_lost_after_ten_seconds() {
    // One client and a spectator that joined mid-match (a follower): the client dies with the host.
    let mut sc = mscenario(6, Profile::GOOD, 1);
    sc.peers[1].join_tick = Some(100);
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 400, how: ExitHow::Crash });
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    assert_eq!(r.host_lost, [100].into());
    assert!(r.events[&100].contains(&Event::HostSilent));
    assert!(r.migrations.is_empty());
    // It gave up GIVE_UP_US after its migration began: about 4 s + 2 s + 10 s into the run.
    assert!(
        r.duration_us >= 4_000_000 + 2_000_000 + GIVE_UP_US
            && r.duration_us < 4_000_000 + 2_000_000 + GIVE_UP_US + 500_000
    );
}

#[test]
fn a_host_frozen_past_the_step_down_threshold_hands_over() {
    let mut sc = mscenario(7, Profile::GOOD, 2);
    // 1.7 s: past the step-down, below HOST_SILENT_US: only the step-down can move the match.
    sc.host_freeze = Some((600, 600 + (STEP_DOWN_US / 10_000) as u32 + 20));
    let r = run_checked(&sc);
    assert!(r.exited.contains(&HOST), "the host stepped down");
    assert!(
        r.events[&1].iter().all(|e| *e != Event::HostSilent),
        "the peers followed its Bye, not a silence"
    );
    assert_eq!(r.hosts, vec![HOST, 1]);
}

#[test]
fn a_host_frozen_briefly_keeps_hosting() {
    let mut sc = mscenario(8, Profile::GOOD, 2);
    sc.host_freeze = Some((600, 600 + (STEP_DOWN_US / 10_000) as u32 - 40));
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST]);
    assert!(r.exited.is_empty() && r.migrations.is_empty());
}

#[test]
fn a_client_outage_does_not_move_the_match() {
    let mut sc = mscenario(9, Profile::TYPICAL, 3);
    sc.outages.push(Outage { peer: 3, from_tick: 500, until_tick: 800 }); // 3 s
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST]);
    assert!(r.events[&3].contains(&Event::HostSilent) && r.events[&3].contains(&Event::HostHeard));
    assert!(r.host_lost.is_empty(), "it returned to its host (N69)");
}

#[test]
fn a_successor_outage_does_not_take_the_match() {
    let mut sc = mscenario(10, Profile::TYPICAL, 3);
    sc.outages.push(Outage { peer: 1, from_tick: 500, until_tick: 800 });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST]);
    assert!(r.events.values().flatten().all(|e| !matches!(e, Event::Resumed { .. })));
    assert!(r.host_lost.is_empty(), "it returned to its host (N69)");
}

#[test]
fn a_late_peer_watches_from_a_state() {
    let mut sc = mscenario(11, Profile::GOOD, 3);
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    // Peer 3's join to the successor takes 7 s after contact (slow relays):
    // the successor resumes without it at its window's end.
    sc.migration.as_mut().unwrap().slow_joins.push((3, 7_000_000));
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
    let m = &r.migrations[0];
    assert!(m.resumed_us >= m.silent_us + WINDOW_US - 100_000, "it waited for peer 3 until its window's end");
    assert!(r.events[&3].iter().any(|e| matches!(e, Event::StateLoaded { .. })), "it watches from a state");
    assert!(!r.host_lost.contains(&3));
    // Its slot (4) stays Keys(0) from the resume on.
    assert!(r.host_log[m.tick as usize + 50..].iter().all(|row| row[4] == 0));
}

#[test]
fn a_second_migration_moves_the_match_again() {
    let mut sc = mscenario(12, Profile::TYPICAL, 3);
    sc.ticks = 2_500;
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 1_500, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1, 2]);
    assert_eq!(r.migrations.len(), 2);
}

#[test]
fn the_fifth_migration_in_a_match_is_host_lost() {
    // N74: six clients; the host, then each new host, crashes in turn.
    let mut sc = mscenario(16, Profile::LAN, 6);
    sc.ticks = 4_000;
    for (k, peer) in [HOST, 1, 2, 3, 4].into_iter().enumerate() {
        sc.exits.push(Exit { peer, tick: 300 + 600 * k as u32, how: ExitHow::Crash });
    }
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    assert_eq!(r.hosts, vec![HOST, 1, 2, 3, 4], "four migrations");
    assert_eq!(r.host_lost, [5, 6, 100].into(), "the fifth is host lost");
}

#[test]
fn a_spectator_takes_over_when_no_player_can() {
    let mut sc = mscenario(13, Profile::LAN, 0); // the host and a spectator only
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 100]);
    let after = &r.host_log[600..900];
    assert!(after.iter().any(|row| row[1] != 0), "the bot plays at the spectator");
}

#[test]
fn profiles_change_mid_run() {
    let mut sc = mscenario(14, Profile::GOOD, 3);
    sc.profile_changes.push(ProfileChange { tick: 300, peer: 2, profile: Profile::AWFUL });
    sc.profile_changes.push(ProfileChange { tick: 900, peer: 2, profile: Profile::GOOD });
    sc.exits.push(Exit { peer: HOST, tick: 600, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
    // The change at 900 reached peer 2's link to its new host: it lost far
    // fewer datagrams than with awful links to the end.
    sc.profile_changes.pop();
    let awful = run_checked(&sc);
    let (got, all) = (r.received[&2].dropped, awful.received[&2].dropped);
    assert!(got * 4 < all * 3, "{got} dropped, {all} with awful links to the end");
}

#[test]
fn a_successor_nobody_joined_in_its_window_takes_no_one_after_it() {
    // D8: peers 2 and 100 reach successor 1 at once, but their joins take
    // 6 s: it closes at its window's end, uncommitted, and they follow
    // the next window (peer 2's own), which peer 1 joins in time.
    let mut sc = mscenario(17, Profile::GOOD, 2);
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    sc.migration.as_mut().unwrap().slow_joins.extend([(2, 6_000_000), (100, 6_000_000)]);
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 2]);
    assert_eq!(r.host_lost, [100].into(), "its join to peer 2 would end past the give-up");
}

#[test]
fn an_exited_hosts_last_rows_that_reached_nobody_are_not_the_match() {
    // B3: nothing passes between the host and its peers for its last
    // 100 ms; it confirms rows (its clients' slots filled) nobody gets.
    let mut sc = mscenario(18, Profile::GOOD, 3);
    for peer in [1, 2, 3, 100] {
        sc.outages.push(Outage { peer, from_tick: 390, until_tick: 450 });
    }
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
    let old = &r.logs[&HOST];
    assert!(old.iter().zip(&r.host_log).any(|(a, b)| a != b), "the exited host's last rows differ");
}

#[test]
fn the_verdict_finds_every_value_a_survivor_held() {
    let mut sc = mscenario(19, Profile::GOOD, 3);
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    let mut r = run_checked(&sc);
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    // A value a survivor held beyond its frontier, changed: lost.
    let h = r.held.iter_mut().find(|h| h.peer == 2).expect("peer 2 rejoined");
    let v = h.slots[3].last_mut().expect("it knew slot 3");
    *v ^= 1;
    let e = harness::verdict(&sc, &r, &mut f).expect_err("a held value the last host lacks");
    assert!(e.what.contains("held"), "{e:?}");
}

#[test]
fn migration_runs_are_deterministic() {
    let mut sc = mscenario(15, Profile::BAD, 3);
    sc.exits.push(Exit { peer: HOST, tick: 500, how: ExitHow::Crash });
    let (a, b) = (run_checked(&sc), run_checked(&sc));
    assert_eq!(a.host_log, b.host_log);
    assert_eq!(a.migrations, b.migrations);
    assert_eq!(a.checksums, b.checksums);
}
