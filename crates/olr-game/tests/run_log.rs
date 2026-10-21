//! The game binary's run log (issue #2): `<data>/logs/<run>.log` with its
//! header and a clean end, the previous run's crash reported, an
//! unwritable directory leaving the terminal alone. Each run fails before
//! any window (`--edit` of a missing directory), so no display is needed.

use std::path::{Path, PathBuf};
use std::process::Output;

/// The game with `--edit <missing>` on data directory `data`, no relaunch
/// (the backend chosen), the level the default one.
fn refused_edit(data: &Path) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_olr"))
        .arg("--edit")
        .arg(data.join("no such pack"))
        .arg("--data-dir")
        .arg(data)
        .env("WGPU_BACKEND", "vulkan")
        .env_remove("OLR_LOG")
        .output()
        .unwrap()
}

fn logs(data: &Path) -> Vec<PathBuf> {
    olr_log::run::runs(&data.join("logs"))
}

#[test]
fn a_run_writes_its_log_and_ends_it_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let out = refused_edit(tmp.path());
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stdout.lines().next(), Some(format!("olr {}", olr_game::VERSION).as_str()), "{stdout}");
    assert!(stderr.lines().any(|l| l.starts_with("error: ") && l.contains("no such pack")), "{stderr}");
    let runs = logs(tmp.path());
    assert_eq!(runs.len(), 1, "{runs:?}");
    let text = std::fs::read_to_string(&runs[0]).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].ends_with(&format!(" info  olr: olr {}", olr_game::VERSION)), "{text}");
    assert!(lines[1].contains(" info  olr: os: "), "{text}");
    assert!(text.contains(" error olr: ") && text.contains("no such pack"), "{text}");
    assert!(lines.last().unwrap().ends_with(olr_log::CLEAN_END), "{text}");
    assert!(!olr_log::run::is_live(&runs[0]), "the lock went with the process");
}

#[test]
fn a_run_that_ended_unexpectedly_is_reported_once() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("logs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("2000-01-01_00-00-00.000Z_1.log"),
        "2000-01-01 00:00:00.000 info  olr: playing\n",
    )
    .unwrap();
    let stderr = String::from_utf8_lossy(&refused_edit(tmp.path()).stderr).into_owned();
    assert!(stderr.contains("warning: the previous run ended unexpectedly"), "{stderr}");
    let stderr = String::from_utf8_lossy(&refused_edit(tmp.path()).stderr).into_owned();
    assert!(!stderr.contains("ended unexpectedly"), "{stderr}");
    assert_eq!(logs(tmp.path()).len(), 3);
}

#[test]
fn an_unwritable_log_directory_leaves_the_terminal() {
    let tmp = tempfile::tempdir().unwrap();
    // The data directory is a file: no `logs` in it.
    let data = tmp.path().join("a file");
    std::fs::write(&data, "x").unwrap();
    let out = refused_edit(&data);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "the game ran on: {stderr}");
    assert!(stderr.contains("warning: cannot write the log in"), "{stderr}");
    assert!(stderr.lines().any(|l| l.starts_with("error: ") && l.contains("no such pack")), "{stderr}");
}
