//! The results of a finished match (OLX's results screen,
//! `CClient::UpdateScore`, `CClient_Draw.cpp:1851-1966`;
//! `docs/porting/olx-notes/bots-and-deathmatch.md` §3.4): the winner (or
//! "noone"), why it ended, and every worm in the mode's order with its
//! lives, score, kills, deaths, suicides and damage. Rematch (a replay:
//! Watch again), Save replay and Back to menu.

use egui::{Align2, Color32, RichText, Ui};

use super::MenuData;
use crate::hud::{Results, table};
use crate::state::{Game, UiAction, UiState};

/// The results panel, centred, with `footer` under the table (the menus'
/// buttons, or the command line's keys).
pub fn panel(ctx: &egui::Context, r: &Results, footer: impl FnOnce(&mut Ui)) {
    egui::Area::new(egui::Id::new("results")).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(
        ctx,
        |ui| {
            egui::Frame::new()
                .fill(Color32::from_black_alpha(200))
                .corner_radius(6.0)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.label(RichText::new("Game over").size(24.0).color(Color32::from_rgb(255, 200, 80)));
                    let winner = r.winner.as_deref().unwrap_or("noone");
                    ui.label(RichText::new(format!("Winner: {winner}")).size(18.0).color(Color32::WHITE));
                    ui.label(RichText::new(format!("by {}", r.reason)).color(Color32::from_gray(190)));
                    ui.add_space(8.0);
                    table(ui, &r.rows);
                    ui.add_space(10.0);
                    footer(ui);
                });
        },
    );
}

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &MenuData, out: &mut Vec<UiAction>) {
    let Some(r) = &data.results else {
        // No results (the app has none to give): a way out still.
        egui::Area::new(egui::Id::new("results")).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(
            ui.ctx(),
            |ui| {
                if ui.button("Back to menu").clicked() {
                    out.push(UiAction::LeaveMatch);
                }
            },
        );
        return;
    };
    let replay = state.game() == Some(Game::Replay);
    panel(ui.ctx(), r, |ui| {
        ui.horizontal(|ui| {
            if replay {
                if ui.button("Watch again").clicked() {
                    out.push(UiAction::Restart);
                }
            } else {
                if ui.button("Rematch").clicked() {
                    out.push(UiAction::StartMatch);
                }
                if ui.button("Save replay").clicked() {
                    out.push(UiAction::SaveReplay);
                }
            }
            if ui.button("Back to menu").clicked() {
                out.push(UiAction::LeaveMatch);
            }
        });
        if state.is_starting() {
            ui.label(RichText::new("Loading…").weak());
        }
        if let Some(m) = state.message() {
            ui.label(RichText::new(m).color(Color32::from_gray(220)));
        }
    });
}
