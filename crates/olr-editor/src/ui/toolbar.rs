//! The toolbar: the tools, the brush, the paint, undo and redo, open,
//! save and close, play, and the view toggles.

use egui::Ui;

use super::{EditorData, labels, number};
use crate::browser::Purpose;
use crate::state::{EditorAction, EditorState, MAX_BOTS};
use crate::tools::{Brush, BrushShape, MAX_BRUSH, PaintChoice, Tool};

/// The mod picker's width (points): a long name is cut, never wider.
const MOD_WIDTH: f32 = 160.0;
/// The space the unsaved mark keeps, shown or not: nothing moves when it
/// comes and goes.
const MARK_WIDTH: f32 = 110.0;

/// Two rows (the playtest: one wrapped row moved its controls as they
/// changed width, and pushed the mod picker past the window's edge):
/// drawing (tools, brush, paint), then the files, history, Play and the
/// view; each control's place is fixed by what precedes it alone.
pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    // Every control as tall as a button, so a row's labels, fields and
    // pickers share its buttons' centre line (the host's theme makes
    // buttons taller than labels: the playtest's crooked Play, Mod, Bots).
    let h = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
    ui.spacing_mut().interact_size.y = ui.spacing().interact_size.y.max(h);
    ui.horizontal_wrapped(|ui| drawing(ui, state, data, out));
    ui.horizontal_wrapped(|ui| session(ui, state, data, out));
}

fn drawing(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    for tool in Tool::ALL {
        let r = ui.selectable_label(state.tool() == tool, tool.name()).on_hover_text(tool.key().to_string());
        if r.clicked() {
            out.push(EditorAction::Tool(tool));
        }
    }
    ui.separator();
    let brush = state.brush();
    for (shape, name) in [(BrushShape::Round, "Round"), (BrushShape::Square, "Square")] {
        if ui.selectable_label(brush.shape == shape, name).clicked() {
            out.push(EditorAction::Brush(Brush { shape, ..brush }));
        }
    }
    if let Some(size) = number(ui, data, "brush-size", "Size", brush.size.into(), 1..=MAX_BRUSH.into()) {
        out.push(EditorAction::Brush(Brush { size: size as u32, ..brush }));
    }
    ui.separator();
    let paint = state.paint();
    let mut colour = match paint {
        PaintChoice::Colour(c) => egui::Color32::from_rgb(c.r, c.g, c.b),
        _ => egui::Color32::from_rgb(140, 90, 40),
    };
    if egui::color_picker::color_edit_button_srgba(ui, &mut colour, egui::color_picker::Alpha::Opaque)
        .changed()
        || (ui.selectable_label(matches!(paint, PaintChoice::Colour(_)), "Colour").clicked())
    {
        let rgba = olr_data::image::Rgba::rgb(colour.r(), colour.g(), colour.b());
        out.push(EditorAction::Paint(PaintChoice::Colour(rgba)));
    }
    for (choice, name) in [(PaintChoice::FrontTile, "Front tile"), (PaintChoice::BackTile, "Back tile")] {
        if ui.selectable_label(paint == choice, name).clicked() {
            out.push(EditorAction::Paint(choice));
        }
    }
}

fn session(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    if ui.add_enabled(state.can_undo(), egui::Button::new(labels::UNDO)).on_hover_text("Ctrl+Z").clicked() {
        out.push(EditorAction::Undo);
    }
    if ui.add_enabled(state.can_redo(), egui::Button::new(labels::REDO)).on_hover_text("Ctrl+Y").clicked() {
        out.push(EditorAction::Redo);
    }
    ui.separator();
    if ui.button(labels::OPEN).clicked() {
        out.push(EditorAction::Browse(Purpose::OpenPack));
    }
    if ui.button(labels::SAVE).on_hover_text("Ctrl+S").clicked() {
        out.push(EditorAction::Save);
    }
    if ui.button(labels::CLOSE).clicked() {
        out.push(EditorAction::Close);
    }
    let mark = if state.dirty() { "unsaved changes" } else { "" };
    ui.add_sized(
        [MARK_WIDTH, ui.spacing().interact_size.y],
        egui::Label::new(egui::RichText::new(mark).weak()),
    );
    ui.separator();
    if ui.button(labels::PLAY).clicked() {
        out.push(EditorAction::Play);
    }
    let mods = &state.catalog().mods;
    let current = mods
        .iter()
        .find(|m| m.id == state.settings().play_mod)
        .map_or(state.settings().play_mod.as_str(), |m| m.name.as_str());
    let label = ui.label("Mod");
    egui::ComboBox::from_id_salt("olr-editor-play-mod")
        .selected_text(current)
        .width(MOD_WIDTH)
        .truncate()
        .show_ui(ui, |ui| {
            for m in mods {
                if ui.selectable_label(m.id == state.settings().play_mod, &m.name).clicked() {
                    out.push(EditorAction::PlayMod(m.id.clone()));
                }
            }
        })
        .response
        .labelled_by(label.id);
    if let Some(bots) =
        number(ui, data, "play-bots", "Bots", state.settings().play_bots.into(), 0..=MAX_BOTS.into())
    {
        out.push(EditorAction::PlayBots(bots as u8));
    }
    ui.separator();
    let mut material = state.material_view();
    if ui.checkbox(&mut material, "Material view").changed() {
        out.push(EditorAction::MaterialView(material));
    }
    let mut grid = state.grid();
    if ui.checkbox(&mut grid, "Grid").changed() {
        out.push(EditorAction::Grid(grid));
    }
}
