//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.

mod common;

mod netsim;
mod play;
mod session;
mod sync;
mod timing;
mod world_sim;
