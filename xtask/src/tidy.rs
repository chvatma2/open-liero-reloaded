//! `cargo xtask tidy`: sweeps superseded build output from `target/` and old
//! Docker oracle images (and, with `--docker-cache`, bounds Docker's build
//! cache). `check` runs the same `target/` sweep by itself when `target/` is
//! bigger than [`SWEEP_BYTES`].
//!
//! Cargo never deletes an artifact whose configuration changed (a profile,
//! a feature set, a dependency in `Cargo.lock`, the toolchain): the new
//! one gets a new hash beside the old. The sweep keeps, in every `deps/`
//! directory of a cargo profile under `target/`, the newest build of each
//! artifact and removes the older ones, and in every `incremental/` the
//! newest sessions of each crate. Only files and directories inside
//! `target/` are removed; never `content/packs`, `target/olx`, `target/dist`
//! (the release zips, [`KEPT`]) or anything
//! a symbolic link points to.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::{oracle, root};

/// `check` sweeps `target/` when it is bigger than this.
pub const SWEEP_BYTES: u64 = 20 * 1024 * 1024 * 1024;
/// Measuring `target/` gives up after this long (and then sweeps: a tree
/// that slow to walk is a big one).
const MEASURE_BUDGET: Duration = Duration::from_secs(10);
/// Builds of one artifact written this close to its newest are the same
/// cargo run's parallel configurations (a crate built for build scripts
/// and for the game with the same features), kept with it.
pub const SAME_RUN: Duration = Duration::from_secs(10 * 60);

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Whether a measured size (`None` = the walk gave up) deserves a sweep.
pub fn over_threshold(size: Option<u64>) -> bool {
    size.is_none_or(|s| s > SWEEP_BYTES)
}

/// Directory size in bytes, or `None` when the walk exceeds `budget`.
/// Symbolic links are counted as links, never followed; a file hard-linked
/// twice (cargo links each binary from `deps/` into its profile directory)
/// counts once, as `du` does.
pub fn dir_size(path: &Path, budget: Duration) -> Option<u64> {
    let start = Instant::now();
    let mut total = 0u64;
    let mut linked = std::collections::BTreeSet::new();
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
            } else if let Ok(m) = e.metadata()
                && first_link(&m, &mut linked)
            {
                total += m.len();
            }
        }
    }
    Some(total)
}

/// Whether `m` is the first of its hard links met (`seen`: those met).
#[cfg(unix)]
fn first_link(m: &std::fs::Metadata, seen: &mut std::collections::BTreeSet<(u64, u64)>) -> bool {
    use std::os::unix::fs::MetadataExt;
    m.nlink() < 2 || seen.insert((m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn first_link(_: &std::fs::Metadata, _: &mut std::collections::BTreeSet<(u64, u64)>) -> bool {
    true
}

/// Directories of `target/` the sweep never enters: `dist`, the `dist`
/// profile's output, holds the release zips (`cargo xtask dist`).
pub const KEPT: [&str; 1] = ["dist"];

/// The cargo profile directories under `target` (`target/debug`,
/// `target/<triple>/release`, a custom target directory's, ...): the
/// directories at most three levels down that hold a `.fingerprint`,
/// but not those in [`KEPT`].
pub fn profile_dirs(target: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(target.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if KEPT.iter().any(|k| dir == target.join(k)) {
            continue;
        }
        if dir.join(".fingerprint").is_dir() {
            out.push(dir);
            continue;
        }
        if depth == 3 {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    out.sort();
    out
}

/// A `deps/` file name split as `(stem, hash, extension)`: `libsyn-<16
/// hex>.rlib` is `("syn", hash, ".rlib")`, `rl_cli-<16 hex>` is
/// `("rl_cli", hash, "")`. `None` for names without the hash.
pub fn split_artifact(name: &str) -> Option<(&str, &str, &str)> {
    let (front, rest) = name.rsplit_once('-')?;
    let hash = rest.get(..16)?;
    let ext = &rest[16..];
    if !hash.bytes().all(|b| b.is_ascii_hexdigit()) || !(ext.is_empty() || ext.starts_with('.')) {
        return None;
    }
    let stem = if ext.is_empty() || ext == ".d" || ext == ".exe" || ext == ".wasm" {
        front
    } else {
        front.strip_prefix("lib").unwrap_or(front)
    };
    (!stem.is_empty()).then_some((stem, hash, ext))
}

/// A crate root's path with each registry `name-x.y.z` component reduced
/// to its semver-compatible series (`syn-2.0.119` to `syn-2`, `windows-sys-0.59.0`
/// to `windows-sys-0.59`): an upgraded dependency supersedes its old builds,
/// while two incompatible versions (`syn` 2 and 3) are both kept.
pub fn version_series(path: &str) -> String {
    path.split('/').map(component_series).collect::<Vec<_>>().join("/")
}

/// `name-x.y.z[-pre][+build]` as `name-x` (x > 0), `name-0.y` (y > 0) or
/// itself; any other component as itself.
fn component_series(c: &str) -> String {
    for (i, _) in c.match_indices('-') {
        let (name, version) = (&c[..i], &c[i + 1..]);
        let core = version.split(['-', '+']).next().unwrap_or("");
        let parts: Vec<&str> = core.split('.').collect();
        let numeric = |p: &&str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
        if name.is_empty() || parts.len() != 3 || !parts.iter().all(numeric) {
            continue;
        }
        return match (parts[0], parts[1]) {
            ("0", "0") => c.to_string(),
            ("0", minor) => format!("{name}-0.{minor}"),
            (major, _) => format!("{name}-{major}"),
        };
    }
    c.to_string()
}

/// The crate root a `.d` file names first (rustc lists the root first), or
/// `""` when there is none to read.
fn crate_root(dep_info: &Path) -> String {
    let mut line = String::new();
    let Ok(f) = std::fs::File::open(dep_info) else { return String::new() };
    if std::io::BufReader::new(f).read_line(&mut line).is_err() {
        return String::new();
    }
    let Some((_, sources)) = line.split_once(": ") else { return String::new() };
    // Spaces inside a path are escaped as `\ `.
    let mut root = String::new();
    let mut chars = sources.trim_end().chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.next() {
                Some(' ') => root.push(' '),
                Some(other) => {
                    root.push('\\');
                    root.push(other);
                }
                None => root.push('\\'),
            },
            ' ' => break,
            _ => root.push(ch),
        }
    }
    root
}

/// What a build produced, by its main file: a library, a proc-macro or
/// other dynamic library, an executable, or metadata only (`cargo check`).
fn kind(exts: &[&str]) -> String {
    const MAIN: [&str; 8] = [".rlib", ".so", ".dylib", ".dll", "", ".exe", ".wasm", ".rmeta"];
    match MAIN.iter().find(|m| exts.contains(m)) {
        Some(m) => (*m).to_string(),
        None => exts.join("+"),
    }
}

/// `mtime` of a path, or the epoch when unreadable (oldest: swept first).
fn modified(meta: &std::fs::Metadata) -> SystemTime {
    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)
}

/// `.fingerprint/<package>-<hash>` directories by hash.
fn fingerprint_dirs(profile: &Path) -> BTreeMap<String, PathBuf> {
    std::fs::read_dir(profile.join(".fingerprint"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let (_, hash) = name.rsplit_once('-')?;
            Some((hash.to_string(), e.path()))
        })
        .collect()
}

/// What cargo's fingerprint (`.fingerprint/<package>-<hash>/<unit>.json`)
/// says about a build: its unit (`lib-syn`, `bin-rl-cli`,
/// `test-bin-rl-cli`, `test-integration-test-soak`) and its features.
/// Empty when there is none to read.
fn fingerprint(dir: Option<&PathBuf>) -> (String, String) {
    let Some(dir) = dir else { return Default::default() };
    let json = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .min();
    let Some(json) = json else { return Default::default() };
    let unit = json.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let text = std::fs::read_to_string(&json).unwrap_or_default();
    (unit, json_string(&text, "features").unwrap_or_default().to_string())
}

/// The raw value (escapes kept) of the string field `key` in `json`.
fn json_string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let open = format!("\"{key}\":\"");
    let rest = &json[json.find(&open)? + open.len()..];
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some(&rest[..i]),
            _ => escaped = false,
        }
    }
    None
}

/// One build of something: what it is (`key`), when it was last written
/// and its files.
#[derive(Clone)]
struct Build<K> {
    key: K,
    time: SystemTime,
    paths: Vec<PathBuf>,
}

/// The paths of the builds older than their key's newest by more than
/// `window`.
fn superseded<K: Ord>(builds: Vec<Build<K>>, window: Duration) -> Vec<PathBuf> {
    let mut newest: BTreeMap<&K, SystemTime> = BTreeMap::new();
    for b in &builds {
        let n = newest.entry(&b.key).or_insert(b.time);
        *n = (*n).max(b.time);
    }
    let cutoffs: Vec<SystemTime> =
        builds.iter().map(|b| newest[&b.key].checked_sub(window).unwrap_or(SystemTime::UNIX_EPOCH)).collect();
    builds.into_iter().zip(cutoffs).filter(|(b, cut)| b.time < *cut).flat_map(|(b, _)| b.paths).collect()
}

/// The `deps/` files of a cargo profile directory's superseded builds. A
/// build is its files `<stem>-<hash>.*`; per artifact, its key (stem, kind,
/// crate root from its `.d` file and, from its fingerprint, unit and
/// features), every build older than the key's newest by more than
/// `window` is superseded. A build without its `.d` (an interrupted build,
/// a sweep by hand) is superseded by a newer build of its stem and kind.
/// Names without a hash (`rustcXXXX` temporaries) are left alone.
pub fn stale_artifacts(profile: &Path, window: Duration) -> Vec<PathBuf> {
    let deps = profile.join("deps");
    // Keyed by (stem, hash): the extensions seen and the build.
    let mut builds: BTreeMap<(String, String), (Vec<String>, Build<()>)> = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(&deps) else { return Vec::new() };
    for e in rd.flatten() {
        let Ok(name) = e.file_name().into_string() else { continue };
        let Some((stem, hash, ext)) = split_artifact(&name) else { continue };
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let (exts, b) = builds.entry((stem.to_string(), hash.to_string())).or_insert_with(|| {
            (Vec::new(), Build { key: (), time: SystemTime::UNIX_EPOCH, paths: Vec::new() })
        });
        exts.push(ext.to_string());
        b.time = b.time.max(modified(&meta));
        b.paths.push(e.path());
    }
    let fingerprints = fingerprint_dirs(profile);
    let mut known = Vec::new();
    let mut unknown = Vec::new();
    let mut newest_of_kind: BTreeMap<(String, String), SystemTime> = BTreeMap::new();
    for ((stem, hash), (exts, b)) in builds {
        let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
        let kind = kind(&exts);
        let root = crate_root(&deps.join(format!("{stem}-{hash}.d")));
        if root.is_empty() {
            unknown.push(Build { key: (stem, kind), time: b.time, paths: b.paths });
            continue;
        }
        let n = newest_of_kind.entry((stem.clone(), kind.clone())).or_insert(b.time);
        *n = (*n).max(b.time);
        let (unit, features) = fingerprint(fingerprints.get(&hash));
        known.push(Build {
            key: (stem, kind, version_series(&root), unit, features),
            time: b.time,
            paths: b.paths,
        });
    }
    let mut stale = superseded(known, window);
    for b in unknown {
        let cutoff = newest_of_kind.get(&b.key).and_then(|n| n.checked_sub(window));
        if cutoff.is_some_and(|c| b.time < c) {
            stale.extend(b.paths);
        }
    }
    stale
}

/// The `incremental/` sessions (`<crate>-<hash>/s-*` directories, with their
/// `.lock` files) older than their crate's newest session by more than
/// `window`, or the whole `<crate>-<hash>` directory when all its sessions
/// are.
pub fn stale_sessions(incremental: &Path, window: Duration) -> Vec<PathBuf> {
    // Per crate directory, its sessions keyed by crate name, with their locks.
    let mut dirs: Vec<(PathBuf, Vec<Build<String>>)> = Vec::new();
    let Ok(rd) = std::fs::read_dir(incremental) else { return Vec::new() };
    for e in rd.flatten() {
        let Ok(name) = e.file_name().into_string() else { continue };
        let Some((krate, _)) = name.rsplit_once('-') else { continue };
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let mut sessions = Vec::new();
        for s in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
            let Ok(sname) = s.file_name().into_string() else { continue };
            let Ok(meta) = s.metadata() else { continue };
            if !sname.starts_with("s-") || !meta.is_dir() {
                continue;
            }
            let mut paths = vec![s.path()];
            if let Some((stem, _)) = sname.rsplit_once('-') {
                let lock = e.path().join(format!("{stem}.lock"));
                if lock.is_file() {
                    paths.push(lock);
                }
            }
            sessions.push(Build { key: krate.to_string(), time: modified(&meta), paths });
        }
        dirs.push((e.path(), sessions));
    }
    let all: Vec<_> = dirs.iter().flat_map(|(_, s)| s.iter().cloned()).collect();
    let stale = superseded(all, window);
    let mut out = Vec::new();
    for (dir, sessions) in dirs {
        let gone = |s: &Build<String>| stale.contains(&s.paths[0]);
        if !sessions.is_empty() && sessions.iter().all(gone) {
            out.push(dir);
        } else {
            out.extend(sessions.iter().filter(|s| gone(s)).flat_map(|s| s.paths.iter().cloned()));
        }
    }
    out
}

/// What a sweep removed.
#[derive(Debug, Default, PartialEq)]
pub struct Swept {
    pub artifacts: usize,
    pub sessions: usize,
    pub bytes: u64,
}

/// Removes every stale artifact and incremental session under `target`.
pub fn sweep(target: &Path, window: Duration) -> Swept {
    let mut swept = Swept::default();
    for profile in profile_dirs(target) {
        for path in stale_artifacts(&profile, window) {
            let size = std::fs::symlink_metadata(&path).map_or(0, |m| m.len());
            if path.starts_with(target) && std::fs::remove_file(&path).is_ok() {
                swept.artifacts += 1;
                swept.bytes += size;
            }
        }
        for path in stale_sessions(&profile.join("incremental"), window) {
            if !path.starts_with(target) {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            let size = if meta.is_dir() { dir_size(&path, Duration::MAX).unwrap_or(0) } else { meta.len() };
            let removed =
                if meta.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
            if removed.is_ok() {
                swept.bytes += size;
                if meta.is_dir() {
                    swept.sessions += 1;
                }
            }
        }
    }
    swept
}

fn describe(s: &Swept) -> String {
    format!(
        "freed {:.1} GB ({} stale artifacts, {} incremental sessions)",
        s.bytes as f64 / GB,
        s.artifacts,
        s.sessions
    )
}

/// `check`'s first step: sweeps `target/` when it is bigger than
/// [`SWEEP_BYTES`] and says in one line what it freed. Never fails.
pub fn sweep_if_large() {
    let target = root().join("target");
    let size = dir_size(&target, MEASURE_BUDGET);
    if !target.is_dir() || !over_threshold(size) {
        return;
    }
    let was = size.map_or_else(|| "too big to measure".to_string(), |s| format!("{:.1} GB", s as f64 / GB));
    let swept = sweep(&target, SAME_RUN);
    println!("tidy target/ was {was} (over {} GB): {}", SWEEP_BYTES as f64 / GB, describe(&swept));
}

/// With `docker_cache`, also bounds Docker's build cache (the machine's,
/// not only this project's: opt-in).
pub fn run(docker_cache: bool) -> anyhow::Result<bool> {
    let target = root().join("target");
    let before = dir_size(&target, Duration::from_secs(60));
    let swept = sweep(&target, SAME_RUN);
    println!("{}", describe(&swept));
    oracle::prune_all(docker_cache);
    let after = dir_size(&target, Duration::from_secs(60));
    if let (Some(b), Some(a)) = (before, after) {
        println!("target/: {:.1} GB -> {:.1} GB", b as f64 / GB, a as f64 / GB);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(600);

    /// Creates `path` (and its parents) under `dir`, modified at `t0 + secs`.
    fn file(dir: &Path, path: &str, secs: u64, body: &str) {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(t0() + Duration::from_secs(secs)).unwrap();
    }

    fn t0() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    /// A build in the cargo profile directory `profile`: `stem`-`hash`
    /// files with the extensions `exts`, a `.d` naming `root` (none when
    /// empty) and, with `fp`, a fingerprint `(unit, features)`, all
    /// modified at `secs`.
    fn build(
        profile: &Path,
        stem: &str,
        hash: &str,
        exts: &[&str],
        root: &str,
        fp: Option<(&str, &str)>,
        secs: u64,
    ) {
        let deps = profile.join("deps");
        for ext in exts {
            let lib = if matches!(*ext, ".rlib" | ".rmeta" | ".so") { "lib" } else { "" };
            file(&deps, &format!("{lib}{stem}-{hash}{ext}"), secs, "");
        }
        if !root.is_empty() {
            let d = deps.join(format!("{stem}-{hash}.d"));
            let body = format!("{}: {root} src/x.rs\n\n{root}:\n", d.display());
            file(&deps, &format!("{stem}-{hash}.d"), secs, &body);
        }
        if let Some((unit, features)) = fp {
            let features = features.replace('"', "\\\"");
            let json =
                format!(r#"{{"rustc":1,"features":"{features}","declared_features":"[]","target":2}}"#);
            file(profile, &format!(".fingerprint/{stem}-{hash}/{unit}.json"), secs, &json);
        }
    }

    fn names(base: &Path, paths: Vec<PathBuf>) -> Vec<String> {
        let mut v: Vec<String> =
            paths.iter().map(|p| p.strip_prefix(base).unwrap().display().to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn sweep_threshold() {
        assert!(over_threshold(None), "a walk that gives up means a big tree");
        assert!(!over_threshold(Some(SWEEP_BYTES)));
        assert!(over_threshold(Some(SWEEP_BYTES + 1)));
    }

    #[test]
    fn splits_artifact_names() {
        assert_eq!(
            split_artifact("libsyn-95ae3d892da4cdb2.rlib"),
            Some(("syn", "95ae3d892da4cdb2", ".rlib"))
        );
        assert_eq!(split_artifact("syn-95ae3d892da4cdb2.d"), Some(("syn", "95ae3d892da4cdb2", ".d")));
        assert_eq!(split_artifact("rl_cli-b6d7acdba407c3c6"), Some(("rl_cli", "b6d7acdba407c3c6", "")));
        assert_eq!(
            split_artifact("libserde_derive-0011223344556677.so"),
            Some(("serde_derive", "0011223344556677", ".so"))
        );
        assert_eq!(
            split_artifact("rl_game-0011223344556677.exe"),
            Some(("rl_game", "0011223344556677", ".exe"))
        );
        for other in ["rustcyq1ZPn", "x-0011.rlib", "-0011223344556677", "a-001122334455667g", "notes.txt"] {
            assert_eq!(split_artifact(other), None, "{other}");
        }
    }

    #[test]
    fn reduces_registry_versions_to_their_series() {
        let reg = "/home/u/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f";
        assert_eq!(
            version_series(&format!("{reg}/syn-2.0.119/src/lib.rs")),
            format!("{reg}/syn-2/src/lib.rs")
        );
        assert_eq!(
            version_series(&format!("{reg}/windows-sys-0.59.0/src/lib.rs")),
            format!("{reg}/windows-sys-0.59/src/lib.rs")
        );
        assert_eq!(
            version_series(&format!("{reg}/foo-0.0.3/src/lib.rs")),
            format!("{reg}/foo-0.0.3/src/lib.rs")
        );
        assert_eq!(
            version_series(&format!("{reg}/bar-1.2.3-alpha.1/src/lib.rs")),
            format!("{reg}/bar-1/src/lib.rs")
        );
        assert_eq!(
            version_series("crates/rl-cli/tests/integration.rs"),
            "crates/rl-cli/tests/integration.rs"
        );
    }

    #[test]
    fn reads_the_features_of_a_fingerprint() {
        let json = r#"{"rustc":1,"features":"[\"default\", \"std\"]","declared_features":"[]"}"#;
        assert_eq!(json_string(json, "features"), Some(r#"[\"default\", \"std\"]"#));
        assert_eq!(json_string(json, "declared_features"), Some("[]"));
        assert_eq!(json_string(json, "profile"), None);
        assert_eq!(json_string(r#"{"features":"[\"a"#, "features"), None);
    }

    #[test]
    fn keeps_the_newest_build_of_each_artifact() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("debug");
        let reg = "/r/index.crates.io-1949cf8c6b5b557f";
        let syn2 = format!("{reg}/syn-2.0.119/src/lib.rs");
        let quote = format!("{reg}/quote-1.0.47/src/lib.rs");
        let (lib, exe) = (&[".rlib", ".rmeta"][..], &[""][..]);
        let h = 3 * 3600;
        // Test binaries named alike in two crates: both are live.
        build(&p, "integration", "00000000000000a1", exe, "crates/rl-cli/tests/integration.rs", None, h);
        build(&p, "integration", "00000000000000a2", exe, "crates/rl-game/tests/integration.rs", None, 0);
        // One crate's test binary rebuilt for new configurations: the old
        // ones go, also the one built only 30 minutes before.
        build(&p, "rl_ui", "00000000000000b1", exe, "crates/rl-ui/src/lib.rs", None, 0);
        build(&p, "rl_ui", "00000000000000b3", exe, "crates/rl-ui/src/lib.rs", None, h - 1800);
        build(&p, "rl_ui", "00000000000000b2", exe, "crates/rl-ui/src/lib.rs", None, h);
        // A binary and its test harness, built at different times: both stay.
        let (bin, harness) = (Some(("bin-rl-cli", "[]")), Some(("test-bin-rl-cli", "[]")));
        build(&p, "rl_cli", "00000000000000c1", exe, "crates/rl-cli/src/main.rs", bin, h);
        build(&p, "rl_cli", "00000000000000c2", exe, "crates/rl-cli/src/main.rs", harness, 0);
        // syn 2 in two feature sets built at different times, an old patch
        // release, syn 3 and a check build (metadata only): only the old
        // patch release goes.
        let (full, derive) = (Some(("lib-syn", r#"["full"]"#)), Some(("lib-syn", r#"["derive"]"#)));
        build(&p, "syn", "00000000000000d1", lib, &syn2, full, h);
        build(&p, "syn", "00000000000000d2", lib, &syn2, derive, 0);
        build(&p, "syn", "00000000000000d3", lib, &format!("{reg}/syn-2.0.118/src/lib.rs"), full, 0);
        build(&p, "syn", "00000000000000d4", lib, &format!("{reg}/syn-3.0.7/src/lib.rs"), None, 0);
        build(&p, "syn", "00000000000000d5", &[".rmeta"], &syn2, full, 0);
        // quote alike twice in the last run (for build scripts and for the
        // game), and once in an old run.
        let fp = Some(("lib-quote", r#"["default"]"#));
        build(&p, "quote", "00000000000000e1", lib, &quote, fp, h);
        build(&p, "quote", "00000000000000e2", lib, &quote, fp, h - 120);
        build(&p, "quote", "00000000000000e3", lib, &quote, fp, 0);
        // A build without its `.d`, older than one with.
        build(&p, "soak", "00000000000000f0", exe, "", None, 0);
        build(&p, "soak", "00000000000000f1", exe, "crates/rl-game/tests/soak.rs", None, h);
        // Files without a hash stay.
        file(&p, "deps/rustcyq1ZPn/x.o", 0, "");
        file(&p, "deps/notes.txt", 0, "");
        let stale = names(&p.join("deps"), stale_artifacts(&p, WINDOW));
        assert_eq!(
            stale,
            [
                "libquote-00000000000000e3.rlib",
                "libquote-00000000000000e3.rmeta",
                "libsyn-00000000000000d3.rlib",
                "libsyn-00000000000000d3.rmeta",
                "quote-00000000000000e3.d",
                "rl_ui-00000000000000b1",
                "rl_ui-00000000000000b1.d",
                "rl_ui-00000000000000b3",
                "rl_ui-00000000000000b3.d",
                "soak-00000000000000f0",
                "syn-00000000000000d3.d",
            ]
        );
    }

    #[test]
    fn keeps_each_crates_newest_incremental_sessions() {
        let t = tempfile::tempdir().unwrap();
        let inc = t.path().join("debug/incremental");
        let h = 3 * 3600;
        let session = |dir: &str, s: &str, secs: u64| {
            file(&inc, &format!("{dir}/{s}-svh/dep-graph.bin"), secs, "x");
            file(&inc, &format!("{dir}/{s}.lock"), secs, "");
            let d = std::fs::File::open(inc.join(format!("{dir}/{s}-svh"))).unwrap();
            d.set_modified(t0() + Duration::from_secs(secs)).unwrap();
        };
        // rl_game: an old configuration's directory, and an old session beside a new one.
        session("rl_game-1abc", "s-old-a", 0);
        session("rl_game-2def", "s-old-b", 0);
        session("rl_game-2def", "s-new-c", h);
        // rl_cli: two directories written in the same run (lib and bin).
        session("rl_cli-3ghi", "s-new-d", h);
        session("rl_cli-4jkl", "s-new-e", h - 480);
        let stale = names(&inc, stale_sessions(&inc, WINDOW));
        assert_eq!(stale, ["rl_game-1abc", "rl_game-2def/s-old-b-svh", "rl_game-2def/s-old-b.lock"]);
    }

    #[test]
    fn sweeps_only_cargo_profiles_inside_target() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("target");
        let reg = "/r/index.crates.io-1949cf8c6b5b557f";
        for profile in ["debug", "x86_64-pc-windows-gnu/release", "measure/debug"] {
            let p = target.join(profile);
            std::fs::create_dir_all(p.join(".fingerprint")).unwrap();
            build(&p, "a", "0000000000000001", &[".rlib"], &format!("{reg}/a-1.0.0/src/lib.rs"), None, 0);
            build(&p, "a", "0000000000000002", &[".rlib"], &format!("{reg}/a-1.0.1/src/lib.rs"), None, 7200);
        }
        // Not a cargo profile (no .fingerprint): an OLX checkout's deps/.
        let olx = target.join("olx/src");
        build(&olx, "a", "0000000000000001", &[".rlib"], "x/a-1.0.0/lib.rs", None, 0);
        build(&olx, "a", "0000000000000002", &[".rlib"], "x/a-1.0.1/lib.rs", None, 7200);
        let profiles = names(&target, profile_dirs(&target));
        assert_eq!(profiles, ["debug", "measure/debug", "x86_64-pc-windows-gnu/release"]);
        let swept = sweep(&target, WINDOW);
        assert_eq!(swept, Swept { artifacts: 6, sessions: 0, bytes: swept.bytes });
        for profile in ["debug", "x86_64-pc-windows-gnu/release", "measure/debug"] {
            let deps = target.join(profile).join("deps");
            assert!(!deps.join("liba-0000000000000001.rlib").exists());
            assert!(deps.join("liba-0000000000000002.rlib").exists());
        }
        assert!(olx.join("deps/liba-0000000000000001.rlib").exists());
    }

    #[test]
    fn never_touches_the_release_zips_directory() {
        // `target/dist` is the `dist` profile's output and holds the zips
        // `cargo xtask dist` wrote: kept whole, stale builds and all.
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("target");
        let reg = "/r/index.crates.io-1949cf8c6b5b557f";
        for profile in ["dist", "x86_64-pc-windows-gnu/dist"] {
            let p = target.join(profile);
            std::fs::create_dir_all(p.join(".fingerprint")).unwrap();
            build(&p, "a", "0000000000000001", &[".rlib"], &format!("{reg}/a-1.0.0/src/lib.rs"), None, 0);
            build(&p, "a", "0000000000000002", &[".rlib"], &format!("{reg}/a-1.0.1/src/lib.rs"), None, 7200);
            file(&p, "incremental/rl_game-1abc/s-old-a-x/f", 0, "x");
            file(&p, "incremental/rl_game-1abc/s-old-a-x.lock", 0, "");
            file(&p, "incremental/rl_game-2def/s-new-b-y/f", 7200, "x");
            file(&p, "incremental/rl_game-2def/s-new-b-y.lock", 7200, "");
        }
        let zip = target.join("dist/rust-liero-0.1.0-linux-x64.zip");
        std::fs::write(&zip, "zip").unwrap();
        assert_eq!(names(&target, profile_dirs(&target)), ["x86_64-pc-windows-gnu/dist"]);
        sweep(&target, WINDOW);
        let dist = target.join("dist");
        assert!(dist.join("deps/liba-0000000000000001.rlib").exists());
        assert!(dist.join("incremental/rl_game-1abc/s-old-a-x/f").exists());
        assert!(zip.exists());
        // Another target's dist builds are swept as any profile's.
        let cross = target.join("x86_64-pc-windows-gnu/dist/deps");
        assert!(!cross.join("liba-0000000000000001.rlib").exists());
        assert!(cross.join("liba-0000000000000002.rlib").exists());
    }

    #[test]
    fn measures_hard_linked_files_once() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("debug/deps")).unwrap();
        std::fs::write(t.path().join("debug/deps/rl_game-0000000000000001"), [0u8; 1000]).unwrap();
        std::fs::write(t.path().join("debug/other"), [0u8; 10]).unwrap();
        std::fs::hard_link(
            t.path().join("debug/deps/rl_game-0000000000000001"),
            t.path().join("debug/rl-game"),
        )
        .unwrap();
        assert_eq!(dir_size(t.path(), Duration::from_secs(5)), Some(1010));
    }
}
