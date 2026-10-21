//! The dialogs: unsaved changes on Close, a fire's kind changed, Add
//! weapon, the weapons' order note and the host's errors. Escape cancels.

use egui::Context;

use super::{Cx, caption, labels, relabel};
use crate::state::{Choice, Dialog, ModAction};

pub(super) fn show(ctx: &Context, cx: &mut Cx) {
    let state = cx.state;
    let Some(d) = state.dialog() else { return };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cx.out.push(ModAction::Choice(Choice::Cancel));
        return;
    }
    let modal = egui::Modal::new(egui::Id::new("olr-mod-editor-dialog"));
    modal.show(ctx, |ui| {
        ui.set_width(420.0);
        match d {
            Dialog::Unsaved => {
                ui.heading("Unsaved changes");
                ui.label(format!("Save the changes to {} before closing?", state.pack().manifest.name));
                ui.horizontal(|ui| {
                    if ui.button(labels::SAVE_AND_CLOSE).clicked() {
                        cx.out.push(ModAction::Choice(Choice::Save));
                    }
                    if ui.button(labels::DONT_SAVE).clicked() {
                        cx.out.push(ModAction::Choice(Choice::DontSave));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        cx.out.push(ModAction::Choice(Choice::Cancel));
                    }
                });
            }
            Dialog::FireKind { weapon, kind } => {
                ui.heading("Change the fire");
                let name = state.src().weapons.iter().find(|(w, _)| w == weapon).map_or("", |(_, w)| w.name.as_str());
                ui.label(format!(
                    "{name} fires {kind} from now on: its fire's settings start again from {kind}'s defaults. \
                     Undo brings them back."
                ));
                ui.horizontal(|ui| {
                    if ui.button(labels::CHANGE).clicked() {
                        cx.out.push(ModAction::Confirm(true));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        cx.out.push(ModAction::Confirm(false));
                    }
                });
            }
            Dialog::AddWeapon { from, weapon } => {
                ui.heading("Add a weapon");
                ui.label("It is added at the end of the list.");
                let from_name = from.as_ref().map(|id| {
                    state.mods().iter().find(|(m, _)| m == id).map_or(id.clone(), |(_, n)| n.clone())
                });
                ui.horizontal(|ui| {
                    caption(ui, labels::COPY_FROM);
                    let r = egui::ComboBox::from_id_salt("olr-mod-editor-add-from")
                        .selected_text(from_name.clone().unwrap_or_else(|| labels::BLANK.into()))
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(from.is_none(), labels::BLANK).clicked() {
                                cx.out.push(ModAction::AddFrom(None));
                            }
                            let own = (state.pack().manifest.id.clone(), format!("{} (this mod)", state.pack().manifest.name));
                            for (id, name) in std::iter::once(&own).chain(state.mods().iter().filter(|(m, _)| *m != own.0)) {
                                if ui.selectable_label(from.as_ref() == Some(id), name).clicked() {
                                    cx.out.push(ModAction::AddFrom(Some(id.clone())));
                                }
                            }
                        })
                        .response;
                    relabel(&r, egui::WidgetType::ComboBox, labels::COPY_FROM);
                });
                if let Some(m) = from {
                    match state.other(m) {
                        None => {
                            ui.weak("Loading…");
                        }
                        Some(src) => {
                            let shown = weapon
                                .as_ref()
                                .and_then(|w| src.weapons.iter().find(|(i, _)| i == w))
                                .map_or("(choose)".to_string(), |(_, w)| w.name.clone());
                            ui.horizontal(|ui| {
                                caption(ui, labels::WEAPON_TO_COPY);
                                let r = egui::ComboBox::from_id_salt("olr-mod-editor-add-weapon")
                                    .selected_text(shown)
                                    .width(240.0)
                                    .height(400.0)
                                    .show_ui(ui, |ui| {
                                        for (id, w) in &src.weapons {
                                            let r = ui.selectable_label(weapon.as_ref() == Some(id), &w.name);
                                            relabel(&r, egui::WidgetType::Button, &labels::copy(&w.name));
                                            if r.clicked() {
                                                cx.out.push(ModAction::AddPick(id.clone()));
                                            }
                                        }
                                    })
                                    .response;
                                relabel(&r, egui::WidgetType::ComboBox, labels::WEAPON_TO_COPY);
                            });
                            ui.weak("Its projectiles, sounds and images are copied with it.");
                        }
                    }
                }
                ui.horizontal(|ui| {
                    let ok = from.is_none() || weapon.is_some();
                    if ui.add_enabled(ok, egui::Button::new(labels::ADD)).clicked() {
                        cx.out.push(ModAction::AddConfirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        cx.out.push(ModAction::Confirm(false));
                    }
                });
            }
            Dialog::Note(text) => {
                ui.heading("Good to know");
                ui.label(text);
                if ui.button(labels::OK).clicked() {
                    cx.out.push(ModAction::Confirm(true));
                }
            }
            Dialog::Error(why) => {
                ui.heading("Something went wrong");
                ui.label(why);
                if ui.button(labels::OK).clicked() {
                    cx.out.push(ModAction::Choice(Choice::Cancel));
                }
            }
        }
    });
}
