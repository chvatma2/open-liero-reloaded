//! A mod's errors and warnings, each with its field path (issue #16): the
//! editor lists them and jumps to each. `olr_format::mods::mod_errors`
//! gives every compile error (`compile_mod` stops at the first), missing
//! assets are named at the field naming them, a file that does not parse
//! is reported with every other one that does not, and
//! `olr_pack::validate`'s other diagnostics (unused projectiles and
//! assets, the manifest) follow.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use olr_data::manifest::PackKind;
use olr_format::FormatError;
use olr_format::mods::{ModSource, RESTRICTIONS_FILE, mod_errors, read_mod_files};
use olr_pack::{Diagnostic, Severity};
use olr_vfs::{ContentSink, ContentSource, DirFs, MemFs, VPath, walk};

use super::path::{File, parse_segs};

/// One problem of a mod.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// The file, by pack path (`weapons/bazooka.ron`), when one is named.
    pub file: Option<String>,
    /// The field's path ([`super::get`]'s), such as
    /// `weapons/bazooka.fire.shot.amount`, when one is known; a file's
    /// whole path (`projectiles/x`) for an issue of the file as a whole.
    pub path: Option<String>,
    /// A parse error's place in the file.
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
}

impl Issue {
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    fn error(file: Option<String>, path: Option<String>, message: String) -> Self {
        Issue { severity: Severity::Error, file, path, line: None, col: None, message }
    }

    /// `e` located: an invalid value at its field's path.
    pub(crate) fn of_format(e: &FormatError) -> Issue {
        match e {
            FormatError::Invalid { file, path, message } => {
                Issue::error(Some(file.clone()), locate(file, path), message.clone())
            }
            FormatError::Parse { file, line, col, message } => Issue {
                severity: Severity::Error,
                file: Some(file.clone()),
                path: None,
                line: Some(*line),
                col: Some(*col),
                message: message.clone(),
            },
            FormatError::Io { file, message } => Issue::error(Some(file.clone()), None, message.clone()),
        }
    }

    fn of_diagnostic(d: &Diagnostic) -> Issue {
        // A whole definition file (an unused shared projectile): its path.
        let path = d.file.as_deref().and_then(File::of_pack_path).map(|f| f.to_string());
        Issue {
            severity: d.severity,
            file: d.file.clone(),
            path,
            line: d.line,
            col: d.col,
            message: d.message.clone(),
        }
    }

    /// What `olr_pack::validate` would print for the same error: its
    /// file, place and message (the field path inside the message).
    fn as_diagnostic(e: &FormatError) -> (Option<String>, Option<usize>, Option<usize>, String) {
        match e {
            FormatError::Invalid { file, path, message } => {
                let m = if path.is_empty() { message.clone() } else { format!("{path}: {message}") };
                (Some(file.clone()), None, None, m)
            }
            FormatError::Parse { file, line, col, message } => {
                (Some(file.clone()), Some(*line), Some(*col), message.clone())
            }
            FormatError::Io { file, message } => (Some(file.clone()), None, None, message.clone()),
        }
    }
}

impl fmt::Display for Issue {
    /// `error: weapons/bazooka.fire.shot.amount: …`, or the file and its
    /// place when no field is known.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{sev}: ")?;
        match (&self.path, &self.file) {
            (Some(p), _) => write!(f, "{p}: ")?,
            (None, Some(file)) => {
                write!(f, "{file}")?;
                if let (Some(l), Some(c)) = (self.line, self.col) {
                    write!(f, ":{l}:{c}")?;
                }
                write!(f, ": ")?;
            }
            (None, None) => {}
        }
        f.write_str(&self.message)
    }
}

/// The path of field `path` (an `olr-format` error path) of `file` (a pack
/// path): `weapons/bazooka.fire.shot.projectile.spawns.spawn`.
fn locate(file: &str, path: &str) -> Option<String> {
    let file = File::of_pack_path(file)?;
    let printed: String = match parse_segs(path) {
        Ok(segs) => segs.iter().map(|s| s.to_string()).collect(),
        Err(_) => format!(".{path}"),
    };
    Some(format!("{file}{}", if path.is_empty() { "" } else { &printed }))
}

/// The errors of `src` with assets `assets` (pack paths): every compile
/// error, then each asset a field names that the pack lacks.
pub(crate) fn source_issues(src: &ModSource, assets: &BTreeSet<String>) -> Vec<Issue> {
    let mut out: Vec<Issue> = mod_errors(src).iter().map(Issue::of_format).collect();
    for (asset, path) in super::walk::assets(src) {
        if !assets.contains(&asset) {
            out.push(Issue::error(
                Some(path.file.pack_path()),
                Some(path.to_string()),
                format!("missing asset {asset:?}"),
            ));
        }
    }
    out
}

/// Every error, then every warning, of the mod pack at `dir`.
pub fn check_dir(dir: &Path) -> Vec<Issue> {
    check_files(&DirFs::new(dir))
}

/// Every error, then every warning, of the pack `pack` would save as.
pub fn check(pack: &crate::pack::Pack) -> Vec<Issue> {
    let files = match crate::pack::files(pack) {
        Ok(f) => f,
        Err(e) => return vec![Issue::error(e.file().map(String::from), None, e.to_string())],
    };
    let mut mem = MemFs::new();
    for (path, bytes) in &files {
        if let Err(e) = mem.write(path, bytes) {
            return vec![Issue::error(Some(path.to_string()), None, e.to_string())];
        }
    }
    check_files(&mem)
}

/// Every error, then every warning, of the mod pack `src`.
pub fn check_files(src: &dyn ContentSource) -> Vec<Issue> {
    let diagnostics = olr_pack::validate(src);
    let is_mod = olr_pack::read_manifest(src).is_ok_and(|m| m.kind == PackKind::Mod);
    let mut out = Vec::new();
    // What `validate` reports of the definitions (it stops at the first
    // error): listed here in full instead.
    let mut listed = BTreeSet::new();
    if is_mod {
        match read_mod_files(src) {
            Ok(m) => {
                let assets: BTreeSet<String> = walk(src, &VPath::root())
                    .map(|paths| paths.iter().map(|p| p.to_string()).collect())
                    .unwrap_or_default();
                let errors = mod_errors(&m);
                listed.extend(errors.iter().map(Issue::as_diagnostic));
                out = source_issues(&m, &assets);
                if !errors.is_empty() {
                    // `validate` warns only about a mod that compiles.
                    out.extend(unused(&m, &assets));
                }
            }
            Err(e) => {
                listed.insert(Issue::as_diagnostic(&e));
                // The reader's error, then every file that does not parse.
                let first = Issue::of_format(&e);
                out = file_errors(src);
                if !out.contains(&first) {
                    out.insert(0, first);
                }
            }
        }
    }
    for d in &diagnostics {
        let key = (d.file.clone(), d.line, d.col, d.message.clone());
        // Its missing assets are named above at the fields naming them.
        let missing_asset = d.file.as_deref() == Some("mod.ron") && d.message.starts_with("missing asset ");
        if is_mod && d.severity == Severity::Error && (listed.contains(&key) || missing_asset) {
            continue;
        }
        out.push(Issue::of_diagnostic(d));
    }
    out.sort_by_key(|i| i.severity);
    out
}

/// `olr_pack::validate`'s warnings about what nothing uses, from the
/// source: shared projectiles no spawn names, and files in `gfx/` and
/// `sfx/` no field names.
fn unused(src: &ModSource, assets: &BTreeSet<String>) -> Vec<Issue> {
    let warn = |file: String, message: &str| {
        let path = File::of_pack_path(&file).map(|f| f.to_string());
        Issue {
            severity: Severity::Warning,
            file: Some(file),
            path,
            line: None,
            col: None,
            message: message.into(),
        }
    };
    let mut out: Vec<Issue> = super::used_by(src)
        .into_iter()
        .filter(|(_, users)| users.is_empty())
        .map(|(id, _)| warn(File::Shared(id).pack_path(), "projectile is never used"))
        .collect();
    let named: BTreeSet<String> = super::walk::assets(src).into_iter().map(|(a, _)| a).collect();
    out.extend(
        assets
            .iter()
            .filter(|a| (a.starts_with("gfx/") || a.starts_with("sfx/")) && !named.contains(*a))
            .map(|a| warn(a.clone(), "asset is never used")),
    );
    out
}

/// Each definition file of `src` that does not parse as its type, with
/// its error (`olr_format::canonical_text` reads one file alone).
fn file_errors(src: &dyn ContentSource) -> Vec<Issue> {
    let Ok(paths) = walk(src, &VPath::root()) else { return Vec::new() };
    let mut out = Vec::new();
    for p in paths {
        let s = p.as_str();
        let definition = s == "mod.ron"
            || s == RESTRICTIONS_FILE
            || ((s.starts_with("weapons/") || s.starts_with("projectiles/"))
                && s.ends_with(".ron")
                && !s.rsplit('/').next().is_some_and(|n| n.starts_with('.')));
        if !definition {
            continue;
        }
        let text = match src.read_max(&p, olr_format::ron_io::MAX_RON_BYTES) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                out.push(Issue::error(Some(s.to_string()), None, e.to_string()));
                continue;
            }
        };
        if let Err(e) = olr_format::canonical_text(s, &text) {
            out.push(Issue::of_format(&e));
        }
    }
    out
}
