//! The Host lost screen (plan 12c, spec §3.7): the match's host is gone
//! and no player could take the match over (or this host stalled, or its
//! network cut it off from its players, and the match moved on without
//! it: titled "Match lost"). It says why
//! (`MenuData::host_lost`, the app's reason) and, once it is saved, where
//! the match's replay is; Replays opens the
//! Replays screen, Back (Esc) the Multiplayer screen.

use egui::{Button, RichText, Ui};

use super::{MenuData, page};
use crate::state::{Screen, UiAction};

/// The page's column.
const WIDTH: f32 = 640.0;

/// What the screen says once the match's replay is saved (the app's
/// `MenuData::host_lost_replay`).
pub const HOST_LOST_REPLAY: &str = "The match's replay is saved in Replays.";

/// The screen's title: a player whose host is gone.
pub const HOST_LOST_TITLE: &str = "Host lost";

/// The screen's title for the host itself, whose stalled game (N70) or
/// cut-off network (ruling SB-2) the match left behind
/// (`MenuData::host_lost_hosting`).
pub const STEPPED_DOWN_TITLE: &str = "Match lost";

pub(super) fn show(ui: &mut Ui, data: &MenuData, out: &mut Vec<UiAction>) {
    let title = if data.host_lost_hosting { STEPPED_DOWN_TITLE } else { HOST_LOST_TITLE };
    let mut ui = page(ui, title, WIDTH, out);
    let ui = &mut ui;
    // Plain lines, no panel: the whole page fits 240 points high (a 4:3
    // desktop at UI scale 3).
    let why = data.host_lost.as_deref().unwrap_or("The match's host is gone.");
    ui.label(RichText::new(why).size(17.0));
    if data.host_lost_replay {
        ui.add_space(4.0);
        ui.label(RichText::new(HOST_LOST_REPLAY).weak());
    }
    ui.add_space(8.0);
    if ui.add(Button::new(RichText::new("Replays").size(17.0))).clicked() {
        out.push(UiAction::Open(Screen::Replays));
    }
}
