//! The deterministic simulation (spec §5), ported from OpenLieroX's LX56
//! physics at commit `d1ad953f` and verified against the oracle traces in
//! `fixtures/oracle` (bit-exact).
//!
//! `#![no_std]` + `alloc`: no files, threads or clock. Apart from setup
//! ([`World::spawn_worm`], [`World::set_loadout`], [`World::place_bonus`]), [`World::tick`] is the only
//! way state changes, and it does not allocate.
//!
//! [`World::tick`] refills the caller's [`TickEvents`] (spec §5.7), and
//! [`World::checksum`] hashes the whole simulation state.
//!
//! Scope: worm movement and collision, keyboard input, digging, the ninja
//! rope, OLX's two clocks, and (M4) weapon slots, firing, the shot round trip,
//! projectile spawning, motion, collision and actions, explosions, dirt,
//! damage, death, bleeding and respawning, beams and the jetpack, and bonuses.
#![no_std]

extern crate alloc;

mod actions;
mod beam;
mod bonus;
mod buf;
mod checksum;
mod damage;
mod effects;
mod events;
mod input;
mod mod_rt;
mod physics;
mod proj_sim;
mod projectile;
mod respawn;
mod rng;
mod rope;
mod settings;
mod shot_codec;
mod shots;
mod spawn;
mod weapons;
mod world;
mod worm;

pub use bonus::{Bonus, BonusItem, BonusKind, MAX_BONUSES};
pub use events::{
    CosmeticKind, Event, EventKind, GAME_SOUND_PREFIX, GAME_SOUNDS, LostArea, SoundId, StampId, TerrainOp,
    TickEvents,
};
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
pub use worm::{Dir, Loadout, PosRecorder, Slot, Worm, WormState};
