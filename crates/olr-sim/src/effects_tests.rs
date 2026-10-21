use super::*;
use crate::SimSettings;
use crate::actions::{DoActionInfo, Event, apply_to, execute};
use crate::bonus::Bonus;
use crate::proj_sim::tests::{Bench, NOW, inert, sim_mod};
use crate::proj_sim::{Col, PJC_TERRAIN};
use alloc::vec;
use alloc::vec::Vec;
use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_data::mod_def::{ActionDef, ActionKind, ProjectileDef, TimerDef};
use olr_math::TimeDiff;
use olr_terrain::{GreenMask, Stamp};

/// A worm whose recorded positions are all `pos`.
fn worm_at(id: i32, pos: Vec2) -> Worm {
    let mut w = Worm::spawned(id, pos, NOW, &SimSettings::lx56());
    for _ in 0..100 {
        w.pos_recorder.push(pos);
    }
    w
}

fn filled(size: u32, colour: Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(size, size);
    for y in 0..size {
        for x in 0..size {
            img.set(x, y, colour);
        }
    }
    img
}

/// Holes 1 to 5 as all-magenta squares of 1, 3, 5, 7 and 9 px: a hole of
/// size `s` carves `(2s + 1)^2` dirt cells.
fn square_holes() -> Vec<Stamp> {
    (0..5).map(|s| Stamp::from_image(&filled(2 * s + 1, Rgba::MAGENTA))).collect()
}

fn action(kind: ActionKind, damage: i32) -> ActionDef {
    ActionDef { damage, ..ActionDef::olx_default(kind) }
}

#[test]
fn explosion_has_no_falloff() {
    let mut b = Bench::open(sim_mod(vec![("p", inert())], &[]), 10);
    let c = Vec2::new(100.0, 100.0);
    let offsets = [(0.0, 0.0), (3.0, 4.0), (-5.0, 0.0), (5.0, 1.0), (0.0, -6.0)];
    b.worms = offsets.iter().zip(0..).map(|(&(x, y), id)| worm_at(id, c + Vec2::new(x, y))).collect();
    // On the explosion now, but recorded elsewhere: the recording counts.
    let mut moved = worm_at(5, Vec2::new(150.0, 150.0));
    moved.pos = c;
    b.worms.push(moved);
    let mut dead = worm_at(6, c);
    dead.alive = false;
    b.worms.push(dead);
    b.with_ctx(NOW, |_, ctx| explosion(&mut ctx.blast(), NOW, c, 40.0, 0, -1, -1));
    let hp: Vec<f32> = b.worms.iter().map(|w| w.health).collect();
    assert_eq!(hp, [60.0, 60.0, 60.0, 100.0, 100.0, 100.0, 100.0], "d^2 <= 25 gets the full damage");
}

#[test]
fn explosion_hole_size_clamps() {
    let mut b =
        Bench::new(sim_mod(vec![("p", inert())], &[]), MaterialGrid::new(400, 50, Material::Dirt), 10);
    b.holes = square_holes();
    b.worms = vec![worm_at(0, Vec2::new(390.0, 45.0))];
    // (int)damage clamped to 0..=4; NaN and huge values truncate to INT_MIN (x86).
    let cases = [
        (-3.0, 1),
        (0.0, 1),
        (0.99, 1),
        (1.0, 9),
        (2.9, 25),
        (4.0, 81),
        (100.0, 81),
        (f32::NAN, 1),
        (3e9, 1),
    ];
    for (k, &(damage, cells)) in cases.iter().enumerate() {
        let (dirt, ents) = (b.worms[0].dirt_count, b.draws.ents);
        let pos = Vec2::new(20.0 + 30.0 * k as f32, 20.0);
        b.with_ctx(NOW, |_, ctx| explosion(&mut ctx.blast(), NOW, pos, damage, 0, 0, -1));
        assert_eq!(b.worms[0].dirt_count - dirt, cells, "damage {damage}: carved, counted for the owner");
        assert_eq!(b.draws.ents - ents, 3, "two dirt particles and the explosion");
    }
    // A theme without holes carves nothing.
    b.holes.clear();
    let dirt = b.worms[0].dirt_count;
    b.with_ctx(NOW, |_, ctx| explosion(&mut ctx.blast(), NOW, Vec2::new(300.0, 20.0), 4.0, 0, 0, -1));
    assert_eq!(b.worms[0].dirt_count, dirt);
}

#[test]
fn timer_explode_uses_playerhit_damage() {
    // Both explode on their timer with damage 1. PlyHit explodes with damage
    // 3 in `a`, so `a`'s timer explosion uses 3 (projectile_doTimerExplode);
    // `b`'s PlyHit injures, so its timer explosion keeps 1.
    let bomb = |ply: ActionDef| ProjectileDef {
        timer: TimerDef { time: 0.02, time_var: 0.0, action: Some(action(ActionKind::Explode, 1)) },
        player_hit: ply,
        ..inert()
    };
    let m = sim_mod(
        vec![("a", bomb(action(ActionKind::Explode, 3))), ("b", bomb(action(ActionKind::Injure, 3)))],
        &[],
    );
    let mut b = Bench::new(m, MaterialGrid::new(100, 100, Material::Dirt), 10);
    b.holes = square_holes();
    b.worms = vec![worm_at(0, Vec2::new(95.0, 5.0))];
    let t = |ms: u64| NOW + TimeDiff(ms);
    for (name, x, want) in [("a", 30.0, 49), ("b", 70.0, 9)] {
        // Resting in dirt: each step collides and goes back (Hit: Nothing).
        b.spawn(name, Vec2::new(x, 50.0), Vec2::ZERO, 0, t(0));
        let i = b.pool.iter().map(|(i, _)| i).last().unwrap();
        b.pool.get_mut(i).unwrap().owner = 0;
        let dirt = b.worms[0].dirt_count;
        b.simulate(t(55));
        assert!(b.pool.get(i).is_none(), "{name}: exploded");
        assert_eq!(b.worms[0].dirt_count - dirt, want, "{name}: the hole's size");
    }
}

#[test]
fn carve_dirt_green_dirt_and_injure_actions() {
    let m = sim_mod(vec![("p", inert())], &[]);
    let mut b = Bench::new(m, MaterialGrid::new(40, 40, Material::Dirt), 10);
    b.holes = square_holes();
    b.worms = vec![worm_at(0, Vec2::new(35.0, 35.0)), worm_at(1, Vec2::new(5.0, 35.0))];
    b.spawn("p", Vec2::new(20.5, 20.5), Vec2::ZERO, 0, NOW);
    b.pool.get_mut(0).unwrap().owner = 0;
    let terrain = Event { timer_hit: false, col: Some(Col::Terrain(PJC_TERRAIN)), dt: TimeDiff(11) };
    let worm = Event { timer_hit: false, col: Some(Col::Worm(1)), dt: TimeDiff(11) };
    let timer = Event { timer_hit: true, col: None, dt: TimeDiff(11) };
    let apply = |b: &mut Bench, a: &ActionDef, ev: &Event| {
        b.with_ctx(NOW, |pool, ctx| {
            let mut info = DoActionInfo::default();
            apply_to(a, None, ev, pool.get_mut(0).unwrap(), &mut info, ctx);
            info.delete_after
        })
    };

    // Carve: a hole of size Damage at once, counted for the owner; not on a worm.
    let carve = action(ActionKind::Carve, 2);
    assert!(!apply(&mut b, &carve, &worm));
    assert_eq!(b.worms[0].dirt_count, 0);
    assert!(apply(&mut b, &carve, &terrain));
    assert_eq!(b.worms[0].dirt_count, 25);
    assert_eq!(b.terrain.material(20, 20), Some(Material::Background));

    // Injure deletes the projectile, InjureWorm does not; neither acts on the timer.
    assert!(apply(&mut b, &action(ActionKind::Injure, 7), &worm));
    assert!(!apply(&mut b, &action(ActionKind::InjureWorm, 7), &worm));
    assert!(!apply(&mut b, &action(ActionKind::Injure, 7), &timer));
    assert!(!apply(&mut b, &action(ActionKind::InjureWorm, 7), &timer));
    assert_eq!((b.worms[0].health, b.worms[1].health), (100.0, 86.0));

    // Dirt: three PlaceDirt(5) at pos - (6,6), pos + (6,-6), pos + (0,6),
    // offsets added in float, then truncated toward zero. Hole 5 is 1 px here.
    let mut b2 = Bench::open(sim_mod(vec![("p", inert())], &[]), 10);
    b2.holes = vec![Stamp::from_image(&filled(1, Rgba::MAGENTA)); 5];
    b2.worms = vec![worm_at(0, Vec2::new(150.0, 150.0))];
    let exec = |b: &mut Bench, pos: Vec2, info: DoActionInfo| {
        b.spawn("p", pos, Vec2::ZERO, 0, NOW);
        let i = b.pool.iter().map(|(i, _)| i).last().unwrap();
        b.pool.get_mut(i).unwrap().owner = 0;
        let (mut info, rand) = (info, b.draws.rand.calls());
        b.with_ctx(NOW, |pool, ctx| execute(pool, i, &mut info, ctx));
        assert!(b.pool.get(i).is_none(), "deleted");
        b.draws.rand.calls() - rand
    };
    assert_eq!(
        exec(&mut b2, Vec2::new(5.5, 10.5), DoActionInfo { dirt: true, ..DoActionInfo::default() }),
        0
    );
    for (x, y) in [(0, 4), (11, 4), (5, 16)] {
        assert_eq!(b2.terrain.material(x, y), Some(Material::Dirt), "({x}, {y})");
    }
    assert_eq!(b2.worms[0].dirt_count, -3, "placed dirt is taken off the owner's count");

    // Green dirt: nothing without the mask; with it, one rand() per green pixel.
    let green = DoActionInfo { green_dirt: true, ..DoActionInfo::default() };
    let draws = exec(&mut b2, Vec2::new(7.9, 30.2), green);
    assert_eq!((b2.terrain.material(7, 30), draws), (Some(Material::Background), 0));
    b2.green_mask = Some(GreenMask::from_image(&filled(1, Rgba::rgb(0, 255, 0))));
    let green = DoActionInfo { green_dirt: true, ..DoActionInfo::default() };
    let draws = exec(&mut b2, Vec2::new(7.9, 30.2), green);
    assert_eq!((b2.terrain.material(7, 30), draws), (Some(Material::Dirt), 1));
    assert_eq!(b2.worms[0].dirt_count, -4);
}

#[test]
fn explosions_destroy_and_explode_nearby_bonuses() {
    // CClient::Explosion (CClient_Game.cpp:255-270): a bonus within 15 px on
    // both axes is destroyed and explodes, damage 15, at the same position,
    // before the outer explosion's own damage. 16 px away stays.
    for on in [true, false] {
        let mut b = Bench::open(sim_mod(vec![("p", inert())], &[]), 10);
        b.settings.bonuses = on;
        let c = Vec2::new(100.0, 100.0);
        b.worms = vec![worm_at(0, c + Vec2::new(0.0, 4.0))];
        b.bonuses[0] = Bonus { used: true, pos: c + Vec2::new(10.0, -15.0), ..Bonus::default() };
        b.bonuses[5] = Bonus { used: true, pos: c + Vec2::new(16.0, 0.0), ..Bonus::default() };
        b.with_ctx(NOW, |_, ctx| explosion(&mut ctx.blast(), NOW, c, 40.0, 0, -1, -1));
        let used = (b.bonuses[0].used, b.bonuses[5].used);
        if on {
            assert_eq!((used, b.worms[0].health), ((false, true), 100.0 - 15.0 - 40.0));
        } else {
            assert_eq!((used, b.worms[0].health), ((true, true), 60.0));
        }
    }
}
