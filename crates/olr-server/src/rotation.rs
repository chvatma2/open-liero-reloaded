//! The map rotation: each entry's match setup (its packs hashed once, at
//! the start), the bots that fill it, the entry played next.

use anyhow::{Context as _, bail};
use olr_bots::BotConfig;
use olr_content::{Content, DEFAULT_PRESET};
use olr_lobby::DEFAULT_SKIN;
use olr_modes::DeathmatchSettings;
use olr_online::matches::{bot_wire, rules_wire, setup_packs};
use olr_proto::{BotWire, Look, MAX_BOTS, MatchSetupWire, valid_setup};
use olr_sim::SimSettings;

use crate::admin::Setting;
use crate::config::Entry;

/// OLX's bot colours, in turn.
/// OLX: src/client/ProfileSystem.cpp:47-59 (as olr-ui's `CPU_COLOURS`).
pub const BOT_COLOURS: [[u8; 3]; 7] =
    [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 0, 255], [0, 255, 255], [128, 128, 128], [128, 255, 0]];

/// An entry with what its setup needs from the content.
#[derive(Clone, Debug)]
struct Prepared {
    entry: Entry,
    packs: Vec<(String, String)>,
    restrictions: String,
}

/// The maps in turn.
#[derive(Clone, Debug)]
pub struct Rotation {
    entries: Vec<Prepared>,
    /// The entry the lobby offers: the next match's.
    next: usize,
}

impl Rotation {
    /// Every entry checked against `content` (its mod and level there, its
    /// setup within the wire's bounds with up to `max_bots` bots).
    pub fn new(content: &Content, entries: &[Entry], max_bots: u8) -> anyhow::Result<Rotation> {
        if entries.is_empty() {
            bail!("the rotation has no map");
        }
        let entries = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let what = || format!("rotation entry {} ({} on {})", i + 1, e.mod_id, e.level);
                let packs = setup_packs(content, &e.mod_id, &e.level).with_context(what)?;
                let presets = content.mod_info(&e.mod_id).with_context(what)?.presets;
                // As the menus choose (olr-ui's `MatchPrefs::set_mod`).
                let restrictions = if presets.iter().any(|p| p == DEFAULT_PRESET) {
                    DEFAULT_PRESET.to_string()
                } else {
                    presets.first().cloned().unwrap_or_default()
                };
                let p = Prepared { entry: e.clone(), packs, restrictions };
                for bots in [0, max_bots] {
                    if !valid_setup(&p.setup(bots)) {
                        bail!("{}: its settings are out of bounds", what());
                    }
                }
                Ok(p)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Rotation { entries, next: 0 })
    }

    /// The entry the next match plays.
    pub fn next(&self) -> &Entry {
        &self.entries[self.next].entry
    }

    /// The next entry's place in the rotation.
    pub fn next_index(&self) -> usize {
        self.next
    }

    /// How many entries the rotation has (at least one).
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The next match's setup with `bots` bots.
    pub fn setup(&self, bots: u8) -> MatchSetupWire {
        self.entries[self.next].setup(bots)
    }

    /// The next entry is the one after.
    pub fn advance(&mut self) {
        self.next = (self.next + 1) % self.entries.len();
    }

    /// Changes the next match's entry (for the rest of the server's run);
    /// refused, unchanged, when the setup would leave the wire's bounds.
    pub fn set(&mut self, s: Setting) -> Result<(), String> {
        let p = &mut self.entries[self.next];
        let mut changed = p.clone();
        match s {
            Setting::TimeLimit(m) => changed.entry.time_limit = m,
            Setting::KillLimit(k) => changed.entry.kill_limit = k,
            Setting::Lives(l) => changed.entry.lives = l,
            Setting::Bonuses(b) => changed.entry.bonuses = b,
        }
        if !valid_setup(&changed.setup(0)) {
            return Err("that value is out of bounds".into());
        }
        *p = changed;
        Ok(())
    }
}

impl Prepared {
    fn setup(&self, bots: u8) -> MatchSetupWire {
        let e = &self.entry;
        let rules = DeathmatchSettings {
            time_limit: e.time_limit,
            kill_limit: e.kill_limit,
            lives: e.lives,
            ..DeathmatchSettings::default()
        };
        MatchSetupWire {
            mod_id: e.mod_id.clone(),
            level_id: e.level.clone(),
            packs: self.packs.clone(),
            rules: rules_wire(&rules),
            bonuses: e.bonuses,
            loading_time: SimSettings::lx56().loading_time,
            restrictions: self.restrictions.clone(),
            bots: fill_bots(bots),
        }
    }
}

/// `n` bots (at most `MAX_BOTS`): "Bot 1", "Bot 2", ... in OLX's bot
/// colours and the default skin, of the default difficulty.
pub fn fill_bots(n: u8) -> Vec<BotWire> {
    (0..usize::from(n).min(MAX_BOTS))
        .map(|i| {
            let look = Look {
                name: format!("Bot {}", i + 1),
                colour: BOT_COLOURS[i % BOT_COLOURS.len()],
                skin: DEFAULT_SKIN.into(),
                skin_hash: olr_proto::NO_HASH.into(),
            };
            bot_wire(&BotConfig::default(), look)
        })
        .collect()
}
