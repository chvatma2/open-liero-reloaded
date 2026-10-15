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
    /// That direction and channel loses everything.
    lost: BTreeSet<(PeerId, PeerId, Channel)>,
    /// That direction and channel takes this many ms longer (reliable
    /// messages stay in order: a retransmission's wait).
    later: BTreeMap<(PeerId, PeerId, Channel), u64>,
    /// The last reliable arrival per direction.
    last_reliable: BTreeMap<(PeerId, PeerId), u64>,
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
            lost: BTreeSet::new(),
            later: BTreeMap::new(),
            last_reliable: BTreeMap::new(),
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
                if self.cut.get(&(id, to)).is_some_and(|&from| now >= from)
                    || self.lost.contains(&(id, to, ch))
                {
                    continue;
                }
                self.sent.push((now, id, to, ch, b.clone()));
                let mut at = now + self.one_way_ms + self.later.get(&(id, to, ch)).copied().unwrap_or(0);
                if ch == Channel::Reliable {
                    let last = self.last_reliable.entry((id, to)).or_default();
                    at = at.max(*last);
                    *last = at;
                }
                self.wire.push((at, id, to, ch, b));
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
fn a_host_knows_when_it_hears_none_of_its_players() {
    // Ruling SB-2: one player still heard is enough; both silent for
    // `HOST_SILENT_US` (its own network down: they move on) is not.
    let mut m = Mesh::new(20, 3);
    m.run_ms(2_000);
    let silent = |m: &Mesh| m.s[&H].players_silent(m.now_ms * 1000);
    assert!(!silent(&m));
    m.cut.insert((B, H), m.now_ms);
    m.run_ms(3_000);
    assert!(!silent(&m), "A is still heard");
    assert!(!m.s[&A].players_silent(m.now_ms * 1000), "a client never is");
    m.cut.insert((A, H), m.now_ms);
    let cut = m.now_ms;
    m.until(HOST_SILENT_US / 1000 + 100, "none heard", |m| silent(m));
    assert!(m.now_ms >= cut + HOST_SILENT_US / 1000, "{} ms after the cut", m.now_ms - cut);
    m.cut.remove(&(A, H));
    m.run_ms(100);
    assert!(!silent(&m), "A heard again");

    // Players that left by `Bye` are not silent: nobody is left to hear.
    let mut m = Mesh::new(20, 4);
    m.run_ms(2_000);
    for id in [A, B] {
        m.s.get_mut(&id).unwrap().leave();
    }
    m.run_ms(100);
    m.alive.insert(A, false);
    m.alive.insert(B, false);
    m.run_ms(3_000);
    assert_eq!(m.count(H, |e| matches!(e, Event::PeerLeft { .. })), 2);
    assert!(!silent(&m));
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
    m.s.get_mut(&B).unwrap().unsuspend();
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
        &encode_control(&Control::Resume { tick: far, at_ms: 0, first: 0, frames: vec![] }),
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
    for c in [
        Control::Collect { known: vec![0; SLOTS as usize] },
        Control::Resume { tick: 5, at_ms: 0, first: 0, frames: vec![] },
    ] {
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
fn a_rejoined_peer_gets_what_it_lacks_with_the_resume() {
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
    let at = m.now_ms;
    let r = m.s.get_mut(&A).unwrap().resume(at * 1000);
    let a_frontier = m.s[&A].frontier();
    m.until(200, "B resumes", |m| m.first(B, |e| matches!(e, Event::Resumed { .. })).is_some());
    let resume =
        m.sent.iter().filter(|(t, from, to, ..)| *t >= at && *from == A && *to == B).find_map(|(.., b)| {
            match decode_control(b) {
                Ok(Control::Resume { tick, first, frames, .. }) => Some((tick, first, frames.len() as u32)),
                _ => None,
            }
        });
    let horizon = m.s[&A].input_log().horizon();
    assert_eq!(resume, Some((r, b_frontier, horizon - b_frontier)), "from its report's ack to our horizon");
    // Before any frame of A's reached it (A holds its first tick a round trip).
    assert!(m.s[&B].frontier() >= a_frontier, "B {} of A's {a_frontier} at the resume", m.s[&B].frontier());
}

/// A, B's new host, after B's report; one-way trips of 30 ms (B's input
/// delay is 1).
fn reported_to_a(seed: u64) -> Mesh {
    let mut m = Mesh::new(30, seed);
    m.run_ms(3_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    m
}

#[test]
fn a_new_host_holds_its_first_tick_a_round_trip() {
    // RS-1: the inputs in flight were lost with the old host; R runs once
    // the rejoined players' first inputs can be here.
    let mut m = reported_to_a(31);
    let r = m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    m.run_ms(50);
    assert_eq!(m.s[&A].current_tick(), r, "held 50 ms after its resume (a 60 ms round trip)");
    m.run_ms(50);
    assert!(m.s[&A].current_tick() > r, "running 100 ms after it");
}

#[test]
fn a_resume_lost_once_still_places_its_player_on_time() {
    // B's `Resume` is lost once (it comes 100 ms late) and none of A's
    // datagrams reach B meanwhile: `at_ms`, on B's own clock, still puts B
    // where its inputs reach A one tick early.
    let mut m = reported_to_a(32);
    m.lost.insert((A, B, Channel::Unreliable));
    m.later.insert((A, B, Channel::Reliable), 100);
    m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    m.until(300, "B resumes", |m| m.first(B, |e| matches!(e, Event::Resumed { .. })).is_some());
    m.lost.clear();
    m.later.clear();
    m.run_ms(30);
    // B leads A by one trip and its margin, less its input delay: 3 + 1 - 1 ticks.
    let lead = m.s[&B].current_tick() as i64 - m.s[&A].current_tick() as i64;
    assert!((2..=4).contains(&lead), "B leads A by {lead} ticks");
}

#[test]
fn a_late_resume_is_overtaken_by_its_datagrams() {
    // B's reliable `Resume` comes 300 ms late; its unreliable twin, in each
    // of A's datagrams to B from the resume on, does not.
    let mut m = reported_to_a(33);
    m.later.insert((A, B, Channel::Reliable), 300);
    let at = m.now_ms;
    m.s.get_mut(&A).unwrap().resume(at * 1000);
    m.until(300, "B resumes", |m| m.first(B, |e| matches!(e, Event::Resumed { .. })).is_some());
    let resumed = m.first(B, |e| matches!(e, Event::Resumed { .. })).unwrap();
    let took = resumed - at;
    assert!(took <= 30 + 10, "B resumed {took} ms after A");
    m.later.clear();
    m.run_ms(1_000);
    // Once B's inputs came, A's datagrams to it are frames again (its acks
    // and margins).
    let twins: Vec<u64> = m
        .sent
        .iter()
        .filter(|(t, from, to, ch, _)| {
            *t > resumed + 200 && *from == A && *to == B && *ch == Channel::Unreliable
        })
        .filter(|(.., b)| matches!(decode_datagram(b, SLOTS as usize), Ok(Datagram::Resume(_))))
        .map(|m| m.0)
        .collect();
    assert!(twins.is_empty(), "resumes sent at {twins:?}");
    assert_eq!(m.count(B, |e| matches!(e, Event::Resumed { .. })), 1, "the late `Resume` changes nothing");
    let after = m.events[&B].iter().filter(|(t, _)| *t >= at).map(|(_, e)| e);
    assert_eq!(after.filter(|e| matches!(e, Event::Stalled { .. } | Event::Malformed { .. })).count(), 0);
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
    // Frames within the match's slots, which decode (the merge's path).
    let fitting = (0u32..16, 0u32..16, any::<u8>()).prop_map(|(known, changed, key)| {
        let changed = changed & known;
        Frame { known, changed, keys: vec![key; changed.count_ones() as usize] }
    });
    let frames =
        prop_oneof![prop::collection::vec(frame.clone(), 0..80), prop::collection::vec(fitting, 0..80)];
    prop_oneof![
        prop::collection::vec(edge.clone(), 0..40).prop_map(|known| Control::Collect { known }),
        (
            edge.clone(),
            edge.clone(),
            frames.clone(),
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
        (edge.clone(), any::<u32>(), edge, frames).prop_map(|(tick, at_ms, first, frames)| Control::Resume {
            tick,
            at_ms,
            first,
            frames
        }),
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

use rl_netcode::SILENCE_TIMEOUT_US;
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
                sc.exits.push(Exit { peer: HOST, tick: 300 + (seed as u32 * 37) % 400, after_us: 0, how });
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
    sc.exits.push(Exit { peer: HOST, tick: 500, after_us: 0, how: ExitHow::Leave });
    let r = run_checked(&sc);
    let left_us = 500 * 10_000;
    assert!(r.migrations[0].silent_us < left_us + 200_000, "{:?}", r.migrations[0]);
}

#[test]
fn the_second_successor_takes_over_when_the_first_is_dead() {
    let mut sc = mscenario(5, Profile::TYPICAL, 3);
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 400, after_us: 0, how: ExitHow::Crash });
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
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 400, after_us: 0, how: ExitHow::Crash });
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
    sc.outages.push(Outage { peer: 3, from_tick: 500, until_tick: 800, one_way: false }); // 3 s
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST]);
    assert!(r.events[&3].contains(&Event::HostSilent) && r.events[&3].contains(&Event::HostHeard));
    assert!(r.host_lost.is_empty(), "it returned to its host (N69)");
}

#[test]
fn a_successor_outage_does_not_take_the_match() {
    let mut sc = mscenario(10, Profile::TYPICAL, 3);
    sc.outages.push(Outage { peer: 1, from_tick: 500, until_tick: 800, one_way: false });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST]);
    assert!(r.events.values().flatten().all(|e| !matches!(e, Event::Resumed { .. })));
    assert!(r.host_lost.is_empty(), "it returned to its host (N69)");
}

#[test]
fn a_late_peer_watches_from_a_state() {
    let mut sc = mscenario(11, Profile::GOOD, 3);
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
    // Peer 3's join to the successor takes 7 s after contact (slow relays):
    // the successor resumes without it at its window's end.
    sc.migration.as_mut().unwrap().slow_joins.push((3, 7_000_000));
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
    let m = &r.migrations[0];
    assert!(m.resumed_us >= m.silent_us + WINDOW_US - 100_000, "it waited for peer 3 until its window's end");
    assert!(m.resumed_us <= m.silent_us + WINDOW_US + 300_000, "and no longer: {m:?}");
    assert!(r.events[&3].iter().any(|e| matches!(e, Event::StateLoaded { .. })), "it watches from a state");
    assert!(!r.host_lost.contains(&3));
    // Its slot (4) stays Keys(0) from the resume on.
    assert!(r.host_log[m.tick as usize + 50..].iter().all(|row| row[4] == 0));
}

#[test]
fn a_second_migration_moves_the_match_again() {
    let mut sc = mscenario(12, Profile::TYPICAL, 3);
    sc.ticks = 2_500;
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 1_500, after_us: 0, how: ExitHow::Crash });
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
        sc.exits.push(Exit { peer, tick: 300 + 600 * k as u32, after_us: 0, how: ExitHow::Crash });
    }
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    assert_eq!(r.hosts, vec![HOST, 1, 2, 3, 4], "four migrations");
    assert_eq!(r.host_lost, [5, 6, 100].into(), "the fifth is host lost");
}

#[test]
fn a_spectator_takes_over_when_no_player_can() {
    let mut sc = mscenario(13, Profile::LAN, 0); // the host and a spectator only
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
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
    sc.exits.push(Exit { peer: HOST, tick: 600, after_us: 0, how: ExitHow::Crash });
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
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
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
        sc.outages.push(Outage { peer, from_tick: 390, until_tick: 450, one_way: false });
    }
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
    let old = &r.logs[&HOST];
    assert!(old.iter().zip(&r.host_log).any(|(a, b)| a != b), "the exited host's last rows differ");
}

#[test]
fn the_verdict_finds_every_value_a_survivor_held() {
    let mut sc = mscenario(19, Profile::GOOD, 3);
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
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
    sc.exits.push(Exit { peer: HOST, tick: 500, after_us: 0, how: ExitHow::Crash });
    let (a, b) = (run_checked(&sc), run_checked(&sc));
    assert_eq!(a.host_log, b.host_log);
    assert_eq!(a.migrations, b.migrations);
    assert_eq!(a.checksums, b.checksums);
    assert_eq!(a.logs, b.logs);
    assert_eq!(a.events, b.events);
}

#[test]
fn the_verdict_bounds_exited_and_lost_peers_rows() {
    // Peer 1 crashes with the host; peer 100 is host lost (the D8 run).
    let mut dead = mscenario(5, Profile::TYPICAL, 3);
    dead.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
    dead.exits.push(Exit { peer: 1, tick: 400, after_us: 0, how: ExitHow::Crash });
    let mut lost = mscenario(17, Profile::GOOD, 2);
    lost.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
    lost.migration.as_mut().unwrap().slow_joins.extend([(2, 6_000_000), (100, 6_000_000)]);
    for (sc, peer) in [(dead, 1), (lost, 100)] {
        let mut r = run_checked(&sc);
        assert!(r.exited.contains(&peer) || r.host_lost.contains(&peer));
        // A row it confirmed long before it went, changed: it must agree
        // with the survivors as far as they go.
        r.logs.get_mut(&peer).unwrap()[100][2] ^= 1;
        let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
        let e = harness::verdict(&sc, &r, &mut f).expect_err("a wrong row of a peer that went");
        assert!(e.what.contains(&format!("peer {peer}")), "{e:?}");
    }
}

#[test]
fn a_peer_in_contact_follows_its_successor_when_the_old_host_returns() {
    // N69: nothing the host sends gets through for 2.5 s (it still hears
    // its players: one-way, so it hosts on, SB-2); its peers move to
    // successor 1. Peer 2's join takes 2 s, so it hears the old host again
    // while in contact: it keeps going to the successor.
    let mut sc = mscenario(21, Profile::GOOD, 3);
    sc.outages.push(Outage { peer: HOST, from_tick: 500, until_tick: 750, one_way: true });
    sc.migration.as_mut().unwrap().slow_joins.push((2, 2_000_000));
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    assert_eq!(r.hosts, vec![HOST, 1]);
    let heard = r.events[&2].iter().position(|e| *e == Event::HostHeard).expect("it heard the old host");
    assert!(r.events[&2][heard..].iter().any(|e| matches!(e, Event::Resumed { .. })), "{:?}", r.events[&2]);
    let m = &r.migrations[0];
    assert!(m.resumed_us - m.silent_us < 3_000_000, "the successor did not wait for it: {m:?}");
    // The old host, alone again, times its former peers out: it drops none
    // of them (they follow another host now).
    assert!(r.dropped.is_empty(), "{:?}", r.dropped);
}

#[test]
fn peers_in_contact_return_with_their_successor_to_the_old_host() {
    // N69 as rl-online's `Planner` has it: nothing the host sends gets
    // through for 2.5 s (one-way: it still hears its players, so it hosts
    // on, SB-2); peers 2, 3 and 100 contact successor 1, but their joins take
    // 2 s, so the old host is heard again first. In contact, they remember
    // it; successor 1, uncommitted, goes back to it and closes its gate, and
    // the contacts it breaks take them back to it too. Nobody is stranded
    // to commit elsewhere later (net-soak's `bad-4s3c1v-seed562805195`).
    for seed in [24, 25] {
        let mut sc = mscenario(seed, Profile::GOOD, 3);
        sc.outages.push(Outage { peer: HOST, from_tick: 500, until_tick: 750, one_way: true });
        sc.migration.as_mut().unwrap().slow_joins.extend([(2, 2_000_000), (3, 2_000_000), (100, 2_000_000)]);
        let r = run_checked(&sc);
        assert_eq!(r.hosts, vec![HOST], "seed {seed}: {:?}", r.migrations);
        assert!(r.host_lost.is_empty() && r.left_behind.is_empty(), "seed {seed}");
        for peer in [1, 2, 3, 100] {
            assert!(r.events[&peer].contains(&Event::HostSilent), "seed {seed}: peer {peer} migrated");
            assert!(r.events[&peer].contains(&Event::HostHeard), "seed {seed}: peer {peer} heard it");
        }
    }
}

#[test]
fn a_host_cut_off_from_its_players_leaves_the_match() {
    // A host's own network drops for 3 s while it keeps running: its peers
    // find it silent and move to a successor, as it finds them silent: it
    // leaves the match (ruling SB-2, as rl-online's `cut_off_check`)
    // instead of playing on alone, and is no survivor. Its rows (fills for
    // its players' slots at the end) agree with the match only as far as
    // the survivors of its migration held. Once as the first host, once as
    // the second (the first stepped down, N70; the net-soak cases
    // `lan-8s2c0v-seed1433102928` and `bad-14s3c1v-seed974389837`, where a
    // follower came back to the old host still playing alone).
    let mut first = mscenario(22, Profile::LAN, 3);
    first.outages.push(Outage { peer: HOST, from_tick: 500, until_tick: 800, one_way: false });
    let mut second = mscenario(23, Profile::LAN, 3);
    second.host_freeze = Some((400, 600));
    second.outages.push(Outage { peer: 1, from_tick: 1_000, until_tick: 1_300, one_way: false });
    for (sc, hosts, behind) in [(first, vec![HOST, 1], HOST), (second, vec![HOST, 1, 2], 1)] {
        let mut r = run_checked(&sc);
        assert_eq!(r.hosts, hosts, "seed {}", sc.seed);
        assert!(r.exited.contains(&behind) && r.left_behind.is_empty(), "seed {}", sc.seed);
        assert_eq!(r.left_from.get(&behind), Some(&behind), "seed {}", sc.seed);
        let old = &r.logs[&behind];
        assert!(old.iter().zip(&r.host_log).any(|(a, b)| a != b), "seed {}: its own rows differ", sc.seed);
        // A row it confirmed long before it was cut off, changed: it must
        // agree with the survivors as far as they go.
        r.logs.get_mut(&behind).unwrap()[100][2] ^= 1;
        let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
        let e = harness::verdict(&sc, &r, &mut f).expect_err("a wrong row of a host cut off");
        assert!(e.what.contains(&format!("peer {behind}")), "{e:?}");
    }
}

#[test]
fn a_new_host_waits_past_its_window_for_a_rejoined_peers_report() {
    // C1: spectator 100 runs a frame every 0.8 s, so it answers the new
    // host's Collect only at its next frame; it crashes between its rejoin
    // and that frame. The new host still waits for its report after its
    // window's end, until the dead peer times out.
    let mut sc = mscenario(20, Profile::LAN, 2);
    sc.peers[2].frame_us = 800_000;
    sc.migration.as_mut().unwrap().slow_joins.push((100, 300_000));
    sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 100, tick: 400, after_us: 2_950_000, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
    assert!(r.exited.contains(&100));
    let m = &r.migrations[0];
    assert!(m.resumed_us > m.silent_us + WINDOW_US + 1_000_000, "it waited for the report: {m:?}");
    assert!(m.resumed_us < m.silent_us + SILENCE_TIMEOUT_US + 1_000_000, "until the peer timed out: {m:?}");
}

/// Every event `what` picks (its tick) a peer had after its own
/// `Resumed`: `(peer, tick, resume tick)`.
fn after_resume(r: &Report, what: impl Fn(&Event) -> Option<u32>) -> Vec<(PeerId, u32, u32)> {
    let mut out = Vec::new();
    for (&peer, events) in &r.events {
        let mut resumed = None;
        for e in events {
            if let Event::Resumed { tick } = *e {
                resumed = Some(tick);
            } else if let (Some(at), Some(tick)) = (resumed, what(e)) {
                out.push((peer, tick, at));
            }
        }
    }
    out
}

/// A migration stops the match; once it resumes, the time sync must carry
/// every peer on without another stall episode.
fn stalls_after_resume(r: &Report) -> Vec<(PeerId, u32, u32)> {
    after_resume(r, |e| match *e {
        Event::Stalled { tick } => Some(tick),
        _ => None,
    })
}

/// Nor may a player's inputs for the resume tick on come too late (filled:
/// its own keys replaced). Ticks before it are the migration's (the
/// resume's lagging rule).
fn lates_after_resume(r: &Report) -> Vec<(PeerId, u32, u32)> {
    let lates = after_resume(r, |e| match *e {
        Event::Late { to, .. } => Some(to - 1),
        _ => None,
    });
    lates.into_iter().filter(|&(_, last, at)| last >= at).collect()
}

#[test]
fn no_peer_stalls_after_a_resume_on_bad_links() {
    // Ruling RS-1: a resume used to leave clients a round trip late (their
    // inputs held every frontier back), a `Resume` lost once a ping later
    // still, and their time sync jumped on stale margins: 30 stall episodes
    // over these 80 migrations. Odd seeds drift the clients' clocks.
    let (mut stalls, mut lates) = (Vec::new(), Vec::new());
    let mut runs = 0;
    for seed in 1..=40 {
        for how in [ExitHow::Crash, ExitHow::Leave] {
            let mut sc = mscenario(seed, Profile::BAD, 3);
            if seed % 2 == 1 {
                for (p, ppm) in sc.peers.iter_mut().zip([2_000, -2_000, 1_000, -1_000]) {
                    p.clock_ppm = ppm;
                }
            }
            sc.exits.push(Exit { peer: HOST, tick: 300 + (seed as u32 * 37) % 400, after_us: 0, how });
            let r = run_checked(&sc);
            assert_eq!(r.migrations.len(), 1, "seed {seed} {how:?}");
            runs += 1;
            stalls.extend(stalls_after_resume(&r).into_iter().map(|s| (seed, how, s)));
            lates.extend(lates_after_resume(&r).into_iter().map(|s| (seed, how, s)));
        }
    }
    assert_eq!(runs, 80);
    assert!(stalls.is_empty(), "{} post-resume stall episodes: {stalls:?}", stalls.len());
    assert!(lates.is_empty(), "{} post-resume late inputs: {lates:?}", lates.len());
}
