//! Failure capture (spec §7.3): a failing match writes its replay and a
//! record of what failed (its spec, seed and tick) to `target/failures/`.
//! `rl-cli replay <replay> --until <tick> --dump` shows the world before
//! the failing tick; `cargo xtask bless-regression --from <record>` makes
//! the match a golden once it is fixed.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use rl_replay::Replay;
use serde::{Deserialize, Serialize};

use crate::runner::Failure;
use crate::spec::MatchSpec;

/// What `<stem>.ron` holds next to `<stem>.rlrp`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FailureRecord {
    /// The match (its seed included): `rl-cli match` with these options plays it again.
    pub spec: MatchSpec,
    /// The failing tick (0-based).
    pub tick: u32,
    pub kind: String,
    pub message: String,
    /// The replay's file name, next to this record.
    pub replay: String,
}

impl FailureRecord {
    pub fn load(path: &Path) -> anyhow::Result<FailureRecord> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// Writes `replay` and the record of `failure` of the match `spec` to
/// `dir`; gives the record's path.
pub fn write(dir: &Path, spec: &MatchSpec, failure: &Failure, replay: &Replay) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let stem = format!("{}-{:?}-tick{}", spec.stem(), failure.kind, failure.tick).to_lowercase();
    let replay_path = dir.join(format!("{stem}.rlrp"));
    std::fs::write(&replay_path, replay.encode())
        .with_context(|| format!("writing {}", replay_path.display()))?;
    let record = FailureRecord {
        spec: spec.clone(),
        tick: failure.tick,
        kind: format!("{:?}", failure.kind),
        message: failure.message.clone(),
        replay: format!("{stem}.rlrp"),
    };
    let text = ron::ser::to_string_pretty(&record, ron::ser::PrettyConfig::default())?;
    let path = dir.join(format!("{stem}.ron"));
    std::fs::write(&path, text + "\n").with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}
