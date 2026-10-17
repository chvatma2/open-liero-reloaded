//! The canonical formatter (plan 13d Task 3).

use olr_edit::fmt::{format, unformatted};
use olr_vfs::VPath;

use crate::common::{content, copy_pack, packs, tree};

fn paths(list: &[VPath]) -> Vec<&str> {
    list.iter().map(VPath::as_str).collect()
}

/// R5: the `check` guard that every bundled pack is canonical.
#[test]
fn bundled_packs_are_canonical() {
    let Some(root) = content() else { return };
    for dir in packs(&root) {
        assert_eq!(unformatted(&dir).unwrap(), Vec::<VPath>::new(), "{}", dir.display());
    }
}

#[test]
fn fmt_rewrites_a_hand_edited_file() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    let original = std::fs::read(dir.join("level.ron")).unwrap();
    let edited =
        "(\n  theme: \"olx-theme-dirt\",\n  size: (504, 350),\n  art_scale: 1,\n  name: \"Dirt Level\",\n)\n";
    std::fs::write(dir.join("level.ron"), edited).unwrap();
    assert_eq!(paths(&unformatted(&dir).unwrap()), ["level.ron"]);
    assert_eq!(paths(&format(&dir).unwrap().files), ["level.ron"]);
    assert_eq!(std::fs::read(dir.join("level.ron")).unwrap(), original);
    assert!(unformatted(&dir).unwrap().is_empty());
}

#[test]
fn fmt_keeps_foreign_files() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    std::fs::write(dir.join("README.txt"), b"  odd\tlayout  ").unwrap();
    std::fs::create_dir(dir.join("extra")).unwrap();
    std::fs::write(dir.join("extra/x.bin"), [0u8, 255, 7]).unwrap();
    std::fs::write(dir.join("level.ron"), "(name:\"Dirt Level\",size:(504,350),theme:\"olx-theme-dirt\")")
        .unwrap();
    assert_eq!(paths(&unformatted(&dir).unwrap()), ["level.ron"]);
    format(&dir).unwrap();
    assert_eq!(std::fs::read(dir.join("README.txt")).unwrap(), b"  odd\tlayout  ");
    assert_eq!(std::fs::read(dir.join("extra/x.bin")).unwrap(), [0u8, 255, 7]);
    assert!(unformatted(&dir).unwrap().is_empty());
}

/// Review A-9: a format file the canonical writer leaves out (an empty
/// `restrictions.ron`) is not canonical, and `format` removes it.
#[test]
fn fmt_removes_an_empty_restrictions_file() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-classic");
    std::fs::write(dir.join("restrictions.ron"), "{}\n").unwrap();
    assert_eq!(paths(&unformatted(&dir).unwrap()), ["restrictions.ron"]);
    assert_eq!(paths(&format(&dir).unwrap().files), ["restrictions.ron"]);
    assert!(!dir.join("restrictions.ron").exists());
    assert!(unformatted(&dir).unwrap().is_empty());
}

/// Review A-1: the save's warnings reach the caller.
#[cfg(unix)]
#[test]
fn fmt_returns_the_saves_warnings() {
    use std::os::unix::fs::PermissionsExt;
    let Some(root) = content() else { return };
    let (tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    std::fs::create_dir(dir.join("locked")).unwrap();
    std::fs::write(dir.join("locked/x"), b"x").unwrap();
    std::fs::set_permissions(dir.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
    std::fs::write(dir.join("level.ron"), "(name:\"Dirt Level\",size:(504,350),theme:\"olx-theme-dirt\")")
        .unwrap();
    let formatted = format(&dir).unwrap();
    let aside = tmp.path().join(".replaced-olx-level-dirt-level");
    std::fs::set_permissions(aside.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(paths(&formatted.files), ["level.ron"]);
    assert_eq!(formatted.warnings.len(), 1, "{:?}", formatted.warnings);
}

/// Review A-2: `unformatted` and `format` agree on a pack that loads but
/// would not check: both refuse it, naming the problem.
#[test]
fn an_invalid_pack_is_refused_by_both() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-classic");
    std::fs::remove_file(dir.join("gfx/missile.png")).unwrap();
    let text = std::fs::read_to_string(dir.join("mod.ron")).unwrap();
    std::fs::write(dir.join("mod.ron"), text.replace("    ", "  ")).unwrap();
    let e = unformatted(&dir).unwrap_err();
    assert!(matches!(e, olr_edit::pack::SaveError::Invalid(_)), "{e}");
    assert!(e.to_string().contains("gfx/missile.png"), "{e}");
    assert!(matches!(format(&dir).unwrap_err(), olr_edit::pack::SaveError::Invalid(_)));
}

#[test]
fn fmt_leaves_an_unloadable_pack_untouched() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    std::fs::write(dir.join("level.ron"), "(name: \"Dirt Level\", oops").unwrap();
    let before = tree(&dir);
    assert!(format(&dir).is_err());
    assert_eq!(tree(&dir), before);
}

#[test]
fn fmt_canonicalises_a_reencoded_png() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    let original = std::fs::read(dir.join("front.png")).unwrap();
    let image = olr_format::image_io::decode_rgba(&original).unwrap();
    let mut reencoded = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut reencoded, image.width(), image.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header().unwrap().write_image_data(image.as_raw()).unwrap();
    }
    assert_ne!(reencoded, original);
    std::fs::write(dir.join("front.png"), &reencoded).unwrap();
    assert_eq!(paths(&unformatted(&dir).unwrap()), ["front.png"]);
    format(&dir).unwrap();
    assert_eq!(std::fs::read(dir.join("front.png")).unwrap(), original);
}
