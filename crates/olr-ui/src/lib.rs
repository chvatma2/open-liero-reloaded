//! Menu and HUD logic for OLR (spec §8.3, §21 G1-G2): the screen
//! state machine, the settings and profiles and their files, and the egui
//! theme. No GPU handle and no window: the widgets (and `olr-game`, which
//! hosts them) drive [`UiState`] with [`UiAction`]s and carry out its
//! [`AppCommand`]s, so every flow is testable headless.

#![forbid(unsafe_code)]

pub mod hud;
pub mod loadouts;
pub mod match_prefs;
pub mod online;
pub mod profiles;
pub mod replays;
pub mod report;
pub mod screens;
pub mod settings;
pub mod skin_preview;
pub mod state;
pub mod store;
pub mod theme;
pub mod update;
pub mod weapon_select;

pub use loadouts::{LOADOUTS_FILE, OwnLoadouts};
pub use match_prefs::{
    BotLevel, BotLoadout, BotPersonality, DEFAULT_MOD, KILL_LIMIT_RANGE, LIVES_RANGE, LOADING_TIME_RANGE,
    MAX_BOTS, MatchPrefs, RulePrefs, TIME_LIMIT_RANGE,
};
pub use online::{
    ADDRESS_CHARS, CHAT_CHARS, CHAT_KEY, DEFAULT_PORT, DownloadCacheView, DownloadView, FindingSettings,
    HostPrefs, InternetEntry, InternetStatus, InternetView, LanEntry, LobbyCommand, LobbyView, MAX_PLAYERS,
    MAX_SERVERS, MAX_TURN, MIGRATED_LISTING, MIN_PLAYERS, MemberView, OnlineSettings, PASSWORD_CHARS,
    PHASE_LOBBY, Password, PingView, Region, SEAT_SPECTATOR, Visibility, is_invite, key_hex, megabytes,
    parse_key_hex, turn_parts, turn_shown, valid_relay, valid_stun, valid_turn,
};
pub use profiles::{
    CPU_COLOURS, DEFAULT_WEAPONS, MAX_NAME_CHARS, MAX_PROFILES, NEW_COLOURS, NameError, PROFILES_VERSION,
    Profile, Profiles, bot_colours,
};
pub use settings::{
    AudioSettings, BLOOD_RANGE, BLOOD_STEP, FrameRate, GAME_KEYS, GameplaySettings, HudArrangement,
    SETTINGS_VERSION, Settings, UI_SCALE_RANGE, UpdateSettings, UpscaleFilter, VideoSettings, WindowPreset,
    clamp_scale, sanitize_bindings,
};
pub use state::{AppCommand, Game, Screen, UiAction, UiState};
pub use store::{
    CACHE_DIR, DOWNLOADS_DIR, IDENTITY_FILE, IDENTITY_VERSION, Loaded, PACKS_DIR, PROFILES_FILE, REPLAYS_DIR,
    SETTINGS_FILE, Secret, Store,
};
