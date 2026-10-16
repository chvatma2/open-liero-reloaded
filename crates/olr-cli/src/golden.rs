//! Golden match checksums (spec §7.2): `fixtures/matches/<name>.ron` holds
//! a match's spec and the checksum stream its replay recorded (before the
//! first tick, every 100 ticks and at the end). `cargo xtask
//! bless-regression --reason` rewrites them.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::runner::MatchRun;
use crate::spec::MatchSpec;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Golden {
    pub spec: MatchSpec,
    /// How the match ended, for a reader.
    #[serde(default)]
    pub end: String,
    #[serde(default)]
    pub ticks: u32,
    /// `(tick, checksum)` as the replay records them.
    #[serde(default)]
    pub checksums: Vec<(u32, u64)>,
}

impl Golden {
    /// What `run` played.
    pub fn of(run: &MatchRun) -> Golden {
        Golden {
            spec: run.spec.clone(),
            end: run.end.to_string(),
            ticks: run.ticks,
            checksums: run.replay.checksums.clone(),
        }
    }

    pub fn load(path: &Path) -> anyhow::Result<Golden> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Every `*.ron` in `dir`, by file name.
    pub fn load_dir(dir: &Path) -> anyhow::Result<Vec<(PathBuf, Golden)>> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<_, _>>()?;
        paths.retain(|p| p.extension().is_some_and(|e| e == "ron"));
        paths.sort();
        paths.into_iter().map(|p| Golden::load(&p).map(|g| (p, g))).collect()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text + "\n").with_context(|| format!("writing {}", path.display()))
    }
}

/// How `actual` differs from `expected`: the first checksum that differs,
/// or the length.
pub fn first_difference(expected: &Golden, actual: &Golden) -> Option<String> {
    if expected.spec != actual.spec {
        return Some("the spec differs".into());
    }
    for (e, a) in expected.checksums.iter().zip(&actual.checksums) {
        if e != a {
            return Some(format!(
                "checksum after {} ticks is {:#018x}, recorded {:#018x} (at tick {})",
                a.0, a.1, e.1, e.0
            ));
        }
    }
    if expected.checksums.len() != actual.checksums.len() || expected.ticks != actual.ticks {
        return Some(format!(
            "{} ticks and {} checksums ({}), recorded {} and {} ({})",
            actual.ticks,
            actual.checksums.len(),
            actual.end,
            expected.ticks,
            expected.checksums.len(),
            expected.end
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bless-regression --from` copies a failure record into the goldens:
    /// it loads as a golden with no stream yet, which differs until blessed.
    #[test]
    fn a_failure_record_loads_as_a_pending_golden() {
        let spec = MatchSpec::new("olx-classic", "olx-level-dirt-level", 4, 9, 300);
        let record = crate::failure::FailureRecord {
            spec: spec.clone(),
            tick: 12,
            kind: "Panic".into(),
            message: "boom".into(),
            replay: "x.rlrp".into(),
        };
        let text = ron::ser::to_string_pretty(&record, ron::ser::PrettyConfig::default()).unwrap();
        let golden: Golden = ron::from_str(&text).unwrap();
        assert_eq!((golden.spec.clone(), golden.checksums.len()), (spec.clone(), 0));
        let played =
            Golden { end: "tick limit".into(), ticks: 300, checksums: vec![(0, 1), (100, 2)], ..golden };
        assert!(first_difference(&played, &played).is_none());
        let mut other = played.clone();
        other.checksums[1].1 = 3;
        let d = first_difference(&played, &other).unwrap();
        assert!(d.contains("after 100 ticks"), "{d}");
        let pending = Golden { spec, end: String::new(), ticks: 0, checksums: vec![] };
        assert!(first_difference(&pending, &played).unwrap().contains("recorded 0"));
    }
}
