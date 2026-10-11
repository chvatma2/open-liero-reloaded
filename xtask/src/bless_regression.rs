//! `cargo xtask bless-regression --reason <text> [--from <failure>]`
//! (spec §7.2, §7.3): re-records the golden checksum streams in
//! `fixtures/matches/` (by running `rl-cli`'s `golden` test with
//! `RL_BLESS_REGRESSION` set) and records why in `fixtures/blessings.log`.
//! `--from target/failures/<name>.ron` first adds that failed match as a new
//! golden (its record names the match; the stream is recorded now, once
//! the failure is fixed).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail, ensure};

use crate::bless::{changed_files, log_blessing, snapshot};
use crate::root;

const DIR: &str = "fixtures/matches";

/// Copies failure record `from` into `dir` as a golden to be recorded;
/// gives its new path. An existing golden of that name is kept: refused.
pub fn add_failure(dir: &Path, from: &Path) -> anyhow::Result<PathBuf> {
    ensure!(
        from.extension().is_some_and(|e| e == "ron"),
        "{} is not a failure record (.ron)",
        from.display()
    );
    let text = std::fs::read_to_string(from).with_context(|| format!("reading {}", from.display()))?;
    ensure!(text.contains("spec:"), "{} names no match (`spec`)", from.display());
    let name = from.file_name().context("a file name")?;
    let dest = dir.join(name);
    ensure!(!dest.exists(), "{} exists already", dest.display());
    std::fs::create_dir_all(dir)?;
    std::fs::write(&dest, text)?;
    Ok(dest)
}

pub fn run(reason: &str, from: Option<&Path>) -> anyhow::Result<bool> {
    ensure!(!reason.trim().is_empty(), "--reason must say why the matches change");
    let root = root();
    let content = std::env::var_os("RL_CONTENT_DIR").map_or_else(|| root.join("target/content"), Into::into);
    if !content.is_dir() {
        bail!("{} not found: run `cargo xtask fetch-olx-content` first", content.display());
    }
    let dir = root.join(DIR);
    let before = snapshot(&dir)?;
    if let Some(from) = from {
        println!("added {}", add_failure(&dir, from)?.display());
    }
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["test", "--quiet", "-p", "rl-cli", "--test", "golden"])
        .current_dir(&root)
        .env("RL_BLESS_REGRESSION", "1")
        .env("RL_CONTENT_DIR", &content)
        .output()
        .context("cannot run cargo")?;
    // A failing run must not leave a changed golden unlogged.
    let changed = changed_files(DIR, &before, &snapshot(&dir)?);
    if !changed.is_empty() {
        log_blessing("bless-regression", reason, &changed)?;
    }
    if !out.status.success() {
        eprintln!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        bail!("rl-cli --test golden failed while blessing");
    }
    if changed.is_empty() {
        println!("the golden matches are unchanged: nothing to bless");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_failure_becomes_a_golden_once() {
        let tmp = tempfile::tempdir().unwrap();
        let failure = tmp.path().join("classic-x-panic-tick5.ron");
        std::fs::write(&failure, "(spec: (mod_id: \"olx-classic\"), tick: 5)").unwrap();
        let dir = tmp.path().join("matches");
        let dest = super::add_failure(&dir, &failure).unwrap();
        assert_eq!(dest, dir.join("classic-x-panic-tick5.ron"));
        assert!(std::fs::read_to_string(&dest).unwrap().contains("olx-classic"));
        assert!(super::add_failure(&dir, &failure).is_err(), "an existing golden is kept");
        let replay = tmp.path().join("x.rlrp");
        std::fs::write(&replay, b"RLRP").unwrap();
        assert!(super::add_failure(&dir, &replay).is_err(), "only a record names the match");
    }
}
