//! `AI_MoveToTarget`'s budget order, dig-on and OLX's `SIGN`, on hand-made
//! worlds.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::image::Rgba;
use olr_data::level::{Material, MaterialGrid};
use olr_data::mod_def::*;
use olr_math::Vec2;
use olr_sim::{Loadout, MatchSetup, OracleMode, SimMod, SimSettings, Slot, World, WormSetup};
use olr_terrain::Terrain;

use super::sign;
use crate::BotConfig;
use crate::bot::{Act, Bot, Ctx};
use crate::brain::{State, Target};
use crate::budget::Budget;

/// A mod with one lobbed weapon (gravity 100, speed 130).
fn lob_mod() -> Arc<SimMod> {
    let shell = ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(255, 255, 255)] },
        gravity: None,
        dampening: 1.0,
        width: 4,
        height: 4,
        timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef::olx_default(ActionKind::Explode),
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
                speed: 130,
                use_angle: true,
                ..SpawnDef::olx_default("shell".into())
            },
        },
    };
    let def = ModDef {
        name: "moving".into(),
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
        weapons: vec![lob],
        projectiles: [("shell".to_string(), shell)].into_iter().collect(),
        restrictions: Default::default(),
    };
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap())
}

/// A 300 × 200 world with `at(x, y)`'s materials and worms at `spawns`.
fn world(at: impl Fn(i32, i32) -> Material, spawns: &[(i32, i32)]) -> World {
    let cells = (0..300 * 200).map(|i| at(i % 300, i / 300)).collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(300, 200, cells).unwrap());
    let s = SimSettings::lx56();
    let worms = spawns
        .iter()
        .map(|&p| WormSetup { team: 0, loadout: Loadout::new(vec![Slot::ready(0); 5], &s), spawn: Some(p) })
        .collect();
    let setup = MatchSetup { worms, ..MatchSetup::new(s, lob_mod(), terrain) };
    World::new(setup, Box::new(OracleMode)).unwrap()
}

/// A bot for worm 0 heading for worm 1 along `path`.
fn moving_bot(w: &World, path: &[Vec2]) -> Bot {
    let mut bot = Bot::new(0, BotConfig::default(), 1, w);
    bot.mind.state = State::Moving;
    bot.mind.target = Target::Worm(1);
    bot.follow.set(path);
    bot
}

fn ctx(w: &World) -> Ctx<'_> {
    Ctx { w, me: &w.worms()[0], now: w.time(), s: w.settings(), t: w.terrain() }
}

/// Review fix 1: a tick that starts a shooting-spot search still runs the
/// jump check. Worm 1 sits in a rock bubble (radius 22-28), so every point of
/// the spot's circle is in the air but out of sight: the search costs about
/// 230 units a point.
#[test]
fn a_spot_search_tick_still_checks_the_jump() {
    let bubble = |x: i32, y: i32| {
        let d2 = (x - 200) * (x - 200) + (y - 100) * (y - 100);
        if (22 * 22..=28 * 28).contains(&d2) { Material::Rock } else { Material::Background }
    };
    let w = world(bubble, &[(50, 150), (200, 100)]);
    for limit in [800, 1200, 1500, 2000] {
        let mut bot = moving_bot(&w, &[Vec2::new(50.0, 150.0), Vec2::new(150.0, 60.0)]);
        let c = ctx(&w);
        let mut b = Budget::new(limit);
        bot.move_to_target(&c, &mut b, &mut Act::default());
        let check = bot.mind.jump_check.map(|(t, _, answer)| (t == c.now, answer));
        assert!(matches!(check, Some((true, Some(_)))), "budget {limit}: the jump check ran ({check:?})");
        assert!(b.used() <= limit);
    }
}

/// Review fix 2: dig-on only digs through dirt. Worm 1 stands behind a wall
/// 10 px from the path's end: rock, then dirt.
#[test]
fn dig_on_only_digs_through_dirt() {
    for (wall, pushed) in [(Material::Rock, false), (Material::Dirt, true)] {
        let at = |x: i32, _| if (100..=104).contains(&x) { wall } else { Material::Background };
        let w = world(at, &[(80, 100), (125, 100)]);
        let end = Vec2::new(80.0, 100.0);
        let mut bot = moving_bot(&w, &[end]);
        let c = ctx(&w);
        bot.dig_on(&c, &mut Budget::new(5000), end);
        let nodes: Vec<Vec2> = bot.follow.nodes().to_vec();
        assert_eq!(nodes.len() == 2, pushed, "{wall:?}: {nodes:?}");
    }
}

/// Review fix 3: OLX's `SIGN` (`include/MathLib.h:60`) is 0 for 0.
#[test]
fn sign_is_olx_sign() {
    assert_eq!((sign(3.0), sign(-0.5), sign(0.0), sign(-0.0), sign(f32::NAN)), (1, -1, 0, 0, 0));
}
