//! The main menu (OLX's `src/client/DeprecatedGUI/Menu_Main.cpp`, laid
//! out anew): local play, the sandbox, the editor, profiles, options, replays,
//! about, report a bug (when the build has a relay to send to) and quit.

use egui::{Button, Color32, RichText, Ui};

use super::message;
use crate::state::{Screen, UiAction, UiState};

const BUTTON: [f32; 2] = [260.0, 36.0];

/// The menu, with the store's `notices` (a newer game's settings file
/// left untouched) as a banner on top.
/// `version` (the game's) shows in the bottom right corner; "Report a
/// bug" only when `reporting` (the build has a relay).
pub(super) fn show(
    ui: &mut Ui,
    state: &UiState,
    notices: &[String],
    version: &str,
    reporting: bool,
    out: &mut Vec<UiAction>,
) {
    if !version.is_empty() {
        // An area of its own: the menu's layout never sees it.
        egui::Area::new(egui::Id::new("olr-main-version"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-8.0, -6.0))
            .interactable(false)
            .show(ui.ctx(), |ui| ui.label(RichText::new(version).weak().size(14.0)));
    }
    ui.vertical_centered(|ui| {
        for n in notices {
            ui.label(RichText::new(n).color(Color32::from_rgb(255, 200, 80)));
        }
        ui.add_space((ui.available_height() * 0.12).max(8.0));
        ui.label(RichText::new("Open Liero Reloaded").size(44.0).strong());
        ui.label(RichText::new("Liero Xtreme as OpenLieroX plays it").weak());
        ui.add_space(28.0);
        let mut button = |ui: &mut Ui, text: &str, action: UiAction| {
            if ui.add_sized(BUTTON, Button::new(RichText::new(text).size(18.0))).clicked() {
                out.push(action);
            }
        };
        button(ui, "Local Play", UiAction::Open(Screen::LocalPlay));
        button(ui, "Multiplayer", UiAction::Open(Screen::Multiplayer));
        button(ui, "Sandbox", UiAction::StartSandbox);
        button(ui, "Editor", UiAction::Open(Screen::Editor));
        button(ui, "Profiles", UiAction::Open(Screen::Profiles));
        button(ui, "Options", UiAction::Open(Screen::Options));
        button(ui, "Replays", UiAction::Open(Screen::Replays));
        button(ui, "About", UiAction::Open(Screen::About));
        if reporting {
            button(ui, super::report::TITLE, UiAction::Open(Screen::Report));
        }
        ui.add_space(12.0);
        button(ui, "Quit", UiAction::Quit);
        ui.add_space(12.0);
        if state.is_starting() {
            ui.label("Loading…");
        }
        message(ui, state);
    });
}
