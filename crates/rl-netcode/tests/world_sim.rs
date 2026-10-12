//! The real world under rollback: snapshots across terrain edits, state
//! transfer and the harness with bots-free scripted inputs (MW 1.0 on Castle
//! Strike; skipped with a notice without `RL_CONTENT_DIR`).

use rl_content::{Content, MatchOptions, Player};
use rl_netcode::harness::{self, Factory, PeerPlan, Scenario};
use rl_netcode::netsim::Profile;
use rl_netcode::{Driver, PeerId, Seat, Simulation, WorldSim, input_delay};
use rl_sim::Keys;

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(std::path::Path::new(&dir)).expect("the content directory"))
}

/// MW 1.0 on Castle Strike: heavy carving. Built with the bots' loadouts,
/// then driven with scripted keys.
fn options(seed: u32) -> MatchOptions {
    MatchOptions::deathmatch(
        "olx-mw-1-0",
        "olx-level-castlestrike",
        seed,
        (0..8).map(|_| Player::Bot(Default::default())).collect(),
    )
}

fn world(c: &Content, seed: u32) -> rl_sim::World {
    c.build_match(&options(seed)).unwrap().world
}

fn keys(t: u32) -> Vec<Keys> {
    (0..8u32)
        .map(|i| {
            Keys(if (t / 9 + i).is_multiple_of(4) { Keys::FIRE | Keys::RIGHT } else { Keys::FIRE | Keys::UP })
        })
        .collect()
}

#[test]
fn restore_undoes_terrain_edits_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut a = WorldSim::new(world(&c, 1));
    let mut b = WorldSim::new(world(&c, 1));
    for t in 0..200 {
        a.tick(&keys(t));
        b.tick(&keys(t));
    }
    let s = a.save();
    for t in 200..400 {
        a.tick(&keys(t + 7)); // a different future
    }
    a.restore(&s);
    for t in 200..400 {
        a.tick(&keys(t));
        b.tick(&keys(t));
    }
    assert_eq!(a.world().checksum(), b.world().checksum());
    assert_eq!(a.world(), b.world());
}

#[test]
fn state_transfer_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut a = WorldSim::new(world(&c, 2));
    for t in 0..300 {
        a.tick(&keys(t));
    }
    let mut b = WorldSim::new(world(&c, 2));
    b.decode_state(&a.encode_state()).unwrap();
    assert_eq!(a.checksum(), b.checksum());
    for t in 300..500 {
        a.tick(&keys(t));
        b.tick(&keys(t));
    }
    assert_eq!(a.world().checksum(), b.world().checksum());
    assert_eq!(b.decode_state(&[0, 1, 2]).map_err(|_| ()), Err(()));
}

/// Every peer builds the same world; drivers press scripted keys.
struct Worlds<'a> {
    content: &'a Content,
}

struct Scripted;

impl Driver<WorldSim> for Scripted {
    fn local_inputs(&mut self, _sim: &WorldSim, tick: u32, slots: &[u8], out: &mut [Keys]) {
        let all = keys(tick);
        for (&s, k) in slots.iter().zip(out.iter_mut()) {
            *k = all[s as usize];
        }
    }
}

impl Factory<WorldSim> for Worlds<'_> {
    fn sim(&mut self, _peer: PeerId) -> WorldSim {
        WorldSim::new(world(self.content, 3))
    }

    fn driver(&mut self, _peer: PeerId, _seat: Option<Seat>) -> Box<dyn Driver<WorldSim>> {
        Box::new(Scripted)
    }
}

#[test]
fn the_world_plays_over_the_network_with_a_late_spectator() {
    let Some(c) = content() else { return };
    let plan = |id: u16, seat, join_tick| PeerPlan {
        id,
        seat,
        profile: Profile::TYPICAL,
        clock_ppm: 0,
        input_delay: input_delay(Profile::TYPICAL.ping_ms),
        frame_us: 6_944,
        join_tick,
        freeze: None,
    };
    let peers = vec![
        plan(1, Seat::Player(6), None),
        plan(2, Seat::Player(7), None),
        plan(3, Seat::Spectator, Some(200)),
    ];
    let sc = Scenario {
        seed: 3,
        slots: 8,
        peers,
        ticks: 400,
        settle_ticks: 100,
        outages: vec![],
        expect_no_stalls: true,
        corrupt: vec![],
    };
    let mut f = Worlds { content: &c };
    let r = harness::run(&sc, &mut f).unwrap();
    harness::verdict(&sc, &r, &mut f).unwrap();
    assert!(r.stats[&1].rollbacks > 0);
    assert_eq!(r.stats[&3].states_loaded, 1);
}
