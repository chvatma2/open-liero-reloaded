//! Whole bots in a world: their work budget, determinism, no allocation
//! while thinking, and robustness (notes §4.3, §4.8).

use crate::common;

use std::sync::Arc;

use common::*;
use rl_bots::{AreaCache, Bot, BotConfig, Difficulty, SearchStatus};
use rl_data::level::Material;
use rl_math::Vec2;
use rl_sim::{Keys, TickEvents, World};
use rl_terrain::Terrain;

/// 400 × 200: a rock floor at 170, a rock wall hanging to 140 at x 150..=165,
/// a dirt mound at x 240..=280 and a 10 px rock step from x 300.
fn course() -> Terrain {
    level(400, 200, |x, y| {
        let border = !inside(x, y, (5, 5, 394, 169));
        let rock = border || (150..=165).contains(&x) && y <= 140 || x >= 300 && y >= 160;
        if rock {
            Material::Rock
        } else if (240..=280).contains(&x) && y >= 130 {
            Material::Dirt
        } else {
            Material::Background
        }
    })
}

const GOALS: [(f32, f32); 2] = [(360.0, 150.0), (330.0, 150.0)];

/// Bots for worms 0 and 1 of `w` (sharing one area cache), each walking to
/// its goal.
fn bots(w: &World, cfg: BotConfig, seed: u32) -> Vec<Bot> {
    let cache = AreaCache::new(w.terrain());
    (0..2)
        .map(|i| {
            let mut b = Bot::with_cache(i, cfg, seed, w, Arc::clone(&cache));
            b.set_goal(Some(Vec2::new(GOALS[i].0, GOALS[i].1)));
            b
        })
        .collect()
}

/// Runs `ticks` ticks; gives every tick's keys and the world's checksums.
fn run(
    w: &mut World,
    bots: &mut [Bot],
    ticks: u32,
    mut each: impl FnMut(&[Bot]),
) -> (Vec<Vec<Keys>>, Vec<u64>) {
    let mut out = TickEvents::new();
    let (mut keys, mut sums) = (Vec::new(), Vec::new());
    for _ in 0..ticks {
        let k: Vec<Keys> = bots.iter_mut().map(|b| b.think(w, &out)).collect();
        each(bots);
        w.tick(&k, &mut out);
        keys.push(k);
        sums.push(w.checksum());
    }
    (keys, sums)
}

#[test]
fn think_stays_within_budget() {
    let mut w = world(course(), &[(40, 160), (60, 160)], 11);
    let cfg = BotConfig { budget: 2600, ..BotConfig::default() };
    let mut bots = bots(&w, cfg, 11);
    assert_eq!(bots[0].budget(), 2600, "the level needs at most {}", rl_bots::min_budget(w.terrain()));
    let (mut most, mut pending, mut full) = (0, 0, 0);
    run(&mut w, &mut bots, 1500, |bots| {
        for b in bots {
            assert!(b.units_used() <= b.budget(), "{} units of {}", b.units_used(), b.budget());
            most = most.max(b.units_used());
            pending += (b.search_status() == SearchStatus::Pending) as u32;
            full += (b.units_used() * 10 >= b.budget() * 9) as u32;
        }
    });
    assert!(pending >= 2, "a search went on across ticks ({pending} pending thinks)");
    assert!(full >= 1 && most <= 2600, "the budget was used ({most} at most)");
    // The placeholder brain walks: under the wall and through the dirt
    // (carving), up the step, to its goal.
    let x = w.worms()[0].pos.x;
    assert!(x > 300.0, "worm 0 got to x {x}");
}

#[test]
fn bots_are_deterministic() {
    let cfg = BotConfig { difficulty: Difficulty::Easy, ..BotConfig::default() };
    let play = |seed: u32| {
        let mut w = world(course(), &[(40, 160), (60, 160)], 5);
        let mut b = bots(&w, cfg, seed);
        run(&mut w, &mut b, 800, |_| {})
    };
    let (keys, sums) = play(5);
    assert_eq!(play(5), (keys.clone(), sums.clone()), "same seed, same keys and checksums");
    let (other, _) = play(6);
    assert_ne!(other, keys, "another seed, other aim offsets");
}

#[test]
fn think_does_not_allocate() {
    let mut w = world(course(), &[(40, 160), (60, 160)], 2);
    let mut bots = bots(&w, BotConfig::default(), 2);
    let mut out = TickEvents::new();
    let mut keys = vec![Keys(0); 2];
    let info = allocation_counter::measure(|| {
        for _ in 0..600 {
            for (k, b) in keys.iter_mut().zip(bots.iter_mut()) {
                *k = b.think(&w, &out);
            }
            w.tick(&keys, &mut out);
        }
    });
    assert_eq!(info.count_total, 0, "thinking allocated");
}

#[test]
fn think_never_panics_on_odd_worlds() {
    let goals =
        [None, Some(Vec2::new(1e9, -1e9)), Some(Vec2::new(f32::NAN, 3.0)), Some(Vec2::new(-20.0, 5.0))];
    let levels =
        [rocks(16, 16, |_, _| true), rocks(16, 16, |_, y| y > 12), rocks(3000, 20, |_, y| y > 15), course()];
    for t in levels {
        for goal in goals {
            let mut w = world(t.clone(), &[(8, 8), (-50, -50)], 9);
            for worm in 0..3 {
                let mut b = Bot::new(worm, BotConfig::default(), 9, &w);
                b.set_goal(goal);
                let mut out = TickEvents::new();
                for _ in 0..80 {
                    let k = b.think(&w, &out);
                    assert!(b.units_used() <= b.budget());
                    w.tick(&[k, k], &mut out);
                }
            }
        }
    }
}

/// Task 4 review: a search toward a target that keeps moving must still
/// finish (`AI_CreatePath` restarts a pending search only after 5 s, or
/// when no worm line joins the old and new start or target).
#[test]
fn a_search_toward_a_moving_target_finishes() {
    let mut w = world(course(), &[(40, 160), (360, 150)], 8);
    let cfg = BotConfig { budget: 1200, ..BotConfig::default() };
    let mut bot = Bot::new(0, cfg, 8, &w);
    let mut out = TickEvents::new();
    let mut found = None;
    for t in 0..800 {
        let k = bot.think(&w, &out);
        // Worm 1 walks to and fro under the bot's nose.
        let side = if (t / 60) % 2 == 0 { Keys::LEFT } else { Keys::RIGHT };
        w.tick(&[k, Keys(side)], &mut out);
        if !bot.path().is_empty() {
            found = Some(t);
            break;
        }
    }
    assert!(found.is_some(), "a path was found while the target moved");
}

/// Task 4 review: a cache from another level of the same size is not
/// shared: the cache knows its level's rock.
#[test]
fn with_cache_keeps_only_a_cache_of_the_same_level() {
    let w = world(course(), &[(40, 160), (60, 160)], 1);
    let other = world(arena(), &[(40, 140), (60, 140)], 1);
    let cache = AreaCache::new(w.terrain());
    let same = Bot::with_cache(0, BotConfig::default(), 1, &w, Arc::clone(&cache));
    assert!(Arc::ptr_eq(same.cache(), &cache));
    let moved = Bot::with_cache(0, BotConfig::default(), 1, &other, Arc::clone(&cache));
    assert!(!Arc::ptr_eq(moved.cache(), &cache), "same size, other rock: a cache of its own");
    assert!(cache.fits(w.terrain()) && !cache.fits(other.terrain()));
}
