//! The Editor screen (plan 14c, spec §6): the editors' way in, a Level
//! and a Skin tab (issue #14). Level: the recoveries first (Recover,
//! Discard), then every level pack, with Edit for the data directory's
//! packs and Edit a copy for the others (bundled content is never edited
//! in place, R5), and New level. Skin: New blank skin, then every skin
//! (searchable), Edit for the data directory's skin packs and Edit a copy
//! for the others (a built-in or OLX skin is never changed). Mod (issue
//! #17): the mod recoveries first, then every mod, the user's first, with
//! Edit for theirs and Edit a copy (a new mod, named in a form) for any.
//! Material (issue #44): the material recoveries first, then Open the
//! material editor (its own list: the built-in materials and the user's).
//! The app opens the editors itself (`olr-game`): this crate does not know
//! them.

use egui::{Color32, ComboBox, RichText, ScrollArea, TextEdit, Ui};
use olr_format::levels::MAX_SIDE;

use super::{MenuData, fit, form_row, page, section};
use crate::state::UiAction;

/// The page's column.
const WIDTH: f32 = 760.0;

/// A list's narrowest width for its table: in a narrower one (a large UI
/// scale, issue #49) each row wraps on its own.
const NARROW: f32 = 520.0;

/// "Edit a copy"'s room beside a skin's name.
const EDIT_A_COPY: f32 = 110.0;

fn narrow(ui: &Ui) -> bool {
    ui.available_width() < NARROW
}
/// The theme a new level takes when there is one of this id.
const DEFAULT_THEME: &str = "olx-theme-dirt";
/// A new level's size by default (OLX's Dirt Level's); its sides are at
/// most the format's `MAX_SIDE`.
const DEFAULT_SIZE: (u32, u32) = (504, 350);

/// The tab shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Level,
    Skin,
    Mod,
    Material,
}

/// The tabs' labels.
pub const LEVEL_TAB: &str = "Level";
pub const SKIN_TAB: &str = "Skin";
pub const MOD_TAB: &str = "Mod";
pub const MATERIAL_TAB: &str = "Material";
/// The Material tab's button.
pub const OPEN_MATERIAL_EDITOR: &str = "Open the material editor";
/// The Mod tab's copy form: the new mod's name, and its button.
pub const MOD_NAME: &str = "New mod's name";
pub const CREATE_MOD: &str = "Create mod";
/// The Skin tab's buttons.
pub const NEW_SKIN: &str = "New blank skin";
pub const SEARCH_SKINS: &str = "Search skins";

/// The tab shown and the forms being filled in.
#[derive(Default)]
pub(super) struct View {
    tab: Tab,
    /// The Skin tab's search.
    skin_search: String,
    /// Edit a copy's form: the pack copied and the new pack's name.
    copy: Option<(String, String)>,
    /// New level's form.
    new: Option<NewLevel>,
    /// The Mod tab's copy form: the mod copied and the new mod's name.
    mod_copy: Option<(String, String)>,
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
    ui.horizontal_wrapped(|ui| {
        for (tab, label) in [
            (Tab::Level, LEVEL_TAB),
            (Tab::Skin, SKIN_TAB),
            (Tab::Mod, MOD_TAB),
            (Tab::Material, MATERIAL_TAB),
        ] {
            if ui.selectable_label(data.editor.tab == tab, RichText::new(label).heading()).clicked() {
                data.editor.tab = tab;
            }
        }
    });
    ui.add_space(8.0);
    if super::short(ui.ctx()) {
        // A short window (issue #49): the tab scrolls, its lists in it.
        ScrollArea::vertical().id_salt("editor tab").auto_shrink([false, false]).show(ui, |ui| {
            tab(ui, data, out);
            crate::pad::follow_focus(ui);
        });
    } else {
        tab(ui, data, out);
    }
}

/// The open tab's content.
fn tab(ui: &mut Ui, data: &mut MenuData, out: &mut Vec<UiAction>) {
    if data.editor.tab == Tab::Skin {
        skins(ui, data, out);
        return;
    }
    if data.editor.tab == Tab::Mod {
        mods(ui, data, out);
        return;
    }
    if data.editor.tab == Tab::Material {
        materials(ui, data, out);
        return;
    }
    if !data.recoveries.is_empty() {
        section(ui, "Unsaved changes", |ui, _| {
            for (id, name, damaged) in &data.recoveries {
                ui.horizontal_wrapped(|ui| {
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
                ui.add(TextEdit::singleline(&mut f.name).desired_width(fit(ui, 240.0))).labelled_by(label);
            });
            form_row(ui, "Width", |ui, label| {
                ui.add(TextEdit::singleline(&mut f.width).desired_width(fit(ui, 80.0))).labelled_by(label);
            });
            form_row(ui, "Height", |ui, label| {
                ui.add(TextEdit::singleline(&mut f.height).desired_width(fit(ui, 80.0))).labelled_by(label);
            });
            form_row(ui, "Theme", |ui, label| {
                let shown = themes.iter().find(|t| t.0 == f.theme).map_or("none", |t| t.1.as_str());
                let combo = ComboBox::from_id_salt("new level theme")
                    .selected_text(shown)
                    .width(fit(ui, 240.0))
                    .show_ui(ui, |ui| {
                        for (id, name) in &themes {
                            ui.selectable_value(&mut f.theme, id.clone(), name);
                        }
                    })
                    .response
                    .labelled_by(label);
                let ids: Vec<&String> = themes.iter().map(|t| &t.0).collect();
                if let Some(id) = crate::pad::pad_choice(ui, &combo, &ids, &&f.theme) {
                    f.theme = id.clone();
                }
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
        // Sent once, the form closes: a second click would make a second pack.
        if create || cancel {
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
                ui.add(TextEdit::singleline(name).desired_width(fit(ui, 240.0))).labelled_by(label);
            });
            form_row(ui, "", |ui, _| {
                copy = ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Copy")).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if copy {
            out.push(UiAction::CopyLevel { from: from.clone(), name: name.trim().to_string() });
        }
        if copy || cancel {
            view.copy = None;
        }
        ui.add_space(10.0);
    }
    section(ui, "Levels", |ui, _| {
        ui.label(RichText::new("Levels you made are edited in place; the others as a copy.").weak());
        super::list_area(ui, ScrollArea::vertical().auto_shrink([false, true]), |ui| {
            let mut level = |ui: &mut Ui, l: &olr_content::LevelInfo| {
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
            };
            if narrow(ui) {
                for l in &data.catalog.levels {
                    ui.horizontal_wrapped(|ui| level(ui, l));
                }
            } else {
                egui::Grid::new("editor levels").num_columns(3).striped(true).spacing([24.0, 4.0]).show(
                    ui,
                    |ui| {
                        for l in &data.catalog.levels {
                            level(ui, l);
                            ui.end_row();
                        }
                    },
                );
            }
            crate::pad::follow_focus(ui);
        });
    });
}

/// The Skin tab (issue #14): New blank skin, then every skin, the user's
/// first, searchable: Edit for theirs, Edit a copy for the others.
fn skins(ui: &mut Ui, data: &mut MenuData, out: &mut Vec<UiAction>) {
    section(ui, "New skin", |ui, _| {
        ui.horizontal_wrapped(|ui| {
            if ui.button(NEW_SKIN).clicked() {
                out.push(UiAction::NewSkin(None));
            }
            ui.label(
                RichText::new("double resolution; or start from any skin below with Edit a copy").weak(),
            );
        });
    });
    ui.add_space(10.0);
    let search = data.editor.skin_search.to_lowercase();
    let mut shown: Vec<(&str, &str, bool)> = data
        .catalog
        .skins
        .iter()
        .filter(|s| search.is_empty() || s.name.to_lowercase().contains(&search) || s.id.contains(&search))
        .map(|s| (s.id.as_str(), s.name.as_str(), data.editable_skins.contains(&s.id)))
        .collect();
    // Yours first, then the others, each in the catalog's order.
    shown.sort_by_key(|s| !s.2);
    let total = data.catalog.skins.len();
    let mut picked = None;
    section(ui, "Skins", |ui, _| {
        ui.label(
            RichText::new("Skins you made are edited in place; the others as a copy (they never change).")
                .weak(),
        );
        ui.horizontal_wrapped(|ui| {
            let l = ui.label(SEARCH_SKINS);
            ui.add(TextEdit::singleline(&mut data.editor.skin_search).desired_width(fit(ui, 240.0)))
                .labelled_by(l.id);
            ui.label(RichText::new(format!("{} of {total} skins", shown.len())).weak());
        });
        let row = ui.spacing().interact_size.y + 4.0;
        let mut rows = |ui: &mut Ui, range: std::ops::Range<usize>| {
            for &(id, name, yours) in &shown[range] {
                ui.horizontal(|ui| {
                    // Room for Edit a copy beside it in a narrow column.
                    let w = (ui.available_width() - EDIT_A_COPY).clamp(60.0, 320.0);
                    ui.add_sized([w, row - 4.0], egui::Label::new(name).truncate());
                    if yours {
                        if ui.button("Edit").clicked() {
                            picked = Some(UiAction::EditSkin(id.to_string()));
                        }
                    } else if ui.button("Edit a copy").clicked() {
                        picked = Some(UiAction::NewSkin(Some(id.to_string())));
                    }
                });
            }
            crate::pad::follow_focus(ui);
        };
        if super::short(ui.ctx()) {
            // The page scrolls ([`super::list_area`]).
            ui.vertical(|ui| rows(ui, 0..shown.len()));
        } else {
            ScrollArea::vertical().id_salt("editor skins").auto_shrink([false, true]).show_rows(
                ui,
                row,
                shown.len(),
                rows,
            );
        }
    });
    out.extend(picked);
}

/// The Mod tab (issue #17): the mod recoveries, then every mod, the user's
/// first: Edit for theirs, Edit a copy (a new mod, named first) for any.
fn mods(ui: &mut Ui, data: &mut MenuData, out: &mut Vec<UiAction>) {
    if !data.mod_recoveries.is_empty() {
        section(ui, "Unsaved changes", |ui, _| {
            for (id, name, damaged) in &data.mod_recoveries {
                ui.horizontal_wrapped(|ui| {
                    if *damaged {
                        ui.label(
                            RichText::new(format!("{id}: damaged")).color(Color32::from_rgb(255, 110, 90)),
                        );
                    } else if ui.button(format!("Recover {name}")).clicked() {
                        out.push(UiAction::RecoverMod(id.clone()));
                    }
                    if ui.button("Discard").clicked() {
                        out.push(UiAction::DiscardModRecovery(id.clone()));
                    }
                });
            }
        });
        ui.add_space(10.0);
    }
    if let Some((from, name)) = &mut data.editor.mod_copy {
        let mut create = false;
        let mut cancel = false;
        let shown =
            data.catalog.mods.iter().find(|m| m.id == *from).map_or(from.as_str(), |m| m.name.as_str());
        section(ui, &format!("New mod from {shown}"), |ui, _| {
            form_row(ui, MOD_NAME, |ui, label| {
                ui.add(TextEdit::singleline(name).desired_width(fit(ui, 240.0))).labelled_by(label);
            });
            form_row(ui, "", |ui, _| {
                create = ui.add_enabled(!name.trim().is_empty(), egui::Button::new(CREATE_MOD)).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if create {
            out.push(UiAction::NewMod { from: from.clone(), name: name.trim().to_string() });
        }
        // Sent once, the form closes: a second click would make a second pack.
        if create || cancel {
            data.editor.mod_copy = None;
        }
        ui.add_space(10.0);
    }
    let mut shown: Vec<(&str, &str, bool)> = data
        .catalog
        .mods
        .iter()
        .map(|m| (m.id.as_str(), m.name.as_str(), data.editable_mods.contains(&m.id)))
        .collect();
    // Yours first, then the others, each in the catalog's order.
    shown.sort_by_key(|m| !m.2);
    let mut copy = None;
    section(ui, "Mods", |ui, _| {
        ui.label(
            RichText::new(
                "Mods you made are edited in place; a new mod starts as a copy of any (the built-in ones never change).",
            )
            .weak(),
        );
        super::list_area(
            ui,
            ScrollArea::vertical().id_salt("editor mods").auto_shrink([false, true]),
            |ui| {
                let mut row = |ui: &mut Ui, (id, name, yours): (&str, &str, bool), grid: bool| {
                    ui.label(name);
                    if yours {
                        if ui.button("Edit").clicked() {
                            out.push(UiAction::EditMod(id.to_string()));
                        }
                    } else if grid {
                        ui.label("");
                    }
                    if ui.button("Edit a copy").clicked() {
                        copy = Some((id.to_string(), format!("{name} copy")));
                    }
                };
                if narrow(ui) {
                    for &m in &shown {
                        ui.horizontal_wrapped(|ui| row(ui, m, false));
                    }
                } else {
                    egui::Grid::new("editor mods").num_columns(3).striped(true).spacing([24.0, 4.0]).show(
                        ui,
                        |ui| {
                            for &m in &shown {
                                row(ui, m, true);
                                ui.end_row();
                            }
                        },
                    );
                }
                crate::pad::follow_focus(ui);
            },
        );
    });
    if copy.is_some() {
        data.editor.mod_copy = copy;
    }
}

/// The Material tab (issue #44): the material recoveries, then the
/// material editor's way in (it lists the materials itself).
fn materials(ui: &mut Ui, data: &mut MenuData, out: &mut Vec<UiAction>) {
    if !data.material_recoveries.is_empty() {
        section(ui, "Unsaved changes", |ui, _| {
            for (id, name, damaged) in &data.material_recoveries {
                ui.horizontal_wrapped(|ui| {
                    if *damaged {
                        ui.label(
                            RichText::new(format!("{id}: damaged")).color(Color32::from_rgb(255, 110, 90)),
                        );
                    } else if ui.button(format!("Recover {name}")).clicked() {
                        out.push(UiAction::RecoverMaterial(id.clone()));
                    }
                    if ui.button("Discard").clicked() {
                        out.push(UiAction::DiscardMaterialRecovery(id.clone()));
                    }
                });
            }
        });
        ui.add_space(10.0);
    }
    section(ui, "Surface materials", |ui, _| {
        ui.label(
            RichText::new(
                "What a level's pixels are made of for the look: stone, wood, metal, glass… Explosions leave \
                 the ones that take heat glowing hot. The built-in materials never change: edit a copy. \
                 Levels keep copies of the materials they use.",
            )
            .weak(),
        );
        ui.add_space(6.0);
        if ui.button(OPEN_MATERIAL_EDITOR).clicked() {
            out.push(UiAction::OpenMaterialEditor);
        }
    });
}
