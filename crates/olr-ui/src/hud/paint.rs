//! Paints a [`HudView`] where a [`HudLayout`] says: OLX's colours
//! (`docs/porting/olx-notes/presentation.md` §9) in an opaque bar along
//! the bottom, text with a one-pixel shadow.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Stroke, StrokeKind, Ui, pos2, vec2};

use super::layout::HudLayout;
use super::minimap::{ALPHA, dot_colour, dot_rect};
use super::model::{LineKind, LogLine, Row, clock, lives_text};
use super::split::{Shared, SplitLayout, ViewPanel};
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
/// The bar (opaque), its top edge, and its parts' panels.
const BAR_FILL: Color32 = Color32::from_rgb(18, 18, 23);
const BAR_EDGE: Color32 = Color32::from_rgb(62, 62, 74);
const PANEL: Color32 = Color32::from_rgb(30, 30, 38);
const LOG_FILL: Color32 = Color32::from_rgb(11, 11, 15);

fn shadowed(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, colour: Color32) -> Rect {
    p.text(pos + vec2(1.0, 1.0), anchor, text, font.clone(), Color32::from_black_alpha(200));
    p.text(pos, anchor, text, font, colour)
}

/// Paints `view` over `ui`'s whole area: the bar along the bottom
/// (opaque: the game draws above it), the clock, notices and keys over the
/// playfield; with two or more views (`HudView::views`), split screen's
/// layout instead.
pub fn show(ui: &Ui, view: &HudView) {
    if view.views.len() >= 2 {
        show_split(ui, view);
        return;
    }
    let l = HudLayout::new(ui.max_rect(), view.arrangement);
    let p = ui.painter();
    p.rect_filled(l.bar, 0.0, BAR_FILL);
    let edge = Stroke::new(1.0, BAR_EDGE);
    p.line_segment([l.bar.left_top(), l.bar.right_top()], edge);
    if let Some([left, right]) = l.sides {
        // The side layout's columns: opaque as the bar, an edge at the frame.
        p.rect_filled(left, 0.0, BAR_FILL);
        p.rect_filled(right, 0.0, BAR_FILL);
        p.line_segment([left.right_top(), left.right_bottom()], edge);
        p.line_segment([right.left_top(), right.left_bottom()], edge);
    }
    if let Some(w) = &view.worm {
        health(p, &l, w);
        if !w.slots.is_empty() {
            weapons(p, &l, w);
        }
    }
    if let Some(m) = &view.minimap {
        minimap(p, l.minimap, m);
    }
    // In the bar, an inset; beside the frame the column is the log's.
    let lines = Lines { rect: l.log, unit: l.unit, text: l.log_text, line: l.line, inset: l.sides.is_none() };
    log(p, &lines, &view.log);
    top(p, &Top { clock: l.clock, unit: l.unit, text: l.text, line: l.line }, view);
    if let (Some(hint), Some(rect)) = (&view.hint, l.hint) {
        let p = p.with_clip_rect(rect);
        let font = FontId::proportional(l.text * 0.85);
        shadowed(&p, rect.right_top(), Align2::RIGHT_TOP, hint, font, Color32::from_gray(190));
    }
    if view.game_over {
        let at = l.playfield.center() - vec2(0.0, 40.0 * l.unit);
        let font = FontId::proportional(40.0 * l.unit);
        shadowed(p, at, Align2::CENTER_CENTER, "Game Over", font, Color32::from_rgb(255, 200, 80));
    }
    if let Some(rows) = view.scoreboard.as_ref().filter(|r| !r.is_empty()) {
        scoreboard(ui, rows, l.playfield.center() - l.screen.center());
    }
}

/// Split screen (issue #30): the views' seams are the game's black; the
/// shared space opaque as the bar, holding the minimap, the clock and the
/// log; a compact HUD in each view; "Game Over" and the scoreboard
/// centred on the screen. No keys: the views' corners are their HUDs'.
fn show_split(ui: &Ui, view: &HudView) {
    let l = SplitLayout::new(ui.max_rect(), view.views.len());
    let p = ui.painter();
    for r in l.panels() {
        p.rect_filled(r, 0.0, BAR_FILL);
    }
    for (i, v) in view.views.iter().enumerate() {
        if let (Some(w), Some(panel)) = (&v.worm, l.view_panel(i)) {
            compact(p, &panel, l.unit, w, v.colour);
        }
    }
    if let Some(m) = &view.minimap {
        minimap(p, l.minimap, m);
    }
    let inset = l.shared != Shared::Sides;
    let lines = Lines { rect: l.log, unit: l.unit, text: l.log_text, line: l.line, inset };
    log(p, &lines, &view.log);
    top(p, &Top { clock: l.clock, unit: l.unit, text: l.text, line: l.line }, view);
    if view.game_over {
        let at = l.screen.center();
        let font = FontId::proportional(40.0 * l.unit);
        shadowed(p, at, Align2::CENTER_CENTER, "Game Over", font, Color32::from_rgb(255, 200, 80));
    }
    if let Some(rows) = view.scoreboard.as_ref().filter(|r| !r.is_empty()) {
        scoreboard(ui, rows, egui::Vec2::ZERO);
    }
}

/// A view's compact HUD (issue #30; OLX's fork drew each viewport's
/// health and weapon bars at its bottom left, `CClient_Draw.cpp:1192-1257`):
/// the player's colour and name with its lives and kills, the health bar
/// (or why the worm is down), the weapon in hand with its charge in OLX's
/// bar colours.
fn compact(p: &Painter, v: &ViewPanel, k: f32, w: &WormHud, colour: Color32) {
    p.rect_filled(v.panel, 4.0 * k, Color32::from_black_alpha(160));
    let font = FontId::proportional(v.text);
    let row = v.name;
    let side = row.height() * 0.7;
    let swatch = Rect::from_center_size(pos2(row.left() + side / 2.0, row.center().y), vec2(side, side));
    p.rect_filled(swatch, 2.0 * k, colour);
    p.rect_stroke(swatch, 2.0 * k, Stroke::new(1.0, Color32::from_black_alpha(200)), StrokeKind::Inside);
    // The kills and lives right, the name in what is left.
    let mut right = row.right();
    let kills = shadowed(
        p,
        pos2(right, row.center().y),
        Align2::RIGHT_CENTER,
        &format!("Kills {}", w.kills),
        font.clone(),
        KILLS,
    );
    right = kills.left() - 6.0 * k;
    if w.lives != -2 {
        let at = pos2(right, row.center().y);
        let lives = format!("Lives {}", lives_text(w.lives));
        right = shadowed(p, at, Align2::RIGHT_CENTER, &lives, font.clone(), LIVES).left() - 6.0 * k;
    }
    let name = Rect::from_min_max(pos2(swatch.right() + 4.0 * k, row.top()), pos2(right, row.bottom()));
    if name.width() > 0.0 {
        let at = pos2(name.left(), row.center().y);
        shadowed(&p.with_clip_rect(name), at, Align2::LEFT_CENTER, &w.name, font.clone(), Color32::WHITE);
    }
    let bar = v.health;
    if w.alive {
        p.rect_filled(bar, 2.0 * k, BAR_BG);
        let fill = Rect::from_min_size(bar.min, vec2(bar.width() * w.health / 100.0, bar.height()));
        p.rect_filled(fill, 2.0 * k, HEALTH);
    } else {
        let text = w.status.as_deref().unwrap_or("Dead");
        let clip = p.with_clip_rect(bar.expand2(vec2(0.0, 3.0 * k)));
        let small = FontId::proportional(v.text * 0.85);
        shadowed(&clip, bar.center(), Align2::CENTER_CENTER, text, small, Color32::from_rgb(255, 200, 80));
    }
    if let Some(s) = w.slots.get(w.current).filter(|_| w.alive) {
        let r = v.weapon;
        let split = r.left() + r.width() * 0.58;
        let name = Rect::from_min_max(r.min, pos2(split - 4.0 * k, r.bottom()));
        let at = pos2(r.left(), r.center().y);
        shadowed(&p.with_clip_rect(name), at, Align2::LEFT_CENTER, &s.name, font, WEAPON_LABEL);
        let bar = Rect::from_min_max(pos2(split, r.top()), r.right_bottom()).shrink2(vec2(0.0, 3.0 * k));
        p.rect_filled(bar, 1.0, Color32::from_black_alpha(160));
        p.rect_filled(
            Rect::from_min_size(bar.min, vec2(bar.width() * s.charge, bar.height())),
            1.0,
            bar_colour(s.bar),
        );
    }
}

/// OLX's weapon bar colour of a state.
fn bar_colour(bar: Bar) -> Color32 {
    match bar {
        Bar::Ready => READY,
        Bar::Reloading => RELOADING,
        Bar::Cooldown => COOLDOWN,
    }
}

/// Where the event log goes and how it reads.
struct Lines {
    rect: Rect,
    unit: f32,
    text: f32,
    line: f32,
    /// Drawn on an inset (in a bar); else its column is its own.
    inset: bool,
}

/// The event log, the newest line at the bottom, a long one wrapped;
/// older ones go up and off the top.
fn log(p: &Painter, l: &Lines, lines: &[LogLine]) {
    let r = l.rect;
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    if l.inset {
        p.rect_filled(r.expand(2.0 * l.unit), 4.0 * l.unit, LOG_FILL);
    }
    let clip = p.with_clip_rect(r);
    let font = FontId::proportional(l.text);
    let pad = 4.0 * l.unit;
    let mut bottom = r.bottom();
    for line in lines.iter().rev() {
        if bottom <= r.top() {
            break;
        }
        let colour = match line.kind {
            LineKind::Event => Color32::from_gray(225),
            LineKind::Chat => Color32::from_rgb(255, 230, 140),
        };
        let galley = p.layout(line.text.clone(), font.clone(), colour, (r.width() - 2.0 * pad).max(1.0));
        // Each wrapped row a line high, centred in it.
        let rows = galley.rows.len().max(1) as f32;
        let top = bottom - l.line * rows;
        let y = top + (l.line * rows - galley.size().y) / 2.0;
        clip.galley(pos2(r.left() + pad, y), galley, colour);
        bottom = top;
    }
}

/// Lives and kills, then the health bar (or why the worm is down).
fn health(p: &Painter, l: &HudLayout, w: &WormHud) {
    let r = l.health;
    p.rect_filled(r.expand(2.0 * l.unit), 4.0 * l.unit, PANEL);
    let font = FontId::proportional(l.panel_text);
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
    let n = olr_sim::MAX_SLOTS as f32;
    let h = (r.height() - 4.0 * l.unit) / n;
    let font = FontId::proportional(l.panel_text * 0.95);
    for (i, s) in w.slots.iter().enumerate().take(olr_sim::MAX_SLOTS) {
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
        let fore = bar_colour(s.bar);
        p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * s.charge, bar.height())), 1.0, fore);
    }
}

/// Where the clock goes and how it reads.
struct Top {
    clock: Rect,
    unit: f32,
    text: f32,
    line: f32,
}

/// The clock and kill limit, and the notices under them.
fn top(p: &Painter, l: &Top, v: &HudView) {
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
}

/// The level at half opacity and a blinking dot per worm (no view
/// rectangle: the player's bigger dot is enough, the user's ruling).
fn minimap(p: &Painter, r: Rect, m: &MinimapView) {
    if r.width() <= 0.0 {
        return;
    }
    p.rect_filled(r, 0.0, Color32::from_black_alpha(ALPHA));
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    p.image(m.texture, r, uv, Color32::from_white_alpha(ALPHA));
    for d in &m.dots {
        p.rect_filled(dot_rect(d.pos, m.map, r, d.big), 0.0, dot_colour(d.colour, m.blink));
    }
}

/// The scoreboard (OLX's `ShowScore`, Tab held), centred over the
/// playfield (`offset` from the window's centre).
fn scoreboard(ui: &Ui, rows: &[Row], offset: egui::Vec2) {
    egui::Area::new(egui::Id::new("scoreboard"))
        .anchor(Align2::CENTER_CENTER, offset)
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
/// the score and every death and suicide, the mode's counts, not OLX's
/// score-gated `iDeaths`; `docs/porting/STATUS.md`); the followed worm's (the
/// player's own) in yellow.
pub fn table(ui: &mut Ui, rows: &[Row]) {
    grid(ui, rows, &Look { font: None, gap: 18.0, name_chars: usize::MAX });
}

/// The results' [`table`] in at most `width` points (issue #49): as it is
/// when it fits; else in a smaller font with the columns closer, the
/// names cut to what is left (a small window at a large UI scale).
pub fn table_within(ui: &mut Ui, rows: &[Row], width: f32) {
    let normal = egui::TextStyle::Monospace.resolve(ui.style());
    if grid_width(ui, rows, &normal, 18.0, usize::MAX) <= width {
        return table(ui, rows);
    }
    let font = FontId::monospace(COMPACT_FONT);
    let char_w = text_width(ui, "0", &font).max(1.0);
    let others = grid_width(ui, rows, &font, COMPACT_GAP, 0);
    let name_chars = ((width - others) / char_w).floor().max(4.0) as usize;
    grid(ui, rows, &Look { font: Some(font), gap: COMPACT_GAP, name_chars });
}

/// The results' table's font size and column gap when it is cut to fit.
const COMPACT_FONT: f32 = 11.0;
const COMPACT_GAP: f32 = 8.0;

const HEADS: [&str; 7] = ["", "Lives", "Score", "Kills", "Deaths", "Suicides", "Damage"];

/// How a table is drawn: its font (`None`: the monospace style), the
/// space between its columns and the most of a name shown.
struct Look {
    font: Option<FontId>,
    gap: f32,
    name_chars: usize,
}

fn cells(r: &Row) -> [String; 6] {
    [
        r.lives.clone(),
        r.score.to_string(),
        r.kills.to_string(),
        r.deaths.to_string(),
        r.suicides.to_string(),
        r.damage.to_string(),
    ]
}

/// `name` cut to `chars` characters, the last an ellipsis.
fn cut(name: &str, chars: usize) -> String {
    if name.chars().count() <= chars {
        return name.to_string();
    }
    name.chars().take(chars.saturating_sub(1)).chain(['…']).collect()
}

fn text_width(ui: &Ui, t: &str, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|f| f.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x)
}

/// The table's width in `font` with `gap` between its columns, its names
/// cut to `name_chars` (0: the names' column without its names).
fn grid_width(ui: &Ui, rows: &[Row], font: &FontId, gap: f32, name_chars: usize) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    let mut widths = HEADS.map(|h| text_width(ui, h, font));
    for (i, r) in rows.iter().enumerate() {
        let name = if name_chars == 0 { String::new() } else { cut(&r.name, name_chars) };
        let first = text_width(ui, &format!("{}.", i + 1), font) + spacing + text_width(ui, &name, font);
        widths[0] = widths[0].max(first);
        for (w, c) in widths[1..].iter_mut().zip(cells(r)) {
            *w = w.max(text_width(ui, &c, font));
        }
    }
    widths.iter().sum::<f32>() + gap * (widths.len() - 1) as f32
}

fn grid(ui: &mut Ui, rows: &[Row], look: &Look) {
    let text = |t: &str| match &look.font {
        Some(f) => RichText::new(t).font(f.clone()),
        None => RichText::new(t).monospace(),
    };
    let head = |ui: &mut Ui, t: &str| {
        ui.label(text(t).color(Color32::from_gray(170)));
    };
    egui::Grid::new("scores").striped(true).spacing([look.gap, 4.0]).show(ui, |ui| {
        for h in HEADS {
            head(ui, h);
        }
        ui.end_row();
        for (i, r) in rows.iter().enumerate() {
            let colour = if r.left {
                Color32::from_gray(140)
            } else if r.followed {
                Color32::from_rgb(255, 230, 120)
            } else {
                Color32::WHITE
            };
            let cell = |ui: &mut Ui, t: String| {
                ui.label(text(&t).color(colour));
            };
            ui.horizontal(|ui| {
                ui.label(text(&format!("{}.", i + 1)).color(colour));
                ui.label(text(&cut(&r.name, look.name_chars)).color(colour));
            });
            for c in cells(r) {
                cell(ui, c);
            }
            ui.end_row();
        }
    });
}
