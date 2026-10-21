//! The chosen node's panel: the world (name, notes, worm, rope, presets
//! and loadouts), a weapon (its fields and fire), a spawn (its fields and
//! projectile: its own or a shared one), a projectile (look, motion,
//! trail, timer and its events) and a timer (its times and event). Each
//! shows its common fields first and its rare ones under More ▸.

use egui::Ui;
use olr_data::restrictions::{ShippedLoadout, WeaponState};
use olr_edit::mods::path::Seg;
use olr_format::mods::{Gfx, Projectile};

use super::field::{self, has_more, section};
use super::{Cx, caption, labels, relabel};
use crate::fields::{self, Place};
use crate::state::{Event, ModAction, PIXEL, get_as, ron};
use crate::tree::{NodeKind, SpawnAt, node_of};

pub(super) fn show(ui: &mut Ui, cx: &mut Cx) {
    let nodes = cx.state.nodes();
    let path = cx.state.node().to_string();
    let Some(node) = nodes.iter().find(|n| n.path == path).cloned() else {
        ui.weak("Nothing to show.");
        return;
    };
    here(ui, cx, &path, &nodes);
    match &node.kind {
        NodeKind::World => world(ui, cx),
        NodeKind::Weapon => weapon(ui, cx, &path),
        NodeKind::Spawn(at) => spawn(ui, cx, &path, *at),
        NodeKind::Projectile { shared } => projectile(ui, cx, &path, shared.as_deref()),
        NodeKind::Timer => timer(ui, cx, &path),
    }
}

/// The problems of the node shown, above its panel.
fn here(ui: &mut Ui, cx: &mut Cx, path: &str, nodes: &[crate::tree::Node]) {
    let mine: Vec<_> = cx
        .state
        .issues()
        .iter()
        .filter(|i| i.path.as_deref().and_then(|p| node_of(nodes, p)).is_some_and(|n| n.path == path))
        .collect();
    for i in mine {
        let colour = if i.is_error() { ui.visuals().error_fg_color } else { ui.visuals().warn_fg_color };
        ui.colored_label(colour, format!("⚠ {}", i.message));
    }
}

/// The More ▸ toggle of panel `path`, and whether it is open (a problem's
/// link into the panel opens it).
fn more(ui: &mut Ui, cx: &mut Cx, path: &str) -> bool {
    let focused = cx.state.focus().is_some_and(|f| f.starts_with(path));
    let open = cx.data.more.contains(path) || focused;
    let r = ui.selectable_label(open, if open { "More ▾" } else { "More ▸" });
    relabel(&r, egui::WidgetType::Button, labels::MORE);
    if r.clicked() && !focused {
        if open {
            cx.data.more.remove(path);
        } else {
            cx.data.more.insert(path.to_string());
        }
    }
    open
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.strong(text);
    ui.separator();
}

/// A link to node `to`, named `label`.
fn link(ui: &mut Ui, cx: &mut Cx, label: &str, to: String) {
    let r = ui.link(format!("{label} ▸"));
    relabel(&r, egui::WidgetType::Link, label);
    if r.clicked() {
        cx.out.push(ModAction::Node(to));
    }
}

fn world(ui: &mut Ui, cx: &mut Cx) {
    ui.heading("World settings");
    section(ui, cx, "ModFile", "mod", "", Place::Common);
    heading(ui, "Worm");
    section(ui, cx, "Worm", "mod.worm", "", Place::Common);
    heading(ui, "Rope");
    section(ui, cx, "Rope", "mod.rope", "", Place::Common);
    if more(ui, cx, "mod") {
        section(ui, cx, "Worm", "mod.worm", "", Place::More);
        section(ui, cx, "Rope", "mod.rope", "", Place::More);
    }
    presets(ui, cx);
}

/// The restriction presets: the one chosen as a table of the weapons, and
/// its shipped loadout.
fn presets(ui: &mut Ui, cx: &mut Cx) {
    heading(ui, "Weapon restrictions");
    let state = cx.state;
    let src = state.src();
    let shown = state.preset().map(String::from);
    ui.horizontal(|ui| {
        caption(ui, labels::PRESET);
        let r = egui::ComboBox::from_id_salt("olr-mod-editor-preset")
            .selected_text(shown.clone().unwrap_or_else(|| "(none)".into()))
            .show_ui(ui, |ui| {
                for name in src.restrictions.keys() {
                    if ui.selectable_label(Some(name) == shown.as_ref(), name).clicked() {
                        cx.out.push(ModAction::Preset(name.clone()));
                    }
                }
            })
            .response;
        relabel(&r, egui::WidgetType::ComboBox, labels::PRESET);
        if let Some(p) = &shown {
            let rm = ui.small_button("Remove");
            relabel(&rm, egui::WidgetType::Button, labels::REMOVE_PRESET);
            if rm.clicked() {
                cx.out.push(ModAction::RemovePreset(p.clone()));
            }
        }
    });
    ui.horizontal(|ui| {
        let r = ui
            .add(egui::TextEdit::singleline(&mut cx.data.new_preset).desired_width(160.0).hint_text("Name"));
        relabel(&r, egui::WidgetType::TextEdit, labels::NEW_PRESET);
        let ok = !cx.data.new_preset.trim().is_empty();
        if ui.add_enabled(ok, egui::Button::new(labels::ADD_PRESET)).clicked() {
            cx.out.push(ModAction::AddPreset(std::mem::take(&mut cx.data.new_preset)));
        }
    });
    let Some(preset) = shown else { return };
    let entries = &src.restrictions[&preset];
    egui::Grid::new("olr-mod-editor-preset-table").num_columns(4).striped(true).show(ui, |ui| {
        for (_, w) in &src.weapons {
            let state_of = entries
                .iter()
                .find(|(k, _)| crate::state::same_name(k, &w.name))
                .map_or(WeaponState::Enabled, |e| *e.1);
            caption(ui, &w.name).on_hover_text("Weapons are matched by name, as OLX does");
            for (s, text) in [
                (WeaponState::Enabled, "Enabled"),
                (WeaponState::Bonus, "Bonus"),
                (WeaponState::Banned, "Banned"),
            ] {
                let r = ui.selectable_label(state_of == s, text);
                relabel(&r, egui::WidgetType::Button, &labels::preset_state(&w.name, text));
                if r.clicked() && state_of != s {
                    cx.out.push(ModAction::PresetState {
                        preset: preset.clone(),
                        weapon: w.name.clone(),
                        state: s,
                    });
                }
            }
            ui.end_row();
        }
    });
    // Its loadout.
    ui.add_space(6.0);
    let loadout = src.loadouts.get(&preset).copied();
    let mut on = loadout.is_some();
    let r = ui
        .checkbox(&mut on, labels::LOADOUT)
        .on_hover_text(fields::get("ModFile.loadouts").map_or("", |f| f.help.as_str()));
    if r.changed() {
        let l = on.then_some(ShippedLoadout { loading_time: 100, unique: false });
        cx.out.push(ModAction::Loadout { preset: preset.clone(), loadout: l });
    }
    if let Some(l) = loadout {
        ui.horizontal(|ui| {
            caption(ui, labels::LOADING_TIME);
            let mut t = l.loading_time;
            let r = ui.add(egui::DragValue::new(&mut t).range(0..=500));
            relabel(&r, egui::WidgetType::DragValue, labels::LOADING_TIME);
            if let Some(f) = fields::get("ShippedLoadout.loading_time") {
                r.clone().on_hover_text(&f.help);
            }
            if (r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged())) && t != l.loading_time {
                cx.commits.push(ModAction::Loadout {
                    preset: preset.clone(),
                    loadout: Some(ShippedLoadout { loading_time: t, ..l }),
                });
            }
            let mut u = l.unique;
            if ui.checkbox(&mut u, labels::UNIQUE).changed() {
                cx.out.push(ModAction::Loadout {
                    preset: preset.clone(),
                    loadout: Some(ShippedLoadout { unique: u, ..l }),
                });
            }
        });
    }
}

fn weapon(ui: &mut Ui, cx: &mut Cx, path: &str) {
    let id = path.strip_prefix("weapons/").unwrap_or(path).to_string();
    let index = cx.state.src().weapons.iter().position(|(w, _)| *w == id).unwrap_or(0);
    ui.heading(format!("Weapon {} ({})", index + 1, id));
    section(ui, cx, "Weapon", path, "", Place::Common);
    heading(ui, "Firing");
    let fire = format!("{path}.fire");
    let kind = cx.state.variant(&fire).unwrap_or_default();
    ui.horizontal(|ui| {
        caption(ui, labels::FIRE);
        let r = egui::ComboBox::from_id_salt("olr-mod-editor-fire")
            .selected_text(&kind)
            .show_ui(ui, |ui| {
                for k in ["Projectile", "Beam", "Special"] {
                    let r = ui.selectable_label(k == kind, k);
                    let r = match fields::variant_help("Fire", k) {
                        Some(h) => r.on_hover_text(h),
                        None => r,
                    };
                    if r.clicked() && k != kind {
                        cx.out.push(ModAction::FireKind { weapon: id.clone(), kind: k.into() });
                    }
                }
            })
            .response;
        relabel(&r, egui::WidgetType::ComboBox, labels::FIRE);
    });
    let owner = format!("Fire::{kind}");
    section(ui, cx, &owner, &fire, "", Place::Common);
    if kind == "Projectile" {
        link(ui, cx, labels::GO_SHOT, format!("{fire}.shot"));
    }
    if (has_more("Weapon") || has_more(&owner)) && more(ui, cx, path) {
        section(ui, cx, "Weapon", path, "", Place::More);
        section(ui, cx, &owner, &fire, "", Place::More);
    }
}

fn spawn(ui: &mut Ui, cx: &mut Cx, path: &str, at: SpawnAt) {
    ui.heading(match at {
        SpawnAt::Shot => "Shot",
        SpawnAt::Event => "Spawn",
        SpawnAt::Trail => "Trail's spawn",
    });
    ui.weak(match at {
        SpawnAt::Shot => "What one shot fires.",
        SpawnAt::Event => "What the projectile's events spawn.",
        SpawnAt::Trail => "What the projectile leaves behind as it flies.",
    });
    section(ui, cx, "Spawn", path, "", Place::Common);
    heading(ui, "What it spawns");
    let shared: Option<String> =
        get_as::<Option<String>>(cx.state.src(), &format!("{path}.shared")).flatten();
    let ids: Vec<String> = cx.state.src().shared.keys().cloned().collect();
    ui.horizontal(|ui| {
        caption(ui, labels::SPAWNED);
        let r = egui::ComboBox::from_id_salt("olr-mod-editor-spawned")
            .selected_text(shared.clone().map_or(labels::OWN.to_string(), |id| format!("Shared: {id}")))
            .width(240.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(shared.is_none(), labels::OWN).clicked() && shared.is_some() {
                    cx.out.push(ModAction::MakeCopy(path.to_string()));
                }
                for id in &ids {
                    if ui.selectable_label(shared.as_ref() == Some(id), format!("Shared: {id}")).clicked()
                        && shared.as_ref() != Some(id)
                    {
                        cx.out.push(ModAction::SpawnShared { spawn: path.to_string(), shared: id.clone() });
                    }
                }
            })
            .response;
        relabel(&r, egui::WidgetType::ComboBox, labels::SPAWNED);
    });
    ui.horizontal(|ui| {
        if shared.is_some() {
            if ui
                .button(labels::MAKE_COPY)
                .on_hover_text("This spawn gets its own copy: changes to it change no other weapon")
                .clicked()
            {
                cx.out.push(ModAction::MakeCopy(path.to_string()));
            }
        } else if ui
            .button(labels::SHARE)
            .on_hover_text("The projectile becomes a shared one other spawns may use")
            .clicked()
        {
            cx.out.push(ModAction::Share(path.to_string()));
        }
        link(ui, cx, labels::GO_PROJECTILE, format!("{path}.projectile"));
    });
    if more(ui, cx, path) {
        section(ui, cx, "Spawn", path, "", Place::More);
    }
}

fn projectile(ui: &mut Ui, cx: &mut Cx, path: &str, shared: Option<&str>) {
    ui.heading("Projectile");
    if let Some(id) = shared {
        let own = path == format!("projectiles/{id}");
        ui.horizontal(|ui| {
            ui.label(format!("Shared projectile {id}: a change here changes it for every user."));
            if !own
                && let Some(spawn) = path.strip_suffix(".projectile")
                && ui.button(labels::MAKE_COPY).clicked()
            {
                cx.out.push(ModAction::MakeCopy(spawn.to_string()));
            }
        });
        let users = cx.state.used_by().remove(id).unwrap_or_default();
        ui.label(format!("{} ({})", labels::USED_BY, users.len()));
        for u in users {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let r = ui.link(&u);
                relabel(&r, egui::WidgetType::Link, &labels::used_by(&u));
                if r.clicked() {
                    cx.out.push(ModAction::GoTo(u.clone()));
                }
            });
        }
    }
    let Some(p) = get_as::<Projectile>(cx.state.src(), path) else { return };
    heading(ui, "Look");
    gfx(ui, cx, path, &p);
    heading(ui, "Motion");
    section(ui, cx, "Projectile", path, "", Place::Common);
    trail(ui, cx, path);
    heading(ui, "Events");
    let mut has_timer = p.timer.is_some();
    ui.horizontal(|ui| {
        let r = ui.checkbox(&mut has_timer, labels::HAS_TIMER);
        if let Some(f) = fields::get("Projectile.timer") {
            r.clone().on_hover_text(&f.help);
        }
        if r.changed() {
            cx.out.push(ModAction::Timer { projectile: path.to_string(), on: has_timer });
        }
        if p.timer.is_some() {
            link(ui, cx, labels::GO_TIMER, format!("{path}.timer"));
        }
    });
    event(ui, cx, path, Event::HitTerrain);
    event(ui, cx, path, Event::HitWorm);
    if more(ui, cx, path) {
        section(ui, cx, "Projectile", path, "", Place::More);
        section(ui, cx, "Gfx::Image", &format!("{path}.gfx"), "", Place::More);
    }
}

/// The projectile's image or pixels.
fn gfx(ui: &mut Ui, cx: &mut Cx, path: &str, p: &Projectile) {
    let at = format!("{path}.gfx");
    let image = matches!(p.gfx, Gfx::Image { .. });
    ui.horizontal(|ui| {
        caption(ui, labels::GFX);
        let mut chosen = None;
        let r = egui::ComboBox::from_id_salt("olr-mod-editor-gfx")
            .selected_text(if image { "Image" } else { "Pixels" })
            .show_ui(ui, |ui| {
                if ui.selectable_label(!image, "Pixels").clicked() && image {
                    chosen = Some(PIXEL.to_string());
                }
                if ui.selectable_label(image, "Image").clicked() && !image {
                    let file = cx.state.assets("gfx").into_iter().next().unwrap_or_default();
                    chosen = Some(format!("Image(file: {})", ron(&file)));
                }
            })
            .response;
        relabel(&r, egui::WidgetType::ComboBox, labels::GFX);
        if let Some(v) = chosen {
            cx.out.push(ModAction::Set { path: at.clone(), value: v });
        }
    });
    match &p.gfx {
        Gfx::Pixels(colours) => {
            let shown = colours.iter().map(|c| c.to_hex()).collect::<Vec<_>>().join(", ");
            ui.horizontal(|ui| {
                for c in colours.iter().take(16) {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(c.0.r, c.0.g, c.0.b));
                }
            });
            ui.horizontal(|ui| {
                caption(ui, labels::COLOURS);
                let key = format!("{at}.0");
                let mut t = cx.data.drafts.get(&key).cloned().unwrap_or_else(|| shown.clone());
                let r = ui.add(egui::TextEdit::singleline(&mut t).desired_width(320.0));
                relabel(&r, egui::WidgetType::TextEdit, labels::COLOURS);
                if let Some(f) = fields::get("Gfx::Pixels.0") {
                    r.clone().on_hover_text(&f.help);
                }
                if r.changed() {
                    cx.data.drafts.insert(key.clone(), t);
                }
                if r.lost_focus()
                    && let Some(d) = cx.data.drafts.remove(&key)
                    && d.trim() != shown
                {
                    let list: Vec<String> =
                        d.split([',', ' ']).filter(|s| !s.is_empty()).map(|s| ron(&s.to_string())).collect();
                    cx.commits.push(ModAction::Set {
                        path: at.clone(),
                        value: format!("Pixels([{}])", list.join(", ")),
                    });
                }
            });
            field::original(ui, cx, &at, labels::GFX);
        }
        Gfx::Image { animation, .. } => {
            section(ui, cx, "Gfx::Image", &at, "", Place::Common);
            let mut on = animation.is_some();
            let r = ui.checkbox(&mut on, labels::ANIMATED);
            if let Some(f) = fields::get("Gfx::Image.animation") {
                r.clone().on_hover_text(&f.help);
            }
            if r.changed() {
                cx.out.push(ModAction::Animation { gfx: at.clone(), on });
            }
            if animation.is_some() {
                section(ui, cx, "Animation", &format!("{at}.animation"), "", Place::Common);
            }
        }
    }
}

fn trail(ui: &mut Ui, cx: &mut Cx, path: &str) {
    let at = format!("{path}.trail");
    let kind = cx.state.variant(&at).unwrap_or_default();
    ui.horizontal(|ui| {
        caption(ui, labels::TRAIL);
        let mut chosen = None;
        let r = egui::ComboBox::from_id_salt("olr-mod-editor-trail")
            .selected_text(&kind)
            .show_ui(ui, |ui| {
                for k in ["Off", "Smoke", "ChemSmoke", "Doomsday", "Explosive", "Projectile"] {
                    let r = ui.selectable_label(k == kind, k);
                    let r = match fields::variant_help("Trail", k) {
                        Some(h) => r.on_hover_text(h),
                        None => r,
                    };
                    if r.clicked() && k != kind {
                        // A variant set by name starts valid (`olr_edit::mods::set`).
                        chosen = Some(k.to_string());
                    }
                }
            })
            .response;
        relabel(&r, egui::WidgetType::ComboBox, labels::TRAIL);
        if let Some(v) = chosen {
            cx.out.push(ModAction::Set { path: at.clone(), value: v });
        }
    });
    if kind == "Projectile" {
        section(ui, cx, "Trail::Projectile", &at, "Trail: ", Place::Common);
        link(ui, cx, labels::GO_TRAIL, format!("{at}.spawn"));
    }
}

fn timer(ui: &mut Ui, cx: &mut Cx, path: &str) {
    ui.heading("Timer");
    section(ui, cx, "Timer", path, "", Place::Common);
    let projectile = path.strip_suffix(".timer").unwrap_or(path).to_string();
    event(ui, cx, &projectile, Event::Timer);
}

/// An event: one effect, with its fields, and whether it spawns.
fn event(ui: &mut Ui, cx: &mut Cx, projectile: &str, e: Event) {
    let Some(view) = cx.state.event(projectile, e) else { return };
    heading(ui, e.name());
    let current = view.effect.as_ref().map(|(_, eff)| variant_name(eff));
    let name = labels::event(e.name(), "Effect");
    ui.horizontal(|ui| {
        caption(ui, "Effect");
        let r = egui::ComboBox::from_id_salt(("olr-mod-editor-effect", e.name()))
            .selected_text(current.clone().unwrap_or_else(|| "(none)".into()))
            .width(200.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "(none)").clicked() && current.is_some() {
                    cx.out.push(ModAction::Effect {
                        projectile: projectile.to_string(),
                        event: e,
                        effect: None,
                    });
                }
                for n in crate::state::effect_names() {
                    let r = ui.selectable_label(current.as_deref() == Some(n), n);
                    let r = match fields::variant_help("Effect", n) {
                        Some(h) => r.on_hover_text(h),
                        None => r,
                    };
                    if r.clicked() && current.as_deref() != Some(n) {
                        cx.out.push(ModAction::Effect {
                            projectile: projectile.to_string(),
                            event: e,
                            effect: Some(n.to_string()),
                        });
                    }
                }
            })
            .response;
        relabel(&r, egui::WidgetType::ComboBox, &name);
    });
    if let (Some((i, _)), Some(n)) = (&view.effect, &current) {
        let base = format!("{}[{i}]", e.list(projectile));
        section(ui, cx, &format!("Effect::{n}"), &base, &labels::event(e.name(), ""), Place::Common);
    }
    if view.extra > 0 {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("and {} more effect(s): LX56 runs one an event", view.extra),
        );
    }
    ui.horizontal(|ui| {
        let mut on = view.spawn.is_some();
        let r = ui.checkbox(&mut on, "Spawns");
        relabel(&r, egui::WidgetType::Checkbox, &labels::event(e.name(), "Spawns"));
        if r.changed() {
            cx.out.push(ModAction::EventSpawn { projectile: projectile.to_string(), event: e, on });
        }
        if let Some(key) = &view.spawn {
            let to = format!("{projectile}.spawns{}", Seg::Name(key.clone()));
            let r = ui.link("Edit the spawn ▸");
            relabel(&r, egui::WidgetType::Link, &labels::event(e.name(), "Edit the spawn"));
            if r.clicked() {
                cx.out.push(ModAction::Node(to));
            }
        }
    });
}

/// An effect's variant name.
fn variant_name(e: &olr_format::mods::Effect) -> String {
    let text = ron(e);
    text.split(['(', ' ']).next().unwrap_or_default().to_string()
}
