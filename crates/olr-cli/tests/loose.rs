//! `olr-cli level loose add|set|remove|list|probe` and `level element set
//! --loose` (the loose dirt design), on copies of the fixture levels
//! `fixtures/live/levels/loose-test` (plain, its `loose.png` made by hand)
//! and `fixtures/edit/composed-level` (layers): each change leaves the
//! pack checking clean, and a refusal leaves it unchanged, never a panic.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::common::copy_dir;

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
    text(&o)
}

/// `olr-cli args…`, which must fail without a panic and leave `pack` as
/// it was: its output.
fn refused(pack: &Path, args: &[&str]) -> String {
    let before = files(pack);
    let o = cli(args);
    let out = text(&o);
    assert!(!o.status.success(), "{args:?} succeeded: {out}");
    assert!(!out.contains("panicked"), "{args:?}: {out}");
    assert!(files(pack) == before, "{args:?} changed the pack");
    out
}

fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out: Vec<_> = walk(dir).into_iter().map(|p| (p.clone(), std::fs::read(&p).unwrap())).collect();
    out.sort();
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
    }
    out
}

fn copy(fixture: &str) -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("level");
    copy_dir(&repo().join(fixture), &dir);
    (t, dir)
}

fn checks(pack: &str) {
    let out = ok(&["pack", "check", pack]);
    assert!(out.contains("0 errors, 0 warnings"), "{out}");
}

#[test]
fn loose_add_set_remove_round_trip() {
    let (_t, dir) = copy("fixtures/live/levels/loose-test");
    let pack = dir.to_str().unwrap();
    let before = ok(&["level", "loose", "list", pack]);
    assert_eq!(
        before,
        "1: sand \"Sand\" repose 30° gravity 300 top-speed 80 slide-speed 25\n\
         2: gravel \"Gravel\" repose 45° gravity 300 top-speed 80 slide-speed 25\n"
    );
    let out = ok(&["level", "loose", "add", pack, "--id", "mud", "--name", "Mud", "--repose", "70"]);
    assert!(out.contains("added loose material mud (3)"), "{out}");
    ok(&["level", "loose", "set", pack, "mud", "--name", "Wet mud", "--gravity", "400"]);
    let list = ok(&["level", "loose", "list", pack]);
    assert!(
        list.ends_with("3: mud \"Wet mud\" repose 70° gravity 400 top-speed 80 slide-speed 25\n"),
        "{list}"
    );
    checks(pack);
    // Out of range, a second mud, an unknown id: refused.
    refused(&dir, &["level", "loose", "set", pack, "mud", "--repose", "90"]);
    refused(&dir, &["level", "loose", "add", pack, "--id", "mud", "--name", "Mud"]);
    refused(&dir, &["level", "loose", "set", pack, "clay", "--repose", "40"]);
    refused(&dir, &["level", "loose", "add", pack, "--id", "Bad id", "--name", "Bad"]);
    ok(&["level", "loose", "remove", pack, "mud"]);
    assert_eq!(ok(&["level", "loose", "list", pack]), before);
    checks(pack);
}

#[test]
fn remove_refuses_a_named_one() {
    // A plain level: loose.png holds sand.
    let (_t, dir) = copy("fixtures/live/levels/loose-test");
    let out = refused(&dir, &["level", "loose", "remove", dir.to_str().unwrap(), "sand"]);
    assert!(out.contains("loose.png"), "{out}");
    // A level of layers: an element names it.
    let (_t, dir) = copy("fixtures/edit/composed-level");
    let pack = dir.to_str().unwrap();
    ok(&["level", "loose", "add", pack, "--id", "sand", "--name", "Sand"]);
    ok(&["level", "element", "set", pack, "gradient", "--loose", "sand"]);
    let out = refused(&dir, &["level", "loose", "remove", pack, "sand"]);
    assert!(out.contains("used by gradient"), "{out}");
    ok(&["level", "element", "set", pack, "gradient", "--loose", "none"]);
    ok(&["level", "loose", "remove", pack, "sand"]);
    assert!(!dir.join("loose.png").exists());
    checks(pack);
}

#[test]
fn element_set_loose() {
    let (_t, dir) = copy("fixtures/edit/composed-level");
    let pack = dir.to_str().unwrap();
    // No such loose material yet.
    let out = refused(&dir, &["level", "element", "set", pack, "gradient", "--loose", "sand"]);
    assert!(out.contains("sand"), "{out}");
    ok(&["level", "loose", "add", pack, "--id", "sand", "--name", "Sand"]);
    // A rock picture cannot be loose.
    let out = refused(&dir, &["level", "element", "set", pack, "tiny", "--loose", "sand"]);
    assert!(out.contains("loose"), "{out}");
    ok(&["level", "element", "set", pack, "gradient", "--loose", "sand"]);
    let list = ok(&["level", "element", "list", pack]);
    assert!(list.lines().any(|l| l.starts_with("2: gradient") && l.ends_with(" loose sand")), "{list}");
    assert!(dir.join("loose.png").is_file(), "the bake wrote loose.png");
    checks(pack);
    // The level plays its loose dirt.
    let kinds = std::fs::read(dir.join("loose.png")).unwrap();
    assert!(kinds.len() > 8);
}

#[test]
fn probe_reports_a_hanging_region_as_moving() {
    let pack = repo().join("fixtures/live/levels/loose-test");
    let pack = pack.to_str().unwrap();
    // The heap on the dirt bridge over the gap: all of it loose, it falls.
    let out = ok(&["level", "loose", "probe", pack, "--box", "85,45,30,19", "--ticks", "300"]);
    assert_eq!(out, "85,45,30,19 480 100.0%\n");
    // The floor rests on rock: nothing moves.
    let out = ok(&["level", "loose", "probe", pack, "--box", "3,96,194,21", "--ticks", "300"]);
    assert_eq!(out, "3,96,194,21 4074 0.0%\n");
}
