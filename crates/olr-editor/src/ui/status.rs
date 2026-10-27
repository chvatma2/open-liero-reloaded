//! The status bar: the problems (errors first), the last message, issue
//! #83's Use your copy in this level, the cursor's world position and
//! material.

use egui::Ui;
use olr_data::level::Material;

use super::labels;
use crate::document::Problem;
use crate::state::{EditorAction, EditorState};
use olr_pack::Severity;

pub(super) fn show(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
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
        } else if state.doc_tab() == crate::state::DocTab::Asset
            && state.asset_key().is_some_and(|k| k.builtin)
        {
            // Issue #77: a built-in asset is saved as the user's.
            ui.label(crate::asset_tab::BUILT_IN_NOTE);
        }
        if state.playing() {
            ui.label("playing");
        }
        // Issue #83: a built-in asset saved as yours; the level's copies of
        // it stay built-in unless this is taken.
        if let Some(offer) = state.use_yours()
            && ui
                .button(labels::USE_YOUR_COPY)
                .on_hover_text(format!("the level's copies of the built-in {} use yours", offer.name))
                .clicked()
        {
            out.push(EditorAction::UseYourCopy);
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
