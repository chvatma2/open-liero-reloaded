//! The game's settings (spec §8.3 options, §9.4 `settings.ron`, §21 G2-G3):
//! video, audio, key bindings and gameplay, with the match the local-play
//! menu starts.
//!
//! The match preferences mirror `rl-modes`' and `rl-bots`' types rather
//! than serialize them: the file holds what the menus edit, under names
//! this crate owns, so a change inside those crates never silently changes
//! the file (as `rl-replay`'s `mirror.rs`). Every conversion names every
//! field. Each struct is `#[serde(default)]`: a field missing from a file
//! takes its default.

use std::ops::RangeInclusive;

use rl_bots::{BotConfig, Difficulty, LoadoutPolicy};
use rl_content::{MatchOptions, Player};
use rl_input::Bindings;
use rl_modes::DeathmatchSettings;
use rl_sim::{MAX_WORMS, SimSettings};
use serde::{Deserialize, Serialize};

/// The version `settings.ron` is written with; any other is refused.
pub const SETTINGS_VERSION: u32 = 1;

/// The UI scale's range (plan 7 Review Focus 5), times the DPI scale.
pub const UI_SCALE_RANGE: RangeInclusive<f32> = 0.5..=3.0;

/// Everything the options menu sets.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub video: VideoSettings,
    pub audio: AudioSettings,
    /// The keys when the selected profile has none of its own.
    pub input: Bindings,
    pub gameplay: GameplaySettings,
}

impl Settings {
    /// Fixes every value a file may hold out of range (NaN, infinite, too
    /// large), one warning each; the rest is kept.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut w = self.video.sanitize();
        w.extend(self.audio.sanitize());
        w.extend(self.gameplay.match_prefs.sanitize());
        w
    }
}

/// The window, presentation and menu scale (spec §8.2, §8.6; §21 G3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    /// The window's size when not fullscreen. The window is not resizable:
    /// sizes come only from these presets (and startup flags).
    pub window: WindowPreset,
    /// Borderless fullscreen.
    pub fullscreen: bool,
    pub frame_rate: FrameRate,
    pub filter: UpscaleFilter,
    /// Interpolated drawing between ticks (off: the latest tick at whole
    /// pixels, like OLX).
    pub interpolation: bool,
    /// The menus' and HUD's scale ([`UI_SCALE_RANGE`]).
    pub ui_scale: f32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        VideoSettings {
            window: WindowPreset::default(),
            fullscreen: false,
            frame_rate: FrameRate::default(),
            filter: UpscaleFilter::default(),
            interpolation: true,
            ui_scale: 1.0,
        }
    }
}

impl VideoSettings {
    /// Clamps the UI scale into [`UI_SCALE_RANGE`] (1 when it is NaN).
    pub fn sanitize(&mut self) -> Vec<String> {
        let fixed = clamp_scale(self.ui_scale);
        if fixed.to_bits() == self.ui_scale.to_bits() {
            return Vec::new();
        }
        let w = format!("video.ui_scale {} is outside {:?}: {fixed}", self.ui_scale, UI_SCALE_RANGE);
        self.ui_scale = fixed;
        vec![w]
    }
}

/// `scale` within [`UI_SCALE_RANGE`]; 1 when it is NaN.
pub fn clamp_scale(scale: f32) -> f32 {
    if scale.is_nan() { 1.0 } else { scale.clamp(*UI_SCALE_RANGE.start(), *UI_SCALE_RANGE.end()) }
}

/// The window sizes offered (§21 G3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowPreset {
    #[default]
    W1280x720,
    W1600x900,
    W1920x1080,
    /// The desktop's size (the monitor the window is on).
    Desktop,
}

impl WindowPreset {
    pub const ALL: [WindowPreset; 4] =
        [WindowPreset::W1280x720, WindowPreset::W1600x900, WindowPreset::W1920x1080, WindowPreset::Desktop];

    /// The window's inner size in physical pixels; `desktop` is the
    /// monitor's size, when known (else 1280 × 720).
    pub fn size(self, desktop: Option<(u32, u32)>) -> (u32, u32) {
        match self {
            WindowPreset::W1280x720 => (1280, 720),
            WindowPreset::W1600x900 => (1600, 900),
            WindowPreset::W1920x1080 => (1920, 1080),
            WindowPreset::Desktop => desktop.filter(|&(w, h)| w > 0 && h > 0).unwrap_or((1280, 720)),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WindowPreset::W1280x720 => "1280 × 720",
            WindowPreset::W1600x900 => "1600 × 900",
            WindowPreset::W1920x1080 => "1920 × 1080",
            WindowPreset::Desktop => "Desktop size",
        }
    }
}

/// How frames are paced (spec §8.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameRate {
    /// Mailbox when available, else FIFO.
    #[default]
    VSync,
    /// Present as fast as the GPU draws (`rl-game --uncapped`).
    Uncapped,
}

/// The upscale filter (`rl_render::UpscaleFilter`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpscaleFilter {
    /// Whole multiples of the art, centred, black bars around.
    Nearest,
    /// Fills the window keeping the aspect; texel edges blend over one pixel.
    #[default]
    Sharp,
}

/// Volumes, 0 to 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub master: f32,
    pub sfx: f32,
    pub music: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings { master: 1.0, sfx: 1.0, music: 0.7 }
    }
}

impl AudioSettings {
    /// Clamps each volume into 0..=1 (its default when it is NaN).
    pub fn sanitize(&mut self) -> Vec<String> {
        let d = AudioSettings::default();
        let mut w = Vec::new();
        for (name, v, default) in [
            ("master", &mut self.master, d.master),
            ("sfx", &mut self.sfx, d.sfx),
            ("music", &mut self.music, d.music),
        ] {
            let fixed = if v.is_nan() { default } else { v.clamp(0.0, 1.0) };
            if fixed.to_bits() != v.to_bits() {
                w.push(format!("audio.{name} {v} is not a volume from 0 to 1: {fixed}"));
                *v = fixed;
            }
        }
        w
    }
}

/// The HUD's look (§21 G5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HudStyle {
    /// egui panels.
    #[default]
    Modern,
    /// OLX's bitmap fonts and layout.
    Classic,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameplaySettings {
    pub hud: HudStyle,
    /// The match local play starts, as last set up.
    pub match_prefs: MatchPrefs,
}

/// OLX's bot difficulties (`rl_bots::Difficulty`; none aims perfectly).
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

/// The bots' starting weapons (`rl_bots::LoadoutPolicy`).
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

/// The Deathmatch rules the menu sets: `rl_modes::DeathmatchSettings`
/// without the respawn loadout (which `rl_content::build_match` decides).
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
    /// The rules as `rl-modes` takes them (OLX's respawn loadout).
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

/// The match local play starts (`rl_content::MatchOptions` without the
/// seed): the human is worm 0, then `bots` bots alike.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MatchPrefs {
    /// A mod pack id, such as `olx-classic`.
    pub mod_id: String,
    /// A level pack id, such as `olx-level-liero-factory`.
    pub level_id: String,
    /// 1 to `MAX_WORMS - 1`.
    pub bots: u8,
    pub difficulty: BotLevel,
    pub loadout: BotLoadout,
    pub rules: RulePrefs,
    /// OLX's `Bonuses`.
    pub bonuses: bool,
    /// OLX's `WeaponRestrictionsFile`.
    pub restrictions: String,
}

impl Default for MatchPrefs {
    /// `rl-game`'s defaults: Classic on Liero Factory against 3 Medium bots.
    fn default() -> Self {
        MatchPrefs {
            mod_id: rl_content::olx_mod_id("Classic"),
            level_id: rl_content::olx_level_id("Liero Factory"),
            bots: 3,
            difficulty: BotLevel::default(),
            loadout: BotLoadout::default(),
            rules: RulePrefs::default(),
            bonuses: false,
            restrictions: rl_content::DEFAULT_PRESET.into(),
        }
    }
}

/// The most bots beside the human.
pub const MAX_BOTS: u8 = (MAX_WORMS - 1) as u8;

impl MatchPrefs {
    /// The match to build on `seed` (`rl_content::build_match`'s input).
    pub fn options(&self, seed: u32) -> MatchOptions {
        let bot = BotConfig {
            difficulty: self.difficulty.difficulty(),
            loadout: self.loadout.policy(),
            ..BotConfig::default()
        };
        let bots = self.bots.clamp(1, MAX_BOTS);
        MatchOptions {
            mod_id: self.mod_id.clone(),
            level_id: self.level_id.clone(),
            seed,
            settings: SimSettings { bonuses: self.bonuses, ..SimSettings::lx56() },
            players: std::iter::once(Player::Human).chain((0..bots).map(|_| Player::Bot(bot))).collect(),
            deathmatch: self.rules.deathmatch(),
            restrictions: self.restrictions.clone(),
        }
    }

    /// Fixes the bot count, empty ids and rules that `rl-modes` or OLX's
    /// ranges (`SimSettings::validate`) refuse, one warning each.
    pub fn sanitize(&mut self) -> Vec<String> {
        let d = MatchPrefs::default();
        let mut w = Vec::new();
        if !(1..=MAX_BOTS).contains(&self.bots) {
            let fixed = self.bots.clamp(1, MAX_BOTS);
            w.push(format!("match.bots {} is not 1 to {MAX_BOTS}: {fixed}", self.bots));
            self.bots = fixed;
        }
        for (name, v, default) in [
            ("mod_id", &mut self.mod_id, d.mod_id),
            ("level_id", &mut self.level_id, d.level_id),
            ("restrictions", &mut self.restrictions, d.restrictions),
        ] {
            if v.trim().is_empty() {
                w.push(format!("match.{name} is empty: {default}"));
                *v = default;
            }
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
