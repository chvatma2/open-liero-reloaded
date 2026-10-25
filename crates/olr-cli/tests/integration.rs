//! The crate's integration tests in one binary (one link instead of one
//! per file). A new test file is a module here, or its own `[[test]]` in
//! `Cargo.toml` when it needs a process of its own or xtask runs it by name.
//!
//! Kept as their own binaries:
//! - `golden`: `bless-regression`;
//! - `gpu_bench`: the GPU tier (`check::GPU_TESTS`);
//! - `grid`: the full tier's bot-match grid (`check::TIMED_TESTS`);
//! - `net_cases`: `bless-regression --from`;
//! - `net_sim`: the full tier's online grid (`check::FULL_TESTS`);
//! - `net_soak_memory`: reads the process's memory (process-global state);
//! - `render_golden`: the GPU tier (`check::GPU_TESTS`) and `bless-render`;
//! - `transport`: the full tier's real-transport matches (`check::FULL_TESTS`).

mod common;

mod asset;
mod cli;
mod corpus;
mod deaths;
mod digest;
mod element;
mod feel;
mod hits;
mod level;
mod look_preview;
mod matches;
mod material;
mod mods;
mod net_repro;
mod net_soak;
mod nostr_probe;
mod perfcount;
mod reloaded;
mod render_profile;
mod scenario;
mod skin;
mod stains;
mod state_transfer;
mod stress;
mod tool_output;
