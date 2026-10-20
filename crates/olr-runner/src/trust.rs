//! What counts as the maintainer's task (spec: "Trusted and untrusted
//! text"). Only text the maintainer controls is trusted; everything else
//! is report material, and an edit by someone else after the approval
//! stops the run.

use crate::github::{IssueDetail, LabelEvent};

/// The first line of every comment the runner writes. The runner acts as
/// the maintainer's account, so this is how its own comments are told
/// apart from the maintainer's.
pub const MARKER: &str = "<!-- olr-runner -->";

/// A labelled piece of issue text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub label: String,
    pub text: String,
}

/// An issue split into the maintainer's task and untrusted report material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub number: u64,
    pub trusted: Vec<Part>,
    pub untrusted: Vec<Part>,
}

/// Why an issue cannot be worked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The latest `approved` was not added by the maintainer.
    NotApprovedByMaintainer,
    /// Someone else edited the body or the title after the approval.
    EditedAfterApproval { by: String },
    /// Nothing the maintainer wrote states the task.
    NoTrustedText,
}

impl Refusal {
    pub fn describe(&self) -> String {
        match self {
            Refusal::NotApprovedByMaintainer => "the `approved` label was not added by the maintainer".into(),
            Refusal::EditedAfterApproval { by } => {
                format!("@{by} edited the issue after it was approved; review the edit and approve again")
            }
            Refusal::NoTrustedText => {
                "nothing the maintainer wrote states the task: edit the body or add a comment saying what to build"
                    .into()
            }
        }
    }
}

/// The latest addition of the `approved` label.
pub fn approved_at(d: &IssueDetail) -> Option<&LabelEvent> {
    d.label_events.iter().rfind(|e| e.added && e.label == "approved")
}

/// Characters GitHub shows as nothing: Unicode tags, bidi controls and
/// zero-width characters.
fn invisible(c: char) -> bool {
    matches!(c, '\u{E0000}'..='\u{E007F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        | '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{00AD}')
}

/// Shows what GitHub hides, HTML comments and invisible characters, as
/// visible text.
pub fn reveal_hidden(text: &str) -> String {
    reveal_comments(text)
        .chars()
        .map(|c| if invisible(c) { format!("[U+{:04X}]", u32::from(c)) } else { c.to_string() })
        .collect()
}

fn reveal_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let inner = &rest[start + 4..];
        let (hidden, after) = match inner.find("-->") {
            Some(end) => (&inner[..end], &inner[end + 3..]),
            None => (inner, ""),
        };
        out.push_str("[hidden HTML comment: ");
        out.push_str(hidden);
        out.push(']');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Splits an approved issue into the maintainer's task and report material.
pub fn classify(d: &IssueDetail, maintainer: &str) -> Result<Task, Refusal> {
    let approval =
        approved_at(d).filter(|e| e.actor == maintainer).ok_or(Refusal::NotApprovedByMaintainer)?;
    let body_owner = d.editor.as_deref().unwrap_or(&d.author);
    if let Some(edited) = &d.last_edited_at
        && edited.as_str() >= approval.at.as_str()
        && body_owner != maintainer
    {
        return Err(Refusal::EditedAfterApproval { by: body_owner.to_string() });
    }
    if let Some(r) = d.renames.iter().find(|r| r.at >= approval.at && r.actor != maintainer) {
        return Err(Refusal::EditedAfterApproval { by: r.actor.clone() });
    }
    let title_owner = d.renames.last().map_or(d.author.as_str(), |r| r.actor.as_str());

    let mut task = Task { number: d.number, trusted: Vec::new(), untrusted: Vec::new() };
    let mut add = |trusted: bool, label: String, text: &str| {
        let part = Part { label, text: reveal_hidden(text) };
        if trusted { task.trusted.push(part) } else { task.untrusted.push(part) }
    };
    add(title_owner == maintainer, "Title".into(), &d.title);
    if !d.body.trim().is_empty() {
        add(body_owner == maintainer, "Body".into(), &d.body);
    }
    for c in &d.comments {
        if c.author == maintainer && c.body.starts_with(MARKER) {
            add(false, format!("Earlier runner report ({})", c.created_at), &c.body);
        } else {
            add(c.author == maintainer, format!("Comment by @{} ({})", c.author, c.created_at), &c.body);
        }
    }
    if task.trusted.is_empty() {
        return Err(Refusal::NoTrustedText);
    }
    Ok(task)
}
