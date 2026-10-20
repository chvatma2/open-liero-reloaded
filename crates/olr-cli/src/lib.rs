//! `olr-cli`'s library: headless bot matches (spec §7.1–§7.3), which the
//! `match` and `replay` commands and the tests (`tests/matches.rs`,
//! `tests/golden.rs`, `tests/grid.rs`) play.

pub mod assets;
pub mod bundle;
pub mod coverage;
pub mod failure;
pub mod feel;
pub mod genlevel;
pub mod golden;
pub mod gpu_bench;
pub mod invariants;
pub mod netsim;
pub mod nostr_probe;
pub mod perfcount;
pub mod quick_bench;
pub mod rollback_cost;
pub mod runner;
pub mod soak;
pub mod spec;
pub mod stress;
pub mod timing;
pub mod transport;
pub mod verify;

pub use verify::verify;
