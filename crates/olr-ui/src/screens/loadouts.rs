//! Loadouts and bots in local play and the lobby (issue #5; not OLX's):
//! the loadout picker ("Default" first, then the mod's, then the player's
//! own) with "Edit loadouts…", the loadout editor and the "Advanced…"
//! bots window, each a modal window over the page. Esc or a click outside
//! closes a window as its Done or Close does.

use egui::{Button, Color32, ComboBox, Id, Key, Modal, RichText, ScrollArea, Slider, TextEdit, Ui};
use olr_content::{MatchLoadout, ModInfo};

use super::local_play::{level_combo, personality_combo};
use super::{MenuData, dialog, fit, form_row, list_area};
use crate::loadouts::{MAX_OWN_LOADOUTS, OwnLoadouts, check_name, choices, explicit, free_name, resolve};
use crate::match_prefs::{BotLoadout, LOADING_TIME_RANGE, MatchPrefs};
use crate::state::UiAction;

const RED: Color32 = Color32::from_rgb(255, 110, 90);

/// The pickers' width.
const COMBO_W: f32 = 200.0;

/// The room "Edit loadouts…" needs beside the picker; less puts it under.
const EDIT_W: f32 = 130.0;

/// What the editor says of the unique rule.
pub const UNIQUE_HINT: &str =
    "No weapon twice in a worm's five slots, unless every weapon on is already in one.";

/// Why an own loadout with no weapon on is not saved.
pub const NO_WEAPON: &str = "Turn at least one weapon on: a loadout without weapons is not saved.";

/// Why a loadout cannot be changed.
pub const READ_ONLY: &str = "Default and the mod's loadouts are read-only: Copy one to change it.";

/// The loadout editor's state while it is open.
pub(super) struct Editor {
    mod_id: String,
    /// The loadout shown, by name.
    selected: String,
    /// Rename's text while renaming.
    rename: Option<String>,
    /// Why the last name or change was refused.
    error: Option<String>,
    /// The selected own loadout as edited to no weapon: shown, not saved.
    draft: Option<MatchLoadout>,
}

impl Editor {
    /// The editor on mod `mod_id`'s loadouts, `selected` shown.
    pub(super) fn new(mod_id: &str, selected: &str) -> Editor {
        Editor {
            mod_id: mod_id.to_string(),
            selected: selected.to_string(),
            rename: None,
            error: None,
            draft: None,
        }
    }
}

/// Re-resolves `p`'s loadout by name in its mod (`Default` when gone; an
/// own one edited meanwhile as it is now), each problem logged once.
pub(super) fn refresh(data: &mut MenuData, own: &OwnLoadouts, p: &mut MatchPrefs) {
    let Some(m) = data.catalog.mod_info(&p.mod_id) else { return };
    let (l, w) = resolve(&p.weapons.name, m, own);
    p.weapons = l;
    for w in w {
        data.warn_once(w);
    }
}

/// The "Loadout" row: the mod's loadouts to choose from and, when
/// `editable`, "Edit loadouts…", which opens the editor.
pub(super) fn picker(
    ui: &mut Ui,
    data: &mut MenuData,
    own: &OwnLoadouts,
    p: &mut MatchPrefs,
    editable: bool,
) {
    let list = data.catalog.mod_info(&p.mod_id).map(|m| choices(m, own)).unwrap_or_default();
    let (mut edit, mut below) = (false, false);
    form_row(ui, "Loadout", |ui, label| {
        let combo = ComboBox::from_id_salt("match loadout")
            .selected_text(p.weapons.name.clone())
            .width(fit(ui, COMBO_W))
            .truncate()
            .show_ui(ui, |ui| {
                for c in &list {
                    if ui.selectable_label(c.loadout.name == p.weapons.name, &c.loadout.name).clicked() {
                        p.weapons = c.loadout.clone();
                    }
                }
            })
            .response
            .labelled_by(label);
        let names: Vec<&str> = list.iter().map(|c| c.loadout.name.as_str()).collect();
        if let Some(name) = crate::pad::pad_choice(ui, &combo, &names, &p.weapons.name.as_str())
            && let Some(c) = list.iter().find(|c| c.loadout.name == name)
        {
            p.weapons = c.loadout.clone();
        }
        // Beside the picker, or under it in a narrow column.
        if editable && ui.available_width() >= EDIT_W {
            edit = ui.add_enabled(!list.is_empty(), Button::new("Edit loadouts…")).clicked();
        } else if editable {
            below = true;
        }
    });
    if below {
        form_row(ui, "", |ui, _| {
            edit = ui.add_enabled(!list.is_empty(), Button::new("Edit loadouts…")).clicked();
        });
    }
    if edit {
        data.loadout_editor = Some(Editor::new(&p.mod_id, &p.weapons.name));
    }
}

/// The loadout editor, while open: its changes to the own loadouts as a
/// `SetLoadouts`; `Some(loadout)` when it closed, the one it showed,
/// to select (resolved as it now is).
pub(super) fn editor(
    ctx: &egui::Context,
    data: &mut MenuData,
    own: &OwnLoadouts,
    out: &mut Vec<UiAction>,
) -> Option<MatchLoadout> {
    let ed = data.loadout_editor.as_mut()?;
    let Some(m) = data.catalog.mod_info(&ed.mod_id) else {
        data.loadout_editor = None;
        return None;
    };
    let mut mine = own.clone();
    let mut done = false;
    let r = Modal::new(Id::new("loadout editor")).show(ctx, |ui| {
        dialog(ui, "loadout editor", 640.0, |ui| {
            ui.heading(format!("Loadouts of {}", m.name));
            ui.add_space(6.0);
            // A narrow window (a large UI scale): the list over the details.
            if ui.available_width() < 520.0 {
                list(ui, m, &mut mine, ed);
                ui.separator();
                details(ui, m, &mut mine, ed);
            } else {
                ui.columns(2, |c| {
                    list(&mut c[0], m, &mut mine, ed);
                    details(&mut c[1], m, &mut mine, ed);
                });
            }
            if let Some(e) = &ed.error {
                ui.add(egui::Label::new(RichText::new(e).color(RED)).wrap());
            }
            ui.add_space(6.0);
            ui.vertical_centered(|ui| done = ui.button("Done").clicked());
        });
    });
    let esc = esc_once(ctx);
    if esc && ed.rename.is_some() {
        ed.rename = None;
    } else if done || r.should_close() || esc {
        let name = ed.selected.clone();
        data.loadout_editor = None;
        let (l, w) = resolve(&name, m, &mine);
        push_own(own, mine, out);
        for w in w {
            data.warn_once(w);
        }
        return Some(l);
    }
    push_own(own, mine, out);
    None
}

/// Esc pressed this frame (a held key's repeats are not).
fn esc_once(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        i.events
            .iter()
            .any(|e| matches!(e, egui::Event::Key { key: Key::Escape, pressed: true, repeat: false, .. }))
    })
}

fn push_own(own: &OwnLoadouts, mut mine: OwnLoadouts, out: &mut Vec<UiAction>) {
    mine.sanitize();
    if mine != *own {
        out.push(UiAction::SetLoadouts(mine));
    }
}

/// The mod's loadouts (the read-only ones marked), New, Copy, Rename and
/// Delete.
fn list(ui: &mut Ui, m: &ModInfo, mine: &mut OwnLoadouts, ed: &mut Editor) {
    let all = choices(m, mine);
    list_area(ui, ScrollArea::vertical().id_salt("loadout list").max_height(320.0), |ui| {
        for c in &all {
            ui.horizontal(|ui| {
                let picked = c.loadout.name == ed.selected;
                if ui.selectable_label(picked, &c.loadout.name).clicked() && !picked {
                    *ed = Editor::new(&ed.mod_id, &c.loadout.name);
                }
                if c.shipped {
                    ui.label(RichText::new("read-only").small().weak());
                }
            });
        }
        crate::pad::follow_focus(ui);
    });
    let shown = all.iter().find(|c| c.loadout.name == ed.selected).cloned();
    let own_one = shown.as_ref().is_some_and(|c| !c.shipped);
    let room = mine.of(&m.id).len() < MAX_OWN_LOADOUTS;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let full = format!("at most {MAX_OWN_LOADOUTS} loadouts a mod");
        if ui.add_enabled(room, Button::new("New")).on_disabled_hover_text(&full).clicked() {
            let l = MatchLoadout {
                name: free_name("New loadout", m, mine),
                ..explicit(&MatchLoadout::default(), m)
            };
            *ed = Editor::new(&ed.mod_id, &l.name);
            mine.of_mut(&m.id).push(l);
        }
        let copy = ui.add_enabled(room && shown.is_some(), Button::new("Copy")).on_disabled_hover_text(&full);
        if copy.clicked()
            && let Some(c) = &shown
        {
            let mut l = explicit(&c.loadout, m);
            l.name = free_name(&format!("{} copy", c.loadout.name), m, mine);
            *ed = Editor::new(&ed.mod_id, &l.name);
            mine.of_mut(&m.id).push(l);
        }
        let rename = ui.add_enabled(own_one && ed.rename.is_none(), Button::new("Rename"));
        if rename.on_disabled_hover_text(READ_ONLY).clicked() {
            ed.rename = Some(ed.selected.clone());
        }
        let delete = ui.add_enabled(own_one, Button::new("Delete")).on_disabled_hover_text(READ_ONLY);
        // At once, unasked (the user's request, 2026-10-25).
        if delete.clicked() {
            mine.of_mut(&m.id).retain(|l| l.name != ed.selected);
            *ed = Editor::new(&ed.mod_id, olr_content::DEFAULT_LOADOUT);
        }
    });
    if let Some(mut text) = ed.rename.take() {
        ed.rename = ui
            .horizontal_wrapped(|ui| {
                let label = ui.label("New name");
                let field = ui
                    .add(TextEdit::singleline(&mut text).desired_width(fit(ui, 160.0)))
                    .labelled_by(label.id);
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if ui.button("OK").clicked() || enter {
                    match check_name(&text, m, mine, Some(&ed.selected)) {
                        Ok(name) => {
                            if let Some(l) = mine.of_mut(&m.id).iter_mut().find(|l| l.name == ed.selected) {
                                l.name = name.clone();
                            }
                            if let Some(d) = &mut ed.draft {
                                d.name = name.clone();
                            }
                            ed.selected = name;
                            ed.error = None;
                            return None;
                        }
                        Err(e) => ed.error = Some(e),
                    }
                }
                if ui.button("Cancel").clicked() {
                    ed.error = None;
                    return None;
                }
                Some(text)
            })
            .inner;
    }
}

/// The selected loadout's weapons (All on, All off), loading time and
/// unique rule; greyed for a read-only one.
fn details(ui: &mut Ui, m: &ModInfo, mine: &mut OwnLoadouts, ed: &mut Editor) {
    let Some(c) = choices(m, mine).into_iter().find(|c| c.loadout.name == ed.selected) else {
        ed.selected = olr_content::DEFAULT_LOADOUT.into();
        return;
    };
    let saved = explicit(&c.loadout, m);
    let mut l = ed.draft.clone().unwrap_or_else(|| saved.clone());
    ui.label(RichText::new(&c.loadout.name).strong());
    if c.shipped {
        ui.add(egui::Label::new(RichText::new(READ_ONLY).weak()).wrap());
    }
    ui.add_enabled_ui(!c.shipped, |ui| {
        // `explicit` lists: an empty one is none on here, not every weapon.
        let mut on: Vec<bool> =
            m.weapons.iter().map(|w| l.weapons.iter().any(|n| n.eq_ignore_ascii_case(w))).collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Weapons").strong());
            if ui.button("All on").clicked() {
                on.fill(true);
            }
            if ui.button("All off").clicked() {
                on.fill(false);
            }
        });
        list_area(ui, ScrollArea::vertical().id_salt("loadout weapons").max_height(240.0), |ui| {
            for (w, on) in m.weapons.iter().zip(on.iter_mut()) {
                ui.checkbox(on, w);
            }
            crate::pad::follow_focus(ui);
        });
        l.weapons = m.weapons.iter().zip(&on).filter(|(_, on)| **on).map(|(w, _)| w.clone()).collect();
        form_row(ui, "Loading time", |ui, label| {
            ui.add(Slider::new(&mut l.loading_time, LOADING_TIME_RANGE).suffix(" %")).labelled_by(label);
        });
        ui.checkbox(&mut l.unique, "Unique weapons");
        ui.add(egui::Label::new(RichText::new(UNIQUE_HINT).small().weak()).wrap());
    });
    if c.shipped || l == ed.draft.clone().unwrap_or_else(|| saved.clone()) {
        return;
    }
    if l.weapons.is_empty() {
        ed.error = Some(NO_WEAPON.into());
        ed.draft = Some(l);
        return;
    }
    ed.error = None;
    ed.draft = None;
    if let Some(slot) = mine.of_mut(&m.id).iter_mut().find(|o| o.name == ed.selected) {
        *slot = l;
    }
}

/// The "Advanced…" bots window, while open: each bot's own difficulty,
/// the personality and the bots' weapons.
pub(super) fn bots_window(ctx: &egui::Context, data: &mut MenuData, p: &mut MatchPrefs) {
    if !data.bots_open {
        return;
    }
    let mut close = false;
    let r = Modal::new(Id::new("bots window")).show(ctx, |ui| {
        dialog(ui, "bots window", 420.0, |ui| {
            ui.heading("Bots");
            ui.add_space(6.0);
            form_row(ui, "", |ui, _| {
                let mut each = p.per_bot.is_some();
                if ui.checkbox(&mut each, "Each bot its own difficulty").changed() {
                    p.set_per_bot(each);
                }
            });
            list_area(ui, ScrollArea::vertical().id_salt("bot levels").max_height(300.0), |ui| {
                match &mut p.per_bot {
                    None => form_row(ui, "Difficulty", |ui, label| {
                        level_combo(ui, label, "bots window difficulty", &mut p.difficulty)
                    }),
                    Some(levels) => {
                        for (i, l) in levels.iter_mut().enumerate() {
                            form_row(ui, &format!("Bot {}", i + 1), |ui, label| {
                                level_combo(ui, label, ("bot", i), l)
                            });
                        }
                    }
                }
                crate::pad::follow_focus(ui);
            });
            form_row(ui, "Personality", |ui, label| {
                personality_combo(ui, label, "personality", &mut p.personality)
            });
            form_row(ui, "Bots' weapons", |ui, label| {
                let name = |b: BotLoadout| match b {
                    BotLoadout::OlxPreset => "OLX's choice",
                    BotLoadout::Random => "Random",
                };
                ComboBox::from_id_salt("bot weapons")
                    .selected_text(name(p.loadout))
                    .width(fit(ui, COMBO_W))
                    .show_ui(ui, |ui| {
                        for b in [BotLoadout::OlxPreset, BotLoadout::Random] {
                            ui.selectable_value(&mut p.loadout, b, name(b));
                        }
                    })
                    .response
                    .labelled_by(label);
            });
            ui.add_space(6.0);
            ui.vertical_centered(|ui| close = ui.button("Close").clicked());
        })
    });
    let esc = esc_once(ctx);
    if close || r.should_close() || esc {
        data.bots_open = false;
    }
}
