//! `olr-cli material list|check`, `level material list|add`, `level
//! element set --surface|--back-surface`, `level element surface` and
//! assets carrying materials (issue #38), on a copy of the fixture level
//! `fixtures/edit/composed-level` (a raster, three pictures and a mixed
//! layer with back art): each operation leaves the pack checking clean,
//! the same rules paint the same files, and hostile input is an error,
//! never a panic, with the pack unchanged.

use std::collections::BTreeMap;
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

/// `olr-cli args…`, which must succeed: its output.
fn ok(args: &[&str]) -> String {
    let o = cli(args);
    assert!(o.status.success(), "{args:?}: {}", text(&o));
    text(&o)
}

/// `olr-cli args…`, which must fail without a panic: its output.
fn refused(args: &[&str]) -> String {
    let o = cli(args);
    let out = text(&o);
    assert!(!o.status.success(), "{args:?} succeeded: {out}");
    assert!(!out.contains("panicked"), "{args:?}: {out}");
    out
}

/// Every file under `dir`, by relative path.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn go(base: &Path, d: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out)
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

/// A copy of the fixture level in a temporary directory.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("level");
    copy_dir(&repo().join("fixtures/edit/composed-level"), &dir);
    (t, dir)
}

/// The pack checks clean: no error, no warning.
fn checks(pack: &str) {
    let out = ok(&["pack", "check", pack]);
    assert!(out.contains("0 errors, 0 warnings"), "{out}");
}

/// A user library holding `lava` (stone's values) and a file that does
/// not read.
fn user_library(t: &Path) -> PathBuf {
    let dir = t.join("materials");
    std::fs::create_dir_all(&dir).unwrap();
    let stone = std::fs::read_to_string(repo().join("content/materials/stone.ron")).unwrap();
    let lava = stone.replace("id: \"stone\"", "id: \"lava\"").replace("name: \"Stone\"", "name: \"Lava\"");
    std::fs::write(dir.join("lava.ron"), lava).unwrap();
    std::fs::write(dir.join("broken.ron"), "(id: \"broken\"").unwrap();
    dir
}

#[test]
fn the_library_lists_built_in_then_the_users() {
    let t = tempfile::tempdir().unwrap();
    let user = user_library(t.path());
    let o = cli(&["material", "list", "--user", user.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.iter().any(|l| l.starts_with("stone \"Stone\" built-in heat: sensitivity 100")), "{out}");
    assert!(lines.iter().any(|l| l.starts_with("wood \"Wood\" built-in")), "{out}");
    assert!(lines.last().unwrap().starts_with("lava \"Lava\" user heat:"), "{out}");
    assert_eq!(lines.iter().filter(|l| l.contains(" built-in")).count(), 8, "{out}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("warning: material broken"), "{err}");
}

#[test]
fn check_reads_each_file_and_refuses_hostile_ones() {
    let t = tempfile::tempdir().unwrap();
    let p = |name: &str| t.path().join(name);
    let stone = std::fs::read_to_string(repo().join("content/materials/stone.ron")).unwrap();
    std::fs::write(p("stone.ron"), &stone).unwrap();
    std::fs::write(p("rock.ron"), &stone).unwrap();
    std::fs::write(p("deep.ron"), "(".repeat(100_000)).unwrap();
    std::fs::write(p("huge.ron"), vec![b' '; 2 << 20]).unwrap();
    std::fs::write(p("binary.ron"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    std::fs::write(p("hot.ron"), stone.replace("capacity: 100", "capacity: 70000")).unwrap();
    std::fs::create_dir(p("dir.ron")).unwrap();
    let ok_file = p("stone.ron");
    let out = ok(&["material", "check", ok_file.to_str().unwrap()]);
    assert!(out.contains("stone \"Stone\" heat:"), "{out}");
    for bad in ["rock.ron", "deep.ron", "huge.ron", "binary.ron", "hot.ron", "dir.ron", "missing.ron"] {
        let f = p(bad);
        let out = refused(&["material", "check", ok_file.to_str().unwrap(), f.to_str().unwrap()]);
        assert!(out.contains(&format!("{bad}: error:")), "{bad}: {out}");
    }
}

#[test]
fn a_fixture_level_takes_materials_and_surfaces() {
    let (t, dir) = fixture();
    let level = dir.to_str().unwrap();
    let user = user_library(t.path());
    let lib = ["--user", user.to_str().unwrap()];
    let out = ok(&[&["level", "material", "add", level, "stone", "wood", "metal"][..], &lib].concat());
    assert!(out.contains("added material stone (1)") && out.contains("added material metal (3)"), "{out}");
    checks(level);
    // Again: the level keeps its own copy; a user one and a file added.
    let lava = user.join("lava.ron");
    let out =
        ok(&[&["level", "material", "add", level, "stone", "--file", lava.to_str().unwrap()][..], &lib]
            .concat());
    assert!(out.contains("kept material stone") && out.contains("added material lava (4)"), "{out}");
    let out = ok(&["level", "material", "list", level]);
    assert!(
        out.starts_with("1: stone \"Stone\"\n2: wood \"Wood\"\n3: metal \"Metal\"\n4: lava \"Lava\""),
        "{out}"
    );

    // Elements' surfaces: a picture's, a backed layer's back art's, the
    // raster's.
    ok(&["level", "element", "set", level, "tiny", "--surface", "stone"]);
    ok(&["level", "element", "set", level, "wall", "--surface", "wood", "--back-surface", "metal"]);
    ok(&["level", "element", "set", level, "base", "--back-surface", "stone"]);
    checks(level);
    let out = ok(&["level", "element", "list", level]);
    assert!(out.contains("tiny Image Rock \"Tiny\" surface stone"), "{out}");
    assert!(out.contains("surface wood back-surface metal"), "{out}");
    assert!(dir.join("look/surface.png").is_file() && dir.join("look/surface-back.png").is_file());

    // Painted by rules, twice: the same files.
    let rules = ["--rule", "rock:*=metal", "--rule", "*/20=lava", "--rule", "background:000000/100=none"];
    let out = ok(&[&["level", "element", "surface", level, "wall"][..], &rules].concat());
    assert!(out.contains("pixels matched"), "{out}");
    ok(&["level", "element", "surface", level, "wall", "--back", "--rule", "*=wood"]);
    ok(&["level", "element", "surface", level, "base", "--rule", "dirt:*=lava"]);
    checks(level);
    let once = tree(&dir);
    ok(&[&["level", "element", "surface", level, "wall"][..], &rules].concat());
    assert_eq!(tree(&dir), once, "the same rules paint the same files");
    let out = ok(&["level", "element", "list", level]);
    assert!(out.contains("surface-map back-surface-map"), "{out}");

    // A map from a PNG of values: the picture's box is 60 × 45.
    let png = t.path().join("tiny.png");
    let mut values = vec![2u8; 60 * 45];
    values[0] = 4;
    std::fs::write(&png, olr_format::image_io::encode_grey(60, 45, &values)).unwrap();
    ok(&["level", "element", "surface", level, "tiny", "--map", png.to_str().unwrap()]);
    checks(level);
    let saved = std::fs::read(dir.join("editor/surface/tiny.png")).unwrap();
    assert_eq!(
        olr_format::image_io::decode_grey_within(&saved, olr_format::image_io::IMAGES).unwrap().2,
        values
    );
    ok(&["level", "element", "surface", level, "tiny", "--clear"]);
    assert!(!dir.join("editor/surface/tiny.png").exists());
    ok(&["level", "element", "set", level, "tiny", "--surface", "none"]);
    checks(level);
}

#[test]
fn hostile_surface_input_is_refused_and_changes_nothing() {
    let (t, dir) = fixture();
    let level = dir.to_str().unwrap();
    ok(&["level", "material", "add", level, "stone", "wood"]);
    let before = tree(&dir);
    let png = |name: &str, w: u32, h: u32, v: u8| {
        let p = t.path().join(name);
        std::fs::write(&p, olr_format::image_io::encode_grey(w, h, &vec![v; (w * h) as usize])).unwrap();
        p.to_str().unwrap().to_string()
    };
    let (small, past, colour) =
        (png("small.png", 3, 3, 1), png("past.png", 60, 45, 3), t.path().join("c.png"));
    std::fs::write(&colour, olr_format::image_io::encode_rgba(&olr_data::image::RgbaImage::new(60, 45)))
        .unwrap();
    let not_png = repo().join("content/materials/stone.ron");
    for args in [
        vec!["level", "element", "set", level, "tiny", "--surface", "lava"],
        vec!["level", "element", "set", level, "tiny", "--back-surface", "wood"],
        vec!["level", "element", "set", level, "nothing", "--surface", "wood"],
        vec!["level", "element", "surface", level, "tiny", "--rule", "*=lava"],
        vec!["level", "element", "surface", level, "tiny", "--back", "--rule", "*=wood"],
        vec!["level", "element", "surface", level, "tiny", "--rule", "zz=wood"],
        vec!["level", "element", "surface", level, "tiny", "--rule", "stone:*=wood"],
        vec!["level", "element", "surface", level, "tiny", "--rule", "*/101=wood"],
        vec!["level", "element", "surface", level, "tiny", "--rule", "*="],
        vec!["level", "element", "surface", level, "tiny", "--rule", "*"],
        vec!["level", "element", "surface", level, "tiny"],
        vec!["level", "element", "surface", level, "tiny", "--map", &small],
        vec!["level", "element", "surface", level, "tiny", "--map", &past],
        vec!["level", "element", "surface", level, "tiny", "--map", colour.to_str().unwrap()],
        vec!["level", "element", "surface", level, "tiny", "--map", not_png.to_str().unwrap()],
        vec!["level", "element", "surface", level, "tiny", "--map", "/dev/null"],
        vec!["level", "material", "add", level, "lava"],
        vec!["level", "material", "add", level],
        vec!["level", "material", "add", level, "--file", not_png.parent().unwrap().to_str().unwrap()],
    ] {
        refused(&args);
        assert!(tree(&dir) == before, "{args:?} changed the pack");
    }
}

#[test]
fn assets_carry_materials_through_save_and_place() {
    let (t, dir) = fixture();
    let level = dir.to_str().unwrap();
    ok(&["level", "material", "add", level, "glass", "stone", "wood"]);
    ok(&["level", "element", "set", level, "tiny", "--surface", "stone"]);
    ok(&["level", "element", "surface", level, "wall", "--rule", "*=wood"]);
    let asset = t.path().join("asset");
    let out = ok(&[
        "asset",
        "save",
        level,
        "--elements",
        "tiny,wall",
        "--name",
        "Bits",
        "--out",
        asset.to_str().unwrap(),
    ]);
    assert!(out.contains("materials stone, wood"), "{out}");
    let text = std::fs::read_to_string(asset.join("asset.ron")).unwrap();
    assert!(text.contains("materials: [") && !text.contains("\"glass\""), "{text}");
    assert!(asset.join("surface/wall.png").is_file());
    let out = ok(&["asset", "check", asset.to_str().unwrap()]);
    assert!(out.contains("materials stone, wood"), "{out}");

    // A new level without a look: a look made, both added; placed again,
    // joined.
    let target = t.path().join("empty");
    ok(&["pack", "new", "level", "empty-level", "--out", target.to_str().unwrap(), "--size", "600x400"]);
    let target = target.to_str().unwrap();
    let out = ok(&["asset", "place", target, asset.to_str().unwrap(), "--at", "0,0"]);
    assert!(out.contains("added material stone") && out.contains("added material wood"), "{out}");
    assert!(out.contains("added a look"), "{out}");
    checks(target);
    let out = ok(&["asset", "place", target, asset.to_str().unwrap(), "--at", "10,10"]);
    assert!(out.contains("joined material stone") && out.contains("joined material wood"), "{out}");
    checks(target);
    let out = ok(&["level", "material", "list", target]);
    assert_eq!(out, "1: stone \"Stone\"\n2: wood \"Wood\"\n");
    let out = ok(&["level", "element", "list", target]);
    assert!(out.contains("bits-tiny Image Rock \"Bits Tiny\" surface stone"), "{out}");

    // A hostile asset: a material it names but lacks.
    std::fs::write(asset.join("asset.ron"), text.replace("id: \"stone\"", "id: \"lava\"")).unwrap();
    let out = refused(&["asset", "check", asset.to_str().unwrap()]);
    assert!(out.contains("stone"), "{out}");
    let before = tree(Path::new(target));
    refused(&["asset", "place", target, asset.to_str().unwrap(), "--at", "0,0"]);
    assert!(tree(Path::new(target)) == before);
}
