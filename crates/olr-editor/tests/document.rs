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
