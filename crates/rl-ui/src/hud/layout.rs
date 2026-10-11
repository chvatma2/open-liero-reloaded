//! Where the HUD's parts go on a screen of any size and UI scale (Review
//! Focus 5). The HUD is an opaque bar along the bottom of the window, as
//! OLX 0.58's local game drew its status bar under the viewports
//! (`CClient_Draw.cpp:483-515` its background, commented out at
//! `d1ad953f`; `:137-138` the minimap at
//! 255, 382 of 640 × 480: centred, in the bar); the user's ruling after the
//! v0.1 playtest. In the bar: the health and the weapons on the left, the
//! minimap in the centre, the event log (kills, deaths, the mode's
//! messages; a multiplayer chat later) at the right edge. The playfield is the
//! window above the bar: the game draws its picture there, never under the
//! bar. Over the playfield: the clock top centre, the keys top right.
//!
//! The bar is [`BAR_FULL`] points high (the UI scale is egui's zoom, so it
//! grows with the scale). On a screen smaller than 640 × 360 points the
//! whole HUD shrinks, down to half size; on a narrow one the minimap
//! shrinks before the log gets narrower than its least.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// OLX's local-game minimap, in its pixels (`MinimapW`/`MinimapH` of
/// `data/frontend/frontend.cfg` `[LocalGameInterface]`,
/// `docs/porting/olx-notes/presentation.md` §2.4).
pub const MINIMAP_OLX: Vec2 = vec2(119.0, 94.0);

/// The screen (points) at which the HUD has its full size; smaller screens
/// shrink it, down to [`MIN_UNIT`].
const FULL: Vec2 = vec2(640.0, 360.0);

/// The smallest HUD size factor.
const MIN_UNIT: f32 = 0.5;

/// The largest minimap scale (OLX pixels to points): 178 × 141 points.
pub const MAX_MINIMAP: f32 = 1.5;

// Sizes at full size, in points.
const MARGIN: f32 = 8.0;
const GAP: f32 = 4.0;
const PANEL_W: f32 = 230.0;
const STATS_H: f32 = 18.0;
const BAR_H: f32 = 18.0;
const ROW_H: f32 = 18.0;
const CLOCK_W: f32 = 200.0;
const CLOCK_H: f32 = 26.0;
const LINE_H: f32 = 17.0;
const HINT_MIN_W: f32 = 300.0;
const TEXT: f32 = 14.0;
/// The log's least width (the minimap shrinks first) and its most (on a
/// wide screen it keeps to the right edge).
const LOG_MIN_W: f32 = 100.0;
const LOG_MAX_W: f32 = 560.0;
/// The bar's parts keep within this width, centred: on a wide screen they
/// stay near the picture rather than in the window's corners.
const CONTENT_MAX_W: f32 = 1600.0;

/// The bar's contents' height at full size: the health and the weapons.
const CONTENT_H: f32 = STATS_H + BAR_H + GAP + ROW_H * rl_sim::MAX_SLOTS as f32 + GAP;

/// The bar's height at full size, in points (150).
pub const BAR_FULL: f32 = CONTENT_H + 2.0 * MARGIN;

/// The HUD's rectangles on one screen, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudLayout {
    pub screen: Rect,
    /// The HUD's size factor (1 full, down to 0.5 on a small screen).
    pub unit: f32,
    /// The body text size, in points.
    pub text: f32,
    /// The log's line height.
    pub line: f32,
    /// The bar along the bottom, the full width.
    pub bar: Rect,
    /// The window above the bar, where the game draws.
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
        let origin = if screen.min.is_finite() { screen.min } else { Pos2::ZERO };
        let (w, h) = (finite(screen.width()), finite(screen.height()));
        let screen = Rect::from_min_size(origin, vec2(w, h));
        let k = (w / FULL.x).min(h / FULL.y).clamp(MIN_UNIT, 1.0);
        let (m, gap) = (MARGIN * k, GAP * k);

        let bar = Rect::from_min_max(pos2(origin.x, origin.y + h - BAR_FULL * k), screen.max);
        let bar = bar.intersect(screen);
        let playfield = Rect::from_min_max(origin, pos2(origin.x + w, bar.top()));
        let inner = bar.shrink(m);
        let inner_w = inner.width().min(CONTENT_MAX_W * k).max(0.0);
        let inner = Rect::from_center_size(inner.center(), vec2(inner_w, inner.height().max(0.0)));

        // Left: the weapons at the bottom, the health above them.
        let panel_w = (PANEL_W * k).min(inner.width().max(0.0));
        let weapons_h = ROW_H * k * rl_sim::MAX_SLOTS as f32 + gap;
        let weapons =
            Rect::from_min_size(pos2(inner.left(), inner.bottom() - weapons_h), vec2(panel_w, weapons_h));
        let health_h = (STATS_H + BAR_H) * k;
        let health = Rect::from_min_size(
            pos2(weapons.left(), weapons.top() - gap - health_h),
            vec2(panel_w, health_h),
        );

        // Centre: the minimap at OLX's proportions, as tall as the bar
        // allows, narrower when the log would have less than its least.
        let left = health.right() + 2.0 * m;
        let log_min = LOG_MIN_W * k;
        let room = (inner.right() - 2.0 * m - log_min - left).max(0.0);
        let s = (inner.height() / MINIMAP_OLX.y).min(MAX_MINIMAP).min(room / MINIMAP_OLX.x).max(0.0);
        let mini = MINIMAP_OLX * s;
        let x = (origin.x + w / 2.0 - mini.x / 2.0).clamp(left, (left + room - mini.x).max(left));
        let minimap = Rect::from_min_size(pos2(x, inner.center().y - mini.y / 2.0), mini);

        // Right: the log, whole lines, bottom-aligned, at the right edge.
        let line = LINE_H * k;
        let lines = (inner.height() / line).floor().max(0.0);
        let log_left = (minimap.right() + 2.0 * m).max(inner.right() - LOG_MAX_W * k);
        let log = Rect::from_min_max(
            pos2(log_left, inner.bottom() - lines * line),
            pos2(inner.right(), inner.bottom()),
        );

        // Top centre of the playfield: the clock; top right (no further
        // than the bar's parts): the keys.
        let clock_w = (CLOCK_W * k).min((w - 2.0 * m).max(0.0));
        let clock_h = (CLOCK_H * k).min(playfield.height());
        let clock = Rect::from_min_size(
            pos2(origin.x + w / 2.0 - clock_w / 2.0, origin.y + m.min(playfield.height() - clock_h)),
            vec2(clock_w, clock_h),
        );
        let hint = Rect::from_min_max(
            pos2(clock.right() + 2.0 * m, origin.y + m),
            pos2((origin.x + w - m).min(inner.right()), origin.y + m + line),
        );
        let hint = (hint.width() >= HINT_MIN_W * k && playfield.contains_rect(hint)).then_some(hint);

        HudLayout {
            screen,
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

    /// The bottom-left panel as painted: the health and the weapons with
    /// their frames.
    pub fn panel(&self) -> Rect {
        self.health.union(self.weapons).expand(2.0 * self.unit)
    }

    /// How many of the log's lines show.
    pub fn log_lines(&self) -> usize {
        if self.line > 0.0 { (self.log.height() / self.line).round() as usize } else { 0 }
    }
}

/// The bar's height in points on a screen of `size` points (what the game
/// leaves under its picture): [`HudLayout::new`]'s.
pub fn bar_height(size: Vec2) -> f32 {
    HudLayout::new(Rect::from_min_size(Pos2::ZERO, size)).bar.height()
}
