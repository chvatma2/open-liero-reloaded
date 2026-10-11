//! OLX's weapon selection over the level before the match (spec §21 G4,
//! `crate::weapon_select`): the five slots, Random and Done, driven by
//! the game's keys (the app feeds them in `MenuData::held`: keyboard or
//! gamepad as bound) or the mouse. Done arms the player; Esc leaves. The
//! slots scroll on a small screen, so Random and Done stay on it.

use egui::{Align2, Button, Color32, RichText, Ui};
use rl_sim::MAX_SLOTS;

use super::MenuData;
use crate::state::UiAction;
use crate::weapon_select::{DONE_ROW, RANDOM_ROW};

/// OLX's `clWeaponSelectionActive` and `clWeaponSelectionDefault`
/// (`data/frontend/colours.cfg`): the row under the cursor stands out.
const ACTIVE: Color32 = Color32::from_rgb(255, 255, 255);
const DEFAULT: Color32 = Color32::from_rgb(150, 150, 150);
const TITLE: Color32 = Color32::from_rgb(0x02, 0xbf, 0xf6);

/// The room the panel needs besides the slots, in points: its margins,
/// Random and Done.
const AROUND_SLOTS: f32 = 110.0;

pub(super) fn show(ui: &mut Ui, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let (Some(menu), Some(mut sel)) = (data.weapons.clone(), data.select.take()) else {
        return;
    };
    let now = ui.input(|i| i.time);
    let row_before = sel.row();
    let mut done = sel.on_keys(&menu, data.held, now);
    let moved = sel.row() != row_before;
    // The slots scroll on a small screen: Random and Done stay on it.
    let room = (ui.ctx().content_rect().height() - AROUND_SLOTS).max(40.0);
    let frame = egui::Frame::new().fill(Color32::from_black_alpha(215)).corner_radius(6.0).inner_margin(18.0);
    egui::Area::new(egui::Id::new("weapon selection"))
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
                        ui.heading(RichText::new("Weapons").color(TITLE));
                        ui.label(RichText::new("Up and down choose a row, left and right change it.").weak());
                        ui.label(RichText::new("Go to Done and press Fire, or click.").weak());
                        ui.add_space(8.0);
                        egui::Grid::new("weapon slots").spacing([8.0, 4.0]).show(ui, |ui| {
                            for slot in 0..MAX_SLOTS {
                                let colour = if sel.row() == slot { ACTIVE } else { DEFAULT };
                                if ui.small_button("◀").clicked() {
                                    sel.set_row(slot);
                                    sel.change(&menu, -1);
                                }
                                let name =
                                    RichText::new(menu.name(sel.slots()[slot])).size(18.0).color(colour);
                                let r = ui.add_sized([220.0, 24.0], Button::new(name).frame(false));
                                if r.clicked() {
                                    sel.set_row(slot);
                                }
                                if moved && sel.row() == slot {
                                    r.scroll_to_me(None);
                                }
                                if ui.small_button("▶").clicked() {
                                    sel.set_row(slot);
                                    sel.change(&menu, 1);
                                }
                                ui.end_row();
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let row = |ui: &mut Ui, label: &str, active: bool| {
                        let colour = if active { ACTIVE } else { DEFAULT };
                        ui.add_sized(
                            [160.0, 28.0],
                            Button::new(RichText::new(label).size(18.0).color(colour)),
                        )
                        .clicked()
                    };
                    if row(ui, "Random", sel.row() == RANDOM_ROW) {
                        sel.set_row(RANDOM_ROW);
                        sel.random(&menu);
                    }
                    if row(ui, "Done", sel.row() == DONE_ROW) {
                        done = true;
                    }
                });
            });
        });
    if done {
        let weapons = sel.slots();
        let names = weapons.iter().map(|&w| menu.name(w).to_string()).collect();
        out.push(UiAction::ChooseWeapons { weapons, names });
    }
    data.select = Some(sel);
}
