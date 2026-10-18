//! `olr-cli level …` on a copy of a bundled level (plan 13d Task 6).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::common::{content_dir, copy_dir};

fn olr_cli(args: &[&str], pack: &Path) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_olr-cli"));
    cmd.arg("level").args(&args[..1]).arg(pack).args(&args[1..]);
    cmd.output().unwrap()
}

fn object(args: &[&str], pack: &Path) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_olr-cli"));
    cmd.args(["level", "object", args[0]]).arg(pack).args(&args[1..]);
    cmd.output().unwrap()
}

/// A copy of Dirt Level in a temporary directory.
fn dirt_level() -> Option<(tempfile::TempDir, PathBuf)> {
    let content = content_dir()?;
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("dirt");
    copy_dir(&Path::new(&content).join("olx-level-dirt-level"), &pack);
    Some((tmp, pack))
}

fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn check(pack: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).args(["pack", "check"]).arg(pack).output().unwrap()
}

#[test]
fn carve_then_check() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    let before = tree(&pack);
    let out = olr_cli(&["carve", "--circle", "100,100,20"], &pack);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = check(&pack);
    assert!(out.status.success(), "{}", stdout(&out));
    let after = tree(&pack);
    assert_ne!(after["material.png"], before["material.png"]);
    assert_ne!(after["thumb.png"], before["thumb.png"], "the thumbnail follows the art");
}

#[test]
fn an_invalid_operation_writes_nothing() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    let before = tree(&pack);
    let out = olr_cli(&["resize", "--size", "5000x10"], &pack);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(tree(&pack), before);
    for bad in [
        &["carve", "--rect", "1,2,3"][..],
        &["carve", "--circle", "1,2,x"],
        &["carve"],
        &["carve", "--rect", "1,2,3,4", "--circle", "1,2,3"],
        &["paint", "--rect", "1,2,3,4", "--layer", "front", "--colour", "12345g"],
        &["fill", "--rect", "1,2,3,4", "--material", "lava", "--colour", "123456"],
        &["stamp", "--at", "1,2", "--image", "/no/such.png", "--layer", "back"],
    ] {
        let out = olr_cli(bad, &pack);
        assert_eq!(out.status.code(), Some(2), "{bad:?} is a clap error");
        assert_eq!(tree(&pack), before, "{bad:?}");
    }
    let out = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
        .args(["level", "preview"])
        .arg(&pack)
        .args(["--out", "x.png", "--scale", "0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--scale"));
}

#[test]
fn object_add_list_remove() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    let out = object(&["add", "--at", "10,20", "--notes", "near the top"], &pack);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout(&object(&["list"], &pack)), "0: Spawn at (10, 20) — near the top\n");
    let out = object(&["move", "0", "--at", "11,21"], &pack);
    assert!(out.status.success());
    assert_eq!(stdout(&object(&["list"], &pack)), "0: Spawn at (11, 21) — near the top\n");
    assert_eq!(object(&["remove", "3"], &pack).status.code(), Some(1));
    assert!(object(&["remove", "0"], &pack).status.success());
    assert_eq!(stdout(&object(&["list"], &pack)), "");
    assert!(check(&pack).status.success());
}

#[test]
fn resize_reports_dropped_objects() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    assert!(object(&["add", "--at", "500,10"], &pack).status.success());
    let out = olr_cli(&["resize", "--size", "400x350", "--material", "rock", "--colour", "808080"], &pack);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout(&out), "dropped: Spawn at (500, 10)\n");
    let out = check(&pack);
    assert!(out.status.success(), "{}", stdout(&out));
}

#[test]
fn preview_writes_a_png_and_leaves_the_pack() {
    let Some((tmp, pack)) = dirt_level() else { return };
    assert!(object(&["add", "--at", "10,20"], &pack).status.success());
    let before = tree(&pack);
    let png = tmp.path().join("preview.png");
    for extra in [&[][..], &["--material"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
            .args(["level", "preview"])
            .arg(&pack)
            .arg("--out")
            .arg(&png)
            .args(["--scale", "2"])
            .args(extra)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let img = olr_format::image_io::decode_rgba(&std::fs::read(&png).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (2 * 504, 2 * 350));
        // The spawn's cross, centred on its block: white at its centre,
        // black just outside.
        let (cx, cy) = (2 * 10 + 1, 2 * 20 + 1);
        assert_eq!(img.get(cx, cy), olr_data::image::Rgba::rgb(255, 255, 255), "{extra:?}");
        assert_eq!(img.get(cx + 2, cy), olr_data::image::Rgba::rgb(0, 0, 0), "{extra:?}");
    }
    assert_eq!(tree(&pack), before);
}

#[test]
fn stamp_and_paint_round_trip_through_fmt() {
    let Some((tmp, pack)) = dirt_level() else { return };
    let mut image = olr_data::image::RgbaImage::new(3, 2);
    image.set(1, 1, olr_data::image::Rgba::rgb(9, 200, 30));
    let file = tmp.path().join("s.png");
    std::fs::write(&file, olr_format::image_io::encode_rgba(&image)).unwrap();
    let file = file.to_str().unwrap();
    for op in [
        &["paint", "--rect", "10,10,40,30", "--layer", "back", "--colour", "#3366ff"][..],
        &["paint", "--circle", "60,60,9", "--layer", "front", "--tile", file],
        &["fill", "--rect", "0,0,20,20", "--material", "rock", "--colour", "777777"],
        &["stamp", "--at", "5,5", "--image", file, "--layer", "front"],
    ] {
        let out = olr_cli(op, &pack);
        assert!(out.status.success(), "{op:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let out = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
        .args(["pack", "fmt", "--check"])
        .arg(&pack)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(check(&pack).status.success());
}

/// Review B-1: negative values are values, not flags; review B-9: a shape
/// wholly outside the level saves an unchanged pack.
#[test]
fn negative_coordinates_are_values() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    let before = tree(&pack);
    let out = olr_cli(&["carve", "--rect", "-100,-100,5,5"], &pack);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(tree(&pack), before, "an empty intersection changes nothing");
    let out = olr_cli(&["carve", "--rect", "-10,-10,30,30"], &pack);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = olr_cli(&["carve", "--circle", "-5,-5,20"], &pack);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(object(&["add", "--at", "-3,-4"], &pack).status.success());
    let out = olr_cli(&["resize", "--size", "500x350", "--offset", "-4,0"], &pack);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout(&out), "dropped: Spawn at (-3, -4)\n");
    let out = olr_cli(&["stamp", "--at", "-1,-1", "--image", "/no/such.png", "--layer", "back"], &pack);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--image"),
        "the bad value is the image, not -1,-1"
    );
}

/// Review B-1: a spawn placed outside the level is saved with a warning.
#[test]
fn a_spawn_outside_the_level_warns() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    let out = object(&["add", "--at", "1000,20"], &pack);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("objects[0]: spawn at (1000, 20) is outside the level (504×350)"), "{stderr}");
    let out = object(&["move", "0", "--at", "10,20"], &pack);
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// Review B-2: an image argument is read bounded and only from a file.
#[cfg(unix)]
#[test]
fn an_image_that_is_no_file_is_refused() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    let out = olr_cli(&["stamp", "--at", "1,1", "--image", "/dev/zero", "--layer", "back"], &pack);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a file"), "{out:?}");
}

/// Review B-3: what an operation did is printed only once it is saved.
#[cfg(unix)]
#[test]
fn nothing_is_reported_when_the_save_fails() {
    use std::os::unix::fs::PermissionsExt;
    let Some((tmp, pack)) = dirt_level() else { return };
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = object(&["add", "--at", "10,20"], &pack);
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "{}", stdout(&out));
}

/// Review B-4: the preview never writes into the pack.
#[test]
fn a_preview_into_the_pack_is_refused() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    let before = tree(&pack);
    let out = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
        .args(["level", "preview"])
        .arg(&pack)
        .arg("--out")
        .arg(pack.join("front.png"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("inside the pack"), "{out:?}");
    assert_eq!(tree(&pack), before);
}

/// Review B-5, B-6: `object list` reads `level.ron` alone, and notes print
/// on one line.
#[test]
fn object_list_reads_level_ron_alone() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    assert!(object(&["add", "--at", "1,2", "--notes", "two\nlines"], &pack).status.success());
    std::fs::write(pack.join("front.png"), b"not a png").unwrap();
    let out = object(&["list"], &pack);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout(&out), "0: Spawn at (1, 2) — two\\nlines\n");
}

/// `olr-cli level <group…> <args…>` with `{pack}` replaced.
fn level(args: &[&str], pack: &Path) -> Output {
    let args: Vec<String> = args.iter().map(|a| a.replace("{pack}", pack.to_str().unwrap())).collect();
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).arg("level").args(&args).output().unwrap()
}

fn ok(o: &Output) -> String {
    assert!(o.status.success(), "{}{}", stdout(o), String::from_utf8_lossy(&o.stderr));
    stdout(o)
}

fn look_of(pack: &Path) -> olr_look::LevelLook {
    let src = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(pack)).unwrap();
    olr_format::levels::compile_look(&src).unwrap().expect("a look")
}

#[test]
fn look_cli_sets_every_field() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    ok(&level(
        &[
            "look",
            "{pack}",
            "--ambient",
            "202040:30",
            "--max-light",
            "150",
            "--steps",
            "4",
            "--softness",
            "2",
            "--exposure",
            "-50",
            "--contrast",
            "110",
            "--saturation",
            "90",
            "--tint",
            "ffeedd",
        ],
        &pack,
    ));
    let look = look_of(&pack);
    assert_eq!((look.ambient.colour, look.ambient.intensity), ([0x20, 0x20, 0x40], 30));
    assert_eq!((look.max_light, look.light_steps, look.softness), (150, 4, 2));
    assert_eq!((look.grade.exposure, look.grade.contrast, look.grade.saturation), (-50, 110, 90));
    assert_eq!(look.grade.tint, [0xff, 0xee, 0xdd]);
    // One flag changes one field.
    ok(&level(&["look", "{pack}", "--steps", "0"], &pack));
    let look = look_of(&pack);
    assert_eq!((look.light_steps, look.max_light), (0, 150));
    ok(&check(&pack));
    ok(&level(&["look", "{pack}", "--clear"], &pack));
    let src = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(&pack)).unwrap();
    assert_eq!(src.look, None);
}

#[test]
fn groups_and_lights_round_trip_through_the_cli() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    ok(&level(
        &[
            "group",
            "add",
            "{pack}",
            "lava",
            "--intensity",
            "150",
            "--reach",
            "unlimited",
            "--animation",
            "Pulse(period_ms: 1000, low: 50, high: 100)",
        ],
        &pack,
    ));
    ok(&level(&["group", "set", "{pack}", "lava", "--colour", "ff4000", "--intensity", "80"], &pack));
    let g = &look_of(&pack).groups[0];
    assert_eq!(
        (g.intensity, g.colour, g.reach),
        (80, olr_look::GlowColour::Tint([255, 64, 0]), olr_look::Reach::Unlimited)
    );
    let out = ok(&level(
        &[
            "light",
            "add",
            "{pack}",
            "--at",
            "10,12",
            "--colour",
            "ff8800",
            "--intensity",
            "120",
            "--reach",
            "64",
            "--radius",
            "3",
            "--animation",
            "Flicker(speed: 8, depth: 25)",
        ],
        &pack,
    ));
    let index = out.trim().strip_prefix("added object ").unwrap().to_string();
    assert!(stdout(&object(&["list"], &pack)).contains(&format!("{index}: Light at (10, 12)")));
    ok(&level(&["light", "set", "{pack}", &index, "--at", "-4,5", "--intensity", "60"], &pack));
    let l = &look_of(&pack).lights[0];
    assert_eq!((l.at, l.radius, l.light.intensity, l.light.colour), ((-4, 5), 3, 60, [255, 136, 0]));
    ok(&check(&pack));
    ok(&level(&["light", "remove", "{pack}", &index], &pack));
    ok(&level(&["group", "remove", "{pack}", "lava"], &pack));
    assert!(look_of(&pack).groups.is_empty() && look_of(&pack).lights.is_empty());
}

#[test]
fn bad_look_values_are_errors() {
    let Some((_tmp, pack)) = dirt_level() else { return };
    let before = tree(&pack);
    for (args, flag) in [
        (vec!["look", "{pack}", "--ambient", "ffffff:101"], "ambient"),
        (vec!["look", "{pack}", "--max-light", "250"], "max_light"),
        (vec!["group", "add", "{pack}", "lava", "--reach", "64", "--animation", "Nope(1)"], "--animation"),
        (vec!["group", "add", "{pack}", "Bad Id", "--reach", "64"], "Bad Id"),
        (
            vec![
                "light",
                "add",
                "{pack}",
                "--at",
                "1,1",
                "--colour",
                "fff",
                "--intensity",
                "5",
                "--reach",
                "9",
            ],
            "--colour",
        ),
        (vec!["light", "set", "{pack}", "9", "--intensity", "5"], "no object 9"),
    ] {
        let o = level(&args, &pack);
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        assert!(!o.status.success() && err.contains(flag), "{args:?}: {err}");
    }
    assert_eq!(tree(&pack), before, "nothing written");
}
