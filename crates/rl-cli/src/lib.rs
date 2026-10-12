//! `rl-cli`'s library: headless bot matches (spec §7.1–§7.3), which the
//! `match` and `replay` commands and the tests (`tests/matches.rs`,
//! `tests/golden.rs`, `tests/grid.rs`) play.

pub mod assets;
pub mod bundle;
pub mod coverage;
pub mod failure;
pub mod genlevel;
pub mod golden;
pub mod gpu_bench;
pub mod invariants;
pub mod netsim;
pub mod runner;
pub mod spec;
pub mod stress;
pub mod timing;
pub mod verify;

pub use verify::verify;
