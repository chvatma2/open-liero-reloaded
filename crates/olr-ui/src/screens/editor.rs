//! The Editor screen (plan 14c, spec §6): the level editor's way in. The
//! recoveries first (Recover, Discard), then every level pack, with Edit
//! for the data directory's packs and Edit a copy for the others (bundled
//! content is never edited in place, R5), and New level. The app opens the
//! editor itself (`olr-game`): this crate does not know it.

use egui::{Color32, ComboBox, RichText, ScrollArea, TextEdit, Ui};

use super::{MenuData, form_row, page, section};
use crate::state::UiAction;

/// The page's column.
const WIDTH: f32 = 760.0;
/// The theme a new level takes when there is one of this id.
const DEFAULT_THEME: &str = "olx-theme-dirt";
/// A new level's size by default (OLX's Dirt Level's) and its bounds.
const DEFAULT_SIZE: (u32, u32) = (504, 350);
const MAX_SIDE: u32 = 4096;

/// The forms being filled in.
#[derive(Default)]
pub(super) struct View {
    /// Edit a copy's form: the pack copied and the new pack's name.
    copy: Option<(String, String)>,
    /// New level's form.
    new: Option<NewLevel>,
}

struct NewLevel {
    name: String,
    width: String,
    height: String,
    theme: String,
}

/// A side typed in the form, within `1..=MAX_SIDE`.
fn side(text: &str) -> Option<u32> {
    text.trim().parse().ok().filter(|v| (1..=MAX_SIDE).contains(v))
}

pub(super) fn show(ui: &mut Ui, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let mut ui = page(ui, "Editor", WIDTH, out);
    let ui = &mut ui;
    if let Some(m) = &data.editor_message {
        ui.label(RichText::new(m).color(Color32::from_rgb(255, 110, 90)));
        ui.add_space(6.0);
    }
    if !data.recoveries.is_empty() {
        section(ui, "Unsaved changes", |ui, _| {
            for (id, name, damaged) in &data.recoveries {
                ui.horizontal(|ui| {
                    if *damaged {
                        ui.label(
                            RichText::new(format!("{id}: damaged")).color(Color32::from_rgb(255, 110, 90)),
                        );
                    } else if ui.button(format!("Recover {name}")).clicked() {
                        out.push(UiAction::Recover(id.clone()));
                    }
                    if ui.button("Discard").clicked() {
                        out.push(UiAction::DiscardRecovery(id.clone()));
                    }
                });
            }
        });
        ui.add_space(10.0);
    }
    let themes: Vec<(String, String)> = data
        .catalog
        .sources
        .iter()
        .filter(|p| p.kind == olr_data::manifest::PackKind::Theme)
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    let view = &mut data.editor;
    if let Some(f) = &mut view.new {
        let mut create = false;
        let mut cancel = false;
        section(ui, "New level", |ui, _| {
            form_row(ui, "Name", |ui, label| {
                ui.add(TextEdit::singleline(&mut f.name).desired_width(240.0)).labelled_by(label);
            });
            form_row(ui, "Width", |ui, label| {
                ui.add(TextEdit::singleline(&mut f.width).desired_width(80.0)).labelled_by(label);
            });
            form_row(ui, "Height", |ui, label| {
                ui.add(TextEdit::singleline(&mut f.height).desired_width(80.0)).labelled_by(label);
            });
            form_row(ui, "Theme", |ui, label| {
                let shown = themes.iter().find(|t| t.0 == f.theme).map_or("none", |t| t.1.as_str());
                ComboBox::from_id_salt("new level theme")
                    .selected_text(shown)
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for (id, name) in &themes {
                            ui.selectable_value(&mut f.theme, id.clone(), name);
                        }
                    })
                    .response
                    .labelled_by(label);
            });
            let ok = !f.name.trim().is_empty()
                && side(&f.width).is_some()
                && side(&f.height).is_some()
                && themes.iter().any(|t| t.0 == f.theme);
            form_row(ui, "", |ui, _| {
                create = ui.add_enabled(ok, egui::Button::new("Create")).clicked();
                cancel = ui.button("Cancel").clicked();
                if side(&f.width).is_none() || side(&f.height).is_none() {
                    ui.label(RichText::new(format!("sides of 1 to {MAX_SIDE}")).weak());
                }
            });
        });
        if create && let (Some(w), Some(h)) = (side(&f.width), side(&f.height)) {
            out.push(UiAction::NewLevel {
                name: f.name.trim().to_string(),
                size: (w, h),
                theme: f.theme.clone(),
            });
        }
        if cancel {
            view.new = None;
        }
        ui.add_space(10.0);
    } else if ui.button("New level").clicked() {
        let theme = themes.iter().find(|t| t.0 == DEFAULT_THEME).or(themes.first());
        view.new = Some(NewLevel {
            name: String::new(),
            width: DEFAULT_SIZE.0.to_string(),
            height: DEFAULT_SIZE.1.to_string(),
            theme: theme.map(|t| t.0.clone()).unwrap_or_default(),
        });
        view.copy = None;
    }
    if let Some((from, name)) = &mut view.copy {
        let mut copy = false;
        let mut cancel = false;
        let level = data.catalog.level(from).map_or(from.as_str(), |l| l.name.as_str());
        section(ui, &format!("Copy {level}"), |ui, _| {
            form_row(ui, "Name", |ui, label| {
                ui.add(TextEdit::singleline(name).desired_width(240.0)).labelled_by(label);
            });
            form_row(ui, "", |ui, _| {
                copy = ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Copy")).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if copy {
            out.push(UiAction::CopyLevel { from: from.clone(), name: name.trim().to_string() });
        }
        if cancel {
            view.copy = None;
        }
        ui.add_space(10.0);
    }
    section(ui, "Levels", |ui, _| {
        ui.label(RichText::new("Levels you made are edited in place; the others as a copy.").weak());
        ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("editor levels").num_columns(3).striped(true).spacing([24.0, 4.0]).show(
                ui,
                |ui| {
                    for l in &data.catalog.levels {
                        ui.label(&l.name);
                        ui.label(RichText::new(format!("{} × {}", l.width, l.height)).weak());
                        if data.editable.contains(&l.id) {
                            if ui.button("Edit").clicked() {
                                out.push(UiAction::EditLevel(l.id.clone()));
                            }
                        } else if ui.button("Edit a copy").clicked() {
                            data.editor.copy = Some((l.id.clone(), format!("{} copy", l.name)));
                            data.editor.new = None;
                        }
                        ui.end_row();
                    }
                },
            );
        });
    });
}
