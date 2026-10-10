//! `cargo xtask fetch-olx-content`: sparse-clones the pinned OpenLieroX commit
//! listed in `content/olx-manifest.ron` (git verifies integrity by hash) and
//! imports the listed items with `rl-cli import`.

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
    let stamp = dir.join(".rl-fetched");
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

/// Fetches the pinned checkout and, if `import` is set, imports it with `rl-cli`.
pub fn run(import: bool) -> anyhow::Result<Paths> {
    let root = root();
    let m = manifest(&root)?;
    let src = root.join("target/olx").join(&m.commit);
    checkout(&m, &src).context("fetching OpenLieroX content")?;
    let gamedir = src.join("share/gamedir");
    let content = root.join("target/content");
    if !import {
        return Ok(Paths { gamedir, content });
    }

    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(&root).args(["run", "--quiet", "-p", "rl-cli", "--", "import", "--olx"]);
    cmd.arg(&gamedir).arg("--out").arg(&content).args(["--revision", &m.commit]);
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
    let out = cmd.output().context("running rl-cli import")?;
    if !out.status.success() {
        bail!(
            "rl-cli import failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(Paths { gamedir, content })
}
