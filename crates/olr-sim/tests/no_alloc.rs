//! Spec §5.3: `World::tick` does not allocate once the world is set up,
//! with firing, explosions, dirt, bleeding, deaths and respawns, and with
//! bonuses spawned, placed, picked up and exploded; nor do its events, also
//! in a cloned world; nor do a game mode's commands (weapons given mid-match,
//! a worm spawned mid-match), nor a world with live copies (issue #86).
//! (`olr-scenario`'s `tests/no_alloc.rs` fires every
//! weapon of the imported Classic mod.)

use std::collections::BTreeMap;
use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_data::mod_def::*;
use olr_sim::{
    BonusItem, CommandError, Fnv, GameMode, Keys, KillCredit, Loadout, MatchSetup, ModeCtx, SimMod,
    SimSettings, Slot, TickEvents, World, WormSetup, olx_kill_credit,
};
use olr_terrain::{GreenMask, Stamp, Terrain};

#[path = "common/world.rs"]
mod synthetic;

#[path = "common/live.rs"]
mod live_level;

/// A plain pixel projectile: explodes on terrain, injures worms.
fn pellet() -> ProjectileDef {
    ProjectileDef {
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
    }
}

/// A Minigun-like weapon that empties and reloads, and a three-pellet
/// shotgun. Pellets bounce, leave a projectile trail and explode (carving,
/// injuring) into bits after half a second; bits bounce, smoke and place
/// green dirt on a timer.
fn sim_mod() -> Arc<SimMod> {
    let bit = ProjectileDef {
        timer: TimerDef {
            time: 0.2,
            time_var: 0.0,
            action: Some(ActionDef::olx_default(ActionKind::GreenDirt)),
        },
        hit: ActionDef::olx_default(ActionKind::Bounce),
        trail: TrailDef { kind: TrailKind::Smoke, delay: 0.1, spawn: None },
        ..pellet()
    };
    let pellet = ProjectileDef {
        timer: TimerDef {
            time: 0.5,
            time_var: 0.0,
            action: Some(ActionDef {
                spawns_projectiles: true,
                damage: 3,
                ..ActionDef::olx_default(ActionKind::Explode)
            }),
        },
        trail: TrailDef {
            kind: TrailKind::Projectile,
            delay: 0.05,
            spawn: Some(SpawnDef { amount: 1, speed: 10, ..SpawnDef::olx_default("bit".into()) }),
        },
        hit: ActionDef::olx_default(ActionKind::Bounce),
        event_spawn: Some(SpawnDef {
            amount: 3,
            speed: 50,
            spread: 360.0,
            ..SpawnDef::olx_default("bit".into())
        }),
        ..pellet()
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
            sound: Some(format!("sfx/{id}.wav")),
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
    // A beam that widens by a part every two steps, and a jetpack that thrusts.
    let beam = WeaponDef {
        id: "beam".into(),
        name: "beam".into(),
        recharge: 1.0,
        drain: 1.0,
        rof: 0.009,
        laser_sight: true,
        kind: WeaponKind::Beam {
            recoil: 0,
            beam: BeamDef {
                colour: Rgba::rgb(255, 0, 0),
                damage: 1,
                player_damage: 2,
                length: 999_999,
                init_width: 1,
                width_increase: 0.5,
                distribute_damage_over_width: true,
            },
        },
    };
    let jetpack = WeaponDef {
        id: "jetpack".into(),
        name: "jetpack".into(),
        recharge: 1.0,
        drain: 1.0,
        rof: 0.0,
        laser_sight: false,
        kind: WeaponKind::Special { special: SpecialKind::Jetpack, thrust: 50 },
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
        weapons: vec![weapon("minigun", 1, 10.0), weapon("shotgun", 3, 20.0), beam, jetpack],
        projectiles: [("pellet".to_string(), pellet), ("bit".to_string(), bit)].into_iter().collect(),
        restrictions: Default::default(),
        loadouts: Default::default(),
    };
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap())
}

/// A mode that gives weapons by plan: `(tick, worm, weapon)` fills every
/// slot of the worm at the end of that tick. With `chaos`, it also gives
/// one slot a weapon every tick and slot 0 the next weapon on every
/// respawn (as the chaos bot will), and spawns `new` at tick 50.
#[derive(Clone, Debug)]
struct Giver {
    ticks: u32,
    plan: Vec<(u32, i32, i32)>,
    chaos: bool,
    respawns: u32,
    new: WormSetup,
    refused: u32,
}

impl Giver {
    fn new(plan: Vec<(u32, i32, i32)>, chaos: bool) -> Self {
        let new = WormSetup { team: 0, loadout: loadout(0), spawn: Some((250, 100)) };
        Giver { ticks: 0, plan, chaos, respawns: 0, new, refused: 0 }
    }

    fn check(&mut self, r: Result<(), CommandError>) {
        if r.is_err() {
            self.refused += 1;
        }
    }
}

impl GameMode for Giver {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn box_clone(&self) -> Box<dyn GameMode> {
        Box::new(self.clone())
    }

    fn checksum(&self, h: &mut Fnv) {
        h.u32(self.ticks);
        h.u32(self.respawns);
    }

    fn on_spawn(&mut self, worm: i32, ctx: &mut ModeCtx) {
        if self.chaos && self.ticks > 0 {
            self.respawns += 1;
            let r = ctx.give_weapon(worm, 0, (self.respawns % 4) as i32);
            self.check(r);
        }
    }

    fn on_kill(&mut self, victim: i32, killer: i32, ctx: &mut ModeCtx) -> KillCredit {
        olx_kill_credit(ctx.worms(), ctx.settings(), victim, killer)
    }

    fn on_tick(&mut self, ctx: &mut ModeCtx) {
        self.ticks += 1;
        for k in 0..self.plan.len() {
            let (at, worm, weapon) = self.plan[k];
            if at == self.ticks {
                for slot in 0..5 {
                    let r = ctx.give_weapon(worm, slot, weapon);
                    self.check(r);
                }
            }
        }
        if self.chaos {
            let t = self.ticks as i32;
            let r = ctx.give_weapon(t % 2, (t % 5) as usize, t % 4);
            self.check(r);
            if self.ticks == 50 {
                let r = ctx.spawn_worm(&self.new);
                self.check(r);
            }
        }
    }
}

fn loadout(weapon: i32) -> Loadout {
    Loadout::new(vec![Slot::ready(weapon); 5], &SimSettings::lx56())
}

/// Two armed worms (minigun and shotgun) on open air above a dirt floor,
/// bonuses on, worm 0 bleeding and worm 1 dying at once (D9), and bonuses
/// placed for the first tick.
fn world(mode: Giver) -> World {
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
    // Open air above a dirt floor, so projectiles fly, bounce and spawn.
    let cells =
        (0..300 * 200).map(|i| if i / 300 < 150 { Material::Background } else { Material::Dirt }).collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(300, 200, cells).unwrap());
    // A bonus every second, each kept for two.
    let settings =
        SimSettings { bonuses: true, bonus_frequency: 1.0, bonus_life: 2.0, ..SimSettings::lx56() };
    let mut green = RgbaImage::new(8, 9);
    for y in 0..9 {
        for x in 0..8 {
            green.set(x, y, if (x + y) % 3 == 0 { Rgba::rgb(108, 80, 0) } else { Rgba::rgb(0, 255, 0) });
        }
    }
    let worms = [(100, 0), (200, 1)]
        .into_iter()
        .map(|(x, weapon)| WormSetup { team: 0, loadout: loadout(weapon), spawn: Some((x, 100)) })
        .collect();
    let setup = MatchSetup {
        holes,
        green_mask: Some(GreenMask::from_image(&green)),
        seed: 1,
        worms,
        ..MatchSetup::new(settings, sim_mod(), terrain)
    };
    let mut w = World::new(setup, Box::new(mode)).unwrap();
    w.set_health(0, 12.0); // bleeds
    w.set_health(1, 0.0); // dies at once (D9) and respawns
    // Worm 0 picks up the health bonus at once; the others fall to the floor,
    // where the pellets explode.
    w.place_bonus(100, 100, BonusItem::Health).unwrap();
    for x in [120, 150, 180, 210, 240] {
        w.place_bonus(x, 120, BonusItem::Weapon(1)).unwrap();
    }
    w
}

#[test]
fn tick_does_not_allocate() {
    // After 600 ticks, worm 0 holds the beam (weapon 2) on worm 1, worm 1 the jetpack (3).
    let mut w = world(Giver::new(vec![(600, 0, 2), (600, 1, 3)], false));
    // Both worms fire most of the time, so shots are fired, delivered and
    // spawned, and the weapons empty and reload.
    let a =
        [Keys(Keys::RIGHT | Keys::ROPE | Keys::UP | Keys::FIRE), Keys(Keys::LEFT | Keys::JUMP | Keys::FIRE)];
    let b = [Keys(Keys::LEFT | Keys::FIRE), Keys(Keys::RIGHT | Keys::DOWN)];
    let (mut flying, mut picked, mut events) = (0, false, 0);
    let mut out = TickEvents::new();
    let info = allocation_counter::measure(|| {
        for i in 0..599 {
            w.tick(if i % 3 == 0 { &a } else { &b }, &mut out);
            events += out.len();
            if i == 5 {
                picked = !w.bonuses()[0].used && w.server_bonuses()[0].used;
            }
            if i == 199 {
                flying = w.projectiles().count();
            }
        }
    });
    assert_eq!(info.count_total, 0, "tick allocated");
    let dirt: Vec<i32> = w.worms().iter().map(|w| w.dirt_count).collect();
    assert!(dirt.iter().any(|&d| d != 0), "explosions carved or bits placed dirt: {dirt:?}");
    assert!(flying > 20, "{flying} projectiles");
    assert!(w.worms()[0].last_blood.0 != 0, "worm 0 bled");
    assert!(w.worms()[1].alive && w.mt_calls() > 0, "worm 1 died and respawned");
    assert!(picked, "worm 0 picked up the health bonus");
    assert!(w.server_bonuses().iter().filter(|b| b.used).count() >= 2, "bonuses spawned");
    assert!(events > 1000, "{events} events");

    // A clone ticks without allocating too, into the same buffer.
    let mut c = w.clone();
    let info = allocation_counter::measure(|| {
        for i in 0..100 {
            c.tick(if i % 3 == 0 { &a } else { &b }, &mut out);
        }
    });
    assert_eq!(info.count_total, 0, "a cloned world's tick allocated");

    let dirt = w.worms()[0].dirt_count;
    let fire = [Keys(Keys::FIRE), Keys(Keys::FIRE)];
    let info = allocation_counter::measure(|| {
        for _ in 0..301 {
            w.tick(&fire, &mut out);
        }
    });
    assert_eq!(info.count_total, 0, "beams and specials allocated");
    assert_eq!(w.worms()[0].slots[0].weapon, 2, "the mode gave the beam");
    assert_ne!(w.worms()[0].dirt_count, dirt, "the beam carved");
}

#[test]
fn give_weapon_mid_match_does_not_allocate() {
    // The chaos bot's use (plan 6 F10): a weapon on every respawn, here also
    // one every tick, and a worm spawned at tick 50; also in a clone.
    let mut w = world(Giver::new(vec![], true));
    let keys = [Keys(Keys::FIRE | Keys::LEFT), Keys(Keys::FIRE | Keys::RIGHT)];
    let mut out = TickEvents::new();
    let mut c = w.clone();
    let info = allocation_counter::measure(|| {
        for _ in 0..400 {
            w.tick(&keys, &mut out);
            c.tick(&keys, &mut out);
        }
    });
    assert_eq!(info.count_total, 0, "mode commands allocated");
    assert_eq!(w.checksum(), c.checksum());
    assert_eq!(w.worms().len(), 3, "a worm spawned mid-match");
    assert!(w.mt_calls() > 0 && w.worms()[1].alive, "worm 1 respawned");
    // The last tick's give: worm 400 % 2, slot 400 % 5, weapon 400 % 4.
    assert_eq!(w.worms()[0].slots[0].weapon, 0);
    assert_eq!(w.worms()[1].slots[4].weapon, 3, "tick 399's give");
}

/// Issue #86: a world with live copies ticks, and is cloned and ticks,
/// without allocating.
#[test]
fn a_live_world_ticks_without_allocating() {
    let mut w = synthetic::world_live(8, 400, 300, live_level::live_level());
    w.set_live_state(0, 1).unwrap();
    let keys: Vec<Vec<Keys>> = (0..300u32)
        .map(|t| {
            (0..8u32)
                .map(|i| Keys(if (t + i) % 5 < 3 { Keys::FIRE | Keys::LEFT } else { Keys::UP | Keys::JUMP }))
                .collect()
        })
        .collect();
    let mut out = TickEvents::new();
    let info = allocation_counter::measure(|| {
        for k in &keys[..200] {
            w.tick(k, &mut out);
        }
    });
    assert_eq!(info.count_total, 0, "a live world's tick allocated");
    assert_eq!(w.live().unwrap().copy(0).unwrap().state_name(), "Alarm");
    let mut c = w.clone();
    let info = allocation_counter::measure(|| {
        for k in &keys[200..] {
            c.tick(k, &mut out);
        }
    });
    assert_eq!(info.count_total, 0, "a cloned live world's tick allocated");
}
