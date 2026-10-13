//! Content and match starts for the tests: Classic on Dirt Level from
//! `RL_CONTENT_DIR` (skipped with a notice without it).
#![allow(dead_code)]

use rl_bots::BotConfig;
use rl_content::{Content, DEFAULT_PRESET};
use rl_modes::DeathmatchSettings;
use rl_online::matches::{bot_wire, rules_wire, setup_packs};
use rl_proto::{Look, MatchSetupWire, MatchStartWire, SlotWire};

pub const MOD: &str = "olx-classic";
pub const LEVEL: &str = "olx-level-dirt-level";

pub fn content() -> Option<Content> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(std::path::Path::new(&dir)).unwrap())
}

pub fn look(name: &str) -> Look {
    Look {
        name: name.into(),
        colour: [200, 80, 40],
        skin: "olx-skin-default".into(),
        skin_hash: rl_proto::NO_HASH.into(),
    }
}

/// Classic on Dirt Level, one bot, ending after `minutes`.
pub fn setup(content: &Content, minutes: f32) -> MatchSetupWire {
    MatchSetupWire {
        mod_id: MOD.into(),
        level_id: LEVEL.into(),
        packs: setup_packs(content, MOD, LEVEL).unwrap(),
        rules: rules_wire(&DeathmatchSettings { time_limit: minutes, ..DeathmatchSettings::default() }),
        bonuses: false,
        loading_time: 100,
        restrictions: DEFAULT_PRESET.into(),
        bots: vec![bot_wire(&BotConfig::default(), look("Bot"))],
    }
}

/// A start of `setup`: member 0 in slot 0, then every bot.
pub fn start(setup: MatchSetupWire, seed: u32) -> MatchStartWire {
    let mut slots = vec![SlotWire { member: Some(0), bot: None, look: look("Host"), loadout: None }];
    slots.extend(setup.bots.iter().enumerate().map(|(i, b)| SlotWire {
        member: None,
        bot: Some(i as u8),
        look: b.look.clone(),
        loadout: None,
    }));
    MatchStartWire { seed, setup, slots, input_delay: vec![(0, 0)], start_checksum: 0 }
}
