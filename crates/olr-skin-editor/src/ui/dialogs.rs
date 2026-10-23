//! The dialogs: the first save's name, unsaved changes on Close, and the
//! host's errors. Escape cancels. Never wider than the window (issue
//! #55).

use egui::Context;

use super::{SkinEditorData, beside, labels};
use crate::state::{Choice, Dialog, SkinAction, SkinEditorState};

/// The dialogs' width (points) in a window wide enough.
const WIDTH: f32 = 380.0;
/// The room (points) a dialog leaves the window's edges in a narrow one.
const EDGES: f32 = 48.0;
/// The name field's narrowest beside its label.
const FIELD_MIN: f32 = 120.0;

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
    let width = WIDTH.min(ctx.content_rect().width() - EDGES).max(0.0);
    modal.show(ctx, |ui| {
        ui.set_width(width);
        match d {
            Dialog::Name => {
                ui.heading("Save the skin");
                ui.label("It is saved with your packs: the profiles' skin picker offers it at once.");
                let name = data.save_name.get_or_insert_with(|| state.doc().name().to_string());
                let entered = beside(ui, labels::SKIN_NAME, FIELD_MIN, |ui, l, room| {
                    let r = ui
                        .add(egui::TextEdit::singleline(name).desired_width(room.min(240.0)))
                        .labelled_by(l);
                    r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                });
                let ok = !name.trim().is_empty();
                ui.horizontal_wrapped(|ui| {
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
                ui.horizontal_wrapped(|ui| {
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
