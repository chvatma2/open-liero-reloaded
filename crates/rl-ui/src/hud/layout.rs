//! Where the HUD's parts go on a screen of any size and UI scale (Review
//! Focus 5): the health and the weapons bottom left, the minimap top right
//! (OLX's single-player corner, `CClient_Draw.cpp:611-617`), the clock top
//! centre, the kill feed top left (OLX's `LocalChatX/Y`), the keys bottom
//! right. Nothing ever covers the health or the weapons: on a small screen
//! the parts shrink (down to half size), the minimap first, then the feed
//! loses lines and the keys go.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use super::model::FEED_LINES;

/// OLX's local-game minimap, in its pixels (`MinimapW`/`MinimapH` of
/// `data/frontend/frontend.cfg` `[LocalGameInterface]`,
/// `docs/porting/olx-notes/presentation.md` §2.4).
pub const MINIMAP_OLX: Vec2 = vec2(119.0, 94.0);

/// The screen (points) at which the HUD has its full size; smaller screens
/// shrink it, down to [`MIN_UNIT`].
const FULL: Vec2 = vec2(640.0, 360.0);

/// The smallest HUD size factor.
const MIN_UNIT: f32 = 0.5;

/// The largest minimap scale (OLX pixels to points).
const MAX_MINIMAP: f32 = 2.5;

// Sizes at full size, in points.
const MARGIN: f32 = 8.0;
const GAP: f32 = 4.0;
const PANEL_W: f32 = 230.0;
const STATS_H: f32 = 18.0;
const BAR_H: f32 = 18.0;
const ROW_H: f32 = 18.0;
const CLOCK_W: f32 = 200.0;
const CLOCK_H: f32 = 26.0;
const FEED_W: f32 = 460.0;
const LINE_H: f32 = 17.0;
const HINT_MIN_W: f32 = 360.0;
const TEXT: f32 = 14.0;

/// The HUD's rectangles on one screen, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudLayout {
    pub screen: Rect,
    /// The HUD's size factor (1 full, down to 0.5 on a small screen).
    pub unit: f32,
    /// The body text size, in points.
    pub text: f32,
    /// The kill feed's line height.
    pub line: f32,
    /// Lives and kills above the health bar.
    pub health: Rect,
    /// One row per weapon slot.
    pub weapons: Rect,
    pub minimap: Rect,
    /// The time left and the kill limit.
    pub clock: Rect,
    /// The kill feed (no height: no room for a line).
    pub feed: Rect,
    /// The keys, when there is room.
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
        let at = |x: f32, y: f32| origin + vec2(x, y);

        // Bottom left: the weapons, the health above them.
        let panel_w = (PANEL_W * k).min((w - 2.0 * m).max(0.0));
        let weapons_h = ROW_H * k * rl_sim::MAX_SLOTS as f32 + gap;
        let weapons = Rect::from_min_size(at(m, h - m - weapons_h), vec2(panel_w, weapons_h));
        let health_h = (STATS_H + BAR_H) * k;
        let health = Rect::from_min_size(
            pos2(weapons.left(), weapons.top() - gap - health_h),
            vec2(panel_w, health_h),
        );

        // Top right: the minimap at OLX's proportions, as large as fits
        // beside the panel or above it.
        let mut s = (w * 0.2 / MINIMAP_OLX.x).min(h * 0.26 / MINIMAP_OLX.y).min(MAX_MINIMAP);
        let beside = (w - m - (health.right() - origin.x) - 2.0 * m) / MINIMAP_OLX.x;
        if s > beside {
            let above = (health.top() - origin.y - 2.0 * m) / MINIMAP_OLX.y;
            s = s.min(beside.max(above));
        }
        let s = s.max(0.0);
        let mini = MINIMAP_OLX * s;
        let minimap = Rect::from_min_size(at(w - m - mini.x, m), mini);

        // Top centre: the clock, left of the minimap if it reaches it.
        let clock_w = (CLOCK_W * k).min((minimap.left() - origin.x - 2.0 * m).max(0.0));
        let clock_h = CLOCK_H * k;
        let cx = (origin.x + w / 2.0).min(minimap.left() - m - clock_w / 2.0);
        let clock = Rect::from_min_size(pos2(cx - clock_w / 2.0, origin.y + m), vec2(clock_w, clock_h));

        // Top left: the feed, beside the clock, or below it when narrow,
        // never down to the health.
        let line = LINE_H * k;
        let beside_w = clock.left() - origin.x - 2.0 * m;
        let (top, width) = if beside_w >= 120.0 * k {
            (origin.y + m, beside_w.min(FEED_W * k))
        } else {
            let top = clock.bottom() + m;
            let right = if minimap.bottom() + m > top { minimap.left() - m } else { origin.x + w - m };
            (top, (right - origin.x - m).min(FEED_W * k))
        };
        let room = (health.top() - m - top).max(0.0);
        let lines = ((room / line).floor() as usize).min(FEED_LINES);
        let feed = Rect::from_min_size(pos2(origin.x + m, top), vec2(width.max(0.0), line * lines as f32));

        // Bottom right: the keys, beside the panel and below the minimap.
        let hint_left = health.right() + 2.0 * m;
        let hint = Rect::from_min_max(
            pos2(hint_left, origin.y + h - m - line),
            pos2(origin.x + w - m, origin.y + h - m),
        );
        let hint = (hint.width() >= HINT_MIN_W * k && hint.top() > minimap.bottom() + m).then_some(hint);

        HudLayout { screen, unit: k, text: TEXT * k, line, health, weapons, minimap, clock, feed, hint }
    }

    /// The minimap's scale: points per OLX minimap pixel.
    pub fn minimap_scale(&self) -> f32 {
        self.minimap.width() / MINIMAP_OLX.x
    }
}
