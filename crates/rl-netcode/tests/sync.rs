//! Checksums, desync repair and state transfer (spec §3.5, §3.6) with the
//! toy simulation.

use proptest::prelude::*;
use rl_netcode::harness::{self, HOST, PeerPlan, Scenario};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{
    Channel, Config, Driver, Event, PeerId, Role, Seat, Session, Simulation, StateRejected, ToySim,
    input_delay,
};
use rl_proto::{
    ClientInputs, Control, Datagram, HostInputs, STATE_CHUNK, Stamp, encode_control, encode_datagram,
};
use rl_sim::Keys;

fn plan(id: u16, seat: Seat, profile: Profile, join_tick: Option<u32>) -> PeerPlan {
    PeerPlan {
        id,
        seat,
        profile,
        clock_ppm: 0,
        input_delay: input_delay(profile.ping_ms),
        frame_us: 6_944,
        join_tick,
        freeze: None,
    }
}

#[test]
fn confirmed_checksums_agree_and_are_exchanged() {
    let peers =
        vec![plan(1, Seat::Player(2), Profile::TYPICAL, None), plan(2, Seat::Spectator, Profile::GOOD, None)];
    let sc = Scenario {
        seed: 2,
        slots: 3,
        peers,
        ticks: 1_000,
        settle_ticks: 200,
        outages: vec![],
        expect_no_stalls: true,
        corrupt: vec![],
        host_freeze: None,
    };
    let mut f = ToyFactory { slots: 3, seed: 2 };
    let r = harness::run(&sc, &mut f).unwrap();
    harness::verdict(&sc, &r, &mut f).unwrap();
    let host = &r.checksums[&HOST];
    assert!(host.len() >= 99);
    assert_eq!(&r.checksums[&1][..99], &host[..99]);
    assert!(r.events.values().flatten().all(|e| !matches!(e, Event::Desync { .. })));
}

#[test]
fn a_spectator_joins_mid_match() {
    let peers = vec![
        plan(1, Seat::Player(2), Profile::TYPICAL, None),
        plan(7, Seat::Spectator, Profile::TYPICAL, Some(600)),
    ];
    let sc = Scenario {
        seed: 3,
        slots: 3,
        peers,
        ticks: 1_500,
        settle_ticks: 200,
        outages: vec![],
        expect_no_stalls: true,
        corrupt: vec![],
        host_freeze: None,
    };
    let mut f = ToyFactory { slots: 3, seed: 3 };
    let r = harness::run(&sc, &mut f).unwrap();
    harness::verdict(&sc, &r, &mut f).unwrap();
    assert!(r.events[&7].iter().any(|e| matches!(e, Event::StateLoaded { .. })));
    assert!(r.events[&HOST].iter().any(|e| matches!(e, Event::StateSent { peer: 7, .. })));
}

#[test]
fn a_desync_is_detected_and_repaired() {
    let peers = vec![plan(1, Seat::Player(2), Profile::GOOD, None)];
    let mut sc = Scenario {
        seed: 9,
        slots: 3,
        peers,
        ticks: 1_200,
        settle_ticks: 200,
        outages: vec![],
        expect_no_stalls: true,
        corrupt: vec![],
        host_freeze: None,
    };
    sc.corrupt.push((1, 500));
    let mut f = ToyFactory { slots: 3, seed: 9 };
    let r = harness::run(&sc, &mut f).unwrap();
    let desync = r.events[&HOST]
        .iter()
        .find_map(|e| match e {
            Event::Desync { tick, .. } => Some(*tick),
            _ => None,
        })
        .unwrap();
    assert!((500..530).contains(&desync), "detected at {desync}");
    let loaded = r.events[&1]
        .iter()
        .find_map(|e| match e {
            Event::StateLoaded { tick } => Some(*tick),
            _ => None,
        })
        .unwrap();
    let after = |v: &Vec<(u32, u64)>| v.iter().filter(|(t, _)| *t > loaded).copied().collect::<Vec<_>>();
    let (h, c) = (after(&r.checksums[&HOST]), after(&r.checksums[&1]));
    assert!(!c.is_empty());
    assert_eq!(&h[..c.len()], &c[..]);
    // The verdict compares the repaired peer from its state on.
    harness::verdict(&sc, &r, &mut f).unwrap();
    assert_eq!(r.stats[&HOST].desyncs, 1);
    assert_eq!(r.stats[&HOST].states_sent, 1);
    assert_eq!(r.stats[&1].states_loaded, 1);
    assert!(r.events[&1].iter().any(|e| matches!(e, Event::Desync { peer: HOST, .. })));
}

#[test]
fn an_uncorrupted_desync_fails_the_verdict() {
    // The same run, but the scenario the verdict judges does not admit the fault.
    let peers = vec![plan(1, Seat::Player(2), Profile::GOOD, None)];
    let mut sc = Scenario {
        seed: 9,
        slots: 3,
        peers,
        ticks: 1_200,
        settle_ticks: 200,
        outages: vec![],
        expect_no_stalls: true,
        corrupt: vec![(1, 500)],
        host_freeze: None,
    };
    let mut f = ToyFactory { slots: 3, seed: 9 };
    let r = harness::run(&sc, &mut f).unwrap();
    sc.corrupt.clear();
    assert!(harness::verdict(&sc, &r, &mut f).is_err());
}

#[test]
fn the_confirmed_state_is_the_one_after_the_last_confirmed_tick() {
    // A remote player who never sends: the host fills its slot 12 ticks
    // late, so its frontier stays behind its current tick.
    let mut f = ToyFactory { slots: 3, seed: 4 };
    let mut host =
        Session::new(Config::new(3, 0), Role::Host { peers: vec![(1, Seat::Player(2))] }, ToySim::new(3), 0);
    let mut drv = <ToyFactory as harness::Factory<ToySim>>::driver(&mut f, HOST, None);
    for ms in 1..=500u64 {
        host.advance(ms * 1000, drv.as_mut());
    }
    assert_eq!(host.current_tick(), 50);
    assert!(host.frontier() < 45);
    let now = host.sim().checksum();
    let sums = host.checksums().to_vec();
    assert!(!sums.is_empty());
    let (tick, bytes) = host.encode_confirmed_state().unwrap();
    assert_eq!(tick + 1, host.frontier());
    assert_eq!(replay(&host, tick).encode_state(), bytes);
    // Encoding re-simulated back to the present: the session goes on unchanged.
    assert_eq!(host.current_tick(), 50);
    assert_eq!(host.sim().checksum(), now);
    assert_eq!(host.checksums(), &sums[..]);
    assert_eq!(host.stats().resim_ticks, 50 - host.frontier() as u64);
    // A rollback after it still restores the right snapshots: the player's
    // late inputs differ from the fills' prediction.
    let first = host.input_log().known_upto(2);
    let late = ClientInputs { stamp: Stamp::default(), tick: first, ack: 0, first, keys: vec![7; 5] };
    host.receive(1, Channel::Unreliable, &encode_datagram(&Datagram::Client(late)), 500_000);
    for ms in 501..=800u64 {
        host.advance(ms * 1000, drv.as_mut());
    }
    let events = host.drain_events();
    assert!(events.iter().any(|e| matches!(e, Event::Rollback { from, .. } if *from == first)));
    let (tick, bytes) = host.encode_confirmed_state().unwrap();
    assert_eq!(replay(&host, tick).encode_state(), bytes);
    // Every confirmed checksum is the replay's.
    for &(t, h) in host.checksums() {
        assert_eq!(replay(&host, t).checksum(), h, "tick {t}");
    }
}

/// The toy world after `tick`, replayed from the host's confirmed inputs.
fn replay(host: &Session<ToySim>, tick: u32) -> ToySim {
    let mut sim = ToySim::new(3);
    for t in 0..=tick {
        let row = host.input_log().row(t).unwrap();
        sim.tick(&[Keys(row[0]), Keys(row[1]), Keys(row[2])]);
    }
    sim
}

/// What a faulty peer's simulation does with a state from the host.
#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    /// Refuses every state.
    Refuse,
    /// Loads it, then diverges again at the next tick (a bug that comes
    /// back): saved in its snapshots, so a rollback repeats it.
    Relapse,
}

#[derive(Clone)]
struct Faulty {
    toy: ToySim,
    fault: Fault,
    relapse: bool,
}

impl Simulation for Faulty {
    type Snapshot = Faulty;
    fn tick(&mut self, keys: &[Keys]) {
        self.toy.tick(keys);
        if std::mem::take(&mut self.relapse) {
            self.toy.displace(0, 1);
        }
    }
    fn save(&mut self) -> Faulty {
        self.clone()
    }
    fn restore(&mut self, s: &Faulty) {
        self.clone_from(s);
    }
    fn forget_before(&mut self, _: &Faulty) {}
    fn checksum(&self) -> u64 {
        self.toy.checksum()
    }
    fn encode_state(&self) -> Vec<u8> {
        self.toy.encode_state()
    }
    fn decode_state(&mut self, bytes: &[u8]) -> Result<(), StateRejected> {
        if self.fault == Fault::Refuse {
            return Err(StateRejected);
        }
        self.toy.decode_state(bytes)?;
        self.relapse = self.fault == Fault::Relapse;
        Ok(())
    }
}

/// The toy match where peer 1's simulation has `fault`.
struct FaultyFactory {
    toy: ToyFactory,
    fault: Fault,
}

struct FaultyDriver(Box<dyn Driver<ToySim>>);

impl Driver<Faulty> for FaultyDriver {
    fn local_inputs(&mut self, sim: &Faulty, tick: u32, slots: &[u8], out: &mut [Keys]) {
        self.0.local_inputs(&sim.toy, tick, slots, out);
    }
}

impl harness::Factory<Faulty> for FaultyFactory {
    fn sim(&mut self, peer: PeerId) -> Faulty {
        let fault = if peer == 1 { self.fault } else { Fault::None };
        Faulty { toy: ToySim::new(self.toy.slots as usize), fault, relapse: false }
    }
    fn driver(&mut self, peer: PeerId, seat: Option<Seat>) -> Box<dyn Driver<Faulty>> {
        Box::new(FaultyDriver(self.toy.driver(peer, seat)))
    }
    fn corrupt(&mut self, sim: &mut Faulty, _index: usize) {
        sim.toy.displace(0, 1);
    }
}

/// Peer 1 desyncs at host tick 500 and its simulation has `fault`: no
/// state repairs it, so the host gives up; the others play on and agree.
/// The host's desync ticks and the ticks of the states it sent peer 1.
fn failing_repairs(fault: Fault, ticks: u32) -> (harness::Report, Vec<u32>, Vec<u32>) {
    let peers =
        vec![plan(1, Seat::Player(2), Profile::GOOD, None), plan(2, Seat::Player(3), Profile::GOOD, None)];
    let sc = Scenario {
        seed: 11,
        slots: 4,
        peers,
        ticks,
        settle_ticks: 200,
        outages: vec![],
        expect_no_stalls: true,
        corrupt: vec![(1, 500)],
        host_freeze: None,
    };
    let mut f = FaultyFactory { toy: ToyFactory { slots: 4, seed: 11 }, fault };
    let r = harness::run(&sc, &mut f).unwrap();
    // The others play on and agree; peer 1 is dropped.
    harness::verdict(&sc, &r, &mut f).unwrap();
    let host = &r.events[&HOST];
    let desyncs: Vec<u32> = host
        .iter()
        .filter_map(|e| match e {
            Event::Desync { tick, peer: 1, .. } => Some(*tick),
            _ => None,
        })
        .collect();
    let sent: Vec<u32> = host
        .iter()
        .filter_map(|e| match e {
            Event::StateSent { tick, peer: 1, .. } => Some(*tick),
            _ => None,
        })
        .collect();
    assert!((500..530).contains(&desyncs[0]));
    assert!(host.iter().any(|e| matches!(e, Event::RepairFailed { peer: 1 })));
    assert!(r.repair_failed.contains(&1) && r.dropped.contains(&1));
    assert!(r.events[&2].iter().all(|e| !matches!(e, Event::Desync { .. })));
    (r, desyncs, sent)
}

#[test]
fn a_peer_that_diverges_after_each_state_is_given_up() {
    // Every state loads, then the peer diverges again: the host re-arms
    // after each and gives up after three.
    let (r, desyncs, sent) = failing_repairs(Fault::Relapse, 1_800);
    assert_eq!(desyncs.len(), 4, "desyncs at {desyncs:?}");
    assert_eq!(sent.len(), 3, "states at {sent:?}");
    // Each re-arm comes once the margin after the state's load has passed.
    for (d, s) in desyncs[1..].iter().zip(&sent) {
        assert!((s + 300..s + 340).contains(d), "desync at {d} after the state at {s}");
    }
    assert_eq!(r.stats[&HOST].desyncs, 4);
    assert_eq!(r.stats[&1].states_loaded, 3);
}

#[test]
fn a_peer_that_refuses_the_state_is_given_up() {
    // It never says it loaded the state: the host waits for that a minute
    // (a multi-MB state may take long), then gives up.
    let (r, desyncs, sent) = failing_repairs(Fault::Refuse, 7_000);
    assert_eq!(desyncs.len(), 1, "desyncs at {desyncs:?}");
    assert_eq!(sent.len(), 1, "states at {sent:?}");
    assert_eq!(r.stats[&1].states_loaded, 0);
    let refused = r.events[&1].iter().filter(|e| matches!(e, Event::StateRejected { .. })).count();
    assert_eq!(refused, 1);
}

fn client() -> Session<ToySim> {
    Session::new(Config::new(3, 1), Role::Client { host: 0, slot: 2 }, ToySim::new(3), 0)
}

fn send(s: &mut Session<ToySim>, c: Control) {
    s.receive(0, Channel::Reliable, &encode_control(&c), 1_000);
}

fn state_events(s: &mut Session<ToySim>) -> Vec<Event> {
    s.drain_events()
        .into_iter()
        .filter(|e| {
            matches!(e, Event::StateArrived { .. } | Event::StateLoaded { .. } | Event::StateRejected { .. })
        })
        .collect()
}

#[test]
fn hostile_state_transfers_are_refused() {
    let good = ToySim::new(3).encode_state();
    let n = good.len() as u32;
    // An index out of range.
    let mut c = client();
    send(&mut c, Control::StateBegin { tick: 5, len: n, chunks: 1 });
    send(&mut c, Control::StateChunk { index: 1, bytes: good.clone() });
    assert_eq!(state_events(&mut c), vec![Event::StateRejected { tick: 5 }]);
    // Later chunks of a refused transfer change nothing.
    send(&mut c, Control::StateChunk { index: 0, bytes: good.clone() });
    assert_eq!(state_events(&mut c), vec![]);
    // A repeated chunk.
    let mut c = client();
    send(&mut c, Control::StateBegin { tick: 5, len: STATE_CHUNK as u32 + 8, chunks: 2 });
    send(&mut c, Control::StateChunk { index: 1, bytes: good[..8].to_vec() });
    send(&mut c, Control::StateChunk { index: 1, bytes: good[..8].to_vec() });
    assert_eq!(state_events(&mut c), vec![Event::StateRejected { tick: 5 }]);
    // More bytes than announced.
    let mut c = client();
    send(&mut c, Control::StateBegin { tick: 5, len: n - 1, chunks: 1 });
    send(&mut c, Control::StateChunk { index: 0, bytes: good.clone() });
    assert_eq!(state_events(&mut c), vec![Event::StateRejected { tick: 5 }]);
    // A chunk count that does not fit the length does not even decode.
    let mut c = client();
    send(&mut c, Control::StateBegin { tick: 5, len: n, chunks: 9 });
    assert_eq!(state_events(&mut c), vec![]);
    assert_eq!(c.stats().malformed, 1);
    // Bytes the simulation refuses: arrived, then rejected, the old state kept.
    let mut c = client();
    let before = c.sim().checksum();
    send(&mut c, Control::StateBegin { tick: 5, len: 3, chunks: 1 });
    send(&mut c, Control::StateChunk { index: 0, bytes: vec![1, 2, 3] });
    assert_eq!(
        state_events(&mut c),
        vec![Event::StateArrived { tick: 5, bytes: vec![1, 2, 3] }, Event::StateRejected { tick: 5 }]
    );
    assert_eq!(c.sim().checksum(), before);
    // A good state loads.
    let mut c = client();
    send(&mut c, Control::StateBegin { tick: 5, len: n, chunks: 1 });
    send(&mut c, Control::StateChunk { index: 0, bytes: good.clone() });
    assert_eq!(
        state_events(&mut c),
        vec![Event::StateArrived { tick: 5, bytes: good }, Event::StateLoaded { tick: 5 }]
    );
    assert_eq!(c.stats().states_loaded, 1);
}

#[test]
fn a_state_arrives_in_chunks_in_any_order() {
    // A toy world wide enough to span three chunks.
    let slots = (2 * STATE_CHUNK + 100 - 16) / 8;
    let mut wide = ToySim::new(slots);
    wide.tick(&[Keys(1); 3]);
    let bytes = wide.encode_state();
    let mut c = Session::new(Config::new(3, 1), Role::Client { host: 0, slot: 2 }, ToySim::new(slots), 0);
    let chunks: Vec<&[u8]> = bytes.chunks(STATE_CHUNK).collect();
    assert_eq!(chunks.len(), 3);
    send(&mut c, Control::StateBegin { tick: 0, len: bytes.len() as u32, chunks: 3 });
    for i in [2u16, 0, 1] {
        send(&mut c, Control::StateChunk { index: i, bytes: chunks[i as usize].to_vec() });
    }
    assert!(matches!(state_events(&mut c)[..], [Event::StateArrived { .. }, Event::StateLoaded { tick: 0 }]));
    assert_eq!(c.sim().checksum(), wide.checksum());
}

fn arb_control() -> impl Strategy<Value = Control> {
    prop_oneof![
        (any::<u32>(), any::<u64>()).prop_map(|(tick, hash)| Control::Checksum { tick, hash }),
        (any::<u32>(), any::<u64>()).prop_map(|(tick, host_hash)| Control::Desync { tick, host_hash }),
        (any::<u32>(), 0u32..200, 0u16..4).prop_map(|(tick, len, chunks)| Control::StateBegin {
            tick,
            len,
            chunks
        }),
        (any::<u32>(), any::<u32>(), any::<u16>()).prop_map(|(tick, len, chunks)| Control::StateBegin {
            tick,
            len,
            chunks
        }),
        (0u16..4, prop::collection::vec(any::<u8>(), 0..120))
            .prop_map(|(index, bytes)| Control::StateChunk { index, bytes }),
        (any::<u32>(), any::<u32>()).prop_map(|(from, to)| Control::Late { from, to }),
        any::<u32>().prop_map(|tick| Control::StateLoaded { tick }),
        Just(Control::Bye),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Hostile controls never panic a client, a joining spectator or the host.
    #[test]
    fn hostile_controls_never_panic(controls in prop::collection::vec(arb_control(), 1..30)) {
        let mut f = ToyFactory { slots: 3, seed: 1 };
        let mut sessions = [
            (0, client()),
            (0, Session::new(Config::new(3, 0), Role::Joining { host: 0 }, ToySim::new(3), 0)),
            (1, Session::new(Config::new(3, 0), Role::Host { peers: vec![(1, Seat::Player(2))] }, ToySim::new(3), 0)),
        ];
        for (from, s) in sessions.iter_mut() {
            let mut drv = <ToyFactory as harness::Factory<ToySim>>::driver(&mut f, 5, None);
            for (i, c) in controls.iter().enumerate() {
                let now = i as u64 * 10_000;
                s.receive(*from, Channel::Reliable, &encode_control(c), now);
                s.advance(now, drv.as_mut());
                s.transmit(&mut |_, _, _| {});
            }
        }
    }
}

#[derive(Default)]
struct Watcher {
    frames: u64,
    loaded: Vec<(PeerId, u32, u32)>,
}

impl harness::Observer<ToySim> for Watcher {
    fn on_event(&mut self, peer: PeerId, e: &Event, session: &mut rl_netcode::Session<ToySim>) {
        if let Event::StateLoaded { tick } = *e {
            self.loaded.push((peer, tick, session.current_tick()));
        }
    }
    fn on_frame(&mut self, _peer: PeerId, _session: &mut rl_netcode::Session<ToySim>) {
        self.frames += 1;
    }
}

#[test]
fn an_observer_sees_events_and_frames_as_they_happen() {
    let sc = Scenario {
        seed: 9,
        slots: 3,
        peers: vec![plan(1, Seat::Player(2), Profile::GOOD, None)],
        ticks: 900,
        settle_ticks: 200,
        outages: vec![],
        expect_no_stalls: true,
        corrupt: vec![(1, 500)],
        host_freeze: None,
    };
    let mut f = ToyFactory { slots: 3, seed: 9 };
    let mut w = Watcher::default();
    let r = harness::run_observed(&sc, &mut f, &mut w).unwrap();
    assert!(w.frames > 1_000);
    let [(peer, tick, now)] = w.loaded[..] else { panic!("loaded: {:?}", w.loaded) };
    assert_eq!(peer, 1);
    assert!((500..560).contains(&tick) && now > tick, "state after {tick} loaded at tick {now}");
    // The same run without an observer is the same run.
    let plain = harness::run(&sc, &mut ToyFactory { slots: 3, seed: 9 }).unwrap();
    assert_eq!(plain.host_log, r.host_log);
}

#[test]
fn frames_older_than_a_joined_spectators_state_are_ignored() {
    // Sent before the host had our state's tick (a spectator joined before
    // any tick was confirmed gets such frames), arriving after the state:
    // their base is before our log's origin, so they cannot be decoded;
    // they are old, not malformed.
    let mut s = Session::new(Config::new(3, 0), Role::Joining { host: 0 }, ToySim::new(3), 0);
    let mut state = ToySim::new(3);
    for _ in 0..=40 {
        state.tick(&[Keys(1), Keys(2), Keys(3)]);
    }
    let bytes = state.encode_state();
    send(&mut s, Control::StateBegin { tick: 40, len: bytes.len() as u32, chunks: 1 });
    send(&mut s, Control::StateChunk { index: 0, bytes });
    assert_eq!(s.confirmed(), 41);
    for first in [1, 30, 40] {
        // Every slot known and unchanged from the tick before `first`.
        let frames = rl_proto::encode_frames(first, 5, 3, &|_, s| Some(s as u8 + 1), &|s| Some(s as u8 + 1));
        let h =
            HostInputs { stamp: Stamp::default(), tick: first + 5, your_ack: 0, margin: 0, first, frames };
        s.receive(0, Channel::Unreliable, &encode_datagram(&Datagram::Host(h)), 2_000);
    }
    assert_eq!(s.stats().malformed, 0);
    assert!(s.drain_events().iter().all(|e| !matches!(e, Event::Malformed { .. })));
    assert_eq!(s.input_log().known_upto(0), 41, "nothing before the state is taken");
}

#[test]
fn the_confirmed_tick_is_the_frontier_or_the_state_floor() {
    let mut c = client();
    assert_eq!(c.confirmed(), 0);
    let mut state = ToySim::new(3);
    for _ in 0..=50 {
        state.tick(&[Keys(4); 3]);
    }
    let bytes = state.encode_state();
    send(&mut c, Control::StateBegin { tick: 50, len: bytes.len() as u32, chunks: 1 });
    send(&mut c, Control::StateChunk { index: 0, bytes });
    // Final from the state on, though no input before it is known.
    assert_eq!((c.frontier(), c.confirmed()), (0, 51));
    // The host's own: its frontier.
    let mut f = ToyFactory { slots: 3, seed: 4 };
    let mut host =
        Session::new(Config::new(3, 0), Role::Host { peers: vec![(1, Seat::Player(2))] }, ToySim::new(3), 0);
    let mut drv = <ToyFactory as harness::Factory<ToySim>>::driver(&mut f, HOST, None);
    for ms in 1..=500u64 {
        host.advance(ms * 1000, drv.as_mut());
    }
    assert!(host.frontier() > 0);
    assert_eq!(host.confirmed(), host.frontier());
}
