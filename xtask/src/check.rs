//! `cargo xtask check`: every step runs; only failing steps print their output.

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

    if failed.is_empty() {
        println!("check passed");
        Ok(true)
    } else {
        println!("check failed: {}", failed.join(", "));
        Ok(false)
    }
}
