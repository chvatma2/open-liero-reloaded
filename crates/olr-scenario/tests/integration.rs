//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.
//!
//! Kept as their own binaries:
//! - `no_alloc`: it counts allocations;
//! - `render_golden`: `bless-render`.

mod oracle;
mod smoke;
