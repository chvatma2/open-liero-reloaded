//! The dialogs: the first save's name, unsaved changes on Close, and the
//! host's errors. Escape cancels.

use egui::Context;

use super::{SkinEditorData, labels};
use crate::state::{Choice, Dialog, SkinAction, SkinEditorState};

pub(super) fn show(
    ctx: &Context,
    state: &SkinEditorState,
    data: &mut SkinEditorData,
    out: &mut Vec<SkinAction>,
) {
    let Some(d) = state.dialog() else {
        data.save_name = None;
        return;
    };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        out.push(SkinAction::Choice(Choice::Cancel));
        return;
    }
    let modal = egui::Modal::new(egui::Id::new("olr-skin-editor-dialog"));
    modal.show(ctx, |ui| {
        ui.set_width(380.0);
        match d {
            Dialog::Name => {
                ui.heading("Save the skin");
                ui.label("It is saved with your packs: the profiles' skin picker offers it at once.");
                let name = data.save_name.get_or_insert_with(|| state.doc().name().to_string());
                let entered = ui
                    .horizontal(|ui| {
                        let l = ui.label(labels::SKIN_NAME);
                        let r =
                            ui.add(egui::TextEdit::singleline(name).desired_width(240.0)).labelled_by(l.id);
                        r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    })
                    .inner;
                let ok = !name.trim().is_empty();
                ui.horizontal(|ui| {
                    if (ui.add_enabled(ok, egui::Button::new(labels::SAVE_SKIN)).clicked() || entered && ok)
                        && let Some(n) = data.save_name.take()
                    {
                        out.push(SkinAction::SaveAs(n));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        out.push(SkinAction::Choice(Choice::Cancel));
                    }
                });
            }
            Dialog::Unsaved => {
                ui.heading("Unsaved changes");
                ui.label(format!("Save the changes to {} before closing?", state.doc().name()));
                ui.horizontal(|ui| {
                    if ui.button(labels::SAVE_AND_CLOSE).clicked() {
                        out.push(SkinAction::Choice(Choice::Save));
                    }
                    if ui.button(labels::DONT_SAVE).clicked() {
                        out.push(SkinAction::Choice(Choice::DontSave));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        out.push(SkinAction::Choice(Choice::Cancel));
                    }
                });
            }
            Dialog::Error(why) => {
                ui.heading("Something went wrong");
                ui.label(why);
                if ui.button(labels::OK).clicked() {
                    out.push(SkinAction::Choice(Choice::Cancel));
                }
            }
        }
    });
}
