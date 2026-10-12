//! Menu and HUD logic for rust-liero (spec §8.3, §21 G1-G2): the screen
//! state machine, the settings and profiles and their files, and the egui
//! theme. No GPU handle and no window: the widgets (and `rl-game`, which
//! hosts them) drive [`UiState`] with [`UiAction`]s and carry out its
//! [`AppCommand`]s, so every flow is testable headless.

#![forbid(unsafe_code)]

pub mod hud;
pub mod import;
pub mod match_prefs;
pub mod profiles;
pub mod replays;
pub mod screens;
pub mod settings;
pub mod skin_preview;
pub mod state;
pub mod store;
pub mod theme;
pub mod weapon_select;

pub use match_prefs::{
    BotLevel, BotLoadout, BotPersonality, KILL_LIMIT_RANGE, LIVES_RANGE, LOADING_TIME_RANGE, MAX_BOTS,
    MatchPrefs, RulePrefs, TIME_LIMIT_RANGE,
};
pub use profiles::{
    CPU_COLOURS, DEFAULT_WEAPONS, MAX_NAME_CHARS, MAX_PROFILES, NEW_COLOURS, NameError, PROFILES_VERSION,
    Profile, Profiles, bot_colours,
};
pub use settings::{
    AudioSettings, FrameRate, GAME_KEYS, GameplaySettings, HudArrangement, ImportSettings, SETTINGS_VERSION,
    Settings, UI_SCALE_RANGE, UpscaleFilter, VideoSettings, WindowPreset, clamp_scale, sanitize_bindings,
};
pub use state::{AppCommand, Game, Screen, UiAction, UiState};
pub use store::{
    IDENTITY_FILE, IDENTITY_VERSION, Loaded, PACKS_DIR, PROFILES_FILE, REPLAYS_DIR, SETTINGS_FILE, Store,
};
