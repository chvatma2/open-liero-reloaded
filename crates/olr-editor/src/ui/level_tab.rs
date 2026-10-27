//! The level tab's layout (issue #83, the asset tab's of issue #78 and
//! the skin editor's, issue #55's sizing rules): the session bar on top,
//! wrapped to the window (the Level | Asset switch, File ▾, Undo, Redo, the
//! unsaved mark, View ▾, Play and its ▾); on the left the tools (Select,
//! Spawn, the Haze brush), the chosen tool's options under them and the
//! asset library (`library.rs`); on the right the copy panel (Copies |
//! Spawns, then the selection's panel or the level's); the status line;
//! the canvas in the middle, or the open script (issue #84,
//! `level_script.rs`). Both side panels take a share of the
//! window's width; below [`NARROW`](super::NARROW) one shows at a time (the
//! session bar's Library | Copy). They scroll vertically only and follow
//! a focus that moved otherwise than by the pointer.

use egui::Ui;

use super::asset::{self, Side};
use super::{
    EditorData, canvas, elements, labels, level, level_script, library, properties, rail, status, toolbar,
};
use crate::state::{EditorAction, EditorState, Selection};

/// Draws the level tab; its actions go to `out`: the bars' and the tools'
/// (the library's too), the list's, the fields', the canvas's.
pub(super) fn show(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: [&mut Vec<EditorAction>; 4],
) {
    let [toolbar, list, fields, canvas] = out;
    asset::see_focus(ui.ctx(), data);
    let follow = data.follow > 0;
    data.follow = data.follow.saturating_sub(1);
    if follow {
        ui.ctx().request_repaint();
    }
    let width = ui.available_rect_before_wrap().width();
    let (narrow, left_w, right_w) = asset::side_widths(width);
    egui::Panel::top("olr-editor-session").show(ui, |ui| session(ui, state, data, narrow, toolbar));
    egui::Panel::bottom("olr-editor-status").show(ui, |ui| status::show(ui, state, toolbar));
    if !narrow || data.side == Side::Tools {
        egui::Panel::left("olr-editor-level-library").exact_size(left_w).resizable(false).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("olr-editor-level-library").show(ui, |ui| {
                rail::grid(ui, state, toolbar);
                ui.separator();
                toolbar::options_panel(ui, state, data, toolbar);
                ui.separator();
                library::show(ui, state, data, toolbar);
                asset::follow_focus(ui, follow);
            });
        });
    }
    if !narrow || data.side == Side::Layers {
        egui::Panel::right("olr-editor-level-copy").exact_size(right_w).resizable(false).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("olr-editor-level-copy").show(ui, |ui| {
                elements::show(ui, state, list);
                ui.separator();
                match state.selection() {
                    Selection::None => level::show(ui, state, data, fields),
                    _ => properties::show(ui, state, data, fields),
                }
                asset::follow_focus(ui, follow);
            });
        });
    }
    level_script::sync(ui, state, data, fields);
    // No fill: the host draws the bake under the canvas (plan 14c).
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        if state.script_open().is_some() {
            level_script::show(ui, state, data, fields);
        } else {
            canvas::show(ui, state, data, canvas);
        }
    });
    asset::see_focus(ui.ctx(), data);
}

/// The level tab's session bar, wrapped to the window: below `NARROW` the
/// Library | Copy switch, then the Level | Asset switch, File ▾, Undo,
/// Redo, the unsaved mark, View ▾, Play and its ▾.
fn session(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    narrow: bool,
    out: &mut Vec<EditorAction>,
) {
    toolbar::button_height(ui);
    ui.horizontal_wrapped(|ui| {
        if narrow {
            for (side, label) in [(Side::Tools, labels::SHOW_LIBRARY), (Side::Layers, labels::SHOW_COPY)] {
                if ui.add(egui::Button::selectable(data.side == side, label)).clicked() {
                    data.side = side;
                }
            }
            ui.separator();
        }
        toolbar::tab_switch(ui, state, out);
        ui.separator();
        toolbar::file_menu(ui, state, out);
        toolbar::undo_redo(ui, state, out);
        // Named for AccessKit (a font may lack "●").
        if state.active_dirty() {
            let mark = ui.weak("unsaved");
            super::relabel(&mark, egui::WidgetType::Label, None, labels::UNSAVED);
            mark.on_hover_text(labels::UNSAVED);
        }
        ui.separator();
        toolbar::view_menu(ui, state, data, out);
        toolbar::play(ui, state, data, out);
    });
}
