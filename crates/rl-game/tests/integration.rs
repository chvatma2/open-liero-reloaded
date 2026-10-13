//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.
//!
//! Kept as their own binaries:
//! - `soak`: the GPU tier (`check::GPU_TESTS`).

mod deathmatch;
mod downloads;
mod finding;
mod import;
mod ingame;
mod menu;
mod online;
mod options;
mod profiles;
mod speed;
mod watch;
mod weapon_name;
