//! The Multiplayer screen (plan 10 task 11, spec §7.1): the games announced
//! on the network (a click joins), joining by address and password (the
//! last address remembered), and "Host a game" below.

use egui::{Button, Color32, Key, RichText, ScrollArea, TextEdit, Ui};

use super::{MenuData, footer, form_row, message, page, primary, section};
use crate::online::{ADDRESS_CHARS, PASSWORD_CHARS, Password};
use crate::state::{Screen, UiAction, UiState};

const WIDTH: f32 = 720.0;

/// The screen's fields.
#[derive(Default)]
pub(super) struct View {
    pub address: String,
    pub password: String,
}

impl View {
    pub fn new(last_address: &str) -> View {
        View { address: last_address.to_string(), password: String::new() }
    }
}

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let connecting = state.is_connecting();
    let mut ui = page(ui, "Multiplayer", WIDTH, out);
    let ui = &mut ui;
    let mut ui = footer(ui, "multiplayer", |ui| {
        let r = primary(ui, "Host a game", !connecting);
        if r.clicked() {
            out.push(UiAction::Open(Screen::Host));
        }
        status(ui, state, data);
    });
    let ui = &mut ui;
    section(ui, "On this network", |ui, _| {
        if data.lan_games.is_empty() {
            ui.label(RichText::new("Searching…").weak());
        }
        let games = data.lan_games.clone();
        ScrollArea::vertical().id_salt("lan games").max_height(160.0).auto_shrink([false, true]).show(
            ui,
            |ui| {
                for g in games {
                    let lock = if g.password { "  🔒" } else { "" };
                    let text = format!("{}   {}   {}/{}{lock}", g.name, g.map, g.players, g.max_players);
                    let full = g.players >= g.max_players;
                    let button = Button::new(RichText::new(text).size(16.0))
                        .min_size(egui::vec2(ui.available_width(), 30.0));
                    let r = ui.add_enabled(!connecting && !full, button);
                    if r.clicked() {
                        if g.password {
                            // A password is typed first: the address goes to the form.
                            data.multiplayer.address = g.addr.to_string();
                            data.multiplayer.password.clear();
                        } else {
                            out.push(UiAction::JoinLan(g.addr));
                        }
                    }
                    if g.password {
                        r.on_hover_text("Password protected: type it below, then Join");
                    }
                }
            },
        );
    });
    ui.add_space(10.0);
    section(ui, "Direct", |ui, _| {
        let v = &mut data.multiplayer;
        let mut join = false;
        form_row(ui, "Address", |ui, label| {
            let field = TextEdit::singleline(&mut v.address)
                .char_limit(ADDRESS_CHARS)
                .hint_text("host[:port]")
                .desired_width(280.0);
            let r = ui.add(field).labelled_by(label);
            join |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        });
        form_row(ui, "Password", |ui, label| {
            let field = TextEdit::singleline(&mut v.password)
                .char_limit(PASSWORD_CHARS)
                .password(true)
                .desired_width(280.0);
            let r = ui.add(field).labelled_by(label);
            join |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        });
        form_row(ui, "", |ui, _| {
            let ready = !connecting && !v.address.trim().is_empty();
            join |= ui.add_enabled(ready, Button::new("Join")).clicked();
        });
        if join && !connecting && !v.address.trim().is_empty() {
            out.push(UiAction::Join {
                addr: v.address.clone(),
                password: Some(Password(v.password.clone())),
            });
        }
    });
}

/// "Connecting…", the app's status line and the failure, under the button.
pub(super) fn status(ui: &mut Ui, state: &UiState, data: &MenuData) {
    if state.is_connecting() {
        ui.label("Connecting…");
    }
    if let Some(s) = &data.online_status {
        ui.label(RichText::new(s).color(Color32::from_gray(190)));
    }
    message(ui, state);
}
