//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.
//!
//! Kept as their own binaries:
//! - `soak` and `canvas_gpu`: the GPU tier (`check::GPU_TESTS`).

mod deathmatch;
mod downloads;
mod editor_host;
mod finding;
mod ingame;
mod join;
mod locals;
mod menu;
mod mod_editor;
mod online;
mod options;
mod profiles;
mod report;
mod run_log;
mod skin_editor;
mod speed;
mod throw_sound;
mod update;
mod user_packs;
mod watch;
mod weapon_name;
