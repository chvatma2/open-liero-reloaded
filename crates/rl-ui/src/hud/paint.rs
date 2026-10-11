//! Paints a [`HudView`] where a [`HudLayout`] says (the Modern style):
//! OLX's colours (`docs/porting/olx-notes/presentation.md` §9) on
//! translucent panels, text with a one-pixel shadow.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Stroke, StrokeKind, Ui, pos2, vec2};

use super::layout::HudLayout;
use super::minimap::{ALPHA, dot_colour, dot_rect, view_rect};
use super::model::{Row, clock, lives_text};
use super::view::{Bar, HudView, MinimapView, WormHud};

/// OLX's clock turns `(255, 50, 50)` in the last 10 s (`CClient_Draw.cpp:762-787`).
const CLOCK_RED_MS: u64 = 10_000;
const RED: Color32 = Color32::from_rgb(255, 50, 50);
/// OLX's health bar: `(64, 255, 64)` on `(128, 128, 128)`.
const HEALTH: Color32 = Color32::from_rgb(64, 255, 64);
const BAR_BG: Color32 = Color32::from_rgb(128, 128, 128);
/// OLX's weapon bar states (`CClient_Draw.cpp:1221-1257`).
const READY: Color32 = Color32::from_rgb(64, 64, 255);
const RELOADING: Color32 = Color32::from_rgb(128, 64, 64);
const COOLDOWN: Color32 = Color32::from_rgb(255, 161, 66);
/// `clWeaponLabel` `#02bff6`; the top bar's lives `#00DE00` and kills `#DE0000`.
const WEAPON_LABEL: Color32 = Color32::from_rgb(0x02, 0xbf, 0xf6);
const LIVES: Color32 = Color32::from_rgb(0x00, 0xde, 0x00);
const KILLS: Color32 = Color32::from_rgb(0xde, 0x00, 0x00);
const PANEL: Color32 = Color32::from_black_alpha(140);

fn shadowed(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, colour: Color32) -> Rect {
    p.text(pos + vec2(1.0, 1.0), anchor, text, font.clone(), Color32::from_black_alpha(200));
    p.text(pos, anchor, text, font, colour)
}

/// Paints `view` over `ui`'s whole area.
pub fn show(ui: &Ui, view: &HudView) {
    let l = HudLayout::new(ui.max_rect());
    let p = ui.painter();
    if let Some(w) = &view.worm {
        health(p, &l, w);
        weapons(p, &l, w);
    }
    top(p, &l, view);
    if let Some(m) = &view.minimap {
        minimap(p, &l, m);
    }
    if let (Some(hint), Some(rect)) = (&view.hint, l.hint) {
        let p = p.with_clip_rect(rect);
        let font = FontId::proportional(l.text * 0.85);
        shadowed(&p, rect.right_bottom(), Align2::RIGHT_BOTTOM, hint, font, Color32::from_gray(190));
    }
    if view.game_over {
        let at = l.screen.center() - vec2(0.0, 60.0 * l.unit);
        let font = FontId::proportional(40.0 * l.unit);
        shadowed(p, at, Align2::CENTER_CENTER, "Game Over", font, Color32::from_rgb(255, 200, 80));
    }
    if let Some(rows) = view.scoreboard.as_ref().filter(|r| !r.is_empty()) {
        scoreboard(ui, rows);
    }
}

/// Lives and kills, then the health bar (or why the worm is down).
fn health(p: &Painter, l: &HudLayout, w: &WormHud) {
    let r = l.health;
    p.rect_filled(r.expand(2.0 * l.unit), 4.0 * l.unit, PANEL);
    let font = FontId::proportional(l.text);
    let row = r.height() / 2.0;
    let mut at = r.left_top() + vec2(4.0 * l.unit, row / 2.0);
    let lives = match w.lives {
        -2 => None,
        n => Some(format!("Lives {}", lives_text(n))),
    };
    if let Some(lives) = lives {
        let used = shadowed(p, at, Align2::LEFT_CENTER, &lives, font.clone(), LIVES);
        at.x = used.right() + 12.0 * l.unit;
    }
    shadowed(p, at, Align2::LEFT_CENTER, &format!("Kills {}", w.kills), font.clone(), KILLS);
    let bar = Rect::from_min_max(pos2(r.left(), r.top() + row), r.right_bottom()).shrink(2.0 * l.unit);
    p.rect_filled(bar, 2.0 * l.unit, BAR_BG);
    if w.alive {
        let fill = Rect::from_min_size(bar.min, vec2(bar.width() * w.health / 100.0, bar.height()));
        p.rect_filled(fill, 2.0 * l.unit, HEALTH);
        let text = format!("{:.0}", w.health);
        shadowed(p, bar.center(), Align2::CENTER_CENTER, &text, font, Color32::WHITE);
    } else {
        let text = w.status.as_deref().unwrap_or("Dead");
        let p = p.with_clip_rect(bar);
        shadowed(&p, bar.center(), Align2::CENTER_CENTER, text, font, Color32::from_rgb(255, 200, 80));
    }
}

/// One row per slot: the weapon's name and its charge bar in OLX's state
/// colours; the slot in hand highlighted.
fn weapons(p: &Painter, l: &HudLayout, w: &WormHud) {
    let r = l.weapons;
    p.rect_filled(r.expand(2.0 * l.unit), 4.0 * l.unit, PANEL);
    let n = rl_sim::MAX_SLOTS as f32;
    let h = (r.height() - 4.0 * l.unit) / n;
    let font = FontId::proportional(l.text * 0.9);
    for (i, s) in w.slots.iter().enumerate().take(rl_sim::MAX_SLOTS) {
        let row =
            Rect::from_min_size(pos2(r.left(), r.top() + 2.0 * l.unit + h * i as f32), vec2(r.width(), h));
        let current = i == w.current && w.alive;
        if current {
            p.rect_filled(row, 2.0 * l.unit, Color32::from_white_alpha(28));
        }
        let split = row.left() + row.width() * 0.58;
        let name_rect = Rect::from_min_max(row.min, pos2(split - 4.0 * l.unit, row.bottom()));
        let colour = if current { WEAPON_LABEL } else { Color32::from_gray(170) };
        let at = pos2(row.left() + 4.0 * l.unit, row.center().y);
        shadowed(&p.with_clip_rect(name_rect), at, Align2::LEFT_CENTER, &s.name, font.clone(), colour);
        let bar =
            Rect::from_min_max(pos2(split, row.top()), row.right_bottom()).shrink2(vec2(3.0, 4.0) * l.unit);
        p.rect_filled(bar, 1.0, Color32::from_black_alpha(160));
        let fore = match s.bar {
            Bar::Ready => READY,
            Bar::Reloading => RELOADING,
            Bar::Cooldown => COOLDOWN,
        };
        p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * s.charge, bar.height())), 1.0, fore);
    }
}

/// The clock and kill limit, the notices under them, and the kill feed.
fn top(p: &Painter, l: &HudLayout, v: &HudView) {
    let c = l.clock;
    let big = FontId::monospace(20.0 * l.unit);
    let small = FontId::proportional(l.text * 0.85);
    let clip = p.with_clip_rect(c.expand(2.0));
    match (v.time_left_ms, v.kill_limit) {
        (Some(ms), limit) => {
            let colour = if ms <= CLOCK_RED_MS { RED } else { Color32::WHITE };
            let t = shadowed(&clip, c.center_top(), Align2::CENTER_TOP, &clock(ms), big, colour);
            if let Some(k) = limit {
                let at = pos2(t.right() + 8.0 * l.unit, t.center().y);
                shadowed(
                    &clip,
                    at,
                    Align2::LEFT_CENTER,
                    &format!("Kill limit {k}"),
                    small.clone(),
                    Color32::from_gray(200),
                );
            }
        }
        (None, Some(k)) => {
            let text = format!("Kill limit {k}");
            shadowed(
                &clip,
                c.center_top(),
                Align2::CENTER_TOP,
                &text,
                small.clone(),
                Color32::from_gray(220),
            );
        }
        (None, None) => {}
    }
    let mut y = c.bottom() + 2.0 * l.unit;
    for n in &v.notices {
        let colour = if n.warn { Color32::from_rgb(255, 90, 70) } else { Color32::from_gray(220) };
        shadowed(p, pos2(c.center().x, y), Align2::CENTER_TOP, &n.text, small.clone(), colour);
        y += l.line;
    }
    let f = l.feed;
    let top = f.top().max(l.screen.top() + v.top_left_reserved);
    let room = ((f.bottom() - top) / l.line).floor().max(0.0) as usize;
    let skip = v.feed.len().saturating_sub(room);
    let fp = p.with_clip_rect(Rect::from_min_max(pos2(f.left(), top), f.max));
    let font = FontId::proportional(l.text);
    for (i, line) in v.feed.iter().skip(skip).enumerate() {
        let at = pos2(f.left(), top + l.line * i as f32);
        shadowed(&fp, at, Align2::LEFT_TOP, line, font.clone(), Color32::from_gray(235));
    }
}

/// The level at half opacity, the view, and a blinking dot per worm.
fn minimap(p: &Painter, l: &HudLayout, m: &MinimapView) {
    let r = l.minimap;
    if r.width() <= 0.0 {
        return;
    }
    p.rect_filled(r, 0.0, Color32::from_black_alpha(ALPHA));
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    p.image(m.texture, r, uv, Color32::from_white_alpha(ALPHA));
    if let Some(v) = m.view {
        let v = view_rect(v, m.map, r);
        if v.width() > 0.0 && v.height() > 0.0 {
            p.rect_stroke(v, 0.0, Stroke::new(1.0, Color32::from_white_alpha(150)), StrokeKind::Inside);
        }
    }
    for d in &m.dots {
        p.rect_filled(dot_rect(d.pos, m.map, r, d.big), 0.0, dot_colour(d.colour, m.blink));
    }
}

/// The scoreboard (OLX's `ShowScore`, Tab held), centred.
fn scoreboard(ui: &Ui, rows: &[Row]) {
    egui::Area::new(egui::Id::new("scoreboard"))
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(Color32::from_black_alpha(190))
                .corner_radius(6.0)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.label(RichText::new("Scores").size(18.0).color(Color32::WHITE));
                    ui.add_space(6.0);
                    table(ui, rows);
                });
        });
}

/// The rows of a scoreboard or of the results as a grid: name, lives,
/// score, kills, deaths, suicides, damage (OLX's results columns plus
/// the deaths and suicides it counts); worm 0 (the player's) in yellow.
pub fn table(ui: &mut Ui, rows: &[Row]) {
    let head = |ui: &mut Ui, t: &str| {
        ui.label(RichText::new(t).monospace().color(Color32::from_gray(170)));
    };
    egui::Grid::new("scores").striped(true).spacing([18.0, 4.0]).show(ui, |ui| {
        for h in ["", "Lives", "Score", "Kills", "Deaths", "Suicides", "Damage"] {
            head(ui, h);
        }
        ui.end_row();
        for (i, r) in rows.iter().enumerate() {
            let colour = if r.worm == 0 { Color32::from_rgb(255, 230, 120) } else { Color32::WHITE };
            let cell = |ui: &mut Ui, t: String| {
                ui.label(RichText::new(t).monospace().color(colour));
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}.", i + 1)).monospace().color(colour));
                ui.label(RichText::new(&r.name).monospace().color(colour));
            });
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
