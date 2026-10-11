//! Local play's setup (OLX's local game menu,
//! `src/client/DeprecatedGUI/Menu_Local.cpp`, and its game settings
//! dialog): the mod, the level grid with thumbnails and sizes, the
//! Deathmatch settings, the bots, the human's profile, and Start.

use egui::{Button, Color32, ComboBox, RichText, ScrollArea, Sense, Slider, Ui, Vec2};

use super::{MenuData, header, message};
use crate::match_prefs::{
    BotLevel, BotLoadout, BotPersonality, KILL_LIMIT_RANGE, LIVES_RANGE, LOADING_TIME_RANGE, MAX_BOTS,
    MatchPrefs, TIME_LIMIT_RANGE,
};
use crate::state::{UiAction, UiState};

/// A level card's picture, in points (the imported thumbnails' 160 × 96,
/// `rl_pack::THUMB_SIZE`), and the space between cards.
const THUMB: Vec2 = Vec2::new(160.0, 96.0);
const GAP: f32 = 10.0;

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
    header(ui, "Local Play", out);
    // Start, and why the last start failed, always in view.
    egui::Panel::bottom("local play start").show(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (starting, importing) = (state.is_starting(), state.is_importing());
            let button = Button::new(RichText::new("Start").size(20.0)).min_size(Vec2::new(180.0, 40.0));
            if ui.add_enabled(!starting && !importing, button).clicked() {
                start = true;
            }
            if starting {
                ui.label("Loading…");
            } else if importing {
                // Its packs are being written (plan 7 task 7).
                ui.label("Importing content…");
            }
            message(ui, state);
        });
        ui.add_space(6.0);
    });
    ui.columns(2, |c| {
        ScrollArea::vertical().id_salt("local play levels").show(&mut c[0], |ui| {
            mod_picker(ui, data, &mut p);
            ui.add_space(8.0);
            level_grid(ui, data, &mut p);
        });
        ScrollArea::vertical().id_salt("local play setup").show(&mut c[1], |ui| {
            rules(ui, data, &mut p);
            ui.add_space(8.0);
            bots(ui, &mut p);
            ui.add_space(8.0);
            ui.label(RichText::new("Player").strong());
            let names: Vec<&str> = profiles.list.iter().map(|p| p.name.as_str()).collect();
            let shown = names.get(profiles.selected).copied().unwrap_or("-").to_string();
            let mut selected = profiles.selected;
            ComboBox::from_id_salt("profile").selected_text(shown).show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ui.selectable_value(&mut selected, i, *n);
                }
            });
            profiles.selected = selected;
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

fn mod_picker(ui: &mut Ui, data: &MenuData, p: &mut MatchPrefs) {
    ui.label(RichText::new("Mod").strong());
    let name = data
        .catalog
        .mod_info(&p.mod_id)
        .map_or_else(|| format!("{} (missing)", p.mod_id), |m| m.name.clone());
    ComboBox::from_id_salt("mod").selected_text(name).width(260.0).show_ui(ui, |ui| {
        for m in &data.catalog.mods {
            if ui.selectable_label(m.id == p.mod_id, &m.name).clicked() {
                p.set_mod(m);
            }
        }
    });
    if data.catalog.mods.is_empty() {
        ui.label(RichText::new("No mods: import OLX's content first.").weak());
    }
}

fn level_grid(ui: &mut Ui, data: &mut MenuData, p: &mut MatchPrefs) {
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

/// OLX's Deathmatch settings (also the options' default match).
pub(super) fn rules(ui: &mut Ui, data: &MenuData, p: &mut MatchPrefs) {
    ui.label(RichText::new("Deathmatch").strong());
    let r = &mut p.rules;
    egui::Grid::new("rules").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        let mut on = r.lives >= 0;
        if ui.checkbox(&mut on, "Lives").changed() {
            r.lives = if on { LIVES_ON } else { -2 };
        }
        if on {
            ui.add(Slider::new(&mut r.lives, LIVES_RANGE));
        } else {
            ui.label("unlimited");
        }
        ui.end_row();
        let mut on = r.kill_limit > 0;
        if ui.checkbox(&mut on, "Kill limit").changed() {
            r.kill_limit = if on { KILL_LIMIT_ON } else { -1 };
        }
        if on {
            ui.add(Slider::new(&mut r.kill_limit, KILL_LIMIT_RANGE));
        } else {
            ui.label("off");
        }
        ui.end_row();
        let mut on = r.time_limit > 0.0;
        if ui.checkbox(&mut on, "Time limit").changed() {
            r.time_limit = if on { TIME_LIMIT_ON } else { -1.0 };
        }
        if on {
            ui.add(Slider::new(&mut r.time_limit, TIME_LIMIT_RANGE).step_by(0.5).suffix(" min"));
        } else {
            ui.label("off");
        }
        ui.end_row();
        ui.label("Loading time");
        ui.add(Slider::new(&mut p.loading_time, LOADING_TIME_RANGE).suffix(" %"));
        ui.end_row();
        ui.label("Weapons");
        let presets = data.catalog.mod_info(&p.mod_id).map(|m| m.presets.clone()).unwrap_or_default();
        ComboBox::from_id_salt("restrictions").selected_text(p.restrictions.clone()).width(200.0).show_ui(
            ui,
            |ui| {
                for name in presets {
                    let checked = name == p.restrictions;
                    if ui.selectable_label(checked, &name).clicked() {
                        p.restrictions = name;
                    }
                }
            },
        );
        ui.end_row();
    });
    ui.checkbox(&mut p.bonuses, "Bonuses");
}

fn level_name(l: BotLevel) -> &'static str {
    match l {
        BotLevel::Easy => "Easy",
        BotLevel::Medium => "Medium",
        BotLevel::Hard => "Hard",
        BotLevel::Xtreme => "Xtreme",
    }
}

fn level_combo(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, level: &mut BotLevel) {
    ComboBox::from_id_salt(id).selected_text(level_name(*level)).show_ui(ui, |ui| {
        for l in BotLevel::ALL {
            ui.selectable_value(level, l, level_name(l));
        }
    });
}

/// The bots (also the options' default match).
pub(super) fn bots(ui: &mut Ui, p: &mut MatchPrefs) {
    ui.label(RichText::new("Bots").strong());
    let mut n = p.bots;
    if ui.add(Slider::new(&mut n, 1..=MAX_BOTS).text("bots")).changed() {
        p.set_bots(n);
    }
    let mut each = p.per_bot.is_some();
    if ui.checkbox(&mut each, "Each bot its own difficulty").changed() {
        p.set_per_bot(each);
    }
    egui::Grid::new("bots").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        match &mut p.per_bot {
            None => {
                ui.label("Difficulty");
                level_combo(ui, "difficulty", &mut p.difficulty);
                ui.end_row();
            }
            Some(levels) => {
                for (i, l) in levels.iter_mut().enumerate() {
                    ui.label(format!("Bot {}", i + 1));
                    level_combo(ui, ("bot", i), l);
                    ui.end_row();
                }
            }
        }
        ui.label("Personality");
        let name = |b: BotPersonality| match b {
            BotPersonality::Olx => "OLX bot",
            BotPersonality::Chaos => "Chaos (fires everything)",
        };
        ComboBox::from_id_salt("personality").selected_text(name(p.personality)).show_ui(ui, |ui| {
            for b in BotPersonality::ALL {
                ui.selectable_value(&mut p.personality, b, name(b));
            }
        });
        ui.end_row();
        ui.label("Loadout");
        let name = |b: BotLoadout| match b {
            BotLoadout::OlxPreset => "OLX's choice",
            BotLoadout::Random => "Random",
        };
        ComboBox::from_id_salt("loadout").selected_text(name(p.loadout)).show_ui(ui, |ui| {
            for b in [BotLoadout::OlxPreset, BotLoadout::Random] {
                ui.selectable_value(&mut p.loadout, b, name(b));
            }
        });
        ui.end_row();
    });
}
