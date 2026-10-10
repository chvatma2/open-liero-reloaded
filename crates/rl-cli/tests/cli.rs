//! End-to-end tests of the `rl-cli` binary on a synthetic OLX game directory.

use std::path::Path;
use std::process::Command;

use rl_data::image::{Rgba, RgbaImage};

fn png(w: u32, h: u32, c: Rgba) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    rl_pack::image_io::encode_rgba(&img)
}

fn gamedir(root: &Path) {
    let theme = root.join("data/themes/Dirt");
    std::fs::create_dir_all(&theme).unwrap();
    std::fs::write(theme.join("theme.txt"), "[General]\nNumStones = 0\nNumMisc = 0\n").unwrap();
    for f in ["Hole1", "Hole2", "Hole3", "Hole4", "Hole5", "FrontTile", "BackTile"] {
        std::fs::write(theme.join(format!("{f}.png")), png(4, 4, Rgba::BLACK)).unwrap();
    }
    let skins = root.join("skins");
    std::fs::create_dir_all(&skins).unwrap();
    std::fs::write(skins.join("default.png"), png(672, 36, Rgba::rgb(200, 200, 200))).unwrap();
    std::fs::write(skins.join("Default.png"), png(672, 36, Rgba::rgb(100, 100, 100))).unwrap();
    std::fs::write(skins.join("broken.png"), b"not a png").unwrap();
    std::fs::write(skins.join("readme.txt"), b"ignored").unwrap();
}

fn rl_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rl-cli"))
}

#[test]
fn import_then_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let status = rl_cli()
        .args(["import", "--olx"])
        .arg(&src)
        .arg("--out")
        .arg(&out)
        .args(["--revision", "abc", "--theme", "data/themes/Dirt", "--skins"])
        .status()
        .unwrap();
    assert!(!status.success(), "the broken skin must make import fail");
    // Both skins slug to the same id; the second gets a suffix.
    assert!(out.join("olx-skin-default/pack.ron").is_file());
    assert!(out.join("olx-skin-default-2/pack.ron").is_file());
    assert!(out.join("olx-theme-dirt/theme.ron").is_file());
    let report = std::fs::read_to_string(out.join("import-report.ron")).unwrap();
    assert!(report.contains("skins/broken.png") && report.contains("error: Some("), "{report}");

    let status = rl_cli().arg("validate").arg(&out).status().unwrap();
    assert!(status.success());
}

#[test]
fn validate_fails_on_broken_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("bad");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("pack.ron"), "(oops").unwrap();
    let out = rl_cli().arg("validate").arg(&pack).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("pack.ron:1:"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn reimport_replaces_stale_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let import = || {
        rl_cli()
            .args(["import", "--olx"])
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .args(["--theme", "data/themes/Dirt"])
            .status()
            .unwrap()
    };
    assert!(import().success());
    let stale = out.join("olx-theme-dirt/stale.png");
    std::fs::write(&stale, b"left over from an older import").unwrap();
    assert!(import().success());
    assert!(!stale.exists(), "re-import must remove files the importer no longer writes");
}

#[test]
fn reimport_removes_packs_no_longer_imported() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let import = |extra: &[&str]| {
        rl_cli()
            .args(["import", "--olx"])
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .args(["--theme", "data/themes/Dirt"])
            .args(extra)
            .status()
            .unwrap()
    };
    import(&["--skins"]);
    assert!(out.join("olx-skin-default/pack.ron").is_file());
    let unrelated = out.join("notes.txt");
    std::fs::write(&unrelated, b"not a pack").unwrap();
    assert!(import(&[]).success());
    assert!(!out.join("olx-skin-default").exists(), "packs dropped from the import must be removed");
    assert!(out.join("olx-theme-dirt/pack.ron").is_file());
    assert!(unrelated.exists(), "only pack directories are removed");
}
