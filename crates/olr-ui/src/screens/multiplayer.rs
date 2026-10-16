//! The Multiplayer screen (plan 10 task 11, plan 12a, spec §7.1), in three
//! tabs:
//!
//! - **Internet:** the games announced on the Nostr relays (the app's game
//!   list), an aligned table with headers when its columns fit the
//!   width, else each game on two lines so that it fits a narrow window: the name
//!   (truncated, a lock before it for a password), then map, mod, players
//!   (the seats taken of the seats, the bots apart: "1/8 +3 bots"; bots
//!   take no seat), ping and region, wrapping. A click selects a game (the app measures its ping); Join
//!   joins it, expecting its key; "Block this host" hides its games for
//!   good. A game of another version is greyed out ("needs version …");
//!   one without an address (plan 12b, N46) is joined through the relays,
//!   its ping "via relays". Above the list, "No relay reachable" or
//!   "Searching…" until a relay sent its games.
//! - **LAN:** the games announced on the local network (a click joins; a
//!   locked one asks for its password in the Direct tab; players as on
//!   the Internet tab). A game is full when its seats are: bots never
//!   fill one.
//! - **Direct:** an address or an invite code (`olr1…`, any case; the app
//!   tells them apart) and a password (the last one typed remembered).
//!
//! "Host a game" below, and while a join waits, its step (the app's
//! `MenuData::joining`) or "Connecting…".

use egui::{Button, Color32, Grid, Key, RichText, ScrollArea, TextEdit, Ui, vec2};

use super::{MenuData, footer, form_row, message, page, primary, section, tab_row};
use crate::online::{ADDRESS_CHARS, InternetEntry, InternetStatus, PASSWORD_CHARS, Password, is_invite};
use crate::state::{Screen, UiAction, UiState};

const WIDTH: f32 = 1100.0;
/// A tab's button.
const TAB: egui::Vec2 = vec2(130.0, 34.0);
const GREY: Color32 = Color32::from_gray(120);
/// The ping column of a game without an address (N46: joined through the
/// relays; its ping is not measured).
pub const VIA_RELAYS: &str = "via relays";

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
        let (tab_size, indent) = tab_row(ui, TAB, Tab::ALL.len());
        if indent > 0.0 {
            ui.add_space(indent);
        }
        // A narrow tab holds its label: less padding (the tab's size is set).
        ui.spacing_mut().button_padding.x = 2.0;
        for t in Tab::ALL {
            let tab = Button::selectable(data.multiplayer.tab == t, RichText::new(t.label()).size(18.0));
            if ui.add_sized(tab_size, tab).clicked() {
                data.multiplayer.tab = t;
            }
        }
    });
    ui.add_space(10.0);
    // A window too short for the tab (320 × 240 points: 4:3 at UI scale 3)
    // scrolls it, so Join stays reachable.
    ScrollArea::vertical().id_salt("multiplayer body").auto_shrink([false, false]).show(ui, |ui| match data
        .multiplayer
        .tab
    {
        Tab::Internet => section(ui, "On the Internet", |ui, _| internet(ui, connecting, data, out)),
        Tab::Lan => section(ui, "On this network", |ui, _| lan(ui, connecting, data, out)),
        Tab::Direct => section(ui, "Join by address or invite", |ui, _| direct(ui, connecting, data, out)),
    });
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
        |ui| match table_widths(ui, &games) {
            Some(widths) => {
                Grid::new("internet table").num_columns(7).striped(true).spacing(vec2(GAP, 6.0)).show(
                    ui,
                    |ui| {
                        for (h, w) in HEADERS.iter().zip(widths) {
                            cell(ui, w, |ui| ui.label(RichText::new(*h).strong()));
                        }
                        ui.end_row();
                        for g in &games {
                            table_row(ui, g, &widths, &mut selected);
                        }
                    },
                );
            }
            None => {
                for (i, g) in games.iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    row(ui, g, &mut selected);
                }
            }
        },
    );
    data.selected_game = selected;
    if data.multiplayer.password_for != selected {
        data.multiplayer.password_for = selected;
        data.multiplayer.internet_password.clear();
    }
    let Some(g) = selected.and_then(|k| games.iter().find(|g| g.key == k)) else { return };
    ui.add_space(8.0);
    let full = g.is_full();
    let why_not = match &g.incompatible {
        Some(v) => Some(v.clone()),
        None if full => Some("This game is full.".into()),
        None => None,
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
        // Block goes on a second line where the two do not fit (a narrow
        // window at a large UI scale).
        ui.horizontal_wrapped(|ui| {
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
    });
    if join && !connecting && why_not.is_none() {
        let password = g.password.then(|| Password(v.internet_password.clone()));
        out.push(UiAction::JoinListed { addr: g.addr, key: g.key, password });
    }
}

const HEADERS: [&str; 7] = ["Name", "Map", "Mod", "Players", "Ping", "Region", ""];
/// The table's gap between columns.
const GAP: f32 = 14.0;
/// The widest the name, map, mod and ping columns grow (longer text is
/// truncated, its hover has all of it); players and region fit their text.
const CAPS: [f32; 7] = [220.0, 150.0, 120.0, f32::INFINITY, 130.0, f32::INFINITY, f32::INFINITY];

/// The ping column's text of a game.
fn ping_text(g: &InternetEntry) -> String {
    match (&g.incompatible, g.addr) {
        (Some(v), _) => v.clone(),
        (None, None) => VIA_RELAYS.into(),
        (None, Some(_)) => g.ping.text(),
    }
}

/// A game's seven cells' texts.
fn cells_of(g: &InternetEntry) -> [String; 7] {
    let playing = if g.playing { " (playing)" } else { "" };
    [
        g.name.clone(),
        g.map.clone(),
        g.mod_name.clone(),
        format!("{}{playing}", g.players_text()),
        ping_text(g),
        g.region.label().into(),
        if g.password { "🔒".into() } else { String::new() },
    ]
}

/// The columns' widths when the table fits `ui.available_width()` (each
/// column its widest text, headers and every game's, up to its cap), else
/// `None`: the games take two lines each (`row`).
fn table_widths(ui: &Ui, games: &[InternetEntry]) -> Option<[f32; 7]> {
    let measure_in = |style: egui::TextStyle, t: &str| {
        let font = style.resolve(ui.style());
        ui.painter().layout_no_wrap(t.to_string(), font, Color32::WHITE).size().x
    };
    let measure = |t: &str| measure_in(egui::TextStyle::Body, t);
    let pad = 2.0 * ui.spacing().button_padding.x;
    let mut widths = HEADERS.map(|h| if h.is_empty() { 0.0 } else { measure(h) * 1.1 });
    for g in games {
        for (i, t) in cells_of(g).iter().enumerate() {
            // The name cell is a selectable button, drawn in the Button style.
            let w = if i == 0 { measure_in(egui::TextStyle::Button, t) + pad } else { measure(t) };
            widths[i] = widths[i].max(w);
        }
    }
    for (w, cap) in widths.iter_mut().zip(CAPS) {
        *w = w.min(cap).ceil();
    }
    let total = widths.iter().sum::<f32>() + GAP * 6.0;
    (total <= ui.available_width()).then_some(widths)
}

/// A table cell `w` wide.
fn cell<R>(ui: &mut Ui, w: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(vec2(w, 0.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(w);
        add(ui)
    })
    .inner
}

/// One game as a row of the aligned table.
fn table_row(ui: &mut Ui, g: &InternetEntry, widths: &[f32; 7], selected: &mut Option<[u8; 32]>) {
    let grey = g.incompatible.is_some();
    let text = |s: String| if grey { RichText::new(s).color(GREY) } else { RichText::new(s) };
    let cells = cells_of(g);
    cell(ui, widths[0], |ui| {
        let b = Button::selectable(*selected == Some(g.key), text(cells[0].clone()))
            .truncate()
            .min_size(vec2(widths[0], 0.0));
        if ui.add(b).on_hover_text(&g.name).clicked() {
            *selected = Some(g.key);
        }
    });
    for i in 1..6 {
        cell(ui, widths[i], |ui| {
            let r = ui.add(egui::Label::new(text(cells[i].clone())).truncate());
            if i == 4 && g.incompatible.is_none() && g.addr.is_none() {
                r.on_hover_text("No address to measure: joined through the Nostr relays.");
            } else {
                r.on_hover_text(&cells[i]);
            }
        });
    }
    cell(ui, widths[6], |ui| ui.label(cells[6].clone()));
    ui.end_row();
}

/// One game of the Internet list, on two lines so that it fits whatever
/// the width (ruling UI-1: 427 points at UI scale 3, 320 on a 4:3
/// desktop): its name, which selects it, truncated (the hover has all of
/// it) after a lock for a password; below it the map, mod, players, ping
/// and region, wrapping.
fn row(ui: &mut Ui, g: &InternetEntry, selected: &mut Option<[u8; 32]>) {
    let grey = g.incompatible.is_some();
    let text = |s: String| if grey { RichText::new(s).color(GREY) } else { RichText::new(s) };
    ui.horizontal(|ui| {
        if g.password {
            ui.label("🔒");
        }
        let name = Button::selectable(*selected == Some(g.key), text(g.name.clone()))
            .truncate()
            .min_size(vec2(ui.available_width(), 0.0));
        if ui.add(name).on_hover_text(&g.name).clicked() {
            *selected = Some(g.key);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(text(g.map.clone()));
        ui.label(text(g.mod_name.clone()));
        let playing = if g.playing { " (playing)" } else { "" };
        ui.label(text(format!("{}{playing}", g.players_text())));
        let r = ui.label(text(ping_text(g)));
        if g.incompatible.is_none() && g.addr.is_none() {
            r.on_hover_text("No address to measure: joined through the Nostr relays.");
        }
        ui.label(text(g.region.label().into()));
    });
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
            let text = format!("{}   {}   {}{lock}", g.name, g.map, g.players_text());
            let full = g.is_full();
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
            .hint_text("host[:port] or olr1…")
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

/// "Connecting…" (or the join's step), the app's status line and the
/// failure, under the button.
pub(super) fn status(ui: &mut Ui, state: &UiState, data: &MenuData) {
    if state.is_connecting() {
        ui.label(data.joining.as_deref().unwrap_or("Connecting…"));
    }
    if let Some(s) = &data.online_status {
        ui.label(RichText::new(s).color(Color32::from_gray(190)));
    }
    message(ui, state);
}
