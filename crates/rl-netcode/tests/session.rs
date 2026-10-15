//! The session over the network model with the toy simulation (spec §9.1):
//! the verdict holds for every profile and many seeds.

use proptest::prelude::*;
use rl_netcode::harness::Factory;
use rl_netcode::harness::{self, HOST, Outage, PeerPlan, Scenario};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{Channel, Config, Role, Seat, Session, Simulation, ToySim, input_delay};
use rl_proto::{
    ClientInputs, Control, Datagram, Frame, HostInputs, MAX_FRAMES, MAX_KEYS, Stamp, encode_control,
    encode_datagram,
};
use rl_sim::Keys;

fn scenario(seed: u64, profile: Profile, clients: u8, spectators: u8) -> Scenario {
    let slots = clients + 2; // two host-local slots (a human and a bot)
    let mut peers = Vec::new();
    for c in 0..clients {
        peers.push(PeerPlan {
            id: 1 + c as u32,
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
            id: 100 + s as u32,
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
        host_freeze: None,
        exits: vec![],
        migration: None,
        profile_changes: vec![],
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

#[test]
fn sim_mut_is_hidden_from_the_docs() {
    // A test and harness hook, like `change_confirmed`.
    let src = include_str!("../src/session.rs");
    assert!(src.contains("#[doc(hidden)]\n    pub fn sim_mut("), "sim_mut is documented as API");
}

/// `0`, `u32::MAX` and their neighbours, values around the current tick
/// (the sessions run tens of ticks), or anything.
fn edge_u32() -> impl Strategy<Value = u32> {
    prop_oneof![Just(0), Just(1), Just(u32::MAX), Just(u32::MAX - 1), 0u32..80, any::<u32>()]
}

fn stamp() -> impl Strategy<Value = Stamp> {
    (edge_u32(), edge_u32(), prop_oneof![Just(0u16), Just(u16::MAX), any::<u16>()])
        .prop_map(|(sent_ms, echo_ms, held_ms)| Stamp { sent_ms, echo_ms, held_ms })
}

/// A frame that decodes for 4 slots.
fn frame() -> impl Strategy<Value = Frame> {
    (0u32..16, 0u32..16, prop::collection::vec(any::<u8>(), 4)).prop_map(|(known, changed, keys)| {
        let changed = changed & known;
        Frame { known, changed, keys: keys[..changed.count_ones() as usize].to_vec() }
    })
}

/// A well-formed datagram with edge values.
fn datagram() -> impl Strategy<Value = Datagram> {
    let margin = prop_oneof![Just(i16::MIN), Just(i16::MAX), Just(0i16), -40i16..40, any::<i16>()];
    prop_oneof![
        (stamp(), edge_u32(), edge_u32(), edge_u32(), prop::collection::vec(any::<u8>(), 0..=MAX_KEYS))
            .prop_map(|(stamp, tick, ack, first, keys)| Datagram::Client(ClientInputs {
                stamp,
                tick,
                ack,
                first,
                keys
            })),
        (stamp(), edge_u32(), edge_u32(), margin, edge_u32(), prop::collection::vec(frame(), 0..=MAX_FRAMES))
            .prop_map(|(stamp, tick, your_ack, margin, first, frames)| Datagram::Host(HostInputs {
                stamp,
                tick,
                your_ack,
                margin,
                first,
                frames
            })),
    ]
}

/// Every final input a session knew stays as it was.
#[derive(Default)]
struct Finals(Vec<Vec<u8>>);

impl Finals {
    fn check(&mut self, s: &Session<ToySim>) {
        let log = s.input_log();
        self.0.resize(4, Vec::new());
        for (slot, known) in self.0.iter_mut().enumerate() {
            let slot = slot as u8;
            for (i, &v) in known.iter().enumerate() {
                assert_eq!(log.get(log.origin() + i as u32, slot), Some(v), "slot {slot} tick {i} changed");
            }
            let upto = log.known_upto(slot);
            for t in log.origin() + known.len() as u32..upto {
                known.push(log.get(t, slot).expect("known"));
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Well-formed datagrams with edge values never panic a client, a
    /// spectator, a joining spectator, the host or a spectator joined from
    /// a state (its log's origin past 0), and never change an input it
    /// took as final.
    #[test]
    fn edge_datagrams_never_panic_or_change_final_inputs(datagrams in prop::collection::vec(datagram(), 1..24)) {
        let host = Role::Host { peers: vec![(1, Seat::Player(1)), (2, Seat::Spectator)] };
        // A spectator joined from the state after tick 30: its log begins at 31.
        let mut joined = Session::new(Config::new(4, 0), Role::Joining { host: 0 }, ToySim::new(4), 0);
        let mut state = ToySim::new(4);
        for _ in 0..=30 {
            state.tick(&[Keys(3); 4]);
        }
        let bytes = state.encode_state();
        let begin = Control::StateBegin { tick: 30, len: bytes.len() as u32, chunks: 1 };
        joined.receive(0, Channel::Reliable, &encode_control(&begin), 0);
        joined.receive(0, Channel::Reliable, &encode_control(&Control::StateChunk { index: 0, bytes }), 0);
        prop_assert_eq!(joined.input_log().origin(), 31);
        let mut sessions = [
            (0, Session::new(Config::new(4, 1), Role::Client { host: 0, slot: 1 }, ToySim::new(4), 0)),
            (0, Session::new(Config::new(4, 0), Role::Spectator { host: 0 }, ToySim::new(4), 0)),
            (0, Session::new(Config::new(4, 0), Role::Joining { host: 0 }, ToySim::new(4), 0)),
            (1, Session::new(Config::new(4, 0), host, ToySim::new(4), 0)),
            (0, joined),
        ];
        let mut f = ToyFactory { slots: 4, seed: 1 };
        for (from, s) in sessions.iter_mut() {
            let mut drv = f.driver(5, None);
            let mut finals = Finals::default();
            for (i, d) in datagrams.iter().enumerate() {
                let now = (i as u64 + 1) * 10_000;
                s.receive(*from, Channel::Unreliable, &encode_datagram(d), now);
                finals.check(s);
                s.advance(now, drv.as_mut());
                s.transmit(&mut |_, _, _| {});
                finals.check(s);
            }
        }
    }
}
