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
    /// `1836-1850` (`weaponCanHit`'s noise), `2342-2348` (weapon change),
    /// `3650` (rifles).
    ///
    /// Deviation (`docs/porting/STATUS.md`, "Bot aim error floor"): the aim
    /// errors (offset, target, gravity and speed noise) scale with at least
    /// [`AIM_ERROR_FLOOR`], so Xtreme, which OLX lets aim perfectly, keeps
    /// half of Hard's error. The fire tolerance stays OLX's.
    pub fn params(self) -> DifficultyParams {
        let level = self.level();
        let f = (3 - level) as f32 / 3.0;
        let e = f.max(AIM_ERROR_FLOOR);
        DifficultyParams {
            aim_offset_max_deg: e * 50.0,
            fire_tolerance_deg: 3.0 + 20.0 * f,
            target_noise_px: e * 50.0,
            gravity_noise: e * 5.0,
            speed_noise: e * 20.0,
            noise_refresh: TimeDiff(500),
            weapon_change_delay: TimeDiff([500, 300, 200, 120][level as usize]),
            rifle_reload_before_engage: level >= 2,
        }
    }
}

/// The least scale of a difficulty's aim errors, against OLX's
/// `(AI_XTREME - level) / AI_XTREME` (1 for Easy, 1/3 for Hard, 0 for
/// Xtreme): no bot aims perfectly (the user's direction: bots get harder
/// through strategy, reactions and reloading, never perfect aim). Measured
/// with 4 Xtreme bots over 20 seeds of 3 minutes (`docs/porting/STATUS.md`):
/// kills per minute drop 3-8 % from OLX's Xtreme and stay above Hard's.
pub const AIM_ERROR_FLOOR: f32 = 1.0 / 6.0;

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
    /// The noise `weaponCanHit` adds to the projectile's gravity (px/s²);
    /// here it also bends the ballistic aim (spec §20 F2).
    pub gravity_noise: f32,
    /// The noise `weaponCanHit` adds to the projectile's speed (px/s); here
    /// it also bends the ballistic aim.
    pub speed_noise: f32,
    /// How often the aim offset and `weaponCanHit`'s noise are drawn again.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapon_can_hit_noise_scales_with_the_level() {
        // `CWormBot.cpp:1844-1847`: gravity ±5f, speed ±20f, target ±50f.
        let p = Difficulty::Easy.params();
        assert_eq!((p.gravity_noise, p.speed_noise, p.target_noise_px), (5.0, 20.0, 50.0));
        let p = Difficulty::Hard.params();
        assert!((p.gravity_noise - 5.0 / 3.0).abs() < 1e-6 && (p.speed_noise - 20.0 / 3.0).abs() < 1e-5);
        // Xtreme: OLX has none; here the aim-error floor (1/6).
        let p = Difficulty::Xtreme.params();
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(close(p.gravity_noise, 5.0 / 6.0) && close(p.speed_noise, 20.0 / 6.0));
        assert!(close(p.target_noise_px, 50.0 / 6.0) && close(p.aim_offset_max_deg, 50.0 / 6.0));
    }

    /// The user's direction (`docs/porting/STATUS.md`, "Bot aim error
    /// floor"): no difficulty aims perfectly, Xtreme stays the most precise
    /// and keeps OLX's 3° fire tolerance.
    #[test]
    fn no_difficulty_aims_perfectly() {
        use Difficulty::*;
        let errors = |d: Difficulty| {
            let p = d.params();
            [p.aim_offset_max_deg, p.target_noise_px, p.gravity_noise, p.speed_noise]
        };
        for d in [Easy, Medium, Hard, Xtreme] {
            assert!(errors(d).iter().all(|&e| e > 0.0), "{d:?} aims perfectly: {:?}", errors(d));
        }
        for (x, h) in errors(Xtreme).into_iter().zip(errors(Hard)) {
            assert!(x < h, "Xtreme's error {x} is not below Hard's {h}");
        }
        assert_eq!(Xtreme.params().fire_tolerance_deg, 3.0);
    }
}
