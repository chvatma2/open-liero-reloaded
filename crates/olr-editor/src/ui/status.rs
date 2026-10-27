//! The status bar: the problems (errors first; issue #84: the first one
//! cut to the line, the rest counted, all of them on hover), the last
//! message, issue #83's Use your copy in this level, the cursor's world
//! position and material.

use egui::Ui;
use olr_data::level::Material;

use super::labels;
use crate::document::Problem;
use crate::state::{EditorAction, EditorState};
use olr_pack::Severity;

pub(super) fn show(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let status = state.status();
    ui.horizontal_wrapped(|ui| {
        problems(ui, &status.problems);
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

/// The first problem, cut to the room, and how many more; each names them
/// all on hover (a level's library breaks may be many: issue #84).
fn problems(ui: &mut Ui, problems: &[Problem]) {
    let Some(first) = problems.first() else { return };
    let colour = |ui: &Ui, p: &Problem| match p.severity {
        Severity::Error => ui.visuals().error_fg_color,
        Severity::Warning => ui.visuals().warn_fg_color,
    };
    let all = |ui: &mut Ui| {
        for p in problems {
            ui.colored_label(colour(ui, p), &p.text);
        }
    };
    let more = problems.len() - 1;
    // Room for the count beside it.
    let room = (ui.available_width() - if more > 0 { 80.0 } else { 0.0 }).max(40.0);
    let text = egui::RichText::new(&first.text).color(colour(ui, first));
    let r = ui.scope(|ui| {
        ui.set_max_width(room);
        ui.add(egui::Label::new(text).truncate())
    });
    r.inner.on_hover_ui(all);
    if more > 0 {
        let r = ui.colored_label(colour(ui, &problems[1]), format!("(+{more} more)"));
        r.on_hover_ui(all);
    }
}
