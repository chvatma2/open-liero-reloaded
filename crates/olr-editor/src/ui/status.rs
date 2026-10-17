//! The status bar: the problems (errors first), the last message, the
//! cursor's world position and material.

use egui::Ui;
use olr_data::level::Material;

use crate::document::Problem;
use crate::state::EditorState;
use olr_pack::Severity;

pub(super) fn show(ui: &mut Ui, state: &EditorState) {
    let status = state.status();
    ui.horizontal_wrapped(|ui| {
        for Problem { severity, text } in &status.problems {
            let colour = match severity {
                Severity::Error => ui.visuals().error_fg_color,
                Severity::Warning => ui.visuals().warn_fg_color,
            };
            ui.colored_label(colour, text);
        }
        if let Some(m) = &status.message {
            ui.label(m);
        }
        if state.playing() {
            ui.label("playing");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some((x, y)) = status.cursor {
                let material = match status.material {
                    Some(Material::Rock) => "rock",
                    Some(Material::Dirt) => "dirt",
                    Some(Material::Background) => "background",
                    None => "outside",
                };
                ui.label(format!("{x}, {y} · {material}"));
            }
        });
    });
}
