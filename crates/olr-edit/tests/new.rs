//! Pack templates (plan 13d Task 4).

use olr_data::level::Material;
use olr_data::manifest::PackKind;
use olr_edit::new::{Template, new_pack};
use olr_vfs::DirFs;

use crate::common::{content, tree};

#[test]
fn a_new_mod_checks_clean_and_compiles() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("my-mod");
    new_pack(&dir, PackKind::Mod, "my-mod", "My Mod", Template::Mod).unwrap();
    let src = DirFs::new(&dir);
    assert_eq!(olr_pack::validate(&src), vec![]);
    let pack = olr_pack::read_mod(&src).unwrap();
    assert_eq!(pack.def.weapons.len(), 1);
    assert_eq!((pack.manifest.id.as_str(), pack.manifest.name.as_str()), ("my-mod", "My Mod"));
    assert_eq!(pack.manifest.version, "0.1.0");
}

#[test]
fn a_new_level_checks_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("t");
    std::fs::create_dir(&dir).unwrap(); // an empty directory is fine
    let template = Template::Level { size: (64, 48), theme: "olx-theme-dirt" };
    new_pack(&dir, PackKind::Level, "t", "T", template).unwrap();
    let src = DirFs::new(&dir);
    assert_eq!(olr_pack::validate(&src), vec![]);
    let level = olr_pack::read_level(&src).unwrap().level;
    assert_eq!((level.def.width, level.def.height), (64, 48));
    for y in 0..48 {
        for x in 0..64 {
            let border = x == 0 || y == 0 || x == 63 || y == 47;
            let want = if border { Material::Rock } else { Material::Dirt };
            assert_eq!(level.material.get(x, y), want, "({x}, {y})");
        }
    }
}

#[test]
fn a_skin_from_the_default_skin() {
    let Some(root) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("my-skin");
    let from = root.join("olx-skin-default");
    new_pack(&dir, PackKind::Skin, "my-skin", "Mine", Template::From(&from)).unwrap();
    let src = DirFs::new(&dir);
    assert_eq!(olr_pack::validate(&src), vec![]);
    let made = olr_pack::read_skin(&src).unwrap();
    let original = olr_pack::read_skin(&DirFs::new(&from)).unwrap();
    assert_eq!(made.manifest.id, "my-skin");
    assert_eq!(made.manifest.origin, None);
    assert_eq!(made.skin.def.name, "Mine");
    assert_eq!(made.skin.def.layout, original.skin.def.layout);
    assert!(made.skin.image == original.skin.image, "the compiled sheet differs");
}

#[test]
fn refuses_an_existing_directory_and_a_bad_id() {
    let tmp = tempfile::tempdir().unwrap();
    let full = tmp.path().join("full");
    std::fs::create_dir(&full).unwrap();
    std::fs::write(full.join("keep.txt"), b"mine").unwrap();
    assert!(new_pack(&full, PackKind::Mod, "full", "Full", Template::Mod).is_err());
    assert_eq!(tree(&full).into_iter().collect::<Vec<_>>(), [("keep.txt".to_string(), b"mine".to_vec())]);

    let bad = tmp.path().join("bad");
    assert!(new_pack(&bad, PackKind::Mod, "My Mod", "My Mod", Template::Mod).is_err());
    assert!(!bad.exists());

    let Some(root) = content() else { return };
    let skin = tmp.path().join("skin");
    let level = root.join("olx-level-dirt-level");
    assert!(new_pack(&skin, PackKind::Skin, "skin", "Skin", Template::From(&level)).is_err());
    assert!(!skin.exists());
}
