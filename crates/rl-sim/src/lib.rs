//! The deterministic simulation (spec §5), ported from OpenLieroX's LX56
//! physics at commit `d1ad953f` and verified against the oracle traces in
//! `fixtures/oracle` (bit-exact).
//!
//! `#![no_std]` + `alloc`: no files, threads or clock. [`World::tick`] is the
//! only way state changes.
//!
//! Scope (M3): worm movement and collision, keyboard input, digging, the ninja
//! rope, and OLX's two clocks. Weapons, projectiles, bonuses and damage come in M4.
#![no_std]

extern crate alloc;

mod input;
mod settings;

pub use input::{KeyLatch, Keys, Latches};
pub use settings::{SettingsError, SimSettings};
