//! Time sync and the lagging-player rule (spec §3.2) with the toy simulation.

use rl_netcode::harness::{self, HOST, Outage, PeerPlan, Scenario};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{Event, Seat, input_delay};

fn plan(id: u32, slot: u8, profile: Profile, ppm: i32) -> PeerPlan {
    PeerPlan {
        id,
        seat: Seat::Player(slot),
        profile,
        clock_ppm: ppm,
        input_delay: input_delay(profile.ping_ms),
        frame_us: 6_944,
        join_tick: None,
        freeze: None,
    }
}

fn scenario(seed: u64, peers: Vec<PeerPlan>, ticks: u32) -> Scenario {
    Scenario {
        seed,
        slots: 2 + peers.len() as u8,
        peers,
        ticks,
        settle_ticks: 300,
        outages: vec![],
        expect_no_stalls: false,
        corrupt: vec![],
        host_freeze: None,
    }
}

fn run(sc: &Scenario) -> harness::Report {
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(sc, &mut f).unwrap();
    harness::verdict(sc, &r, &mut f).unwrap();
    r
}

#[test]
fn drift_is_corrected() {
    // Clocks 0.5 % fast and slow: without adjustment they would drift 30 ticks a minute apart.
    let sc = scenario(1, vec![plan(1, 2, Profile::GOOD, 5_000), plan(2, 3, Profile::GOOD, -5_000)], 6_000);
    let r = run(&sc);
    for p in [1u32, 2] {
        assert_eq!(r.stats[&HOST].fills, 0, "nobody's input was filled");
        let m = r.stats[&p].margin.unwrap();
        assert!((0..=3).contains(&m), "peer {p} margin {m}");
    }
}

#[test]
fn players_within_the_window_never_stall() {
    for profile in [Profile::LAN, Profile::GOOD, Profile::TYPICAL, Profile::BAD] {
        for seed in 1..=10 {
            let peers = (0..4).map(|i| plan(1 + i, 2 + i as u8, profile, (i as i32 - 2) * 1_000)).collect();
            let mut sc = scenario(seed, peers, 2_000);
            sc.expect_no_stalls = true;
            run(&sc);
        }
    }
}

#[test]
fn late_inputs_lose_to_the_fill() {
    // Peer 1 loses its link for 0.5 s: the host fills its slot; every peer, peer 1 included, ends with the fill.
    let mut sc = scenario(4, vec![plan(1, 2, Profile::GOOD, 0), plan(2, 3, Profile::GOOD, 0)], 1_500);
    sc.outages.push(Outage { peer: 1, from_tick: 500, until_tick: 550 });
    let r = run(&sc);
    assert!(r.stats[&HOST].fills > 0);
    assert!(r.events[&1].iter().any(|e| matches!(e, Event::Late { .. })));
    assert!(r.stats[&1].own_corrections > 0);
    assert_eq!(r.stats[&2].stalls, 0, "the other player never waited");
}

#[test]
fn a_frozen_client_catches_up() {
    let mut p = plan(1, 2, Profile::GOOD, 0);
    p.freeze = Some((600, 1_000)); // 4 s without a single frame
    let sc = scenario(6, vec![p, plan(2, 3, Profile::GOOD, 0)], 2_000);
    let r = run(&sc);
    assert!(r.stats[&1].jumps >= 1);
    assert_eq!(r.stats[&2].stalls, 0);
    assert!(r.events[&HOST].iter().any(|e| matches!(e, Event::PeerLate { peer: 1 })));
}

#[test]
fn a_silent_peer_times_out() {
    let mut sc = scenario(8, vec![plan(1, 2, Profile::GOOD, 0)], 2_000);
    sc.outages.push(Outage { peer: 1, from_tick: 300, until_tick: 1_400 });
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    assert!(r.events[&HOST].iter().any(|e| matches!(e, Event::PeerTimedOut { peer: 1 })));
    assert!(r.events[&1].iter().any(|e| matches!(e, Event::PeerTimedOut { peer: HOST })));
}

#[test]
fn a_frozen_host_stalls_its_clients_who_recover() {
    // The host stops for 2 s (a hitch, a suspended laptop): no frames, its
    // clock stands still, arrivals wait. Its clients stall at the
    // prediction limit, then play on with it.
    let sc = Scenario {
        host_freeze: Some((1_000, 1_200)),
        ..scenario(7, vec![plan(1, 2, Profile::GOOD, 0), plan(2, 3, Profile::TYPICAL, 0)], 3_000)
    };
    let r = run(&sc);
    assert!(r.dropped.is_empty(), "dropped {:?}", r.dropped);
    for p in [1u32, 2] {
        let stalls: Vec<u32> = r.events[&p]
            .iter()
            .filter_map(|e| match e {
                Event::Stalled { tick } => Some(*tick),
                _ => None,
            })
            .collect();
        assert!(!stalls.is_empty(), "peer {p} waited for the host");
        let last = *stalls.last().unwrap();
        assert!(last < 1_600, "peer {p} still stalled at tick {last}: {stalls:?}");
    }
}
