//! The left panel: what is painted (Colour, Player colour, Glow: one
//! switch), the tools and the brush size, the paint colour with the
//! recent ones or the glow strength, the onion skin, and the frame
//! operations (copy, paste to the frame, its row or its column, flip,
//! shift by a pixel).

use egui::{RichText, Ui};
use olr_data::image::Rgba;

use super::{SkinEditorData, colour32, hex, labels, parse_hex, relabel};
use crate::paint::Target;
use crate::state::{GLOW_STRENGTHS, Onion, PasteTo, SkinAction, SkinEditorState, Tool};

pub(super) fn show(
    ui: &mut Ui,
    state: &SkinEditorState,
    data: &mut SkinEditorData,
    out: &mut Vec<SkinAction>,
) {
    ui.strong("Paint");
    ui.horizontal_wrapped(|ui| {
        for t in Target::ALL {
            if ui.selectable_label(state.target() == t, t.name()).clicked() {
                out.push(SkinAction::Target(t));
            }
        }
    });
    ui.weak(match state.target() {
        Target::Colour => "The skin's pixels.",
        Target::Tint => "Pixels shown in the player's colour (magenta here).",
        Target::Glow => "Pixels that glow in the dark (yellow here).",
    });
    ui.separator();
    ui.strong("Tools");
    egui::Grid::new("olr-skin-editor-tool-grid").num_columns(2).show(ui, |ui| {
        for (i, tool) in Tool::ALL.into_iter().enumerate() {
            let r = ui.selectable_label(state.tool() == tool, tool.name()).on_hover_text(format!(
                "{} ({})",
                tool.name(),
                tool.key()
            ));
            if r.clicked() {
                out.push(SkinAction::Tool(tool));
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    ui.add_enabled_ui(state.tool().sized(), |ui| {
        ui.horizontal(|ui| {
            for (size, label) in [(1, labels::SIZE_1), (2, labels::SIZE_2)] {
                if ui.selectable_label(state.size() == size, label).clicked() {
                    out.push(SkinAction::Size(size));
                }
            }
        });
    });
    ui.separator();
    match state.target() {
        Target::Glow => glow_strength(ui, state, out),
        _ => colour(ui, state, data, out),
    }
    ui.separator();
    ui.horizontal(|ui| {
        let l = ui.label(labels::ONION);
        egui::ComboBox::from_id_salt("olr-skin-editor-onion")
            .selected_text(state.onion().name())
            .show_ui(ui, |ui| {
                for o in Onion::ALL {
                    if ui.selectable_label(state.onion() == o, o.name()).clicked() {
                        out.push(SkinAction::Onion(o));
                    }
                }
            })
            .response
            .labelled_by(l.id);
    });
    ui.separator();
    ui.strong("Frame");
    ui.horizontal_wrapped(|ui| {
        if ui.button(labels::COPY_FRAME).clicked() {
            out.push(SkinAction::CopyFrame);
        }
        let can = state.clipboard().is_some();
        for (label, to) in [
            (labels::PASTE, PasteTo::Frame),
            (labels::PASTE_ROW, PasteTo::Row),
            (labels::PASTE_COLUMN, PasteTo::Column),
        ] {
            if ui.add_enabled(can, egui::Button::new(label)).clicked() {
                out.push(SkinAction::Paste(to));
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button(labels::FLIP).on_hover_text("Mirror left to right").clicked() {
            out.push(SkinAction::Flip { vertical: false });
        }
        if ui.button(labels::FLIP_VERTICAL).clicked() {
            out.push(SkinAction::Flip { vertical: true });
        }
    });
    ui.horizontal(|ui| {
        ui.label("Shift");
        for (text, label, dx, dy) in [
            ("←", labels::SHIFT_LEFT, -1, 0),
            ("→", labels::SHIFT_RIGHT, 1, 0),
            ("↑", labels::SHIFT_UP, 0, -1),
            ("↓", labels::SHIFT_DOWN, 0, 1),
        ] {
            let r = ui.button(text).on_hover_text(label);
            relabel(&r, egui::WidgetType::Button, label);
            if r.clicked() {
                out.push(SkinAction::Shift { dx, dy });
            }
        }
    });
    let mut all = state.shift_all();
    if ui.checkbox(&mut all, labels::SHIFT_ALL).changed() {
        out.push(SkinAction::ShiftAll(all));
    }
}

/// The paint colour: a picker, its `#rrggbb` and the recent colours.
fn colour(ui: &mut Ui, state: &SkinEditorState, data: &mut SkinEditorData, out: &mut Vec<SkinAction>) {
    ui.horizontal(|ui| {
        let c = state.colour();
        let mut c32 = colour32(c);
        let r = egui::widgets::color_picker::color_edit_button_srgba(
            ui,
            &mut c32,
            egui::widgets::color_picker::Alpha::Opaque,
        );
        relabel(&r, egui::WidgetType::ColorButton, labels::PAINT_COLOUR);
        if r.changed() {
            out.push(SkinAction::Colour(Rgba::rgb(c32.r(), c32.g(), c32.b())));
        }
        let l = ui.label(labels::HEX);
        let mut text = data.hex.clone().unwrap_or_else(|| hex(c));
        let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0)).labelled_by(l.id);
        if field.has_focus() {
            data.hex = Some(text);
        } else if data.hex.take().is_some()
            && let Some(c) = parse_hex(&text)
        {
            out.push(SkinAction::Colour(c));
        }
    });
    if state.target() == Target::Colour && !state.recent().is_empty() {
        ui.weak("Recent");
        ui.horizontal_wrapped(|ui| {
            for &c in state.recent() {
                let (rect, r) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 2.0, colour32(c));
                if c == state.colour() {
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                        egui::StrokeKind::Outside,
                    );
                }
                relabel(&r, egui::WidgetType::Button, &hex(c));
                if r.on_hover_text(hex(c)).clicked() {
                    out.push(SkinAction::Colour(c));
                }
            }
        });
    }
}

/// The glow painted: a quarter to full, as the game draws it.
fn glow_strength(ui: &mut Ui, state: &SkinEditorState, out: &mut Vec<SkinAction>) {
    ui.label(labels::GLOW_STRENGTH);
    ui.horizontal(|ui| {
        for g in GLOW_STRENGTHS {
            let text = format!("{} %", (u32::from(g) * 100 + 127) / 255);
            if ui.selectable_label(state.glow_strength() == g, RichText::new(text)).clicked() {
                out.push(SkinAction::GlowStrength(g));
            }
        }
    });
}
