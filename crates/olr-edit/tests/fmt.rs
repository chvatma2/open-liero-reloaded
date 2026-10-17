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
    assert_eq!(paths(&format(&dir).unwrap()), ["level.ron"]);
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
