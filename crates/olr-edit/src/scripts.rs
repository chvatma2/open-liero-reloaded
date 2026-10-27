//! The scripts a placed level runs, checked by name (issue #89; the
//! asset-based levels design, call 20): `olr_script::check_level` on each
//! live copy's default script (its asset's, unless switched off) and own
//! script, and the level script, against the names the level's live
//! copies have ([`names`]). A level whose run scripts have a problem does
//! not save (`pack::save`, [`crate::pack::SaveError::Scripts`]); an asset
//! with a broken script still does (it runs nowhere until a live copy
//! uses it); a library version that would break a script is a
//! [`crate::library::Break::Script`].

use std::fmt;

use olr_format::levels::PlacedSource;
use olr_script::{CopyNames, RunScript, ScriptIssue, ScriptNames, check_level};

/// Which script a problem is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Whose {
    /// Live copy `copy`'s asset's default script (`asset`, the asset's
    /// name).
    Default { copy: String, asset: String },
    /// Live copy `copy`'s own script.
    Copy(String),
    /// The level script.
    Level,
}

impl Whose {
    /// The live copy the script runs on (none: the level script).
    pub fn copy(&self) -> Option<&str> {
        match self {
            Whose::Default { copy, .. } | Whose::Copy(copy) => Some(copy),
            Whose::Level => None,
        }
    }
}

impl fmt::Display for Whose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Whose::Default { copy, asset } => write!(f, "{copy}'s default script ({asset}'s)"),
            Whose::Copy(copy) => write!(f, "{copy}'s script"),
            Whose::Level => write!(f, "the level script"),
        }
    }
}

/// A problem of a script the level runs: `crane's script, line 3: copy
/// crane has no exposed value "claw"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptProblem {
    pub whose: Whose,
    pub issue: ScriptIssue,
}

impl fmt::Display for ScriptProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, line {}: {}", self.whose, self.issue.line, self.issue.message)
    }
}

/// The names `src`'s scripts may use: each live copy's id, its asset's
/// states, exposed names and sounds (a copy whose asset is missing has
/// none of them).
pub fn names(src: &PlacedSource) -> ScriptNames {
    let copies = src
        .file
        .copies
        .iter()
        .filter(|c| c.live)
        .map(|c| {
            let file = src.assets.get(&c.asset).map(|a| &a.file);
            CopyNames {
                id: c.id.clone(),
                states: file.map(|f| f.states.iter().map(|s| s.name.clone()).collect()).unwrap_or_default(),
                exposed: file.map(|f| f.exposed.iter().map(|x| x.name.clone()).collect()).unwrap_or_default(),
                sounds: file.map(|f| f.sounds.clone()).unwrap_or_default(),
            }
        })
        .collect();
    ScriptNames { copies }
}

/// The scripts `src` runs, in run order (as the played form gathers
/// them): the level script, then each live copy's default script (unless
/// switched off) and own script.
pub fn run_scripts(src: &PlacedSource) -> Vec<(Whose, &str)> {
    let mut out = Vec::new();
    if let Some(text) = &src.level_script {
        out.push((Whose::Level, text.as_str()));
    }
    for c in src.file.copies.iter().filter(|c| c.live) {
        if c.default_script
            && let Some(a) = src.assets.get(&c.asset)
            && let Some(text) = &a.script
        {
            out.push((Whose::Default { copy: c.id.clone(), asset: a.file.name.clone() }, text.as_str()));
        }
        if c.script
            && let Some(text) = src.scripts.get(&c.id)
        {
            out.push((Whose::Copy(c.id.clone()), text.as_str()));
        }
    }
    out
}

/// The problems of every script `src` runs ([`run_scripts`]) against its
/// [`names`], script by script, by line.
pub fn problems(src: &PlacedSource) -> Vec<ScriptProblem> {
    let runs = run_scripts(src);
    let scripts: Vec<RunScript<'_>> =
        runs.iter().map(|(w, text)| RunScript { copy: w.copy(), text }).collect();
    check_level(&names(src), &scripts)
        .into_iter()
        .filter_map(|i| Some(ScriptProblem { whose: runs.get(i.script)?.0.clone(), issue: i.issue }))
        .collect()
}
