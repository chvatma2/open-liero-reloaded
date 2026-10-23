//! The material editor's panels driven by label (`egui_kittest`, issue
//! #44): a material created, edited and saved; Edit a copy of a built-in
//! one; a field's bounds; Delete; a recovered material saved, and Close
//! asking first. The test host keeps the user's library in memory and
//! answers at once, as the game's does from the disk.

use std::collections::BTreeMap;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_format::materials::{MaterialSource, material_text, read_material};
use olr_material_editor::ui::{self, MaterialEditorData, labels};
use olr_material_editor::{Key, MaterialAction as A, MaterialCommand as C, MaterialEditorState};

use crate::common::{library, material};

/// The editor, what it asked the host to do, and the user's library.
pub(crate) struct App {
    pub(crate) state: MaterialEditorState,
    pub(crate) data: MaterialEditorData,
    pub(crate) commands: Vec<C>,
    /// The user's files by id: their text, as the host writes them.
    files: BTreeMap<String, String>,
}

pub(crate) fn harness(state: MaterialEditorState) -> Harness<'static, App> {
    let files = state
        .library()
        .iter()
        .filter(|e| !e.builtin)
        .map(|e| (e.material.id.clone(), material_text(&e.material).unwrap()))
        .collect();
    let app = App { state, data: MaterialEditorData::default(), commands: Vec::new(), files };
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut App| {
            let actions = ui::show(ui, &app.state, &mut app.data);
            if !actions.is_empty() {
                ui.ctx().request_repaint();
            }
            let mut queue: std::collections::VecDeque<A> = actions.into();
            while let Some(a) = queue.pop_front() {
                for c in app.state.update(a) {
                    let answer = match &c {
                        C::Create(m) if app.files.contains_key(&m.id) => Some(A::Failed("taken".into())),
                        C::Create(m) => {
                            app.files.insert(m.id.clone(), material_text(m).unwrap());
                            Some(A::Created(m.clone()))
                        }
                        C::Save(m) => {
                            app.files.insert(m.id.clone(), material_text(m).unwrap());
                            Some(A::Saved(m.clone()))
                        }
                        C::Rename { from, to } => {
                            app.files.remove(from);
                            app.files.insert(to.id.clone(), material_text(to).unwrap());
                            Some(A::Renamed { from: from.clone(), to: to.clone() })
                        }
                        C::Delete(id) => {
                            app.files.remove(id);
                            Some(A::Deleted(id.clone()))
                        }
                        C::Close | C::Preview(_) => None,
                    };
                    queue.extend(answer);
                    app.commands.push(c);
                }
            }
        },
        app,
    );
    h.run();
    h
}

pub(crate) fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

/// Types `text` into the field named `name`, and Enter.
fn type_into(h: &mut Harness<'static, App>, name: &str, text: &str) {
    h.get_by_label(name).click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(name).type_text(text);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
}

/// The user's file `id`, read back as the game reads it.
fn file(h: &Harness<'static, App>, id: &str) -> MaterialSource {
    let text = h.state().files.get(id).unwrap_or_else(|| panic!("no file {id}"));
    read_material(&format!("{id}.ron"), text).unwrap()
}

fn doc(h: &Harness<'static, App>) -> MaterialSource {
    h.state().state.doc().unwrap().clone()
}

/// Names a new material in the name dialog.
fn name_it(h: &mut Harness<'static, App>, name: &str) {
    h.get_by_label(labels::MATERIAL_NAME).click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(labels::MATERIAL_NAME).type_text(name);
    h.run();
    click(h, labels::CREATE);
}

#[test]
fn create_edit_and_save_a_material() {
    let mut h = harness(MaterialEditorState::new(library()));
    click(&mut h, labels::NEW);
    name_it(&mut h, "Basalt");
    assert_eq!(h.state().state.selected(), Some(&Key::user("basalt")));
    // Written at once, stone's heat.
    assert_eq!(file(&h, "basalt").heat.unwrap().capacity, 100);
    assert!(h.query_by_label(labels::UNSAVED).is_none());

    type_into(&mut h, labels::CAPACITY, "250");
    type_into(&mut h, labels::COOLING, "40");
    type_into(&mut h, labels::NOTES, "Dark and dense");
    // The notes are multi-line: Enter is a new line; a click elsewhere ends it.
    click(&mut h, labels::CAPACITY);
    click(&mut h, labels::OWN_RAMP);
    type_into(&mut h, &labels::key_colour(0), "#102030");
    type_into(&mut h, &labels::key_glow(1), "55");
    click(&mut h, labels::ADD_KEY);
    click(&mut h, &labels::remove_key(6));
    assert!(h.query_by_label(labels::UNSAVED).is_some());
    let edited = doc(&h);
    let heat = edited.heat.as_ref().unwrap();
    assert_eq!((heat.capacity, heat.cooling), (250, 40));
    assert!(edited.notes.starts_with("Dark and dense"), "{:?}", edited.notes);
    assert_eq!(heat.look.ramp.len(), olr_look::LAVA_RAMP.len());
    // The key added went between 650 and 850; the hottest went.
    assert_eq!(heat.look.ramp[0].colour.to_hex(), "#102030");
    assert_eq!(heat.look.ramp[1].glow, 55);
    assert_eq!(heat.look.ramp[4].temperature, 750);
    assert_eq!(heat.look.ramp[5].temperature, 850);

    click(&mut h, labels::SAVE);
    assert!(h.query_by_label(labels::UNSAVED).is_none());
    assert_eq!(file(&h, "basalt"), edited);
    // The library lists it after the built-in ones, in name order.
    let users: Vec<&str> =
        h.state().state.library().iter().filter(|e| !e.builtin).map(|e| e.material.name.as_str()).collect();
    assert_eq!(users, ["Basalt", "Lava rock"]);
    // Undo, then Ctrl+S saves the step back.
    click(&mut h, labels::UNDO);
    assert!(h.query_by_label(labels::UNSAVED).is_some());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    h.run();
    assert_eq!(file(&h, "basalt").heat.unwrap().look.ramp.len(), olr_look::LAVA_RAMP.len() + 1);
}

#[test]
fn edit_a_copy_of_a_built_in_material() {
    let mut h = harness(MaterialEditorState::new(library()));
    click(&mut h, "Wood");
    assert!(h.state().state.read_only());
    // Its fields show but never change.
    assert!(h.get_by_label(labels::CAPACITY).accesskit_node().is_disabled());
    assert!(h.get_by_label(labels::SAVE).accesskit_node().is_disabled());
    assert!(h.get_by_label(labels::DELETE).accesskit_node().is_disabled());
    type_into(&mut h, labels::CAPACITY, "999");
    assert_eq!(doc(&h).heat.unwrap().capacity, 50);

    click(&mut h, labels::EDIT_A_COPY);
    click(&mut h, labels::CREATE);
    assert_eq!(h.state().state.selected(), Some(&Key::user("wood-copy")));
    assert_eq!(doc(&h).name, "Wood copy");
    type_into(&mut h, labels::CAPACITY, "70");
    click(&mut h, labels::SAVE);
    assert_eq!(file(&h, "wood-copy").heat.unwrap().capacity, 70);
    // The built-in one is as it was; nothing was written for it.
    let wood = h.state().state.library().iter().find(|e| e.builtin && e.material.id == "wood").unwrap();
    assert_eq!(wood.material.heat.as_ref().unwrap().capacity, 50);
    assert!(!h.state().files.contains_key("wood"));
    assert!(h.state().commands.iter().all(|c| match c {
        C::Save(m) | C::Create(m) => m.id != "wood",
        _ => true,
    }));
}

/// Issue #57: the light soak set through the fields, saved, and undone.
#[test]
fn light_soak_is_set_saved_and_undone() {
    let mut h = harness(MaterialEditorState::new(library()));
    assert_eq!(doc(&h).light_soak, None, "the level's");
    click(&mut h, labels::LIGHT_SOAK);
    click(&mut h, &labels::soak_px(2));
    assert_eq!(doc(&h).light_soak, Some(2));
    assert!(h.query_by_label(labels::UNSAVED).is_some());
    click(&mut h, labels::SAVE);
    assert_eq!(file(&h, "lava-rock").light_soak, Some(2));
    assert!(h.state().files["lava-rock"].contains("light_soak: 2,"));
    click(&mut h, labels::LIGHT_SOAK);
    click(&mut h, labels::LIGHT_SOAK_LEVEL);
    assert_eq!(doc(&h).light_soak, None);
    click(&mut h, labels::UNDO);
    assert_eq!(doc(&h).light_soak, Some(2), "undo restores it");
    click(&mut h, labels::UNDO);
    assert_eq!(doc(&h).light_soak, None);
    click(&mut h, labels::REDO);
    assert_eq!(doc(&h).light_soak, Some(2));
    // Its bounds: 0 to 12, whatever is asked.
    for (asked, got) in [(99, olr_look::MAX_SOAK), (-3, 0)] {
        h.state_mut().state.update(A::Set(olr_material_editor::Edit::LightSoak(Some(asked))));
        assert_eq!(doc(&h).light_soak, Some(got));
    }
}

#[test]
fn a_field_keeps_to_its_bounds() {
    let mut h = harness(MaterialEditorState::new(library()));
    type_into(&mut h, labels::CAPACITY, "5000");
    assert_eq!(doc(&h).heat.unwrap().capacity, olr_look::MAX_CAPACITY);
    type_into(&mut h, labels::CAPACITY, "1");
    assert_eq!(doc(&h).heat.unwrap().capacity, olr_look::MIN_CAPACITY);
    type_into(&mut h, labels::SENSITIVITY, "-4");
    assert_eq!(doc(&h).heat.unwrap().sensitivity, 0);
    type_into(&mut h, labels::LIGHT, "250");
    assert_eq!(doc(&h).heat.unwrap().look.light, 100);
    // Not a number: the field shows the value again.
    type_into(&mut h, labels::COOLING, "warm");
    assert_eq!(doc(&h).heat.unwrap().cooling, 100);
    // A key's temperature stays between its neighbours'.
    click(&mut h, labels::OWN_RAMP);
    type_into(&mut h, &labels::key_temperature(0), "900");
    assert_eq!(doc(&h).heat.unwrap().look.ramp[0].temperature, olr_look::LAVA_RAMP[1].temperature - 1);
    // What was typed saves: it is all within bounds.
    click(&mut h, labels::SAVE);
    assert_eq!(file(&h, "lava-rock").heat.unwrap().capacity, olr_look::MIN_CAPACITY);
    // Off, no heat field is shown.
    click(&mut h, labels::HEATS);
    assert!(h.query_by_label(labels::CAPACITY).is_none());
    assert!(doc(&h).heat.is_none());
}

#[test]
fn delete_a_material() {
    let mut h = harness(MaterialEditorState::new(library()));
    click(&mut h, labels::DELETE);
    click(&mut h, labels::CANCEL);
    assert!(h.state().files.contains_key("lava-rock"));
    click(&mut h, labels::DELETE);
    click(&mut h, labels::DELETE_OK);
    assert!(!h.state().files.contains_key("lava-rock"));
    assert!(h.query_by_label("Lava rock").is_none());
    assert!(h.state().state.doc().is_none());
    // Rename too is of the user's files.
    click(&mut h, "Stone");
    click(&mut h, labels::COPY);
    name_it(&mut h, "Granite");
    click(&mut h, labels::RENAME);
    h.get_by_label(labels::MATERIAL_NAME).click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(labels::MATERIAL_NAME).type_text("Pink granite");
    h.run();
    click(&mut h, labels::RENAME_OK);
    assert!(!h.state().files.contains_key("granite"));
    assert_eq!(file(&h, "pink-granite").name, "Pink granite");
}

#[test]
fn recovered_changes_are_saved_or_asked_about() {
    let recovered = material("lava-rock", "Lava rock", 640);
    let mut h = harness(MaterialEditorState::recovered(library(), recovered));
    assert!(h.query_by_label(labels::UNSAVED).is_some());
    click(&mut h, labels::SAVE);
    assert_eq!(file(&h, "lava-rock").heat.unwrap().capacity, 640);
    assert!(h.query_by_label(labels::UNSAVED).is_none());

    // Close with changes asks; Cancel stays, Don't save closes.
    type_into(&mut h, labels::CAPACITY, "700");
    click(&mut h, labels::CLOSE);
    click(&mut h, labels::CANCEL);
    assert!(!h.state().commands.contains(&C::Close));
    click(&mut h, labels::CLOSE);
    click(&mut h, labels::DONT_SAVE);
    assert!(h.state().commands.contains(&C::Close));
    assert_eq!(file(&h, "lava-rock").heat.unwrap().capacity, 640);

    // One whose file is gone is listed as not saved, and Save writes it.
    let gone = material("gone", "Gone", 300);
    let mut h = harness(MaterialEditorState::recovered(library(), gone));
    assert!(h.query_by_label("Gone (not saved)").is_some());
    click(&mut h, labels::SAVE);
    assert_eq!(file(&h, "gone").heat.unwrap().capacity, 300);
    assert!(h.query_by_label("Gone (not saved)").is_none());
    assert!(h.query_by_label("Gone").is_some());
    // The preview's place is kept for the live previews.
    assert!(h.query_by_label(labels::PREVIEW).is_some());
    assert!(h.query_by_label(labels::RAMP).is_some());
}
