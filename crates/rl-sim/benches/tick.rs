//! `World::tick` benchmarks (spec §7, §21 G8), on a mod and level made
//! here so they need no imported content: a tick with OLX's projectile
//! pool full (3000 bouncing pellets), and a tick of 32 worms walking and
//! digging with nothing in the air. `cargo xtask bench` runs them and
//! compares the medians with `fixtures/bench/baselines.ron`.

use std::collections::BTreeMap;
use std::sync::Arc;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::*;
use rl_sim::{
    Keys, Loadout, MatchSetup, OracleMode, SimMod, SimSettings, Slot, TickEvents, World, WormSetup,
};
use rl_terrain::{Stamp, Terrain};

fn main() {
    divan::main();
}

/// A pellet that bounces for a minute and hurts nobody: once fired, it
/// stays in the pool.
fn pellet() -> ProjectileDef {
    ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(208, 215, 208)] },
        gravity: Some(30),
        dampening: 1.0,
        width: 2,
        height: 2,
        timer: TimerDef { time: 60.0, time_var: 0.0, action: None },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef::olx_default(ActionKind::Bounce),
        player_hit: ActionDef::olx_default(ActionKind::Bounce),
        event_spawn: None,
    }
}

/// One automatic weapon firing five pellets a shot, never empty.
fn sim_mod() -> Arc<SimMod> {
    let spray = WeaponDef {
        id: "spray".into(),
        name: "spray".into(),
        recharge: 1.0,
        drain: 0.0,
        rof: 0.01,
        laser_sight: false,
        kind: WeaponKind::Projectile {
            class: WeaponClass::Automatic,
            recoil: 0,
            sound: None,
            spawn: SpawnDef {
                amount: 5,
                speed: 200,
                spread: 40.0,
                use_angle: true,
                use_random_rot: true,
                ..SpawnDef::olx_default("pellet".into())
            },
        },
    };
    let def = ModDef {
        name: "bench".into(),
        worm: WormParams {
            angle_speed: 150.0,
            ground_speed: 8.0,
            air_speed: 4.0,
            gravity: 100.0,
            jump_force: -65.0,
            air_friction: 0.0,
            ground_friction: 0.4,
        },
        rope: RopeParams { max_length: 300, rest_length: 27, strength: 4.5 },
        weapons: vec![spray],
        projectiles: [("pellet".to_string(), pellet())].into_iter().collect(),
        restrictions: Default::default(),
    };
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).expect("the bench mod compiles"))
}

/// A `w` × `h` box: rock walls and floor, dirt in its lower third, air above.
fn world(worms: usize, w: u32, h: u32) -> World {
    let cells = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if x < 4 || x >= w - 4 || y < 4 || y >= h - 4 {
                Material::Rock
            } else if y > h * 2 / 3 {
                Material::Dirt
            } else {
                Material::Background
            }
        })
        .collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).expect("a grid"));
    let holes = [5u32, 7, 9, 11, 15]
        .into_iter()
        .map(|s| {
            let mut img = RgbaImage::new(s, s);
            for y in 0..s {
                for x in 0..s {
                    img.set(x, y, Rgba::MAGENTA);
                }
            }
            Stamp::from_image(&img)
        })
        .collect();
    let settings = SimSettings::lx56();
    let loadout = Loadout::new(vec![Slot::ready(0); 5], &settings);
    let worms = (0..worms)
        .map(|i| {
            let x = 20 + (i as u32 * (w - 40) / worms.max(1) as u32) as i32;
            WormSetup { team: 0, loadout: loadout.clone(), spawn: Some((x, (h / 2) as i32)) }
        })
        .collect();
    let setup = MatchSetup { holes, seed: 7, worms, ..MatchSetup::new(settings, sim_mod(), terrain) };
    World::new(setup, Box::new(OracleMode)).expect("the bench world")
}

/// Eight worms firing until OLX's pool (3000) is full, then still firing.
fn full_pool() -> (World, Vec<Keys>) {
    let mut w = world(8, 640, 400);
    let keys = vec![Keys(Keys::FIRE | Keys::UP); 8];
    let mut ev = TickEvents::new();
    for _ in 0..3000 {
        w.tick(&keys, &mut ev);
        if w.projectiles().count() >= w.projectile_capacity() {
            break;
        }
    }
    assert_eq!(w.projectiles().count(), w.projectile_capacity(), "the pool fills");
    (w, keys)
}

/// One tick with the projectile pool full (3000 projectiles).
#[divan::bench(sample_count = 60)]
fn tick_full_pool(b: divan::Bencher) {
    let (w, keys) = full_pool();
    b.with_inputs(|| (w.clone(), TickEvents::new())).bench_local_refs(|(w, ev)| w.tick(&keys, ev));
}

/// One tick of 32 worms walking and digging (a 32-player match's worms
/// alone).
#[divan::bench(sample_count = 60, sample_size = 50)]
fn tick_32_worms(b: divan::Bencher) {
    let mut w = world(32, 1200, 500);
    let mut ev = TickEvents::new();
    let walk = |t: u32| vec![Keys(if (t / 50).is_multiple_of(2) { Keys::RIGHT } else { Keys::LEFT }); 32];
    for t in 0..200 {
        w.tick(&walk(t), &mut ev);
    }
    let keys = walk(1);
    b.with_inputs(|| (w.clone(), TickEvents::new())).bench_local_refs(|(w, ev)| w.tick(&keys, ev));
}
