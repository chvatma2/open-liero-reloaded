//! OLX's weapon selection over the level before the match (spec §21 G4,
//! `crate::weapon_select`): the five slots, Random and Done, driven by
//! the game's keys (the app feeds them in `MenuData::held`: keyboard or
//! gamepad as bound) or the mouse. Done arms the player; Esc leaves. The
//! slots scroll on a small screen, so Random and Done stay on it.
//!
//! Several local players (issue #32) choose at once, each in a panel over
//! their own view (split screen's layout) driven by their own keys
//! (`MenuData::held_by`); the mouse acts on keyboard 1's player's panel
//! alone. A player done waits for the others; the match plays when all
//! are.

use egui::{Align2, Button, Color32, Rect, RichText, Ui};
use olr_sim::{Keys, MAX_SLOTS};

use super::MenuData;
use crate::hud::SplitLayout;
use crate::state::{UiAction, UiState};
use crate::weapon_select::{DONE_ROW, RANDOM_ROW, WeaponMenu, WeaponSelect};

/// OLX's `clWeaponSelectionActive` and `clWeaponSelectionDefault`
/// (`data/frontend/colours.cfg`): the row under the cursor stands out.
const ACTIVE: Color32 = Color32::from_rgb(255, 255, 255);
const DEFAULT: Color32 = Color32::from_rgb(150, 150, 150);
const TITLE: Color32 = Color32::from_rgb(0x02, 0xbf, 0xf6);

/// The room the panel needs besides the slots, in points: its margins,
/// Random and Done.
const AROUND_SLOTS: f32 = 110.0;

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let Some(menu) = data.weapons.clone() else { return };
    let mut selects = std::mem::take(&mut data.selects);
    let now = ui.input(|i| i.time);
    let ctx = ui.ctx().clone();
    let n = selects.len();
    // One player: over the playfield, as before; several (issue #32):
    // each over their own view, as split screen lays them out.
    let screen = ctx.content_rect();
    let areas =
        if n > 1 { SplitLayout::new(screen, n).views().to_vec() } else { vec![super::playfield(&ctx).0] };
    // The mouse is keyboard 1's: it picks in that player's panel alone.
    let mouse = if n > 1 { state.mouse_player() } else { Some(0) };
    for (k, sel) in selects.iter_mut().enumerate() {
        let Some(&area) = areas.get(k) else { break };
        if n > 1 && state.has_chosen(k) {
            waiting(&ctx, k, area, screen, &state.player_name(k));
            continue;
        }
        let held = if n > 1 { data.held_by.get(k).copied().unwrap_or_default() } else { data.held };
        let title = if n > 1 { state.player_name(k) } else { "Weapons".to_string() };
        let at = Panel { k, area, screen, title, clickable: mouse == Some(k), hints: n == 1 };
        if panel(&ctx, &menu, sel, held, now, &at) {
            let weapons = sel.slots();
            let names = weapons.iter().map(|&w| menu.name(w).to_string()).collect();
            out.push(if n > 1 {
                UiAction::ChooseWeaponsOf { player: k, weapons, names }
            } else {
                UiAction::ChooseWeapons { weapons, names }
            });
        }
    }
    data.selects = selects;
}

/// Where and how a player's panel shows.
struct Panel {
    /// The local player.
    k: usize,
    /// Their view (the playfield for one player) and the window.
    area: Rect,
    screen: Rect,
    title: String,
    /// The mouse acts on it (keyboard 1's player, or the one player).
    clickable: bool,
    /// The two lines of help (one player: the room is there).
    hints: bool,
}

/// One player's selection, driven by `held` (that player's keys) at
/// `now`, and by the mouse when `at.clickable`: whether it is done.
fn panel(
    ctx: &egui::Context,
    menu: &WeaponMenu,
    sel: &mut WeaponSelect,
    held: Keys,
    now: f64,
    at: &Panel,
) -> bool {
    let row_before = sel.row();
    let mut done = sel.on_keys(menu, held, now);
    let moved = sel.row() != row_before;
    // The slots scroll on a small screen: Random and Done stay on it.
    let room = (at.area.height() - AROUND_SLOTS).max(40.0);
    let offset = at.area.center() - at.screen.center();
    let frame = egui::Frame::new().fill(Color32::from_black_alpha(215)).corner_radius(6.0).inner_margin(18.0);
    egui::Area::new(egui::Id::new(("weapon selection", at.k))).anchor(Align2::CENTER_CENTER, offset).show(
        ctx,
        |ui| {
            frame.show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
                        ui.heading(RichText::new(&at.title).color(TITLE));
                        if at.hints {
                            ui.label(
                                RichText::new("Up and down choose a row, left and right change it.").weak(),
                            );
                            ui.label(RichText::new("Go to Done and press Fire, or click.").weak());
                        }
                        ui.add_space(8.0);
                        egui::Grid::new(("weapon slots", at.k)).spacing([8.0, 4.0]).show(ui, |ui| {
                            for slot in 0..MAX_SLOTS {
                                let colour = if sel.row() == slot { ACTIVE } else { DEFAULT };
                                let name =
                                    RichText::new(menu.name(sel.slots()[slot])).size(18.0).color(colour);
                                if !at.clickable {
                                    let r = ui.add_sized([220.0, 24.0], egui::Label::new(name));
                                    if moved && sel.row() == slot {
                                        r.scroll_to_me(None);
                                    }
                                    ui.end_row();
                                    continue;
                                }
                                if ui.small_button("◀").clicked() {
                                    sel.set_row(slot);
                                    sel.change(menu, -1);
                                }
                                let r = ui.add_sized([220.0, 24.0], Button::new(name).frame(false));
                                if r.clicked() {
                                    sel.set_row(slot);
                                }
                                if moved && sel.row() == slot {
                                    r.scroll_to_me(None);
                                }
                                if ui.small_button("▶").clicked() {
                                    sel.set_row(slot);
                                    sel.change(menu, 1);
                                }
                                ui.end_row();
                            }
                        });
                        crate::pad::follow_focus(ui);
                    });
                    ui.add_space(6.0);
                    let row = |ui: &mut Ui, label: &str, active: bool| {
                        let colour = if active { ACTIVE } else { DEFAULT };
                        let text = RichText::new(label).size(18.0).color(colour);
                        if at.clickable {
                            ui.add_sized([160.0, 28.0], Button::new(text)).clicked()
                        } else {
                            ui.add_sized([160.0, 28.0], egui::Label::new(text));
                            false
                        }
                    };
                    if row(ui, "Random", sel.row() == RANDOM_ROW) {
                        sel.set_row(RANDOM_ROW);
                        sel.random(menu);
                    }
                    if row(ui, "Done", sel.row() == DONE_ROW) {
                        done = true;
                    }
                });
            });
        },
    );
    done
}

/// A player who is done, in their view, waiting for the others.
fn waiting(ctx: &egui::Context, k: usize, area: Rect, screen: Rect, name: &str) {
    let frame = egui::Frame::new().fill(Color32::from_black_alpha(215)).corner_radius(6.0).inner_margin(18.0);
    egui::Area::new(egui::Id::new(("weapon selection", k)))
        .anchor(Align2::CENTER_CENTER, area.center() - screen.center())
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading(RichText::new(name).color(TITLE));
                    ui.label(RichText::new(WAITING).color(ACTIVE));
                });
            });
        });
}

/// A player done while others choose (issue #32).
pub const WAITING: &str = "Ready: waiting for the others";
