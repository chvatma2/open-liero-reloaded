//! Loading and saving a pack's authoring source (plan 13d Task 1).

use olr_edit::pack::{Source, files, load, save};

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
