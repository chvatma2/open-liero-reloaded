//! Issue #91 on hand-made worlds with a live part: the probes see it, the
//! area cache does not, no chosen stop is inside its reach, a crushed bot
//! gets out, the budget holds, and a level without live parts does no new
//! work.

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::level::{Material, MaterialGrid};
use olr_math::Vec2;
use olr_sim::{
    EventKind, Keys, LIVE_SUBPIXELS, LiveCopy, LiveCopySetup, LiveLevel, LivePart, LivePartKind, LiveProp,
    LiveSolid, LiveState, LiveTarget, Loadout, MatchSetup, OracleMode, SimSettings, Slot, TickEvents, World,
    WormSetup,
};
use olr_terrain::{PX_ROCK, Terrain};

use super::{ESCAPE_MS, HALF_WORM};
use crate::bot::{Bot, Ctx};
use crate::budget::Budget;
use crate::moving::tests::lob_mod;
use crate::nav::{AreaCache, Grower, fast_trace_line, worm_line_clear};
use crate::probe::trace_line;
use crate::{BotConfig, Personality};

const PX: i64 = LIVE_SUBPIXELS;
/// The block's values: `X`, `Y`, its layer shown.
const X: u16 = 0;
const Y: u16 = 1;
const SHOWN: u16 = 2;

/// A rock block of `size` at `at`, moved by `X` within `xs` and `Y` within
/// `ys` (pixels), its layer shown while `SHOWN` (starting at `shown`) is 1.
fn block(at: (i32, i32), size: (u32, u32), xs: (i64, i64), ys: (i64, i64), shown: i64) -> LiveCopy {
    let prop = |target, (min, max): (i64, i64), default| LiveProp { name: None, target, min, max, default };
    let part =
        LivePart { kind: LivePartKind::Rock, layer: Some(0), at: (0, 0), picture: 0, glow_strength: 100 };
    LiveCopy::new(LiveCopySetup {
        id: "block".into(),
        at,
        size,
        props: vec![
            prop(LiveTarget::X, xs, 0),
            prop(LiveTarget::Y, ys, 0),
            prop(LiveTarget::LayerVisible(0), (0, 1), shown),
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

/// A 300 × 200 world: air over a rock floor from y = 150, worms at
/// `spawns`, and `live`'s copies.
fn world(spawns: &[(i32, i32)], live: Option<LiveCopy>) -> World {
    let cells =
        (0..300 * 200).map(|i| if i / 300 >= 150 { Material::Rock } else { Material::Background }).collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(300, 200, cells).unwrap());
    let s = SimSettings::lx56();
    let worms = spawns
        .iter()
        .map(|&p| WormSetup { team: 0, loadout: Loadout::new(vec![Slot::ready(0); 5], &s), spawn: Some(p) })
        .collect();
    let mut setup = MatchSetup { worms, ..MatchSetup::new(s, lob_mod(), terrain) };
    setup.live = live.map(|c| Arc::new(LiveLevel::new(vec![c], Vec::new(), None, 0)));
    World::new(setup, Box::new(OracleMode)).unwrap()
}

fn ctx<'a>(w: &'a World, t: &'a olr_sim::Solid<'a>) -> Ctx<'a> {
    Ctx { w, me: &w.worms()[0], now: w.time(), s: w.settings(), t }
}

/// The block's reach grown by a worm's half size.
fn grown_reach(w: &World) -> (i32, i32, i32, i32) {
    let r = w.live().unwrap().reach_rects()[0].rect;
    (r.0 - HALF_WORM, r.1 - HALF_WORM, r.2 + HALF_WORM, r.3 + HALF_WORM)
}

fn inside(r: (i32, i32, i32, i32), p: Vec2) -> bool {
    p.x >= r.0 as f32 && p.x < r.2 as f32 && p.y >= r.1 as f32 && p.y < r.3 as f32
}

/// Ticks `w` `n` times with `bots` thinking, each tick's events to the
/// next; `each` sees every tick's events.
fn play(w: &mut World, bots: &mut [Bot], n: u32, mut each: impl FnMut(&World, &[Bot], &TickEvents)) {
    let mut ev = TickEvents::new();
    for _ in 0..n {
        let mut keys: Vec<Keys> = vec![Keys(0); w.worms().len()];
        for bot in bots.iter_mut() {
            keys[bot.worm()] = bot.think(w, &ev);
        }
        w.tick(&keys, &mut ev);
        each(w, bots, &ev);
    }
}

#[test]
fn probes_see_a_live_part_where_it_is() {
    // A wall 10 px wide at x = 140, from y = 60 to the floor.
    let mut w = world(&[(50, 140), (250, 140)], Some(block((140, 60), (10, 90), (-100, 100), (0, 0), 1)));
    let (a, z) = (Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0));
    let b = &mut Budget::new(100_000);
    {
        let solid = w.solid();
        let c = ctx(&w, &solid);
        // Through the query: rock; on the ground alone: air.
        assert!(trace_line(c.t, b, a, z, 5).is_some_and(|(_, ty)| ty & PX_ROCK != 0));
        assert!(trace_line(w.terrain(), b, a, z, 5).is_some_and(|(_, ty)| ty & PX_ROCK == 0));
        assert_eq!(worm_line_clear(c.t, b, a, z), Some(false));
        assert_eq!(worm_line_clear(w.terrain(), b, a, z), Some(true));
        // A rope thrown at the wall hooks on it.
        let (hit, _) = fast_trace_line(c.t, z, a, PX_ROCK, 1000);
        assert_eq!(hit.map(|(x, _)| x), Some(140));
    }
    // Moved 100 px right, it is there and no longer here.
    w.set_live_value(0, X, 100 * PX).unwrap();
    let solid = w.solid();
    let c = ctx(&w, &solid);
    assert_eq!(worm_line_clear(c.t, b, a, z), Some(true));
    assert_eq!(worm_line_clear(c.t, b, Vec2::new(200.0, 100.0), Vec2::new(280.0, 100.0)), Some(false));
}

#[test]
fn the_area_cache_reads_the_ground_only() {
    let w = world(&[(50, 140), (250, 140)], Some(block((140, 60), (10, 90), (-100, 100), (0, 0), 1)));
    // An area grown beside the wall runs through it: the ground's.
    let r = Grower::new((120, 100), 300, 200).step(w.terrain(), &mut Budget::new(u32::MAX)).unwrap();
    assert!(r.contains(145, 100), "{r:?}");
    // A bot's cache is the ground's: the same key as the level without
    // the part, and a search over the query grows the same areas.
    let plain = world(&[(50, 140), (250, 140)], None);
    let cache = AreaCache::new(w.terrain());
    assert!(cache.fits(plain.terrain()));
    let mut bots = vec![Bot::with_cache(0, BotConfig::default(), 1, &w, cache.clone())];
    let mut w = w;
    play(&mut w, &mut bots, 300, |_, _, _| {});
    assert!(!cache.is_empty());
    assert!((0..cache.len()).filter_map(|i| cache.get(i)).any(|a| a.contains(145, 100)));
}

#[test]
fn a_bot_never_picks_a_spot_inside_a_parts_reach() {
    // A slab sweeping the top left of the map, x 0 to 140, y 0 to 30.
    let w = world(&[(50, 140), (250, 140)], Some(block((0, 0), (40, 30), (0, 100), (0, 0), 1)));
    let reach = grown_reach(&w);
    let solid = w.solid();
    let c = ctx(&w, &solid);
    let mut bot = Bot::new(0, BotConfig::default(), 7, &w);
    let mut found = 0;
    for _ in 0..200 {
        let b = &mut Budget::new(100_000);
        if bot.find_random_spot(&c, b) {
            found += 1;
            assert!(!inside(reach, bot.mind.pos_target), "{:?} in {reach:?}", bot.mind.pos_target);
        }
    }
    assert!(found > 0);
    // A worm under the slab: its shooting spots and the path's end are out.
    let w = world(&[(150, 140), (60, 20)], Some(block((0, 0), (40, 30), (0, 100), (0, 0), 1)));
    let solid = w.solid();
    let c = ctx(&w, &solid);
    let mut bot = Bot::new(0, BotConfig::default(), 7, &w);
    bot.mind.target = crate::brain::Target::Worm(w.worms()[1].id);
    for _ in 0..50 {
        let p = bot.path_target(&c, &mut Budget::new(100_000));
        assert!(!inside(reach, p), "{p:?} in {reach:?}");
    }
    let spot = bot.shooting_spot(&c, &mut Budget::new(100_000), &w.worms()[1]);
    assert!(!inside(reach, spot), "{spot:?}");
}

#[test]
fn a_crushed_bot_heads_out_of_the_reach() {
    // A block 30 px square appears over the worm (too far to push it
    // out): wedged, it is crushed.
    let mut w = world(&[(150, 140), (30, 140)], Some(block((135, 115), (30, 30), (0, 0), (-60, 0), 0)));
    let reach = grown_reach(&w);
    let mut bots = vec![Bot::new(0, BotConfig::default(), 1, &w)];
    bots[0].set_goal(Some(Vec2::new(30.0, 140.0)));
    bots[0].set_goal(None);
    play(&mut w, &mut bots, 60, |_, _, _| {});
    assert!(bots[0].escaping_to().is_none());
    w.set_live_value(0, SHOWN, 1).unwrap();
    let mut crushed_at = None;
    let mut t = 0;
    play(&mut w, &mut bots, 10, |_, bots, ev| {
        t += 1;
        if crushed_at.is_none()
            && ev.iter().any(|e| matches!(e.kind, EventKind::Crushed { worm: 0, copy: 0 }))
        {
            crushed_at = Some(t);
            assert!(bots[0].escaping_to().is_none(), "not before the crush");
        }
    });
    assert!(crushed_at.is_some(), "the worm was crushed");
    // The tick after the crush the bot heads out, a racing goal, its own
    // scripted goal (none) kept for after.
    let p = bots[0].escaping_to().expect("heading out");
    assert!(!inside(reach, p), "{p:?} in {reach:?}");
    assert_eq!(bots[0].goal(), None);
    // The part lifts away: the escape ends within its second.
    w.set_live_value(0, Y, -60 * PX).unwrap();
    play(&mut w, &mut bots, (ESCAPE_MS / 10) as u32 + 1, |_, _, _| {});
    assert!(bots[0].escaping_to().is_none());
}

#[test]
fn a_bot_steps_out_of_a_falling_parts_way() {
    // A block 20 px wide comes down on the worm, a pixel a tick from 60 px
    // above: the bot walks out of its way and is never crushed.
    let mut w = world(&[(150, 140), (30, 140)], Some(block((140, 60), (20, 20), (0, 0), (0, 80), 1)));
    let reach = grown_reach(&w);
    let mut bots = vec![Bot::new(0, BotConfig::default(), 1, &w)];
    play(&mut w, &mut bots, 60, |_, _, _| {});
    w.move_live_value_for_test(0, Y, 80 * PX, PX).unwrap();
    let mut crushed = 0;
    let mut fled = false;
    play(&mut w, &mut bots, 150, |_, bots, ev| {
        fled |= bots[0].escaping_to().is_some();
        crushed += ev.iter().filter(|e| matches!(e.kind, EventKind::Crushed { worm: 0, .. })).count();
    });
    assert!(fled);
    assert_eq!(crushed, 0);
    assert!(!inside((reach.0, 0, reach.2, 200), w.worms()[0].pos) || w.worms()[0].pos.y < 60.0);
}

#[test]
fn think_stays_within_its_budget_beside_live_parts() {
    let block = block((60, 100), (24, 4), (-50, 150), (-60, 30), 1);
    let mut w = world(&[(40, 140), (250, 140), (150, 140)], Some(block));
    let cache = AreaCache::new(w.terrain());
    let mut bots: Vec<Bot> = (0..3)
        .map(|i| {
            let personality = if i == 2 { Personality::Chaos } else { Personality::Deathmatch };
            let cfg = BotConfig { personality, ..BotConfig::default() };
            Bot::with_cache(i, cfg, 5, &w, cache.clone())
        })
        .collect();
    let mut ev = TickEvents::new();
    for t in 0..1500u32 {
        // The part sweeps back and forth, up and down.
        if t % 100 == 0 {
            let (x, y) = if t % 200 == 0 { (150, -60) } else { (-50, 30) };
            w.move_live_value_for_test(0, X, x * PX, 3 * PX).unwrap();
            w.move_live_value_for_test(0, Y, y * PX, 2 * PX).unwrap();
        }
        let mut keys = vec![Keys(0); 3];
        for bot in &mut bots {
            keys[bot.worm()] = bot.think(&w, &ev);
            assert!(bot.units_used() <= bot.budget(), "tick {t}: {} > {}", bot.units_used(), bot.budget());
        }
        w.tick(&keys, &mut ev);
    }
    assert!(bots.iter().all(|b| b.reach_checks() > 0));
}

#[test]
fn no_live_parts_no_new_work() {
    let run = |live: Option<LiveCopy>| {
        let mut w = world(&[(40, 140), (250, 140)], live);
        let mut bots = vec![
            Bot::new(0, BotConfig::default(), 3, &w),
            Bot::new(1, BotConfig { personality: Personality::Chaos, ..BotConfig::default() }, 3, &w),
        ];
        let mut units = Vec::new();
        play(&mut w, &mut bots, 600, |_, bots, _| units.push((bots[0].units_used(), bots[1].units_used())));
        (bots.iter().map(Bot::reach_checks).sum::<u64>(), units)
    };
    // Without live parts no reach check runs.
    let (checks, plain_units) = run(None);
    assert_eq!(checks, 0);
    // Nor with a part that never moves or appears: it has no reach, and
    // as rock where the ground is already rock, the bots play the same.
    let mut fixed = block((200, 160), (20, 20), (0, 0), (0, 0), 1);
    fixed.props[usize::from(SHOWN)].min = 1;
    let (checks, units) = run(Some(fixed));
    assert_eq!(checks, 0);
    assert_eq!(units, plain_units);
}
