//! The checks on a session's commits before the runner pushes them.

/// Where a session records verification it could not do.
pub const DEFERRED_FILE: &str = "docs/deferred-verification.md";

/// Paths whose change needs the render goldens (`cargo xtask check --render`).
const RENDER_PATHS: [&str; 8] = [
    "crates/olr-render/",
    "crates/olr-look/",
    "crates/olr-scene/",
    "crates/olr-present/",
    "crates/olr-fx/",
    "crates/olr-ui/",
    "fixtures/render/",
    "fixtures/ui/",
];

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
