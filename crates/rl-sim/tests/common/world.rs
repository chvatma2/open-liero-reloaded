//! The synthetic world of the bench and the snapshot tests: a mod and a
//! level made here, so they need no imported content.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::*;
use rl_sim::{Loadout, MatchSetup, OracleMode, SimMod, SimSettings, Slot, World, WormSetup};
use rl_terrain::{Stamp, Terrain};

/// A pellet that hits as `hit` says (bouncing for a minute and hurting nobody, for `Bounce`): once fired, it
/// stays in the pool.
pub fn pellet(hit: ActionKind) -> ProjectileDef {
    ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(208, 215, 208)] },
        gravity: Some(30),
        dampening: 1.0,
        width: 2,
        height: 2,
        timer: TimerDef { time: 60.0, time_var: 0.0, action: None },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef::olx_default(hit),
        player_hit: ActionDef::olx_default(ActionKind::Bounce),
        event_spawn: None,
    }
}

/// One automatic weapon firing five pellets a shot, never empty.
pub fn sim_mod(hit: ActionKind) -> Arc<SimMod> {
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
        projectiles: [("pellet".to_string(), pellet(hit))].into_iter().collect(),
        restrictions: Default::default(),
    };
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).expect("the bench mod compiles"))
}

/// A `w` × `h` box: rock walls and floor, dirt in its lower third, air above.
pub fn world(worms: usize, w: u32, h: u32) -> World {
    build(worms, w, h, ActionKind::Bounce)
}

/// [`world`] with pellets that carve a hole where they hit the terrain, so
/// firing edits it.
pub fn world_carving(worms: usize, w: u32, h: u32) -> World {
    build(worms, w, h, ActionKind::Carve)
}

fn build(worms: usize, w: u32, h: u32, hit: ActionKind) -> World {
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
    let setup = MatchSetup { holes, seed: 7, worms, ..MatchSetup::new(settings, sim_mod(hit), terrain) };
    World::new(setup, Box::new(OracleMode)).expect("the bench world")
}
