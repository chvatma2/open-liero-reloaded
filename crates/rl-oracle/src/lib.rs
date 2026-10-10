//! The fidelity oracle (spec §6).
//!
//! A [`Scenario`] describes a deterministic OLX run: level, mod, seed, worm
//! spawns and a per-frame key schedule. `cargo xtask oracle-record` plays it
//! in a patched OpenLieroX (`tools/olx-oracle/`) and stores the resulting
//! [`Trace`] in `fixtures/oracle/<name>.jsonl.gz`. The Rust simulation later
//! replays the same scenario and [`compare`] reports the first diverging field.

mod keys;
mod scenario;

pub use keys::Keys;
pub use scenario::{InputChange, Scenario, Tolerance, WormSpawn};

/// Errors from reading scenarios and traces.
#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("{0}")]
    Scenario(String),
    #[error("trace line {line}: {message}")]
    Trace { line: usize, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
