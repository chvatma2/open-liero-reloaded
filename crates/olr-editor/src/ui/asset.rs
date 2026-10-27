//! The asset tab's layout (issue #78; the asset-based levels design's "The
//! asset tab"), the skin editor's (`olr-skin-editor`'s `ui/mod.rs`): the
//! session bar on top (the Level | Asset switch, File ▾, Undo, Redo, the
//! asset's name and its unsaved mark, View ▾), the tools on the left (the
//! rail's buttons, then the chosen tool's options), the layers on the
//! right (room for issue #79's states, the Layers tree, the selection's
//! properties or the asset's, the previews), the status line, and the
//! canvas in the middle. It fits any window (issue #55's rules): the side
//! panels take a share of its width, and below [`NARROW`] one shows at a
//! time (the session bar's Tools | Layers). The side panels scroll
//! vertically only, and follow a focus that moved otherwise than by the
//! pointer.

use egui::Ui;

use super::{EditorData, canvas, elements, labels, level, previews, properties, rail, status, toolbar};
use crate::state::{EditorAction, EditorState, Selection};

/// Below this width (points) one side panel shows at a time.
pub const NARROW: f32 = 900.0;
/// The tools panel: its share of the window's width and its widths
/// (points, its frame's margins in): the skin editor's.
const TOOLS_SHARE: f32 = 0.2;
const TOOLS_MIN: f32 = 200.0;
const TOOLS_MAX: f32 = 220.0;
/// The layers panel: its share and its narrowest, the skin editor's
/// frames panel's; at most `LAYERS_MAX`.
const LAYERS_SHARE: f32 = 0.37;
const LAYERS_MIN: f32 = 240.0;
const LAYERS_MAX: f32 = 400.0;
/// A narrow window's side panel's share (235 points at 427).
const NARROW_SHARE: f32 = 0.55;
/// The passes the side panels' scroll areas follow a focus that moved
/// otherwise than by the pointer.
const FOLLOW_PASSES: u8 = 8;

/// The side panel a narrow window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Side {
    #[default]
    Tools,
    Layers,
}

/// Draws the asset tab; its actions go to `out`: the bars' and the tools',
/// the list's, the fields', the canvas's.
pub(super) fn show(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: [&mut Vec<EditorAction>; 4],
) {
    let [toolbar, list, fields, canvas] = out;
    see_focus(ui.ctx(), data);
    let follow = data.follow > 0;
    data.follow = data.follow.saturating_sub(1);
    if follow {
        ui.ctx().request_repaint();
    }
    let width = ui.available_rect_before_wrap().width();
    let narrow = width < NARROW;
    let share = |s: f32, min: f32, max: f32| (width * s).clamp(min, max.max(min)).min(width);
    let (tools_w, layers_w) = if narrow {
        (share(NARROW_SHARE, TOOLS_MIN, width), share(NARROW_SHARE, LAYERS_MIN, width))
    } else {
        (share(TOOLS_SHARE, TOOLS_MIN, TOOLS_MAX), share(LAYERS_SHARE, LAYERS_MIN, LAYERS_MAX))
    };
    egui::Panel::top("olr-editor-session").show(ui, |ui| session(ui, state, data, narrow, toolbar));
    egui::Panel::bottom("olr-editor-status").show(ui, |ui| status::show(ui, state));
    if !narrow || data.side == Side::Tools {
        egui::Panel::left("olr-editor-asset-tools").exact_size(tools_w).resizable(false).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("olr-editor-asset-tools").show(ui, |ui| {
                rail::grid(ui, state, toolbar);
                ui.separator();
                toolbar::options_panel(ui, state, data, toolbar);
                follow_focus(ui, follow);
            });
        });
    }
    if !narrow || data.side == Side::Layers {
        egui::Panel::right("olr-editor-asset-layers").exact_size(layers_w).resizable(false).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("olr-editor-asset-layers").show(ui, |ui| {
                // Issue #79's states panel comes first, above the layers.
                ui.strong(labels::LAYERS);
                elements::tree(ui, state, list);
                ui.separator();
                match state.selection() {
                    Selection::None => level::asset(ui, state, data, fields),
                    _ => properties::show(ui, state, data, fields),
                }
                ui.separator();
                previews::show(ui, state, data);
                follow_focus(ui, follow);
            });
        });
    }
    // No fill: the host draws the bake under the canvas (plan 14c).
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| canvas::show(ui, state, data, canvas));
    see_focus(ui.ctx(), data);
}

/// The asset tab's session bar, wrapped to the window: below [`NARROW`] the
/// Tools | Layers switch, then the Level | Asset switch, File ▾ (Save,
/// Close), Undo, Redo, the asset's name and its unsaved mark, View ▾.
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
            for (side, label) in [(Side::Tools, labels::TOOLS_TAB), (Side::Layers, labels::LAYERS_TAB)] {
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
        ui.separator();
        let name = state.active_document().map(|d| d.meta().name).unwrap_or_default();
        let text = format!("Asset: {name}");
        // A name too long for the row's rest starts the next row.
        if super::text_width(ui, &text, egui::TextStyle::Body) > ui.available_width() {
            ui.end_row();
        }
        ui.add(egui::Label::new(egui::RichText::new(text).strong()).truncate());
        // The level tab's mark, named for AccessKit (a font may lack "●").
        if state.active_dirty() {
            let mark = ui.weak("unsaved");
            super::relabel(&mark, egui::WidgetType::Label, None, labels::UNSAVED);
            mark.on_hover_text(labels::UNSAVED);
        }
        toolbar::view_menu(ui, state, data, out);
    });
}

/// Notes a focus that moved otherwise than by the pointer (Tab, a screen
/// reader): the side panels' scroll areas follow it for the next passes
/// (the skin editor's `see_focus`).
fn see_focus(ctx: &egui::Context, data: &mut EditorData) {
    let focused = ctx.memory(|m| m.focused());
    if focused == data.seen_focus {
        return;
    }
    data.seen_focus = focused;
    let pointer = ctx.input(|i| i.pointer.any_pressed() || i.pointer.any_released() || i.pointer.any_down());
    if focused.is_some() && !pointer {
        data.follow = FOLLOW_PASSES;
    }
}

/// Ends a side panel's scroll area's content (`ui`): while `follow`,
/// scrolls the focused widget into view when it is in this area and not
/// all shown (egui scrolls to no focused widget by itself; the skin
/// editor's `follow_focus`, `olr_ui::pad`'s).
fn follow_focus(ui: &Ui, follow: bool) {
    if !follow {
        return;
    }
    let ctx = ui.ctx();
    let Some(r) = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)) else { return };
    if r.layer_id != ui.layer_id() || !ui.min_rect().contains(r.rect.center()) {
        return;
    }
    if ui.clip_rect().contains_rect(r.rect) {
        return;
    }
    // Where it was when this area last scrolled to it: it did not move.
    let last = ui.id().with("olr-editor follow last");
    if ctx.data(|d| d.get_temp::<(egui::Id, egui::Rect)>(last)) == Some((r.id, r.rect)) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(last, (r.id, r.rect)));
    ui.scroll_to_rect(r.rect, None);
}
