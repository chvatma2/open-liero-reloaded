//! Pack validation with file and position information (spec §9.1).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rl_data::manifest::PackKind;
use rl_data::mod_def::ModDef;
use rl_vfs::{ContentSource, VPath};

use crate::{ModPack, PackError, read_level, read_manifest, read_mod, read_skin, read_theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
}

impl Diagnostic {
    fn new(severity: Severity, file: impl Into<String>, message: impl Into<String>) -> Self {
        Self { severity, file: Some(file.into()), line: None, col: None, message: message.into() }
    }

    fn from_error(e: &PackError) -> Self {
        match e {
            PackError::Ron { file, line, col, message } => Self {
                severity: Severity::Error,
                file: Some(file.clone()),
                line: Some(*line),
                col: Some(*col),
                message: message.clone(),
            },
            PackError::Image { file, message } | PackError::Invalid { file, message } => {
                Self::new(Severity::Error, file.clone(), message.clone())
            }
            PackError::Vfs(v) => {
                Self { severity: Severity::Error, file: None, line: None, col: None, message: v.to_string() }
            }
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{sev}: ")?;
        if let Some(file) = &self.file {
            write!(f, "{file}")?;
            if let (Some(l), Some(c)) = (self.line, self.col) {
                write!(f, ":{l}:{c}")?;
            }
            write!(f, ": ")?;
        }
        f.write_str(&self.message)
    }
}

/// Checks a pack and returns every problem found, errors first. An empty list
/// means the pack is valid.
pub fn validate(src: &dyn ContentSource) -> Vec<Diagnostic> {
    let manifest = match read_manifest(src) {
        Ok(m) => m,
        Err(e) => return vec![Diagnostic::from_error(&e)],
    };
    let mut out = Vec::new();
    let id_ok = !manifest.id.is_empty()
        && manifest.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !id_ok {
        out.push(Diagnostic::new(
            Severity::Error,
            "pack.ron",
            format!("id {:?} must be a lowercase slug", manifest.id),
        ));
    }
    let result = match manifest.kind {
        PackKind::Mod => read_mod(src).map(|p| out.extend(check_mod(&p))),
        PackKind::Level => read_level(src).map(|_| ()),
        PackKind::Skin => read_skin(src).map(|_| ()),
        PackKind::Theme => read_theme(src).map(|_| ()),
    };
    if let Err(e) = result {
        out.push(Diagnostic::from_error(&e));
    }
    out.sort_by_key(|d| d.severity);
    out
}

fn check_mod(pack: &ModPack) -> Vec<Diagnostic> {
    use Severity::*;
    let def = &pack.def;
    let mut out = Vec::new();

    let mut ids = BTreeSet::new();
    for w in &def.weapons {
        if !ids.insert(w.id.as_str()) {
            out.push(Diagnostic::new(Error, "mod.ron", format!("weapon {:?} is listed twice", w.id)));
        }
    }

    let mut referenced: BTreeSet<&str> = BTreeSet::new();
    let mut used_assets: BTreeSet<&str> = BTreeSet::new();
    for w in &def.weapons {
        let file = format!("weapons/{}.ron", w.id);
        if let Some(p) = w.projectile() {
            referenced.insert(p);
            if !def.projectiles.contains_key(p) {
                out.push(Diagnostic::new(Error, &file, format!("unknown projectile {p:?}")));
            }
        }
        if let Some(s) = w.sound() {
            used_assets.insert(s);
        }
    }
    for (name, p) in &def.projectiles {
        let file = format!("projectiles/{name}.ron");
        for c in p.children() {
            referenced.insert(c);
            if !def.projectiles.contains_key(c) {
                out.push(Diagnostic::new(Error, &file, format!("unknown projectile {c:?}")));
            }
        }
        used_assets.extend(p.assets());
    }
    for asset in &used_assets {
        let present = VPath::new(asset).map(|p| pack.assets.contains_key(&p)).unwrap_or(false);
        if !present {
            out.push(Diagnostic::new(Error, "mod.ron", format!("missing asset {asset:?}")));
        }
    }
    for name in def.projectiles.keys() {
        if !referenced.contains(name.as_str()) {
            out.push(Diagnostic::new(Warning, format!("projectiles/{name}.ron"), "projectile is never used"));
        }
    }
    for path in pack.assets.keys() {
        if !used_assets.contains(path.as_str()) {
            out.push(Diagnostic::new(Warning, path.as_str(), "asset is never used"));
        }
    }
    if let Some(cycle) = find_cycle(def) {
        out.push(Diagnostic::new(
            Error,
            format!("projectiles/{}.ron", cycle[0]),
            format!("projectiles spawn each other in a cycle: {}", cycle.join(" -> ")),
        ));
    }
    out
}

/// Returns the first cycle in the projectile spawn graph, if any, as a list of
/// names starting and ending with the same projectile.
fn find_cycle(def: &ModDef) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Visiting,
        Done,
    }
    fn visit<'a>(
        def: &'a ModDef,
        name: &'a str,
        state: &mut BTreeMap<&'a str, State>,
        stack: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        match state.get(name) {
            Some(State::Done) => return None,
            Some(State::Visiting) => {
                let start = stack.iter().position(|n| *n == name).expect("visiting nodes are on the stack");
                let mut cycle: Vec<String> = stack[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(name.to_string());
                return Some(cycle);
            }
            None => {}
        }
        let p = def.projectiles.get(name)?;
        state.insert(name, State::Visiting);
        stack.push(name);
        for c in p.children() {
            if let Some(cycle) = visit(def, c, state, stack) {
                return Some(cycle);
            }
        }
        stack.pop();
        state.insert(name, State::Done);
        None
    }
    let mut state = BTreeMap::new();
    for name in def.projectiles.keys() {
        if let Some(c) = visit(def, name, &mut state, &mut Vec::new()) {
            return Some(c);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level_pack::tests::sample_level;
    use crate::mod_pack::tests::sample_mod;
    use crate::{write_level, write_mod};
    use rl_data::mod_def::SpawnDef;
    use rl_vfs::MemFs;

    fn written(pack: &ModPack) -> MemFs {
        let mut fs = MemFs::new();
        write_mod(&mut fs, pack).unwrap();
        fs
    }

    #[test]
    fn valid_packs_have_no_diagnostics() {
        assert_eq!(validate(&written(&sample_mod())), vec![]);
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        assert_eq!(validate(&fs), vec![]);
    }

    #[test]
    fn reports_unknown_projectile() {
        let mut pack = sample_mod();
        pack.def.projectiles.get_mut("rocket").unwrap().event_spawn =
            Some(SpawnDef::olx_default("nope".into()));
        let d = validate(&written(&pack));
        assert!(
            d.iter().any(|d| d.to_string() == "error: projectiles/rocket.ron: unknown projectile \"nope\""),
            "{d:?}"
        );
    }

    #[test]
    fn reports_cycles() {
        let mut pack = sample_mod();
        pack.def.projectiles.get_mut("bits").unwrap().event_spawn =
            Some(SpawnDef::olx_default("rocket".into()));
        let d = validate(&written(&pack));
        let msg = d.iter().find(|d| d.message.contains("cycle")).expect("cycle reported");
        assert_eq!(msg.severity, Severity::Error);
        assert!(msg.message.contains("bits -> rocket -> bits"), "{}", msg.message);
    }

    #[test]
    fn reports_missing_asset_and_unused_items() {
        let mut pack = sample_mod();
        pack.assets.remove(&VPath::new("sfx/boom.ogg").unwrap());
        pack.assets.insert(VPath::new("gfx/extra.png").unwrap(), b"x".to_vec());
        let d: Vec<String> = validate(&written(&pack)).iter().map(|d| d.to_string()).collect();
        assert!(d.contains(&"error: mod.ron: missing asset \"sfx/boom.ogg\"".to_string()), "{d:?}");
        assert!(d.contains(&"warning: gfx/extra.png: asset is never used".to_string()), "{d:?}");
    }

    #[test]
    fn reports_parse_errors_with_position() {
        let fs = written(&sample_mod()).with("mod.ron", "(name: \"x\",\n oops)");
        let d = validate(&fs);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].file.as_deref(), d[0].line), (Some("mod.ron"), Some(2)));
    }

    #[test]
    fn reports_missing_manifest() {
        let d = validate(&MemFs::new());
        assert_eq!(d[0].severity, Severity::Error);
        assert!(d[0].message.contains("pack.ron"), "{}", d[0].message);
    }
}
