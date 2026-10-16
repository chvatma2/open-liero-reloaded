//! `cargo xtask fetch-olx-content`: sparse-clones the pinned OpenLieroX commit
//! listed in `content/olx-manifest.ron` (git verifies integrity by hash) and
//! imports the listed items with `olr-cli import`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::root;

#[derive(Deserialize)]
struct Manifest {
    repository: String,
    commit: String,
    /// Directories needed by importers but not imported themselves (shared images, sounds).
    shared: Vec<String>,
    mods: Vec<String>,
    levels: Vec<String>,
    themes: Vec<String>,
    skins: bool,
    /// Files of the core pack `olx-core`.
    core: Vec<String>,
    /// Skins from outside the checkout, committed in the repository.
    #[serde(default)]
    extra_skins: Option<ExtraSkins>,
}

/// A committed directory of OLX skin PNGs with its origin.
#[derive(Deserialize)]
struct ExtraSkins {
    /// Relative to the repository root.
    dir: String,
    source: String,
    revision: String,
}

pub struct Paths {
    /// `share/gamedir` of the checkout.
    pub gamedir: PathBuf,
    /// Imported native packs.
    pub content: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().context("running git")?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn patterns(m: &Manifest) -> Vec<String> {
    let mut p: Vec<String> = Vec::new();
    for d in m.shared.iter().chain(&m.mods).chain(&m.themes) {
        p.push(format!("/share/gamedir/{d}/"));
    }
    for f in &m.levels {
        p.push(format!("/share/gamedir/{f}"));
    }
    if m.skins {
        p.push("/share/gamedir/skins/".into());
    }
    for f in &m.core {
        p.push(format!("/share/gamedir/{f}"));
    }
    p
}

fn checkout(m: &Manifest, dir: &Path) -> anyhow::Result<()> {
    let pats = patterns(m);
    let stamp = dir.join(".olr-fetched");
    let want = format!("{}\n{}", m.commit, pats.join("\n"));
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s == want) {
        return Ok(());
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    git(dir, &["init", "--quiet"])?;
    git(dir, &["remote", "add", "origin", &m.repository])?;
    let mut sparse = vec!["sparse-checkout", "set", "--no-cone"];
    sparse.extend(pats.iter().map(String::as_str));
    git(dir, &sparse)?;
    git(dir, &["fetch", "--quiet", "--depth", "1", "--filter=blob:none", "origin", &m.commit])?;
    git(dir, &["checkout", "--quiet", &m.commit])?;
    let head = git(dir, &["rev-parse", "HEAD"])?;
    if head != m.commit {
        bail!("checked out {head}, expected {}", m.commit);
    }
    std::fs::write(stamp, want)?;
    Ok(())
}

fn manifest(root: &Path) -> anyhow::Result<Manifest> {
    let text = std::fs::read_to_string(root.join("content/olx-manifest.ron"))?;
    ron::from_str(&text).context("parsing content/olx-manifest.ron")
}

/// The OLX mod directories the manifest imports, such as `"Classic"`.
pub fn mods() -> anyhow::Result<Vec<String>> {
    Ok(manifest(&root())?.mods)
}

/// The committed, converted packs (`content/packs`, the default `--content`).
pub const PACKS: &str = "content/packs";
/// Where `verify` imports afresh to compare with [`PACKS`].
const FRESH: &str = "target/content-fresh";

/// Fetches the pinned checkout and, if `import` is set, imports it with
/// `olr-cli` into [`PACKS`] (`cargo xtask fetch-olx-content`; commit the
/// result).
pub fn run(import: bool) -> anyhow::Result<Paths> {
    let root = root();
    let m = manifest(&root)?;
    let gamedir = fetch(&root, &m)?;
    let content = root.join(PACKS);
    if import {
        import_into(&root, &m, &gamedir, &content)?;
    }
    Ok(Paths { gamedir, content })
}

/// Fetches the pinned checkout, imports it into a scratch directory and
/// fails unless the committed [`PACKS`] are exactly that import (an
/// importer or manifest change not re-imported and committed).
pub fn verify() -> anyhow::Result<Paths> {
    let root = root();
    let m = manifest(&root)?;
    let gamedir = fetch(&root, &m)?;
    let fresh = root.join(FRESH);
    import_into(&root, &m, &gamedir, &fresh)?;
    let content = root.join(PACKS);
    let stale = differences(&fresh, &content)?;
    if !stale.is_empty() {
        let shown: Vec<_> = stale.iter().take(10).map(|d| format!("  {d}")).collect();
        let more = stale.len().saturating_sub(shown.len());
        bail!(
            "{PACKS} differs from a fresh import ({} files{}):\n{}\nrun `cargo xtask fetch-olx-content` and commit {PACKS}",
            stale.len(),
            if more > 0 { format!(", {more} more not shown") } else { String::new() },
            shown.join("\n")
        );
    }
    Ok(Paths { gamedir, content })
}

/// The pinned checkout's `share/gamedir`.
fn fetch(root: &Path, m: &Manifest) -> anyhow::Result<PathBuf> {
    let src = root.join("target/olx").join(&m.commit);
    checkout(m, &src).context("fetching OpenLieroX content")?;
    Ok(src.join("share/gamedir"))
}

/// Every file that differs between directories `a` (fresh) and `b`
/// (committed), by relative path, sorted: `+` only in the fresh import,
/// `-` only committed, `~` different bytes.
pub fn differences(a: &Path, b: &Path) -> anyhow::Result<Vec<String>> {
    let (fa, fb) = (files(a)?, files(b)?);
    let mut out = Vec::new();
    for (path, bytes) in &fa {
        match fb.get(path) {
            None => out.push(format!("+ {path}")),
            Some(other) if other != bytes => out.push(format!("~ {path}")),
            Some(_) => {}
        }
    }
    out.extend(fb.keys().filter(|p| !fa.contains_key(*p)).map(|p| format!("- {p}")));
    out.sort_by(|x, y| x[2..].cmp(&y[2..]));
    Ok(out)
}

/// The files under `dir` by `/`-separated relative path (none if `dir` is missing).
fn files(dir: &Path) -> anyhow::Result<std::collections::BTreeMap<String, Vec<u8>>> {
    let mut out = std::collections::BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries {
            let path = e?.path();
            if path.is_dir() {
                todo.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>();
                out.insert(rel.join("/"), std::fs::read(&path)?);
            }
        }
    }
    Ok(out)
}

/// Imports the manifest's items from `gamedir` into `out` (pruned: `out` is
/// this import's).
fn import_into(root: &Path, m: &Manifest, gamedir: &Path, out: &Path) -> anyhow::Result<()> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root).args(["run", "--quiet", "-p", "olr-cli", "--", "import", "--olx"]);
    cmd.arg(gamedir).arg("--out").arg(out).args(["--revision", &m.commit]);
    for x in &m.mods {
        cmd.args(["--mod", x]);
    }
    for x in &m.levels {
        cmd.args(["--level", x]);
    }
    for x in &m.themes {
        cmd.args(["--theme", x]);
    }
    if m.skins {
        cmd.arg("--skins");
    }
    for x in &m.core {
        cmd.args(["--core", x]);
    }
    if let Some(e) = &m.extra_skins {
        cmd.arg("--extra-skins").arg(&e.dir);
        cmd.args(["--extra-skins-source", &e.source, "--extra-skins-revision", &e.revision]);
    }
    let out = cmd.output().context("running olr-cli import")?;
    if !out.status.success() {
        bail!(
            "olr-cli import failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differences_name_every_stale_file() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let write = |root: &Path, rel: &str, bytes: &[u8]| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        };
        write(a.path(), "pack/same.ron", b"x");
        write(b.path(), "pack/same.ron", b"x");
        assert!(differences(a.path(), b.path()).unwrap().is_empty());
        write(a.path(), "pack/changed.png", b"new");
        write(b.path(), "pack/changed.png", b"old");
        write(a.path(), "new/pack.ron", b"n");
        write(b.path(), "gone/pack.ron", b"g");
        assert_eq!(
            differences(a.path(), b.path()).unwrap(),
            ["- gone/pack.ron", "+ new/pack.ron", "~ pack/changed.png"]
        );
    }
}
