//! The hand-made playtest packs (`content/playtest/`: the night Castle
//! Strike; the day one ships as Castle Strike Reloaded, `tests/reloaded.rs`):
//! each checks clean against the imported content, and, on the GPU tier, a
//! bot match plays and draws on it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::content_dir;

fn playtest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/playtest")
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).args(args).output().unwrap()
}

#[test]
fn playtest_packs_check_clean() {
    let Some(content) = content_dir() else { return };
    let dir = playtest_dir();
    let o = cli(&["pack", "check", dir.to_str().unwrap(), "--content", &content]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success() && out.contains("checked 1 packs: 0 errors, 0 warnings"),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// The night Castle Strike (plan 15c) plays and draws lit.
#[test]
fn the_lit_castle_strike_plays() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let Some(content) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = playtest_dir();
    let level = "olr-level-castlestrike-night";
    let out = tmp.path().join(format!("{level}.png"));
    let o = cli(&[
        "render-match",
        "--mod",
        "olx-classic",
        "--level",
        level,
        "--bots",
        "3",
        "--seed",
        "1",
        "--ticks",
        "200",
        "--packs",
        dir.to_str().unwrap(),
        "--content",
        &content,
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("glowing discs"), "a lit match");
}
