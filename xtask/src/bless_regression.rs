//! `cargo xtask bless-regression --reason <text> [--from <failure>]`
//! (spec §7.2, §7.3): re-records the golden checksum streams in
//! `fixtures/matches/` (by running `rl-cli`'s `golden` test with
//! `RL_BLESS_REGRESSION` set) and records why in `fixtures/blessings.log`.
//! `--from target/failures/<name>.ron` first adds that failed match as a new
//! golden (its record names the match; the stream is recorded now, once
//! the failure is fixed). A record named `net-*.ron` (`rl-cli net-sim`'s) is
//! kept as a net case in `fixtures/net/cases/` instead, and the run checks
//! the cases rather than re-recording goldens.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail, ensure};

use crate::bless::{changed_files, log_blessing, snapshot};
use crate::root;

const DIR: &str = "fixtures/matches";

/// What `add_failure` kept the record as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// A golden match, still to be recorded.
    Golden,
    /// A net case in `fixtures/net/cases`.
    NetCase,
}

const NET_DIR: &str = "fixtures/net/cases";

/// Copies failure record `from` into `dir` as a golden to be recorded (a
/// `net-*` record into the net cases beside it, `dir/../net/cases`); gives
/// its new path. An existing file of that name is kept: refused.
pub fn add_failure(dir: &Path, from: &Path) -> anyhow::Result<(PathBuf, Kept)> {
    ensure!(
        from.extension().is_some_and(|e| e == "ron"),
        "{} is not a failure record (.ron)",
        from.display()
    );
    let text = std::fs::read_to_string(from).with_context(|| format!("reading {}", from.display()))?;
    ensure!(text.contains("spec:"), "{} names no match (`spec`)", from.display());
    let name = from.file_name().context("a file name")?;
    let is_net = name.to_string_lossy().starts_with("net-");
    let dir = if is_net { dir.parent().unwrap_or(dir).join("net/cases") } else { dir.to_path_buf() };
    let dest = dir.join(name);
    ensure!(!dest.exists(), "{} exists already", dest.display());
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&dest, text)?;
    Ok((dest, if is_net { Kept::NetCase } else { Kept::Golden }))
}

pub fn run(reason: &str, from: Option<&Path>) -> anyhow::Result<bool> {
    ensure!(!reason.trim().is_empty(), "--reason must say why the matches change");
    let root = root();
    let content = std::env::var_os("RL_CONTENT_DIR").map_or_else(|| root.join("content/packs"), Into::into);
    if !content.is_dir() {
        bail!("{} not found: run `cargo xtask fetch-olx-content` first", content.display());
    }
    let dir = root.join(DIR);
    let before = snapshot(&dir)?;
    if let Some(from) = from {
        let before_net = snapshot(&root.join(NET_DIR))?;
        let (added, kept) = add_failure(&dir, from)?;
        println!("added {}", added.display());
        if kept == Kept::NetCase {
            // A net case: nothing to record, it must pass now.
            let changed = changed_files(NET_DIR, &before_net, &snapshot(&root.join(NET_DIR))?);
            log_blessing("bless-regression", reason, &changed)?;
            let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .args(["test", "--quiet", "-p", "rl-cli", "--test", "net_cases"])
                .current_dir(&root)
                .env("RL_CONTENT_DIR", &content)
                .status()
                .context("cannot run cargo")?;
            ensure!(status.success(), "rl-cli --test net_cases failed: the case still fails");
            return Ok(true);
        }
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
        let (dest, kind) = super::add_failure(&dir, &failure).unwrap();
        assert_eq!(kind, super::Kept::Golden);
        assert_eq!(dest, dir.join("classic-x-panic-tick5.ron"));
        assert!(std::fs::read_to_string(&dest).unwrap().contains("olx-classic"));
        assert!(super::add_failure(&dir, &failure).is_err(), "an existing golden is kept");
        let replay = tmp.path().join("x.rlrp");
        std::fs::write(&replay, b"RLRP").unwrap();
        assert!(super::add_failure(&dir, &replay).is_err(), "only a record names the match");
    }

    #[test]
    fn a_net_failure_becomes_a_net_case() {
        let tmp = tempfile::tempdir().unwrap();
        let failure = tmp.path().join("net-x.ron");
        std::fs::write(&failure, "(spec: (mod_id: \"olx-classic\"))").unwrap();
        let dir = tmp.path().join("fixtures/matches");
        let cases = tmp.path().join("fixtures/net/cases");
        let before = crate::bless::snapshot(&cases).unwrap();
        let (dest, kind) = super::add_failure(&dir, &failure).unwrap();
        assert_eq!(kind, super::Kept::NetCase);
        assert_eq!(dest, cases.join("net-x.ron"));
        // What `run` logs with the reason.
        let changed =
            crate::bless::changed_files(super::NET_DIR, &before, &crate::bless::snapshot(&cases).unwrap());
        assert_eq!(changed, ["fixtures/net/cases/net-x.ron"]);
        assert!(!dir.exists(), "nothing lands among the match goldens");
        assert!(super::add_failure(&dir, &failure).is_err(), "an existing case is kept");
    }
}
