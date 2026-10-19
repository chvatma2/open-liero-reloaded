//! The hand-made playtest packs (`content/playtest/`, such as the lit
//! Castle Strike): each checks clean against the imported content, and,
//! on the GPU tier, a bot match plays and draws on it.

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
        o.status.success() && out.contains("checked 2 packs: 0 errors, 0 warnings"),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// Both lit Castle Strikes (night and day, plan 15c) play and draw lit.
#[test]
fn the_lit_castle_strikes_play() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let Some(content) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = playtest_dir();
    for level in ["olr-level-castlestrike-night", "olr-level-castlestrike-day"] {
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
        assert!(o.status.success(), "{level}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(String::from_utf8_lossy(&o.stdout).contains("glowing discs"), "{level}: a lit match");
    }
}
