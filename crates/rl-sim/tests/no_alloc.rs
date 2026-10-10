//! Spec §5.3: `World::tick` does not allocate once the world is set up.

use std::collections::BTreeMap;
use std::sync::Arc;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::*;
use rl_math::AbsTime;
use rl_sim::{Keys, Loadout, SimMod, SimSettings, Slot, World};
use rl_terrain::{Stamp, Terrain};

/// A Minigun-like weapon that empties and reloads, and a three-pellet shotgun.
fn sim_mod() -> Arc<SimMod> {
    let pellet = ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(208, 215, 208), Rgba::rgb(176, 176, 176)] },
        gravity: Some(70),
        dampening: 1.0,
        width: 2,
        height: 2,
        timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef::olx_default(ActionKind::Explode),
        player_hit: ActionDef::olx_default(ActionKind::Injure),
        event_spawn: None,
    };
    let weapon = |id: &str, amount: i32, drain: f32| WeaponDef {
        id: id.into(),
        name: id.into(),
        recharge: 1.0,
        drain,
        rof: 0.015,
        laser_sight: false,
        kind: WeaponKind::Projectile {
            class: WeaponClass::Automatic,
            recoil: 6,
            sound: None,
            spawn: SpawnDef {
                amount,
                speed: 250,
                spread: 3.0,
                use_angle: true,
                use_random_rot: true,
                add_parent_vel: true,
                ..SpawnDef::olx_default("pellet".into())
            },
        },
    };
    let def = ModDef {
        name: "test".into(),
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
        weapons: vec![weapon("minigun", 1, 10.0), weapon("shotgun", 3, 20.0)],
        projectiles: [("pellet".to_string(), pellet)].into_iter().collect(),
    };
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap())
}

#[test]
fn tick_does_not_allocate() {
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
    let terrain = Terrain::from_grid(&MaterialGrid::new(300, 200, Material::Dirt));
    let mut w = World::new(SimSettings::lx56(), sim_mod(), terrain, holes, AbsTime(0), 1);
    w.spawn_worm(100, 100);
    w.spawn_worm(200, 100);
    for (i, weapon) in [(0, 0), (1, 1)] {
        let slot = Slot { weapon, charge: 1.0, reloading: false, last_fire: 0.0 };
        let loadout = Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 };
        w.set_loadout(i, &loadout);
    }
    // Both worms fire most of the time, so shots are fired, delivered and
    // spawned, and the weapons empty and reload.
    let a =
        [Keys(Keys::RIGHT | Keys::ROPE | Keys::UP | Keys::FIRE), Keys(Keys::LEFT | Keys::JUMP | Keys::FIRE)];
    let b = [Keys(Keys::LEFT | Keys::FIRE), Keys(Keys::RIGHT | Keys::DOWN)];
    let info = allocation_counter::measure(|| {
        for i in 0..200 {
            w.tick(if i % 3 == 0 { &a } else { &b });
        }
    });
    assert_eq!(info.count_total, 0, "tick allocated");
    assert!(w.projectiles().count() > 20, "{} projectiles", w.projectiles().count());
}
