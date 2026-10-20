//! The comments the runner leaves on an issue.

use std::path::Path;

use crate::gate::DEFERRED_FILE;
use crate::trust::{MARKER, Refusal};

/// Why an issue goes back to the maintainer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandBack {
    Untrusted(Refusal),
    Blocked(String),
    NoResult(String),
    Watchdog(&'static str),
    DirtyTree,
    NoCommits,
    BadSubjects(Vec<String>),
    Protected(Vec<String>),
    CheckFailed(String),
    MainBroken,
    RebaseConflict,
}

impl HandBack {
    pub fn describe(&self) -> String {
        match self {
            HandBack::Untrusted(r) => format!("Not started: {}.", r.describe()),
            HandBack::Blocked(reason) => format!("The session stopped as blocked:\n\n{reason}"),
            HandBack::NoResult(why) => format!("The session ended without a usable result: {why}"),
            HandBack::Watchdog(why) => format!("The watchdog stopped the session: {why}."),
            HandBack::DirtyTree => "The session left uncommitted changes.".into(),
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
        }
    }
}

fn list(items: &[String]) -> String {
    items.iter().map(|i| format!("- `{i}`")).collect::<Vec<_>>().join("\n")
}

/// A pushed issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    pub summary: String,
    pub first: String,
    pub last: String,
    pub commits: usize,
    pub claude_md: Vec<String>,
    pub new_packages: Vec<String>,
    pub deferred: bool,
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
    c
}

pub fn handback_comment(h: &HandBack, log: &Path) -> String {
    format!(
        "{MARKER}\nHanded back to the maintainer.\n\n{}\n\nSession log on the runner's PC: `{}`\n\nTo retry: answer here if \
         needed, then remove `needs-maintainer` and add `approved`.",
        h.describe(),
        log.display()
    )
}
