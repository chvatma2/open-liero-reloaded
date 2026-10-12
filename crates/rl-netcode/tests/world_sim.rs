//! The real world under rollback: snapshots across terrain edits, state
//! transfer, the harness with bots-free scripted inputs (MW 1.0 on Castle
//! Strike) and colour maps that follow rollbacks (MW 1.0 on Dirt Level);
//! skipped with a notice without `RL_CONTENT_DIR`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rl_content::{Content, MatchOptions, Player};
use rl_netcode::harness::{self, Factory, Observer, PeerPlan, Scenario};
use rl_netcode::netsim::Profile;
use rl_netcode::{Driver, PeerId, Seat, Session, Simulation, TickInfo, WorldSim, input_delay};
use rl_sim::Keys;
use rl_terrain::{Area, ColorEdit, ColorMap};

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

/// MW 1.0 on Dirt Level, which digging and explosions carve (Castle
/// Strike's terrain is rock and air).
const DIRT_LEVEL: &str = "olx-level-dirt-level";

fn dirt_world(c: &Content, seed: u32) -> rl_sim::World {
    c.build_match(&MatchOptions { level_id: DIRT_LEVEL.into(), ..options(seed) }).unwrap().world
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

/// The presentation learns what a restore put back.
#[test]
fn restore_reports_the_areas_it_put_back_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut a = WorldSim::new(dirt_world(&c, 1));
    for t in 0..200 {
        a.tick(&keys(t));
    }
    let s = a.save();
    for t in 200..400 {
        a.tick(&keys(t + 7));
    }
    let carved = a.world().terrain().clone();
    assert!(a.take_restored_areas().is_empty(), "ticks restore nothing");
    a.restore(&s);
    let areas = a.take_restored_areas();
    let covered = |x: i64, y: i64| {
        areas
            .iter()
            .any(|r| (r.x0 as i64..r.x1 as i64).contains(&x) && (r.y0 as i64..r.y1 as i64).contains(&y))
    };
    let t = a.world().terrain();
    let (w, h) = (t.width() as i64, t.height() as i64);
    let mut put_back = 0;
    for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
        if carved.material(x, y) != t.material(x, y) {
            assert!(covered(x, y), "({x}, {y}) was put back unreported");
            put_back += 1;
        }
    }
    assert!(put_back > 0, "the other future carved");
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
    let (w, h) = (b.world().terrain().width() as i32, b.world().terrain().height() as i32);
    assert_eq!(
        b.take_restored_areas(),
        [Area { x0: 0, y0: 0, x1: w, y1: h }],
        "a load replaces the whole map"
    );
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

/// What a peer's presentation does with the terrain: the edits of every
/// tick simulated (re-simulated ones included), in order, since its last
/// frame.
#[derive(Default)]
struct Painted {
    edits: BTreeMap<PeerId, Vec<Paint>>,
}

enum Paint {
    Edit(ColorEdit),
    Lost(Area),
}

/// [`Scripted`], keeping each simulated tick's terrain edits for the
/// presentation.
struct Painter {
    peer: PeerId,
    painted: Rc<RefCell<Painted>>,
}

impl Driver<WorldSim> for Painter {
    fn local_inputs(&mut self, sim: &WorldSim, tick: u32, slots: &[u8], out: &mut [Keys]) {
        Scripted.local_inputs(sim, tick, slots, out);
    }

    fn simulated(&mut self, sim: &WorldSim, _info: TickInfo) {
        let mut p = self.painted.borrow_mut();
        let edits = p.edits.entry(self.peer).or_default();
        let events = sim.events();
        edits.extend(events.iter().filter_map(|e| rl_content::color_edit(&e.kind)).map(Paint::Edit));
        if let Some(a) = events.lost_area() {
            edits.push(Paint::Lost(rl_content::lost_area(a)));
        }
    }
}

struct PaintingWorlds<'a> {
    content: &'a Content,
    painted: Rc<RefCell<Painted>>,
}

impl Factory<WorldSim> for PaintingWorlds<'_> {
    fn sim(&mut self, _peer: PeerId) -> WorldSim {
        WorldSim::new(dirt_world(self.content, 3))
    }

    fn driver(&mut self, peer: PeerId, _seat: Option<Seat>) -> Box<dyn Driver<WorldSim>> {
        Box::new(Painter { peer, painted: self.painted.clone() })
    }
}

/// Every peer's colour map, kept as plan 10's game keeps it: after each
/// frame, the areas the session's restores and loads put back are rebuilt
/// from the terrain, then the frame's edits are applied in order.
struct Presentations {
    blank: ColorMap,
    maps: BTreeMap<PeerId, ColorMap>,
    painted: Rc<RefCell<Painted>>,
    rebuilt: usize,
}

impl Observer<WorldSim> for Presentations {
    fn on_frame(&mut self, peer: PeerId, session: &mut Session<WorldSim>) {
        let map = self.maps.entry(peer).or_insert_with(|| self.blank.clone());
        let areas = session.take_restored_areas();
        let terrain = session.sim().world().terrain();
        for &a in &areas {
            map.rebuild_area(a, terrain);
        }
        self.rebuilt += areas.len();
        for p in self.painted.borrow_mut().edits.remove(&peer).unwrap_or_default() {
            match p {
                Paint::Edit(e) => map.apply(e),
                Paint::Lost(a) => map.rebuild_area(a, terrain),
            }
        }
        let _ = map.take_dirty().count();
        // Before its first tick (a joining spectator's: before its state),
        // the setup's spawn holes are not reported yet.
        assert!(
            session.current_tick() == 0 || map.material() == terrain,
            "peer {peer}, tick {}: the colour map shows terrain the world no longer has",
            session.current_tick()
        );
    }
}

/// Rollbacks undo mispredicted carving; the colour maps follow, and a late
/// spectator's follows its loaded state.
#[test]
fn colour_maps_follow_rollbacks_and_loads() {
    let Some(c) = content() else { return };
    let level = c.load_level(DIRT_LEVEL).unwrap();
    let theme = c.load_theme(&level.theme).unwrap();
    let plan = |id: u16, seat, join_tick| PeerPlan {
        id,
        seat,
        profile: Profile::BAD,
        clock_ppm: 0,
        input_delay: input_delay(Profile::BAD.ping_ms),
        frame_us: 6_944,
        join_tick,
        freeze: None,
    };
    let sc = Scenario {
        seed: 5,
        slots: 8,
        peers: vec![
            plan(1, Seat::Player(5), None),
            plan(2, Seat::Player(6), None),
            plan(3, Seat::Player(7), None),
            plan(4, Seat::Spectator, Some(150)),
        ],
        ticks: 400,
        settle_ticks: 100,
        outages: vec![],
        expect_no_stalls: false,
        corrupt: vec![],
    };
    let painted = Rc::new(RefCell::new(Painted::default()));
    let mut f = PaintingWorlds { content: &c, painted: painted.clone() };
    let mut obs =
        Presentations { blank: level.color_map(&theme), maps: BTreeMap::new(), painted, rebuilt: 0 };
    let r = harness::run_observed(&sc, &mut f, &mut obs).unwrap();
    assert!(r.stats[&1].rollbacks > 0);
    assert_eq!(r.stats[&4].states_loaded, 1);
    assert!(obs.rebuilt > 1, "areas were put back: {}", obs.rebuilt);
    eprintln!("{} areas rebuilt", obs.rebuilt);
}
