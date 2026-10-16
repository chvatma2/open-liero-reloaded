//! The imported OLX content for the match tests (`OLR_CONTENT_DIR`, which
//! `.cargo/config.toml` defaults to `content/packs`); tests skip with a
//! notice without it.
#![allow(dead_code)]

use std::path::Path;

use olr_content::Content;

pub const CLASSIC: &str = "olx-classic";
pub const MW: &str = "olx-mw-1-0";
pub const MODS: [&str; 2] = [CLASSIC, MW];
pub const LEVELS: [&str; 4] =
    ["olx-level-castlestrike", "olx-level-dirt-level", "olx-level-fossilfacility", "olx-level-liero-factory"];

/// The content directory, or `None` (with a notice) without it.
pub fn content_dir() -> Option<String> {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(dir)
}

/// The imported content, or `None` (with a notice) without it.
pub fn content() -> Option<Content> {
    content_dir().map(|d| Content::open(Path::new(&d)).expect("the content directory"))
}

/// Whether the `cargo xtask full` tier runs (`OLR_FULL_TESTS`); a notice if not.
pub fn full_tier() -> bool {
    let on = std::env::var_os("OLR_FULL_TESTS").is_some();
    if !on {
        eprintln!("OLR_FULL_TESTS not set (runs in `cargo xtask full`); skipping");
    }
    on
}

/// Writes `failure` of `run` to `target/failures/` (spec §7.3) and gives a
/// message naming the record, for the test's panic.
pub fn capture(run: &olr_cli::runner::MatchRun, failure: &olr_cli::runner::Failure) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/failures");
    match olr_cli::failure::write(&dir, &run.spec, failure, &run.replay) {
        Ok(path) => format!("{}: {failure}\n  captured in {}", run.spec.stem(), path.display()),
        Err(e) => format!("{}: {failure}\n  (not captured: {e:#})", run.spec.stem()),
    }
}

/// The repo's `target/failures`, where failing runs write their records.
pub fn failures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/failures")
}

/// The packs a match of Classic on Dirt Level needs, and the default skin.
pub const MATCH_PACKS: [&str; 5] =
    ["olx-core", "olx-classic", "olx-theme-dirt", "olx-level-dirt-level", "olx-skin-default"];

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// A content directory with [`MATCH_PACKS`] from `dir` but `leave_out`
/// (a client that downloads the rest from its host).
pub fn partial_content(dir: &Path, leave_out: &[&str]) -> tempfile::TempDir {
    let to = tempfile::tempdir().unwrap();
    for id in MATCH_PACKS.iter().filter(|id| !leave_out.contains(id)) {
        copy_dir(&dir.join(id), &to.path().join(id));
    }
    to
}
