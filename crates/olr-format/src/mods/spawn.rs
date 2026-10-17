//! Spawns: how a weapon's shot, a projectile's events and its trail
//! create child projectiles (OLX `Proj_SpawnInfo`). LX56 fixes several
//! flags per context (`docs/porting/olx-notes/mod-fields.md` §1.5), so
//! each context has its own defaults and imported files carry none of
//! them.

use std::fmt::Debug;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use super::types::Projectile;

/// The defaults that differ between spawn contexts.
pub trait SpawnContext: Clone + Copy + Debug + Default + PartialEq + 'static {
    const RELATIVE_TO_PARENT: bool;
    const SPEED_VAR_DIAGONAL: bool;
    const RANDOM_ROTATION: bool;
    const INHERIT_VELOCITY: bool;
}

/// A weapon's shot: aimed along the worm's aim, random start rotation,
/// inherits the worm's velocity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShotContext;
impl SpawnContext for ShotContext {
    const RELATIVE_TO_PARENT: bool = true;
    const SPEED_VAR_DIAGONAL: bool = false;
    const RANDOM_ROTATION: bool = true;
    const INHERIT_VELOCITY: bool = true;
}

/// A projectile's event spawn (`GeneralSpawnInfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventContext;
impl SpawnContext for EventContext {
    const RELATIVE_TO_PARENT: bool = false;
    const SPEED_VAR_DIAGONAL: bool = false;
    const RANDOM_ROTATION: bool = false;
    const INHERIT_VELOCITY: bool = false;
}

/// A projectile trail's spawn (`Proj_Trail`): speed variation along (1, 1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrailContext;
impl SpawnContext for TrailContext {
    const RELATIVE_TO_PARENT: bool = false;
    const SPEED_VAR_DIAGONAL: bool = true;
    const RANDOM_ROTATION: bool = false;
    const INHERIT_VELOCITY: bool = false;
}

pub type ShotSpawn = Spawn<ShotContext>;
pub type EventSpawn = Spawn<EventContext>;
pub type TrailSpawn = Spawn<TrailContext>;

/// Per child `i` in `0..amount`, the direction is
/// `parent_velocity_as_direction ? parent.vel * parent_velocity_direction_scale
/// : angle + (relative_to_parent ? parent heading : 0) + r1 * spread` (degrees),
/// and the velocity is `dir * speed + (speed_var_diagonal ? (1, 1) : dir) *
/// speed_var * r2 + (inherit_velocity ? inherit_velocity_factor * parent.vel : 0)`,
/// with r1, r2 in [-1, 1] (OLX `PhysicsLX56_Projectiles.cpp:562-640`,
/// `olr-sim/src/spawn.rs`).
///
/// LX56 fixes four flags per context, so their defaults depend on where the
/// spawn is (`mod-fields.md` §9; a file writes them only when they differ):
/// `relative_to_parent` is true for a weapon's shot and false for an event
/// spawn and a trail; `speed_var_diagonal` is true for a trail only;
/// `random_rotation` and `inherit_velocity` are true for a shot only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "SpawnWire", into = "SpawnWire", bound = "C: SpawnContext")]
pub struct Spawn<C: SpawnContext> {
    /// Children per spawn, at most `MAX_AMOUNT` (1000) (OLX `amount`). Default 1.
    pub amount: i32,
    /// px/s, an integer; unitless when `parent_velocity_as_direction` (OLX `speed`).
    pub speed: i32,
    /// px/s, ± range (OLX `speed_var`).
    pub speed_var: f32,
    /// Degrees, ± half-width (360 = any direction) (OLX `spread`).
    pub spread: f32,
    /// Adds the parent's heading to `angle` (OLX `use_angle`).
    pub relative_to_parent: bool,
    /// Degrees (integer): 0 = +x (right), 90 = +y (down) (OLX `angle`).
    pub angle: i32,
    /// Direction from the parent's velocity instead of an angle (OLX
    /// `use_parent_velocity_for_spread`; LX56 reads it for trails only).
    pub parent_velocity_as_direction: bool,
    /// Unitless scale of that velocity (OLX `parent_vel_spread_factor`). Default 0.3.
    pub parent_velocity_direction_scale: f32,
    /// Speed variation along (1, 1) instead of the direction (OLX
    /// `use_special11_vec_for_speed_var`).
    pub speed_var_diagonal: bool,
    /// A random start rotation for a rotating child (OLX `use_random_rot`).
    pub random_rotation: bool,
    /// Adds `inherit_velocity_factor` × the parent's velocity. For a shot
    /// the parent is the worm, whose velocity counts as zero unless the
    /// game's `ProjRelativeVel` setting is on (OLX `add_parent_vel`).
    pub inherit_velocity: bool,
    /// Unitless factor on the parent's velocity (OLX `parent_vel_factor`). Default 1.0.
    pub inherit_velocity_factor: f32,
    /// The id of a shared projectile (`projectiles/<id>.ron`). Exactly one
    /// of `shared` and `projectile` is set (OLX `proj`).
    pub shared: Option<String>,
    /// The spawned projectile, defined here (OLX `proj`); inline projectiles nest at most `MAX_INLINE_DEPTH` (32) deep.
    pub projectile: Option<Box<Projectile>>,
    /// Zero-sized marker selecting the context's defaults; never written.
    pub context: PhantomData<C>,
}

impl<C: SpawnContext> Default for Spawn<C> {
    fn default() -> Self {
        Self {
            shared: None,
            projectile: None,
            amount: 1,
            speed: 0,
            speed_var: 0.0,
            spread: 0.0,
            relative_to_parent: C::RELATIVE_TO_PARENT,
            angle: 0,
            parent_velocity_as_direction: false,
            parent_velocity_direction_scale: 0.3,
            speed_var_diagonal: C::SPEED_VAR_DIAGONAL,
            random_rotation: C::RANDOM_ROTATION,
            inherit_velocity: C::INHERIT_VELOCITY,
            inherit_velocity_factor: 1.0,
            context: PhantomData,
        }
    }
}

/// The text form: a field is written only when it differs from its
/// context's default (floats by bits).
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename = "Spawn")]
struct SpawnWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    amount: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speed: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speed_var: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    spread: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relative_to_parent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_velocity_as_direction: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_velocity_direction_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speed_var_diagonal: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    random_rotation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inherit_velocity: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inherit_velocity_factor: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shared: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projectile: Option<Box<Projectile>>,
}

fn differs<T: PartialEq>(v: T, d: T) -> Option<T> {
    (v != d).then_some(v)
}

fn differs_f32(v: f32, d: f32) -> Option<f32> {
    (v.to_bits() != d.to_bits()).then_some(v)
}

impl<C: SpawnContext> From<Spawn<C>> for SpawnWire {
    fn from(s: Spawn<C>) -> Self {
        let d = Spawn::<C>::default();
        SpawnWire {
            shared: s.shared,
            projectile: s.projectile,
            amount: differs(s.amount, d.amount),
            speed: differs(s.speed, d.speed),
            speed_var: differs_f32(s.speed_var, d.speed_var),
            spread: differs_f32(s.spread, d.spread),
            relative_to_parent: differs(s.relative_to_parent, d.relative_to_parent),
            angle: differs(s.angle, d.angle),
            parent_velocity_as_direction: differs(
                s.parent_velocity_as_direction,
                d.parent_velocity_as_direction,
            ),
            parent_velocity_direction_scale: differs_f32(
                s.parent_velocity_direction_scale,
                d.parent_velocity_direction_scale,
            ),
            speed_var_diagonal: differs(s.speed_var_diagonal, d.speed_var_diagonal),
            random_rotation: differs(s.random_rotation, d.random_rotation),
            inherit_velocity: differs(s.inherit_velocity, d.inherit_velocity),
            inherit_velocity_factor: differs_f32(s.inherit_velocity_factor, d.inherit_velocity_factor),
        }
    }
}

impl<C: SpawnContext> From<SpawnWire> for Spawn<C> {
    fn from(w: SpawnWire) -> Self {
        let d = Spawn::<C>::default();
        Spawn {
            shared: w.shared,
            projectile: w.projectile,
            amount: w.amount.unwrap_or(d.amount),
            speed: w.speed.unwrap_or(d.speed),
            speed_var: w.speed_var.unwrap_or(d.speed_var),
            spread: w.spread.unwrap_or(d.spread),
            relative_to_parent: w.relative_to_parent.unwrap_or(d.relative_to_parent),
            angle: w.angle.unwrap_or(d.angle),
            parent_velocity_as_direction: w
                .parent_velocity_as_direction
                .unwrap_or(d.parent_velocity_as_direction),
            parent_velocity_direction_scale: w
                .parent_velocity_direction_scale
                .unwrap_or(d.parent_velocity_direction_scale),
            speed_var_diagonal: w.speed_var_diagonal.unwrap_or(d.speed_var_diagonal),
            random_rotation: w.random_rotation.unwrap_or(d.random_rotation),
            inherit_velocity: w.inherit_velocity.unwrap_or(d.inherit_velocity),
            inherit_velocity_factor: w.inherit_velocity_factor.unwrap_or(d.inherit_velocity_factor),
            context: PhantomData,
        }
    }
}
