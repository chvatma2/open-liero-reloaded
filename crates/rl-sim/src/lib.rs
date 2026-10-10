//! The deterministic simulation (spec §5), ported from OpenLieroX's LX56
//! physics at commit `d1ad953f` and verified against the oracle traces in
//! `fixtures/oracle` (bit-exact).
//!
//! `#![no_std]` + `alloc`: no files, threads or clock. Apart from setup
//! ([`World::spawn_worm`], [`World::set_loadout`]), [`World::tick`] is the only
//! way state changes, and it does not allocate.
//!
//! Scope (M3): worm movement and collision, keyboard input, digging, the ninja
//! rope, and OLX's two clocks. Weapons, projectiles, bonuses and damage come in M4.
#![no_std]

extern crate alloc;

mod input;
mod physics;
mod rope;
mod settings;
mod world;
mod worm;

pub use input::{KeyLatch, Keys, Latches};
pub use rope::NinjaRope;
pub use settings::{SettingsError, SimSettings};
pub use world::World;
pub use worm::{Dir, Loadout, Slot, Worm, WormState};
