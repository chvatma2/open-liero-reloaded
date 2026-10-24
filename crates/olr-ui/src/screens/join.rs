//! Local play's join step (issue #32, `crate::join`; not OLX's): a card
//! per local player, keyboard 1's first, then the free places'. Each seat's
//! own keys drive its own card (the app feeds `MenuData::seats`): any
//! button joins, Left and Right step the profile, Fire, Jump or Start
//! readies, Rope or Dig leaves; every player ready starts the match. No
//! pad moves the menus' focus here. The mouse is keyboard 1's: it acts on
//! keyboard 1's card alone (Join, the profile's arrows, Ready, Leave),
//! besides Start (the match with the players joined, ready or not) and
//! Back.

use egui::{Color32, RichText, Sense, Ui, vec2};
use olr_input::Seat;

use super::{MenuData, columns, footer, message, page, primary, section};
use crate::join::{MAX_LOCAL_PLAYERS, seat_name};
use crate::state::{UiAction, UiState};

/// The page's column: four cards side by side.
const WIDTH: f32 = 1100.0;

/// What a seat does here, under the cards.
pub const JOIN_HELP: &str = "Press a button of your own keys or gamepad to join. \
    Left and right: your profile. Fire, Jump or Start: ready. Rope or Dig: leave.";

/// A free place's card.
pub const PRESS_TO_JOIN: &str = "Press a button to join";

/// Keyboard 1's Join, for the mouse.
pub const JOIN_KEYBOARD_1: &str = "Join with keyboard 1";

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let lineup = state.lineup();
    out.extend(data.join.on_seats(lineup, &data.seats));
    let mut ui = page(ui, "Players", WIDTH, out);
    let ui = &mut ui;
    let mut ui = footer(ui, "join", |ui| {
        let starting = state.is_starting();
        if primary(ui, super::start_label(starting), !lineup.is_empty() && !starting).clicked() {
            out.push(UiAction::StartMatch);
        }
        message(ui, state);
    });
    let ui = &mut ui;
    ui.label(RichText::new(JOIN_HELP).weak());
    ui.add_space(8.0);
    let keyboard_1 = lineup.iter().any(|j| j.seat == Seat::Keyboard1);
    columns(ui, MAX_LOCAL_PLAYERS, |c| {
        for (k, ui) in c.iter_mut().enumerate() {
            match lineup.get(k) {
                Some(j) => {
                    section(ui, &seat_name(j.seat), |ui, _| card(ui, state, data, k, out));
                }
                None => {
                    section(ui, &format!("Player {}", k + 1), |ui, _| {
                        ui.label(RichText::new(PRESS_TO_JOIN).weak());
                        if !keyboard_1 && k == lineup.len() && ui.button(JOIN_KEYBOARD_1).clicked() {
                            out.push(UiAction::JoinSeat(Seat::Keyboard1));
                        }
                    });
                }
            }
        }
    });
}

/// Local player `k`'s card: the colour and name, the skin, ready or not;
/// on keyboard 1's (the mouse's) its buttons.
fn card(ui: &mut Ui, state: &UiState, data: &MenuData, k: usize, out: &mut Vec<UiAction>) {
    let j = state.lineup()[k];
    let profile = state.player_profile(k);
    let colour =
        profile.map_or(Color32::from_gray(160), |p| Color32::from_rgb(p.colour[0], p.colour[1], p.colour[2]));
    let mouse = j.seat == Seat::Keyboard1;
    ui.horizontal(|ui| {
        if mouse && ui.add_enabled(!j.ready, egui::Button::new("◀")).clicked() {
            out.push(UiAction::StepProfile { seat: j.seat, by: -1 });
        }
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        ui.painter().rect_filled(rect, 3.0, colour);
        ui.label(RichText::new(state.player_name(k)).size(18.0).strong());
        if mouse && ui.add_enabled(!j.ready, egui::Button::new("▶")).clicked() {
            out.push(UiAction::StepProfile { seat: j.seat, by: 1 });
        }
    });
    let skin = match profile {
        Some(p) => data.catalog.skin(&p.skin_id).map_or_else(|| p.skin_id.clone(), |s| s.name.clone()),
        None => "default".to_string(),
    };
    ui.label(RichText::new(format!("Skin: {skin}")).weak());
    ui.add_space(4.0);
    let (text, c) =
        if j.ready { ("Ready", Color32::from_rgb(120, 220, 120)) } else { ("Not ready", Color32::GRAY) };
    ui.label(RichText::new(text).color(c).strong());
    if mouse {
        ui.horizontal(|ui| {
            if ui.button(if j.ready { "Not ready" } else { "Ready" }).clicked() {
                out.push(UiAction::SetReady { seat: j.seat, ready: !j.ready });
            }
            if ui.button("Leave").clicked() {
                out.push(UiAction::LeaveSeat(j.seat));
            }
        });
    }
}
