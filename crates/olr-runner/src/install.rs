//! `olr-runner install`: the installed copy, the systemd timer, the
//! sessions' Claude Code config and the labels.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::github::GitHub;
use crate::settings::{Settings, home};

pub fn service_unit() -> String {
    "[Unit]\nDescription=OLR issue runner: one poll-and-work pass\n[Service]\nType=oneshot\n\
     ExecStart=%h/.local/bin/olr-runner tick\nTimeoutStartSec=infinity\n"
        .into()
}

pub fn timer_unit() -> String {
    "[Unit]\nDescription=OLR issue runner every 15 minutes\n[Timer]\nOnBootSec=2min\n\
     OnUnitInactiveSec=15min\n[Install]\nWantedBy=timers.target\n"
        .into()
}

/// The sessions' `settings.json`: no plugins, the output-compression hook,
/// and permissions bypassed inside a strict Bash sandbox that cannot reach
/// GitHub or the credentials.
pub fn session_settings(workspace: &Path, home: &Path) -> Value {
    let at = |p: &str| home.join(p).display().to_string();
    json!({
        "model": "opus",
        "effortLevel": "high",
        "autoMemoryEnabled": false,
        "promptCacheTtl": "1h",
        "autoCompactWindow": 400000,
        "skipDangerousModePermissionPrompt": true,
        "enabledPlugins": {},
        "hooks": {
            "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "rtk hook claude" }] }]
        },
        "permissions": {
            "defaultMode": "bypassPermissions",
            "deny": [
                "Bash(git push*)",
                "Bash(gh *)",
                "Read(~/.config/gh/**)",
                "Read(~/.git-credentials)",
                "Read(~/.ssh/**)"
            ]
        },
        "sandbox": {
            "enabled": true,
            "failIfUnavailable": true,
            "allowUnsandboxedCommands": false,
            "network": { "allowedDomains": ["crates.io", "*.crates.io"], "strictAllowlist": true },
            "filesystem": {
                "allowWrite": [workspace.display().to_string(), at(".cargo")],
                "denyRead": [at(".config/gh"), at(".git-credentials"), at(".ssh")]
            }
        }
    })
}

fn on_path(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn systemctl(args: &[&str]) -> Result<()> {
    let status =
        Command::new("systemctl").arg("--user").args(args).status().context("cannot run systemctl")?;
    anyhow::ensure!(status.success(), "systemctl --user {} failed", args.join(" "));
    Ok(())
}

/// Installs everything and prints each prerequisite as `ok` or `MISSING`.
pub fn run(s: &Settings, gh: &dyn GitHub) -> Result<()> {
    let home = home();
    let bin = home.join(".local/bin/olr-runner");
    std::fs::create_dir_all(bin.parent().expect("has a parent"))?;
    let exe = std::env::current_exe()?;
    if exe != bin {
        // Copy beside and rename, so a running timer never sees half a file.
        let tmp = bin.with_extension("new");
        std::fs::copy(&exe, &tmp)?;
        std::fs::rename(&tmp, &bin)?;
    }
    println!("ok   installed {}", bin.display());

    let units = home.join(".config/systemd/user");
    std::fs::create_dir_all(&units)?;
    std::fs::write(units.join("olr-runner.service"), service_unit())?;
    std::fs::write(units.join("olr-runner.timer"), timer_unit())?;
    std::fs::create_dir_all(&s.claude_config_dir)?;
    std::fs::create_dir_all(&s.state_dir)?;
    let settings = serde_json::to_string_pretty(&session_settings(&s.workspace, &home))?;
    std::fs::write(s.claude_config_dir.join("settings.json"), settings)?;
    println!("ok   wrote the systemd units and {}", s.claude_config_dir.join("settings.json").display());

    gh.ensure_labels()?;
    println!("ok   labels");

    let mut missing = false;
    let mut check = |ok: bool, what: &str, fix: &str| {
        if ok {
            println!("ok   {what}");
        } else {
            missing = true;
            println!("MISSING {what}: {fix}");
        }
    };
    let gh_ok = Command::new("gh").args(["auth", "status"]).output().is_ok_and(|o| o.status.success());
    check(gh_ok, "gh login", "run `gh auth login`");
    for program in ["bwrap", "socat", "ionice", "rtk", &s.claude_bin] {
        check(on_path(program), program, "install it and put it on PATH");
    }
    let login = s.claude_config_dir.join(".credentials.json").exists();
    let fix = format!(
        "run `! CLAUDE_CONFIG_DIR={} claude`, then /login, then install again",
        s.claude_config_dir.display()
    );
    check(login, "Claude login for the sessions", &fix);

    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", "olr-runner.timer"])?;
    println!("ok   olr-runner.timer enabled (every 15 minutes)");
    if missing {
        anyhow::bail!("prerequisites missing (see above)");
    }
    Ok(())
}
