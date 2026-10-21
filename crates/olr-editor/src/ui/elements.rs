//! The right panel's tabs (issue #8): Elements (the element list, top
//! first: drag a row to reorder, eye and lock toggles, add a layer),
//! Assets (the asset library's place, to come) and Spawns (the level's
//! objects). The tab follows the selection (`EditorState::tab`).

use egui::Ui;
use olr_edit::element::Property;
use olr_format::levels::{ElementKind, LevelObject};

use super::labels;
use crate::state::{EditorAction, EditorState, PanelTab, Selection};

/// The lists' share of the panel's height: the properties take the rest.
const LIST_SHARE: f32 = 0.4;

pub(super) fn show(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    ui.horizontal(|ui| {
        for (tab, label) in [
            (PanelTab::Elements, labels::ELEMENTS),
            (PanelTab::Assets, labels::ASSETS),
            (PanelTab::Spawns, labels::SPAWNS),
        ] {
            if ui.selectable_label(state.tab() == tab, label).clicked() && state.tab() != tab {
                out.push(EditorAction::Tab(tab));
            }
        }
    });
    ui.separator();
    match state.tab() {
        PanelTab::Elements => elements(ui, state, out),
        PanelTab::Assets => {
            ui.weak("Saved assets will appear here.");
        }
        PanelTab::Spawns => spawns(ui, state, out),
    }
}

fn elements(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    egui::ScrollArea::vertical()
        .id_salt("olr-editor-element-list")
        .max_height(ui.available_height() * LIST_SHARE)
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
                    let selected = state.selection().has(&e.id);
                    // The name selects on a click and reorders on a drag.
                    let r =
                        ui.add(egui::Button::selectable(selected, name).sense(egui::Sense::click_and_drag()));
                    r.dnd_set_drag_payload(e.id.clone());
                    // Ctrl toggles it in a group, Shift selects a range (plan 14e R8).
                    if r.clicked() {
                        let m = ui.input(|i| i.modifiers);
                        out.push(if m.command {
                            EditorAction::SelectToggle(e.id.clone())
                        } else if m.shift {
                            EditorAction::SelectRange(e.id.clone())
                        } else {
                            EditorAction::Select(Selection::Element(e.id.clone()))
                        });
                    }
                });
                if let Some(dragged) = row.response.dnd_release_payload::<String>()
                    && *dragged != e.id
                {
                    out.push(EditorAction::MoveElement { id: (*dragged).clone(), to: index });
                }
            }
        });
    if ui.button(labels::ADD_LAYER).on_hover_text("an empty layer to paint on").clicked() {
        out.push(EditorAction::AddLayer);
    }
}

fn spawns(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    if doc.level().objects.is_empty() {
        ui.weak("No spawns yet: place them with the Spawn tool (P).");
    }
    egui::ScrollArea::vertical()
        .id_salt("olr-editor-spawn-list")
        .max_height(ui.available_height() * LIST_SHARE)
        .show(ui, |ui| spawn_rows(ui, state, out));
}

fn spawn_rows(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    for (i, o) in doc.level().objects.iter().enumerate() {
        let at = o.at();
        let kind = match o {
            LevelObject::Spawn { .. } => "Spawn",
        };
        let selected = *state.selection() == Selection::Object(i);
        if ui.selectable_label(selected, format!("{kind} {} at ({}, {})", i + 1, at.0, at.1)).clicked() {
            out.push(EditorAction::Select(Selection::Object(i)));
        }
    }
}
