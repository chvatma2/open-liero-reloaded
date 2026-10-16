//! `cargo xtask check` and `cargo xtask full`: every step runs; only failing
//! steps print their output.

use std::cell::RefCell;
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
/// then the GPU tier (the renderer's GPU tests,
/// the render goldens and the HUD snapshots, with `OLR_GPU_TESTS` set), skipped with a notice
/// when no graphics adapter opens, the audio device check (OLX sounds
/// on the default output), skipped with a notice when no device opens,
/// the end-to-end autoplay on Xvfb (`e2e.rs`; skipped without Xvfb), the
/// Docker NAT tests (`nat.rs`; skipped without Docker), and
/// the benchmarks and the short stress run against their baselines
/// (`bench.rs`), and the feel numbers of online play against theirs
/// (`net.rs`).
pub fn full(offline: bool) -> anyhow::Result<bool> {
    run_with(offline, true)
}

/// The full tier's long CPU tests as `(package, test target)`, run with
/// `OLR_FULL_TESTS` set: the headless bot-match grid (both mods × 4 levels ×
/// 3 seeds, chaos and Deathmatch bots, with its coverage) and the
/// 30-minute bounded match (spec §7.1, plan 6 Review Focus 5), and the
/// online grid of bots over the network profiles (`net_sim`, plan 9), the
/// 10 000 unreliable datagrams over real sockets (`olr-net`'s `link`,
/// 2 000 in `check`), and real-transport matches of `olr-server` and 8 bot
/// clients per network profile and a spectator joining mid-match
/// (`olr-cli`'s `transport`, spec §9.4; two clients in `check`), and
/// olr-game's integration tests again for its full-tier migration case
/// (plan 12c Task 9: `a_client_whose_frames_stop_stays_joined_with_migration`).
///
/// [`TIMED_TESTS`] are not in this list: they run first and alone.
pub const FULL_TESTS: &[(&str, &str)] =
    &[("olr-cli", "net_sim"), ("olr-cli", "transport"), ("olr-net", "link"), ("olr-game", "integration")];

/// The full tier's tests that assert wall-clock budgets (`grid`'s p99 tick
/// of 10 ms): a budget measures the code only when nothing else runs, so
/// `bot-grid` runs these in their own nextest invocation, one test at a
/// time, before [`FULL_TESTS`] (beside the online tests the grid's p99
/// doubled, from 4.3 ms to 9 ms, and went over 10 ms in `full`).
pub const TIMED_TESTS: &[(&str, &str)] = &[("olr-cli", "grid")];

/// The test targets of the GPU tier as `(package, test target)`.
pub const GPU_TESTS: &[(&str, &str)] = &[
    ("olr-render", "gpu"),
    ("olr-cli", "render_golden"),
    ("olr-cli", "gpu_bench"),
    ("olr-game", "soak"),
    ("olr-ui", "hud_snapshots"),
];

/// Whether `olr-render`'s `adapter` example opens a graphics adapter:
/// `Ok(Ok(adapter))` when it does, `Ok(Err(notice))` when none opens, `Err`
/// when the probe itself fails.
pub fn probe_adapter() -> Result<Result<String, String>, String> {
    let probe = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["run", "--quiet", "-p", "olr-render", "--example", "adapter"])
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
    envs.push(("OLR_GPU_TESTS", PathBuf::from("1")));
    cargo(&args, &envs)
}

/// The full tier's long CPU tests: [`TIMED_TESTS`] alone and one at a time,
/// then [`FULL_TESTS`] together.
fn full_cpu_step(envs: &[(&str, PathBuf)]) -> StepResult {
    let mut envs = envs.to_vec();
    envs.push(("OLR_FULL_TESTS", PathBuf::from("1")));
    let nextest = has_nextest();
    for (tests, serial) in [(TIMED_TESTS, true), (FULL_TESTS, false)] {
        let mut args = if nextest {
            vec!["nextest", "run", "--no-fail-fast", "--status-level", "fail", "--final-status-level", "fail"]
        } else {
            vec!["test", "--quiet"]
        };
        for (package, test) in tests {
            args.extend(["-p", package, "--test", test]);
        }
        if serial {
            args.extend(if nextest { ["--test-threads", "1"] } else { ["--", "--test-threads=1"] });
        }
        cargo(&args, &envs)?;
    }
    Ok(())
}

/// `olr-audio`'s `device_check` example: plays two OLX sounds on the default
/// output device. Exit 3 means no device opened (the game runs silently).
fn audio_step(envs: &[(&str, PathBuf)]) -> StepResult {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(["run", "--quiet", "-p", "olr-audio", "--example", "device_check"]).current_dir(root());
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
    crate::tidy::sweep_if_large();
    let rules = layers::load_rules()?;
    let failed = RefCell::new(Vec::new());
    // A step's line: `ok   <name> (<detail><s>s)`, or its failure and output.
    let finish = |name: &str, t: Instant, result: Result<String, String>| match result {
        Ok(detail) => println!("ok   {name} ({detail}{:.1}s)", t.elapsed().as_secs_f32()),
        Err(out) => {
            println!("FAIL {name} ({:.1}s)\n{out}", t.elapsed().as_secs_f32());
            failed.borrow_mut().push(name.to_string());
        }
    };
    let step = |name: &str, f: &mut dyn FnMut() -> StepResult| {
        let t = Instant::now();
        finish(name, t, f().map(|()| String::new()));
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
        step("olx-content", &mut || match fetch::verify() {
            Ok(paths) => {
                envs = vec![("OLR_OLX_GAMEDIR", paths.gamedir), ("OLR_CONTENT_DIR", paths.content)];
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

    step("determinism", &mut || {
        crate::determinism::step(crate::determinism::Opts { wasm: true, ..Default::default() })
    });

    if gpu {
        step("bot-grid", &mut || full_cpu_step(&envs));
        step("gpu", &mut || gpu_step(&envs));
        step("audio", &mut || audio_step(&envs));
        let content = envs.iter().find(|(k, _)| *k == "OLR_CONTENT_DIR").map(|(_, v)| v.clone());
        step("e2e", &mut || crate::e2e::run(content.as_deref()));
        let t = Instant::now();
        finish("net-nat", t, crate::nat::step(offline));
        step("bench", &mut || crate::bench::bench_step(&envs, false));
        step("stress", &mut || crate::bench::stress_step(content.as_deref(), false));
        step("net-feel", &mut || crate::net::feel_step(content.as_deref()));
        step("determinism-windows", &mut || {
            crate::determinism::step(crate::determinism::Opts { windows: true, ..Default::default() })
        });
        step("determinism-arm64", &mut || {
            crate::determinism::step(crate::determinism::Opts { arm64: true, ..Default::default() })
        });
    }
    let what = if gpu { "full" } else { "check" };
    let failed = failed.into_inner();
    if failed.is_empty() {
        println!("{what} passed");
        Ok(true)
    } else {
        println!("{what} failed: {}", failed.join(", "));
        Ok(false)
    }
}
