//! Loose dirt in the editor (the loose dirt design): the level's loose
//! materials (added, changed, removed only when unused, each one step)
//! and a dirt layer's loose material, baked into `loose.png` on save.

use olr_edit::element::Property;
use olr_editor::{EditorAction as A, EditorState};
use olr_format::levels::LooseSource;

use crate::common::{content, ground, run};

fn loose(s: &EditorState) -> Vec<LooseSource> {
    s.active_document().unwrap().loose().to_vec()
}

fn element(s: &EditorState, id: &str) -> olr_format::levels::Element {
    let doc = s.active_document().unwrap();
    doc.items()[doc.index_of(id).unwrap()].element.clone()
}

fn message(s: &EditorState) -> String {
    s.status().message.clone().unwrap_or_default()
}

#[test]
fn adding_a_loose_material_is_undoable() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    assert!(loose(&s).is_empty());
    run(&mut s, A::AddLoose);
    run(&mut s, A::AddLoose);
    let ids: Vec<String> = loose(&s).iter().map(|m| m.id.clone()).collect();
    assert_eq!(ids, ["loose", "loose-2"], "new ids, made unique");
    assert_eq!(loose(&s)[0], LooseSource::new("loose", "Loose dirt"));
    // Its fields change in one step each; a value out of range is refused.
    let steep = LooseSource { name: "Sand".into(), repose: 30, gravity: 400, ..loose(&s)[0].clone() };
    run(&mut s, A::SetLoose { id: "loose".into(), material: steep.clone() });
    assert_eq!(loose(&s)[0], steep);
    run(&mut s, A::SetLoose { id: "loose".into(), material: LooseSource { repose: 90, ..steep.clone() } });
    assert_eq!(loose(&s)[0], steep);
    assert!(message(&s).contains("repose"), "{}", message(&s));
    run(&mut s, A::Undo);
    assert_eq!(loose(&s)[0], LooseSource::new("loose", "Loose dirt"));
    run(&mut s, A::Undo);
    run(&mut s, A::Undo);
    assert!(loose(&s).is_empty());
    run(&mut s, A::Redo);
    assert_eq!(loose(&s).len(), 1);
}

#[test]
fn a_dirt_layer_takes_a_loose_material() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    run(&mut s, A::AddLoose);
    // A rock picture refuses it; a dirt one takes it, one step.
    run(&mut s, A::Set("tiny".into(), Property::Loose(Some("loose".into()))));
    assert_eq!(element(&s, "tiny").loose, None);
    run(&mut s, A::Set("gradient".into(), Property::Loose(Some("gravel".into()))));
    assert_eq!(element(&s, "gradient").loose, None, "no such material");
    run(&mut s, A::Set("gradient".into(), Property::Loose(Some("loose".into()))));
    assert_eq!(element(&s, "gradient").loose.as_deref(), Some("loose"));
    let level = s.active_document().unwrap().level();
    let map = level.loose_map.as_ref().expect("the bake marks its dirt loose");
    assert!(map.contains(&1));
    assert!(map.iter().zip(&level.material).all(|(v, m)| *v == 0 || *m == olr_data::level::Material::Dirt));
    run(&mut s, A::Undo);
    assert_eq!(element(&s, "gradient").loose, None);
    assert_eq!(s.active_document().unwrap().level().loose_map, None);
}

#[test]
fn removing_a_used_material_is_refused_with_a_notice() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    run(&mut s, A::AddLoose);
    run(&mut s, A::Set("gradient".into(), Property::Loose(Some("loose".into()))));
    run(&mut s, A::RemoveLoose("loose".into()));
    assert_eq!(loose(&s).len(), 1);
    assert!(message(&s).contains("Gradient"), "{}", message(&s));
    run(&mut s, A::Set("gradient".into(), Property::Loose(None)));
    run(&mut s, A::RemoveLoose("loose".into()));
    assert!(loose(&s).is_empty());
}

/// Saved from the ground, `level.ron` lists the materials, the ground's
/// `asset.ron` too, and `loose.png` marks the layer's dirt; opened again,
/// all is as it was, and the level plays its loose dirt.
#[test]
fn a_saved_level_keeps_its_loose_dirt() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    let dir = s.active_document().unwrap().dir().to_path_buf();
    run(&mut s, A::AddLoose);
    run(&mut s, A::Set("gradient".into(), Property::Loose(Some("loose".into()))));
    let map = s.active_document().unwrap().level().loose_map.clone().expect("baked");
    run(&mut s, A::Save);
    assert!(!s.active_dirty());
    let files = crate::common::tree(&dir);
    assert!(files.contains_key("loose.png"), "loose.png saved");
    let pack = olr_edit::pack::load(&dir).unwrap();
    let olr_edit::pack::Source::Level(level) = pack.source.clone() else { panic!("a level") };
    assert_eq!(level.loose, [LooseSource::new("loose", "Loose dirt")]);
    assert_eq!(level.loose_map.as_ref(), Some(&map));
    let ground = &pack.placed.as_ref().unwrap().assets[olr_format::levels::GROUND_KEY];
    assert_eq!(ground.file.loose, level.loose);
    let doc = olr_editor::Document::open(pack, dir, &crate::common::catalog(&root)).unwrap();
    assert_eq!(doc.loose(), level.loose);
    // The level's own (level tab) document holds them too.
    assert_eq!(s.document().unwrap().loose(), level.loose);
}
