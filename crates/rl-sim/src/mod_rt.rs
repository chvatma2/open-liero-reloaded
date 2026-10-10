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
    ActionDef, BeamDef, ModDef, ProjectileDef, ProjectileGfx, SpawnDef, SpecialKind, TrailKind, WeaponKind,
};

use crate::events::{GAME_SOUND_PREFIX, GAME_SOUNDS, SoundId};

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
    /// `smpSample`, if `UseSound`.
    pub sound: Option<SoundId>,
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
    /// The `Sound` of the `Hit`, `PlayerHit` and `Timer` actions.
    pub hit_sound: Option<SoundId>,
    pub player_hit_sound: Option<SoundId>,
    pub timer_sound: Option<SoundId>,
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
    sounds: Vec<String>,
}

impl SimMod {
    /// Compiles `def`. `frame_counts` maps an image's pack path (such as
    /// `gfx/flame.png`) to its frame count, `width / height`.
    pub fn compile(def: &ModDef, frame_counts: &BTreeMap<String, u32>) -> Result<SimMod, SimModError> {
        let sounds = GAME_SOUNDS.iter().map(|s| String::from(*s)).collect();
        let mut b = Builder { def, frame_counts, projectiles: Vec::new(), stack: Vec::new(), sounds };
        let mut weapons = Vec::with_capacity(def.weapons.len());
        for w in &def.weapons {
            let (mut special, mut thrust) = (SpecialKind::None, 0);
            let sound = b.sound(w.sound());
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
                sound,
            });
        }
        Ok(SimMod { weapons, projectiles: b.projectiles, sounds: b.sounds })
    }

    /// The sounds events name ([`SoundId`] indexes this): OLX's
    /// [`GAME_SOUNDS`] (prefixed [`GAME_SOUND_PREFIX`], paths in OLX's data
    /// directory), then the mod's sound paths (in its pack, never prefixed) in first use (each weapon,
    /// then its projectiles' `Hit`, `PlayerHit` and `Timer` actions, in
    /// projectile load order).
    pub fn sounds(&self) -> &[String] {
        &self.sounds
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
    sounds: Vec<String>,
}

impl<'a> Builder<'a> {
    /// The id of sound `name`, added if new. A table already holding every
    /// `u16` gets no more sounds. A name with [`GAME_SOUND_PREFIX`] is not a
    /// mod sound and gets none.
    fn sound(&mut self, name: Option<&str>) -> Option<SoundId> {
        let name = name.filter(|n| !n.starts_with(GAME_SOUND_PREFIX))?;
        let i = match self.sounds.iter().position(|s| s == name) {
            Some(i) => i,
            None => {
                self.sounds.push(name.into());
                self.sounds.len() - 1
            }
        };
        u16::try_from(i).ok().map(SoundId)
    }

    fn action_sound(&mut self, a: Option<&ActionDef>) -> Option<SoundId> {
        self.sound(a.and_then(|a| a.sound.as_deref()))
    }

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
        let hit_sound = self.action_sound(Some(&pdef.hit));
        let player_hit_sound = self.action_sound(Some(&pdef.player_hit));
        let timer_sound = self.action_sound(pdef.timer.action.as_ref());
        self.projectiles.push(SimProjectile {
            name: path.clone(),
            def: pdef.clone(),
            frames,
            event_spawn: None,
            trail_spawn: None,
            hit_sound,
            player_hit_sound,
            timer_sound,
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
#[path = "mod_rt_tests.rs"]
pub(crate) mod tests;
