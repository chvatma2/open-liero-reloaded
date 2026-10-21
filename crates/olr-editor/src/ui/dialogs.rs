//! A centred modal per `Dialog`.

use egui::Context;

use super::labels;
use crate::state::{Choice, Dialog, EditorAction, EditorState, Pending};

pub(super) fn show(
    ctx: &Context,
    state: &EditorState,
    data: &mut super::EditorData,
    out: &mut Vec<EditorAction>,
) {
    let Some(dialog) = state.dialog() else {
        data.asset_name = None;
        return;
    };
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
            Dialog::RemoveGroup { id } => {
                ui.label(format!("Remove glow group {id:?}? Its elements stop glowing."));
                ui.horizontal(|ui| {
                    if ui.button(labels::REMOVE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::AssetName { suggested } => {
                let first = data.asset_name.is_none();
                let name = data.asset_name.get_or_insert_with(|| suggested.clone());
                ui.label("Save as asset: its name");
                let l = ui.label(labels::ASSET_NAME);
                let r = ui.add(egui::TextEdit::singleline(name).desired_width(280.0)).labelled_by(l.id);
                if first {
                    r.request_focus();
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    if ui.button(labels::SAVE).clicked() || enter {
                        out.push(EditorAction::NameAsset(name.clone()));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::AssetExists { name, .. } => {
                ui.label(format!("You have an asset named {name:?} already."));
                ui.horizontal(|ui| {
                    if ui.button(labels::REPLACE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::KEEP_BOTH).clicked() {
                        choose(out, Choice::KeepBoth);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::RemoveAsset { name, .. } => {
                ui.label(format!("Remove the asset {name:?} from your assets?"));
                ui.horizontal(|ui| {
                    if ui.button(labels::REMOVE).clicked() {
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
    if modal.should_close()
        && !out.iter().any(|a| matches!(a, EditorAction::Choose(_) | EditorAction::NameAsset(_)))
    {
        out.push(EditorAction::Choose(Choice::Cancel));
    }
}
