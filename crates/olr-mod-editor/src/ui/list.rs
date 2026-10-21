//! The left list (World, the weapons in order with their operations, the
//! shared projectiles) and the item's projectile tree.

use egui::Ui;

use super::{Cx, labels, relabel};
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
