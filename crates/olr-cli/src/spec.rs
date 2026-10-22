//! What a headless bot match plays (spec §7.1): `olr-cli match`'s options,
//! a failure's record and a golden match's header; and the scripted keys
//! of its local human players (`--locals`, issue #29).

use olr_bots::BotConfig;
use olr_content::{MatchOptions, Player};
use olr_sim::Keys;
use serde::{Deserialize, Serialize};

/// The game mode (`--mode`). Only Deathmatch exists so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Mode {
    #[default]
    Deathmatch,
}

/// How every bot plays (`--personality`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Personality {
    /// OLX's bot.
    #[default]
    #[value(name = "dm")]
    Deathmatch,
    /// Fires everything; its loadout rotates on every spawn (spec §20 F10).
    Chaos,
    /// Presses nothing (plan 15c).
    Idle,
}

/// The bots' difficulty (`--difficulty`); `mixed` gives bot `i` Easy,
/// Medium, Hard and Xtreme in turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
    Xtreme,
    #[default]
    Mixed,
}

fn kill_limit_default() -> i32 {
    15
}

fn time_limit_default() -> f32 {
    6.0
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The most local human players (`--locals`): split screen's views.
pub const MAX_LOCALS: usize = olr_replay::MAX_VIEWERS;

/// The keys local player `player` (0 to [`MAX_LOCALS`] - 1) presses in tick
/// `tick` of a headless match (`--locals`, issue #29): walking one way then
/// the other, jumping, aiming up and down, firing in bursts, throwing the
/// rope and changing weapons, each on periods of its own, so no two players
/// press alike and every worm's keys in the replay show whose they are.
pub fn local_keys(player: usize, tick: u32) -> Keys {
    let p = player as u32;
    let phase = |period: u32| (tick + 17 * p) % period;
    let walk = 120 + 40 * p;
    let right = (phase(walk) < walk / 2) != (p % 2 == 1);
    let mut k = if right { Keys::RIGHT } else { Keys::LEFT };
    if phase(50 + 10 * p) < 5 {
        k |= Keys::JUMP;
    }
    k |= match (tick / (30 + 5 * p)) % 4 {
        0 => Keys::UP,
        2 => Keys::DOWN,
        _ => 0,
    };
    if (tick / (70 + 15 * p)).is_multiple_of(2) && tick % 10 < 5 {
        k |= Keys::FIRE;
    }
    if phase(300 + 50 * p) < 2 {
        k |= Keys::ROPE;
    }
    if phase(500 + 60 * p) == 0 {
        k = Keys::CHANGE | Keys::RIGHT;
    }
    Keys(k)
}

/// A headless match of bots: everything that decides how it plays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, clap::Args)]
pub struct MatchSpec {
    /// The mod pack, such as `olx-classic` or `olx-mw-1-0`.
    #[arg(long = "mod")]
    pub mod_id: String,
    /// The level pack, such as `olx-level-dirt-level`.
    #[arg(long = "level")]
    pub level_id: String,
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub mode: Mode,
    /// How many bots play (at least 2 worms in all, with `--locals`).
    #[arg(long)]
    pub bots: usize,
    /// Local human players (0 to 4: split screen's), worms 0 to N - 1
    /// before the bots, each pressing its own scripted keys
    /// ([`local_keys`]; issue #29).
    #[arg(long, default_value_t = 0)]
    #[serde(default, skip_serializing_if = "is_zero")]
    pub locals: usize,
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub difficulty: Difficulty,
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub personality: Personality,
    /// The match seed; each bot's generator is seeded from it too.
    #[arg(long)]
    pub seed: u32,
    /// The most ticks (100 a second) to play; the match may end sooner.
    #[arg(long)]
    pub ticks: u32,
    /// Bonuses fall every this many seconds (OLX's `BonusFrequency`); none
    /// without it.
    #[arg(long)]
    #[serde(default)]
    pub bonus_frequency: Option<f32>,
    /// `KillLimit`: the match ends when a score reaches it (0 or less: off).
    #[arg(long, default_value_t = kill_limit_default(), allow_negative_numbers = true)]
    #[serde(default = "kill_limit_default")]
    pub kill_limit: i32,
    /// `TimeLimit` in minutes (0: off).
    #[arg(long, default_value_t = time_limit_default())]
    #[serde(default = "time_limit_default")]
    pub time_limit: f32,
    /// OLX's `LoadingTime` in percent (0: weapons reload at once); OLX's
    /// LX56 100 without it.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading_time: Option<i32>,
    /// The projectile pool's size (1 to 65 536); OLX's 3000 without it.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_capacity: Option<usize>,
}

impl MatchSpec {
    /// A Deathmatch of `bots` bots (OLX's bots, mixed difficulties, OLX's
    /// limits, no bonuses).
    pub fn new(mod_id: &str, level_id: &str, bots: usize, seed: u32, ticks: u32) -> Self {
        MatchSpec {
            mod_id: mod_id.into(),
            level_id: level_id.into(),
            mode: Mode::Deathmatch,
            bots,
            locals: 0,
            difficulty: Difficulty::Mixed,
            personality: Personality::Deathmatch,
            seed,
            ticks,
            bonus_frequency: None,
            kill_limit: kill_limit_default(),
            time_limit: time_limit_default(),
            loading_time: None,
            pool_capacity: None,
        }
    }

    /// Bot `worm`'s configuration.
    pub fn bot_config(&self, worm: usize) -> BotConfig {
        use olr_bots::Difficulty as D;
        let difficulty = match self.difficulty {
            Difficulty::Easy => D::Easy,
            Difficulty::Medium => D::Medium,
            Difficulty::Hard => D::Hard,
            Difficulty::Xtreme => D::Xtreme,
            Difficulty::Mixed => [D::Easy, D::Medium, D::Hard, D::Xtreme][worm % 4],
        };
        let personality = match self.personality {
            Personality::Deathmatch => olr_bots::Personality::Deathmatch,
            Personality::Chaos => olr_bots::Personality::Chaos,
            Personality::Idle => olr_bots::Personality::Idle,
        };
        BotConfig { personality, difficulty, ..BotConfig::default() }
    }

    /// What `olr_content::build_match` assembles: the local humans
    /// (`--locals`, random weapons), then the bots (bot `i` is
    /// [`MatchSpec::bot_config`]`(i)`), OLX's LX56 settings with the bonus
    /// and loading time options, the Deathmatch limits, the default
    /// restrictions and the pool size (OLX's unless given).
    pub fn match_options(&self) -> MatchOptions {
        let humans = (0..self.locals).map(|_| Player::Human);
        let players = humans.chain((0..self.bots).map(|i| Player::Bot(self.bot_config(i)))).collect();
        let mut o = MatchOptions::deathmatch(&self.mod_id, &self.level_id, self.seed, players);
        if let Some(f) = self.bonus_frequency {
            (o.settings.bonuses, o.settings.bonus_frequency) = (true, f);
        }
        if let Some(l) = self.loading_time {
            o.settings.loading_time = l;
        }
        (o.deathmatch.kill_limit, o.deathmatch.time_limit) = (self.kill_limit, self.time_limit);
        if let Some(p) = self.pool_capacity {
            o.pool_capacity = p;
        }
        o
    }

    /// A file name for this match: mod, level, local players (when any),
    /// bots, personality, difficulty and seed.
    pub fn stem(&self) -> String {
        let p = match self.personality {
            Personality::Deathmatch => "dm",
            Personality::Chaos => "chaos",
            Personality::Idle => "idle",
        };
        let d = format!("{:?}", self.difficulty).to_lowercase();
        let short = |id: &str| id.trim_start_matches("olx-level-").trim_start_matches("olx-").to_string();
        let locals = if self.locals > 0 { format!("{}l-", self.locals) } else { String::new() };
        let (m, l) = (short(&self.mod_id), short(&self.level_id));
        format!("{m}-{l}-{locals}{}{p}-{d}-seed{}", self.bots, self.seed)
    }
}
