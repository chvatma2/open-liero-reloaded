//! The session's prompt: the maintainer's task, fenced report material and
//! the rules (`prompt.md`).

use std::path::Path;

use crate::trust::{MARKER, Part, Task};

const TEMPLATE: &str = include_str!("prompt.md");

/// Untrusted text that can neither close the fence (in any case or
/// spacing) nor pose as a runner comment.
fn neutralise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("</") {
        let after = &rest[at + 2..];
        let squeezed: String = after.chars().filter(|c| !c.is_whitespace()).take(17).collect();
        out.push_str(&rest[..at]);
        // `<\/` reads the same to a person and closes nothing.
        out.push_str(if squeezed.eq_ignore_ascii_case("untrusted-report>") { "<\\/" } else { "</" });
        rest = after;
    }
    out.push_str(rest);
    out.replace(MARKER, "<!-- olr-runner (quoted) -->")
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
