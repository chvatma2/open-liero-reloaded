//! The game's settings (spec §8.3 options, §9.4 `settings.ron`, §21 G2-G3):
//! video, audio, key bindings and gameplay, with the match the local-play
//! menu starts.
//!
//! The match preferences mirror `olr-modes`' and `olr-bots`' types rather
//! than serialize them: the file holds what the menus edit, under names
//! this crate owns, so a change inside those crates never silently changes
//! the file (as `olr-replay`'s `mirror.rs`). Every conversion names every
//! field. Each struct is `#[serde(default)]`: a field missing from a file
//! takes its default.

use std::ops::RangeInclusive;

use olr_input::{Bindings, KeyCode, key_name};
use serde::{Deserialize, Serialize};

use crate::match_prefs::MatchPrefs;
use crate::online::OnlineSettings;

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
    /// The online screens' (plan 10 task 11).
    pub online: OnlineSettings,
    /// The update check's (issue #4).
    pub updates: UpdateSettings,
}

/// What the update check offers (issue #4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// "Include pre-releases": `None` until the player changes it, and
    /// then the installed build's channel (a pre-release build includes
    /// them, a final one does not). Under Flatpak the branch decides.
    pub include_prereleases: Option<bool>,
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
        w.extend(self.online.sanitize());
        w
    }
}

/// The keys the game keeps for itself (`olr-game`'s `on_key` and the
/// menus), with what they do: never bound to an action.
pub const GAME_KEYS: [(KeyCode, &str); 8] = [
    (KeyCode::Escape, "the menus"),
    (KeyCode::F5, "restart"),
    (KeyCode::F2, "the debug panel"),
    (KeyCode::F3, "the network panel"),
    (KeyCode::Tab, "the scores"),
    (KeyCode::KeyH, "the health bars"),
    (KeyCode::PageUp, "fast-forward"),
    (KeyCode::PageDown, "slowing down"),
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
#[serde(from = "VideoFile")]
pub struct VideoSettings {
    /// The window's size when not fullscreen. The window is not resizable:
    /// sizes come only from these presets (and startup flags).
    pub window: WindowPreset,
    /// Borderless fullscreen at the monitor's size (the options offer it
    /// with the window sizes, in place of a desktop-sized window).
    pub fullscreen: bool,
    pub frame_rate: FrameRate,
    pub filter: UpscaleFilter,
    /// Interpolated drawing between ticks (off: the latest tick at whole
    /// pixels, like OLX).
    pub interpolation: bool,
    /// The menus' and the game's panels' scale ([`UI_SCALE_RANGE`]), on
    /// top of the height's (`theme::pixels_per_point`).
    pub ui_scale: f32,
    /// Where the HUD goes on a window wider than 16:9.
    pub hud_layout: HudArrangement,
}

/// Where the HUD goes (the user's ruling after the v0.1 playtest). The
/// game's 16:9 frame and its view are the same in every layout; only the
/// space beside the frame on a window wider than 16:9 differs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HudArrangement {
    /// Side panels when the window has room for them (21:9 and wider),
    /// else the bar.
    #[default]
    Auto,
    /// Everything in the bar under the playfield; black beside the frame.
    Bar,
    /// The health and weapons in a panel left of the frame, the event log
    /// right of it, the minimap in the bar; the bar alone when there is no
    /// room beside the frame.
    Sides,
}

impl HudArrangement {
    pub const ALL: [HudArrangement; 3] = [HudArrangement::Auto, HudArrangement::Bar, HudArrangement::Sides];

    /// The options' name for it.
    pub fn label(self) -> &'static str {
        match self {
            HudArrangement::Auto => "Auto",
            HudArrangement::Bar => "Bottom bar",
            HudArrangement::Sides => "Side panels",
        }
    }
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
            hud_layout: HudArrangement::Auto,
        }
    }
}

/// [`VideoSettings`] as a file holds it: v0.1.0's files may say
/// `window: Desktop`, which is fullscreen now (the user's ruling after the
/// v0.1 playtest), not a reason to reset the file.
#[derive(Deserialize)]
#[serde(default)]
struct VideoFile {
    window: SavedPreset,
    fullscreen: bool,
    frame_rate: FrameRate,
    filter: UpscaleFilter,
    interpolation: bool,
    ui_scale: f32,
    hud_layout: HudArrangement,
}

#[derive(Clone, Copy, Deserialize)]
enum SavedPreset {
    W1280x720,
    W1600x900,
    W1920x1080,
    Desktop,
}

impl Default for VideoFile {
    fn default() -> Self {
        let v = VideoSettings::default();
        VideoFile {
            window: SavedPreset::W1280x720,
            fullscreen: v.fullscreen,
            frame_rate: v.frame_rate,
            filter: v.filter,
            interpolation: v.interpolation,
            ui_scale: v.ui_scale,
            hud_layout: v.hud_layout,
        }
    }
}

impl From<VideoFile> for VideoSettings {
    fn from(f: VideoFile) -> Self {
        let (window, desktop) = match f.window {
            SavedPreset::W1280x720 => (WindowPreset::W1280x720, false),
            SavedPreset::W1600x900 => (WindowPreset::W1600x900, false),
            SavedPreset::W1920x1080 => (WindowPreset::W1920x1080, false),
            SavedPreset::Desktop => (WindowPreset::default(), true),
        };
        VideoSettings {
            window,
            fullscreen: f.fullscreen || desktop,
            frame_rate: f.frame_rate,
            filter: f.filter,
            interpolation: f.interpolation,
            ui_scale: f.ui_scale,
            hud_layout: f.hud_layout,
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

/// The window sizes offered (§21 G3). The screen's size is fullscreen
/// ([`VideoSettings::fullscreen`]): a window that size would put its
/// title bar's height under the taskbar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowPreset {
    #[default]
    W1280x720,
    W1600x900,
    W1920x1080,
}

impl WindowPreset {
    pub const ALL: [WindowPreset; 3] =
        [WindowPreset::W1280x720, WindowPreset::W1600x900, WindowPreset::W1920x1080];

    /// The window's inner size in physical pixels.
    pub fn size(self) -> (u32, u32) {
        match self {
            WindowPreset::W1280x720 => (1280, 720),
            WindowPreset::W1600x900 => (1600, 900),
            WindowPreset::W1920x1080 => (1920, 1080),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WindowPreset::W1280x720 => "1280 × 720",
            WindowPreset::W1600x900 => "1600 × 900",
            WindowPreset::W1920x1080 => "1920 × 1080",
        }
    }
}

/// How frames are paced (spec §8.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameRate {
    /// Mailbox when available, else FIFO.
    #[default]
    VSync,
    /// Present as fast as the GPU draws (`olr --uncapped`).
    Uncapped,
}

/// The upscale filter (`olr_render::UpscaleFilter`).
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameplaySettings {
    // The HUD has one style (plan 7 task 6, the user's call: G5's Classic
    // style was dropped); a file written with `hud: Classic` still loads,
    // the field ignored.
    /// The match local play starts, as last set up.
    pub match_prefs: MatchPrefs,
}
