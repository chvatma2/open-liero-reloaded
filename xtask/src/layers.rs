//! Enforces the crate layering in `xtask/layers.ron` (spec §4).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Rules {
    pub layers: BTreeMap<String, u32>,
    pub same_layer: Vec<(String, String)>,
    pub no_std: Vec<String>,
    pub wasm: Vec<String>,
}

pub fn load_rules() -> anyhow::Result<Rules> {
    let path = crate::root().join("xtask/layers.ron");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// A workspace crate and the workspace crates it depends on (normal and build
/// dependencies; dev-dependencies are only used by tests and are not checked).
pub struct Krate {
    pub name: String,
    pub deps: Vec<String>,
    pub lib_rs: Option<String>,
    pub has_claude_md: bool,
}

/// Every rule violation, as human-readable lines.
pub fn violations(rules: &Rules, krates: &[Krate]) -> Vec<String> {
    let mut out = Vec::new();
    let same: BTreeSet<(&str, &str)> =
        rules.same_layer.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    for k in krates {
        let Some(&layer) = rules.layers.get(&k.name) else {
            out.push(format!("{}: not listed in xtask/layers.ron", k.name));
            continue;
        };
        if !k.has_claude_md && k.name != "xtask" {
            out.push(format!("{}: missing CLAUDE.md", k.name));
        }
        for d in &k.deps {
            let Some(&dl) = rules.layers.get(d) else { continue };
            let allowed = dl < layer || (dl == layer && same.contains(&(k.name.as_str(), d.as_str())));
            if !allowed {
                out.push(format!("{} (layer {layer}) must not depend on {d} (layer {dl})", k.name));
            }
        }
        if rules.no_std.contains(&k.name) && !k.lib_rs.as_deref().unwrap_or("").contains("#![no_std]") {
            out.push(format!("{}: listed as no_std but src/lib.rs lacks #![no_std]", k.name));
        }
    }
    out
}

pub fn workspace_krates() -> anyhow::Result<Vec<Krate>> {
    let meta =
        cargo_metadata::MetadataCommand::new().manifest_path(crate::root().join("Cargo.toml")).exec()?;
    let members: BTreeSet<String> = meta.workspace_packages().iter().map(|p| p.name.to_string()).collect();
    Ok(meta
        .workspace_packages()
        .iter()
        .map(|p| {
            let dir = p.manifest_path.parent().expect("manifest has a dir");
            Krate {
                name: p.name.to_string(),
                deps: p
                    .dependencies
                    .iter()
                    .filter(|d| d.kind != cargo_metadata::DependencyKind::Development)
                    .filter(|d| members.contains(&d.name))
                    .map(|d| d.name.clone())
                    .collect(),
                lib_rs: std::fs::read_to_string(dir.join("src/lib.rs")).ok(),
                has_claude_md: dir.join("CLAUDE.md").is_file(),
            }
        })
        .collect())
}

pub fn run() -> anyhow::Result<()> {
    let v = violations(&load_rules()?, &workspace_krates()?);
    if v.is_empty() {
        return Ok(());
    }
    anyhow::bail!("layering violations:\n  {}", v.join("\n  "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules {
            layers: [("a", 1), ("b", 2), ("c", 2), ("d", 3)]
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            same_layer: vec![("b".into(), "c".into())],
            no_std: vec!["a".into()],
            wasm: vec![],
        }
    }

    fn k(name: &str, deps: &[&str]) -> Krate {
        Krate {
            name: name.into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            lib_rs: Some("#![no_std]".into()),
            has_claude_md: true,
        }
    }

    #[test]
    fn allows_downward_and_listed_same_layer() {
        assert!(violations(&rules(), &[k("a", &[]), k("b", &["a", "c"]), k("d", &["b"])]).is_empty());
    }

    #[test]
    fn rejects_upward_unlisted_same_layer_and_unknown_crates() {
        let v = violations(&rules(), &[k("a", &["b"]), k("c", &["b"]), k("zzz", &[])]);
        assert_eq!(
            v,
            vec![
                "a (layer 1) must not depend on b (layer 2)",
                "c (layer 2) must not depend on b (layer 2)",
                "zzz: not listed in xtask/layers.ron",
            ]
        );
    }

    #[test]
    fn requires_no_std_and_claude_md() {
        let mut a = k("a", &[]);
        a.lib_rs = Some("//! no attribute".into());
        a.has_claude_md = false;
        let v = violations(&rules(), &[a]);
        assert_eq!(v, vec!["a: missing CLAUDE.md", "a: listed as no_std but src/lib.rs lacks #![no_std]"]);
    }

    #[test]
    fn real_workspace_obeys_the_rules() {
        let v = violations(&load_rules().unwrap(), &workspace_krates().unwrap());
        assert!(v.is_empty(), "{v:#?}");
    }
}
