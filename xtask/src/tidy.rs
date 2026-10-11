//! `cargo xtask tidy`: removes rebuildable bloat from `target/` and old Docker
//! oracle images (and, with `--docker-cache`, bounds Docker's build cache). Never touches `target/content` or `target/olx` (imported
//! content, slow to fetch), nor compiled artifacts other than incremental caches.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{oracle, root};

/// `check` warns when `target/` is bigger than this.
pub const WARN_BYTES: u64 = 25 * 1024 * 1024 * 1024;
/// Measuring gives up after this long.
const MEASURE_BUDGET: Duration = Duration::from_secs(1);

/// Whether a measured size (`None` = gave up) deserves the warning.
pub fn over_threshold(size: Option<u64>) -> bool {
    size.is_some_and(|s| s > WARN_BYTES)
}

/// Directory size in bytes, or `None` when the walk exceeds `budget`.
pub fn dir_size(path: &Path, budget: Duration) -> Option<u64> {
    let start = Instant::now();
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if start.elapsed() > budget {
            return None;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    Some(total)
}

/// Prints a one-line warning when `target/` is large. Never fails.
pub fn warn_if_large() {
    let target = root().join("target");
    let size = dir_size(&target, MEASURE_BUDGET);
    if over_threshold(size) {
        let gb = size.unwrap_or(0) as f64 / (1024.0 * 1024.0 * 1024.0);
        println!("warn target/ is {gb:.0} GB; run `cargo xtask tidy` to reclaim space");
    }
}

/// `target/<profile>/incremental` and `target/<triple>/<profile>/incremental`
/// directories under `target`. Only directories literally named `incremental`
/// that sit exactly one or two levels below `target`'s children are returned.
pub fn incremental_dirs(target: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(target) else { return out };
    for a in rd.flatten() {
        let name = a.file_name();
        if name == "content" || name == "olx" || !a.path().is_dir() {
            continue;
        }
        let inc = a.path().join("incremental");
        if inc.is_dir() {
            out.push(inc);
        }
        if let Ok(rd2) = std::fs::read_dir(a.path()) {
            for b in rd2.flatten() {
                let inc = b.path().join("incremental");
                if b.path().is_dir() && inc.is_dir() {
                    out.push(inc);
                }
            }
        }
    }
    out
}

/// With `docker_cache`, also bounds Docker's build cache (the machine's,
/// not only this project's: opt-in).
pub fn run(docker_cache: bool) -> anyhow::Result<bool> {
    let target = root().join("target");
    let before = dir_size(&target, Duration::from_secs(60));
    for dir in incremental_dirs(&target) {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => println!("removed {}", dir.display()),
            Err(e) => eprintln!("warning: cannot remove {}: {e}", dir.display()),
        }
    }
    oracle::prune_all(docker_cache);
    let after = dir_size(&target, Duration::from_secs(60));
    if let (Some(b), Some(a)) = (before, after) {
        println!("target/: {:.1} GB -> {:.1} GB", b as f64 / 1e9, a as f64 / 1e9);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_decision() {
        assert!(!over_threshold(None));
        assert!(!over_threshold(Some(WARN_BYTES)));
        assert!(over_threshold(Some(WARN_BYTES + 1)));
    }

    #[test]
    fn finds_incremental_dirs_but_not_content() {
        let t = std::env::temp_dir().join(format!("rl-tidy-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        for d in ["debug/incremental", "wasm32/debug/incremental", "content/x/incremental", "olx/incremental"]
        {
            std::fs::create_dir_all(t.join(d)).unwrap();
        }
        let mut got: Vec<String> =
            incremental_dirs(&t).iter().map(|p| p.strip_prefix(&t).unwrap().display().to_string()).collect();
        got.sort();
        assert_eq!(got, ["debug/incremental", "wasm32/debug/incremental"]);
        assert_eq!(dir_size(&t, Duration::from_secs(5)), Some(0));
        let _ = std::fs::remove_dir_all(&t);
    }
}
