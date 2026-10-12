//! The imported OLX content for the match tests (`RL_CONTENT_DIR`, which
//! `.cargo/config.toml` defaults to `content/packs`); tests skip with a
//! notice without it.
#![allow(dead_code)]

use std::path::Path;

use rl_content::Content;

pub const CLASSIC: &str = "olx-classic";
pub const MW: &str = "olx-mw-1-0";
pub const MODS: [&str; 2] = [CLASSIC, MW];
pub const LEVELS: [&str; 4] =
    ["olx-level-castlestrike", "olx-level-dirt-level", "olx-level-fossilfacility", "olx-level-liero-factory"];

/// The content directory, or `None` (with a notice) without it.
pub fn content_dir() -> Option<String> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(dir)
}

/// The imported content, or `None` (with a notice) without it.
pub fn content() -> Option<Content> {
    content_dir().map(|d| Content::open(Path::new(&d)).expect("the content directory"))
}

/// Whether the `cargo xtask full` tier runs (`RL_FULL_TESTS`); a notice if not.
pub fn full_tier() -> bool {
    let on = std::env::var_os("RL_FULL_TESTS").is_some();
    if !on {
        eprintln!("RL_FULL_TESTS not set (runs in `cargo xtask full`); skipping");
    }
    on
}

/// Writes `failure` of `run` to `target/failures/` (spec §7.3) and gives a
/// message naming the record, for the test's panic.
pub fn capture(run: &rl_cli::runner::MatchRun, failure: &rl_cli::runner::Failure) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/failures");
    match rl_cli::failure::write(&dir, &run.spec, failure, &run.replay) {
        Ok(path) => format!("{}: {failure}\n  captured in {}", run.spec.stem(), path.display()),
        Err(e) => format!("{}: {failure}\n  (not captured: {e:#})", run.spec.stem()),
    }
}

/// The repo's `target/failures`, where failing runs write their records.
pub fn failures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/failures")
}
