//! A synthetic match for the replay tests: a small mod, an arena with dirt
//! and rock, four Deathmatch worms (two of them chaos bots), and keys from
//! a seeded generator.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::*;
use rl_modes::{ChaosCycle, Deathmatch, DeathmatchSettings, RespawnLoadout};
use rl_replay::{Assets, ModeSpec, PackRef, Packs};
use rl_sim::{Keys, Loadout, MatchSetup, SimMod, SimSettings, Slot, WormSetup};
use rl_terrain::{Stamp, Terrain};

/// A bouncing, exploding pellet weapon and four short killing beams.
pub fn sim_mod() -> Arc<SimMod> {
    let pellet = ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(255, 255, 255)] },
        gravity: Some(70),
        dampening: 1.0,
        width: 2,
        height: 2,
        timer: TimerDef {
            time: 0.6,
            time_var: 0.2,
            action: Some(ActionDef::olx_default(ActionKind::Explode)),
        },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef::olx_default(ActionKind::Bounce),
        player_hit: ActionDef { damage: 15, ..ActionDef::olx_default(ActionKind::Injure) },
        event_spawn: None,
    };
    let gun = WeaponDef {
        id: "gun".into(),
        name: "gun".into(),
        recharge: 1.0,
        drain: 0.0,
        rof: 0.2,
        laser_sight: false,
        kind: WeaponKind::Projectile {
            class: WeaponClass::Other(0),
            recoil: 0,
            sound: None,
            spawn: SpawnDef {
                amount: 2,
                speed: 120,
                spread: 10.0,
                use_angle: true,
                ..SpawnDef::olx_default("pellet".into())
            },
        },
    };
    let beam = |id: &str| WeaponDef {
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
                damage: 1,
                player_damage: 5,
                length: 80,
                init_width: 1,
                width_increase: 0.0,
                distribute_damage_over_width: false,
            },
        },
    };
    let def = ModDef {
        name: "replay".into(),
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
        weapons: vec![gun, beam("b1"), beam("b2"), beam("b3"), beam("b4")],
        projectiles: [("pellet".to_string(), pellet)].into_iter().collect(),
        restrictions: Default::default(),
    };
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap())
}

/// 320 × 200: a rock floor, a dirt band and open air above.
pub fn arena() -> Terrain {
    let (w, h) = (320u32, 200u32);
    let cells = (0..w * h)
        .map(|i| match i / w {
            y if y >= 180 => Material::Rock,
            y if y >= 140 => Material::Dirt,
            _ => Material::Background,
        })
        .collect();
    Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap())
}

pub fn holes() -> Vec<Stamp> {
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

/// What the synthetic match is made of, as `rl-content` would resolve it.
pub fn assets() -> Assets {
    Assets { sim_mod: sim_mod(), terrain: arena(), holes: holes(), green_mask: None }
}

/// Pack ids and hashes that name the synthetic assets.
pub fn packs() -> Packs {
    let r = |id: &str| PackRef { id: id.into(), hash: format!("{id}-hash") };
    Packs { mod_pack: r("replay-mod"), level: r("replay-level"), theme: r("replay-theme") }
}

/// `n` worms (at least 2) in a Deathmatch whose worms 2 and 3 are chaos
/// bots, with bonuses on: the setup as `World::new` gets it, and the mode.
pub fn deathmatch(n: usize, seed: u32) -> (MatchSetup, ModeSpec) {
    let mut settings = SimSettings::lx56();
    settings.bonuses = true;
    settings.bonus_frequency = 2.0;
    let worms = (0..n)
        .map(|i| WormSetup {
            team: 0,
            loadout: Loadout::new((0..5).map(|s| Slot::ready((s + i as i32) % 5)).collect(), &settings),
            spawn: if i == 0 { Some((40, 120)) } else { None },
        })
        .collect();
    let a = assets();
    let mut setup = MatchSetup {
        holes: a.holes,
        seed,
        worms,
        bonus_weapons: Some(vec![0, 2, 4]),
        ..MatchSetup::new(settings, a.sim_mod, a.terrain)
    };
    let dm = DeathmatchSettings {
        respawn_loadout: RespawnLoadout::ChaosCycle(ChaosCycle::new(&[2, 3], 5)),
        kill_limit: 0,
        time_limit: 0.0,
        ..DeathmatchSettings::default()
    };
    let spec = ModeSpec::Deathmatch(dm.clone());
    Deathmatch::prepare(dm, &mut setup).unwrap();
    (setup, spec)
}

/// Keys for `n` worms from a seeded xorshift: each worm holds a random
/// combination for 1 to `hold` ticks.
pub struct KeyGen {
    state: u64,
    held: Vec<(Keys, u32)>,
    hold: u32,
}

impl KeyGen {
    pub fn new(seed: u64, n: usize, hold: u32) -> Self {
        KeyGen { state: seed | 1, held: vec![(Keys(0), 0); n], hold: hold.max(1) }
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    pub fn row(&mut self) -> Vec<Keys> {
        for i in 0..self.held.len() {
            if self.held[i].1 == 0 {
                let r = self.next();
                self.held[i] = (Keys(r as u8), 1 + (r >> 8) as u32 % self.hold);
            }
            self.held[i].1 -= 1;
        }
        self.held.iter().map(|h| h.0).collect()
    }
}
