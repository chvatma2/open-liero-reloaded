//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.
//!
//! Kept as their own binaries:
//! - `hud_snapshots`: the GPU tier (`check::GPU_TESTS`) and `bless-render`.

mod editor;
mod flows;
mod ingame;
mod join;
mod layout;
mod loadouts;
mod online;
mod options;
mod pad;
mod profiles;
mod report;
mod state;
mod store;
mod update;
