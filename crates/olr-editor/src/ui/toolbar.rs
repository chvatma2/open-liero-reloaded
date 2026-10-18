//! The toolbar: the tools, the brush, the paint, undo and redo, open,
//! save and close, play, and the view toggles.

use egui::Ui;

use super::{EditorData, labels, number};
use crate::browser::Purpose;
use crate::state::{EditorAction, EditorState, MAX_BOTS};
use crate::tools::{Brush, BrushShape, MAX_BRUSH, PaintChoice, Tool};
use olr_format::levels::ElementMaterial;

/// The mod picker's width (points): a long name is cut, never wider.
const MOD_WIDTH: f32 = 160.0;
/// The space the unsaved mark keeps, shown or not: nothing moves when it
/// comes and goes.
const MARK_WIDTH: f32 = 110.0;
/// The space a tool's options keep at the tools' row's end, whichever
/// tool is chosen (plan 14d R13).
const OPTIONS_WIDTH: f32 = 220.0;
/// "Paint as"'s picker width.
const PAINT_AS_WIDTH: f32 = 110.0;
/// "Paint as": the layer's own material, or one of the three.
const PAINT_AS: [(Option<ElementMaterial>, &str); 4] = [
    (None, "Layer's"),
    (Some(ElementMaterial::Rock), "Rock"),
    (Some(ElementMaterial::Dirt), "Dirt"),
    (Some(ElementMaterial::Background), "Background"),
];

/// Three rows (the playtest: one wrapped row moved its controls as they
/// changed width, and pushed the mod picker past the window's edge; plan
/// 14d's tool options did not fit a drawing row): the tools and the chosen
/// tool's options, the brush and paint, then the files, history, Play and
/// the view; each control's place is fixed by what precedes it alone.
pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    // Every control as tall as a button, so a row's labels, fields and
    // pickers share its buttons' centre line (the host's theme makes
    // buttons taller than labels: the playtest's crooked Play, Mod, Bots).
    let h = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
    ui.spacing_mut().interact_size.y = ui.spacing().interact_size.y.max(h);
    ui.horizontal_wrapped(|ui| tools(ui, state, data, out));
    ui.horizontal_wrapped(|ui| drawing(ui, state, data, out));
    ui.horizontal_wrapped(|ui| session(ui, state, data, out));
}

fn tools(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    for tool in Tool::ALL {
        let r = ui.selectable_label(state.tool() == tool, tool.name()).on_hover_text(tool.key().to_string());
        if r.clicked() {
            out.push(EditorAction::Tool(tool));
        }
    }
    ui.separator();
    // The tool's options, in a space of their own whatever the tool.
    let size = egui::vec2(OPTIONS_WIDTH, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_size(size);
        options(ui, state, data, out);
    });
}

fn drawing(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
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

/// The selected drawn layer's own material, when one is selected.
fn selected_material(state: &EditorState) -> Option<ElementMaterial> {
    let crate::state::Selection::Element(id) = state.selection() else { return None };
    let doc = state.document()?;
    match &doc.items()[doc.index_of(id)?].element.kind {
        olr_format::levels::ElementKind::Drawn { material, .. } => Some(*material),
        _ => None,
    }
}

/// "Paint as"'s name for `m`: "Layer's" says the selected layer's material
/// (a mixed layer's is hidden behind "Mixed" in its properties).
fn paint_as_name(m: Option<ElementMaterial>, layer: Option<ElementMaterial>) -> String {
    let name = |m: ElementMaterial| PAINT_AS.iter().find(|(p, _)| *p == Some(m)).map_or("", |(_, n)| n);
    match (m, layer) {
        (Some(m), _) => name(m).into(),
        (None, Some(l)) => format!("Layer's ({})", name(l)),
        (None, None) => "Layer's".into(),
    }
}

/// The chosen tool's options: "Paint as" for the painting tools, "Restore"
/// for the eraser, "Tolerance" for Remove colour.
fn options(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    match state.tool() {
        Tool::Brush | Tool::Rect | Tool::Ellipse | Tool::Line => {
            let current = state.paint_material();
            let layer = selected_material(state);
            let name = paint_as_name(current, layer);
            let label = ui.label(labels::PAINT_AS);
            egui::ComboBox::from_id_salt("olr-editor-paint-as")
                .selected_text(name)
                .width(PAINT_AS_WIDTH)
                .show_ui(ui, |ui| {
                    for (m, _) in PAINT_AS {
                        let n = paint_as_name(m, layer);
                        if ui.selectable_label(m == current, n).clicked() && m != current {
                            out.push(EditorAction::PaintMaterial(m));
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        }
        Tool::Eraser => {
            let mut restore = state.restore();
            if ui
                .checkbox(&mut restore, labels::RESTORE)
                .on_hover_text("bring back a picture's erased pixels")
                .changed()
            {
                out.push(EditorAction::Restore(restore));
            }
        }
        Tool::Wand => {
            if let Some(t) =
                number(ui, data, "tolerance", labels::TOLERANCE, state.tolerance().into(), 0..=100)
            {
                out.push(EditorAction::Tolerance(t as u8));
            }
        }
        _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 14d minors: "Layer's" names the layer's own material.
    #[test]
    fn paint_as_names_the_layers_material() {
        assert_eq!(paint_as_name(None, Some(ElementMaterial::Background)), "Layer's (Background)");
        assert_eq!(paint_as_name(None, None), "Layer's");
        assert_eq!(paint_as_name(Some(ElementMaterial::Rock), Some(ElementMaterial::Dirt)), "Rock");
    }
}
