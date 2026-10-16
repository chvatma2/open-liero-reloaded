//! Unit tests of [`crate::SimMod`], and mod builders other tests use.
use super::*;
use alloc::string::ToString;
use alloc::vec;
use olr_data::image::Rgba;
use olr_data::mod_def::{
    ActionDef, ActionKind, RopeParams, TimerDef, TrailDef, WeaponClass, WeaponDef, WormParams,
};

pub(crate) fn pixel_projectile(spawn: Option<SpawnDef>) -> ProjectileDef {
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
        event_spawn: spawn,
    }
}

pub(crate) fn weapon_spawn(projectile: &str) -> SpawnDef {
    SpawnDef {
        amount: 1,
        speed: 250,
        spread: 3.0,
        use_angle: true,
        use_random_rot: true,
        add_parent_vel: true,
        ..SpawnDef::olx_default(projectile.to_string())
    }
}

pub(crate) fn projectile_weapon(id: &str, projectile: &str) -> WeaponDef {
    WeaponDef {
        id: id.to_string(),
        name: id.to_string(),
        recharge: 0.14,
        drain: 1.43,
        rof: 0.015,
        laser_sight: false,
        kind: WeaponKind::Projectile {
            class: WeaponClass::Automatic,
            recoil: 6,
            sound: None,
            spawn: weapon_spawn(projectile),
        },
    }
}

pub(crate) fn mod_def(weapons: Vec<WeaponDef>, projectiles: Vec<(&str, ProjectileDef)>) -> ModDef {
    ModDef {
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
        weapons,
        projectiles: projectiles.into_iter().map(|(n, p)| (n.to_string(), p)).collect(),
        restrictions: Default::default(),
    }
}

/// A mod with one Minigun-like weapon.
pub(crate) fn test_mod() -> alloc::sync::Arc<SimMod> {
    let def =
        mod_def(vec![projectile_weapon("minigun", "minigun")], vec![("minigun", pixel_projectile(None))]);
    alloc::sync::Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap())
}

fn spawning(child: &str, amount: i32) -> ProjectileDef {
    pixel_projectile(Some(SpawnDef { amount, ..SpawnDef::olx_default(child.to_string()) }))
}

#[test]
fn shared_definitions_expand_in_olx_load_order() {
    // Two weapons share "shell"; its children are named per occurrence.
    let mut shell = spawning("bits", 5);
    shell.trail = TrailDef {
        kind: TrailKind::Projectile,
        delay: 0.1,
        spawn: Some(SpawnDef::olx_default("smoke".into())),
    };
    let def = mod_def(
        vec![projectile_weapon("a", "shell"), projectile_weapon("b", "shell")],
        vec![("shell", shell), ("bits", pixel_projectile(None)), ("smoke", pixel_projectile(None))],
    );
    let m = SimMod::compile(&def, &BTreeMap::new()).unwrap();
    let names: Vec<_> = m.projectiles().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["a", "a.spawn", "a.trail", "b", "b.spawn", "b.trail"]);
    assert_eq!(m.weapons()[1].spawn.as_ref().unwrap().proj, 3);
    assert_eq!(m.projectiles()[3].event_spawn.as_ref().unwrap().proj, 4);
    assert_eq!(m.projectile_index("b.trail"), Some(5));
}

#[test]
fn compiling_rejects_missing_and_bounds_cycles() {
    // A missing reference is an error.
    let def = mod_def(vec![projectile_weapon("gun", "nope")], vec![]);
    assert_eq!(
        SimMod::compile(&def, &BTreeMap::new()),
        Err(SimModError::MissingProjectile { name: "nope".into(), by: "gun".into() })
    );
    let def = mod_def(vec![projectile_weapon("gun", "a")], vec![("a", spawning("gone", 1))]);
    assert!(matches!(
        SimMod::compile(&def, &BTreeMap::new()),
        Err(SimModError::MissingProjectile { by, .. }) if by == "gun"
    ));

    // A projectile that spawns itself 1000 times compiles: the child is the
    // parent (OLX spawns lazily), and the pool bounds it at run time.
    let def = mod_def(vec![projectile_weapon("gun", "self")], vec![("self", spawning("self", 1000))]);
    let m = SimMod::compile(&def, &BTreeMap::new()).unwrap();
    assert_eq!(m.projectiles().len(), 1);
    assert_eq!(m.projectiles()[0].event_spawn.as_ref().unwrap().proj, 0);

    // A cycle through two definitions closes at the first repeat.
    let def =
        mod_def(vec![projectile_weapon("gun", "a")], vec![("a", spawning("b", 2)), ("b", spawning("a", 2))]);
    let m = SimMod::compile(&def, &BTreeMap::new()).unwrap();
    assert_eq!(m.projectiles().len(), 2);
    assert_eq!(m.projectiles()[1].event_spawn.as_ref().unwrap().proj, 0);

    // Sharing that doubles at every level (2^40 occurrences) stops at the
    // occurrence limit instead of hanging.
    let mut projs = Vec::new();
    let names: Vec<String> = (0..=40).map(|i| format!("p{i}")).collect();
    for i in 0..40 {
        let mut p = spawning(&names[i + 1], 1);
        p.trail = TrailDef {
            kind: TrailKind::Projectile,
            delay: 0.1,
            spawn: Some(SpawnDef::olx_default(names[i + 1].clone())),
        };
        projs.push((names[i].as_str(), p));
    }
    projs.push((names[40].as_str(), pixel_projectile(None)));
    let def = mod_def(vec![projectile_weapon("gun", "p0")], projs);
    assert_eq!(SimMod::compile(&def, &BTreeMap::new()), Err(SimModError::TooManyProjectiles));

    // A chain deeper than OLX's guard.
    let names: Vec<String> = (0..=200).map(|i| format!("q{i}")).collect();
    let mut projs: Vec<(&str, ProjectileDef)> =
        (0..200).map(|i| (names[i].as_str(), spawning(&names[i + 1], 1))).collect();
    projs.push((names[200].as_str(), pixel_projectile(None)));
    let def = mod_def(vec![projectile_weapon("gun", "q0")], projs);
    assert!(matches!(SimMod::compile(&def, &BTreeMap::new()), Err(SimModError::TooDeep { .. })));

    // An absurd amount.
    let def = mod_def(vec![projectile_weapon("gun", "a")], vec![("a", spawning("a", 1_000_000))]);
    assert_eq!(
        SimMod::compile(&def, &BTreeMap::new()),
        Err(SimModError::AbsurdAmount { path: "gun".into(), amount: 1_000_000 })
    );
    // The limit covers real mods (at most 150) with a wide margin.
    let def = mod_def(vec![projectile_weapon("gun", "a")], vec![("a", spawning("a", MAX_AMOUNT + 1))]);
    assert!(matches!(SimMod::compile(&def, &BTreeMap::new()), Err(SimModError::AbsurdAmount { .. })));
}

#[test]
fn image_frame_counts_come_from_the_caller() {
    let mut p = pixel_projectile(None);
    p.gfx = ProjectileGfx::Image {
        file: "gfx/flame.png".into(),
        rotating: false,
        rot_increment: 0,
        rot_speed: 0,
        use_angle: false,
        use_spec_angle: false,
        angle_images: 0,
        animation: None,
    };
    let def = mod_def(vec![projectile_weapon("gun", "flame")], vec![("flame", p)]);
    let counts = [("gfx/flame.png".to_string(), 7)].into_iter().collect();
    assert_eq!(SimMod::compile(&def, &counts).unwrap().projectiles()[0].frames, Some(7));
    assert_eq!(SimMod::compile(&def, &BTreeMap::new()).unwrap().projectiles()[0].frames, None);
}
