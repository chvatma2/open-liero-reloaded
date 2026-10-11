//! Menu and HUD logic for rust-liero (spec §8.3, §21 G1-G2): the screen
//! state machine, the settings and profiles and their files, and the egui
//! theme. No GPU handle and no window: the widgets (and `rl-game`, which
//! hosts them) drive [`UiState`] with [`UiAction`]s and carry out its
//! [`AppCommand`]s, so every flow is testable headless.

#![forbid(unsafe_code)]

pub mod match_prefs;
pub mod profiles;
pub mod replays;
pub mod screens;
pub mod settings;
pub mod state;
pub mod store;
pub mod theme;

pub use match_prefs::{
    BotLevel, BotLoadout, BotPersonality, KILL_LIMIT_RANGE, LIVES_RANGE, LOADING_TIME_RANGE, MAX_BOTS,
    MatchPrefs, RulePrefs, TIME_LIMIT_RANGE,
};
pub use profiles::{MAX_NAME_CHARS, MAX_PROFILES, PROFILES_VERSION, Profile, Profiles};
pub use settings::{
    AudioSettings, FrameRate, GameplaySettings, HudStyle, SETTINGS_VERSION, Settings, UI_SCALE_RANGE,
    UpscaleFilter, VideoSettings, WindowPreset, clamp_scale,
};
pub use state::{AppCommand, Game, Screen, UiAction, UiState};
pub use store::{Loaded, PROFILES_FILE, REPLAYS_DIR, SETTINGS_FILE, Store};
