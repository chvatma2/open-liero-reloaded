//! What a bot is (spec §12.1 H3: data, not code). Task 4 of plan 6 adds the
//! difficulty's numbers, the aim model and the work budget.

/// How a bot plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Personality {
    /// OLX's bot.
    #[default]
    Deathmatch,
    /// Fires everything, ignoring restrictions, for coverage (spec §20 F10).
    Chaos,
}

/// OLX's `AI_EASY` .. `AI_XTREME` (`include/CWorm.h`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Easy,
    #[default]
    Medium,
    Hard,
    Xtreme,
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

/// A bot's configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BotConfig {
    pub personality: Personality,
    pub difficulty: Difficulty,
    pub loadout: LoadoutPolicy,
}
