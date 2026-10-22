//! The material editor's state (issue #44): the library opened, built-in
//! materials read-only, every field's bounds, the ramp's keys, New, Copy,
//! Rename and Delete as commands answered by the host, the unsaved
//! changes' question, undo and redo, a recovered material.

use olr_format::materials::MaterialSource;
use olr_look::{LAVA_RAMP, MAX_RAMP_KEYS};
use olr_material_editor::{
    Choice, Dialog, Edit, Key, MaterialAction as A, MaterialCommand as C, MaterialEditorState, Next, Purpose,
    free_id,
};

use crate::common::{library, material};

fn heat(s: &MaterialEditorState) -> &olr_format::materials::HeatSource {
    s.doc().unwrap().heat.as_ref().unwrap()
}

#[test]
fn it_opens_the_users_first_material_and_built_in_ones_never_change() {
    let mut s = MaterialEditorState::new(library());
    assert_eq!(s.selected(), Some(&Key::user("lava-rock")));
    assert!(!s.read_only() && !s.dirty());
    s.update(A::Select(Key::builtin("stone")));
    assert!(s.read_only());
    s.update(A::Set(Edit::Capacity(300)));
    assert_eq!(heat(&s).capacity, 100);
    assert!(s.status().contains("edit a copy"), "{}", s.status());
    assert!(s.update(A::Save).is_empty());
    for a in [A::Rename, A::Delete] {
        s.update(a);
        assert_eq!(s.dialog(), None);
    }
    // A library of only built-in materials opens the first.
    let builtins: Vec<_> = library().into_iter().filter(|e| e.builtin).collect();
    let s = MaterialEditorState::new(builtins);
    assert_eq!(s.selected(), Some(&Key::builtin("stone")));
    // An empty one opens nothing, and nothing acts on it.
    let mut s = MaterialEditorState::new(Vec::new());
    assert!(s.doc().is_none());
    for a in [A::Save, A::Copy, A::Rename, A::Delete, A::Undo, A::Set(Edit::Capacity(5))] {
        assert!(s.update(a).is_empty());
    }
    assert_eq!(s.update(A::Close), vec![C::Close]);
}

#[test]
fn every_field_is_clamped_to_its_bounds() {
    let mut s = MaterialEditorState::new(library());
    type Case = (Edit, fn(&olr_format::materials::HeatSource) -> u16, u16);
    let cases: [Case; 10] = [
        (Edit::Sensitivity(101), |h| h.sensitivity.into(), 100),
        (Edit::Sensitivity(-5), |h| h.sensitivity.into(), 0),
        (Edit::Capacity(5000), |h| h.capacity, 1000),
        (Edit::Capacity(3), |h| h.capacity, 10),
        (Edit::Conductivity(1001), |h| h.conductivity, 1000),
        (Edit::Conductivity(-1), |h| h.conductivity, 0),
        (Edit::Cooling(0), |h| h.cooling, 1),
        (Edit::Cooling(i64::MAX), |h| h.cooling, 1000),
        (Edit::Light(200), |h| h.look.light.into(), 100),
        (Edit::Light(i64::MIN), |h| h.look.light.into(), 0),
    ];
    for (e, get, want) in cases {
        s.update(A::Set(e.clone()));
        assert_eq!(get(heat(&s)), want, "{e:?}");
    }
    assert!(s.problems().is_empty());
    // A key's temperature stays between its neighbours'.
    s.update(A::Set(Edit::OwnRamp(true)));
    assert_eq!(heat(&s).look.ramp.len(), LAVA_RAMP.len());
    s.update(A::Set(Edit::KeyTemperature(1, 5000)));
    assert_eq!(heat(&s).look.ramp[1].temperature, LAVA_RAMP[2].temperature - 1);
    s.update(A::Set(Edit::KeyTemperature(1, 0)));
    assert_eq!(heat(&s).look.ramp[1].temperature, LAVA_RAMP[0].temperature + 1);
    s.update(A::Set(Edit::KeyTemperature(0, -9)));
    assert_eq!(heat(&s).look.ramp[0].temperature, 1);
    s.update(A::Set(Edit::KeyTemperature(5, 2000)));
    assert_eq!(heat(&s).look.ramp[5].temperature, 1000);
    s.update(A::Set(Edit::KeyGlow(2, 999)));
    assert_eq!(heat(&s).look.ramp[2].glow, 100);
    // A key that is not there changes nothing.
    let before = s.doc().cloned();
    s.update(A::Set(Edit::KeyGlow(40, 1)));
    s.update(A::Set(Edit::RemoveKey(40)));
    assert_eq!(s.doc().cloned(), before);
    assert!(s.problems().is_empty());
}

#[test]
fn the_ramps_keys_are_added_and_removed() {
    let mut s = MaterialEditorState::new(library());
    // Adding a key to the engine's ramp starts an own one from its keys.
    s.update(A::Set(Edit::AddKey));
    let keys = &heat(&s).look.ramp;
    assert_eq!(keys.len(), LAVA_RAMP.len() + 1);
    assert!(keys.windows(2).all(|w| w[0].temperature < w[1].temperature));
    // In the middle of the widest gap (450 to 650, 650 to 850: the hottest).
    assert_eq!(keys[4].temperature, 750);
    while heat(&s).look.ramp.len() < MAX_RAMP_KEYS {
        s.update(A::Set(Edit::AddKey));
    }
    let full = s.doc().cloned();
    s.update(A::Set(Edit::AddKey));
    assert_eq!(s.doc().cloned(), full);
    assert!(s.problems().is_empty());
    s.update(A::Set(Edit::RemoveKey(0)));
    assert_eq!(heat(&s).look.ramp.len(), MAX_RAMP_KEYS - 1);
    s.update(A::Set(Edit::KeyColour(0, [1, 2, 3])));
    assert_eq!(heat(&s).look.ramp[0].colour.to_hex(), "#010203");
    // Off: the engine's ramp again.
    s.update(A::Set(Edit::OwnRamp(false)));
    assert!(heat(&s).look.ramp.is_empty());
    // Without heat, it never heats; on again, stone's.
    s.update(A::Set(Edit::Heats(false)));
    assert!(s.doc().unwrap().heat.is_none());
    s.update(A::Set(Edit::Capacity(5)));
    assert!(s.doc().unwrap().heat.is_none());
    s.update(A::Set(Edit::Heats(true)));
    assert_eq!(heat(&s).capacity, 100);
}

#[test]
fn new_and_copy_write_a_material_with_a_free_id() {
    let mut s = MaterialEditorState::new(library());
    assert!(s.update(A::New).is_empty());
    assert_eq!(s.dialog(), Some(&Dialog::Name(Purpose::New)));
    // A name too long is refused, the dialog stays.
    assert!(s.update(A::Named("x".repeat(65))).is_empty());
    assert_eq!(s.dialog(), Some(&Dialog::Name(Purpose::New)));
    let out = s.update(A::Named("  Stone ".into()));
    let [C::Create(m)] = out.as_slice() else { panic!("{out:?}") };
    // The built-in stone keeps its id.
    assert_eq!((m.id.as_str(), m.name.as_str()), ("stone-2", "Stone"));
    assert_eq!(m.heat.as_ref().unwrap().capacity, 100);
    s.update(A::Created(m.clone()));
    assert_eq!(s.selected(), Some(&Key::user("stone-2")));
    assert!(!s.dirty());
    let users: Vec<&str> =
        s.library().iter().filter(|e| !e.builtin).map(|e| e.material.name.as_str()).collect();
    assert_eq!(users, ["Lava rock", "Stone"]);

    // Edit a copy of a built-in material: its values, a new id.
    s.update(A::Select(Key::builtin("wood")));
    s.update(A::Copy);
    assert_eq!(s.suggested_name(Purpose::Copy), "Wood copy");
    let out = s.update(A::Named("Wood copy".into()));
    let [C::Create(m)] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!(m.id, "wood-copy");
    assert_eq!(m.heat.as_ref().unwrap().capacity, 50);
}

#[test]
fn unsaved_changes_are_asked_about() {
    let mut s = MaterialEditorState::new(library());
    s.update(A::Set(Edit::Capacity(300)));
    assert!(s.dirty());
    // Another material: asked first; Cancel stays.
    assert!(s.update(A::Select(Key::builtin("wood"))).is_empty());
    assert_eq!(s.dialog(), Some(&Dialog::Unsaved(Next::Select(Key::builtin("wood")))));
    s.update(A::Choice(Choice::Cancel));
    assert_eq!(s.selected(), Some(&Key::user("lava-rock")));
    assert!(s.dirty());
    // Save: the save, then (answered) the other material.
    s.update(A::Select(Key::builtin("wood")));
    let out = s.update(A::Choice(Choice::Save));
    let [C::Save(m)] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!(m.heat.as_ref().unwrap().capacity, 300);
    s.update(A::Saved(m.clone()));
    assert_eq!(s.selected(), Some(&Key::builtin("wood")));
    let saved = s.library().iter().find(|e| e.material.id == "lava-rock").unwrap();
    assert_eq!(saved.material.heat.as_ref().unwrap().capacity, 300);
    // Don't save: the library's material again.
    s.update(A::Select(Key::user("lava-rock")));
    s.update(A::Set(Edit::Cooling(7)));
    s.update(A::Close);
    assert_eq!(s.dialog(), Some(&Dialog::Unsaved(Next::Close)));
    assert_eq!(s.update(A::Choice(Choice::DontSave)), vec![C::Close]);
    assert_eq!(heat(&s).cooling, 100);
    // A failed save drops what waited.
    s.update(A::Set(Edit::Cooling(7)));
    s.update(A::Close);
    s.update(A::Choice(Choice::Save));
    assert!(s.update(A::Failed("disk full".into())).is_empty());
    assert_eq!(s.dialog(), Some(&Dialog::Error("disk full".into())));
    s.update(A::Choice(Choice::Cancel));
    assert!(s.update(A::Saved(s.doc().unwrap().clone())).is_empty());
}

#[test]
fn undo_and_redo_step_through_the_changes() {
    let mut s = MaterialEditorState::new(library());
    assert!(!s.can_undo());
    s.update(A::Set(Edit::Capacity(200)));
    s.update(A::Set(Edit::Capacity(300)));
    s.update(A::Undo);
    assert_eq!(heat(&s).capacity, 200);
    s.update(A::Undo);
    assert_eq!(heat(&s).capacity, 120);
    assert!(!s.dirty() && !s.can_undo() && s.can_redo());
    s.update(A::Redo);
    assert_eq!(heat(&s).capacity, 200);
    // A change drops the redo.
    s.update(A::Set(Edit::Cooling(50)));
    assert!(!s.can_redo());
    // The same value is no step.
    s.update(A::Set(Edit::Cooling(50)));
    s.update(A::Undo);
    assert_eq!((heat(&s).capacity, heat(&s).cooling), (200, 100));
}

#[test]
fn rename_and_delete_are_the_users_files() {
    let mut s = MaterialEditorState::new(library());
    s.update(A::Set(Edit::Capacity(300)));
    s.update(A::Rename);
    assert_eq!(s.suggested_name(Purpose::Rename), "Lava rock");
    // The same name is nothing to do.
    assert!(s.update(A::Named("Lava rock".into())).is_empty());
    s.update(A::Rename);
    let out = s.update(A::Named("Magma".into()));
    let [C::Rename { from, to }] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!((from.as_str(), to.id.as_str(), to.name.as_str()), ("lava-rock", "magma", "Magma"));
    // The file keeps the saved values; the change stays unsaved.
    assert_eq!(to.heat.as_ref().unwrap().capacity, 120);
    s.update(A::Renamed { from: from.clone(), to: to.clone() });
    assert_eq!(s.selected(), Some(&Key::user("magma")));
    assert_eq!(s.doc().unwrap().name, "Magma");
    assert!(s.dirty());
    s.update(A::Undo);
    assert!(!s.dirty());
    assert_eq!(s.doc().unwrap().id, "magma");
    assert!(!s.library().iter().any(|e| e.material.id == "lava-rock"));
    // A name whose id is its own keeps it.
    s.update(A::Rename);
    let out = s.update(A::Named("MAGMA".into()));
    let [C::Rename { to, .. }] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!(to.id, "magma");
    s.update(A::Renamed { from: "magma".into(), to: to.clone() });

    s.update(A::Delete);
    assert_eq!(s.dialog(), Some(&Dialog::Delete));
    assert_eq!(s.update(A::ConfirmDelete), vec![C::Delete("magma".into())]);
    s.update(A::Deleted("magma".into()));
    assert!(s.doc().is_none());
    assert!(s.library().iter().all(|e| e.builtin));
}

#[test]
fn a_recovered_material_is_unsaved_until_saved() {
    let mut m = material("lava-rock", "Lava rock", 640);
    let mut s = MaterialEditorState::recovered(library(), m.clone());
    assert!(s.dirty());
    assert!(s.status().contains("Recovered"), "{}", s.status());
    assert_eq!(s.update(A::Save), vec![C::Save(m.clone())]);
    // One whose file is gone: listed apart, Save writes it again.
    m.id = "gone".into();
    let mut s = MaterialEditorState::recovered(library(), m.clone());
    assert!(s.dirty() && s.unsaved_file());
    s.update(A::Rename);
    assert_eq!(s.dialog(), None);
    assert_eq!(s.update(A::Save), vec![C::Save(m.clone())]);
    s.update(A::Saved(m.clone()));
    assert!(!s.dirty() && !s.unsaved_file());
    assert!(s.library().iter().any(|e| !e.builtin && e.material.id == "gone"));
    // Don't save closes one never written.
    let mut s = MaterialEditorState::recovered(library(), m.clone());
    s.update(A::Select(Key::builtin("stone")));
    s.update(A::Choice(Choice::DontSave));
    assert_eq!(s.selected(), Some(&Key::builtin("stone")));
    // A hand-made recovery out of bounds refuses to save, never panics.
    let bad = MaterialSource { name: String::new(), ..material("lava-rock", "x", 5) };
    let mut s = MaterialEditorState::recovered(library(), bad);
    assert!(!s.problems().is_empty());
    assert!(s.update(A::Save).is_empty());
    assert!(s.status().starts_with("Not saved"), "{}", s.status());
}

#[test]
fn ids_are_free_portable_and_bounded() {
    assert_eq!(free_id("Lava Rock!", |_| false), "lava-rock");
    assert_eq!(free_id("!!!", |_| false), "material");
    assert_eq!(free_id("con", |_| false), "con-2");
    assert_eq!(free_id("stone", |c| c == "stone" || c == "stone-2"), "stone-3");
    let long = free_id(&"a".repeat(100), |c| c == "a".repeat(64));
    assert_eq!(long, format!("{}-2", "a".repeat(62)));
    assert!(olr_format::materials::valid_material_id(&long));
    // Everything taken: no id, never an endless search.
    assert_eq!(free_id("stone", |_| true), "");
}
