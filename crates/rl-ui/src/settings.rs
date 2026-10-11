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

use rl_input::{Bindings, KeyCode, key_name};
use serde::{Deserialize, Serialize};

use crate::match_prefs::MatchPrefs;

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
        // The game's keys are dropped; keys that leave an action
        // unreachable get OLX's back (Review Focus 2).
        w.extend(sanitize_bindings(&mut self.input).into_iter().map(|x| format!("input: {x}")));
        w.extend(self.gameplay.match_prefs.sanitize());
        w
    }
}

/// The keys the game keeps for itself (`rl-game`'s `on_key` and the
/// menus), with what they do: never bound to an action.
pub const GAME_KEYS: [(KeyCode, &str); 5] = [
    (KeyCode::Escape, "the menus"),
    (KeyCode::F5, "restart"),
    (KeyCode::F2, "the debug panel"),
    (KeyCode::Tab, "the scores"),
    (KeyCode::KeyH, "the health bars"),
];

/// Makes bindings from a file or a widget usable: the [`GAME_KEYS`] are
/// dropped (a hand-edited file may bind them), then every action left
/// without a key or pad input gets OLX's back (`Bindings::repair`). One
/// warning per fix.
pub fn sanitize_bindings(b: &mut Bindings) -> Vec<String> {
    let mut w = Vec::new();
    b.keys.retain(|&(a, k)| match GAME_KEYS.iter().find(|(g, _)| *g == k) {
        Some((_, what)) => {
            w.push(format!("{} is the game's ({what}): unbound from {}", key_name(k), a.name()));
            false
        }
        None => true,
    });
    w.extend(b.repair());
    w
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

/// Volumes, 0 to 1. The game's sound plays at `master * sfx`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// OLX's sound volume (`Audio.Volume / 100`).
    pub master: f32,
    /// The game's sound effects.
    pub sfx: f32,
    /// Music (none plays yet).
    pub music: f32,
}

impl Default for AudioSettings {
    /// OLX's default volume, 70.
    /// OLX: src/client/Options.cpp:209 (`Audio.Volume`, 70).
    fn default() -> Self {
        AudioSettings { master: 0.7, sfx: 1.0, music: 0.7 }
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
