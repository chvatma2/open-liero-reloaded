//! The dialogs: unsaved changes on Close, a fire's kind changed, Add
//! weapon, the weapons' order note and the host's errors; issue #19's New
//! image, Rename, the frame-count question and the import browser.
//! Escape cancels.

use egui::Context;

use super::{Cx, caption, labels, relabel};
use crate::state::{BrowseAction, Choice, Dialog, ModAction};

pub(super) fn show(ctx: &Context, cx: &mut Cx) {
    let state = cx.state;
    let Some(d) = state.dialog() else { return };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cx.out.push(ModAction::Choice(Choice::Cancel));
        return;
    }
    if *d == Dialog::Browse {
        browser(ctx, cx);
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
            Dialog::NewImage { copy_of } => new_image(ui, cx, copy_of.clone()),
            Dialog::Rename(path) => rename(ui, cx, path),
            Dialog::FrameCount { image, before, after } => {
                ui.heading("The frame count changes");
                ui.label(format!(
                    "{image} has {before} frame{} and will have {after}. An image's pixels are only its look, \
                     but its frame count reaches the game: an animation, frames following the heading and a \
                     rotation step through them, so the weapon may play differently. Play is the way to check.",
                    if *before == 1 { "" } else { "s" }
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
            Dialog::Browse => {}
        }
    });
}

/// New image: its name, and a blank one's frame side and count, or a copy.
fn new_image(ui: &mut egui::Ui, cx: &mut Cx, copy_of: Option<String>) {
    ui.heading(match &copy_of {
        Some(c) => format!("A copy of {c}"),
        None => "A new image".to_string(),
    });
    let d = &mut cx.data.new_image;
    ui.horizontal(|ui| {
        caption(ui, labels::IMAGE_NAME);
        let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(200.0).hint_text("rocket"));
        relabel(&r, egui::WidgetType::TextEdit, labels::IMAGE_NAME);
        ui.weak("in gfx/, .png");
    });
    if copy_of.is_none() {
        ui.horizontal(|ui| {
            caption(ui, labels::FRAME_SIDE);
            let r = ui.add(egui::DragValue::new(&mut d.side).range(1..=256));
            relabel(&r, egui::WidgetType::DragValue, labels::FRAME_SIDE);
            caption(ui, labels::FRAME_COUNT);
            let r = ui.add(egui::DragValue::new(&mut d.frames).range(1..=64));
            relabel(&r, egui::WidgetType::DragValue, labels::FRAME_COUNT);
        });
        ui.weak("Frames are square, side by side: a projectile following its heading uses one a direction.");
    }
    ui.horizontal(|ui| {
        let ok = !d.name.trim().is_empty();
        if ui.add_enabled(ok, egui::Button::new(labels::MAKE)).clicked() {
            cx.out.push(ModAction::NewImage {
                name: d.name.trim().to_string(),
                side: d.side.max(1),
                frames: d.frames.max(1),
                copy_of: copy_of.clone(),
            });
        }
        if ui.button(labels::CANCEL).clicked() {
            cx.out.push(ModAction::Confirm(false));
        }
    });
}

/// Rename an image or sound: its new name in the same folder.
fn rename(ui: &mut egui::Ui, cx: &mut Cx, path: &str) {
    ui.heading(format!("Rename {path}"));
    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
    let text = match &mut cx.data.rename {
        Some((p, t)) if p == path => t,
        slot => &mut slot.insert((path.to_string(), file.to_string())).1,
    };
    ui.horizontal(|ui| {
        caption(ui, labels::NEW_NAME);
        let r = ui.add(egui::TextEdit::singleline(text).desired_width(220.0));
        relabel(&r, egui::WidgetType::TextEdit, labels::NEW_NAME);
        ui.weak(format!("in {folder}/"));
    });
    ui.weak("Every field naming it is renamed with it.");
    let to = text.trim().to_string();
    ui.horizontal(|ui| {
        if ui.add_enabled(!to.is_empty(), egui::Button::new(labels::RENAME)).clicked() {
            cx.out.push(ModAction::RenameAsset { from: path.to_string(), to: format!("{folder}/{to}") });
        }
        if ui.button(labels::CANCEL).clicked() {
            cx.out.push(ModAction::Confirm(false));
        }
    });
}

/// The import browser (the level editor's, `olr-editor`'s `ui/browser.rs`):
/// the path (Enter browses), Up, the entries (a click selects, a double
/// click picks), Choose and Cancel; Escape or the backdrop closes it.
fn browser(ctx: &Context, cx: &mut Cx) {
    let Some(b) = cx.state.browser() else { return };
    let dir = b.dir().to_path_buf();
    if cx.data.browse_path.as_ref().is_none_or(|(d, _)| *d != dir) {
        cx.data.browse_path = Some((dir.clone(), dir.display().to_string()));
    }
    let mut answered = false;
    let modal = egui::Modal::new(egui::Id::new("olr-mod-editor-browser")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading(b.import().title());
        ui.horizontal(|ui| {
            caption(ui, labels::BROWSER_PATH);
            if let Some((_, text)) = &mut cx.data.browse_path {
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(380.0));
                relabel(&r, egui::WidgetType::TextEdit, labels::BROWSER_PATH);
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    cx.out.push(ModAction::Browse(BrowseAction::To(text.trim().into())));
                }
            }
            if ui.add_enabled(dir.parent().is_some(), egui::Button::new(labels::UP)).clicked() {
                cx.out.push(ModAction::Browse(BrowseAction::Up));
            }
        });
        if let Some(m) = b.message() {
            ui.colored_label(ui.visuals().warn_fg_color, m);
        }
        ui.separator();
        egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, false]).show(ui, |ui| {
            let shown = b.shown();
            if shown.is_empty() {
                ui.weak("Nothing to choose here.");
            }
            for e in shown {
                let text = if e.is_dir { format!("📁 {}", e.name) } else { e.name.clone() };
                let selected = b.selected() == Some(e.name.as_str());
                let r = ui.push_id((&dir, &e.name), |ui| ui.selectable_label(selected, text)).inner;
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &e.name)
                });
                if r.double_clicked() {
                    cx.out.push(ModAction::Browse(BrowseAction::Pick(e.name.clone())));
                    answered = true;
                } else if r.clicked() {
                    cx.out.push(ModAction::Browse(BrowseAction::Select(e.name.clone())));
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(b.selected().is_some(), egui::Button::new(labels::CHOOSE)).clicked() {
                cx.out.push(ModAction::Browse(BrowseAction::Choose));
                answered = true;
            }
            if ui.button(labels::CANCEL).clicked() {
                cx.out.push(ModAction::Confirm(false));
                answered = true;
            }
        });
    });
    if modal.should_close() && !answered {
        cx.out.push(ModAction::Confirm(false));
    }
}
