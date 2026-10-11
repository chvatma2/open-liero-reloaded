//! In game: the HUD is the app's to paint (`crate::hud::show`, over the
//! picture, in menu games and the command line's alike); this screen adds
//! the pause menu (Esc): Resume, Options (the game stays paused; Back
//! returns here) and Quit to menu. The match does not tick while it shows.

use egui::{Align2, Color32, Ui};

use crate::state::{Screen, UiAction, UiState};

pub(super) fn show(ui: &mut Ui, state: &UiState, out: &mut Vec<UiAction>) {
    if !state.is_paused() {
        return;
    }
    let (_, offset) = super::playfield(ui.ctx());
    egui::Area::new(egui::Id::new("pause menu")).anchor(Align2::CENTER_CENTER, offset).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(Color32::from_black_alpha(210)).corner_radius(6.0).inner_margin(16.0).show(
            ui,
            |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Paused");
                    ui.add_space(8.0);
                    let button =
                        |ui: &mut Ui, label: &str| ui.add_sized([200.0, 32.0], egui::Button::new(label));
                    if button(ui, "Resume").clicked() {
                        out.push(UiAction::Resume);
                    }
                    if button(ui, "Options").clicked() {
                        out.push(UiAction::Open(Screen::Options));
                    }
                    if button(ui, "Quit to menu").clicked() {
                        out.push(UiAction::LeaveMatch);
                    }
                });
            },
        );
    });
}
