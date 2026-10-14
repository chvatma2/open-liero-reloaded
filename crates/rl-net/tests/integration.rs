//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.
//!
//! Kept as their own binaries:
//! - `link`: the full tier's 10 000 datagrams (`check::FULL_TESTS`).

mod candidates;
mod core_wire;
mod endpoint;
mod fuzz;
mod lan;
mod natsim;
