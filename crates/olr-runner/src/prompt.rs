//! The session's prompt: the maintainer's task, fenced report material and
//! the rules (`prompt.md`).

use std::path::Path;

use crate::trust::{MARKER, Part, Task};

const TEMPLATE: &str = include_str!("prompt.md");
const FENCE_END: &str = "</untrusted-report>";

/// Untrusted text that can neither close the fence nor pose as a runner comment.
fn neutralise(text: &str) -> String {
    text.replace(FENCE_END, "</untrusted-report\u{200b}>").replace(MARKER, "<!-- olr-runner (quoted) -->")
}

fn section(parts: &[Part], clean: impl Fn(&str) -> String) -> String {
    if parts.is_empty() {
        return "(none)".into();
    }
    parts.iter().map(|p| format!("### {}\n\n{}", p.label, clean(&p.text))).collect::<Vec<_>>().join("\n\n")
}

/// The prompt for `task`, naming the result and notes files.
pub fn render(task: &Task, result: &Path, notes: &Path) -> String {
    TEMPLATE
        .replace("{task}", &section(&task.trusted, |t| t.to_string()))
        .replace("{report}", &section(&task.untrusted, neutralise))
        .replace("{issue}", &task.number.to_string())
        .replace("{result}", &result.display().to_string())
        .replace("{notes}", &notes.display().to_string())
}
