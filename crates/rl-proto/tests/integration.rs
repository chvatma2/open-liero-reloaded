//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.

#[path = "../src/fingerprint_impl.rs"]
mod fingerprint_impl;

mod fingerprint;
mod fuzz;
mod fuzz_lobby;
