//! The runner's settings: defaults from the spec, overridable by an
//! optional `~/.config/olr-runner/settings.ron`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Everything the runner needs to know about this machine and repository.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `owner/name` on GitHub.
    pub repo: String,
    /// The only login whose approval and text count.
    pub maintainer: String,
    /// What the workspace clones and pushes to.
    pub remote_url: String,
    /// A session with no stream event for this long is stopped.
    pub idle_secs: u64,
    /// A session's active time (usage-limit waits excluded) is capped here.
    pub ceiling_secs: u64,
    /// The wait after a usage limit whose reset time is unknown.
    pub limit_fallback_secs: u64,
    /// Globs a pushed diff must not touch.
    pub protected: Vec<String>,
    /// The Claude Code executable.
    pub claude_bin: String,
    /// The gate's check, run in the workspace.
    pub check_cmd: Vec<String>,
    /// The gate's check, instead of `check_cmd`, when a change touches rendering.
    pub render_check_cmd: Vec<String>,
    /// Run the gate's checks inside bubblewrap (`gate::sandboxed`).
    pub gate_sandbox: bool,
    /// The runner's own clone.
    pub workspace: PathBuf,
    /// Lock, pause flag, current run, history and session logs.
    pub state_dir: PathBuf,
    /// The sessions' `CLAUDE_CONFIG_DIR`.
    pub claude_config_dir: PathBuf,
}

/// `$HOME`.
pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/".into()))
}

/// `~/.config/olr-runner`.
pub fn config_dir() -> PathBuf {
    home().join(".config/olr-runner")
}

impl Default for Settings {
    fn default() -> Self {
        let home = home();
        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        Settings {
            repo: "chvatma2/open-liero-reloaded".into(),
            maintainer: "chvatma2".into(),
            remote_url: "https://github.com/chvatma2/open-liero-reloaded.git".into(),
            idle_secs: 45 * 60,
            ceiling_secs: 8 * 60 * 60,
            limit_fallback_secs: 30 * 60,
            protected: strings(&[
                "crates/olr-runner/**",
                ".github/**",
                ".claude/**",
                "fixtures/oracle/*.jsonl.gz",
            ]),
            claude_bin: "claude".into(),
            check_cmd: strings(&["cargo", "xtask", "check"]),
            render_check_cmd: strings(&["cargo", "xtask", "check", "--render"]),
            gate_sandbox: true,
            workspace: home.join(".local/share/olr-runner/work"),
            state_dir: home.join(".local/state/olr-runner"),
            claude_config_dir: home.join(".config/olr-runner/claude"),
        }
    }
}

impl Settings {
    /// The defaults, overridden by `~/.config/olr-runner/settings.ron` if it exists.
    pub fn load() -> anyhow::Result<Settings> {
        let path = config_dir().join("settings.ron");
        match std::fs::read_to_string(&path) {
            Ok(text) => ron::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
        }
    }
}
