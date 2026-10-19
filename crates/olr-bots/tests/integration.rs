//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.

mod common;

mod aim;
mod bot;
mod brain;
mod chaos;
mod idle;
mod matches;
mod motor;
mod nav;
mod rope;
