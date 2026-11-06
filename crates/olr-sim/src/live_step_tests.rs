//! Unit tests of solid live parts (issue #90): standing, carrying,
//! pushing, stopping, crushing, appearing parts, the rope, spawns, and the
//! step through a restore and a state transfer.
use super::*;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::MaterialGrid;
use olr_math::TimeDiff;
use olr_terrain::{PX_ROCK, Stamp};

use crate::effects::{Blast, explosion};
use crate::events::{CosmeticKind, StampId, TerrainOp, TickEvents};
use crate::input::Keys;
use crate::live::{
    LIVE_SUBPIXELS, LiveCopy, LiveCopySetup, LiveMotion, LivePart, LivePartKind, LiveProp, LiveSolid,
    LiveState, LiveTarget,
};
use crate::projectile::Projectile;
use crate::script::{Director, LiveCommand, LiveCtx};
use crate::solid::OnParts;
use crate::{Loadout, MatchSetup, OracleMode, SimSettings, WormSetup};

const PX: i64 = LIVE_SUBPIXELS;
/// The block's values.
const X: u16 = 0;
const Y: u16 = 1;
const SHOWN: u16 = 2;

/// A rock block of `size` at `at`: `X` and `Y` within ±300 px, its layer
/// shown while `SHOWN` (starting at `shown`) is 1.
fn block(id: &str, at: (i32, i32), size: (u32, u32), shown: i64) -> LiveCopy {
    let prop = |target, min, max, default| LiveProp { name: None, target, min, max, default };
    let part =
        LivePart { kind: LivePartKind::Rock, layer: Some(0), at: (0, 0), picture: 0, glow_strength: 100 };
    LiveCopy::new(LiveCopySetup {
        id: id.into(),
        at,
        size,
        props: vec![
            prop(LiveTarget::X, -300, 300, 0),
            prop(LiveTarget::Y, -300, 300, 0),
            prop(LiveTarget::LayerVisible(0), 0, 1, shown),
        ],
        states: vec![LiveState {
            name: "On".into(),
            values: Vec::new(),
            parts: vec![part],
            enter_sound: None,
            loop_sound: None,
        }],
        pictures: vec![LiveSolid::new(size, true, |_, _| true)],
        start_state: 0,
        start_values: Vec::new(),
        health: None,
        scripts: Vec::new(),
        sounds: Vec::new(),
    })
}

fn holes() -> Vec<Stamp> {
    [5, 7, 9, 11, 15]
        .into_iter()
        .map(|size| {
            let mut img = RgbaImage::new(size, size);
            for y in 0..size {
                for x in 0..size {
                    img.set(x, y, Rgba::MAGENTA);
                }
            }
            Stamp::from_image(&img)
        })
        .collect()
}

/// A 200 × 200 cave: rock from row 180 down, dirt in `dirt` (x0, y0, x1, y1).
fn terrain(dirt: Option<(u32, u32, u32, u32)>) -> Terrain {
    let mut cells = vec![Material::Background; 200 * 200];
    for y in 0..200u32 {
        for x in 0..200u32 {
            let m = &mut cells[(y * 200 + x) as usize];
            if y >= 180 {
                *m = Material::Rock;
            } else if dirt.is_some_and(|(x0, y0, x1, y1)| (x0..x1).contains(&x) && (y0..y1).contains(&y)) {
                *m = Material::Dirt;
            }
        }
    }
    Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap())
}

/// A director that, with `ping_pong`, sends copy 0's `X` between 0 and
/// 20 px at 1 px a tick; else does nothing. Motions need a director to
/// travel in a state transfer.
#[derive(Clone, Debug)]
struct Steer {
    ping_pong: bool,
}

impl Director for Steer {
    fn box_clone(&self) -> Box<dyn Director> {
        Box::new(self.clone())
    }

    fn timer_functions(&self, _script: u16) -> u16 {
        0
    }

    fn run(&self, ctx: &mut LiveCtx<'_>) {
        if !self.ping_pong {
            return;
        }
        let Some(c) = ctx.view().copy(0) else { return };
        if !c.motions()[usize::from(X)].moving() {
            let target = if c.values()[usize::from(X)] > 0 { 0 } else { 20 * PX };
            ctx.apply(LiveCommand::Move { copy: 0, prop: X, target, speed: PX });
        }
    }
}

fn setup(copies: Vec<LiveCopy>, director: Option<Steer>, dirt: Option<(u32, u32, u32, u32)>) -> MatchSetup {
    let level = Arc::new(LiveLevel::new(copies, Vec::new(), None, 0));
    MatchSetup {
        holes: holes(),
        live: Some(level),
        director: director.map(|d| Box::new(d) as Box<dyn Director>),
        ..MatchSetup::new(SimSettings::lx56(), crate::mod_rt::tests::test_mod(), terrain(dirt))
    }
}

fn world(copies: Vec<LiveCopy>) -> World {
    World::build(setup(copies, None, None), Box::new(OracleMode))
}

fn ticks(w: &mut World, n: usize) {
    for _ in 0..n {
        w.tick_new(&[Keys(0)]);
    }
}

fn move_to(w: &mut World, copy: u16, prop: u16, px: i64, speed_px: i64) {
    w.live.as_mut().unwrap().move_value(copy, prop, px * PX, speed_px * PX).unwrap();
}

fn rect(w: &World, copy: u16) -> (i32, i32, u32, u32) {
    w.live().unwrap().copy(copy).unwrap().rect()
}

fn at(w: &World) -> (i32, i32) {
    let p = w.worms()[0].pos;
    (p.x as i32, p.y as i32)
}

/// A worm standing on a 40 × 4 platform at (80, 100): its feet at row 100.
fn standing() -> World {
    let mut w = world(vec![block("lift", (80, 100), (40, 4), 1)]);
    w.spawn_worm(100, 80);
    ticks(&mut w, 150);
    assert_eq!(at(&w), (100, 95), "on the platform");
    w
}

#[test]
fn a_worm_stands_on_a_live_rock_part() {
    let w = standing();
    assert!(w.worms()[0].on_ground);
    assert_eq!(w.solid().flags(100, 100), PX_ROCK);
    assert_eq!(w.terrain().flags(100, 100) & PX_ROCK, 0, "the ground under it is unchanged");
}

#[test]
fn a_moving_part_carries_a_standing_worm() {
    let mut w = standing();
    let x0 = w.worms()[0].pos.x;
    move_to(&mut w, 0, X, 20, 1);
    ticks(&mut w, 30);
    assert_eq!(rect(&w, 0).0, 100);
    assert!((w.worms()[0].pos.x - x0 - 20.0).abs() < 0.01, "carried sideways: {}", w.worms()[0].pos.x);
    assert_eq!(at(&w).1, 95);
    move_to(&mut w, 0, Y, 30, 1);
    ticks(&mut w, 40);
    assert_eq!(rect(&w, 0).1, 130);
    assert_eq!(at(&w).1, 125, "carried down");
}

#[test]
fn a_part_moving_up_pushes_a_worm_up() {
    let mut w = standing();
    move_to(&mut w, 0, Y, -40, 2);
    ticks(&mut w, 30);
    assert_eq!(rect(&w, 0).1, 60);
    assert_eq!(at(&w).1, 55, "pushed up with it");
}

fn blocked_events(e: &TickEvents) -> usize {
    e.iter().filter(|e| matches!(e.kind, EventKind::LiveBlocked { copy: 0, .. })).count()
}

#[test]
fn a_part_stops_at_ground_rock_and_another_part() {
    // Ground rock from row 180: a 4-high block at row 170 goes 6 px down.
    let mut w = world(vec![block("a", (80, 170), (40, 4), 1)]);
    move_to(&mut w, 0, Y, 30, 8);
    let mut blocked = 0;
    for _ in 0..10 {
        blocked += blocked_events(&w.tick_new(&[]));
    }
    assert_eq!(blocked, 1);
    assert_eq!(rect(&w, 0).1, 176, "it stops on the rock");
    let c = w.live().unwrap().copy(0).unwrap();
    assert_eq!(c.values()[usize::from(Y)].div_euclid(PX), 6, "its value at the last place reached");
    assert_eq!(c.motions()[usize::from(Y)], LiveMotion::default(), "the motion ended");
    // Another part: `a` goes right into `b`, 30 px to its right.
    let mut w = world(vec![block("a", (20, 50), (10, 10), 1), block("b", (60, 50), (10, 10), 1)]);
    move_to(&mut w, 0, X, 100, 4);
    let mut blocked = 0;
    for _ in 0..20 {
        blocked += blocked_events(&w.tick_new(&[]));
    }
    assert_eq!(blocked, 1);
    assert_eq!(rect(&w, 0).0, 50, "it stops against b");
    assert_eq!(rect(&w, 1).0, 60, "b stays");
}

/// A worm standing on the floor (feet at row 180) under a 20 × 10 block
/// coming down at 1 px a tick.
fn under_a_press() -> World {
    let mut w = World::build(
        setup(vec![block("press", (90, 140), (20, 10), 1)], Some(Steer { ping_pong: false }), None),
        Box::new(OracleMode),
    );
    w.spawn_worm(100, 160);
    ticks(&mut w, 100);
    assert_eq!(at(&w), (100, 175), "on the floor");
    move_to(&mut w, 0, Y, 100, 1);
    w
}

#[test]
fn a_wedged_worm_stops_the_part() {
    let mut w = under_a_press();
    ticks(&mut w, 40);
    let (_, y, _, h) = rect(&w, 0);
    assert_eq!(y + h as i32, 171, "it stops above the worm's head (row 171)");
    assert_eq!(at(&w), (100, 175));
    assert!(w.crush_count(0) > 0);
    assert!(w.live().unwrap().copy(0).unwrap().motions()[usize::from(Y)].moving(), "it goes on trying");
}

#[test]
fn crushing_comes_in_chunks_with_a_marker() {
    let mut w = under_a_press();
    let mut chunks = Vec::new();
    for t in 0..100 {
        if chunks.len() == 3 {
            break;
        }
        let e = w.tick_new(&[]);
        let kinds: Vec<EventKind> = e.iter().map(|e| e.kind).collect();
        if let Some(i) = kinds.iter().position(|k| matches!(k, EventKind::Crushed { worm: 0, copy: 0 })) {
            let next = kinds[i + 1..]
                .iter()
                .find(|k| !matches!(k, EventKind::SpawnCosmetic { .. } | EventKind::Sound { .. }));
            assert!(
                matches!(next, Some(EventKind::Damage { worm: 0, amount, by: -1, .. }) if *amount == CRUSH_CHUNK),
                "the chunk follows its marker: {next:?}"
            );
            let droppers = kinds
                .iter()
                .filter(|k| matches!(k, EventKind::SpawnCosmetic { kind: CosmeticKind::BloodDropper, .. }))
                .count();
            assert_eq!(droppers, 10, "ten blood rounds a chunk");
            chunks.push(t);
        }
        assert!(
            !kinds.iter().any(|k| matches!(k, EventKind::Damage { worm: 0, .. }))
                || !chunks.is_empty() && *chunks.last().unwrap() == t
        );
    }
    assert_eq!(chunks.len(), 3, "{chunks:?}");
    assert_eq!(chunks[1] - chunks[0], CRUSH_EVERY as usize);
    assert_eq!(chunks[2] - chunks[1], CRUSH_EVERY as usize);
    assert_eq!(w.worms()[0].health, 100.0 - 3.0 * CRUSH_CHUNK);
    // A free tick starts the count again.
    let y = w.live().unwrap().copy(0).unwrap().values()[usize::from(Y)];
    w.set_live_value(0, Y, y - 2 * PX).unwrap();
    ticks(&mut w, 1);
    assert_eq!(w.crush_count(0), 0);
    move_to(&mut w, 0, Y, 100, 1);
    let first = (0..5).find(|_| {
        let e = w.tick_new(&[]);
        e.iter().any(|e| matches!(e.kind, EventKind::Crushed { worm: 0, .. }))
    });
    assert!(first.is_some(), "a chunk on the first wedged tick again");
    assert_eq!(w.crush_count(0), 1);
}

#[test]
fn a_crushed_worm_dies_credited_to_nobody() {
    let mut w = under_a_press();
    let mut kill = None;
    for _ in 0..400 {
        for e in w.tick_new(&[]).iter() {
            match e.kind {
                EventKind::Kill { victim, killer } => kill = Some((victim, killer)),
                EventKind::Damage { by, .. } => assert_eq!(by, -1),
                _ => {}
            }
        }
        if kill.is_some() {
            break;
        }
    }
    assert_eq!(kill, Some((0, -1)), "killed by nobody");
    assert_eq!(w.worms()[0].suicides, 0);
    assert_eq!(w.worms()[0].kills, 0);
}

#[test]
fn a_fixed_spawn_inside_a_part_falls_back_to_find_spot() {
    let mut s = setup(vec![block("slab", (80, 90), (40, 20), 1)], None, None);
    let s2 = s.clone();
    let loadout = Loadout::new(Vec::new(), &s.settings);
    s.worms = vec![WormSetup { team: 0, loadout: loadout.clone(), spawn: Some((100, 100)) }];
    let w = World::build(s, Box::new(OracleMode));
    let (x, y) = at(&w);
    assert_ne!((x, y), (100, 100));
    assert!(w.solid().spawn_clear_of_parts(x, y));
    assert!(w.mt_calls() > 0, "it drew a FindSpot place");
    // Beside the part, a fixed place stands.
    let mut s = s2;
    s.worms = vec![WormSetup { team: 0, loadout, spawn: Some((100, 60)) }];
    let w = World::build(s, Box::new(OracleMode));
    assert_eq!(at(&w), (100, 60));
    assert_eq!(w.mt_calls(), 0);
}

#[test]
fn the_crush_count_survives_a_restore_and_a_transfer() {
    let mut w = under_a_press();
    w.enable_rollback();
    while w.crush_count(0) < 3 {
        ticks(&mut w, 1);
    }
    let snap = w.save();
    let state = w.state();
    ticks(&mut w, 5);
    assert_eq!(w.crush_count(0), 8);
    let later = w.checksum();
    w.restore(&snap).unwrap();
    assert_eq!(w.crush_count(0), 3);
    let mut other = World::build(
        setup(vec![block("press", (90, 140), (20, 10), 1)], Some(Steer { ping_pong: false }), None),
        Box::new(OracleMode),
    );
    other.load_state(state.clone()).unwrap();
    assert_eq!(other.crush_count(0), 3);
    assert_eq!(other.checksum(), w.checksum());
    ticks(&mut w, 5);
    ticks(&mut other, 5);
    assert_eq!(w.checksum(), later);
    assert_eq!(other.checksum(), later);
    // A transfer without a count per worm slot is refused.
    let mut bad = state;
    bad.live_mut_for_test().as_mut().unwrap().crush_mut_for_test().pop();
    assert_eq!(other.load_state(bad), Err(crate::StateError::Live));
}

#[test]
fn an_appearing_part_pushes_a_worm_out_or_wedges_it() {
    // A 6 × 20 door at x 97..103 shown over the worm: right, 6 px, is the
    // shortest way out (left takes 7, up more than 8).
    let mut w = world(vec![block("door", (97, 160), (6, 20), 0)]);
    w.spawn_worm(100, 160);
    ticks(&mut w, 100);
    assert_eq!(at(&w), (100, 175));
    w.set_live_value(0, SHOWN, 1).unwrap();
    ticks(&mut w, 1);
    assert_eq!(at(&w), (106, 175), "pushed out to the right");
    assert_eq!(w.crush_count(0), 0);
    // A wide one leaves nowhere within 8 px: wedged while it is there.
    let mut w = world(vec![block("door", (70, 160), (60, 20), 0)]);
    w.spawn_worm(100, 160);
    ticks(&mut w, 100);
    w.set_live_value(0, SHOWN, 1).unwrap();
    let e = w.tick_new(&[]);
    assert!(e.iter().any(|e| matches!(e.kind, EventKind::Crushed { worm: 0, copy: 0 })));
    assert_eq!(at(&w), (100, 175));
    ticks(&mut w, 3);
    assert_eq!(w.crush_count(0), 4);
    w.set_live_value(0, SHOWN, 0).unwrap();
    ticks(&mut w, 1);
    assert_eq!(w.crush_count(0), 0);
}

#[test]
fn a_rope_hooked_to_a_part_follows_it() {
    let mut w = world(vec![block("lift", (80, 100), (40, 4), 1)]);
    w.spawn_worm(100, 160);
    ticks(&mut w, 100);
    let r = &mut w.worm_mut(0).rope;
    (r.released, r.attached, r.shooting, r.player_attached) = (true, true, false, -1);
    r.pos = Vec2::new(100.5, 101.5);
    r.vel = Vec2::ZERO;
    move_to(&mut w, 0, X, 10, 1);
    ticks(&mut w, 15);
    let r = &w.worms()[0].rope;
    assert!(r.attached, "still hooked");
    assert_eq!(r.pos, Vec2::new(110.5, 101.5));
}

#[test]
fn projectiles_and_beams_stop_at_a_live_part() {
    // Over dirt (rows 40..70), the part reads rock: a projectile's box in
    // it is not in dirt only, and a beam stops there.
    let w = World::build(
        setup(vec![block("slab", (50, 50), (10, 10), 1)], None, Some((40, 40, 70, 70))),
        Box::new(OracleMode),
    );
    let parts = w.live.as_ref().unwrap().solid_parts().unwrap();
    let q = OnParts { terrain: &w.terrain, parts };
    let p = Projectile { radius: (1, 1), ..Projectile::default() };
    let c = crate::proj_sim::terrain_collision(&p, 55, 55, &q);
    assert!(c.collided && !c.only_dirt);
    let c = crate::proj_sim::terrain_collision(&p, 45, 45, &q);
    assert!(c.collided && c.only_dirt, "the ground's dirt beside it");
    let c = crate::proj_sim::terrain_collision(&p, 20, 20, &q);
    assert!(!c.collided);
    assert!(crate::beam::solid(&w.terrain, Some(parts), Vec2::new(55.5, 55.5)));
    assert!(!crate::beam::solid(&w.terrain, Some(parts), Vec2::new(20.5, 20.5)));
    // In play: a worm falls onto it and no further.
    let mut w =
        World::build(setup(vec![block("slab", (50, 50), (10, 10), 1)], None, None), Box::new(OracleMode));
    w.spawn_worm(55, 30);
    ticks(&mut w, 100);
    assert_eq!(at(&w), (55, 45));
}

#[test]
fn an_explosion_at_a_part_carves_only_the_ground() {
    let mut w = World::build(
        setup(vec![block("slab", (50, 50), (10, 10), 1)], None, Some((40, 40, 70, 70))),
        Box::new(OracleMode),
    );
    let now = w.time;
    let mut blast = Blast {
        settings: &w.settings,
        terrain: &mut w.terrain,
        holes: &w.holes,
        worms: &mut w.worms,
        draws: &mut w.draws,
        bonuses: &mut w.bonuses.client,
        now,
        wall_clock: now + TimeDiff(10),
        physics_dt: TimeDiff(11),
        referee: &mut w.referee,
        live: None,
    };
    explosion(&mut blast, now, Vec2::new(55.0, 55.0), 10.0, 0, -1, -1);
    assert_eq!(w.terrain.material(55, 55), Some(Material::Background), "the dirt under it is carved");
    assert_eq!(w.solid().material(55, 55), Some(Material::Rock), "the part is not");
    assert_eq!(w.live.as_ref().unwrap().solids.parts()[0].at, (50, 50));
}

#[test]
fn nothing_spawns_inside_a_part() {
    let mut w = world(vec![block("wall", (0, 0), (150, 180), 1)]);
    for _ in 0..50 {
        let (x, y) = w.find_spot();
        assert!(w.solid().spawn_clear_of_parts(x, y), "({x}, {y})");
        assert!(x >= 153, "({x}, {y})");
    }
}

#[test]
fn the_live_step_survives_a_restore_and_a_state_transfer() {
    let make = || {
        World::build(
            setup(vec![block("lift", (80, 100), (40, 4), 1)], Some(Steer { ping_pong: true }), None),
            Box::new(OracleMode),
        )
    };
    let mut w = make();
    w.spawn_worm(100, 80);
    w.enable_rollback();
    ticks(&mut w, 5);
    let snap = w.save();
    let mut sums = Vec::new();
    for _ in 0..80 {
        ticks(&mut w, 1);
        sums.push(w.checksum());
    }
    let rode = w.worms()[0].pos.x;
    w.restore(&snap).unwrap();
    assert_eq!(w.solid().live_rock(85, 100), rect(&w, 0).0 <= 85, "the index follows the restore");
    for (t, &s) in sums.iter().enumerate() {
        ticks(&mut w, 1);
        assert_eq!(w.checksum(), s, "tick {t} after the restore");
    }
    assert_eq!(w.worms()[0].pos.x, rode);
    // A transfer midway rides on alike.
    let state = w.state();
    let mut other = make();
    other.load_state(state).unwrap();
    for (x, y) in [(80, 100), (100, 101), (119, 103), (125, 100)] {
        assert_eq!(other.solid().flags(x, y), w.solid().flags(x, y), "({x}, {y})");
    }
    for t in 0..60 {
        ticks(&mut w, 1);
        ticks(&mut other, 1);
        assert_eq!(other.checksum(), w.checksum(), "tick {t} after the transfer");
    }
    assert!(w.worms()[0].on_ground, "it rides the lift");
}

#[test]
fn more_rock_parts_than_the_index_holds_are_refused() {
    let blocks =
        |n: usize| (0..n).map(|i| block(&alloc::format!("b{i}"), (2 * i as i32, 0), (1, 1), 1)).collect();
    assert!(World::new(setup(blocks(crate::MAX_LIVE_SOLID_PARTS), None, None), Box::new(OracleMode)).is_ok());
    let e = World::new(setup(blocks(crate::MAX_LIVE_SOLID_PARTS + 1), None, None), Box::new(OracleMode));
    assert_eq!(e.err(), Some(crate::SetupError::Live));
}

/// The crushed runs a tick reported: `(x, y, n)`.
fn crushed(e: &TickEvents) -> Vec<(i32, i32, u16)> {
    e.iter()
        .filter_map(|e| match e.kind {
            EventKind::TerrainEdit { op: TerrainOp::Crush, stamp: StampId::Row(n), x, y } => Some((x, y, n)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_moving_part_crushes_the_dirt_it_moves_into() {
    // A 20 × 10 block over dirt (rows 110..130, x 80..120) comes down 5 px.
    let mut w = World::build(
        setup(vec![block("press", (90, 100), (20, 10), 1)], None, Some((80, 110, 120, 130))),
        Box::new(OracleMode),
    );
    w.enable_rollback();
    let snap = w.save();
    move_to(&mut w, 0, Y, 5, 1);
    let mut runs = Vec::new();
    for _ in 0..8 {
        runs.extend(crushed(&w.tick_new(&[])));
    }
    assert_eq!(rect(&w, 0).1, 105, "dirt never stops it");
    assert_eq!(runs, (110..115).map(|y| (90, y, 20)).collect::<Vec<_>>(), "a run a row, as it went");
    for y in 110..115 {
        for x in 90..110 {
            assert_eq!(w.terrain().material(x, y), Some(Material::Background), "({x}, {y}) crushed");
        }
    }
    for (x, y) in [(89, 112), (110, 112), (100, 115)] {
        assert_eq!(w.terrain().material(x, y), Some(Material::Dirt), "({x}, {y}) outside it stays");
    }
    // Back up and down again over what it cleared: nothing more to crush.
    move_to(&mut w, 0, Y, 0, 1);
    ticks(&mut w, 8);
    move_to(&mut w, 0, Y, 5, 1);
    for _ in 0..8 {
        assert_eq!(crushed(&w.tick_new(&[])), [], "no dirt left there");
    }
    w.restore(&snap).unwrap();
    assert_eq!(w.terrain().material(100, 112), Some(Material::Dirt), "a restore puts it back");
}

/// [`block`] shaped as a cup: its bottom row and its two side columns.
fn cup(id: &str, at: (i32, i32), size: (u32, u32)) -> LiveCopy {
    let mut c = block(id, at, size, 1);
    c.pictures = vec![LiveSolid::new(size, true, move |x, y| y == size.1 - 1 || x == 0 || x == size.0 - 1)];
    c
}

/// The cave of [`terrain`] with loose dirt (kind 1, the default material)
/// in each box, no director.
fn loose_world(copies: Vec<LiveCopy>, loose: &[(u32, u32, u32, u32)]) -> World {
    loose_world_with(copies, loose, SimSettings::lx56())
}

/// [`loose_world`] with `settings`.
fn loose_world_with(copies: Vec<LiveCopy>, loose: &[(u32, u32, u32, u32)], settings: SimSettings) -> World {
    let inside =
        |x: u32, y: u32| loose.iter().any(|&(x0, y0, x1, y1)| (x0..x1).contains(&x) && (y0..y1).contains(&y));
    let mut cells = vec![Material::Background; 200 * 200];
    let mut kinds = vec![0u8; 200 * 200];
    for y in 0..200u32 {
        for x in 0..200u32 {
            let i = (y * 200 + x) as usize;
            if y >= 180 {
                cells[i] = Material::Rock;
            } else if inside(x, y) {
                cells[i] = Material::Dirt;
                kinds[i] = 1;
            }
        }
    }
    let terrain =
        Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap()).with_loose(kinds).unwrap();
    let mut s = setup(copies, None, None);
    s.terrain = terrain;
    s.settings = settings;
    s.loose = crate::LooseLevel::new(vec![olr_data::level::LooseMaterial::DEFAULT]).map(Arc::new);
    World::build(s, Box::new(OracleMode))
}

fn loose_count(w: &World) -> usize {
    let t = w.terrain();
    (0..t.height() as i64).map(|y| (0..t.width() as i64).filter(|&x| t.loose(x, y) != 0).count()).sum()
}

fn dirt_crushed(e: &TickEvents) -> u32 {
    e.iter()
        .map(|e| match e.kind {
            EventKind::DirtCrushed { cells, .. } => cells,
            _ => 0,
        })
        .sum()
}

#[test]
fn a_part_pushes_a_loose_row_as_one() {
    // A 20 × 10 block on the floor at (90, 170); loose grains x 110..115 on
    // the floor's row 179, against its right side.
    let mut w = loose_world(vec![block("pusher", (90, 170), (20, 10), 1)], &[(110, 179, 115, 180)]);
    move_to(&mut w, 0, X, 2, 1);
    ticks(&mut w, 4);
    assert_eq!(rect(&w, 0).0, 92);
    for x in 112..117 {
        assert_eq!(w.terrain().loose(x, 179), 1, "({x}, 179)");
    }
    assert_eq!(loose_count(&w), 5, "none crushed");
}

#[test]
fn a_grain_pushed_against_rock_or_the_edge_is_crushed() {
    // Down onto 10 full rows of loose grains lying on the rock at 180,
    // wider than it (their corners slide at the repose).
    let mut w = loose_world(vec![block("press", (90, 160), (20, 10), 1)], &[(70, 170, 130, 180)]);
    move_to(&mut w, 0, Y, 5, 1);
    let mut crushed = 0;
    for _ in 0..8 {
        crushed += dirt_crushed(&w.tick_new(&[]));
    }
    assert_eq!(rect(&w, 0).1, 165);
    assert_eq!(crushed, 100, "20 × 5 crushed, reported");
    assert_eq!(loose_count(&w), 500);
    // At the level's left edge a pushed grain is crushed, never moved out.
    let mut w = loose_world(vec![block("wall", (10, 170), (10, 10), 1)], &[(0, 179, 10, 180)]);
    move_to(&mut w, 0, X, -10, 1);
    ticks(&mut w, 12);
    assert_eq!(rect(&w, 0).0, 0);
    assert_eq!(loose_count(&w), 0);
}

#[test]
fn a_rising_cup_carries_its_grains() {
    // A 12 × 8 cup at (94, 150), its inside (10 × 7) full of grains.
    let mut w = loose_world(vec![cup("claw", (94, 150), (12, 8))], &[(95, 150, 105, 157)]);
    ticks(&mut w, 5);
    assert_eq!(loose_count(&w), 70, "they rest in it");
    move_to(&mut w, 0, Y, -30, 1);
    ticks(&mut w, 40);
    assert_eq!(rect(&w, 0).1, 120);
    assert_eq!(loose_count(&w), 70, "all kept");
    for y in 120..127 {
        for x in 95..105 {
            assert_eq!(w.terrain().loose(x, y), 1, "({x}, {y})");
        }
    }
    move_to(&mut w, 0, X, 20, 1);
    ticks(&mut w, 30);
    assert_eq!(rect(&w, 0).0, 114);
    assert_eq!(loose_count(&w), 70, "carried sideways too");
    for y in 120..127 {
        for x in 115..125 {
            assert_eq!(w.terrain().loose(x, y), 1, "({x}, {y})");
        }
    }
}

/// Replays a tick's edits on `t` as a colour map does: its events in order
/// (a crushed run, a grain a part moved), then its loose moves.
fn replay(t: &mut Terrain, events: &TickEvents, moves: &[(u32, u32)]) {
    let wd = t.width() as i32;
    for e in events.iter() {
        match e.kind {
            EventKind::TerrainEdit { op: TerrainOp::Crush, stamp: StampId::Row(n), x, y } => {
                t.crush_row(x, y, u32::from(n));
            }
            EventKind::GrainMoved { from, to } => {
                t.move_grain((from.1 * wd + from.0) as u32, (to.1 * wd + to.0) as u32);
            }
            EventKind::TerrainEdit { .. } => panic!("no other edit here: {:?}", e.kind),
            _ => {}
        }
    }
    for &(from, to) in moves {
        t.move_grain(from, to);
    }
}

#[test]
fn part_edits_replay_in_order() {
    // The cup carrying, a row pushed and grains crushed, all in one world.
    let mut w = loose_world(
        vec![cup("claw", (94, 150), (12, 8)), block("press", (30, 160), (20, 10), 1)],
        &[(95, 150, 105, 157), (30, 170, 50, 180), (50, 179, 56, 180)],
    );
    let mut t = w.terrain().clone();
    move_to(&mut w, 0, Y, -20, 1);
    move_to(&mut w, 1, Y, 5, 1);
    for tick in 0..60 {
        if tick == 25 {
            move_to(&mut w, 0, X, 15, 1);
            move_to(&mut w, 1, X, 4, 1);
        }
        let e = w.tick_new(&[]);
        replay(&mut t, &e, w.loose_moves());
        assert_eq!(&t, w.terrain(), "tick {tick}");
    }
    assert!(loose_count(&w) > 70, "something survived");
}

#[test]
fn a_restore_mid_carry_re_steps_the_same() {
    let mut w = loose_world(vec![cup("claw", (94, 150), (12, 8))], &[(95, 150, 105, 157)]);
    w.enable_rollback();
    move_to(&mut w, 0, Y, -30, 1);
    ticks(&mut w, 10);
    let snap = w.save();
    let mut sums = Vec::new();
    for t in 0..30 {
        if t == 15 {
            move_to(&mut w, 0, X, 10, 1);
        }
        ticks(&mut w, 1);
        sums.push(w.checksum());
    }
    w.restore(&snap).unwrap();
    for (t, &s) in sums.iter().enumerate() {
        if t == 15 {
            move_to(&mut w, 0, X, 10, 1);
        }
        ticks(&mut w, 1);
        assert_eq!(w.checksum(), s, "tick {t} after the restore");
    }
}

// Conveyors (the production line design).

/// The belt's conveyor value.
const CONVEYOR: u16 = 3;

/// A still rock belt of `size` at `at` whose layer has a conveyor value
/// (-100..=100 px/s, starting at 0).
fn belt(id: &str, at: (i32, i32), size: (u32, u32)) -> LiveCopy {
    let b = block(id, at, size, 1);
    let mut props = b.props.clone();
    props.push(LiveProp { name: None, target: LiveTarget::Conveyor(0), min: -100, max: 100, default: 0 });
    LiveCopy::new(LiveCopySetup {
        id: b.id.clone(),
        at: b.at,
        size: b.size,
        props,
        states: b.states.clone(),
        pictures: b.pictures.clone(),
        start_state: b.start_state,
        start_values: b.start_values.clone(),
        health: b.health,
        scripts: b.scripts.clone(),
        sounds: b.sounds.clone(),
    })
}

#[test]
fn conveyed_px_spreads_a_speed_over_the_ticks() {
    assert_eq!((0..100).map(|t| conveyed_px(t, 50)).sum::<i64>(), 50);
    assert_eq!((0..100).map(|t| conveyed_px(t, -37)).sum::<i64>(), -37);
    assert!((0..1000).all(|t| conveyed_px(t, 100) == 1 && conveyed_px(t, 0) == 0));
    assert!((0..1000).all(|t| (-1..=1).contains(&conveyed_px(t, 73))));
}

#[test]
fn a_running_belt_carries_a_standing_worm_and_grains() {
    let mut w = loose_world(vec![belt("belt", (50, 120), (100, 4))], &[(90, 119, 91, 120)]);
    w.spawn_worm(70, 100);
    ticks(&mut w, 150);
    let (x0, y0) = at(&w);
    assert_eq!(y0, 115, "standing on the belt");
    w.set_live_value(0, CONVEYOR, 50).unwrap();
    ticks(&mut w, 100);
    let (x, y) = at(&w);
    assert!((x - x0 - 50).abs() <= 1 && y == y0, "the worm rode from {x0} to {x}, row {y}");
    assert_eq!(w.terrain().loose(140, 119), 1, "the grain rode 50 px");
    assert_eq!(loose_count(&w), 1);
    assert_eq!(rect(&w, 0), (50, 120, 100, 4), "the belt itself never moves");
    w.set_live_value(0, CONVEYOR, 0).unwrap();
    ticks(&mut w, 50);
    assert_eq!(at(&w).0, x, "a stopped belt carries nothing");
    assert_eq!(w.terrain().loose(140, 119), 1);
}

#[test]
fn a_conveyed_worm_stops_at_a_wall() {
    let wall = block("wall", (130, 100), (4, 20), 1);
    let mut w = loose_world(vec![belt("belt", (50, 120), (100, 4)), wall], &[(100, 119, 101, 120)]);
    w.spawn_worm(70, 100);
    ticks(&mut w, 150);
    w.set_live_value(0, CONVEYOR, 100).unwrap();
    ticks(&mut w, 150);
    assert_eq!(w.terrain().loose(129, 119), 1, "the grain stopped at the wall");
    let (x, y) = at(&w);
    assert!((122..=126).contains(&x) && y == 115, "the worm stopped short of it at ({x}, {y})");
    assert_eq!(w.solid().flags(i64::from(x), i64::from(y)) & PX_ROCK, 0);
}

#[test]
fn a_bonus_rides_the_belt() {
    // Bonuses on, none spawned during the test.
    let settings =
        SimSettings { bonuses: true, bonus_frequency: 1000.0, bonus_life: 1000.0, ..SimSettings::lx56() };
    let mut w = loose_world_with(vec![belt("belt", (50, 120), (100, 4))], &[], settings);
    w.place_bonus(120, 100, crate::BonusItem::Health).unwrap();
    ticks(&mut w, 150);
    let b = *w.bonuses().iter().find(|b| b.used).expect("the bonus fell onto the belt");
    w.set_live_value(0, CONVEYOR, -50).unwrap();
    ticks(&mut w, 100);
    let a = *w.bonuses().iter().find(|b| b.used).unwrap();
    assert!(
        (a.pos.x - (b.pos.x - 50.0)).abs() <= 1.0 && a.pos.y == b.pos.y,
        "from {:?} to {:?}",
        b.pos,
        a.pos
    );
}

#[test]
fn a_level_wide_conveyor_stays_bounded() {
    let mut w = loose_world(vec![belt("belt", (0, 120), (200, 4))], &[(0, 110, 200, 120)]);
    w.set_live_value(0, CONVEYOR, 100).unwrap();
    ticks(&mut w, 100);
    assert_eq!(loose_count(&w), 2000, "every grain kept");
    let t = w.terrain();
    for y in 0..200 {
        for x in 0..200 {
            if t.loose(x, y) != 0 {
                assert_eq!(w.solid().flags(x, y) & PX_ROCK, 0, "a grain in rock at ({x}, {y})");
            }
        }
    }
}
