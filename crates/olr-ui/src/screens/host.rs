//! The Host screen (plan 10 task 11, spec §7.1): the game's name, who can
//! join (anyone on the Internet, this network, or by invite only: N43), a
//! password, the most players and the port; Start hosting opens the lobby.

use egui::{ComboBox, DragValue, RichText, Slider, TextEdit, Ui};

use super::multiplayer::status;
use super::{MenuData, fit, footer, form_row, page, primary, section};
use crate::online::{HostPrefs, MAX_PLAYERS, MIN_PLAYERS, Visibility};
use crate::state::{UiAction, UiState};

const WIDTH: f32 = 640.0;

/// The screen's fields: the choices being edited.
#[derive(Default)]
pub(super) struct View {
    pub prefs: HostPrefs,
}

impl View {
    /// The remembered choices; an empty name is the profile playing's.
    pub fn new(state: &UiState) -> View {
        let mut prefs = state.settings().online.host.clone();
        if prefs.name.is_empty() {
            prefs.name = state.profiles().selected().map(|p| p.name.clone()).unwrap_or_default();
        }
        View { prefs }
    }
}

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let connecting = state.is_connecting();
    let mut start = false;
    let mut ui = page(ui, "Host a game", WIDTH, out);
    let ui = &mut ui;
    let mut ui = footer(ui, "host", |ui| {
        start = primary(ui, "Start hosting", !connecting).clicked();
        status(ui, state, data);
    });
    let ui = &mut ui;
    let p = &mut data.host.prefs;
    section(ui, "Game", |ui, _| {
        form_row(ui, "Name", |ui, label| {
            ui.add(
                TextEdit::singleline(&mut p.name)
                    .char_limit(crate::MAX_NAME_CHARS)
                    .desired_width(fit(ui, 240.0)),
            )
            .labelled_by(label);
        });
        form_row(ui, "Who can join", |ui, label| {
            ComboBox::from_id_salt("visibility")
                .selected_text(p.visibility.label())
                .width(fit(ui, 240.0))
                .show_ui(ui, |ui| {
                    for v in Visibility::ALL {
                        ui.selectable_value(&mut p.visibility, v, v.label());
                    }
                })
                .response
                .labelled_by(label);
        });
        form_row(ui, "Password", |ui, label| {
            ui.add(
                TextEdit::singleline(&mut p.password.0)
                    .char_limit(crate::online::PASSWORD_CHARS)
                    .password(true)
                    .hint_text("none")
                    .desired_width(fit(ui, 240.0)),
            )
            .labelled_by(label);
        });
        form_row(ui, "Maximum players", |ui, label| {
            ui.add(Slider::new(&mut p.max_players, MIN_PLAYERS..=MAX_PLAYERS)).labelled_by(label);
        });
        form_row(ui, "Port", |ui, label| {
            ui.add(DragValue::new(&mut p.port).range(1..=u16::MAX)).labelled_by(label);
        });
        let said = match p.visibility {
            Visibility::Public => {
                "Listed on the Internet tab and on this network; the lobby shows its invite code."
            }
            Visibility::Lan => "Listed on this network; others join by address.",
            Visibility::Invite => "Listed nowhere; the lobby shows its invite code to share.",
        };
        form_row(ui, "", |ui, _| {
            ui.add(egui::Label::new(RichText::new(said).weak()).wrap());
        });
    });
    if start {
        out.push(UiAction::Host(p.clone()));
    }
}
