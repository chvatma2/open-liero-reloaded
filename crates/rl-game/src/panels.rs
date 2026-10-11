//! The match overlay's panels (egui): the time-limit clock, the kill
//! feed, the scoreboard (Tab, OLX's `ShowScore`), the results of a
//! finished match and a replay's progress. Their contents come from
//! `hud.rs`; this only paints.

use egui::{Align2, Color32, FontId, Painter, RichText};

use crate::hud::{Results, Row, clock};
use crate::session::{Driver, Session};

/// OLX's clock turns `(255, 50, 50)` in the last 10 s.
const CLOCK_RED_MS: u64 = 10_000;

/// The results show this many ticks after the end (`GAMEOVER_WAIT`, 3 s,
/// `CClient_Draw.cpp:893-905`); "Game Over" shows until then.
pub const RESULTS_AFTER: u64 = 300;

fn shadowed(p: &Painter, pos: egui::Pos2, anchor: Align2, text: &str, font: FontId, colour: Color32) {
    p.text(pos + egui::vec2(1.0, 1.0), anchor, text, font.clone(), Color32::from_black_alpha(200));
    p.text(pos, anchor, text, font, colour);
}

/// The clock (top centre), the replay's progress under it, and the kill
/// feed (top left, below the F2 panel when it shows).
pub fn top(p: &Painter, rect: egui::Rect, s: &Session, debug: bool) {
    let mut y = rect.top() + 8.0;
    if let Some(ms) = s.time_left_ms {
        let colour = if ms <= CLOCK_RED_MS { Color32::from_rgb(255, 50, 50) } else { Color32::WHITE };
        shadowed(
            p,
            egui::pos2(rect.center().x, y),
            Align2::CENTER_TOP,
            &clock(ms),
            FontId::monospace(20.0),
            colour,
        );
        y += 26.0;
    }
    if let Driver::Replay(d) = &s.driver {
        let (t, n) = (d.cursor.tick(), d.replay.ticks());
        let line = if d.is_done() {
            format!("Replay finished ({:.1} s): F5 plays it again, Esc quits", f64::from(n) / 100.0)
        } else {
            format!("Replay {:.1} s / {:.1} s", f64::from(t) / 100.0, f64::from(n) / 100.0)
        };
        let at = egui::pos2(rect.center().x, y);
        shadowed(p, at, Align2::CENTER_TOP, &line, FontId::proportional(14.0), Color32::from_gray(220));
        y += 18.0;
        if let Some(m) = d.mismatch {
            let line = format!("diverged from the recording at tick {} (other content or build?)", m.tick);
            let at = egui::pos2(rect.center().x, y);
            shadowed(
                p,
                at,
                Align2::CENTER_TOP,
                &line,
                FontId::proportional(14.0),
                Color32::from_rgb(255, 90, 70),
            );
        }
    }
    let mut at = rect.left_top() + egui::vec2(10.0, if debug { 130.0 } else { 10.0 });
    for line in s.feed.visible(s.ticks()) {
        shadowed(p, at, Align2::LEFT_TOP, line, FontId::proportional(14.0), Color32::from_gray(235));
        at.y += 17.0;
    }
}

/// "Game Over", until the results show.
pub fn game_over(p: &Painter, rect: egui::Rect) {
    let at = rect.center() - egui::vec2(0.0, 60.0);
    let colour = Color32::from_rgb(255, 200, 80);
    shadowed(p, at, Align2::CENTER_CENTER, "Game Over", FontId::proportional(40.0), colour);
}

/// The rows of a scoreboard as an egui grid.
fn table(ui: &mut egui::Ui, rows: &[Row]) {
    let head = |ui: &mut egui::Ui, t: &str| {
        ui.label(RichText::new(t).monospace().color(Color32::from_gray(170)));
    };
    egui::Grid::new("scores").striped(true).spacing([18.0, 4.0]).show(ui, |ui| {
        for h in ["", "Lives", "Score", "Kills", "Deaths", "Suicides", "Damage"] {
            head(ui, h);
        }
        ui.end_row();
        for (i, r) in rows.iter().enumerate() {
            let colour = if r.worm == 0 { Color32::from_rgb(255, 230, 120) } else { Color32::WHITE };
            let cell = |ui: &mut egui::Ui, t: String| {
                ui.label(RichText::new(t).monospace().color(colour));
            };
            cell(ui, format!("{}. {}", i + 1, r.name));
            cell(ui, r.lives.clone());
            cell(ui, r.score.to_string());
            cell(ui, r.kills.to_string());
            cell(ui, r.deaths.to_string());
            cell(ui, r.suicides.to_string());
            cell(ui, r.damage.to_string());
            ui.end_row();
        }
    });
}

fn panel(ctx: &egui::Context, id: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Area::new(egui::Id::new(id))
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_black_alpha(190))
                .corner_radius(6.0)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    body(ui);
                });
        });
}

/// Tab: the scoreboard (the mode's order), unless the results show.
pub fn scoreboard(ctx: &egui::Context, s: &Session) {
    let rows = crate::hud::rows(s.world.scores(), &s.names);
    if rows.is_empty() {
        return;
    }
    panel(ctx, "scoreboard", |ui| {
        ui.label(RichText::new("Scores").size(18.0).color(Color32::WHITE));
        ui.add_space(6.0);
        table(ui, &rows);
    });
}

/// The results of a finished match: the winner, why it ended, every
/// worm's row, and the keys.
pub fn results(ctx: &egui::Context, r: &Results, replay: bool) {
    panel(ctx, "results", |ui| {
        ui.label(RichText::new("Game over").size(24.0).color(Color32::from_rgb(255, 200, 80)));
        let winner = r.winner.as_deref().unwrap_or("noone");
        ui.label(RichText::new(format!("Winner: {winner}")).size(18.0).color(Color32::WHITE));
        ui.label(RichText::new(format!("by {}", r.reason)).color(Color32::from_gray(190)));
        ui.add_space(8.0);
        table(ui, &r.rows);
        ui.add_space(8.0);
        let keys = if replay { "F5 watch again    Esc quit" } else { "F5 rematch    Esc quit" };
        ui.label(RichText::new(keys).color(Color32::from_gray(200)));
    });
}
