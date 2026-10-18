//! Loading and saving a pack's authoring source (plan 13d Task 1).

use olr_edit::pack::{SaveError, Source, files, load, save};

use crate::common::{content, copy_pack, packs, tree};

#[test]
fn load_then_files_reproduces_every_bundled_pack() {
    let Some(root) = content() else { return };
    for dir in packs(&root) {
        let pack = load(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let written: std::collections::BTreeMap<String, Vec<u8>> =
            files(&pack).unwrap().into_iter().map(|(p, b)| (p.as_str().to_string(), b)).collect();
        let on_disk = tree(&dir);
        assert_eq!(
            written.keys().collect::<Vec<_>>(),
            on_disk.keys().collect::<Vec<_>>(),
            "{}",
            dir.display()
        );
        for (path, bytes) in &on_disk {
            assert!(written[path] == *bytes, "{}: {path} differs", dir.display());
        }
    }
}

#[test]
fn save_keeps_foreign_files() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    std::fs::write(dir.join("notes.txt"), b"my notes\r\n").unwrap();
    let before = tree(&dir);
    let pack = load(&dir).unwrap();
    assert!(save(&dir, &pack).unwrap().is_empty());
    assert_eq!(tree(&dir), before);
}

#[test]
fn save_refuses_an_invalid_result() {
    let Some(root) = content() else { return };
    let (tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    let before = tree(&dir);
    let mut pack = load(&dir).unwrap();
    let Source::Level(level) = &mut pack.source else { panic!("not a level") };
    level.art_scale = 9;
    assert!(save(&dir, &pack).is_err());
    assert_eq!(tree(&dir), before);
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1, "no hidden sibling left");
}

#[test]
fn a_mod_saved_without_a_weapon_loses_its_file() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-classic");
    let mut pack = load(&dir).unwrap();
    let Source::Mod(m) = &mut pack.source else { panic!("not a mod") };
    let (id, weapon) = m.weapons.pop().unwrap();
    for preset in m.restrictions.values_mut() {
        preset.remove(&weapon.name);
    }
    let file = dir.join("weapons").join(format!("{id}.ron"));
    assert!(file.exists());
    save(&dir, &pack).unwrap();
    assert!(!file.exists(), "{} is left", file.display());
    let errors: Vec<_> = olr_pack::validate(&olr_vfs::DirFs::new(&dir))
        .into_iter()
        .filter(|d| d.severity == olr_pack::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// Review A-6: a pack is bounded as a whole, as its archive would be: at
/// most `MAX_ARCHIVE_FILES` files and `MAX_UNPACKED` bytes, checked from
/// the file lengths before anything is read.
#[test]
fn a_pack_too_large_as_a_whole_does_not_load() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    std::fs::create_dir(dir.join("big")).unwrap();
    for i in 0..9 {
        // Sparse: nothing is written to disk.
        std::fs::File::create(dir.join(format!("big/{i}.bin"))).unwrap().set_len(60 << 20).unwrap();
    }
    let e = load(&dir).unwrap_err().to_string();
    assert!(e.contains("bytes"), "{e}");

    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    std::fs::create_dir(dir.join("many")).unwrap();
    for i in 0..olr_pack::archive::MAX_ARCHIVE_FILES {
        std::fs::write(dir.join(format!("many/{i}")), b"").unwrap();
    }
    let e = load(&dir).unwrap_err().to_string();
    assert!(e.contains("files"), "{e}");
}

/// Review A-3: a result the writers accept but `validate` refuses.
#[test]
fn save_refuses_a_result_that_does_not_validate() {
    let Some(root) = content() else { return };
    let (tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    let before = tree(&dir);
    let mut pack = load(&dir).unwrap();
    pack.manifest.id = "Not A Slug".into();
    assert!(matches!(save(&dir, &pack), Err(SaveError::Invalid(_))));
    assert_eq!(tree(&dir), before);
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1, "no hidden sibling left");
}

/// Review A-1: `olr-cli pack fmt .` names the pack by a path with no file name.
#[test]
fn save_through_a_path_without_a_file_name() {
    let Some(root) = content() else { return };
    let (tmp, dir) = copy_pack(&root, "olx-classic");
    let path = dir.join("weapons").join("..");
    let mut pack = load(&path).unwrap();
    let Source::Mod(m) = &mut pack.source else { panic!("not a mod") };
    m.name = "Renamed".into();
    save(&path, &pack).unwrap();
    assert!(String::from_utf8(std::fs::read(dir.join("mod.ron")).unwrap()).unwrap().contains("Renamed"));
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1, "no hidden sibling left");
}

/// Review A-1: a symlinked pack is saved where the link points; the link stays.
#[cfg(unix)]
#[test]
fn save_through_a_symlink_keeps_the_link() {
    let Some(root) = content() else { return };
    let (tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&dir, &link).unwrap();
    let mut pack = load(&link).unwrap();
    let Source::Level(level) = &mut pack.source else { panic!("not a level") };
    level.name = "Renamed".into();
    save(&link, &pack).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    let text = String::from_utf8(std::fs::read(dir.join("level.ron")).unwrap()).unwrap();
    assert!(text.contains("Renamed"), "{text}");
}

/// Review A-4: a file whose name a pack path cannot hold would be lost by
/// the whole-pack save, so the pack does not load.
#[cfg(unix)]
#[test]
fn a_file_the_format_cannot_name_refuses_the_load() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    std::fs::write(dir.join("a\\b.txt"), b"x").unwrap();
    let e = load(&dir).unwrap_err().to_string();
    assert!(e.contains("a\\b.txt"), "{e}");
}

/// Dirt Level, composed of its raster base and one drawn dirt layer.
fn composed(dir: &std::path::Path) -> olr_edit::pack::Pack {
    let mut pack = load(dir).unwrap();
    let Source::Level(level) = &pack.source else { panic!("not a level") };
    let mut src = olr_edit::compose::compose_from(level);
    let (w, h) = (level.front.width(), level.front.height());
    src.files.insert(
        olr_format::levels::layer_file("ground"),
        olr_format::image_io::encode_rgba(&olr_data::image::RgbaImage::new(w, h)),
    );
    src.composition.elements.push(olr_format::levels::Element {
        id: "ground".into(),
        name: "Ground".into(),
        visible: true,
        locked: false,
        kind: olr_format::levels::ElementKind::drawn(olr_format::levels::ElementMaterial::Dirt),
        glow: None,
    });
    pack.composition = Some(src);
    pack
}

#[test]
fn a_composition_survives_load_and_save() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    let pack = composed(&dir);
    let written = files(&pack).unwrap();
    assert!(written.contains_key(&olr_vfs::VPath::new("editor/composition.ron").unwrap()));
    assert!(written.contains_key(&olr_vfs::VPath::new("editor/layers/ground.png").unwrap()));
    save(&dir, &pack).unwrap();
    assert_eq!(load(&dir).unwrap().composition, pack.composition);
    assert_eq!(olr_pack::validate(&olr_vfs::DirFs::new(&dir)), vec![]);
}

#[test]
fn a_pack_without_editor_loads_with_none() {
    let Some(root) = content() else { return };
    assert_eq!(load(&root.join("olx-level-dirt-level")).unwrap().composition, None);
}

/// `pack fmt` canonicalises `composition.ron` like any authoring file.
#[test]
fn fmt_rewrites_a_hand_edited_composition() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
    save(&dir, &composed(&dir)).unwrap();
    let canonical = std::fs::read_to_string(dir.join("editor/composition.ron")).unwrap();
    std::fs::write(dir.join("editor/composition.ron"), canonical.replace("    ", "  ")).unwrap();
    let listed: Vec<String> =
        olr_edit::fmt::unformatted(&dir).unwrap().iter().map(|p| p.as_str().to_string()).collect();
    assert_eq!(listed, ["editor/composition.ron"]);
    olr_edit::fmt::format(&dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("editor/composition.ron")).unwrap(), canonical);
}
