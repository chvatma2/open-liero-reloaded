//! Where the HUD's parts go (Review Focus 5). The game is designed for a
//! 16:9 screen: the largest 16:9 rectangle centred in the window is its
//! [`frame`] (all of a 16:9 window; black above and below on a narrower
//! one, at the sides on a wider one). The bottom 20 % of the frame is an
//! opaque bar, as OLX 0.58's local game drew its status bar under the
//! viewports (`CClient_Draw.cpp:483-515`, its background
//! `DrawRectFill(0, 382, 640, 480)`, commented out at `d1ad953f`;
//! `:137-138` the minimap at 255, 382 of 640 × 480, 128 × 96: centred, a
//! fifth of the height); the user's ruling after the v0.1 playtest. In
//! the bar: the health and the weapons on the left, the minimap in the
//! centre, as tall as the bar allows, the event log (kills, deaths, the
//! mode's messages; a multiplayer chat later) on the right. The playfield
//! is the frame above the bar, 16 : 7.2: the game draws its fixed view
//! there (`rl_scene::Viewport::GAME`), never under the bar. Over the
//! playfield: the clock top centre, the keys top right.
//!
//! Every size is given for a frame [`REFERENCE_HEIGHT`] (720) points high
//! and scaled with the frame (`unit`), so the HUD takes the same share of
//! the frame on every window and at every UI scale.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// OLX's local-game minimap, in its pixels (`MinimapW`/`MinimapH` of
/// `data/frontend/frontend.cfg` `[LocalGameInterface]`,
/// `docs/porting/olx-notes/presentation.md` §2.4).
pub const MINIMAP_OLX: Vec2 = vec2(119.0, 94.0);

/// The frame's height at which the HUD's sizes below are given.
pub const REFERENCE_HEIGHT: f32 = 720.0;

/// The frame's aspect (16:9).
const ASPECT: f32 = 16.0 / 9.0;

/// The bar's share of the frame's height (OLX 0.58: 98 of 480 pixels).
pub const BAR_SHARE: f32 = 0.2;

// Sizes for a frame of REFERENCE_HEIGHT (a bar of 144).
const MARGIN: f32 = 8.0;
const GAP: f32 = 4.0;
/// The minimap's space above and below it in the bar.
const MINIMAP_PAD: f32 = 6.0;
const PANEL_W: f32 = 300.0;
const STATS_H: f32 = 16.0;
const BAR_H: f32 = 16.0;
const ROW_H: f32 = 17.6;
const CLOCK_W: f32 = 200.0;
const CLOCK_H: f32 = 26.0;
const LINE_H: f32 = 16.0;
const HINT_MIN_W: f32 = 300.0;
const TEXT: f32 = 13.0;

/// The largest 16:9 rectangle centred in `screen` (any unit: points or
/// pixels); nothing for a screen of no size or not a number.
pub fn frame(screen: Rect) -> Rect {
    let (w, h) = (finite(screen.width()), finite(screen.height()));
    let centre = if screen.center().is_finite() { screen.center() } else { Pos2::ZERO };
    let size = if w > h * ASPECT { vec2(h * ASPECT, h) } else { vec2(w, w / ASPECT) };
    Rect::from_center_size(centre, size)
}

/// The playfield of `screen`: its [`frame`] above the bar (16 : 7.2).
pub fn playfield(screen: Rect) -> Rect {
    let f = frame(screen);
    Rect::from_min_max(f.min, pos2(f.right(), f.bottom() - f.height() * BAR_SHARE))
}

/// The HUD's rectangles on one screen, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudLayout {
    pub screen: Rect,
    /// The 16:9 frame the game is designed for.
    pub frame: Rect,
    /// Points per reference point: the frame's height over
    /// [`REFERENCE_HEIGHT`].
    pub unit: f32,
    /// The body text size, in points.
    pub text: f32,
    /// The log's line height.
    pub line: f32,
    /// The bar: the bottom fifth of the frame.
    pub bar: Rect,
    /// The frame above the bar, where the game draws.
    pub playfield: Rect,
    /// Lives and kills above the health bar.
    pub health: Rect,
    /// One row per weapon slot.
    pub weapons: Rect,
    pub minimap: Rect,
    /// The event log, whole lines.
    pub log: Rect,
    /// The time left and the kill limit, top centre over the playfield.
    pub clock: Rect,
    /// The keys, top right over the playfield, when there is room.
    pub hint: Option<Rect>,
}

/// `v` if it is a finite size, else 0.
fn finite(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

impl HudLayout {
    /// The layout for `screen` (points; what egui's whole window is).
    pub fn new(screen: Rect) -> HudLayout {
        let frame = frame(screen);
        let screen = if screen.is_finite() { screen } else { frame };
        let k = frame.height() / REFERENCE_HEIGHT;
        let (m, gap) = (MARGIN * k, GAP * k);
        let at = |x: f32, y: f32| frame.min + vec2(x, y) * k;

        let playfield = playfield(screen);
        let bar = Rect::from_min_max(pos2(frame.left(), playfield.bottom()), frame.max);
        let inner = bar.shrink(m);

        // Left: the weapons at the bottom, the health above them.
        let weapons_h = ROW_H * k * rl_sim::MAX_SLOTS as f32 + gap;
        let weapons =
            Rect::from_min_size(pos2(inner.left(), inner.bottom() - weapons_h), vec2(PANEL_W * k, weapons_h));
        let health_h = (STATS_H + BAR_H) * k;
        let health = Rect::from_min_size(
            pos2(weapons.left(), weapons.top() - gap - health_h),
            vec2(PANEL_W * k, health_h),
        );

        // Centre: the minimap at OLX's proportions, as tall as the bar.
        let mini_h = (bar.height() - 2.0 * MINIMAP_PAD * k).max(0.0);
        let mini = vec2(mini_h * MINIMAP_OLX.x / MINIMAP_OLX.y, mini_h);
        let minimap = Rect::from_center_size(bar.center(), mini);

        // Right: the log, whole lines, bottom-aligned.
        let line = LINE_H * k;
        let lines = if line > 0.0 { (inner.height() / line + 1e-3).floor() } else { 0.0 };
        let log = Rect::from_min_max(
            pos2(minimap.right() + 2.0 * m, inner.bottom() - lines * line),
            pos2(inner.right(), inner.bottom()),
        );

        // Over the playfield: the clock top centre, the keys top right.
        let clock = Rect::from_center_size(
            pos2(frame.center().x, at(0.0, MARGIN + CLOCK_H / 2.0).y),
            vec2(CLOCK_W, CLOCK_H) * k,
        );
        let hint = Rect::from_min_max(
            pos2(clock.right() + 2.0 * m, playfield.top() + m),
            pos2(playfield.right() - m, playfield.top() + m + line),
        );
        let hint = (hint.width() >= HINT_MIN_W * k).then_some(hint);

        HudLayout {
            screen,
            frame,
            unit: k,
            text: TEXT * k,
            line,
            bar,
            playfield,
            health,
            weapons,
            minimap,
            log,
            clock,
            hint,
        }
    }

    /// The minimap's scale: points per OLX minimap pixel.
    pub fn minimap_scale(&self) -> f32 {
        self.minimap.width() / MINIMAP_OLX.x
    }

    /// The left panel as painted: the health and the weapons with their
    /// frames.
    pub fn panel(&self) -> Rect {
        self.health.union(self.weapons).expand(2.0 * self.unit)
    }

    /// How many of the log's lines show.
    pub fn log_lines(&self) -> usize {
        if self.line > 0.0 { (self.log.height() / self.line).round() as usize } else { 0 }
    }
}
