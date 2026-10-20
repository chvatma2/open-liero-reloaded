//! The checks on a session's commits before the runner pushes them.

use std::path::Path;

/// Where a session records verification it could not do.
pub const DEFERRED_FILE: &str = "docs/deferred-verification.md";

/// Paths whose change needs the render goldens (`cargo xtask check
/// --render`): the renderer and what it draws (the simulation, terrain,
/// scenarios and content the goldens replay).
const RENDER_PATHS: [&str; 14] = [
    "crates/olr-render/",
    "crates/olr-look/",
    "crates/olr-scene/",
    "crates/olr-present/",
    "crates/olr-fx/",
    "crates/olr-ui/",
    "crates/olr-sim/",
    "crates/olr-terrain/",
    "crates/olr-scenario/",
    "crates/olr-content/",
    "content/packs/",
    "fixtures/render/",
    "fixtures/ui/",
    "fixtures/look/",
];

/// `cmd` inside bubblewrap for the gate, which builds and runs the
/// session's code: the file system read-only but for the workspace,
/// Cargo's directory and `/tmp`; the GitHub, SSH and Claude credentials
/// and the runner's own config hidden; at the lowest CPU and I/O priority.
/// The network stays (the online check fetches OLX).
pub fn sandboxed(cmd: &[String], workspace: &Path, home: &Path) -> Vec<String> {
    let s = |p: &Path| p.display().to_string();
    let mut out: Vec<String> =
        ["bwrap", "--ro-bind", "/", "/", "--dev-bind", "/dev", "/dev", "--proc", "/proc"]
            .map(String::from)
            .to_vec();
    out.extend(["--bind".into(), "/tmp".into(), "/tmp".into()]);
    out.extend(["--bind".into(), s(workspace), s(workspace)]);
    let cargo = home.join(".cargo");
    if cargo.is_dir() {
        out.extend(["--bind".into(), s(&cargo), s(&cargo)]);
    }
    for hidden in [".config/gh", ".ssh", ".config/olr-runner", ".claude"] {
        let dir = home.join(hidden);
        if dir.is_dir() {
            out.extend(["--tmpfs".into(), s(&dir)]);
        }
    }
    let creds = home.join(".git-credentials");
    if creds.exists() {
        out.extend(["--ro-bind".into(), "/dev/null".into(), s(&creds)]);
    }
    out.extend(["--chdir".into(), s(workspace), "--".into()]);
    out.extend(["nice", "-n", "19", "ionice", "-c", "3"].map(String::from));
    out.extend(cmd.iter().cloned());
    out
}

/// Subjects that do not end in `(#<issue>)`.
pub fn bad_subjects(subjects: &[String], issue: u64) -> Vec<String> {
    let suffix = format!("(#{issue})");
    subjects.iter().filter(|s| !s.trim_end().ends_with(&suffix)).cloned().collect()
}

/// `*` within one path segment; `**` as a whole segment matches the rest.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn segments(pattern: &[&str], path: &[&str]) -> bool {
        match (pattern.first(), path.first()) {
            (Some(&"**"), _) => !path.is_empty(),
            (Some(p), Some(s)) => wildcard(p.as_bytes(), s.as_bytes()) && segments(&pattern[1..], &path[1..]),
            (None, None) => true,
            _ => false,
        }
    }
    fn wildcard(p: &[u8], s: &[u8]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((b'*', rest)) => (0..=s.len()).any(|i| wildcard(rest, &s[i..])),
            Some((c, rest)) => s.first() == Some(c) && wildcard(rest, &s[1..]),
        }
    }
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    segments(&pattern, &path)
}

/// The changed files that match a protected pattern.
pub fn protected_hits(files: &[String], patterns: &[String]) -> Vec<String> {
    files.iter().filter(|f| patterns.iter().any(|p| glob_match(p, f))).cloned().collect()
}

pub fn touches_render(files: &[String]) -> bool {
    files.iter().any(|f| RENDER_PATHS.iter().any(|p| f.starts_with(p)))
}

/// Changed `CLAUDE.md` files: allowed, but they steer every later session,
/// so the report names them.
pub fn claude_md_edits(files: &[String]) -> Vec<String> {
    files.iter().filter(|f| *f == "CLAUDE.md" || f.ends_with("/CLAUDE.md")).cloned().collect()
}

/// `name version` of the `Cargo.lock` packages in `new_lock` but not in `old_lock`.
pub fn new_packages(old_lock: &str, new_lock: &str) -> Vec<String> {
    fn packages(lock: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut name = None;
        for line in lock.lines() {
            let value = |key: &str| line.strip_prefix(key).map(|v| v.trim().trim_matches('"').to_string());
            if line.starts_with("[[package]]") {
                name = None;
            } else if let Some(n) = value("name = ") {
                name = Some(n);
            } else if let (Some(v), Some(n)) = (value("version = "), name.take()) {
                out.push(format!("{n} {v}"));
            }
        }
        out
    }
    let old = packages(old_lock);
    packages(new_lock).into_iter().filter(|p| !old.contains(p)).collect()
}
