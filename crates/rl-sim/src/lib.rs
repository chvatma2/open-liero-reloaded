//! The deterministic simulation (spec §5), ported from OpenLieroX's LX56
//! physics at commit `d1ad953f` and verified against the oracle traces in
//! `fixtures/oracle` (bit-exact).
//!
//! `#![no_std]` + `alloc`: no files, threads or clock. Apart from setup
//! ([`World::spawn_worm`], [`World::set_loadout`]), [`World::tick`] is the only
//! way state changes, and it does not allocate.
//!
//! Scope: worm movement and collision, keyboard input, digging, the ninja
//! rope, OLX's two clocks, and (M4) weapon slots, firing, the shot round trip
//! and projectile spawning. Projectile motion, bonuses and damage follow.
#![no_std]

extern crate alloc;

mod input;
mod mod_rt;
mod physics;
mod projectile;
mod rng;
mod rope;
mod settings;
mod shot_codec;
mod shots;
mod weapons;
mod world;
mod worm;

pub use input::{KeyLatch, Keys, Latches};
pub use mod_rt::{
    MAX_AMOUNT, MAX_DEPTH, MAX_PROJECTILES, SimMod, SimModError, SimProjectile, SimWeapon, SpawnInfo,
    WeaponType,
};
pub use projectile::{Projectile, ProjectilePool};
pub use rope::NinjaRope;
pub use settings::{SettingsError, SimSettings};
pub use shots::{MAX_SHOOTINGS, RoundTrip, Shot, encode_decode, spawn_time};
pub use world::{CLOCK_OFFSET_MS, MAX_WORMS, World};
pub use worm::{Dir, Loadout, Slot, Worm, WormState};
