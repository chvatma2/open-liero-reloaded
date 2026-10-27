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
        data.new_asset = None;
        data.expose_colour = None;
        return;
    };
    let modal = egui::Modal::new(egui::Id::new("olr-editor-dialog")).show(ctx, |ui| {
        let choose = |out: &mut Vec<EditorAction>, c| out.push(EditorAction::Choose(c));
        match dialog {
            Dialog::Unsaved { then } => {
                // Issue #77: naming the level or the asset.
                let name = |d: Option<&crate::document::Document>| d.map_or(String::new(), |d| d.level().name.clone());
                let (of, before) = match then {
                    Pending::Open(_) => (format!("the level {:?}", name(state.document())), "opening another level"),
                    Pending::New(_) => (format!("the level {:?}", name(state.document())), "making a new level"),
                    Pending::Close => (format!("the level {:?}", name(state.document())), "closing"),
                    Pending::EditAsset { .. } => {
                        (format!("the asset {:?}", name(state.asset_document())), "opening another asset")
                    }
                    Pending::NewAsset(_) => {
                        (format!("the asset {:?}", name(state.asset_document())), "making a new asset")
                    }
                    Pending::CloseAsset => (format!("the asset {:?}", name(state.asset_document())), "closing"),
                };
                ui.label(format!("Save the changes to {of} before {before}?"));
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
            Dialog::RemoveState { name, .. } => {
                ui.label(format!("Remove {name} and its own pictures?"));
                ui.horizontal(|ui| {
                    if ui.button(labels::REMOVE).clicked() {
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
            Dialog::NewAsset(start) => {
                let n = data.new_asset.get_or_insert_with(|| start.clone());
                ui.label("New asset: its name, size and art scale");
                let l = ui.label(labels::ASSET_NAME);
                ui.add(egui::TextEdit::singleline(&mut n.name).desired_width(280.0)).labelled_by(l.id);
                let side = 1..=olr_format::levels::MAX_SIDE;
                for (label, v) in [(labels::WIDTH, &mut n.size.0), (labels::HEIGHT, &mut n.size.1)] {
                    ui.horizontal(|ui| {
                        let l = ui.label(label);
                        ui.add(egui::DragValue::new(v).range(side.clone())).labelled_by(l.id);
                    });
                }
                ui.horizontal(|ui| {
                    let l = ui.label(labels::ART_SCALE);
                    ui.add(egui::DragValue::new(&mut n.art_scale).range(1..=olr_format::levels::MAX_ART_SCALE))
                        .labelled_by(l.id);
                });
                ui.horizontal(|ui| {
                    if ui.button(labels::CREATE).clicked() {
                        out.push(EditorAction::MakeAsset(n.clone()));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::Unexpose { name } => {
                ui.label(format!("Unexpose {name}? Copies' values and scripts using {name} will be errors."));
                ui.horizontal(|ui| {
                    if ui.button(labels::UNEXPOSE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::ExposeColour { property, label } => {
                let first = data.expose_colour.is_none();
                let text = data.expose_colour.get_or_insert_with(|| "#ffffff".into());
                ui.label(format!(
                    "{label}'s glow group takes each pixel's own colour: expose it with a default colour"
                ));
                let l = ui.label(labels::DEFAULT_COLOUR);
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(120.0)).labelled_by(l.id);
                if first {
                    r.request_focus();
                }
                let colour = super::look::parse_hex(text);
                ui.horizontal(|ui| {
                    let expose = ui.add_enabled(colour.is_some(), egui::Button::new(labels::EXPOSE));
                    if let Some(c) = colour
                        && expose.on_disabled_hover_text("a colour is #rrggbb").clicked()
                    {
                        out.push(EditorAction::Expose {
                            property: property.clone(),
                            default: Some(olr_format::assets::ValueSource::Colour(c)),
                        });
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::RemoveScript => {
                ui.label("Remove the asset's script?");
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
        && !out.iter().any(|a| {
            matches!(
                a,
                EditorAction::Choose(_)
                    | EditorAction::NameAsset(_)
                    | EditorAction::MakeAsset(_)
                    | EditorAction::Expose { .. }
            )
        })
    {
        out.push(EditorAction::Choose(Choice::Cancel));
    }
}
