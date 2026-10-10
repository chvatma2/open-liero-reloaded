//! A small arena for Deathmatch tests: worms on a rock floor, beams that
//! kill or graze at once, and a bomb that blows up its own worm.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::*;
use rl_modes::{Deathmatch, DeathmatchSettings};
use rl_sim::{
    EventKind, Keys, Loadout, MatchResult, MatchSetup, Message, SimMod, SimSettings, Slot, TickEvents, World,
    WormSetup,
};
use rl_terrain::{Stamp, Terrain};

/// A beam that kills any worm it reaches (1000 damage).
pub const LASER: i32 = 0;
/// A beam that takes 30 health.
pub const GRAZE: i32 = 1;
/// A still projectile that explodes on its own worm 20 ms after the shot.
pub const BOMB: i32 = 2;

fn beam(id: &str, player_damage: i32) -> WeaponDef {
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
                player_damage,
                length: 999,
                init_width: 1,
                width_increase: 0.0,
                distribute_damage_over_width: false,
            },
        },
    }
}

pub fn sim_mod() -> Arc<SimMod> {
    let blast = ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(255, 255, 255)] },
        gravity: Some(0),
        dampening: 1.0,
        width: 1,
        height: 1,
        timer: TimerDef {
            time: 0.02,
            time_var: 0.0,
            action: Some(ActionDef { damage: 1000, ..ActionDef::olx_default(ActionKind::Explode) }),
        },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef::olx_default(ActionKind::Nothing),
        player_hit: ActionDef::olx_default(ActionKind::Nothing),
        event_spawn: None,
    };
    let bomb = WeaponDef {
        id: "bomb".into(),
        name: "bomb".into(),
        recharge: 1.0,
        drain: 0.0,
        rof: 0.05,
        laser_sight: false,
        kind: WeaponKind::Projectile {
            class: WeaponClass::Other(0),
            recoil: 0,
            sound: None,
            spawn: SpawnDef { amount: 1, speed: 0, ..SpawnDef::olx_default("blast".into()) },
        },
    };
    let def = ModDef {
        name: "arena".into(),
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
        weapons: vec![beam("laser", 1000), beam("graze", 30), bomb],
        projectiles: [("blast".to_string(), blast)].into_iter().collect(),
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

/// The arena's setup: a 400 × 200 map, open above a rock floor at y 150,
/// worm `i` at `(xs[i].0, 140)` with weapon `xs[i].1` in every slot. Beams
/// start at the worm, so a worm's beam hits the next worm to its right.
pub fn setup(xs: &[(i32, i32)]) -> MatchSetup {
    let cells =
        (0..400 * 200).map(|i| if i / 400 < 150 { Material::Background } else { Material::Rock }).collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(400, 200, cells).unwrap());
    let settings = SimSettings { shoot_spawn_distance: 0.0, ..SimSettings::lx56() };
    let worms = xs
        .iter()
        .map(|&(x, weapon)| WormSetup {
            team: 0,
            loadout: Loadout::new(vec![Slot::ready(weapon); 5], &settings),
            spawn: Some((x, 140)),
        })
        .collect();
    MatchSetup { holes: holes(), seed: 7, worms, ..MatchSetup::new(settings, sim_mod(), terrain) }
}

/// A Deathmatch world in the arena.
pub fn world(dm: DeathmatchSettings, xs: &[(i32, i32)]) -> World {
    let mut s = setup(xs);
    let mode = Deathmatch::prepare(dm, &mut s).unwrap();
    World::new(s, Box::new(mode)).unwrap()
}

/// What a test watches for, with the tick (0-based) it came in.
#[derive(Clone, Debug, PartialEq)]
pub enum Seen {
    Kill { victim: i32, killer: i32 },
    Message(Message),
    GameOver(MatchResult),
}

/// A world ticked with the keys a test gives, logging kills, messages and
/// the game over.
pub struct Run {
    pub w: World,
    pub out: TickEvents,
    pub ticks: u32,
    pub log: Vec<(u32, Seen)>,
}

impl Run {
    pub fn new(w: World) -> Self {
        Run { w, out: TickEvents::new(), ticks: 0, log: Vec::new() }
    }

    pub fn tick(&mut self, keys: &[Keys]) {
        self.w.tick(keys, &mut self.out);
        for e in self.out.iter() {
            let seen = match e.kind {
                EventKind::Kill { victim, killer } => Seen::Kill { victim, killer },
                EventKind::Message(m) => Seen::Message(m),
                EventKind::GameOver(r) => Seen::GameOver(r),
                _ => continue,
            };
            self.log.push((self.ticks, seen));
        }
        self.ticks += 1;
    }

    pub fn idle(&mut self, n: u32) {
        for _ in 0..n {
            self.tick(&[]);
        }
    }

    /// Worms `who` press fire for one tick; then everyone waits `wait` ticks.
    pub fn fire(&mut self, who: &[usize], wait: u32) {
        let mut keys = vec![Keys(0); self.w.worms().len()];
        for &i in who {
            keys[i] = Keys(Keys::FIRE);
        }
        self.tick(&keys);
        self.idle(wait);
    }

    /// Ticks until the world's frame `sf` (`World::server_frame`).
    pub fn run_to_frame(&mut self, sf: u64) {
        while self.w.server_frame() < sf {
            self.tick(&[]);
        }
    }

    pub fn kills(&self) -> Vec<(u32, i32, i32)> {
        self.log
            .iter()
            .filter_map(|(t, s)| match s {
                Seen::Kill { victim, killer } => Some((*t, *victim, *killer)),
                _ => None,
            })
            .collect()
    }

    pub fn messages(&self) -> Vec<Message> {
        self.log
            .iter()
            .filter_map(|(_, s)| match s {
                Seen::Message(m) => Some(*m),
                _ => None,
            })
            .collect()
    }

    pub fn game_overs(&self) -> Vec<(u32, MatchResult)> {
        self.log
            .iter()
            .filter_map(|(t, s)| match s {
                Seen::GameOver(r) => Some((*t, *r)),
                _ => None,
            })
            .collect()
    }

    /// `(worm, score)` of the scoreboard, best first.
    pub fn board(&self) -> Vec<(i32, i32)> {
        self.w.scores().iter().map(|s| (s.worm, s.score)).collect()
    }
}

pub fn msg(text: rl_sim::TextId, args: [i32; 3]) -> Message {
    Message { text, args }
}
