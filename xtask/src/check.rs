//! `cargo xtask check` and `cargo xtask full`: every step runs; only failing
//! steps print their output.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use crate::{fetch, layers, root};

/// Captured result of one step: `Ok(())` or the output to show.
type StepResult = Result<(), String>;

fn cargo(args: &[&str], envs: &[(&str, PathBuf)]) -> StepResult {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(args).current_dir(root());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines.len().saturating_sub(200);
    Err(lines[tail..].join("\n"))
}

fn has_nextest() -> bool {
    Command::new("cargo").args(["nextest", "--version"]).output().is_ok_and(|o| o.status.success())
}

pub fn run(offline: bool) -> anyhow::Result<bool> {
    run_with(offline, false)
}

/// `cargo xtask full`: `check`, then the bot-match grid ([`FULL_TESTS`]),
/// then the GPU tier (the renderer's GPU tests
/// and the render goldens, with `RL_GPU_TESTS` set), skipped with a notice
/// when no graphics adapter opens, and the audio device check (OLX sounds
/// on the default output), skipped with a notice when no device opens.
pub fn full(offline: bool) -> anyhow::Result<bool> {
    run_with(offline, true)
}

/// The full tier's long CPU tests as `(package, test target)`, run with
/// `RL_FULL_TESTS` set: the headless bot-match grid (both mods × 4 levels ×
/// 3 seeds, chaos and Deathmatch bots, with its coverage) and the
/// 30-minute bounded match (spec §7.1, plan 6 Review Focus 5).
pub const FULL_TESTS: &[(&str, &str)] = &[("rl-cli", "grid")];

/// The test targets of the GPU tier as `(package, test target)`.
pub const GPU_TESTS: &[(&str, &str)] =
    &[("rl-render", "gpu"), ("rl-cli", "render_golden"), ("rl-game", "soak")];

/// Whether `rl-render`'s `adapter` example opens a graphics adapter:
/// `Ok(Ok(adapter))` when it does, `Ok(Err(notice))` when none opens, `Err`
/// when the probe itself fails.
pub fn probe_adapter() -> Result<Result<String, String>, String> {
    let probe = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["run", "--quiet", "-p", "rl-render", "--example", "adapter"])
        .current_dir(root())
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    let said = String::from_utf8_lossy(&probe.stdout).trim().to_string();
    match probe.status.code() {
        Some(0) => Ok(Ok(said)),
        Some(3) => Ok(Err(said)),
        _ => Err(format!("{said}{}", String::from_utf8_lossy(&probe.stderr))),
    }
}

fn gpu_step(envs: &[(&str, PathBuf)]) -> StepResult {
    match probe_adapter()? {
        Ok(said) => println!("     gpu adapter: {said}"),
        Err(said) => {
            println!("skip gpu ({said})");
            return Ok(());
        }
    }
    let mut args =
        vec!["nextest", "run", "--no-fail-fast", "--status-level", "fail", "--final-status-level", "fail"];
    if !has_nextest() {
        args = vec!["test", "--quiet"];
    }
    for (package, test) in GPU_TESTS {
        args.extend(["-p", package, "--test", test]);
    }
    let mut envs = envs.to_vec();
    envs.push(("RL_GPU_TESTS", PathBuf::from("1")));
    cargo(&args, &envs)
}

/// The full tier's long CPU tests ([`FULL_TESTS`]).
fn full_cpu_step(envs: &[(&str, PathBuf)]) -> StepResult {
    let mut args =
        vec!["nextest", "run", "--no-fail-fast", "--status-level", "fail", "--final-status-level", "fail"];
    if !has_nextest() {
        args = vec!["test", "--quiet"];
    }
    for (package, test) in FULL_TESTS {
        args.extend(["-p", package, "--test", test]);
    }
    let mut envs = envs.to_vec();
    envs.push(("RL_FULL_TESTS", PathBuf::from("1")));
    cargo(&args, &envs)
}

/// `rl-audio`'s `device_check` example: plays two OLX sounds on the default
/// output device. Exit 3 means no device opened (the game runs silently).
fn audio_step(envs: &[(&str, PathBuf)]) -> StepResult {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(["run", "--quiet", "-p", "rl-audio", "--example", "device_check"]).current_dir(root());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.code() {
        Some(0) => Ok(()),
        Some(3) => {
            println!("skip audio ({said})");
            Ok(())
        }
        _ => Err(format!("{said}{}", String::from_utf8_lossy(&out.stderr))),
    }
}

fn run_with(offline: bool, gpu: bool) -> anyhow::Result<bool> {
    crate::tidy::warn_if_large();
    let rules = layers::load_rules()?;
    let mut failed = Vec::new();
    let mut step = |name: &str, f: &mut dyn FnMut() -> StepResult| {
        let t = Instant::now();
        match f() {
            Ok(()) => println!("ok   {name} ({:.1}s)", t.elapsed().as_secs_f32()),
            Err(out) => {
                println!("FAIL {name} ({:.1}s)\n{out}", t.elapsed().as_secs_f32());
                failed.push(name.to_string());
            }
        }
    };

    step("fmt", &mut || cargo(&["fmt", "--all", "--", "--check"], &[]));
    step("clippy", &mut || {
        cargo(&["clippy", "--quiet", "--workspace", "--all-targets", "--", "-D", "warnings"], &[])
    });
    step("layers", &mut || layers::run().map_err(|e| format!("{e:#}")));
    step("wasm", &mut || {
        if rules.wasm.is_empty() {
            return Ok(());
        }
        let mut args = vec!["build", "--quiet", "--target", "wasm32-unknown-unknown"];
        for c in &rules.wasm {
            args.extend(["-p", c.as_str()]);
        }
        cargo(&args, &[])
    });
    let mut envs = Vec::new();
    if offline {
        println!("skip olx-content (--offline)");
    } else {
        step("olx-content", &mut || match fetch::run(true) {
            Ok(paths) => {
                envs = vec![("RL_OLX_GAMEDIR", paths.gamedir), ("RL_CONTENT_DIR", paths.content)];
                Ok(())
            }
            Err(e) => Err(format!("{e:#}")),
        });
    }
    step("test", &mut || {
        if has_nextest() {
            let args = [
                "nextest",
                "run",
                "--workspace",
                "--no-fail-fast",
                "--status-level",
                "fail",
                "--final-status-level",
                "fail",
            ];
            cargo(&args, &envs)
        } else {
            cargo(&["test", "--quiet", "--workspace"], &envs)
        }
    });

    if gpu {
        step("bot-grid", &mut || full_cpu_step(&envs));
        step("gpu", &mut || gpu_step(&envs));
        step("audio", &mut || audio_step(&envs));
    }
    let what = if gpu { "full" } else { "check" };
    if failed.is_empty() {
        println!("{what} passed");
        Ok(true)
    } else {
        println!("{what} failed: {}", failed.join(", "));
        Ok(false)
    }
}
