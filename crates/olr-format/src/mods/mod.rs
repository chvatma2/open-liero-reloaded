//! The mod authoring format (spec §2): `mod.ron`, `weapons/<id>.ron`,
//! `projectiles/<id>.ron` (shared projectiles) and `restrictions.ron`.

use std::collections::BTreeMap;

use olr_data::restrictions::Preset;
use serde::{Deserialize, Serialize};

use crate::defaults as d;

mod compile;
mod files;
pub(crate) use files::ModFile;
mod lift;
mod spawn;
mod types;

pub use compile::{
    MAX_AMOUNT, MAX_COLOURS, MAX_EFFECTS, MAX_INLINE_DEPTH, MAX_SHARED, MAX_WEAPONS, compile_mod,
};
pub use files::{RESTRICTIONS_FILE, read_mod_files, write_mod_files};
pub use lift::{LIFTED_SPAWN, lift_mod};
pub use spawn::{
    EventContext, EventSpawn, ShotContext, ShotSpawn, Spawn, SpawnContext, TrailContext, TrailSpawn,
};
pub use types::*;

/// A mod as its authoring files hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModSource {
    /// Shown in menus; bot loadouts and weapon restrictions match on it
    /// (OLX lgs header `ModName`).
    pub name: String,
    /// Free text for authors; ignored by the game.
    pub notes: String,
    /// Worm physics (`mod-fields.md` §3).
    pub worm: Worm,
    /// Ninja rope (`mod-fields.md` §4).
    pub rope: Rope,
    /// In order: a weapon's index is its id in the game.
    pub weapons: Vec<(String, Weapon)>,
    /// `projectiles/<id>.ron`: projectiles several places use, by id.
    pub shared: BTreeMap<String, Projectile>,
    /// `restrictions.ron`: named weapon-restriction presets (`WeaponRules`).
    pub restrictions: BTreeMap<String, Preset>,
}

/// Worm physics of the mod (OLX `gs_worm_t`, `CGameScript.cpp:45-55`).
/// Defaults are OLX's FeatureList values (`FeatureList.cpp:67-71`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worm {
    /// × 90 px/s² while walking on the ground, until the max ground speed
    /// (OLX `ground_speed`; FeatureList `WormGroundSpeed`).
    #[serde(default = "d::worm_ground_acceleration", skip_serializing_if = "d::is_worm_ground_acceleration")]
    pub ground_acceleration: f32,
    /// × 90 px/s² while moving in the air, up to the max air speed
    /// (OLX `air_speed`; FeatureList `WormAirSpeed`).
    #[serde(default = "d::worm_air_acceleration", skip_serializing_if = "d::is_worm_air_acceleration")]
    pub air_acceleration: f32,
    /// px/s² added to the vertical velocity (OLX `gravity`; FeatureList `WormGravity`).
    #[serde(default = "d::worm_gravity", skip_serializing_if = "d::is_worm_gravity")]
    pub gravity: f32,
    /// px/s, negative = up: a ground jump sets the vertical speed to it
    /// (OLX `jump_force`; FeatureList `WormJumpForce`).
    #[serde(default = "d::worm_jump_speed", skip_serializing_if = "d::is_worm_jump_speed")]
    pub jump_speed: f32,
    /// Quadratic drag in the air, 1/px (OLX `air_friction`; FeatureList `WormAirFriction`).
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub air_drag: f32,
    /// Unit: none (would be deg/s). Ignored by OLX ("was always ignored",
    /// `CGameScript.cpp:45`); kept for losslessness.
    #[serde(default = "d::worm_unused_angle_speed", skip_serializing_if = "d::is_worm_unused_angle_speed")]
    pub unused_angle_speed: f32,
    /// Unit: none. Ignored by OLX (`CGameScript.cpp:53`); kept for losslessness.
    #[serde(
        default = "d::worm_unused_ground_friction",
        skip_serializing_if = "d::is_worm_unused_ground_friction"
    )]
    pub unused_ground_friction: f32,
}

impl Default for Worm {
    fn default() -> Self {
        Self {
            ground_acceleration: d::worm_ground_acceleration(),
            air_acceleration: d::worm_air_acceleration(),
            gravity: d::worm_gravity(),
            jump_speed: d::worm_jump_speed(),
            air_drag: 0.0,
            unused_angle_speed: d::worm_unused_angle_speed(),
            unused_ground_friction: d::worm_unused_ground_friction(),
        }
    }
}

impl Worm {
    /// Every field equals its default, by bits.
    pub fn is_default(&self) -> bool {
        d::is_worm_ground_acceleration(&self.ground_acceleration)
            && d::is_worm_air_acceleration(&self.air_acceleration)
            && d::is_worm_gravity(&self.gravity)
            && d::is_worm_jump_speed(&self.jump_speed)
            && d::is_zero(&self.air_drag)
            && d::is_worm_unused_angle_speed(&self.unused_angle_speed)
            && d::is_worm_unused_ground_friction(&self.unused_ground_friction)
    }
}

/// Ninja rope of the mod (`CGameScript.cpp:787-800`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rope {
    /// px: a flying hook farther than this stops and falls (FeatureList
    /// `RopeMaxLength`; OLX `CNinjaRope.cpp`).
    #[serde(default = "d::rope_max_length", skip_serializing_if = "d::is_rope_max_length")]
    pub max_length: i32,
    /// px: no pull while the worm is closer (FeatureList `RopeRestLength`).
    #[serde(default = "d::rope_rest_length", skip_serializing_if = "d::is_rope_rest_length")]
    pub rest_length: i32,
    /// × 100 px/s² toward the hook (FeatureList `RopeStrength`).
    #[serde(default = "d::rope_strength", skip_serializing_if = "d::is_rope_strength")]
    pub strength: f32,
}

impl Default for Rope {
    fn default() -> Self {
        Self { max_length: 300, rest_length: 30, strength: 3.5 }
    }
}

impl Rope {
    pub fn is_default(&self) -> bool {
        d::is_rope_max_length(&self.max_length)
            && d::is_rope_rest_length(&self.rest_length)
            && d::is_rope_strength(&self.strength)
    }
}
