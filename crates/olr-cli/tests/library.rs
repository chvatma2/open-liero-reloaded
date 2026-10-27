//! `olr-cli level library …`, `level export` and `level import` (issue
//! #76): a level's links to temporary libraries reported, updated,
//! restored, adopted and relinked; a level exported and imported into an
//! empty data folder, checking clean and playing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::asset::{build_lamp, tree};
use crate::common::{MATCH_PACKS, copy_dir};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).current_dir(repo()).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn ok(args: &[&str]) -> String {
    let o = cli(args);
    assert!(o.status.success(), "{args:?}: {}", text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `olr-cli <args>` refused, `pack` left byte for byte: its stderr.
fn refused(args: &[&str], pack: &Path) -> String {
    let before = tree(pack);
    let o = cli(args);
    assert!(!o.status.success(), "{args:?} succeeded: {}", text(&o));
    assert!(tree(pack) == before, "{args:?} changed the pack");
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn checks_clean(pack: &str) {
    let out = ok(&["pack", "check", "--content", "content/packs", pack]);
    assert!(out.contains("0 errors, 0 warnings"), "{out}");
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// A new plain level `id` in `t`.
fn new_level(t: &Path, id: &str) -> PathBuf {
    let dir = t.join(id);
    ok(&["pack", "new", "level", id, "--out", s(&dir), "--size", "200x120", "--name", "Lvl"]);
    dir
}

#[test]
fn cli_reports_updates_restores_adopts_and_relinks() {
    let t = tempfile::tempdir().unwrap();
    let user = t.path().join("user");
    let lamp = user.join("lamp");
    copy_dir(&build_lamp(t.path()), &lamp);
    let dir = new_level(t.path(), "lvl");
    let (p, u) = (s(&dir), s(&user));
    ok(&["level", "copy", "add", p, s(&lamp), "--at", "2,2", "--id", "lamp", "--user", u]);
    ok(&["level", "copy", "set", p, "lamp", "--live", "--state", "On", "--value", "lamps_glow=50"]);
    ok(&["level", "copy", "add", p, "content/assets/strip-light", "--at", "60,2", "--id", "strip"]);
    checks_clean(p);
    let library = || ok(&["level", "library", p, "--user", u]);
    assert_eq!(library(), "a: same (yours lamp)\nb: same (built-in strip-light)\n");

    // A change breaking nothing: reported, then taken.
    ok(&["asset", "state", "value", s(&lamp), "On", "lamp.glow", "40"]);
    assert_eq!(library(), "a: changed (yours lamp)\nb: same (built-in strip-light)\n");
    assert_eq!(ok(&["level", "library", "update", p, "a", "--user", u]), "updated a\n");
    checks_clean(p);
    assert_eq!(library(), "a: same (yours lamp)\nb: same (built-in strip-light)\n");

    // A breaking one: reported with its break, refused, then forced.
    ok(&["asset", "unexpose", s(&lamp), "lamps_glow"]);
    assert_eq!(
        library(),
        "a: changed (yours lamp)\n  lamp: value lamps_glow: no exposed property lamps_glow\n\
         b: same (built-in strip-light)\n"
    );
    let e = refused(&["level", "library", "update", p, "a", "--user", u], &dir);
    assert!(e.contains("lamp: value lamps_glow: no exposed property lamps_glow"), "{e}");
    assert_eq!(
        ok(&["level", "library", "update", p, "--all", "--force", "--user", u]),
        "updated a\nlamp: dropped value lamps_glow\n"
    );
    checks_clean(p);
    assert_eq!(library(), "a: same (yours lamp)\nb: same (built-in strip-light)\n");

    // The user's own copy of the built-in strip light, followed.
    let mine = user.join("strip-light-2");
    copy_dir(&repo().join("content/assets/strip-light"), &mine);
    ok(&["asset", "state", "add", s(&mine), "Off"]);
    assert_eq!(
        ok(&["level", "library", "relink", p, "b", "--yours", "strip-light-2", "--user", u]),
        "b now follows yours strip-light-2\n"
    );
    checks_clean(p);
    assert_eq!(library(), "a: same (yours lamp)\nb: same (yours strip-light-2)\n");

    // A missing one given back from the level's copy.
    std::fs::remove_dir_all(&lamp).unwrap();
    assert_eq!(library(), "a: missing (yours lamp)\nb: same (yours strip-light-2)\n");
    let e = refused(&["level", "library", "update", p, "a", "--user", u], &dir);
    assert!(e.contains("restore"), "{e}");
    assert_eq!(ok(&["level", "library", "restore", p, "a", "--user", u]), "restored a as yours strip-lamp\n");
    checks_clean(p);
    assert!(user.join("strip-lamp/asset.ron").is_file());
    assert_eq!(library(), "a: same (yours strip-lamp)\nb: same (yours strip-light-2)\n");

    // Adopted: the level's own, linked no more.
    assert_eq!(ok(&["level", "library", "adopt", p, "b", "--user", u]), "b is the level's own now\n");
    checks_clean(p);
    assert_eq!(library(), "a: same (yours strip-lamp)\n");
    let e = refused(&["level", "library", "adopt", p, "nope"], &dir);
    assert!(e.contains("no such asset"), "{e}");
}

#[test]
fn cli_exports_and_imports_a_level() {
    let t = tempfile::tempdir().unwrap();
    let author = t.path().join("author-assets");
    // The author's copy of the torch, changed: a user asset.
    let torch = author.join("torch");
    copy_dir(&repo().join("content/assets/torch"), &torch);
    ok(&["asset", "state", "add", s(&torch), "Out"]);
    let own = build_lamp(t.path());
    let dir = new_level(t.path(), "lvl");
    let p = s(&dir);
    ok(&["level", "copy", "add", p, "content/assets/strip-light", "--at", "10,10"]);
    ok(&["level", "copy", "add", p, s(&torch), "--at", "40,10", "--user", s(&author)]);
    ok(&["level", "copy", "add", p, s(&own), "--at", "80,10"]);
    assert_eq!(
        ok(&["level", "copy", "list", p])
            .lines()
            .map(|l| l.split(" at ").next().unwrap())
            .collect::<Vec<_>>(),
        [
            "ground: \"Ground\" ground (own)",
            "strip-light: \"Strip light\" a (built-in strip-light)",
            "torch: \"Torch\" b (yours torch)",
            "strip-lamp: \"Strip lamp\" c (own)",
        ]
    );
    let file = t.path().join("lvl.olrlevel");
    let out = ok(&["level", "export", p, "--out", s(&file)]);
    let size = std::fs::metadata(&file).unwrap().len();
    assert_eq!(out, format!("exported {p} to {}: {size} bytes, 4 assets\n", s(&file)));
    // Never inside the pack.
    refused(&["level", "export", p, "--out", s(&dir.join("x.olrlevel"))], &dir);

    // Into an empty data folder.
    let data = t.path().join("data");
    let (packs, user) = (data.join("packs"), data.join("assets"));
    let out = ok(&["level", "import", s(&file), "--packs", s(&packs), "--user", s(&user)]);
    let to = packs.join("lvl");
    assert_eq!(
        out,
        format!(
            "imported lvl (\"Lvl\") into {}\nown ground\nreused built-in strip-light\nadded \"Torch\" as yours torch\nown c\n",
            s(&to)
        )
    );
    checks_clean(s(&to));
    assert_eq!(tree(&user.join("torch")).remove("asset.ron"), tree(&torch).remove("asset.ron"));
    assert_eq!(
        ok(&["level", "library", s(&to), "--user", s(&user)]),
        "a: same (built-in strip-light)\nb: same (yours torch)\n"
    );
    // A second import is another level.
    let out = ok(&["level", "import", s(&file), "--packs", s(&packs), "--user", s(&user)]);
    assert!(out.starts_with("imported lvl-2 (\"Lvl 2\")") && out.contains("reused yours torch"), "{out}");

    // It plays: a short bot match on a content folder holding it.
    let content = t.path().join("content");
    for id in MATCH_PACKS {
        copy_dir(&repo().join("content/packs").join(id), &content.join(id));
    }
    copy_dir(&to, &content.join("lvl"));
    let out = ok(&[
        "match",
        "--content",
        s(&content),
        "--mod",
        "olx-classic",
        "--level",
        "lvl",
        "--bots",
        "2",
        "--seed",
        "1",
        "--ticks",
        "200",
        "--no-verify",
        "--failures",
        s(&t.path().join("failures")),
    ]);
    assert!(!out.is_empty());
}
