//! `cargo xtask tidy`: removes rebuildable bloat from `target/` and old Docker
//! oracle images (and, with `--docker-cache`, bounds Docker's build cache). Never touches `content/packs` or `target/olx` (imported
//! content, slow to fetch), nor compiled artifacts other than incremental caches
//! and stale per-configuration executables in `deps/`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{oracle, root};

/// `check` warns when `target/` is bigger than this.
pub const WARN_BYTES: u64 = 25 * 1024 * 1024 * 1024;
/// Executables kept per name in `deps/` (one per live build configuration).
const KEEP_EXECUTABLES: usize = 3;
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
    profile_dirs(target, "incremental")
}

/// `target/<profile>/<leaf>` and `target/<triple>/<profile>/<leaf>`, skipping
/// the imported content.
fn profile_dirs(target: &Path, leaf: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(target) else { return out };
    for a in rd.flatten() {
        let name = a.file_name();
        if name == "content" || name == "olx" || !a.path().is_dir() {
            continue;
        }
        let dir = a.path().join(leaf);
        if dir.is_dir() {
            out.push(dir);
        }
        if let Ok(rd2) = std::fs::read_dir(a.path()) {
            for b in rd2.flatten() {
                let dir = b.path().join(leaf);
                if b.path().is_dir() && dir.is_dir() {
                    out.push(dir);
                }
            }
        }
    }
    out
}

/// The name of a linked executable in `deps/` without its hash: `stem-<16 hex>`
/// with no extension (libraries and `.d` files have one).
fn executable_stem(name: &str) -> Option<&str> {
    let (stem, hash) = name.rsplit_once('-')?;
    (hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && !stem.is_empty()).then_some(stem)
}

/// Removes all but the `keep` newest executables of each stem in a `deps/`
/// directory. Test and binary executables pile up one per build
/// configuration (profile, features, flags) and are never reused once the
/// configuration changes; removing a live one only costs a relink.
/// Returns the removed paths.
pub fn prune_stale_executables(deps: &Path, keep: usize) -> Vec<PathBuf> {
    let mut by_stem: std::collections::BTreeMap<String, Vec<(std::time::SystemTime, PathBuf)>> =
        Default::default();
    let Ok(rd) = std::fs::read_dir(deps) else { return Vec::new() };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(stem) = name.to_str().and_then(executable_stem) else { continue };
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        by_stem.entry(stem.to_string()).or_default().push((mtime, e.path()));
    }
    let mut removed = Vec::new();
    for mut files in by_stem.into_values() {
        files.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, path) in files.into_iter().skip(keep) {
            if std::fs::remove_file(&path).is_ok() {
                removed.push(path);
            }
        }
    }
    removed
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
    let mut pruned = 0;
    for deps in profile_dirs(&target, "deps") {
        pruned += prune_stale_executables(&deps, KEEP_EXECUTABLES).len();
    }
    println!("removed {pruned} stale executables");
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

    #[test]
    fn prunes_stale_executables_keeping_the_newest() {
        let t = std::env::temp_dir().join(format!("rl-tidy-deps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let deps = t.join("debug/deps");
        std::fs::create_dir_all(&deps).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let names = [
            "rl_game-0000000000000001",
            "rl_game-0000000000000002",
            "rl_game-0000000000000003",
            "rl_game-0000000000000004",
            "rl_game-0000000000000005",
            "store-00000000000000aa",
            "librl_game-0000000000000001.rlib",
            "rl_game-0000000000000001.d",
        ];
        for (i, n) in names.iter().enumerate() {
            let f = std::fs::File::create(deps.join(n)).unwrap();
            f.set_modified(old + Duration::from_secs(i as u64)).unwrap();
        }
        let removed = prune_stale_executables(&deps, 3);
        assert_eq!(removed.len(), 2);
        let mut left: Vec<String> = std::fs::read_dir(&deps)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "librl_game-0000000000000001.rlib",
                "rl_game-0000000000000001.d",
                "rl_game-0000000000000003",
                "rl_game-0000000000000004",
                "rl_game-0000000000000005",
                "store-00000000000000aa",
            ]
        );
        let _ = std::fs::remove_dir_all(&t);
    }
}
