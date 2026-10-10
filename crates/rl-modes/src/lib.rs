//! Game modes for `rl-sim` matches (spec §5.8, §20 F6): OLX's Deathmatch.
//!
//! `#![no_std]` + `alloc`. A mode is handed to `rl_sim::World::new` and
//! called back by the world where OLX calls its `CGameMode`.
#![no_std]

extern crate alloc;

mod deathmatch;
mod rules;
pub mod text;

pub use deathmatch::{Deathmatch, DeathmatchError, DeathmatchSettings};
pub use rules::{compare, olx_round};
