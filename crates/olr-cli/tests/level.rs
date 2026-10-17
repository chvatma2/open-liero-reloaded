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
