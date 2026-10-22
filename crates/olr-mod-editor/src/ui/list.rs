//! The left list (World, the weapons in order with their operations, the
//! shared projectiles; issue #19's Art, the images with their thumbnails
//! and users' count, and Sounds) and the item's projectile tree.

use egui::Ui;

use super::{Cx, draw_thumb, labels, relabel, thumb};
use crate::browser::Import;
use crate::state::ModAction;
use crate::tree::Item;

pub(super) fn items(ui: &mut Ui, cx: &mut Cx) {
    let state = cx.state;
    let src = state.src();
    if ui.selectable_label(*state.item() == Item::World, labels::WORLD).clicked() {
        cx.out.push(ModAction::Select(Item::World));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong("Weapons");
        if ui.small_button(labels::ADD_WEAPON).clicked() {
            cx.out.push(ModAction::AddWeapon);
        }
    });
    let last = src.weapons.len().saturating_sub(1);
    for (i, (id, w)) in src.weapons.iter().enumerate() {
        let chosen = *state.item() == Item::Weapon(id.clone());
        let r =
            ui.selectable_label(chosen, &w.name).on_hover_text(format!("weapons/{id}.ron, weapon {}", i + 1));
        if r.clicked() {
            cx.out.push(ModAction::Select(Item::Weapon(id.clone())));
        }
        if chosen {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let up = ui.add_enabled(i > 0, egui::Button::new("▲").small());
                relabel(&up, egui::WidgetType::Button, labels::MOVE_UP);
                if up.on_hover_text(labels::MOVE_UP).clicked() {
                    cx.out.push(ModAction::MoveWeapon { id: id.clone(), by: -1 });
                }
                let down = ui.add_enabled(i < last, egui::Button::new("▼").small());
                relabel(&down, egui::WidgetType::Button, labels::MOVE_DOWN);
                if down.on_hover_text(labels::MOVE_DOWN).clicked() {
                    cx.out.push(ModAction::MoveWeapon { id: id.clone(), by: 1 });
                }
                let dup = ui.small_button("Duplicate");
                relabel(&dup, egui::WidgetType::Button, labels::DUPLICATE);
                if dup.clicked() {
                    cx.out.push(ModAction::DuplicateWeapon(id.clone()));
                }
                let rm = ui.small_button("Remove");
                relabel(&rm, egui::WidgetType::Button, labels::REMOVE);
                if rm.clicked() {
                    cx.out.push(ModAction::RemoveWeapon(id.clone()));
                }
            });
        }
    }
    ui.add_space(6.0);
    ui.strong("Shared projectiles");
    if src.shared.is_empty() {
        ui.weak("None");
    }
    for id in src.shared.keys() {
        let chosen = *state.item() == Item::Shared(id.clone());
        if ui.selectable_label(chosen, id).on_hover_text(format!("projectiles/{id}.ron")).clicked() {
            cx.out.push(ModAction::Select(Item::Shared(id.clone())));
        }
    }
    art(ui, cx);
}

/// The Art section: New image, Import PNG…, and the images, each with its
/// thumbnail and how many fields use it; then Sounds.
fn art(ui: &mut Ui, cx: &mut Cx) {
    let state = cx.state;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong(labels::ART);
        let r = ui.small_button("New…");
        relabel(&r, egui::WidgetType::Button, labels::NEW_IMAGE);
        if r.on_hover_text("A blank image of a size").clicked() {
            cx.out.push(ModAction::AskNewImage(None));
        }
        let r = ui.small_button("Import…");
        relabel(&r, egui::WidgetType::Button, labels::IMPORT_PNG);
        if r.on_hover_text("A PNG strip of square frames").clicked() {
            cx.out.push(ModAction::Import(Import::Image));
        }
    });
    let images = state.assets("gfx");
    if images.is_empty() {
        ui.weak("None");
    }
    for path in images {
        let chosen = *state.item() == Item::Art(path.clone());
        let users = state.asset_users(&path).len();
        ui.horizontal(|ui| {
            match thumb(ui, cx, &path) {
                Some(t) => draw_thumb(ui, &t, 18.0),
                None => {
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                }
            }
            let name = path.strip_prefix("gfx/").unwrap_or(&path);
            let r = ui.selectable_label(chosen, name).on_hover_text(match users {
                0 => "not used".to_string(),
                n => format!("used by {n} field{}", if n == 1 { "" } else { "s" }),
            });
            relabel(&r, egui::WidgetType::Button, &labels::image(&path));
            if r.clicked() {
                cx.out.push(ModAction::Select(Item::Art(path.clone())));
            }
            if users == 0 {
                ui.weak("unused");
            }
        });
    }
    ui.add_space(6.0);
    let n = state.assets("sfx").len();
    let r = ui.selectable_label(*state.item() == Item::Sounds, format!("{} ({n})", labels::SOUNDS));
    relabel(&r, egui::WidgetType::Button, labels::SOUNDS);
    if r.clicked() {
        cx.out.push(ModAction::Select(Item::Sounds));
    }
}

/// The item's nodes, indented by depth; the one shown is marked.
pub(super) fn tree(ui: &mut Ui, cx: &mut Cx) {
    ui.strong("Projectiles");
    ui.weak("Click a part to edit it.");
    ui.add_space(4.0);
    for n in cx.state.nodes() {
        ui.horizontal(|ui| {
            ui.add_space(n.depth as f32 * 14.0);
            let r = ui.selectable_label(n.path == cx.state.node(), &n.label).on_hover_text(&n.path);
            relabel(&r, egui::WidgetType::Button, &labels::node(&n.label));
            if r.clicked() {
                cx.out.push(ModAction::Node(n.path.clone()));
            }
        });
    }
}
