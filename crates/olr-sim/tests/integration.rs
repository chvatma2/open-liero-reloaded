//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.
//!
//! Kept as their own binaries:
//! - `no_alloc`: it counts allocations.

#[path = "common/world.rs"]
mod world;

#[path = "common/live.rs"]
mod live_level;

mod live;
mod snapshot;
mod state;
