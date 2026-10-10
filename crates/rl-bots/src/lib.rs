//! OLX's bots for `rl-sim` matches (spec §5.9, §20 F1-F3, F7, F10), a port
//! of `CWormBotInputHandler` (`src/common/CWormBot.cpp`) at commit `d1ad953f`.
//! Notes: `docs/porting/olx-notes/bots-and-deathmatch.md`.
//!
//! `#![no_std]` + `alloc`. So far: the bot's configuration, its own random
//! generator ([`BotRng`]) and the weapons it starts with ([`loadout`]).
#![no_std]

extern crate alloc;

mod config;
pub mod loadout;
mod rng;

pub use config::{BotConfig, Difficulty, LoadoutPolicy, Personality};
pub use loadout::{
    BotWeapons, GameType, choose_loadout, get_random_weapons, olx_bot_weapons, random_enabled_weapon,
};
pub use rng::BotRng;
