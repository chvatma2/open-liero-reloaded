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
