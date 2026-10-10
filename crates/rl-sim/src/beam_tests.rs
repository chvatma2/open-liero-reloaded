//! Unit tests of beams and special weapons.
use super::*;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::{WeaponDef, WeaponKind};
use rl_math::{GlibcRand, Mt19937};

use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use crate::shots::ShootList;
use crate::weapons::{ShootCtx, player_shoot, simulate_worm_weapon, worm_shoot};
use crate::worm::Slot;

const T0: AbsTime = AbsTime(100_000);

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

/// A 100 × 50 map, open left of `dirt_from` and dirt from there on.
fn terrain(dirt_from: u32) -> Terrain {
    let cells = (0..50 * 100)
        .map(|i| if i % 100 >= dirt_from { Material::Dirt } else { Material::Background })
        .collect();
    Terrain::from_grid(&MaterialGrid::from_cells(100, 50, cells).unwrap())
}

/// Classic's Laser (`laser.ron`).
fn laser() -> WeaponDef {
    WeaponDef {
        id: "laser".to_string(),
        name: "Laser".to_string(),
        recharge: 0.18,
        drain: 1.25,
        rof: 0.009,
        laser_sight: false,
        kind: WeaponKind::Beam {
            recoil: 0,
            beam: BeamDef {
                colour: Rgba::rgb(255, 0, 0),
                damage: 0,
                player_damage: 1,
                length: 999_999,
                init_width: 1,
                width_increase: 0.0,
                distribute_damage_over_width: false,
            },
        },
    }
}

fn jetpack(rof: f32, drain: f32) -> WeaponDef {
    WeaponDef {
        id: "jetpack".to_string(),
        name: "Jetpack".to_string(),
        recharge: 0.1,
        drain,
        rof,
        laser_sight: false,
        kind: WeaponKind::Special { special: SpecialKind::Jetpack, thrust: 100 },
    }
}

/// Weapon 0 is a gun, weapon 1 the given one.
fn sim_mod(second: WeaponDef) -> SimMod {
    let def =
        mod_def(vec![projectile_weapon("gun", "pellet"), second], vec![("pellet", pixel_projectile(None))]);
    SimMod::compile(&def, &BTreeMap::new()).unwrap()
}

fn slot(weapon: i32) -> Slot {
    Slot { weapon, charge: 1.0, reloading: false, last_fire: 0.0 }
}

fn worm(id: i32, x: f32, y: f32) -> Worm {
    Worm::spawned(id, Vec2::new(x, y), T0, &SimSettings::lx56())
}

fn draws() -> Draws {
    Draws::new(GlibcRand::new(1), Mt19937::new(1), 0)
}

#[test]
fn beam_carves_first_solid_pixel() {
    let m = sim_mod(laser());
    let s = SimSettings::lx56();
    let (holes, mut t, mut d, mut parts) = (holes(), terrain(60), draws(), Vec::with_capacity(50));
    // Worm 0 holds the gun; the laser is in slots 1 and 2. Worm 1 is 5 px
    // off the beam's line: `GetLength2() < 25` misses it.
    let mut ws = vec![worm(0, 20.0, 25.0), worm(1, 40.0, 30.0)];
    ws[0].slots = vec![slot(0), slot(1), slot(1)];
    let shot = Shot { worm: 0, weapon: 1, pos: Vec2::new(20.0, 25.0), angle: 0, ..Shot::default() };
    let mut ctx = BeamCtx { terrain: &mut t, holes: &holes, draws: &mut d, parts: &mut parts };
    process_shot_beam(
        &shot,
        &m.weapons()[1],
        &mut ws,
        &mut ctx,
        &s,
        T0,
        &mut [Bonus::default(); MAX_BONUSES],
        &mut crate::referee::Referee::oracle(),
    );
    // The first slot holding the beam becomes current and draws for 0.3 s.
    assert_eq!((ws[0].current_weapon, ws[0].slots[1].last_fire), (1, 0.3));
    // Damage 0 is the smallest hole (5 × 5), centred on the first dirt pixel (60, 25).
    assert_eq!(ws[0].dirt_count, 3 * 5);
    assert_eq!(ctx.terrain.material(62, 25), Some(Material::Background));
    assert_eq!(ctx.terrain.material(63, 25), Some(Material::Dirt));
    assert_eq!((ws[1].health, ctx.draws.rand.calls(), ctx.draws.ents), (100.0, 0, 0));

    // A worm within 5 px takes PlyDamage (and bleeds); the beam stops there.
    ws[1].pos = Vec2::new(40.0, 29.0);
    process_shot_beam(
        &shot,
        &m.weapons()[1],
        &mut ws,
        &mut ctx,
        &s,
        T0,
        &mut [Bonus::default(); MAX_BONUSES],
        &mut crate::referee::Referee::oracle(),
    );
    assert_eq!((ws[1].health, ws[0].dirt_count), (99.0, 15));
    assert!(ctx.draws.rand.calls() > 0, "blood");

    // DrawBeam: from the worm's own position at its aim, every sub-step. It
    // carves the first dirt, now at x = 63, and its explosion takes an
    // entity. A worm in the way stops it unharmed.
    let (calls, ents) = (ctx.draws.rand.calls(), ctx.draws.ents);
    ws[0].pos = Vec2::new(20.5, 25.7);
    ws[1].pos = Vec2::new(40.0, 40.0);
    draw_beam(&mut ws, 0, &m, &mut ctx);
    assert_eq!((ws[0].dirt_count, ctx.draws.ents), (15 + 15, ents + 1));
    assert_eq!(ctx.terrain.material(63, 25), Some(Material::Background));
    ws[1].pos = Vec2::new(40.5, 27.0);
    draw_beam(&mut ws, 0, &m, &mut ctx);
    assert_eq!((ws[0].dirt_count, ctx.draws.ents, ws[1].health), (30, ents + 2, 99.0));
    assert_eq!(ctx.draws.rand.calls(), calls, "no damage, no draws");
    // Facing left at angle 0, it aims at 180° and stops at the map edge (rock).
    (ws[0].face, ws[1].alive) = (Dir::Left, false);
    draw_beam(&mut ws, 0, &m, &mut ctx);
    assert_eq!((ws[0].dirt_count, ctx.draws.ents), (30, ents + 3));
    // Each drawn beam names its worm and the weapon's colour (`SetWormBeamEntity`).
    let beams: Vec<_> = ctx
        .draws
        .events
        .iter()
        .filter_map(|e| match e.kind {
            crate::EventKind::SpawnCosmetic { kind: CosmeticKind::Beam { worm, colour }, .. } => {
                Some((worm, colour))
            }
            _ => None,
        })
        .collect();
    assert_eq!(beams, vec![(0, Rgba::rgb(255, 0, 0)); 3]);
}

#[test]
fn beam_sets_lastfire_burst() {
    // Per sub-step: simulateWormWeapon (LastFire -= 11 ms), WormShoot (fires
    // at LastFire <= 0, sets ROF 0.009), PlayerShoot (draws while LastFire
    // > 0, then -= 10 ms). The laser fires every sub-step until its first
    // shot is delivered and sets LastFire = 0.3; then it only draws, and
    // fires again in the 15th sub-step: 0.3 - 14 * 0.021 - 0.011 <= 0.
    let m = sim_mod(laser());
    let s = SimSettings::lx56();
    let (holes, mut t, mut d, mut parts) = (holes(), terrain(100), draws(), Vec::with_capacity(50));
    let mut ws = vec![worm(0, 20.0, 25.0)];
    ws[0].slots = vec![slot(1); 5];
    let (mut list, mut rand) = (ShootList::new(), GlibcRand::new(1));
    let dt = TimeDiff(11).seconds();
    let mut fired = Vec::new();
    for step in 1..=18 {
        simulate_worm_weapon(&mut ws[0], dt, &s, &m);
        let before = rand.calls();
        let mut sctx = ShootCtx {
            sim_mod: &m,
            settings: &s,
            list: &mut list,
            rand: &mut rand,
            server_time: TimeDiff(0),
        };
        worm_shoot(&mut ws[0], &mut sctx);
        if rand.calls() > before {
            fired.push(step);
        }
        let mut ctx = BeamCtx { terrain: &mut t, holes: &holes, draws: &mut d, parts: &mut parts };
        player_shoot(&mut ws, 0, &m, &mut ctx);
        if step == 2 {
            // Model R delivers the first shots between frames.
            let shot = Shot { worm: 0, weapon: 1, pos: Vec2::new(20.0, 25.0), ..Shot::default() };
            process_shot_beam(
                &shot,
                &m.weapons()[1],
                &mut ws,
                &mut ctx,
                &s,
                T0,
                &mut [Bonus::default(); MAX_BONUSES],
                &mut crate::referee::Referee::oracle(),
            );
            assert_eq!(ws[0].current_slot().last_fire, 0.3);
        }
    }
    assert_eq!(fired, [1, 2, 17, 18]);
    // DrawBeam ran in every sub-step and hit the map's right edge each time.
    assert_eq!(d.ents, 18);
}

#[test]
fn jetpack_quirks() {
    let s = SimSettings::lx56();
    let (holes, mut t, mut parts) = (holes(), terrain(100), Vec::new());
    let mut list = ShootList::new();
    let mut step = |m: &SimMod, w: &mut Worm, d: &mut Draws| {
        let mut rand = GlibcRand::new(1);
        let mut sctx =
            ShootCtx { sim_mod: m, settings: &s, list: &mut list, rand: &mut rand, server_time: TimeDiff(0) };
        worm_shoot(w, &mut sctx);
        assert_eq!(rand.calls(), 0, "specials add no shot");
        let mut ctx = BeamCtx { terrain: &mut t, holes: &holes, draws: d, parts: &mut parts };
        player_shoot(core::slice::from_mut(w), 0, m, &mut ctx);
    };
    // ROF > 0: WormShoot sets LastFire and drains, so PlayerShoot returns: no thrust.
    let m = sim_mod(jetpack(0.05, 10.0));
    let (mut w, mut d) = (worm(0, 20.0, 25.0), draws());
    w.slots = vec![slot(1)];
    step(&m, &mut w, &mut d);
    assert_eq!((w.vel.y, w.current_slot().charge, d.rand.calls(), d.ents), (0.0, 1.0 - 10.0 / 100.0, 0, 0));

    // ROF 0: WormShoot drains, then ShootSpecial thrusts, sprays (one
    // GetRandomNum, one entity) and drains again.
    let m = sim_mod(jetpack(0.0, 10.0));
    let (mut w, mut d) = (worm(0, 20.0, 25.0), draws());
    w.slots = vec![slot(1)];
    w.vel.y = 3.0;
    step(&m, &mut w, &mut d);
    assert_eq!(w.vel.y, 3.0 + -50.0 * (100.0 * TimeDiff(10).seconds()));
    assert_eq!((w.current_slot().charge, d.rand.calls(), d.ents), (1.0 - 0.1 - 0.1, 1, 1));

    // ShootSpecial reloads only below 0; WormShoot at 0 or below.
    let m = sim_mod(jetpack(0.0, 50.0));
    let (mut w, mut d) = (worm(0, 20.0, 25.0), draws());
    w.slots = vec![slot(1)];
    step(&m, &mut w, &mut d);
    assert_eq!((w.current_slot().charge, w.current_slot().reloading), (0.0, false));
    let vy = w.vel.y;
    step(&m, &mut w, &mut d);
    assert_eq!((w.current_slot().charge, w.current_slot().reloading), (0.0, true));
    assert_eq!((w.vel.y, d.rand.calls()), (vy, 1), "a reloading jetpack does not thrust");
}

#[test]
fn beam_explodes_a_bonus_within_3_px() {
    let m = sim_mod(laser());
    let s = SimSettings { bonuses: true, ..SimSettings::lx56() };
    let (holes, mut d, mut parts) = (holes(), draws(), Vec::with_capacity(50));
    let mut ws = vec![worm(0, 20.0, 25.0)];
    ws[0].slots = vec![slot(1)];
    let shot = Shot { worm: 0, weapon: 1, pos: Vec2::new(20.0, 25.0), spawn: T0, ..Shot::default() };
    let bonus_at = |x: f32, y: f32| {
        let mut b = [Bonus::default(); MAX_BONUSES];
        b[3] = Bonus { used: true, pos: Vec2::new(x, y), ..Bonus::default() };
        b
    };
    // 2 px off the line: the part at x = 38 is within 3 px (sqrt(4 + 4)). It
    // stops there in an explosion of damage 0, which destroys the bonus and
    // explodes it (damage 15): two explosion entities, and the dirt at
    // x = 60 is never reached.
    let mut t = terrain(60);
    let mut bonuses = bonus_at(40.0, 27.0);
    let mut ctx = BeamCtx { terrain: &mut t, holes: &holes, draws: &mut d, parts: &mut parts };
    process_shot_beam(
        &shot,
        &m.weapons()[1],
        &mut ws,
        &mut ctx,
        &s,
        T0,
        &mut bonuses,
        &mut crate::referee::Referee::oracle(),
    );
    assert!(!bonuses[3].used);
    assert_eq!((ctx.draws.ents, ws[0].dirt_count), (2, 0));
    assert_eq!(ctx.terrain.material(60, 25), Some(Material::Dirt));

    // With Bonuses off the beam still stops at it, but nothing explodes it.
    let lx56 = SimSettings::lx56();
    let mut bonuses = bonus_at(40.0, 27.0);
    process_shot_beam(
        &shot,
        &m.weapons()[1],
        &mut ws,
        &mut ctx,
        &lx56,
        T0,
        &mut bonuses,
        &mut crate::referee::Referee::oracle(),
    );
    assert!(bonuses[3].used);
    assert_eq!((ctx.draws.ents, ws[0].dirt_count), (3, 0));

    // Exactly 3 px away is not within (`GetLength() < 3`): the beam carves the dirt.
    let mut bonuses = bonus_at(40.0, 28.0);
    process_shot_beam(
        &shot,
        &m.weapons()[1],
        &mut ws,
        &mut ctx,
        &s,
        T0,
        &mut bonuses,
        &mut crate::referee::Referee::oracle(),
    );
    assert!(bonuses[3].used);
    assert_eq!((ctx.draws.ents, ws[0].dirt_count), (3, 15));
}
