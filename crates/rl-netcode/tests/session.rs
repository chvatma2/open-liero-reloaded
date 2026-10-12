//! The session over the network model with the toy simulation (spec §9.1):
//! the verdict holds for every profile and many seeds.

use proptest::prelude::*;
use rl_netcode::harness::{self, HOST, Outage, PeerPlan, Scenario};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{Channel, Config, Role, Seat, Session, ToySim, input_delay};

fn scenario(seed: u64, profile: Profile, clients: u8, spectators: u8) -> Scenario {
    let slots = clients + 2; // two host-local slots (a human and a bot)
    let mut peers = Vec::new();
    for c in 0..clients {
        peers.push(PeerPlan {
            id: 1 + c as u16,
            seat: Seat::Player(2 + c),
            profile,
            clock_ppm: 0,
            input_delay: input_delay(profile.ping_ms),
            frame_us: 6_944,
            join_tick: None,
            freeze: None,
        });
    }
    for s in 0..spectators {
        peers.push(PeerPlan {
            id: 100 + s as u16,
            seat: Seat::Spectator,
            profile,
            clock_ppm: 0,
            input_delay: 0,
            frame_us: 16_667,
            join_tick: None,
            freeze: None,
        });
    }
    Scenario {
        seed,
        slots,
        peers,
        ticks: 1500,
        settle_ticks: 200,
        outages: Vec::new(),
        expect_no_stalls: false,
        corrupt: vec![],
    }
}

fn check(sc: &Scenario) {
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let report = harness::run(sc, &mut f).unwrap_or_else(|e| panic!("seed {}: {e:?}", sc.seed));
    harness::verdict(sc, &report, &mut f)
        .unwrap_or_else(|e| panic!("seed {} {:?}: {e:?}", sc.seed, sc.peers[0].profile.name));
}

#[test]
fn every_profile_converges() {
    for profile in Profile::ALL {
        for seed in 1..=20 {
            check(&scenario(seed, profile, 3, 1));
        }
    }
}

#[test]
fn a_lone_host_plays_without_clients() {
    check(&scenario(1, Profile::LAN, 0, 0));
}

#[test]
fn rollbacks_happen_and_are_counted() {
    let sc = scenario(3, Profile::TYPICAL, 3, 0);
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    let client = &r.stats[&1];
    assert!(client.rollbacks > 0);
    assert!(client.depth_hist.iter().sum::<u64>() == client.rollbacks);
    assert_eq!(r.stats[&HOST].own_corrections, 0);
}

#[test]
fn an_outage_recovers() {
    let mut sc = scenario(5, Profile::GOOD, 2, 0);
    sc.outages.push(Outage { peer: 1, from_tick: 400, until_tick: 450 });
    check(&sc);
}

#[test]
fn the_input_delay_formula() {
    assert_eq!([0, 39, 40, 80, 119, 150, 400].map(input_delay), [0, 0, 1, 2, 2, 3, 3]);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn receive_never_panics(packets in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..1300), 1..20)) {
        let mut host = Session::new(Config::new(4, 0), Role::Host { peers: vec![(1, Seat::Player(1))] }, ToySim::new(4), 0);
        let mut client = Session::new(Config::new(4, 1), Role::Client { host: 0, slot: 1 }, ToySim::new(4), 0);
        for (i, p) in packets.iter().enumerate() {
            let ch = if i % 2 == 0 { Channel::Unreliable } else { Channel::Reliable };
            host.receive(1, ch, p, i as u64 * 1000);
            client.receive(0, ch, p, i as u64 * 1000);
        }
    }
}
