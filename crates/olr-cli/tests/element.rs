//! `olr-cli level element …`, `level bake` and stale bakes (plan 14a Task 8).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use olr_data::image::{Rgba, RgbaImage};

use crate::common::{content_dir, copy_dir};

/// A copy of Dirt Level and the content directory.
fn dirt_level() -> Option<(tempfile::TempDir, PathBuf, String)> {
    let content = content_dir()?;
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("dirt");
    copy_dir(&Path::new(&content).join("olx-level-dirt-level"), &pack);
    Some((tmp, pack, content))
}

/// `olr-cli <args…>` with `{pack}` and `{content}` replaced.
fn cli(args: &[&str], pack: &Path, content: &str) -> Output {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.replace("{pack}", pack.to_str().unwrap()).replace("{content}", content))
        .collect();
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).args(&args).output().unwrap()
}

fn ok(o: &Output) -> String {
    assert!(
        o.status.success(),
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Every file under `dir`, by relative path.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn go(base: &Path, d: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out)
            } else {
                out.insert(
                    p.strip_prefix(base).unwrap().to_string_lossy().into_owned(),
                    std::fs::read(&p).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

/// A 20 × 10 PNG, left half red, right half blue, beside the pack.
fn sign_png(tmp: &Path) -> PathBuf {
    let mut img = RgbaImage::new(20, 10);
    for y in 0..10 {
        for x in 0..20 {
            img.set(x, y, if x < 10 { Rgba::rgb(255, 0, 0) } else { Rgba::rgb(0, 0, 255) });
        }
    }
    let path = tmp.join("sign.png");
    std::fs::write(&path, olr_format::image_io::encode_rgba(&img)).unwrap();
    path
}

/// Composes the pack: the sign image and a cave layer with a circle drawn.
fn compose(tmp: &Path, pack: &Path, content: &str) {
    let png = sign_png(tmp);
    let out = ok(&cli(
        &[
            "level",
            "element",
            "add-image",
            "{pack}",
            png.to_str().unwrap(),
            "--at",
            "-5,10",
            "--size",
            "40x20",
            "--pixel-size",
            "2",
            "--colours",
            "2",
            "--material",
            "rock",
            "--name",
            "Sign",
            "--content",
            "{content}",
        ],
        pack,
        content,
    ));
    assert_eq!(out, "added element sign\n");
    ok(&cli(
        &[
            "level",
            "element",
            "add-layer",
            "{pack}",
            "--material",
            "background",
            "--name",
            "Cave",
            "--content",
            "{content}",
        ],
        pack,
        content,
    ));
    ok(&cli(
        &[
            "level",
            "element",
            "draw",
            "{pack}",
            "cave",
            "--circle",
            "100,100,10",
            "--colour",
            "202020",
            "--content",
            "{content}",
        ],
        pack,
        content,
    ));
}

#[test]
fn compose_a_level_from_an_image_and_a_layer() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    let original = std::fs::read(pack.join("material.png")).unwrap();
    compose(tmp.path(), &pack, &content);
    let list = ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content));
    assert_eq!(
        list,
        "0: base Raster - \"Base\"\n1: sign Image Rock \"Sign\"\n2: cave Drawn Background \"Cave\"\n"
    );
    let check = ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
    assert!(check.contains("0 errors"), "{check}");
    assert_ne!(std::fs::read(pack.join("material.png")).unwrap(), original);
    let preview = tmp.path().join("m.png");
    ok(&cli(
        &["level", "preview", "{pack}", "--out", preview.to_str().unwrap(), "--material"],
        &pack,
        &content,
    ));
    let img = olr_format::image_io::decode_rgba(&std::fs::read(&preview).unwrap()).unwrap();
    assert_eq!(img.get(100, 105), Rgba::rgb(0, 0, 0), "the drawn cave is background");
}

#[test]
fn pack_check_reports_a_stale_bake() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    let front = olr_format::image_io::decode_rgba(&std::fs::read(pack.join("front.png")).unwrap()).unwrap();
    std::fs::write(
        pack.join("front.png"),
        olr_format::image_io::encode_rgba(&RgbaImage::new(front.width(), front.height())),
    )
    .unwrap();
    let out = cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("front.png: differs from the composition's bake"));
    ok(&cli(&["level", "bake", "{pack}", "--content", "{content}"], &pack, &content));
    ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
}

#[test]
fn raster_operations_on_a_composed_level_are_refused() {
    let Some((_tmp, pack, content)) = dirt_level() else { return };
    ok(&cli(&["level", "element", "add-layer", "{pack}", "--content", "{content}"], &pack, &content));
    let before = tree(&pack);
    let out = cli(&["level", "carve", "{pack}", "--circle", "1,1,1"], &pack, &content);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("a composed level: edit its elements"), "{out:?}");
    assert_eq!(tree(&pack), before);
}

#[test]
fn add_image_refuses_a_hostile_file_and_writes_nothing() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    let junk = tmp.path().join("junk.png");
    std::fs::write(&junk, b"not an image").unwrap();
    let before = tree(&pack);
    let out = cli(
        &[
            "level",
            "element",
            "add-image",
            "{pack}",
            junk.to_str().unwrap(),
            "--at",
            "0,0",
            "--size",
            "4x4",
            "--content",
            "{content}",
        ],
        &pack,
        &content,
    );
    assert!(!out.status.success());
    assert_eq!(tree(&pack), before);
}

#[test]
fn set_hide_raise_remove() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    ok(&cli(
        &["level", "element", "set", "{pack}", "sign", "--visible", "false", "--content", "{content}"],
        &pack,
        &content,
    ));
    assert!(
        ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content))
            .contains("1: sign Image Rock \"Sign\" hidden")
    );
    ok(&cli(&["level", "element", "raise", "{pack}", "sign", "--content", "{content}"], &pack, &content));
    assert!(ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content)).contains("2: sign"));
    ok(&cli(&["level", "element", "remove", "{pack}", "sign", "--content", "{content}"], &pack, &content));
    assert!(!ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content)).contains("sign"));
    assert_eq!(std::fs::read_dir(pack.join("editor/images")).map(|d| d.count()).unwrap_or(0), 0);
}

#[test]
fn resize_a_composed_level() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    ok(&cli(
        &["level", "resize", "{pack}", "--size", "520x350", "--offset", "16,0", "--content", "{content}"],
        &pack,
        &content,
    ));
    let text = std::fs::read_to_string(pack.join("editor/composition.ron")).unwrap();
    assert!(text.contains("at: (11, 10)"), "{text}");
    ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
}

/// Batch C review: `set --colours N|none` (an optional value of an optional
/// flag) parses and applies.
#[test]
fn set_colours_and_none() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    ok(&cli(
        &["level", "element", "set", "{pack}", "sign", "--colours", "4", "--content", "{content}"],
        &pack,
        &content,
    ));
    let text = std::fs::read_to_string(pack.join("editor/composition.ron")).unwrap();
    assert!(text.contains("colours: 4"), "{text}");
    ok(&cli(
        &["level", "element", "set", "{pack}", "sign", "--colours", "none", "--content", "{content}"],
        &pack,
        &content,
    ));
    let text = std::fs::read_to_string(pack.join("editor/composition.ron")).unwrap();
    assert!(!text.contains("colours"), "{text}");
    let out = cli(
        &["level", "element", "set", "{pack}", "sign", "--colours", "x", "--content", "{content}"],
        &pack,
        &content,
    );
    assert_eq!(out.status.code(), Some(2), "a clap error: {out:?}");
}

/// Deferred minors: `list` reads `composition.ron` alone, so it still lists
/// a composition whose file is missing (when an agent most needs it).
#[test]
fn list_works_with_a_missing_file() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    std::fs::remove_file(pack.join("editor/layers/cave.png")).unwrap();
    let list = ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content));
    assert!(list.contains("2: cave Drawn Background"), "{list}");
}

/// Deferred minors: a JPEG cut in its scan is refused through the CLI too.
#[test]
fn add_image_refuses_a_cut_jpeg() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    let jpeg =
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../olr-format/tests/data/gradient.jpg")).unwrap();
    let cut = tmp.path().join("cut.jpg");
    std::fs::write(&cut, &jpeg[..jpeg.len() / 2]).unwrap();
    let before = tree(&pack);
    let out = cli(
        &[
            "level",
            "element",
            "add-image",
            "{pack}",
            cut.to_str().unwrap(),
            "--at",
            "0,0",
            "--size",
            "4x4",
            "--content",
            "{content}",
        ],
        &pack,
        &content,
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cut.jpg"), "{out:?}");
    assert_eq!(tree(&pack), before);
}

/// Plan 14d Task 6: `split`, `merge-down` and `mask` on a composed level.
#[test]
fn merge_split_and_mask_commands() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    let run = |args: &[&str]| {
        let mut all = vec!["level", "element"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--content", "{content}"]);
        ok(&cli(&all, &pack, &content))
    };
    let out = run(&["split", "{pack}", "base"]);
    assert_eq!(out, "added element background\nadded element dirt\nadded element rock\n");
    run(&["merge-down", "{pack}", "dirt"]);
    run(&["mask", "{pack}", "sign", "--colour-at", "0,15", "--tolerance", "5"]);
    run(&["mask", "{pack}", "sign", "--rect", "10,10,4,4", "--restore"]);
    run(&["draw", "{pack}", "background", "--rect", "20,20,4,4", "--colour", "808080", "--material", "rock"]);
    let composition = std::fs::read_to_string(pack.join("editor/composition.ron")).unwrap();
    assert!(composition.contains("mask: true"), "{composition}");
    assert!(composition.contains("mixed: true") && composition.contains("back: true"), "{composition}");
    assert!(pack.join("editor/masks/sign.png").is_file());
    let e = cli(
        &["level", "element", "mask", "{pack}", "sign", "--colour-at", "0,15", "--restore"],
        &pack,
        &content,
    );
    assert!(!e.status.success() && String::from_utf8_lossy(&e.stderr).contains("--restore takes a shape"));
    // --tolerance is --colour-at's alone (plan 14d minors).
    let e = cli(
        &["level", "element", "mask", "{pack}", "sign", "--rect", "0,0,1,1", "--tolerance", "5"],
        &pack,
        &content,
    );
    assert!(!e.status.success() && String::from_utf8_lossy(&e.stderr).contains("colour-at"));
    // The bake is current.
    ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
}

#[test]
fn fill_and_duplicate_commands() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    let run = |args: &[&str]| {
        let mut all = vec!["level", "element"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--content", "{content}"]);
        ok(&cli(&all, &pack, &content))
    };
    assert_eq!(run(&["duplicate", "{pack}", "sign"]), "added element sign-2\n");
    run(&["split", "{pack}", "base"]);
    let texture = sign_png(tmp.path());
    run(&["fill", "{pack}", "background", "--at", "5,5", "--texture", texture.to_str().unwrap()]);
    run(&[
        "fill",
        "{pack}",
        "cave",
        "--at",
        "100,100",
        "--tolerance",
        "5",
        "--colour",
        "404040",
        "--material",
        "rock",
    ]);
    let list = ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content));
    assert!(
        list.contains("sign Image Rock \"Sign\"\n") && list.contains("sign-2 Image Rock \"Sign 2\""),
        "{list}"
    );
    let e = cli(
        &["level", "element", "fill", "{pack}", "cave", "--at", "-1,0", "--colour", "404040"],
        &pack,
        &content,
    );
    assert!(!e.status.success() && String::from_utf8_lossy(&e.stderr).contains("outside"));
    ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
}

/// Adds `look` (one glow group, `lava`) to the pack's level.ron.
fn add_look(pack: &Path) {
    let path = pack.join("level.ron");
    let text = std::fs::read_to_string(&path).unwrap();
    let text = text.trim_end().strip_suffix(')').unwrap().to_string()
        + "    look: (glow_groups: [(id: \"lava\", reach: Pixels(64))]),\n)\n";
    std::fs::write(path, text).unwrap();
}

#[test]
fn cli_sets_and_paints_glow() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    add_look(&pack);
    let run = |args: &[&str]| {
        let mut all = vec!["level", "element"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--content", "{content}"]);
        cli(&all, &pack, &content)
    };
    ok(&run(&["split", "{pack}", "base"]));
    let e = run(&["set", "{pack}", "dirt", "--glow", "60"]);
    assert!(!e.status.success() && String::from_utf8_lossy(&e.stderr).contains("--glow-group"));
    let e = run(&["set", "{pack}", "dirt", "--glow", "60", "--glow-group", "nowhere"]);
    assert!(!e.status.success() && String::from_utf8_lossy(&e.stderr).contains("nowhere"));
    ok(&run(&["set", "{pack}", "dirt", "--glow", "60", "--glow-group", "lava"]));
    ok(&run(&["glow", "{pack}", "dirt", "--rect", "0,0,10,10", "--strength", "0"]));
    let composition = std::fs::read_to_string(pack.join("editor/composition.ron")).unwrap();
    assert!(composition.contains("strength: 60") && composition.contains("mask: true"), "{composition}");
    assert!(pack.join("editor/glow/dirt.png").is_file());
    for f in ["look/glow.png", "look/glow-back.png", "look/groups.png", "look/groups-back.png"] {
        assert!(pack.join(f).is_file(), "{f}");
    }
    ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
    // --glow 0 removes the glow, its mask and the level's look images.
    ok(&run(&["set", "{pack}", "dirt", "--glow", "0"]));
    assert!(!pack.join("editor/glow/dirt.png").exists());
    assert!(!pack.join("look").join("glow.png").exists());
}

/// The user's request (2026-10-19): `glow --colour-at` lights the
/// connected pixels alike in colour, the rest of the layer dark.
#[test]
fn cli_glows_pixels_by_colour() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    add_look(&pack);
    let run = |args: &[&str]| {
        let mut all = vec!["level", "element"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--content", "{content}"]);
        cli(&all, &pack, &content)
    };
    ok(&run(&["split", "{pack}", "base"]));
    ok(&run(&["draw", "{pack}", "dirt", "--rect", "20,20,6,6", "--colour", "ff2020"]));
    ok(&run(&["set", "{pack}", "dirt", "--glow", "100", "--glow-group", "lava"]));
    let e = run(&["glow", "{pack}", "dirt", "--rect", "0,0,4,4", "--tolerance", "5", "--strength", "100"]);
    assert!(!e.status.success() && String::from_utf8_lossy(&e.stderr).contains("--colour-at"));
    ok(&run(&["glow", "{pack}", "dirt", "--colour-at", "22,22", "--tolerance", "5", "--strength", "80"]));
    let bytes = std::fs::read(pack.join("editor/glow/dirt.png")).unwrap();
    let (w, _, mask) =
        olr_format::image_io::decode_grey_within(&bytes, olr_format::image_io::LEVEL_ART).unwrap();
    let v = |x: u32, y: u32| mask[(y * w + x) as usize];
    assert_eq!((v(20, 20), v(25, 25)), (204, 204), "80 % on the red");
    assert_eq!((v(26, 22), v(100, 100)), (0, 0), "dark elsewhere");
    ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
}

#[test]
fn cli_sets_clear() {
    let Some((tmp, pack, content)) = dirt_level() else { return };
    compose(tmp.path(), &pack, &content);
    let run = |args: &[&str]| {
        let mut all = vec!["level", "element"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--content", "{content}"]);
        cli(&all, &pack, &content)
    };
    ok(&run(&["split", "{pack}", "base"]));
    ok(&run(&["set", "{pack}", "rock", "--clear", "true"]));
    let composition = std::fs::read_to_string(pack.join("editor/composition.ron")).unwrap();
    assert!(composition.contains("clear: true"), "{composition}");
    assert!(pack.join("look/clear.png").is_file());
    ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
    ok(&run(&["set", "{pack}", "rock", "--clear", "false"]));
    assert!(!pack.join("look/clear.png").exists());
}
