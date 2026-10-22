//! The dialogs: a name (New, Copy, Rename; the file it gets shown), Delete,
//! unsaved changes, and the host's errors. Escape cancels.

use egui::Context;

use super::{MaterialEditorData, labels};
use crate::state::{Choice, Dialog, MaterialAction, MaterialEditorState, Next, Purpose};

pub(super) fn show(
    ctx: &Context,
    state: &MaterialEditorState,
    data: &mut MaterialEditorData,
    out: &mut Vec<MaterialAction>,
) {
    let Some(d) = state.dialog() else {
        data.name = None;
        return;
    };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        out.push(MaterialAction::Choice(Choice::Cancel));
        return;
    }
    let name = state.doc().map(|m| m.name.clone()).unwrap_or_default();
    let modal = egui::Modal::new(egui::Id::new("olr-material-editor-dialog"));
    modal.show(ctx, |ui| {
        ui.set_width(400.0);
        match d {
            Dialog::Name(p) => {
                let (heading, ok) = match p {
                    Purpose::New => ("A new material".to_string(), labels::CREATE),
                    Purpose::Copy => (format!("A copy of {name}"), labels::CREATE),
                    Purpose::Rename => (format!("Rename {name}"), labels::RENAME_OK),
                };
                ui.heading(heading);
                ui.label("It is kept in your materials; levels take copies of the materials they use.");
                let text = data.name.get_or_insert_with(|| state.suggested_name(*p));
                let entered = ui
                    .horizontal(|ui| {
                        let l = ui.label(labels::MATERIAL_NAME);
                        let r =
                            ui.add(egui::TextEdit::singleline(text).desired_width(240.0)).labelled_by(l.id);
                        r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    })
                    .inner;
                let trimmed = text.trim();
                let fine = (1..=olr_format::materials::MAX_MATERIAL_NAME).contains(&trimmed.chars().count());
                let keep = (*p == Purpose::Rename).then(|| state.selected().map(|k| k.id.as_str())).flatten();
                if fine {
                    ui.weak(format!("File: {}.ron", state.id_for(trimmed, keep)));
                } else {
                    ui.weak(format!("1 to {} characters", olr_format::materials::MAX_MATERIAL_NAME));
                }
                ui.horizontal(|ui| {
                    if (ui.add_enabled(fine, egui::Button::new(ok)).clicked() || entered && fine)
                        && let Some(n) = data.name.take()
                    {
                        out.push(MaterialAction::Named(n));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        out.push(MaterialAction::Choice(Choice::Cancel));
                    }
                });
            }
            Dialog::Delete => {
                ui.heading(format!("Delete {name}?"));
                ui.label("Its file is removed from your materials. Levels keep their own copies.");
                if state.dirty() {
                    ui.label("Its unsaved changes go with it.");
                }
                ui.horizontal(|ui| {
                    if ui.button(labels::DELETE_OK).clicked() {
                        out.push(MaterialAction::ConfirmDelete);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        out.push(MaterialAction::Choice(Choice::Cancel));
                    }
                });
            }
            Dialog::Unsaved(next) => {
                ui.heading("Unsaved changes");
                let then = match next {
                    Next::Select(_) => "opening another material",
                    Next::Create(_) => "opening the new one",
                    Next::Close => "closing",
                };
                ui.label(format!("Save the changes to {name} before {then}?"));
                ui.horizontal(|ui| {
                    if ui.button(labels::SAVE_CHANGES).clicked() {
                        out.push(MaterialAction::Choice(Choice::Save));
                    }
                    if ui.button(labels::DONT_SAVE).clicked() {
                        out.push(MaterialAction::Choice(Choice::DontSave));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        out.push(MaterialAction::Choice(Choice::Cancel));
                    }
                });
            }
            Dialog::Error(why) => {
                ui.heading("Something went wrong");
                ui.label(why);
                if ui.button(labels::OK).clicked() {
                    out.push(MaterialAction::Choice(Choice::Cancel));
                }
            }
        }
    });
}
