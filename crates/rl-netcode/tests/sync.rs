//! Checksums, desync repair and state transfer (spec §3.5, §3.6) with the
//! toy simulation.

use proptest::prelude::*;
use rl_netcode::harness::{self, HOST, PeerPlan, Scenario};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{Channel, Config, Event, Role, Seat, Session, Simulation, ToySim, input_delay};
use rl_proto::{Control, STATE_CHUNK, encode_control};
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
    let (tick, bytes) = host.encode_confirmed_state().unwrap();
    assert_eq!(tick + 1, host.frontier());
    let mut replay = ToySim::new(3);
    for t in 0..=tick {
        let row = host.input_log().row(t).unwrap();
        replay.tick(&[Keys(row[0]), Keys(row[1]), Keys(row[2])]);
    }
    assert_eq!(replay.encode_state(), bytes);
    // Encoding re-simulated back to the present: the session goes on unchanged.
    assert_eq!(host.current_tick(), 50);
    assert_eq!(host.sim().checksum(), now);
    assert_eq!(host.stats().resim_ticks, 50 - host.frontier() as u64);
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
