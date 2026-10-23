//! `olr-cli asset save|place|check` (issue #9) and the built-in assets in
//! `content/assets/`: each reads whole and is what its command in the
//! README makes; the torch carries Castle Strike Reloaded's `flames`, not
//! its `sky`; a saved asset places into a level pack that checks clean.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assets_dir() -> PathBuf {
    repo().join("content/assets")
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).current_dir(repo()).args(args).output().unwrap()
}

fn text(o: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// The asset folders of `content/assets`, sorted.
fn built_in() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(assets_dir()).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

#[test]
fn built_in_assets_check() {
    let dirs = built_in();
    assert!(dirs.len() >= 3, "{dirs:?}");
    let mut args = vec!["asset", "check"];
    args.extend(dirs.iter().map(|d| d.to_str().unwrap()));
    let o = cli(&args);
    assert!(o.status.success(), "{}", text(&o));
    // And as the editor's Assets tab lists them: named, with a thumbnail.
    let (list, warnings) = olr_edit::asset::list_library(&assets_dir(), usize::MAX);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(list.len(), dirs.len());
    assert!(list.iter().all(|(_, s)| s.thumb.is_some() && s.glows), "every starter asset glows");
}

/// Every file under `dir`, by `/`-separated relative path, with its bytes.
fn tree(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn go(base: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_str().unwrap().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

/// Splits a README command line into its arguments (double quotes group).
fn words(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted) = (Vec::new(), String::new(), false);
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

#[test]
fn built_in_assets_are_what_their_commands_make() {
    let readme = std::fs::read_to_string(assets_dir().join("README.md")).unwrap();
    let commands: Vec<Vec<String>> =
        readme.lines().map(str::trim).filter(|l| l.starts_with("olr-cli asset save ")).map(words).collect();
    assert_eq!(commands.len(), built_in().len(), "one command per asset");
    let t = tempfile::tempdir().unwrap();
    for c in commands {
        let out = c.iter().position(|w| w == "--out").map(|i| c[i + 1].clone()).unwrap();
        let fresh = t.path().join(Path::new(&out).file_name().unwrap());
        let mut args: Vec<&str> = c[1..].iter().map(String::as_str).collect();
        let i = args.iter().position(|w| *w == "--out").unwrap();
        args[i + 1] = fresh.to_str().unwrap();
        let o = cli(&args);
        assert!(o.status.success(), "{c:?}: {}", text(&o));
        assert_eq!(tree(&fresh), tree(&repo().join(&out)), "{out}");
    }
}

#[test]
fn the_torch_carries_flames_not_sky() {
    let torch = olr_edit::asset::load_dir(&assets_dir().join("torch")).unwrap();
    let groups: Vec<&str> = torch.groups.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(groups, ["flames"]);
    assert!(matches!(torch.groups[0].animation, olr_format::look::AnimationSource::Flicker { .. }));
    assert!(torch.pieces.iter().all(|p| p.element.id != "background-2"), "no duplicate background");
    assert!(
        torch.pieces.iter().any(|p| p.element.glow.as_ref().is_some_and(|g| g.group == "flames" && g.mask))
    );
    // Without --layers, a rectangle reaching past the torch's hole cuts
    // `background-2` too (what the README's --layers avoids); its `sky`
    // mask is dark in the cavern, so its glow and group stay behind.
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("wide");
    let o = cli(&[
        "asset",
        "save",
        "content/reloaded/olr-level-castlestrike-reloaded",
        "--rect",
        "200,420,20,22",
        "--name",
        "Wide torch",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let wide = olr_edit::asset::load_dir(&out).unwrap();
    let ids: Vec<&str> = wide.pieces.iter().map(|p| p.element.id.as_str()).collect();
    assert_eq!(ids, ["background", "background-2"]);
    assert_eq!(wide.pieces[1].element.glow, None);
    assert!(wide.groups.iter().all(|g| g.id != "sky"), "{:?}", wide.groups);
}

#[test]
fn cli_saves_and_places_an_asset() {
    let t = tempfile::tempdir().unwrap();
    let level = t.path().join("empty");
    let o =
        cli(&["pack", "new", "level", "empty-level", "--out", level.to_str().unwrap(), "--size", "120x80"]);
    assert!(o.status.success(), "{}", text(&o));
    let level = level.to_str().unwrap();
    // A level without a look: the asset's lighting is added, and said.
    let o = cli(&["asset", "place", level, "content/assets/torch", "--at", "10,20"]);
    let out = text(&o);
    assert!(o.status.success(), "{out}");
    assert!(out.contains("added element torch") && out.contains("added glow group flames"), "{out}");
    assert!(out.contains(olr_edit::asset::ADDED_LIGHTING), "{out}");
    // Placed again: a new id and name, the group joined.
    let o = cli(&["asset", "place", level, "content/assets/torch", "--at", "40,20"]);
    let out = text(&o);
    assert!(out.contains("added element torch-2") && out.contains("joined glow group flames"), "{out}");
    let o = cli(&["pack", "check", level]);
    assert!(o.status.success(), "{}", text(&o));
    let o = cli(&["level", "element", "list", level]);
    assert!(text(&o).contains("torch-2 Drawn Background \"Torch 2\""), "{}", text(&o));
    // Saved back out by its elements, it is the torch moved.
    let out = t.path().join("again");
    let o = cli(&[
        "asset",
        "save",
        level,
        "--elements",
        "torch",
        "--name",
        "Torch",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let (a, b) = (
        olr_edit::asset::load_dir(&out).unwrap(),
        olr_edit::asset::load_dir(&assets_dir().join("torch")).unwrap(),
    );
    assert_eq!((a.size, &a.groups), (b.size, &b.groups));
    assert_eq!(a.pieces[0].decoded, b.pieces[0].decoded);
    assert_eq!(a.pieces[0].glow_mask, b.pieces[0].glow_mask);
}

/// Issue #59: an asset of several pieces lands in a folder named after it.
#[test]
fn cli_places_a_multi_piece_asset_in_a_folder() {
    let t = tempfile::tempdir().unwrap();
    let level = t.path().join("empty");
    let o =
        cli(&["pack", "new", "level", "empty-level", "--out", level.to_str().unwrap(), "--size", "120x80"]);
    assert!(o.status.success(), "{}", text(&o));
    let level = level.to_str().unwrap();
    let o = cli(&["asset", "place", level, "content/assets/ceiling-lamp", "--at", "10,20"]);
    let out = text(&o);
    assert!(o.status.success(), "{out}");
    assert!(
        out.contains(
            "added element ceiling-lamp-rock\nadded element ceiling-lamp-lamps\nadded folder ceiling-lamp\n"
        ),
        "{out}"
    );
    let o = cli(&["level", "element", "list", level]);
    assert_eq!(
        text(&o),
        "0: base Raster - \"Base\"\nfolder ceiling-lamp \"Ceiling lamp\"\n  1: ceiling-lamp-rock Drawn Rock \"Ceiling lamp Rock\" surface stone surface-map\n  2: ceiling-lamp-lamps Drawn Background \"Ceiling lamp Lamps\" glow 100% lamps surface glass\n",
    );
    let o = cli(&["pack", "check", level]);
    assert!(o.status.success(), "{}", text(&o));
    // One piece stays plain.
    let o = cli(&["asset", "place", level, "content/assets/torch", "--at", "60,20"]);
    assert!(!text(&o).contains("folder"), "{}", text(&o));
}

#[test]
fn cli_refuses_bad_assets_and_arguments() {
    let t = tempfile::tempdir().unwrap();
    let bad = t.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("asset.ron"), "(name: \"X\", size: (0, 0), elements: [])").unwrap();
    let o = cli(&["asset", "check", "content/assets/torch", bad.to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(text(&o).contains("bad: error:"), "{}", text(&o));
    // One of --elements and --rect.
    let o = cli(&[
        "asset",
        "save",
        "content/reloaded/olr-level-castlestrike-reloaded",
        "--name",
        "X",
        "--out",
        "x",
    ]);
    assert!(!o.status.success());
    let o = cli(&[
        "asset",
        "save",
        "content/reloaded/olr-level-castlestrike-reloaded",
        "--elements",
        "rock",
        "--rect",
        "0,0,4,4",
        "--name",
        "X",
        "--out",
        "x",
    ]);
    assert!(!o.status.success());
    assert!(!repo().join("x").exists());
    // A rectangle off the level.
    let o = cli(&[
        "asset",
        "save",
        "content/reloaded/olr-level-castlestrike-reloaded",
        "--rect",
        "5000,0,4,4",
        "--name",
        "X",
        "--out",
        t.path().join("off").to_str().unwrap(),
    ]);
    assert!(!o.status.success() && text(&o).contains("no pixels"), "{}", text(&o));
}
