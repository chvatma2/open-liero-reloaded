//! A centred modal per `Dialog`.

use egui::Context;

use super::labels;
use crate::state::{Choice, Dialog, EditorAction, EditorState, Pending};

pub(super) fn show(ctx: &Context, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(dialog) = state.dialog() else { return };
    let modal = egui::Modal::new(egui::Id::new("olr-editor-dialog")).show(ctx, |ui| {
        let choose = |out: &mut Vec<EditorAction>, c| out.push(EditorAction::Choose(c));
        match dialog {
            Dialog::Unsaved { then } => {
                let before = match then {
                    Pending::Open(_) => "opening another level",
                    Pending::New(_) => "making a new level",
                    Pending::Close => "closing",
                };
                ui.label(format!("Save the changes before {before}?"));
                ui.horizontal(|ui| {
                    if ui.button(labels::SAVE).clicked() {
                        choose(out, Choice::Save);
                    }
                    if ui.button(labels::DONT_SAVE).clicked() {
                        choose(out, Choice::Discard);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::ArtScale { to } => {
                ui.label(format!(
                    "Art scale {to} rescales the drawn and raster layers (nearest neighbour) and reprocesses the images."
                ));
                ui.horizontal(|ui| {
                    if ui.button("Change").clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::Error(e) => {
                ui.label(e);
                if ui.button("OK").clicked() {
                    choose(out, Choice::Cancel);
                }
            }
        }
    });
    // Escape or a click on the backdrop cancels, unless a button answered.
    if modal.should_close() && !out.iter().any(|a| matches!(a, EditorAction::Choose(_))) {
        out.push(EditorAction::Choose(Choice::Cancel));
    }
}
