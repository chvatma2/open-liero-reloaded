//! The element list, top first: drag a row to reorder, eye and lock
//! toggles, add a layer; the objects below.

use egui::Ui;
use olr_edit::element::Property;
use olr_format::levels::{ElementKind, LevelObject};

use super::labels;
use crate::state::{EditorAction, EditorState, Selection};

pub(super) fn show(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    ui.heading("Elements");
    egui::ScrollArea::vertical()
        .id_salt("olr-editor-element-list")
        .max_height(ui.available_height() * 0.6)
        .show(ui, |ui| {
            for (index, item) in doc.items().iter().enumerate().rev() {
                let e = &item.element;
                let row = ui.horizontal(|ui| {
                    let mut visible = e.visible;
                    if ui.toggle_value(&mut visible, "👁").on_hover_text(labels::VISIBLE).clicked() {
                        out.push(EditorAction::Set(e.id.clone(), Property::Visible(visible)));
                    }
                    let mut locked = e.locked;
                    if ui.toggle_value(&mut locked, "🔒").on_hover_text(labels::LOCKED).clicked() {
                        out.push(EditorAction::Set(e.id.clone(), Property::Locked(locked)));
                    }
                    let kind = match e.kind {
                        ElementKind::Image { .. } => "🖼",
                        ElementKind::Drawn { .. } => "✏",
                        ElementKind::Raster => "▦",
                    };
                    ui.label(kind);
                    let name = if e.name.is_empty() { e.id.as_str() } else { e.name.as_str() };
                    let selected = matches!(state.selection(), Selection::Element(s) if *s == e.id);
                    // The name selects on a click and reorders on a drag.
                    let r =
                        ui.add(egui::Button::selectable(selected, name).sense(egui::Sense::click_and_drag()));
                    r.dnd_set_drag_payload(e.id.clone());
                    if r.clicked() {
                        out.push(EditorAction::Select(Selection::Element(e.id.clone())));
                    }
                });
                if let Some(dragged) = row.response.dnd_release_payload::<String>()
                    && *dragged != e.id
                {
                    out.push(EditorAction::MoveElement { id: (*dragged).clone(), to: index });
                }
            }
        });
    ui.horizontal(|ui| {
        if ui.button(labels::ADD_LAYER).clicked() {
            out.push(EditorAction::AddLayer);
        }
        ui.add_enabled(false, egui::Button::new(labels::IMPORT_IMAGE))
            .on_disabled_hover_text("the file browser comes with the game's editor");
    });
    ui.separator();
    ui.heading(labels::OBJECTS);
    for (i, o) in doc.level().objects.iter().enumerate() {
        let LevelObject::Spawn { at, .. } = o;
        let selected = *state.selection() == Selection::Object(i);
        if ui.selectable_label(selected, format!("Spawn {} at ({}, {})", i + 1, at.0, at.1)).clicked() {
            out.push(EditorAction::Select(Selection::Object(i)));
        }
    }
}
