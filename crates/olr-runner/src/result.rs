//! The result file a session writes when it finishes.

use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Done,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionResult {
    pub status: Status,
    /// The summary (done) or the reason (blocked).
    pub text: String,
}

/// The first non-empty line is `done` or `blocked`; the rest is the text.
pub fn parse(text: &str) -> Result<SessionResult> {
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    let status = match lines.next().map(|l| l.trim().to_ascii_lowercase()).as_deref() {
        Some("done") => Status::Done,
        Some("blocked") => Status::Blocked,
        Some(other) => bail!("the result file starts with {other:?}, not `done` or `blocked`"),
        None => bail!("the result file is empty"),
    };
    Ok(SessionResult { status, text: lines.collect::<Vec<_>>().join("\n").trim().to_string() })
}
