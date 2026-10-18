//! The document (plan 14b Task 4): a level pack as the editor holds it.

use olr_editor::{DocError, Document, Severity};
use olr_format::levels::LevelObject;

use crate::common::{catalog, content, copy_pack, files, fixture, open_fixture, tree};

#[test]
fn opening_and_saving_changes_nothing() {
    let Some(root) = content() else { return };
    let (_tmp, doc) = open_fixture(&root);
    assert!(files(&doc) == tree(doc.dir()), "the composed fixture comes back byte for byte");

    let (_tmp, dir) = copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    let on_disk = tree(&dir);
    let doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog(&root)).unwrap();
    let saved = files(&doc);
    let gained: Vec<&String> = saved.keys().filter(|k| !on_disk.contains_key(*k)).collect();
    assert_eq!(
        gained,
        [
            "editor/composition.ron",
            "editor/layers/base.back.png",
            "editor/layers/base.front.png",
            "editor/layers/base.material.png"
        ]
    );
    for (path, bytes) in &on_disk {
        assert!(saved.get(path) == Some(bytes), "{path} unchanged");
    }
}

#[test]
fn a_missing_theme_is_an_error() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let mut cat = catalog(&root);
    cat.themes.clear();
    let e = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &cat).unwrap_err();
    assert!(matches!(&e, DocError::NoTheme(t) if t == "olx-theme-dirt"), "{e}");
}

#[test]
fn a_mod_pack_is_not_a_level() {
    let Some(root) = content() else { return };
    let dir = root.join("olx-classic");
    let e = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog(&root)).unwrap_err();
    assert!(matches!(e, DocError::NotALevel), "{e}");
}

#[test]
fn diagnostics_list_spawns_outside_and_none() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let open = |objects: Vec<LevelObject>| {
        let mut pack = olr_edit::pack::load(&dir).unwrap();
        let olr_edit::pack::Source::Level(l) = &mut pack.source else { panic!() };
        l.objects = objects;
        Document::open(pack, dir.clone(), &catalog(&root)).unwrap().diagnostics()
    };
    let none = open(Vec::new());
    assert!(none.iter().any(|p| p.severity == Severity::Warning && p.text.contains("no spawn")), "{none:?}");
    let outside = open(vec![LevelObject::Spawn { at: (-5, 0), notes: String::new() }]);
    assert!(
        outside.iter().any(|p| p.severity == Severity::Warning && p.text.contains("outside the level")),
        "{outside:?}"
    );
    assert!(!outside.iter().any(|p| p.text.contains("no spawn")), "{outside:?}");
}

/// The CRC-32 of PNG chunks (ISO 3309).
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// `png` with a `tEXt` chunk before its `IEND`: the same pixels, other bytes.
fn with_text(png: &[u8]) -> Vec<u8> {
    let (body, iend) = png.split_at(png.len() - 12);
    let data = b"Comment\0from another editor";
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    let mut typed = b"tEXt".to_vec();
    typed.extend_from_slice(data);
    chunk.extend_from_slice(&typed);
    chunk.extend_from_slice(&crc32(&typed).to_be_bytes());
    [body, &chunk, iend].concat()
}

/// A layer another program wrote (other bytes, same pixels) is saved as it
/// was until it changes.
#[test]
fn a_foreign_layer_file_is_kept_until_changed() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let path = dir.join("editor/layers/tunnels.png");
    let foreign = with_text(&std::fs::read(&path).unwrap());
    std::fs::write(&path, &foreign).unwrap();
    let mut doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir.clone(), &catalog(&root)).unwrap();
    assert!(files(&doc)["editor/layers/tunnels.png"] == foreign);
    doc.paint(
        "tunnels",
        &olr_edit::level::Shape::Rect { x: 0, y: 0, w: 2, h: 2 },
        &olr_edit::level::Paint::Colour(olr_data::image::Rgba::rgb(1, 2, 3)),
    )
    .unwrap();
    assert!(files(&doc)["editor/layers/tunnels.png"] != foreign);
}

/// Stray files in `editor/` are kept, and new elements' files go beside them.
#[test]
fn stray_editor_files_are_never_overwritten() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    std::fs::write(dir.join("editor/images/photo.jpg"), b"not mine to replace").unwrap();
    std::fs::write(dir.join("editor/layers/layer.png"), b"nor this").unwrap();
    let mut doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir.clone(), &catalog(&root)).unwrap();
    let jpeg = std::fs::read(fixture().join("editor/images/tiny.jpg")).unwrap();
    doc.add_image("photo.jpg", jpeg, (10, 10)).unwrap();
    doc.add_layer(olr_format::levels::ElementMaterial::Dirt, "Layer").unwrap();
    let f = files(&doc);
    assert_eq!(f["editor/images/photo.jpg"], b"not mine to replace");
    assert_eq!(f["editor/layers/layer.png"], b"nor this");
    assert!(f.contains_key("editor/images/photo-2.jpg"));
    assert!(f.contains_key("editor/layers/layer-2.png"));
}

/// A snapshot makes the same pack as its document (plan 14c: the host's
/// recovery copy is encoded from one on a thread of its own).
#[test]
fn a_snapshot_packs_as_its_document() {
    let Some(root) = crate::common::content() else { return };
    let (_tmp, mut doc) = crate::common::open_fixture(&root);
    let shape = olr_edit::level::Shape::Rect { x: 10, y: 10, w: 20, h: 8 };
    doc.paint("tunnels", &shape, &olr_edit::level::Paint::Colour(olr_data::image::Rgba::rgb(9, 9, 9)))
        .unwrap();
    let snap = doc.snapshot();
    assert_eq!(snap.to_pack(), doc.to_pack());
    let moved = std::thread::spawn(move || snap.to_pack()).join().unwrap();
    assert_eq!(moved, doc.to_pack());
}
