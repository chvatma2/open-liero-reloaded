//! The fidelity oracle (spec §6).
//!
//! A [`Scenario`] describes a deterministic OLX run: level, mod, seed, worm
//! spawns and a per-frame key schedule. `cargo xtask oracle-record` plays it
//! in a patched OpenLieroX (`tools/olx-oracle/`) and stores the resulting
//! [`Trace`] in `fixtures/oracle/<name>.jsonl.gz`. The Rust simulation later
//! replays the same scenario and [`compare`] reports the first diverging field.

mod compare;
mod keys;
mod scenario;
mod trace;

pub use compare::{Divergence, Fields, compare, compare_fields};
pub use keys::Keys;
pub use scenario::{BonusKind, BonusPlacement, InputChange, Scenario, Tolerance, WormSpawn, WormWeapons};
pub use trace::{
    BonusRec, Frame, Header, Num, ProjRec, RopeRec, ServerBonusRec, ShotRec, SlotRec, Trace, WormRec, gunzip,
    gzip,
};

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
