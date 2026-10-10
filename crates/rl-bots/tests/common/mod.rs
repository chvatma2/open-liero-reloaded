//! Synthetic levels and a small mod for the bots' tests.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::*;
use rl_sim::{
    Keys, Loadout, MatchSetup, OracleMode, SimMod, SimSettings, Slot, TickEvents, World, WormSetup,
};
use rl_terrain::{Stamp, Terrain};

/// A `w` × `h` level whose pixel `(x, y)` is `at(x, y)`.
pub fn level(w: u32, h: u32, at: impl Fn(i32, i32) -> Material) -> Terrain {
    let cells = (0..w * h).map(|i| at((i % w) as i32, (i / w) as i32)).collect();
    Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap())
}

/// Rock where `rock(x, y)`, background elsewhere.
pub fn rocks(w: u32, h: u32, rock: impl Fn(i32, i32) -> bool) -> Terrain {
    level(w, h, |x, y| if rock(x, y) { Material::Rock } else { Material::Background })
}

/// Whether `(x, y)` is in the inclusive box `x0..=x1` × `y0..=y1`.
pub fn inside(x: i32, y: i32, (x0, y0, x1, y1): (i32, i32, i32, i32)) -> bool {
    (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
}

/// The lobbing weapon: speed `LOB_SPEED`, gravity 100, 10 damage on a hit.
pub const LOB: i32 = 0;
pub const LOB_SPEED: i32 = 130;

fn beam(id: &str) -> WeaponDef {
    WeaponDef {
        id: id.into(),
        name: id.into(),
        recharge: 1.0,
        drain: 0.0,
        rof: 0.05,
        laser_sight: false,
        kind: WeaponKind::Beam {
            recoil: 0,
            beam: BeamDef {
                colour: Rgba::rgb(255, 0, 0),
                damage: 0,
                player_damage: 1,
                length: 50,
                init_width: 1,
                width_increase: 0.0,
                distribute_damage_over_width: false,
            },
        },
    }
}

/// Weapon 0 lobs a 4 × 4 shell; weapons 1 to 4 are short beams.
pub fn sim_mod() -> Arc<SimMod> {
    let shell = ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(255, 255, 255)] },
        gravity: None,
        dampening: 1.0,
        width: 4,
        height: 4,
        timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef::olx_default(ActionKind::Disappear),
        player_hit: ActionDef { damage: 10, ..ActionDef::olx_default(ActionKind::Injure) },
        event_spawn: None,
    };
    let lob = WeaponDef {
        id: "lob".into(),
        name: "lob".into(),
        recharge: 1.0,
        drain: 0.0,
        rof: 0.5,
        laser_sight: false,
        kind: WeaponKind::Projectile {
            class: WeaponClass::Other(0),
            recoil: 0,
            sound: None,
            spawn: SpawnDef {
                amount: 1,
                speed: LOB_SPEED,
                use_angle: true,
                ..SpawnDef::olx_default("shell".into())
            },
        },
    };
    let def = ModDef {
        name: "bots".into(),
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
        weapons: vec![lob, beam("b1"), beam("b2"), beam("b3"), beam("b4")],
        projectiles: [("shell".to_string(), shell)].into_iter().collect(),
        restrictions: Default::default(),
    };
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap())
}

fn holes() -> Vec<Stamp> {
    [5u32, 7, 9, 11, 15]
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
        .collect()
}

/// A world on `terrain` with worms at `spawns`, each holding weapons 0 to 4.
pub fn world(terrain: Terrain, spawns: &[(i32, i32)], seed: u32) -> World {
    let settings = SimSettings::lx56();
    let worms = spawns
        .iter()
        .map(|&p| WormSetup {
            team: 0,
            loadout: Loadout::new((0..5).map(Slot::ready).collect(), &settings),
            spawn: Some(p),
        })
        .collect();
    let setup = MatchSetup { holes: holes(), seed, worms, ..MatchSetup::new(settings, sim_mod(), terrain) };
    World::new(setup, Box::new(OracleMode)).unwrap()
}

/// A flat arena: 400 × 200, open above a rock floor at y 150.
pub fn arena() -> Terrain {
    rocks(400, 200, |_, y| y >= 150)
}

/// Ticks `w` `n` times with nobody pressing anything.
pub fn idle(w: &mut World, out: &mut TickEvents, n: u32) {
    for _ in 0..n {
        w.tick(&[], out);
    }
}

/// Ticks with `keys` for worm 0.
pub fn press(w: &mut World, out: &mut TickEvents, k: Keys) {
    w.tick(&[k], out);
}
