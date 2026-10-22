//! OLR's game (spec §8.6): a winit window, the 100 Hz fixed-tick
//! loop over the simulation, interpolated wgpu rendering with cosmetic
//! entities, sound, input and an egui overlay. It plays the M5 sandbox
//! (spec §19 E11: one player and idle targets on any level and mod), a
//! Deathmatch against bots (M6), replays and online matches (plan 10).
//!
//! The pure parts (the tick clock, the surface state machine, window
//! requests and the window-system choice, the sandbox setup, scripts,
//! timing) are unit-tested; the window, GPU and devices are
//! checked by running the game (`--autoquit-after`, `--demo`, `--shot`).

/// This build's version: a release's, as `cargo xtask dist` names it
/// (`0.2.0-dev.192.24b2ac4`, or `0.2.0` for a final one), else the
/// workspace's marked as a local build. Shown only (`--version`, About, the
/// main menu, the log): no replay, game list or compatibility check reads it.
pub const VERSION: &str = match option_env!("OLR_BUILD_VERSION") {
    Some(v) => v,
    None => concat!(env!("CARGO_PKG_VERSION"), "-dev (local build)"),
};

pub mod app;
pub mod autoplay;
pub mod clock;
pub mod drive;
pub mod editor;
pub mod finding;
pub mod gpu;
pub mod http;
pub mod hud;
pub mod latency;
pub mod logging;
pub mod net;
pub mod online;
pub mod overlay;
pub mod pointer;
pub mod report;
pub mod script;
pub mod session;
pub mod setup;
pub mod shell;
pub mod speed;
pub mod stats;
pub mod surface;
pub mod throw_sound;
pub mod update;
pub mod video;
pub mod window;
pub mod wsl;
