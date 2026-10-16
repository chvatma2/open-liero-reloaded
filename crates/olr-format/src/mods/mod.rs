//! The mod authoring format (spec §2): `mod.ron`, `weapons/<id>.ron`,
//! `projectiles/<id>.ron` (shared projectiles) and `restrictions.ron`.

use std::collections::BTreeMap;

use olr_data::restrictions::Preset;
use serde::{Deserialize, Serialize};

use crate::defaults as d;

mod spawn;
mod types;

pub use spawn::{
    EventContext, EventSpawn, ShotContext, ShotSpawn, Spawn, SpawnContext, TrailContext, TrailSpawn,
};
pub use types::*;

/// A mod as its authoring files hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModSource {
    pub name: String,
    pub notes: String,
    pub worm: Worm,
    pub rope: Rope,
    /// In order: a weapon's index is its id in the game.
    pub weapons: Vec<(String, Weapon)>,
    /// `projectiles/<id>.ron`: projectiles several places use.
    pub shared: BTreeMap<String, Projectile>,
    pub restrictions: BTreeMap<String, Preset>,
}

/// Worm physics of the mod (OLX `gs_worm_t`, `CGameScript.cpp:45-55`).
/// Defaults are OLX's FeatureList values (`FeatureList.cpp:67-71`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worm {
    /// × 90 px/s² while walking on the ground.
    #[serde(default = "d::worm_ground_acceleration", skip_serializing_if = "d::is_worm_ground_acceleration")]
    pub ground_acceleration: f32,
    /// × 90 px/s² while moving in the air.
    #[serde(default = "d::worm_air_acceleration", skip_serializing_if = "d::is_worm_air_acceleration")]
    pub air_acceleration: f32,
    /// px/s².
    #[serde(default = "d::worm_gravity", skip_serializing_if = "d::is_worm_gravity")]
    pub gravity: f32,
    /// px/s, negative = up: a ground jump sets the vertical speed to it.
    #[serde(default = "d::worm_jump_speed", skip_serializing_if = "d::is_worm_jump_speed")]
    pub jump_speed: f32,
    /// Quadratic drag in the air, 1/px.
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub air_drag: f32,
    /// Ignored by OLX ("was always ignored", `CGameScript.cpp:45`).
    #[serde(default = "d::worm_unused_angle_speed", skip_serializing_if = "d::is_worm_unused_angle_speed")]
    pub unused_angle_speed: f32,
    /// Ignored by OLX (`CGameScript.cpp:53`).
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
    /// px: a flying hook farther than this stops and falls.
    #[serde(default = "d::rope_max_length", skip_serializing_if = "d::is_rope_max_length")]
    pub max_length: i32,
    /// px: no pull while the worm is closer.
    #[serde(default = "d::rope_rest_length", skip_serializing_if = "d::is_rope_rest_length")]
    pub rest_length: i32,
    /// × 100 px/s² toward the hook.
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
