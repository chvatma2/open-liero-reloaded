//! OLX's bots for `rl-sim` matches (spec §5.9, §20 F1-F3, F7, F10), a port
//! of `CWormBotInputHandler` (`src/common/CWormBot.cpp`) at commit `d1ad953f`.
//! Notes: `docs/porting/olx-notes/bots-and-deathmatch.md`.
//!
//! `#![no_std]` + `alloc`. A [`Bot`] reads a `World` once per tick and
//! presses its worm's [`rl_sim::Keys`] through the [`Motor`]: OLX's brain
//! (targets, shooting, weapon choice, rope, digging) or the chaos
//! personality. It navigates over rock-free areas ([`nav`]), aims
//! ([`aim`]), draws only from its own [`BotRng`] and spends at most its work
//! [`Budget`] per tick. It also chooses the weapons a bot starts with
//! ([`loadout`]).
#![no_std]

extern crate alloc;

pub mod aim;
mod bot;
mod brain;
mod budget;
mod chaos;
mod config;
pub mod loadout;
pub mod motor;
mod moving;
pub mod nav;
mod probe;
mod rng;
mod rope;
mod shoot;
mod spot;
mod weapons;

pub use aim::{Aim, AimState, Launch, ShotNoise, ballistic, ballistic_from, olx_direct};
pub use bot::{Bot, min_budget};
pub use budget::Budget;
pub use config::{
    AIM_ERROR_FLOOR, AimModel, BotConfig, BotConfigError, DEFAULT_BUDGET, Difficulty, DifficultyParams,
    LoadoutPolicy, Personality,
};
pub use loadout::{
    BotWeapons, GameType, choose_loadout, get_random_weapons, olx_bot_weapons, random_enabled_weapon,
};
pub use motor::{Intent, Motor};
pub use nav::{AreaCache, Search, SearchError, SearchStatus};
pub use rng::BotRng;
