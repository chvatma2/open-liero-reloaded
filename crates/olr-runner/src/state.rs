//! The runner's state on this machine: the lock, the pause flag, the run in
//! progress (for crash recovery), the history and the session logs.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::report::Done;

pub struct State {
    pub dir: PathBuf,
}

/// How far the run in progress got: what a recovery may still do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// The issue is being claimed; no session yet. A recovery hands it back.
    Claimed,
    /// The session is running. A recovery resumes it once.
    Session,
    /// The session finished; its commits are being checked and pushed. A
    /// recovery runs the gate again, never the session.
    Gate { summary: String },
    /// Pushed; only the report is left.
    Pushed(Done),
}

/// The run in progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Current {
    pub issue: u64,
    pub session_id: String,
    /// `origin/main` when the run started.
    pub base: String,
    /// Session running time so far (usage-limit waits excluded).
    pub active_secs: u64,
    pub log: PathBuf,
    pub phase: Phase,
    /// Times the session was resumed after an interruption.
    pub resumes: u32,
}

fn unix_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

impl State {
    pub fn new(dir: &Path) -> Result<State> {
        std::fs::create_dir_all(dir.join("runs"))
            .with_context(|| format!("cannot create {}", dir.display()))?;
        Ok(State { dir: dir.to_path_buf() })
    }

    /// The machine-wide lock, held while the returned file lives; `None`
    /// when another runner holds it.
    pub fn lock(&self) -> Result<Option<File>> {
        let file = OpenOptions::new().create(true).truncate(false).write(true).open(self.dir.join("lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e.into()),
        }
    }

    pub fn paused(&self) -> bool {
        self.dir.join("paused").exists()
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        let path = self.dir.join("paused");
        if paused {
            std::fs::write(path, "")?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn current(&self) -> Option<Current> {
        ron::from_str(&std::fs::read_to_string(self.dir.join("current.ron")).ok()?).ok()
    }

    pub fn set_current(&self, current: Option<&Current>) -> Result<()> {
        let path = self.dir.join("current.ron");
        match current {
            Some(c) => {
                // Written beside and renamed, so a kill never leaves half a file.
                let tmp = path.with_extension("ron.new");
                std::fs::write(&tmp, ron::to_string(c)?)?;
                std::fs::rename(tmp, path)?;
            }
            None if path.exists() => std::fs::remove_file(path)?,
            None => {}
        }
        Ok(())
    }

    /// Appends `line` to the history, unless it repeats the last entry's
    /// message (an idle runner polls every 15 minutes).
    pub fn record(&self, line: &str) -> Result<()> {
        if self.history(1).first().and_then(|h| h.split_once(' ')).is_some_and(|(_, last)| last == line) {
            return Ok(());
        }
        let mut f = OpenOptions::new().create(true).append(true).open(self.dir.join("history.log"))?;
        writeln!(f, "{} {line}", unix_secs())?;
        Ok(())
    }

    /// The last `n` history lines (`<unix secs> <message>`), oldest first.
    pub fn history(&self, n: usize) -> Vec<String> {
        let text = std::fs::read_to_string(self.dir.join("history.log")).unwrap_or_default();
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines[lines.len().saturating_sub(n)..].to_vec()
    }

    /// When the workspace's `target/` was last emptied (unix seconds;
    /// `run::fresh_start`), if ever.
    pub fn fresh_start(&self) -> Option<u64> {
        std::fs::read_to_string(self.dir.join("fresh-start")).ok()?.trim().parse().ok()
    }

    pub fn set_fresh_start(&self, secs: u64) -> Result<()> {
        std::fs::write(self.dir.join("fresh-start"), secs.to_string())?;
        Ok(())
    }

    /// Where a running session's process id is kept, so a later tick can
    /// stop a session an interrupted runner left behind.
    pub fn session_pid(&self) -> PathBuf {
        self.dir.join("session.pid")
    }

    /// A new session log for `issue`.
    pub fn new_log(&self, issue: u64) -> PathBuf {
        self.dir.join("runs").join(format!("{issue}-{}.jsonl", unix_secs()))
    }
}
