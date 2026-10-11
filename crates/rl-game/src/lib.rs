//! rust-liero's game (spec §8.6): a winit window, the 100 Hz fixed-tick
//! loop over the simulation, interpolated wgpu rendering with cosmetic
//! entities, sound, input and an egui overlay. It plays the M5 sandbox
//! (spec §19 E11: one player and idle targets on any level and mod), a
//! Deathmatch against bots (M6) and replays.
//!
//! The pure parts (the tick clock, the surface state machine, window
//! requests and the window-system choice, the sandbox setup, scripts,
//! timing) are unit-tested; the window, GPU and devices are
//! checked by running the game (`--autoquit-after`, `--demo`, `--shot`).

pub mod app;
pub mod autoplay;
pub mod clock;
pub mod drive;
pub mod gpu;
pub mod hud;
pub mod latency;
pub mod overlay;
pub mod script;
pub mod session;
pub mod setup;
pub mod shell;
pub mod speed;
pub mod stats;
pub mod surface;
pub mod video;
pub mod window;
pub mod wsl;
