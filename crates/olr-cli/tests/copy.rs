//! `olr-cli level copy …` and `level script` (issue #75): copies of assets
//! placed, moved, resized, set and removed, each step checking clean; the
//! origin detected; refusals leave the pack; `asset place` is gone.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::asset::{build_lamp, tree};
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

/// `olr-cli <args>` succeeding: its stdout.
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

/// `pack check` with the stale-bake check: clean.
fn checks_clean(pack: &str) {
    let out = ok(&["pack", "check", "--content", "content/packs", pack]);
    assert!(out.contains("0 errors, 0 warnings"), "{out}");
}

/// A copy of Castle Strike Reloaded in `t`.
fn castle_strike(t: &Path) -> PathBuf {
    let pack = t.join("castle");
    copy_dir(&repo().join("content/reloaded/olr-level-castlestrike-reloaded"), &pack);
    pack
}

const GROUND: &str = "ground: \"Ground\" ground (own) at 0,0 1350 × 550 locked\n";

#[test]
fn cli_places_moves_resizes_and_removes_copies() {
    let t = tempfile::tempdir().unwrap();
    let dir = castle_strike(t.path());
    let p = dir.to_str().unwrap();
    let original = tree(&dir);
    // Twice: one asset, key a.
    let out = ok(&["level", "copy", "add", p, "content/assets/torch", "--at", "100,50"]);
    assert_eq!(out, "added copy torch of \"Torch\" (asset a, new)\n");
    checks_clean(p);
    let out = ok(&["level", "copy", "add", p, "content/assets/torch", "--at", "140,50"]);
    assert_eq!(out, "added copy torch-2 of \"Torch\" (asset a)\n");
    checks_clean(p);
    let list = |want: &str| assert_eq!(ok(&["level", "copy", "list", p]), want);
    let torch2 = "torch-2: \"Torch\" a (built-in torch) at 140,50 14 × 16\n";
    list(&format!("{GROUND}torch: \"Torch\" a (built-in torch) at 100,50 14 × 16\n{torch2}"));
    // A key names a new asset: refused for a reused one.
    let e = refused(&["level", "copy", "add", p, "content/assets/torch", "--at", "0,0", "--key", "t"], &dir);
    assert!(e.contains("in the level already"), "{e}");
    // Moved, resized keeping the aspect, then freely.
    assert_eq!(ok(&["level", "copy", "move", p, "torch", "--at", "10,-5"]), "moved copy torch to 10,-5\n");
    checks_clean(p);
    assert_eq!(ok(&["level", "copy", "resize", p, "torch", "--width", "7"]), "resized copy torch to 7 × 8\n");
    checks_clean(p);
    list(&format!("{GROUND}torch: \"Torch\" a (built-in torch) at 10,-5 7 × 8\n{torch2}"));
    assert_eq!(
        ok(&["level", "copy", "resize", p, "torch", "--size", "20x5"]),
        "resized copy torch to 20 × 5\n"
    );
    checks_clean(p);
    let torch = "torch: \"Torch\" a (built-in torch) at 10,-5 20 × 5 free\n";
    list(&format!("{GROUND}{torch}{torch2}"));
    // Raised over torch-2, then back.
    assert_eq!(ok(&["level", "copy", "raise", p, "torch"]), "raised copy torch\n");
    checks_clean(p);
    list(&format!("{GROUND}{torch2}{torch}"));
    refused(&["level", "copy", "raise", p, "torch"], &dir);
    assert_eq!(ok(&["level", "copy", "lower", p, "torch"]), "lowered copy torch\n");
    checks_clean(p);
    list(&format!("{GROUND}{torch}{torch2}"));
    // Removed: the asset stays with torch-2, then goes with it.
    assert_eq!(ok(&["level", "copy", "remove", p, "torch"]), "removed copy torch\n");
    checks_clean(p);
    list(&format!("{GROUND}{torch2}"));
    assert_eq!(ok(&["level", "copy", "remove", p, "torch-2"]), "removed copy torch-2\nremoved asset a\n");
    checks_clean(p);
    list(GROUND);
    assert!(tree(&dir) == original, "the level is as it was");
    // The ground is locked; an unknown copy is refused.
    assert!(refused(&["level", "copy", "remove", p, "ground"], &dir).contains("locked"));
    assert!(refused(&["level", "copy", "move", p, "nope", "--at", "0,0"], &dir).contains("no such copy"));
}

#[test]
fn cli_sets_live_state_values_and_scripts() {
    let t = tempfile::tempdir().unwrap();
    let lamp = build_lamp(t.path());
    let dir = castle_strike(t.path());
    let p = dir.to_str().unwrap();
    let out = ok(&["level", "copy", "add", p, lamp.to_str().unwrap(), "--at", "30,40"]);
    assert!(out.starts_with("added copy strip-lamp of \"Strip lamp\" (asset a, new)\n"), "{out}");
    let set = |args: &[&str]| {
        let mut all = vec!["level", "copy", "set", p, "strip-lamp"];
        all.extend_from_slice(args);
        ok(&all)
    };
    let out = set(&["--live", "--state", "Off", "--value", "lamps_glow=40"]);
    assert_eq!(out, "copy strip-lamp: live\ncopy strip-lamp: state Off\ncopy strip-lamp: lamps_glow = 40\n");
    checks_clean(p);
    let line = |rest: &str| format!("{GROUND}strip-lamp: \"Strip lamp\" a (own) at 30,40 20 × 7{rest}\n");
    assert_eq!(ok(&["level", "copy", "list", p]), line(" live state Off lamps_glow=40"));
    set(&["--unset", "lamps_glow", "--start-state", "--default-script", "off"]);
    checks_clean(p);
    assert_eq!(ok(&["level", "copy", "list", p]), line(" live no-default-script"));
    // Scripts: the copy's and the level's.
    let script = t.path().join("copy.rhai");
    std::fs::write(&script, "// on\nfn tick() {}\n").unwrap();
    let s = script.to_str().unwrap();
    assert_eq!(
        ok(&["level", "copy", "script", p, "strip-lamp", "--set", s]),
        "copy strip-lamp: set the script: 2 lines\n"
    );
    assert_eq!(ok(&["level", "script", p, "--set", s]), "set the level script: 2 lines\n");
    checks_clean(p);
    assert!(dir.join("editor/scripts/strip-lamp.rhai").is_file() && dir.join("editor/level.rhai").is_file());
    assert_eq!(
        ok(&["level", "copy", "list", p]),
        format!("{}level script: 2 lines\n", line(" live script no-default-script"))
    );
    // Renamed, its script follows.
    assert_eq!(set(&["--id", "porch"]), "renamed copy strip-lamp to porch\n");
    assert!(
        dir.join("editor/scripts/porch.rhai").is_file()
            && !dir.join("editor/scripts/strip-lamp.rhai").exists()
    );
    // Wrong values are refused with the reason.
    let porch = |args: &[&str]| {
        let mut all = vec!["level", "copy", "set", p, "porch"];
        all.extend_from_slice(args);
        refused(&all, &dir)
    };
    for (args, why) in [
        (&["--value", "lamps_glow=true"][..], "lamps_glow"),
        (&["--value", "lamps_glow=500"], "500"),
        (&["--value", "nope=1"], "exposes no value \"nope\""),
        (&["--value", "lamps_glow"], "NAME=VALUE"),
        (&["--state", "Nope"], "no state \"Nope\""),
        (&["--distant", "50"], "porch"),
    ] {
        let e = porch(args);
        assert!(e.contains(why), "{args:?}: {e}");
    }
    // The ground has nothing to drive.
    let e = refused(&["level", "copy", "set", p, "ground", "--unlocked", "--live"], &dir);
    let why = olr_edit::asset::LiveRefusal::NothingToDrive.to_string();
    assert!(e.contains(&why), "{e}");
    // Cleared again.
    assert_eq!(ok(&["level", "copy", "script", p, "porch", "--clear"]), "copy porch: cleared the script\n");
    assert_eq!(ok(&["level", "script", p, "--clear"]), "cleared the level script\n");
    assert!(!dir.join("editor/level.rhai").exists() && !dir.join("editor/scripts").exists());
    checks_clean(p);
}

#[test]
fn cli_add_detects_the_origin() {
    let t = tempfile::tempdir().unwrap();
    let dir = castle_strike(t.path());
    let p = dir.to_str().unwrap();
    let user = t.path().join("library");
    copy_dir(&repo().join("content/assets/torch"), &user.join("torch"));
    let elsewhere = t.path().join("elsewhere/torch");
    copy_dir(&repo().join("content/assets/torch"), &elsewhere);
    let u = user.to_str().unwrap();
    ok(&["level", "copy", "add", p, "content/assets/torch", "--at", "0,0", "--user", u]);
    let out =
        ok(&["level", "copy", "add", p, user.join("torch").to_str().unwrap(), "--at", "0,0", "--user", u]);
    assert_eq!(out, "added copy torch-2 of \"Torch\" (asset b, new)\n");
    let out = ok(&[
        "level",
        "copy",
        "add",
        p,
        elsewhere.to_str().unwrap(),
        "--at",
        "0,0",
        "--user",
        u,
        "--key",
        "mine",
        "--id",
        "flame",
        "--height",
        "8",
    ]);
    assert_eq!(out, "added copy flame of \"Torch\" (asset mine, new)\n");
    checks_clean(p);
    assert_eq!(
        ok(&["level", "copy", "list", p]),
        format!(
            "{GROUND}torch: \"Torch\" a (built-in torch) at 0,0 14 × 16\n\
             torch-2: \"Torch\" b (yours torch) at 0,0 14 × 16\n\
             flame: \"Torch\" mine (own) at 0,0 7 × 8\n"
        )
    );
    assert!(dir.join("editor/mine/asset.ron").is_file());
    // A chosen key that is taken or reserved.
    let other = t.path().join("other/torch");
    copy_dir(&repo().join("content/assets/torch"), &other);
    std::fs::write(
        other.join("asset.ron"),
        std::fs::read_to_string(other.join("asset.ron")).unwrap().replace("\"Torch\"", "\"Lamp\""),
    )
    .unwrap();
    let o = other.to_str().unwrap();
    for key in ["mine", "layers", "ground", "Big"] {
        let e = refused(&["level", "copy", "add", p, o, "--at", "0,0", "--key", key], &dir);
        assert!(e.contains(key), "{key}: {e}");
    }
}

#[test]
fn cli_refuses_an_unconverted_level() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("old");
    let p = dir.to_str().unwrap();
    ok(&["pack", "new", "level", "old-level", "--out", p, "--size", "40x20"]);
    ok(&["level", "element", "add-layer", p, "--material", "rock", "--name", "Wall"]);
    crate::element::make_02(&dir);
    let script = t.path().join("s.rhai");
    std::fs::write(&script, "// s\n").unwrap();
    let s = script.to_str().unwrap();
    for args in [
        &["level", "copy", "add", p, "content/assets/torch", "--at", "0,0"][..],
        &["level", "copy", "list", p],
        &["level", "copy", "move", p, "ground", "--at", "1,1"],
        &["level", "copy", "remove", p, "ground"],
        &["level", "script", p, "--set", s],
    ] {
        let e = refused(args, &dir);
        assert!(e.contains("a level from OLR 0.2: run olr-cli level convert"), "{args:?}: {e}");
    }
}

/// A plain level gets its ground first, as `level element` does.
#[test]
fn cli_gives_a_plain_level_its_ground_first() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("plain");
    let p = dir.to_str().unwrap();
    ok(&["pack", "new", "level", "plain-level", "--out", p, "--size", "120x80"]);
    let out = ok(&["level", "copy", "add", p, "content/assets/torch", "--at", "10,20"]);
    assert!(out.contains("added copy torch of \"Torch\" (asset a, new)\nadded glow group flames\n"), "{out}");
    assert!(out.contains(olr_edit::asset::ADDED_LIGHTING), "{out}");
    checks_clean(p);
    assert_eq!(
        ok(&["level", "copy", "list", p]),
        "ground: \"Ground\" ground (own) at 0,0 120 × 80 locked\n\
         torch: \"Torch\" a (built-in torch) at 10,20 14 × 16\n"
    );
}

#[test]
fn asset_place_is_gone() {
    let o =
        cli(&["asset", "place", "content/reloaded/olr-level-castlestrike-reloaded", "content/assets/torch"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("unrecognized subcommand 'place'"), "{}", text(&o));
}

/// The place tests of issue #9, as copies: a saved asset placed back.
#[test]
fn cli_saves_and_places_an_asset() {
    let t = tempfile::tempdir().unwrap();
    let dir = castle_strike(t.path());
    let p = dir.to_str().unwrap();
    let out = t.path().join("saved");
    let o = out.to_str().unwrap();
    ok(&["asset", "save", p, "--rect", "100,100,30,20", "--name", "Wall bit", "--out", o]);
    let added = ok(&["level", "copy", "add", p, o, "--at", "300,40"]);
    assert!(added.starts_with("added copy wall-bit of \"Wall bit\" (asset a, new)\n"), "{added}");
    checks_clean(p);
    assert!(
        ok(&["level", "copy", "list", p]).ends_with("wall-bit: \"Wall bit\" a (own) at 300,40 30 × 20\n")
    );
    // The copy's asset is the saved folder, byte for byte but its
    // thumbnail.
    let mut saved = tree(&out);
    let mut copied = tree(&dir.join("editor/a"));
    saved.remove("thumb.png");
    copied.remove("thumb.png");
    assert!(
        saved == copied,
        "{:?} vs {:?}",
        saved.keys().collect::<Vec<_>>(),
        copied.keys().collect::<Vec<_>>()
    );
    // A multi-piece asset is one copy.
    let out = ok(&["level", "copy", "add", p, "content/assets/ceiling-lamp", "--at", "10,20"]);
    assert!(out.starts_with("added copy ceiling-lamp of \"Ceiling lamp\" (asset b, new)\n"), "{out}");
    checks_clean(p);
}
