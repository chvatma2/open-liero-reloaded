//! Local play's setup (OLX's local game menu,
//! `src/client/DeprecatedGUI/Menu_Local.cpp`, and its game settings
//! dialog): the mod, the level grid with thumbnails and sizes, the
//! Deathmatch settings, the bots, the human's profile, and Start. Laid
//! out anew in the page frame: the map on the left, the match and the
//! players (the human's profile, the bots) in panels on the right, Start
//! centred below.

use egui::{Color32, ComboBox, RichText, ScrollArea, Sense, Slider, Ui, Vec2};

use super::{
    MenuData, columns, footer, form_check_row, form_row, message, page, primary, section, tall_section,
};
use crate::match_prefs::{
    BotLevel, BotLoadout, BotPersonality, KILL_LIMIT_RANGE, LIVES_RANGE, LOADING_TIME_RANGE, MAX_BOTS,
    MatchPrefs, TIME_LIMIT_RANGE,
};
use crate::state::{UiAction, UiState};

/// A level card's picture, in points (the imported thumbnails' 160 × 96,
/// `olr_pack::THUMB_SIZE`), and the space between cards.
const THUMB: Vec2 = Vec2::new(160.0, 96.0);
const GAP: f32 = 10.0;

/// The page's column (two panels side by side).
const WIDTH: f32 = 1100.0;

/// The space between the panels of a column.
const SECTION_GAP: f32 = 8.0;

/// The pickers' width.
const COMBO_W: f32 = 200.0;

/// What a rule is set to when it is switched on (OLX's defaults,
/// `src/common/FeatureList.cpp:31-33`; lives as OLX's menu starts them).
const LIVES_ON: i32 = 10;
const KILL_LIMIT_ON: i32 = 15;
const TIME_LIMIT_ON: f32 = 6.0;

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let before = state.settings().gameplay.match_prefs.clone();
    let mut p = before.clone();
    // A saved mod or level that is no longer imported: the first listed.
    if data.catalog.mod_info(&p.mod_id).is_none()
        && let Some(m) = data.catalog.mods.first()
    {
        p.set_mod(m);
    }
    if data.catalog.level(&p.level_id).is_none()
        && let Some(l) = data.catalog.levels.first()
    {
        p.level_id = l.id.clone();
    }
    let mut profiles = state.profiles().clone();
    let mut start = false;
    let mut ui = page(ui, "Local Play", WIDTH, out);
    let ui = &mut ui;
    // Start, and why the last start failed, always in view.
    let mut ui = footer(ui, "local play", |ui| {
        let starting = state.is_starting();
        if primary(ui, "Start", !starting).clicked() {
            start = true;
        }
        if starting {
            ui.label("Loading…");
        }
        message(ui, state);
    });
    let ui = &mut ui;
    columns(ui, 2, |c| {
        tall_section(&mut c[0], "Map", |ui, _| {
            mod_picker(ui, data, &mut p);
            ui.add_space(8.0);
            ScrollArea::vertical().id_salt("local play levels").auto_shrink([false, false]).show(ui, |ui| {
                level_grid(ui, data, &mut p);
            });
        });
        ScrollArea::vertical().id_salt("local play setup").show(&mut c[1], |ui| {
            section(ui, "Match (Deathmatch)", |ui, _| {
                if rules(ui, data, &mut p) {
                    p = olx_match(data, &p);
                }
            });
            ui.add_space(SECTION_GAP);
            // The human, then the bots.
            section(ui, "Players", |ui, _| {
                form_row(ui, "Player", |ui, label| {
                    let names: Vec<&str> = profiles.list.iter().map(|p| p.name.as_str()).collect();
                    let shown = names.get(profiles.selected).copied().unwrap_or("-").to_string();
                    let mut selected = profiles.selected;
                    ComboBox::from_id_salt("profile")
                        .selected_text(shown)
                        .width(COMBO_W)
                        .show_ui(ui, |ui| {
                            for (i, n) in names.iter().enumerate() {
                                ui.selectable_value(&mut selected, i, *n);
                            }
                        })
                        .response
                        .labelled_by(label);
                    profiles.selected = selected;
                });
                bots(ui, &mut p);
            });
        });
    });
    // The changes first, so a Start in the same frame starts them.
    if p != before {
        out.push(UiAction::SetMatch(p));
    }
    if profiles != *state.profiles() {
        out.push(UiAction::SetProfiles(profiles));
    }
    if start {
        out.push(UiAction::StartMatch);
    }
}

pub(super) fn mod_picker(ui: &mut Ui, data: &MenuData, p: &mut MatchPrefs) {
    let name = data
        .catalog
        .mod_info(&p.mod_id)
        .map_or_else(|| format!("{} (missing)", p.mod_id), |m| m.name.clone());
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new("Mod").strong());
        ComboBox::from_id_salt("mod")
            .selected_text(name)
            .width(260.0)
            .show_ui(ui, |ui| {
                for m in &data.catalog.mods {
                    if ui.selectable_label(m.id == p.mod_id, &m.name).clicked() {
                        p.set_mod(m);
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
    if data.catalog.mods.is_empty() {
        ui.label(RichText::new("No mods: import OLX's content with `olr-cli import`.").weak());
    }
}

pub(super) fn level_grid(ui: &mut Ui, data: &mut MenuData, p: &mut MatchPrefs) {
    ui.label(RichText::new("Level").strong());
    if data.catalog.level(&p.level_id).is_none() {
        ui.label(RichText::new(format!("{} (missing)", p.level_id)).color(Color32::from_rgb(255, 110, 90)));
    }
    let levels: Vec<(String, String, String)> = data
        .catalog
        .levels
        .iter()
        .map(|l| (l.id.clone(), l.name.clone(), format!("{} × {}", l.width, l.height)))
        .collect();
    let per_row = ((ui.available_width() + GAP) / (THUMB.x + GAP)).floor().max(1.0) as usize;
    egui::Grid::new("levels").spacing([GAP, GAP]).show(ui, |ui| {
        for (n, (id, name, size)) in levels.into_iter().enumerate() {
            let selected = id == p.level_id;
            ui.vertical(|ui| {
                ui.set_width(THUMB.x);
                let (rect, resp) = ui.allocate_exact_size(THUMB, Sense::click());
                // Decoded only once its card is in view.
                let thumb = if ui.is_rect_visible(rect) { data.thumb(ui.ctx(), &id) } else { None };
                let painter = ui.painter_at(rect);
                match &thumb {
                    Some(t) => {
                        painter.rect_filled(rect, 4.0, Color32::from_rgb(24, 22, 20));
                        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        painter.image(t.id(), fit(t.size_vec2(), rect), uv, Color32::WHITE);
                    }
                    // No thumbnail yet: the name and size (plan 7 ruling Q1).
                    None => {
                        painter.rect_filled(rect, 4.0, Color32::from_rgb(44, 40, 36));
                        let font = egui::FontId::proportional(14.0);
                        let text = format!("{name}\n{size}");
                        let c = Color32::from_gray(200);
                        painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, font, c);
                    }
                }
                let stroke = if selected {
                    egui::Stroke::new(3.0, Color32::from_rgb(255, 200, 80))
                } else {
                    egui::Stroke::new(1.0, Color32::from_gray(70))
                };
                painter.rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Inside);
                let clicked = ui.selectable_label(selected, &name).clicked();
                ui.label(RichText::new(&size).small().weak());
                if resp.clicked() || clicked {
                    p.level_id = id.clone();
                }
            });
            if (n + 1) % per_row == 0 {
                ui.end_row();
            }
        }
    });
}

/// The largest rectangle of `size`'s aspect centred in `rect`: a
/// thumbnail of any shape keeps it (letterboxed).
pub(crate) fn fit(size: Vec2, rect: egui::Rect) -> egui::Rect {
    if !(size.x > 0.0 && size.y > 0.0) {
        return rect;
    }
    let k = (rect.width() / size.x).min(rect.height() / size.y);
    egui::Rect::from_center_size(rect.center(), size * k)
}

/// The match OLX starts with: its rules, bots and weapons, the mod and
/// level kept (the mod's presets decide the weapons).
pub(super) fn olx_match(data: &MenuData, p: &MatchPrefs) -> MatchPrefs {
    let mut d =
        MatchPrefs { mod_id: p.mod_id.clone(), level_id: p.level_id.clone(), ..MatchPrefs::default() };
    if let Some(m) = data.catalog.mod_info(&d.mod_id) {
        d.set_mod(m);
    }
    d
}

/// OLX's Deathmatch settings: a rule switched off says so in place of
/// its slider. True when the player asked for OLX's match back.
pub(super) fn rules(ui: &mut Ui, data: &MenuData, p: &mut MatchPrefs) -> bool {
    let r = &mut p.rules;
    form_check_row(ui, r.lives >= 0, "Lives", |ui, on, changed| {
        if changed {
            r.lives = if on { LIVES_ON } else { -2 };
        }
        if on {
            ui.add(Slider::new(&mut r.lives, LIVES_RANGE));
        } else {
            ui.label(RichText::new("unlimited").weak());
        }
    });
    form_check_row(ui, r.kill_limit > 0, "Kill limit", |ui, on, changed| {
        if changed {
            r.kill_limit = if on { KILL_LIMIT_ON } else { -1 };
        }
        if on {
            ui.add(Slider::new(&mut r.kill_limit, KILL_LIMIT_RANGE));
        } else {
            ui.label(RichText::new("off").weak());
        }
    });
    form_check_row(ui, r.time_limit > 0.0, "Time limit", |ui, on, changed| {
        if changed {
            r.time_limit = if on { TIME_LIMIT_ON } else { -1.0 };
        }
        if on && !ui.is_enabled() {
            // Read-only (a client's lobby): the slider's range would clamp
            // the host's value (a 0.2-minute limit shown as 0.50).
            ui.label(format!("{} min", r.time_limit));
        } else if on {
            ui.add(Slider::new(&mut r.time_limit, TIME_LIMIT_RANGE).step_by(0.5).suffix(" min"));
        } else {
            ui.label(RichText::new("off").weak());
        }
    });
    form_row(ui, "Loading time", |ui, _| {
        ui.add(Slider::new(&mut p.loading_time, LOADING_TIME_RANGE).suffix(" %"));
    });
    form_row(ui, "Weapons", |ui, label| {
        let presets = data.catalog.mod_info(&p.mod_id).map(|m| m.presets.clone()).unwrap_or_default();
        ComboBox::from_id_salt("restrictions")
            .selected_text(p.restrictions.clone())
            .width(COMBO_W)
            .show_ui(ui, |ui| {
                for name in presets {
                    let checked = name == p.restrictions;
                    if ui.selectable_label(checked, &name).clicked() {
                        p.restrictions = name;
                    }
                }
            })
            .response
            .labelled_by(label);
    });
    form_row(ui, "", |ui, _| {
        ui.checkbox(&mut p.bonuses, "Bonuses");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.button("Reset the match to OLX's settings").clicked()
        })
        .inner
    })
}

fn level_name(l: BotLevel) -> &'static str {
    match l {
        BotLevel::Easy => "Easy",
        BotLevel::Medium => "Medium",
        BotLevel::Hard => "Hard",
        BotLevel::Xtreme => "Xtreme",
    }
}

/// A difficulty picker, labelled by `label` (found by its name).
pub(super) fn level_combo(
    ui: &mut Ui,
    label: egui::Id,
    id: impl std::hash::Hash + std::fmt::Debug,
    level: &mut BotLevel,
) {
    ComboBox::from_id_salt(id)
        .selected_text(level_name(*level))
        .width(COMBO_W)
        .show_ui(ui, |ui| {
            for l in BotLevel::ALL {
                ui.selectable_value(level, l, level_name(l));
            }
        })
        .response
        .labelled_by(label);
}

/// A personality picker, labelled by `label`.
pub(super) fn personality_combo(
    ui: &mut Ui,
    label: egui::Id,
    id: impl std::hash::Hash + std::fmt::Debug,
    personality: &mut BotPersonality,
) {
    let name = |b: BotPersonality| match b {
        BotPersonality::Olx => "OLX bot",
        BotPersonality::Chaos => "Chaos (fires everything)",
        BotPersonality::Idle => "Idle (does nothing)",
    };
    ComboBox::from_id_salt(id)
        .selected_text(name(*personality))
        .width(COMBO_W)
        .show_ui(ui, |ui| {
            for b in BotPersonality::ALL {
                ui.selectable_value(personality, b, name(b));
            }
        })
        .response
        .labelled_by(label);
}

/// The bots: how many, their difficulty (one for all or each its own),
/// personality and loadout.
fn bots(ui: &mut Ui, p: &mut MatchPrefs) {
    form_row(ui, "Bots", |ui, label| {
        let mut n = p.bots;
        if ui.add(Slider::new(&mut n, 1..=MAX_BOTS)).labelled_by(label).changed() {
            p.set_bots(n);
        }
    });
    form_row(ui, "", |ui, _| {
        let mut each = p.per_bot.is_some();
        if ui.checkbox(&mut each, "Each bot its own difficulty").changed() {
            p.set_per_bot(each);
        }
    });
    match &mut p.per_bot {
        None => {
            form_row(ui, "Difficulty", |ui, label| level_combo(ui, label, "difficulty", &mut p.difficulty))
        }
        Some(levels) => {
            for (i, l) in levels.iter_mut().enumerate() {
                form_row(ui, &format!("Bot {}", i + 1), |ui, label| level_combo(ui, label, ("bot", i), l));
            }
        }
    }
    form_row(ui, "Personality", |ui, label| personality_combo(ui, label, "personality", &mut p.personality));
    form_row(ui, "Loadout", |ui, label| {
        let name = |b: BotLoadout| match b {
            BotLoadout::OlxPreset => "OLX's choice",
            BotLoadout::Random => "Random",
        };
        ComboBox::from_id_salt("loadout")
            .selected_text(name(p.loadout))
            .width(COMBO_W)
            .show_ui(ui, |ui| {
                for b in [BotLoadout::OlxPreset, BotLoadout::Random] {
                    ui.selectable_value(&mut p.loadout, b, name(b));
                }
            })
            .response
            .labelled_by(label);
    });
}
