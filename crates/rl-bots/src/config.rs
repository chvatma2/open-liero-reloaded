//! What a bot is (spec §12.1 H3: data, not code): its personality, its
//! difficulty and the numbers behind it (notes §1.9, §4.5), its aim model
//! (spec §20 F2) and its work budget (F7).

use rl_math::TimeDiff;

/// How a bot plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Personality {
    /// OLX's bot.
    #[default]
    Deathmatch,
    /// Fires everything, ignoring restrictions, for coverage (spec §20 F10).
    Chaos,
}

/// OLX's `AI_EASY` .. `AI_XTREME` (`include/ProfileSystem.h:52-55`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Easy,
    #[default]
    Medium,
    Hard,
    Xtreme,
}

impl Difficulty {
    /// OLX's `iAiDiffLevel`, 0 (Easy) to 3 (Xtreme).
    pub fn level(self) -> i32 {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Medium => 1,
            Difficulty::Hard => 2,
            Difficulty::Xtreme => 3,
        }
    }

    /// The numbers OLX derives from the level (notes §1.9). Most are
    /// `x · (AI_XTREME - level) / AI_XTREME`.
    /// OLX: `src/common/CWormBot.cpp:1682-1706` (aim offset and tolerance),
    /// `1836-1850` (target noise), `2342-2348` (weapon change), `3650` (rifles).
    pub fn params(self) -> DifficultyParams {
        let level = self.level();
        let f = (3 - level) as f32 / 3.0;
        DifficultyParams {
            aim_offset_max_deg: f * 50.0,
            fire_tolerance_deg: 3.0 + 20.0 * f,
            target_noise_px: f * 50.0,
            noise_refresh: TimeDiff(500),
            weapon_change_delay: TimeDiff([500, 300, 200, 120][level as usize]),
            rifle_reload_before_engage: level >= 2,
        }
    }
}

/// A difficulty's numbers (notes §4.5), overridable through
/// [`BotConfig::params`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DifficultyParams {
    /// The largest offset added to the wanted aim angle (`AI_SetAim`):
    /// a draw in `-1..=1` times this, refreshed every `noise_refresh`.
    pub aim_offset_max_deg: f32,
    /// How close the aim must be to fire (`AI_SetAim`'s good aim).
    pub fire_tolerance_deg: f32,
    /// The noise `weaponCanHit` adds to the target, per axis.
    pub target_noise_px: f32,
    /// How often the aim offset and target noise are drawn again.
    pub noise_refresh: TimeDiff,
    /// The least time between weapon changes (`AI_GetBestWeapon`).
    pub weapon_change_delay: TimeDiff,
    /// Rifle games: reload before engaging (Hard and Xtreme).
    pub rifle_reload_before_engage: bool,
}

/// How a bot aims a projectile weapon (spec §20 F2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AimModel {
    /// The gravity-aware solver ([`crate::aim::ballistic`]): the low root,
    /// rejected when the target is out of reach.
    #[default]
    Ballistic,
    /// OLX's aim: straight at the target ([`crate::aim::olx_direct`]), the
    /// solver used only as a feasibility test.
    OlxDirect,
}

/// Which weapons a Deathmatch bot starts with (spec §20 F1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LoadoutPolicy {
    /// OLX's presets (rifles, 100lt, mortars), else random weapons:
    /// `CWormBotInputHandler::initWeaponSelection`.
    #[default]
    OlxPreset,
    /// Always random weapons (`CWorm::GetRandomWeapons`), for variety.
    Random,
}

/// The work units a bot may spend in one `think` by default (notes §4.3).
pub const DEFAULT_BUDGET: u32 = 8192;

/// A bot's configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BotConfig {
    pub personality: Personality,
    pub difficulty: Difficulty,
    pub loadout: LoadoutPolicy,
    /// The difficulty's numbers, or `None` for [`Difficulty::params`].
    pub params: Option<DifficultyParams>,
    pub aim: AimModel,
    /// Work units (terrain probes) per `think` ([`crate::Bot::budget`]
    /// raises it to what the level's longest trace needs).
    pub budget: u32,
}

impl Default for BotConfig {
    fn default() -> Self {
        BotConfig {
            personality: Personality::default(),
            difficulty: Difficulty::default(),
            loadout: LoadoutPolicy::default(),
            params: None,
            aim: AimModel::default(),
            budget: DEFAULT_BUDGET,
        }
    }
}

impl BotConfig {
    /// The numbers the bot plays with.
    pub fn params(&self) -> DifficultyParams {
        self.params.unwrap_or_else(|| self.difficulty.params())
    }
}
