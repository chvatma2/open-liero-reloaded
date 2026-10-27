//! A world made here for the runtime's tests: a box of rock with two
//! worms, a mod of one harmless weapon, and live copies with scripts.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use olr_data::image::Rgba;
use olr_data::level::{Material, MaterialGrid};
use olr_data::mod_def::*;
use olr_sim::{
    EventKind, Keys, LiveCopy, LiveCopySetup, LiveLevel, LivePart, LivePartKind, LiveProp, LiveSolid,
    LiveState, LiveTarget, Loadout, MatchSetup, OracleMode, SimMod, SimSettings, Slot, TickEvents, World,
    WormSetup,
};
use olr_terrain::Terrain;

fn sim_mod() -> Arc<SimMod> {
    let pellet = ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(208, 215, 208)] },
        gravity: Some(30),
        dampening: 1.0,
        width: 2,
        height: 2,
        timer: TimerDef { time: 1.0, time_var: 0.0, action: None },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef::olx_default(ActionKind::Bounce),
        player_hit: ActionDef::olx_default(ActionKind::Bounce),
        event_spawn: None,
    };
    let gun = WeaponDef {
        id: "gun".into(),
        name: "gun".into(),
        recharge: 1.0,
        drain: 0.0,
        rof: 0.1,
        laser_sight: false,
        kind: WeaponKind::Projectile {
            class: WeaponClass::Automatic,
            recoil: 0,
            sound: None,
            spawn: SpawnDef { amount: 1, speed: 200, ..SpawnDef::olx_default("pellet".into()) },
        },
    };
    let def = ModDef {
        name: "script-test".into(),
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
        weapons: vec![gun],
        projectiles: [("pellet".to_string(), pellet)].into_iter().collect(),
        restrictions: Default::default(),
        loadouts: Default::default(),
    };
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).expect("the test mod compiles"))
}

/// A copy `id` at `x` with states `states` (each a background box:
/// scenery, which moves through rock and other copies) and one exposed
/// value `v` (from -10^9 to 10^9, default `-1`) that no state sets,
/// running scripts `scripts`.
pub fn copy(id: &str, x: i32, states: &[&str], scripts: Vec<u16>) -> LiveCopy {
    copy_of(LivePartKind::Background, id, x, states, scripts)
}

/// [`copy`] of rock (issue #90: solid, stopped by rock).
pub fn rock_copy(id: &str, x: i32, states: &[&str], scripts: Vec<u16>) -> LiveCopy {
    copy_of(LivePartKind::Rock, id, x, states, scripts)
}

fn copy_of(kind: LivePartKind, id: &str, x: i32, states: &[&str], scripts: Vec<u16>) -> LiveCopy {
    let parts = vec![LivePart { kind, layer: None, at: (0, 0), picture: 0, glow_strength: 100 }];
    LiveCopy::new(LiveCopySetup {
        id: id.into(),
        at: (x, 40),
        size: (4, 4),
        props: vec![
            LiveProp {
                name: Some("v".into()),
                target: LiveTarget::GlowStrength(0),
                min: -1_000_000_000,
                max: 1_000_000_000,
                default: -1,
            },
            LiveProp { name: Some("x".into()), target: LiveTarget::X, min: -50, max: 50, default: 0 },
        ],
        states: states
            .iter()
            .map(|s| LiveState {
                name: (*s).into(),
                values: Vec::new(),
                parts: parts.clone(),
                enter_sound: None,
                loop_sound: None,
            })
            .collect(),
        pictures: vec![LiveSolid::new((4, 4), kind == LivePartKind::Rock, |_, _| true)],
        start_state: 0,
        start_values: Vec::new(),
        health: None,
        scripts,
        sounds: Vec::new(),
    })
}

/// A level of `copies` and `scripts`, the level script `level_script`.
pub fn level(copies: Vec<LiveCopy>, scripts: &[&str], level_script: Option<u16>) -> Arc<LiveLevel> {
    Arc::new(LiveLevel::new(copies, scripts.iter().map(|s| (*s).to_string()).collect(), level_script, 0))
}

/// The setup of a 200 × 120 box with two worms on `level`, its director
/// compiled; `scripts` false leaves the director out.
pub fn setup(level: Arc<LiveLevel>, seed: u32, scripts: bool) -> MatchSetup {
    let (w, h) = (200u32, 120u32);
    let cells = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if x < 4 || x >= w - 4 || y < 4 || y >= h - 4 { Material::Rock } else { Material::Background }
        })
        .collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).expect("a grid"));
    let settings = SimSettings::lx56();
    let loadout = Loadout::new(vec![Slot::ready(0); 5], &settings);
    let worms = [(40, 100), (160, 100)]
        .into_iter()
        .map(|at| WormSetup { team: 0, loadout: loadout.clone(), spawn: Some(at) })
        .collect();
    let director = if scripts { olr_script::director(&level).expect("the scripts compile") } else { None };
    MatchSetup { seed, worms, live: Some(level), director, ..MatchSetup::new(settings, sim_mod(), terrain) }
}

/// The world of [`setup`] with its scripts.
pub fn world(level: Arc<LiveLevel>) -> World {
    World::new(setup(level, 7, true), Box::new(OracleMode)).expect("the world")
}

/// Worm 0 walks and fires now and then (a few events a tick).
pub fn keys(t: u32) -> Vec<Keys> {
    let k = if t % 50 < 25 { Keys::RIGHT } else { Keys::LEFT };
    let fire = if t.is_multiple_of(40) { Keys::FIRE } else { 0 };
    vec![Keys(k | fire), Keys(0)]
}

/// Ticks `w` `n` times; gives the tick's script errors (instances) of
/// each.
pub fn play(w: &mut World, n: u32) -> Vec<Vec<u16>> {
    let mut ev = TickEvents::new();
    (0..n)
        .map(|_| {
            let t = w.server_frame() as u32;
            w.tick(&keys(t), &mut ev);
            ev.iter()
                .filter_map(|e| match e.kind {
                    EventKind::ScriptError { instance } => Some(instance),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// Copy `id`'s state name and value `v`.
pub fn read(w: &World, id: &str) -> (String, i64) {
    let c = w.live().unwrap().copy_by_id(id).unwrap();
    (c.state_name().to_string(), c.value("v").unwrap())
}
