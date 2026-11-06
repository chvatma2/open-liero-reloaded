//! Unit tests of loose dirt (spec `2026-11-05-loose-dirt-design.md`): the
//! fall, riding, the angle of repose, what holds grains up, sleeping and
//! waking, the cap, the edges, and the step through a restore and a state
//! transfer.
use super::*;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{LooseMaterial, Material, MaterialGrid};
use olr_terrain::{Stamp, Terrain};

use crate::input::Keys;
use crate::live::{
    LiveCopy, LiveCopySetup, LivePart, LivePartKind, LiveProp, LiveSolid, LiveState, LiveTarget,
};
use crate::{LiveLevel, MatchSetup, OracleMode, SimSettings, World};

/// A map of `w` × `h`: `cell(x, y)` gives each cell's material and loose kind.
fn map(w: u32, h: u32, cell: impl Fn(u32, u32) -> (Material, u8)) -> Terrain {
    let (mut cells, mut kinds) = (Vec::new(), Vec::new());
    for y in 0..h {
        for x in 0..w {
            let (m, k) = cell(x, y);
            cells.push(m);
            kinds.push(k);
        }
    }
    Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap()).with_loose(kinds).unwrap()
}

fn hole(n: u32) -> Stamp {
    let mut img = RgbaImage::new(n, n);
    for y in 0..n {
        for x in 0..n {
            img.set(x, y, Rgba::MAGENTA);
        }
    }
    Stamp::from_image(&img)
}

fn setup_with(terrain: Terrain, materials: Vec<LooseMaterial>, live: Option<Arc<LiveLevel>>) -> MatchSetup {
    MatchSetup {
        holes: vec![hole(5)],
        loose: Some(Arc::new(LooseLevel::new(materials).unwrap())),
        live,
        ..MatchSetup::new(SimSettings::lx56(), crate::mod_rt::tests::test_mod(), terrain)
    }
}

fn world_with(terrain: Terrain, repose: u8) -> World {
    let m = LooseMaterial { repose, ..LooseMaterial::DEFAULT };
    World::build(setup_with(terrain, vec![m], None), Box::new(OracleMode))
}

fn world(terrain: Terrain) -> World {
    world_with(terrain, 45)
}

fn ticks(w: &mut World, n: usize) {
    for _ in 0..n {
        w.tick_new(&[Keys(0)]);
    }
}

/// Every loose cell, row-major.
fn grains(w: &World) -> Vec<(u32, u32)> {
    let t = w.terrain();
    let mut out = Vec::new();
    for y in 0..t.height() {
        for x in 0..t.width() {
            if t.loose(i64::from(x), i64::from(y)) != 0 {
                out.push((x, y));
            }
        }
    }
    out
}

const AIR: (Material, u8) = (Material::Background, 0);
const ROCK: (Material, u8) = (Material::Rock, 0);
const SAND: (Material, u8) = (Material::Dirt, 1);

/// Rock from row `floor` down, `sand(x, y)` loose dirt, air elsewhere.
fn cave(w: u32, h: u32, floor: u32, sand: impl Fn(u32, u32) -> bool) -> Terrain {
    map(w, h, |x, y| {
        if y >= floor {
            ROCK
        } else if sand(x, y) {
            SAND
        } else {
            AIR
        }
    })
}

#[test]
fn a_grain_falls_from_rest_and_reaches_its_top_speed() {
    let mut w = world(cave(100, 140, 120, |x, y| (x, y) == (50, 10)));
    ticks(&mut w, 1);
    assert_eq!(grains(&w), [(50, 10)], "0.02 px fallen after a tick");
    ticks(&mut w, 99);
    // Gravity 300 px/s² is 8 sub-pixels a tick a tick, the top speed
    // 80 px/s 205: Σ min(8t, 205) for t in 1..=100 is 17975 sub-pixels.
    let (_, y) = grains(&w)[0];
    assert!((79..=81).contains(&y), "fell {} px, 17975/256 = 70.2 expected", y - 10);
    ticks(&mut w, 400);
    assert_eq!(grains(&w), [(50, 119)], "landed on the floor");
    ticks(&mut w, 10);
    assert!(w.loose_moves().is_empty(), "and still");
    assert_eq!(w.loose_state_len(), 0, "nothing moving");
}

#[test]
fn a_column_falls_as_one() {
    let mut w = world(cave(100, 140, 120, |x, y| x == 50 && (10..20).contains(&y)));
    // It lands at about tick 138 (100 px: 2600 sub-pixels by tick 25, then
    // 205 a tick), and then its top slides off.
    for t in 0..130 {
        ticks(&mut w, 1);
        let ys: Vec<u32> = grains(&w).iter().map(|&(_, y)| y).collect();
        assert_eq!(ys.len(), 10);
        assert!(ys.windows(2).all(|p| p[1] == p[0] + 1), "tick {t}: a gap in {ys:?}");
    }
}

#[test]
fn a_dug_hole_fills() {
    // A bin: rock walls at x 39 and 60, sand between them up to row 100.
    let bin = |x: u32, y: u32| {
        if y >= 120 || x == 39 || x == 60 {
            ROCK
        } else if (40..60).contains(&x) && y >= 100 {
            SAND
        } else {
            AIR
        }
    };
    let mut w = world(map(100, 140, bin));
    ticks(&mut w, 10);
    w.terrain.carve_hole(&w.holes[0].clone(), 50, 115);
    assert_eq!(w.terrain().material(50, 115), Some(Material::Background));
    ticks(&mut w, 300);
    for y in 113..=117 {
        for x in 48..=52 {
            assert_eq!(w.terrain().material(x, y), Some(Material::Dirt), "({x}, {y}) filled");
        }
    }
}

/// The slope of a settled pile's flanks, in degrees: each flank's columns
/// between 20 % and 80 % of the peak, fitted by least squares.
fn flank_angle(w: &World) -> f64 {
    let t = w.terrain();
    let heights: Vec<f64> = (0..t.width())
        .map(|x| (0..t.height()).filter(|&y| t.loose(i64::from(x), i64::from(y)) != 0).count() as f64)
        .collect();
    let peak = heights.iter().cloned().fold(0.0, f64::max);
    let top = heights.iter().position(|&h| h == peak).unwrap();
    let fit = |xs: Vec<usize>| {
        let n = xs.len() as f64;
        let (mx, my) =
            (xs.iter().map(|&x| x as f64).sum::<f64>() / n, xs.iter().map(|&x| heights[x]).sum::<f64>() / n);
        let cov: f64 = xs.iter().map(|&x| (x as f64 - mx) * (heights[x] - my)).sum();
        let var: f64 = xs.iter().map(|&x| (x as f64 - mx).powi(2)).sum();
        (cov / var).abs()
    };
    let band = |h: f64| h >= 0.2 * peak && h <= 0.8 * peak;
    let left: Vec<usize> = (0..top).filter(|&x| band(heights[x])).collect();
    let right: Vec<usize> = (top..heights.len()).filter(|&x| band(heights[x])).collect();
    assert!(left.len() >= 2 && right.len() >= 2, "flanks to fit: {heights:?}");
    let slope = (fit(left) + fit(right)) / 2.0;
    libm::atan(slope).to_degrees()
}

#[test]
fn piles_settle_at_their_repose() {
    for repose in [25u8, 45, 65] {
        let mut w =
            world_with(cave(240, 140, 120, |x, y| (110..130).contains(&x) && (60..80).contains(&y)), repose);
        ticks(&mut w, 2500);
        let angle = flank_angle(&w);
        assert!((angle - f64::from(repose)).abs() <= 8.0, "repose {repose}: the flanks lie at {angle:.1}°");
    }
}

#[test]
fn a_worm_holds_grains_up() {
    let mut w = world(cave(200, 140, 120, |x, y| (94..107).contains(&x) && (40..60).contains(&y)));
    w.spawn_worm(100, 110);
    // The worm lands on the floor before the grains reach it.
    ticks(&mut w, 60);
    let p = w.worms()[0].pos;
    let (wx, wy) = (p.x as i64, p.y as i64);
    for _ in 0..300 {
        ticks(&mut w, 1);
        for &(x, y) in &grains(&w) {
            let (dx, dy) = (i64::from(x) - wx, i64::from(y) - wy);
            assert!(
                !(dx.abs() <= 3 && (-4..=5).contains(&dy)),
                "a grain at ({x}, {y}) inside the worm at ({wx}, {wy})"
            );
        }
    }
    assert!(
        grains(&w).iter().any(|&(x, y)| x.abs_diff(wx as u32) <= 3 && y < wy as u32 - 4),
        "some rest on it"
    );
}

/// A rock block of `size` at `at`, shown while its value 0 is 1.
fn block(at: (i32, i32), size: (u32, u32)) -> LiveCopy {
    let part = LivePart {
        kind: LivePartKind::Rock,
        layer: Some(0),
        at: (0, 0),
        picture: 0,
        glow_strength: 100,
        clip: false,
    };
    LiveCopy::new(LiveCopySetup {
        id: "shelf".into(),
        at,
        size,
        props: vec![LiveProp { name: None, target: LiveTarget::LayerVisible(0), min: 0, max: 1, default: 1 }],
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

#[test]
fn a_live_rock_part_holds_grains_up_until_it_goes() {
    let level = Arc::new(LiveLevel::new(vec![block((80, 100), (40, 4))], Vec::new(), None, 0));
    let terrain = cave(200, 140, 120, |x, y| (90..110).contains(&x) && (90..100).contains(&y));
    let mut w =
        World::build(setup_with(terrain, vec![LooseMaterial::DEFAULT], Some(level)), Box::new(OracleMode));
    ticks(&mut w, 100);
    assert!(grains(&w).iter().all(|&(_, y)| y < 100), "held on the shelf");
    w.set_live_value(0, 0, 0).unwrap();
    ticks(&mut w, 150);
    assert!(grains(&w).iter().all(|&(_, y)| y >= 104), "fell once the shelf went");
}

#[test]
fn a_frozen_world_moves_none() {
    let mut w = world(cave(100, 140, 120, |x, y| x == 50 && y < 10));
    w.referee.over = Some(crate::MatchResult { winner: 0, reason: crate::EndReason::LastStanding });
    w.referee.over_frame = 0;
    w.server_frame = 10_000;
    let before = grains(&w);
    ticks(&mut w, 100);
    assert!(w.loose_moves().is_empty());
    assert_eq!(grains(&w), before);
}

#[test]
fn the_cap_holds() {
    let mut w = world(cave(400, 300, 290, |_, y| y < 75));
    let n = grains(&w).len();
    assert_eq!(n, 30_000);
    let mut capped = false;
    for _ in 0..1500 {
        ticks(&mut w, 1);
        let m = w.loose_moves().len() as u32;
        assert!(m <= MAX_LOOSE_MOVES);
        capped |= m == MAX_LOOSE_MOVES;
    }
    assert!(capped, "the collapse reached the cap");
    assert_eq!(grains(&w).len(), n, "none lost");
    assert!(grains(&w).iter().all(|&(_, y)| y >= 290 - 75), "all landed");
}

#[test]
fn edges_never_index_out() {
    let edge = |x: u32, y: u32| x == 0 || y == 0 || x == 9 || y == 9;
    let mut w = world(map(10, 10, |x, y| if edge(x, y) && (x + y) % 2 == 0 { SAND } else { AIR }));
    let n = grains(&w).len();
    ticks(&mut w, 300);
    assert_eq!(grains(&w).len(), n, "none lost at the edges");
    assert!(grains(&w).iter().all(|&(x, y)| x < 10 && y < 10));
}

#[test]
fn settled_grains_wake_on_a_nearby_edit() {
    let mut w = world(cave(200, 140, 120, |x, y| (90..110).contains(&x) && (100..120).contains(&y)));
    let mut still = 0;
    for _ in 0..3000 {
        ticks(&mut w, 1);
        still = if w.loose_moves().is_empty() { still + 1 } else { 0 };
        if still == 50 {
            break;
        }
    }
    assert_eq!(still, 50, "the pile settled");
    let base = grains(&w).iter().filter(|&&(_, y)| y == 119).map(|&(x, _)| x).min().unwrap();
    w.terrain.carve_hole(&w.holes[0].clone(), base as i32 + 4, 117);
    let moved = (0..20).any(|_| {
        ticks(&mut w, 1);
        !w.loose_moves().is_empty()
    });
    assert!(moved, "the dug pile moves again");
}

fn collapse() -> World {
    world(cave(160, 140, 120, |x, y| (60..100).contains(&x) && (20..60).contains(&y)))
}

#[test]
fn two_worlds_agree() {
    let (mut a, mut b) = (collapse(), collapse());
    for t in 0..500 {
        ticks(&mut a, 1);
        ticks(&mut b, 1);
        assert_eq!(a.checksum(), b.checksum(), "tick {t}");
    }
}

#[test]
fn a_restore_mid_collapse_re_steps_the_same() {
    for rollback in [true, false] {
        let mut w = collapse();
        if rollback {
            w.enable_rollback();
        }
        ticks(&mut w, 20);
        let s = w.save();
        let mut sums = Vec::new();
        for _ in 0..40 {
            ticks(&mut w, 1);
            sums.push((w.checksum(), w.sync_checksum()));
        }
        w.restore(&s).unwrap();
        for (i, sum) in sums.iter().enumerate() {
            ticks(&mut w, 1);
            assert_eq!((w.checksum(), w.sync_checksum()), *sum, "rollback {rollback}, tick {}", 21 + i);
        }
    }
}

#[test]
fn a_state_transfer_mid_collapse_plays_on_alike() {
    let mut a = collapse();
    ticks(&mut a, 20);
    let state = crate::WorldState::decode(&a.state().encode()).unwrap();
    let mut b = collapse();
    b.load_state(state).unwrap();
    assert_eq!(a.checksum(), b.checksum(), "loaded");
    for t in 0..80 {
        ticks(&mut a, 1);
        ticks(&mut b, 1);
        assert_eq!(a.checksum(), b.checksum(), "tick {}", 21 + t);
    }
}

#[test]
fn loose_levels_refuse_what_does_not_fit() {
    let bad = |m: LooseMaterial| LooseLevel::new(vec![m]).is_none();
    let d = LooseMaterial::DEFAULT;
    assert!(bad(LooseMaterial { repose: 4, ..d }) && bad(LooseMaterial { repose: 86, ..d }));
    assert!(bad(LooseMaterial { gravity: 0, ..d }) && bad(LooseMaterial { gravity: 2001, ..d }));
    assert!(bad(LooseMaterial { top_speed: 0, ..d }) && bad(LooseMaterial { top_speed: 501, ..d }));
    assert!(bad(LooseMaterial { slide_speed: 101, ..d }));
    assert!(LooseLevel::new(vec![d; 17]).is_none() && LooseLevel::new(vec![d; 16]).is_some());
    // A kind past the materials, or loose dirt without materials, is refused at setup.
    let t = cave(20, 20, 18, |x, y| (x, y) == (5, 5));
    let two =
        Terrain::from_grid(&MaterialGrid::new(20, 20, Material::Dirt)).with_loose(vec![2; 400]).unwrap();
    assert!(World::new(setup_with(two, vec![d], None), Box::new(OracleMode)).is_err());
    let none = MatchSetup { loose: None, ..setup_with(t, vec![d], None) };
    assert!(World::new(none, Box::new(OracleMode)).is_err());
}

/// Review Focus 2: a grain dug while it falls is gone, and dirt placed
/// where it was is ordinary dirt.
#[test]
fn a_grain_dug_while_falling_is_gone() {
    let mut w = world(cave(100, 140, 120, |x, y| (x, y) == (50, 10)));
    ticks(&mut w, 60);
    let (x, y) = grains(&w)[0];
    assert!(y > 10 && w.loose_state_len() == 1, "falling");
    let hole = w.holes[0].clone();
    w.terrain.carve_hole(&hole, x as i32, y as i32);
    w.terrain.place_dirt(&hole, x as i32, y as i32);
    ticks(&mut w, 1);
    assert!(grains(&w).is_empty(), "no loose dirt left");
    assert_eq!(w.loose_state_len(), 0, "and nothing moving");
    assert_eq!(
        w.terrain().material(i64::from(x), i64::from(y)),
        Some(Material::Dirt),
        "the placed dirt stays"
    );
}

/// `any_in` (a sleeping chunk row's one look) reads exactly its range of
/// both sets.
#[test]
fn any_in_reads_exactly_its_range() {
    for set in [0u32, 1, 62, 63, 64, 65, 127, 128, 150, 191] {
        let mut b = vec![0u64; 3];
        set_bit(&mut b, set);
        for from in 0..192 {
            for to in from..=192 {
                assert_eq!(any_in(&b, &b, from, to), (from..to).contains(&set), "bit {set}, {from}..{to}");
                assert!(!any_in(&b, &[0; 3], from, to), "and nothing");
            }
        }
    }
    assert!(!any_in(&[0u64; 3], &[u64::MAX; 3], 0, 192));
    assert!(any_in(&[0b110], &[0b011], 0, 64), "both at bit 1");
    assert!(!any_in(&[0b100], &[0b011], 0, 64));
}

/// Settled grains cost nothing: a worm far from them keeps its chunks
/// awake (their bits are state), but a tick scans no chunk without loose
/// dirt.
#[test]
fn chunks_without_loose_dirt_are_not_scanned() {
    let mut w = world(cave(200, 140, 120, |x, y| (10..20).contains(&x) && (110..120).contains(&y)));
    w.spawn_worm(150, 110);
    let mut still = 0;
    for _ in 0..3000 {
        ticks(&mut w, 1);
        still = if w.loose_moves().is_empty() { still + 1 } else { 0 };
        if still == 50 {
            break;
        }
    }
    assert_eq!(still, 50, "the pile settled");
    ticks(&mut w, 1);
    let l = w.loose.as_ref().unwrap();
    let chunk = (110 >> CHUNK_SHIFT) * l.cols + (150 >> CHUNK_SHIFT);
    assert!(bit(&l.scratch.now, chunk), "the worm's chunk was awake");
    assert_eq!(l.scanned, 0, "chunks scanned");
}
