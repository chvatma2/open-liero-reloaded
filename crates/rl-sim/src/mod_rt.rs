//! The mod as the simulation uses it: [`rl_data::ModDef`] compiled to indices.
//!
//! OLX loads an LX56 (`script.lgs`) mod as a tree: each weapon owns its
//! projectile, which owns its spawned and trail projectiles, and every
//! occurrence is a separate `proj_t` numbered in load order
//! (`CGameScript::LoadProjectile`, `src/common/CGameScript.cpp:833-1157`:
//! the projectile itself, then its `GeneralSpawnInfo` child, then its trail
//! child). A pack shares identical definitions by name, so [`SimMod::compile`]
//! expands them back into OLX's per-occurrence tree. Indices and path names
//! (`<weapon id>`, then `.spawn` or `.trail` per level) are then OLX's, and
//! the oracle's projectile types compare directly.
//!
//! Packs are untrusted: a missing reference, a tree that is too deep or too
//! large, or an absurd spawn amount is a [`SimModError`]. A projectile that
//! spawns one of its ancestors (a cycle, which OLX's text mods can express)
//! is allowed: the child refers back to the ancestor's index, and the
//! projectile pool bounds what it spawns at run time.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use rl_data::mod_def::{
    BeamDef, ModDef, ProjectileDef, ProjectileGfx, SpawnDef, SpecialKind, TrailKind, WeaponKind,
};

/// At most this many projectile occurrences after expansion. Classic has 69
/// and MW 1.0 398; a shared definition reached on exponentially many paths
/// stops here instead of exhausting memory.
pub const MAX_PROJECTILES: usize = 4096;
/// OLX's own nesting guard (`CGameScript::LoadProjectile`: `depth > 128`).
pub const MAX_DEPTH: usize = 128;
/// The largest `Amount` a spawn may have. Real mods use at most 150; a huge
/// amount would make one spawn event loop for minutes.
pub const MAX_AMOUNT: i32 = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimModError {
    /// `by` (a weapon id or a projectile path) names a projectile the mod lacks.
    MissingProjectile { name: String, by: String },
    /// A trail of kind `Projectile` without its spawn information.
    MissingTrailSpawn { path: String },
    /// More than [`MAX_PROJECTILES`] projectile occurrences.
    TooManyProjectiles,
    /// Nested deeper than [`MAX_DEPTH`].
    TooDeep { path: String },
    /// A spawn amount above [`MAX_AMOUNT`].
    AbsurdAmount { path: String, amount: i32 },
}

impl fmt::Display for SimModError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProjectile { name, by } => write!(f, "{by} spawns missing projectile {name:?}"),
            Self::MissingTrailSpawn { path } => write!(f, "{path} has a projectile trail without spawn info"),
            Self::TooManyProjectiles => write!(f, "more than {MAX_PROJECTILES} projectiles after expansion"),
            Self::TooDeep { path } => write!(f, "{path} is nested deeper than {MAX_DEPTH}"),
            Self::AbsurdAmount { path, amount } => {
                write!(f, "{path} spawns {amount} projectiles at once (at most {MAX_AMOUNT})")
            }
        }
    }
}

/// OLX `Wpn_Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponType {
    Projectile,
    Beam,
    Special,
}

/// OLX `Proj_SpawnInfo` with its projectile as an index into [`SimMod::projectiles`].
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnInfo {
    pub proj: usize,
    pub amount: i32,
    pub speed: i32,
    pub speed_var: f32,
    pub spread: f32,
    pub use_angle: bool,
    pub angle: i32,
    pub use_parent_velocity_for_spread: bool,
    pub parent_vel_spread_factor: f32,
    pub use_special11_vec_for_speed_var: bool,
    pub use_random_rot: bool,
    pub add_parent_vel: bool,
    /// `MatrixD2<float>(f)`: `f` on the diagonal.
    pub parent_vel_factor: f32,
}

impl SpawnInfo {
    fn new(s: &SpawnDef, proj: usize, path: &str) -> Result<Self, SimModError> {
        if s.amount > MAX_AMOUNT {
            return Err(SimModError::AbsurdAmount { path: path.into(), amount: s.amount });
        }
        Ok(Self {
            proj,
            amount: s.amount,
            speed: s.speed,
            speed_var: s.speed_var,
            spread: s.spread,
            use_angle: s.use_angle,
            angle: s.angle,
            use_parent_velocity_for_spread: s.use_parent_velocity_for_spread,
            parent_vel_spread_factor: s.parent_vel_spread_factor,
            use_special11_vec_for_speed_var: s.use_special11_vec_for_speed_var,
            use_random_rot: s.use_random_rot,
            add_parent_vel: s.add_parent_vel,
            parent_vel_factor: s.parent_vel_factor,
        })
    }
}

/// OLX `weapon_t`; its index in [`SimMod::weapons`] is OLX's weapon id.
#[derive(Clone, Debug, PartialEq)]
pub struct SimWeapon {
    /// The pack's weapon id (OLX's name, slugged and made unique).
    pub id: String,
    pub name: String,
    pub kind: WeaponType,
    pub recoil: i32,
    pub recharge: f32,
    pub drain: f32,
    pub rof: f32,
    pub laser_sight: bool,
    /// `Proj`, for projectile weapons.
    pub spawn: Option<SpawnInfo>,
    pub beam: Option<BeamDef>,
    /// `Special`, for special weapons (else `SPC_NONE`).
    pub special: SpecialKind,
    /// `tSpecial.Thrust` (an `int` in OLX).
    pub thrust: i32,
}

/// One OLX `proj_t` occurrence.
#[derive(Clone, Debug, PartialEq)]
pub struct SimProjectile {
    /// The path name, such as `grenade.spawn` (the oracle header's name).
    pub name: String,
    pub def: ProjectileDef,
    /// The image's frame count (`width / height`), for image projectiles
    /// whose image the caller found (spec §18 D3).
    pub frames: Option<u32>,
    /// `GeneralSpawnInfo`, what `spawns_projectiles` actions spawn.
    pub event_spawn: Option<SpawnInfo>,
    /// The `TRL_PROJECTILE` trail's spawn information.
    pub trail_spawn: Option<SpawnInfo>,
}

impl SimProjectile {
    /// `proj_t::Rotating` and `RotIncrement` (image projectiles only).
    pub fn rotation(&self) -> Option<i32> {
        match &self.def.gfx {
            ProjectileGfx::Image { rotating: true, rot_increment, .. } => Some(*rot_increment),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimMod {
    weapons: Vec<SimWeapon>,
    projectiles: Vec<SimProjectile>,
}

impl SimMod {
    /// Compiles `def`. `frame_counts` maps an image's pack path (such as
    /// `gfx/flame.png`) to its frame count, `width / height`.
    pub fn compile(def: &ModDef, frame_counts: &BTreeMap<String, u32>) -> Result<SimMod, SimModError> {
        let mut b = Builder { def, frame_counts, projectiles: Vec::new(), stack: Vec::new() };
        let mut weapons = Vec::with_capacity(def.weapons.len());
        for w in &def.weapons {
            let (mut special, mut thrust) = (SpecialKind::None, 0);
            let (kind, recoil, spawn, beam) = match &w.kind {
                WeaponKind::Projectile { recoil, spawn, .. } => {
                    let proj = b.expand(&spawn.projectile, &w.id, w.id.clone())?;
                    (WeaponType::Projectile, *recoil, Some(SpawnInfo::new(spawn, proj, &w.id)?), None)
                }
                WeaponKind::Beam { recoil, beam } => (WeaponType::Beam, *recoil, None, Some(*beam)),
                WeaponKind::Special { special: sp, thrust: t } => {
                    (special, thrust) = (*sp, *t as i32);
                    (WeaponType::Special, 0, None, None)
                }
            };
            weapons.push(SimWeapon {
                id: w.id.clone(),
                name: w.name.clone(),
                kind,
                recoil,
                recharge: w.recharge,
                drain: w.drain,
                rof: w.rof,
                laser_sight: w.laser_sight,
                spawn,
                beam,
                special,
                thrust,
            });
        }
        Ok(SimMod { weapons, projectiles: b.projectiles })
    }

    pub fn weapons(&self) -> &[SimWeapon] {
        &self.weapons
    }

    /// `wpnslot_t::weapon()` (`src/common/CWorm.cpp:127-132`): `None` for an
    /// id outside the weapon list (such as -1, a disabled slot).
    pub fn weapon(&self, id: i32) -> Option<&SimWeapon> {
        usize::try_from(id).ok().and_then(|i| self.weapons.get(i))
    }

    /// Projectiles in OLX's load order.
    pub fn projectiles(&self) -> &[SimProjectile] {
        &self.projectiles
    }

    /// The index of the projectile with path name `name`.
    pub fn projectile_index(&self, name: &str) -> Option<usize> {
        self.projectiles.iter().position(|p| p.name == name)
    }
}

struct Builder<'a> {
    def: &'a ModDef,
    frame_counts: &'a BTreeMap<String, u32>,
    projectiles: Vec<SimProjectile>,
    /// The definitions on the current path, with their indices (for cycles).
    stack: Vec<(&'a str, usize)>,
}

impl<'a> Builder<'a> {
    /// Adds `name` as the occurrence `path` and its children, depth first in
    /// OLX's load order, and returns its index.
    fn expand(&mut self, name: &'a str, by: &str, path: String) -> Result<usize, SimModError> {
        let Some((key, pdef)) = self.def.projectiles.get_key_value(name) else {
            return Err(SimModError::MissingProjectile { name: name.into(), by: by.into() });
        };
        if let Some(&(_, ancestor)) = self.stack.iter().find(|(n, _)| *n == name) {
            return Ok(ancestor);
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(SimModError::TooDeep { path });
        }
        if self.projectiles.len() >= MAX_PROJECTILES {
            return Err(SimModError::TooManyProjectiles);
        }
        let idx = self.projectiles.len();
        let frames = match &pdef.gfx {
            ProjectileGfx::Image { file, .. } => self.frame_counts.get(file).copied(),
            ProjectileGfx::Pixel { .. } => None,
        };
        self.projectiles.push(SimProjectile {
            name: path.clone(),
            def: pdef.clone(),
            frames,
            event_spawn: None,
            trail_spawn: None,
        });
        self.stack.push((key.as_str(), idx));
        if let Some(s) = &pdef.event_spawn {
            let child = self.expand(&s.projectile, &path, format!("{path}.spawn"))?;
            self.projectiles[idx].event_spawn = Some(SpawnInfo::new(s, child, &path)?);
        }
        if pdef.trail.kind == TrailKind::Projectile {
            let Some(s) = &pdef.trail.spawn else {
                return Err(SimModError::MissingTrailSpawn { path });
            };
            let child = self.expand(&s.projectile, &path, format!("{path}.trail"))?;
            self.projectiles[idx].trail_spawn = Some(SpawnInfo::new(s, child, &path)?);
        }
        self.stack.pop();
        Ok(idx)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;
    use rl_data::image::Rgba;
    use rl_data::mod_def::{
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
        let def = mod_def(
            vec![projectile_weapon("gun", "a")],
            vec![("a", spawning("b", 2)), ("b", spawning("a", 2))],
        );
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
}
