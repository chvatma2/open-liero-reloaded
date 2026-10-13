//! The Multiplayer screen (plan 10 task 11, plan 12a, spec §7.1), in three
//! tabs:
//!
//! - **Internet:** the games announced on the Nostr relays (the app's game
//!   list): name, map, mod, players, ping, region and a lock for a
//!   password. A click selects a game (the app measures its ping); Join
//!   joins it, expecting its key; "Block this host" hides its games for
//!   good. A game of another version is greyed out ("needs version …"),
//!   one without a public address "cannot be joined yet". Above the list,
//!   "No relay reachable" or "Searching…" until a relay sent its games.
//! - **LAN:** the games announced on the local network (a click joins; a
//!   locked one asks for its password in the Direct tab).
//! - **Direct:** an address or an invite code (`rlx1…`, any case; the app
//!   tells them apart) and a password (the last one typed remembered).
//!
//! "Host a game" below.

use egui::{Button, Color32, Grid, Key, RichText, ScrollArea, TextEdit, Ui, vec2};

use super::{MenuData, footer, form_row, message, page, primary, section};
use crate::online::{
    ADDRESS_CHARS, CANNOT_JOIN_YET, InternetEntry, InternetStatus, PASSWORD_CHARS, Password, is_invite,
};
use crate::state::{Screen, UiAction, UiState};

const WIDTH: f32 = 900.0;
/// A tab's button.
const TAB: egui::Vec2 = vec2(130.0, 34.0);
const GREY: Color32 = Color32::from_gray(120);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Internet,
    Lan,
    Direct,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Internet, Tab::Lan, Tab::Direct];

    fn label(self) -> &'static str {
        match self {
            Tab::Internet => "Internet",
            Tab::Lan => "LAN",
            Tab::Direct => "Direct",
        }
    }
}

/// The screen's fields.
#[derive(Default)]
pub(super) struct View {
    pub tab: Tab,
    /// The Direct tab's address or invite code.
    pub address: String,
    pub password: String,
    /// The Internet tab's password for the selected game.
    pub internet_password: String,
    /// The game `internet_password` was typed for: another selection
    /// clears it (one game's password is never offered to another, which
    /// could guess it offline from the proof).
    pub password_for: Option<[u8; 32]>,
}

impl View {
    pub fn new(last_address: &str) -> View {
        View { address: last_address.to_string(), ..View::default() }
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
    // The tabs, centred over the column.
    ui.horizontal(|ui| {
        let n = Tab::ALL.len() as f32;
        let row = TAB.x * n + ui.spacing().item_spacing.x * (n - 1.0);
        ui.add_space(((ui.available_width() - row) / 2.0).max(0.0));
        for t in Tab::ALL {
            let tab = Button::selectable(data.multiplayer.tab == t, RichText::new(t.label()).size(18.0));
            if ui.add_sized(TAB, tab).clicked() {
                data.multiplayer.tab = t;
            }
        }
    });
    ui.add_space(10.0);
    match data.multiplayer.tab {
        Tab::Internet => section(ui, "On the Internet", |ui, _| internet(ui, connecting, data, out)),
        Tab::Lan => section(ui, "On this network", |ui, _| lan(ui, connecting, data, out)),
        Tab::Direct => section(ui, "Join by address or invite", |ui, _| direct(ui, connecting, data, out)),
    }
}

/// The games on the relays, the selected one's Join and Block.
fn internet(ui: &mut Ui, connecting: bool, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let view = &data.internet;
    match view.status {
        InternetStatus::NoRelay => {
            ui.label(RichText::new("No relay reachable").color(Color32::from_rgb(255, 110, 90)));
        }
        InternetStatus::Searching => {
            ui.label(RichText::new("Searching…").weak());
        }
        InternetStatus::Listed if view.games.is_empty() => {
            ui.label(RichText::new("No games found").weak());
        }
        InternetStatus::Listed => {}
    }
    let games = view.games.clone();
    let mut selected = data.selected_game;
    ScrollArea::vertical().id_salt("internet games").max_height(300.0).auto_shrink([false, true]).show(
        ui,
        |ui| {
            Grid::new("internet table").num_columns(7).striped(true).spacing(vec2(14.0, 6.0)).show(
                ui,
                |ui| {
                    for h in ["Name", "Map", "Mod", "Players", "Ping", "Region", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for g in &games {
                        row(ui, g, &mut selected);
                    }
                },
            );
        },
    );
    data.selected_game = selected;
    if data.multiplayer.password_for != selected {
        data.multiplayer.password_for = selected;
        data.multiplayer.internet_password.clear();
    }
    let Some(g) = selected.and_then(|k| games.iter().find(|g| g.key == k)) else { return };
    ui.add_space(8.0);
    let full = g.players >= g.max_players;
    let why_not = match (&g.incompatible, g.addr) {
        (Some(v), _) => Some(v.clone()),
        (None, None) => Some(format!("This game {CANNOT_JOIN_YET}: it has no public address.")),
        (None, Some(_)) if full => Some("This game is full.".into()),
        _ => None,
    };
    let v = &mut data.multiplayer;
    let mut join = false;
    if g.password {
        form_row(ui, "Game password", |ui, label| {
            let field = TextEdit::singleline(&mut v.internet_password)
                .char_limit(PASSWORD_CHARS)
                .password(true)
                .desired_width(280.0);
            let r = ui.add(field).labelled_by(label);
            join |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        });
    }
    form_row(ui, "", |ui, _| {
        let r = ui.add_enabled(!connecting && why_not.is_none(), Button::new("Join"));
        let r = match &why_not {
            Some(why) => r.on_disabled_hover_text(why),
            None => r,
        };
        join |= r.clicked();
        if ui
            .button("Block this host")
            .on_hover_text("Its games are hidden from now on (Options lists them).")
            .clicked()
        {
            out.push(UiAction::BlockHost(g.key));
        }
    });
    if join
        && !connecting
        && why_not.is_none()
        && let Some(addr) = g.addr
    {
        let password = g.password.then(|| Password(v.internet_password.clone()));
        out.push(UiAction::JoinListed { addr, key: g.key, password });
    }
}

/// One game of the Internet list: its name selects it.
fn row(ui: &mut Ui, g: &InternetEntry, selected: &mut Option<[u8; 32]>) {
    let grey = g.incompatible.is_some();
    let text = |s: String| if grey { RichText::new(s).color(GREY) } else { RichText::new(s) };
    if ui.selectable_label(*selected == Some(g.key), text(g.name.clone())).clicked() {
        *selected = Some(g.key);
    }
    ui.label(text(g.map.clone()));
    ui.label(text(g.mod_name.clone()));
    let playing = if g.playing { " (playing)" } else { "" };
    ui.label(text(format!("{}/{}{playing}", g.players, g.max_players)));
    let ping = match (&g.incompatible, g.addr) {
        (Some(v), _) => v.clone(),
        (None, None) => CANNOT_JOIN_YET.into(),
        (None, Some(_)) => g.ping.text(),
    };
    ui.label(text(ping));
    ui.label(text(g.region.label().into()));
    ui.label(if g.password { "🔒" } else { "" });
    ui.end_row();
}

/// The games on this network: a click joins (a locked one asks for its
/// password in the Direct tab).
fn lan(ui: &mut Ui, connecting: bool, data: &mut MenuData, out: &mut Vec<UiAction>) {
    if data.lan_games.is_empty() {
        ui.label(RichText::new("Searching…").weak());
    }
    let games = data.lan_games.clone();
    ScrollArea::vertical().id_salt("lan games").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
        for g in games {
            let lock = if g.password { "  🔒" } else { "" };
            let text = format!("{}   {}   {}/{}{lock}", g.name, g.map, g.players, g.max_players);
            let full = g.players >= g.max_players;
            let button =
                Button::new(RichText::new(text).size(16.0)).min_size(egui::vec2(ui.available_width(), 30.0));
            let r = ui.add_enabled(!connecting && !full, button);
            if r.clicked() {
                if g.password {
                    // A password is typed first: the address goes to the form.
                    data.multiplayer.address = g.addr.to_string();
                    data.multiplayer.password.clear();
                    data.multiplayer.tab = Tab::Direct;
                } else {
                    out.push(UiAction::JoinLan(g.addr));
                }
            }
            if g.password {
                r.on_hover_text("Password protected: type it in Direct, then Join");
            }
        }
    });
}

/// An address or an invite code, and a password.
fn direct(ui: &mut Ui, connecting: bool, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let v = &mut data.multiplayer;
    let mut join = false;
    form_row(ui, "Address or invite code", |ui, label| {
        let field = TextEdit::singleline(&mut v.address)
            .char_limit(ADDRESS_CHARS)
            .hint_text("host[:port] or rlx1…")
            .desired_width(380.0);
        let r = ui.add(field).labelled_by(label);
        join |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    });
    if is_invite(&v.address) {
        form_row(ui, "", |ui, _| {
            ui.label(RichText::new("An invite code: the game it names, checked by its key.").weak());
        });
    }
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
        out.push(UiAction::Join { addr: v.address.clone(), password: Some(Password(v.password.clone())) });
    }
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
