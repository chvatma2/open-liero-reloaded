//! `cargo xtask e2e` and `full`'s `e2e` step (spec §7, §21 G7): plays
//! `fixtures/autoplay/menus.ron` in the real game on a virtual X server
//! (Xvfb), with `--fake-clock` (deterministic: the same frames, replay and
//! screenshots every run) and a fresh data directory holding
//! `fixtures/autoplay/settings.ron`. It passes when the game exits 0, its
//! structured log (`autoplay.jsonl`) ends in `pass`, every screenshot the
//! script names was written, and stderr has no warning, error or panic.
//!
//! Rendering is forced onto Mesa's software rasteriser, so the run is the
//! same on every machine and the WSL relaunch (`rl-game`'s `wsl.rs`, which
//! would pick the real GPU through d3d12) never happens: `WGPU_BACKEND=gl`,
//! `LIBGL_ALWAYS_SOFTWARE=1`, `GALLIUM_DRIVER=llvmpipe`, no Wayland, and
//! `--window-system x11`. Sound is off (`--no-sound`). Without Xvfb the
//! step is skipped with a notice.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::root;

/// The script and the settings it starts from.
pub const SCRIPT: &str = "fixtures/autoplay/menus.ron";
pub const SETTINGS: &str = "fixtures/autoplay/settings.ron";

/// Where a run writes (its data directory and screenshots); made afresh.
pub const OUT: &str = "target/e2e";

/// The longest a run may take.
const TIMEOUT: Duration = Duration::from_secs(240);

/// The software rendering environment (see the module's doc).
pub const SOFTWARE_GL: [(&str, &str); 3] =
    [("WGPU_BACKEND", "gl"), ("LIBGL_ALWAYS_SOFTWARE", "1"), ("GALLIUM_DRIVER", "llvmpipe")];

/// A virtual X server, stopped when dropped.
struct Xvfb {
    child: Child,
    display: String,
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        stop(&mut self.child);
    }
}

/// Stops Xvfb with SIGTERM, so it removes its lock file (SIGKILL would
/// leave `/tmp/.X<n>-lock` behind), then waits for it.
fn stop(child: &mut Child) {
    let term = Command::new("kill").arg(child.id().to_string()).status();
    if !term.is_ok_and(|s| s.success()) {
        let _ = child.kill();
    }
    let _ = child.wait();
}

/// Starts Xvfb on the first free display from `:90`, 1920 × 1080 (the
/// largest window preset fits); ready once it writes its display number
/// (`-displayfd`). WSLg mounts `/tmp/.X11-unix` read-only, so Xvfb listens
/// on the abstract socket there and the lock files tell free displays.
/// `Ok(None)`: Xvfb is not installed.
fn xvfb() -> Result<Option<Xvfb>, String> {
    for n in 90..130 {
        if Path::new(&format!("/tmp/.X{n}-lock")).exists() {
            continue;
        }
        let child = Command::new("Xvfb")
            .args([&format!(":{n}"), "-screen", "0", "1920x1080x24", "-nolisten", "tcp", "-displayfd", "1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot start Xvfb: {e}")),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        if let Some(out) = child.stdout.take() {
            std::thread::spawn(move || {
                let mut line = String::new();
                let _ = std::io::BufRead::read_line(&mut std::io::BufReader::new(out), &mut line);
                let _ = tx.send(line);
            });
        }
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(line) if line.trim() == n.to_string() => {
                return Ok(Some(Xvfb { child, display: format!(":{n}") }));
            }
            _ => stop(&mut child),
        }
    }
    Err("no free X display from :90 to :129 for Xvfb".into())
}

/// The screenshots `script` takes, by name.
fn shots(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in script.split("Screenshot(\"").skip(1) {
        if let Some(end) = part.find('"') {
            out.push(part[..end].to_string());
        }
    }
    out
}

/// What is wrong with a run's stderr: its warning, error and panic lines.
pub fn bad_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("warning:") || t.starts_with("error:") || l.contains("panicked")
        })
        .collect()
}

/// Runs the script; `content` is the imported content directory.
pub fn run(content: Option<&Path>) -> Result<(), String> {
    let root = root();
    let Some(content) = content else {
        println!("skip e2e (no OLX content: --offline)");
        return Ok(());
    };
    let build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--quiet", "-p", "rl-game"])
        .current_dir(&root)
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !build.status.success() {
        return Err(String::from_utf8_lossy(&build.stderr).into_owned());
    }
    let Some(x) = xvfb()? else {
        println!("skip e2e (Xvfb is not installed)");
        return Ok(());
    };
    let out = root.join(OUT);
    let _ = std::fs::remove_dir_all(&out);
    let data = out.join("data");
    let shot_dir = out.join("shots");
    std::fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    std::fs::copy(root.join(SETTINGS), data.join("settings.ron")).map_err(|e| format!("{SETTINGS}: {e}"))?;

    let mut cmd = Command::new(root.join("target/debug/rl-game"));
    cmd.args(["--window-system", "x11", "--fake-clock", "--no-sound"])
        .arg("--autoplay")
        .arg(root.join(SCRIPT))
        .arg("--content")
        .arg(content)
        .arg("--data-dir")
        .arg(&data)
        .arg("--shot-dir")
        .arg(&shot_dir)
        .current_dir(&root)
        .env("DISPLAY", &x.display)
        .env_remove("WAYLAND_DISPLAY")
        .envs(SOFTWARE_GL)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let child = cmd.spawn().map_err(|e| format!("cannot start rl-game: {e}"))?;
    let output = wait(child, TIMEOUT)?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let log = std::fs::read_to_string(shot_dir.join("autoplay.jsonl")).unwrap_or_default();
    let tail: Vec<&str> = log.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
    let report = |why: &str| {
        format!(
            "{why}\n--- autoplay.jsonl (last lines)\n{}\n--- stdout\n{}\n--- stderr\n{}",
            tail.join("\n"),
            stdout.trim(),
            stderr.trim()
        )
    };
    if !output.status.success() {
        return Err(report(&format!("rl-game exited with {}", output.status)));
    }
    if !log.lines().last().is_some_and(|l| l.contains("\"event\":\"pass\"")) {
        return Err(report("the autoplay log does not end in a pass"));
    }
    let bad = bad_lines(&stderr);
    if !bad.is_empty() {
        return Err(report(&format!("the log is not clean: {bad:?}")));
    }
    let script = std::fs::read_to_string(root.join(SCRIPT)).map_err(|e| format!("{SCRIPT}: {e}"))?;
    for name in shots(&script) {
        let png = shot_dir.join(format!("{name}.png"));
        if !png.is_file() {
            return Err(report(&format!("no screenshot {}", png.display())));
        }
    }
    let frames = log
        .lines()
        .last()
        .and_then(|l| l.split("\"frame\":").nth(1))
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next().map(str::to_string));
    println!(
        "     e2e: {SCRIPT} passed on Xvfb {} in {:.1} s ({} frames)",
        x.display,
        started.elapsed().as_secs_f32(),
        frames.unwrap_or_default()
    );
    Ok(())
}

/// Waits for `child` and its output, killing it after `timeout`.
fn wait(child: Child, timeout: Duration) -> Result<std::process::Output, String> {
    let id = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(out) => out.map_err(|e| format!("waiting for rl-game: {e}")),
        Err(_) => {
            let _ = Command::new("kill").args(["-9", &id.to_string()]).status();
            Err(format!("rl-game ran over {} s: killed", timeout.as_secs()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_names_its_screenshots_and_the_log_is_checked() {
        assert_eq!(
            shots(r#"(steps: [(ui: Screenshot("a")), (ui: Click("b")), (ui: Screenshot("c-1"))])"#),
            ["a", "c-1"]
        );
        let script = std::fs::read_to_string(root().join(SCRIPT)).unwrap();
        assert!(shots(&script).len() >= 5);
        let err = "rl-game: GPU x\nwarning: no gamepads\nthread 'main' panicked at x\nerror: y\nMesa: fine\n";
        assert_eq!(bad_lines(err), ["warning: no gamepads", "thread 'main' panicked at x", "error: y"]);
    }
}
