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
use crate::events::{CosmeticKind, TickEvents};
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
