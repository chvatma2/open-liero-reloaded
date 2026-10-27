//! The comments the runner leaves on an issue.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::gate::DEFERRED_FILE;
use crate::trust::{MARKER, Refusal};

/// Why an issue goes back to the maintainer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandBack {
    Untrusted(Refusal),
    Blocked(String),
    NoResult(String),
    Watchdog(&'static str),
    /// `git status --porcelain` lines.
    DirtyTree(Vec<String>),
    NoCommits,
    BadSubjects(Vec<String>),
    Protected(Vec<String>),
    CheckFailed(String),
    MainBroken,
    RebaseConflict,
    /// The session's commits do not descend from the `main` it started on.
    NotOnMain,
    PushFailed(String),
    /// The runner was interrupted and could not safely carry on.
    Interrupted(&'static str),
    /// The session could not run at all (login, network, the binary); the
    /// runner pauses rather than hand back the whole queue.
    Infrastructure(String),
}

impl HandBack {
    pub fn describe(&self) -> String {
        match self {
            HandBack::Untrusted(r) => format!("Not started: {}.", r.describe()),
            HandBack::Blocked(reason) => format!("The session stopped as blocked:\n\n{reason}"),
            HandBack::NoResult(why) => format!("The session ended without a usable result: {why}"),
            HandBack::Watchdog(why) => format!("The watchdog stopped the session: {why}."),
            HandBack::DirtyTree(paths) => format!("The session left uncommitted changes:\n{}", list(paths)),
            HandBack::NoCommits => "The session made no commits.".into(),
            HandBack::BadSubjects(s) => format!("Commit subjects without the issue number:\n{}", list(s)),
            HandBack::Protected(files) => format!("The change touches protected paths:\n{}", list(files)),
            HandBack::CheckFailed(tail) => format!("`cargo xtask check` failed on the change:\n\n```\n{tail}\n```"),
            HandBack::MainBroken => {
                "`cargo xtask check` fails on `main` itself, so nothing can be verified. The runner is paused \
                 (`olr-runner resume` once main is fixed)."
                    .into()
            }
            HandBack::RebaseConflict => "`main` moved during the session and the change no longer rebases cleanly.".into(),
            HandBack::NotOnMain => "The session rewrote history below the `main` it started on.".into(),
            HandBack::PushFailed(why) => format!("The push to `main` was refused:\n\n```\n{why}\n```"),
            HandBack::Interrupted(when) => format!("The runner was interrupted {when}."),
            HandBack::Infrastructure(why) => format!(
                "The session could not run: {why}\n\nThe runner is paused (`olr-runner resume` once fixed)."
            ),
        }
    }
}

fn list(items: &[String]) -> String {
    items.iter().map(|i| format!("- `{i}`")).collect::<Vec<_>>().join("\n")
}

/// A pushed issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {
    pub summary: String,
    pub first: String,
    pub last: String,
    pub commits: usize,
    pub claude_md: Vec<String>,
    pub new_packages: Vec<String>,
    pub deferred: bool,
    /// The change touched rendering, but no graphics adapter checked the goldens.
    #[serde(default)]
    pub render_unverified: bool,
}

pub fn done_comment(d: &Done) -> String {
    let mut c = format!(
        "{MARKER}\nImplemented by the issue runner: {} commit(s) on `main`, `{}..{}`.\n\n{}\n\nTo undo: `git revert --no-edit {}^..{}`",
        d.commits, d.first, d.last, d.summary, d.first, d.last
    );
    if !d.claude_md.is_empty() {
        c += &format!("\n\nEdited `CLAUDE.md` files (they steer later sessions):\n{}", list(&d.claude_md));
    }
    if !d.new_packages.is_empty() {
        c += &format!("\n\nNew dependencies:\n{}", list(&d.new_packages));
    }
    if d.deferred {
        c += &format!("\n\nNeeds heavier verification before release: see `{DEFERRED_FILE}`.");
    }
    if d.render_unverified {
        c += "\n\nThe render goldens were not checked (no graphics adapter): run `cargo xtask check --render` before release.";
    }
    c
}

pub fn handback_comment(h: &HandBack, log: &Path) -> String {
    format!(
        "{MARKER}\nHanded back to the maintainer.\n\n{}\n\nSession log on the runner's PC: `{}`\n\nUntil this is unblocked \
         the runner starts no newer issue of the milestone.\n\nTo retry: answer here if needed, then remove \
         `needs-maintainer` and add `approved`. To go on without it: remove `needs-maintainer`.",
        h.describe(),
        log.display()
    )
}
