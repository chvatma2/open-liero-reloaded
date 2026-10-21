//! The match local play starts (spec §21; OLX's local game menu,
//! `src/client/DeprecatedGUI/Menu_Local.cpp`): the mod, the level, the
//! Deathmatch rules, the bots and their difficulty, and the loadout (issue
//! #5). Mirrors of `olr-modes`' and `olr-bots`' types (see `settings.rs`),
//! each `#[serde(default)]`.

use std::ops::RangeInclusive;

use olr_bots::{BotConfig, Difficulty, LoadoutPolicy, Personality};
use olr_content::{MatchLoadout, MatchOptions, ModInfo, Player, WeaponChoice};
pub use olr_data::loadouts::LOADING_TIME_RANGE;
use olr_modes::DeathmatchSettings;
use olr_sim::{MAX_WORMS, SimSettings};
use serde::{Deserialize, Serialize};

/// OLX's bot difficulties (`olr_bots::Difficulty`; none aims perfectly).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BotLevel {
    Easy,
    #[default]
    Medium,
    Hard,
    Xtreme,
}

impl BotLevel {
    pub const ALL: [BotLevel; 4] = [BotLevel::Easy, BotLevel::Medium, BotLevel::Hard, BotLevel::Xtreme];

    pub fn difficulty(self) -> Difficulty {
        match self {
            BotLevel::Easy => Difficulty::Easy,
            BotLevel::Medium => Difficulty::Medium,
            BotLevel::Hard => Difficulty::Hard,
            BotLevel::Xtreme => Difficulty::Xtreme,
        }
    }
}

/// The bots' starting weapons (`olr_bots::LoadoutPolicy`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BotLoadout {
    /// OLX's presets (rifles, 100lt, mortars), else random.
    #[default]
    OlxPreset,
    Random,
}

impl BotLoadout {
    pub fn policy(self) -> LoadoutPolicy {
        match self {
            BotLoadout::OlxPreset => LoadoutPolicy::OlxPreset,
            BotLoadout::Random => LoadoutPolicy::Random,
        }
    }
}

/// How the bots play (`olr_bots::Personality`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BotPersonality {
    /// OLX's bot.
    #[default]
    Olx,
    /// Fires everything it has, ignoring restrictions (spec §20 F10).
    Chaos,
    /// Presses nothing, so the player can explore a level (plan 15c).
    Idle,
}

impl BotPersonality {
    pub const ALL: [BotPersonality; 3] = [BotPersonality::Olx, BotPersonality::Chaos, BotPersonality::Idle];

    pub fn personality(self) -> Personality {
        match self {
            BotPersonality::Olx => Personality::Deathmatch,
            BotPersonality::Chaos => Personality::Chaos,
            BotPersonality::Idle => Personality::Idle,
        }
    }
}

/// The Deathmatch rules the menu sets: `olr_modes::DeathmatchSettings`
/// without the respawn loadout (which `olr_content::build_match` decides).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RulePrefs {
    /// Below 0 unlimited.
    pub lives: i32,
    /// 0 or less is off.
    pub kill_limit: i32,
    /// Minutes; 0 or less is off.
    pub time_limit: f32,
    pub allow_negative_score: bool,
    pub death_decreases_score: f32,
    pub suicide_decreases_score: bool,
    pub sprees: [i32; 5],
    pub dying_sprees: [i32; 5],
}

impl Default for RulePrefs {
    fn default() -> Self {
        RulePrefs::from(&DeathmatchSettings::default())
    }
}

impl From<&DeathmatchSettings> for RulePrefs {
    fn from(d: &DeathmatchSettings) -> Self {
        let DeathmatchSettings {
            lives,
            kill_limit,
            time_limit,
            allow_negative_score,
            death_decreases_score,
            suicide_decreases_score,
            sprees,
            dying_sprees,
            respawn_loadout: _,
        } = d.clone();
        RulePrefs {
            lives,
            kill_limit,
            time_limit,
            allow_negative_score,
            death_decreases_score,
            suicide_decreases_score,
            sprees,
            dying_sprees,
        }
    }
}

impl RulePrefs {
    /// The rules as `olr-modes` takes them (OLX's respawn loadout).
    pub fn deathmatch(&self) -> DeathmatchSettings {
        let RulePrefs {
            lives,
            kill_limit,
            time_limit,
            allow_negative_score,
            death_decreases_score,
            suicide_decreases_score,
            sprees,
            dying_sprees,
        } = self.clone();
        DeathmatchSettings {
            lives,
            kill_limit,
            time_limit,
            allow_negative_score,
            death_decreases_score,
            suicide_decreases_score,
            sprees,
            dying_sprees,
            respawn_loadout: Default::default(),
        }
    }
}

/// The match local play starts (`olr_content::MatchOptions` without the
/// seed): the human is worm 0, then `bots` bots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MatchPrefs {
    /// A mod pack id, such as `olx-classic`.
    pub mod_id: String,
    /// A level pack id, such as `olx-level-liero-factory`.
    pub level_id: String,
    /// 1 to [`MAX_BOTS`].
    pub bots: u8,
    /// Every bot's difficulty, unless `per_bot` sets each one's.
    pub difficulty: BotLevel,
    /// Each bot's own difficulty (OLX's bots are profiles, each with its
    /// own level): bot `i` plays `per_bot[i]`.
    pub per_bot: Option<Vec<BotLevel>>,
    pub personality: BotPersonality,
    /// The bots' choice of starting weapons.
    pub loadout: BotLoadout,
    pub rules: RulePrefs,
    /// The match's loadout as last chosen (`crate::loadouts::resolve`):
    /// its name, which the menus look up again in the mod's loadouts (a
    /// name gone plays `Default`), and what it holds, which plays. An
    /// older file's `bonuses`, `restrictions` and `loading_time` are
    /// ignored: it plays `Default`.
    pub weapons: MatchLoadout,
}

/// The mod a fresh profile plays, locally and hosting (issue #21, the
/// maintainer's call): Liero v1.0 Reloaded, Classic with OLR's look, one
/// of OLR's own packs (`content/reloaded/`). Where it is not installed
/// Local Play shows the first mod listed instead.
pub const DEFAULT_MOD: &str = "olr-mod-liero-v1-reloaded";

impl Default for MatchPrefs {
    /// Liero v1.0 Reloaded ([`DEFAULT_MOD`]) on Liero Factory against 3
    /// Medium bots.
    fn default() -> Self {
        MatchPrefs {
            mod_id: DEFAULT_MOD.into(),
            level_id: olr_content::olx_level_id("Liero Factory"),
            bots: 3,
            difficulty: BotLevel::default(),
            per_bot: None,
            personality: BotPersonality::default(),
            loadout: BotLoadout::default(),
            rules: RulePrefs::default(),
            weapons: MatchLoadout::default(),
        }
    }
}

/// The most bots beside the human.
pub const MAX_BOTS: u8 = (MAX_WORMS - 1) as u8;

/// OLX's menu ranges (`src/common/FeatureList.cpp:31-35`; `LoadingTime`
/// is a loadout's, [`LOADING_TIME_RANGE`]): `Lives` (below 0 unlimited), `KillLimit` (below 1 off).
pub const LIVES_RANGE: RangeInclusive<i32> = 1..=150;
pub const KILL_LIMIT_RANGE: RangeInclusive<i32> = 1..=150;
/// `TimeLimit` in minutes (0 or less off).
pub const TIME_LIMIT_RANGE: RangeInclusive<f32> = 0.5..=20.0;

impl MatchPrefs {
    /// Bot `i`'s difficulty (0 is the first bot, worm 1).
    pub fn bot_level(&self, i: usize) -> BotLevel {
        self.per_bot.as_ref().and_then(|l| l.get(i).copied()).unwrap_or(self.difficulty)
    }

    /// `n` bots (clamped to 1..=[`MAX_BOTS`]); each bot's own difficulty
    /// is kept, a new bot gets the common one.
    pub fn set_bots(&mut self, n: u8) {
        self.bots = n.clamp(1, MAX_BOTS);
        let (bots, d) = (usize::from(self.bots), self.difficulty);
        if let Some(l) = &mut self.per_bot {
            l.resize(bots, d);
        }
    }

    /// Every bot at one difficulty (`None`), or each at its own (starting
    /// from the common one).
    pub fn set_per_bot(&mut self, on: bool) {
        self.per_bot = on.then(|| (0..usize::from(self.bots)).map(|i| self.bot_level(i)).collect());
    }

    /// Mod `m`, with the loadout of the same name when `m` (or the
    /// player, `own`) has one, else `Default` (`crate::loadouts::resolve`);
    /// what was wrong with it.
    pub fn set_mod(&mut self, m: &ModInfo, own: &crate::loadouts::OwnLoadouts) -> Vec<String> {
        self.mod_id = m.id.clone();
        let (l, w) = crate::loadouts::resolve(&self.weapons.name, m, own);
        self.weapons = l;
        w
    }

    /// The match to build on `seed` (`olr_content::build_match`'s input).
    pub fn options(&self, seed: u32) -> MatchOptions {
        let bots = usize::from(self.bots.clamp(1, MAX_BOTS));
        let bot = |i: usize| BotConfig {
            personality: self.personality.personality(),
            difficulty: self.bot_level(i).difficulty(),
            loadout: self.loadout.policy(),
            ..BotConfig::default()
        };
        MatchOptions {
            mod_id: self.mod_id.clone(),
            level_id: self.level_id.clone(),
            seed,
            settings: SimSettings::lx56(),
            players: std::iter::once(Player::Human).chain((0..bots).map(|i| Player::Bot(bot(i)))).collect(),
            deathmatch: self.rules.deathmatch(),
            weapons: WeaponChoice::Loadout(self.weapons.clone()),
            pool_capacity: olr_sim::MatchSetup::DEFAULT_POOL_CAPACITY,
        }
    }

    /// Fixes the bot count, the per-bot list's length, empty ids, the
    /// loadout's name and loading time and rules that `olr-modes` or OLX's ranges
    /// (`SimSettings::validate`) refuse, one warning each.
    pub fn sanitize(&mut self) -> Vec<String> {
        let d = MatchPrefs::default();
        let mut w = Vec::new();
        if !(1..=MAX_BOTS).contains(&self.bots) {
            let fixed = self.bots.clamp(1, MAX_BOTS);
            w.push(format!("match.bots {} is not 1 to {MAX_BOTS}: {fixed}", self.bots));
            self.bots = fixed;
        }
        if let Some(l) = &self.per_bot
            && l.len() != usize::from(self.bots)
        {
            w.push(format!("match.per_bot has {} bots, not {}: fitted", l.len(), self.bots));
            self.set_bots(self.bots);
        }
        for (name, v, default) in
            [("mod_id", &mut self.mod_id, d.mod_id), ("level_id", &mut self.level_id, d.level_id)]
        {
            if v.trim().is_empty() {
                w.push(format!("match.{name} is empty: {default}"));
                *v = default;
            }
        }
        let l = &mut self.weapons;
        let name = olr_data::loadouts::clean_name(&l.name);
        if name != l.name {
            w.push(format!("match.weapons.name {:?} is not usable: {name:?}", l.name));
            l.name = name;
        }
        if !LOADING_TIME_RANGE.contains(&l.loading_time) {
            let fixed = l.loading_time.clamp(*LOADING_TIME_RANGE.start(), *LOADING_TIME_RANGE.end());
            w.push(format!(
                "match.weapons.loading_time {} is not within {LOADING_TIME_RANGE:?}: {fixed}",
                l.loading_time
            ));
            l.loading_time = fixed;
        }
        let r = &mut self.rules;
        if !r.time_limit.is_finite() {
            w.push(format!(
                "match.rules.time_limit {} is not a number of minutes: {}",
                r.time_limit, d.rules.time_limit
            ));
            r.time_limit = d.rules.time_limit;
        }
        let mut s = SimSettings::lx56();
        r.deathmatch().apply(&mut s);
        if let Err(e) = s.validate() {
            w.push(format!("match.rules: {}: OLX's defaults", e.0));
            *r = RulePrefs { time_limit: r.time_limit, ..RulePrefs::default() };
        }
        w
    }
}
