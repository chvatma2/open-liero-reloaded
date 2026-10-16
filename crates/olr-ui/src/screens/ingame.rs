//! In game: the HUD is the app's to paint (`crate::hud::show`, over the
//! picture, in menu games and the command line's alike); this screen adds
//! the pause menu (Esc): Resume, Options (the game stays paused; Back
//! returns here) and Quit to menu. The match does not tick while it shows,
//! except online, where nothing pauses: there it is the match's menu, and
//! Quit to menu leaves the online game.
//!
//! Online, the chat key (`settings.online.chat_key`, T by default; the app
//! opens it) opens the chat (spec §7.2): a one-line field over the bottom
//! of the playfield ([`MenuData::ingame_chat`] holds its text while it is
//! open; the app sends no key to the game meanwhile). Enter sends
//! the line (`LobbyChat`) and closes it, Esc closes it (and does not open
//! the menu: `show`'s typing check), as does a click elsewhere.

use egui::{Align2, Color32, Key, TextEdit, Ui};

use super::MenuData;
use crate::online::CHAT_CHARS;
use crate::state::{Screen, UiAction, UiState};

/// The chat field's width, in points (at most the playfield's).
const CHAT_WIDTH: f32 = 420.0;

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let online = state.is_online();
    if !online || state.is_paused() {
        data.ingame_chat = None;
    }
    if data.ingame_chat.is_some() {
        chat(ui, data, out);
    }
    if !state.is_paused() {
        return;
    }
    let (_, offset) = super::playfield(ui.ctx());
    egui::Area::new(egui::Id::new("pause menu")).anchor(Align2::CENTER_CENTER, offset).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(Color32::from_black_alpha(210)).corner_radius(6.0).inner_margin(16.0).show(
            ui,
            |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading(if online { "Menu" } else { "Paused" });
                    if online {
                        ui.label("The match goes on.");
                    }
                    ui.add_space(8.0);
                    let button =
                        |ui: &mut Ui, label: &str| ui.add_sized([200.0, 32.0], egui::Button::new(label));
                    if button(ui, "Resume").clicked() {
                        out.push(UiAction::Resume);
                    }
                    if button(ui, "Options").clicked() {
                        out.push(UiAction::Open(Screen::Options));
                    }
                    if button(ui, if online { "Leave the game" } else { "Quit to menu" }).clicked() {
                        out.push(UiAction::LeaveMatch);
                    }
                });
            },
        );
    });
}

/// The open chat field, at the bottom of the playfield.
fn chat(ui: &mut Ui, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let (field, _) = super::playfield(ui.ctx());
    let width = CHAT_WIDTH.min(field.width() - 16.0).max(40.0);
    let at = field.left_bottom() + egui::vec2(8.0, -8.0);
    let mut close = false;
    egui::Area::new(egui::Id::new("in-game chat")).pivot(Align2::LEFT_BOTTOM).fixed_pos(at).show(
        ui.ctx(),
        |ui| {
            egui::Frame::new()
                .fill(Color32::from_black_alpha(190))
                .corner_radius(4.0)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let label = ui.label("Say:");
                        let Some(text) = &mut data.ingame_chat else { return };
                        let r = ui
                            .add(
                                TextEdit::singleline(text).char_limit(CHAT_CHARS).desired_width(width - 40.0),
                            )
                            .labelled_by(label.id);
                        if r.lost_focus() {
                            // Enter sends; Esc or a click elsewhere only closes.
                            if ui.input(|i| i.key_pressed(Key::Enter)) && !text.trim().is_empty() {
                                out.push(UiAction::LobbyChat(std::mem::take(text)));
                            }
                            close = true;
                        } else if !r.has_focus() {
                            // Just opened: the field takes the keys from the
                            // next frame (the chat key's text is not typed).
                            r.request_focus();
                        }
                    });
                });
        },
    );
    if close {
        data.ingame_chat = None;
    }
}
