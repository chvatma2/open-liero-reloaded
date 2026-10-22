//! Split screen's layout (issue #30): 2 to 4 local players' views on one
//! screen. Every view is the solo playfield's shape (16 : 7.2, where the
//! game draws `olr_scene::Viewport::GAME`'s 427 × 192 world pixels), so
//! each player sees the solo player's area of the level, drawn smaller
//! (the maintainer's fair-view rule; OLX's split gave each player less of
//! the level, `docs/porting/STATUS.md`). The views are equal, at the
//! largest size the window allows, a thin black seam between them (OLX:
//! 4 screen pixels of 480, `CClient.cpp:1653-1677`, the gaps filled with
//! `clViewportSplit` by `CClient_Draw.cpp:561-609`).
//!
//! The arrangement is the one giving the largest views of those listed in
//! [`candidates`], the maintainer's first on a tie: 2 stacked top and
//! bottom, 3 and 4 in a 2 × 2 grid; side by side, or in a row, where a
//! window is wide enough for that to be larger. One shared minimap, clock
//! and event log go in the spare space ([`Shared`]): with 3 views in a
//! grid the empty quadrant (as OLX's radar), else a strip under the views
//! or the columns beside them. Each view has a compact HUD at its bottom
//! left ([`ViewPanel`]; OLX's fork drew each viewport's bars there,
//! `CClient_Draw.cpp:1192-1209`).
//!
//! As [`super::HudLayout`], every size is given for a frame
//! [`REFERENCE_HEIGHT`] high and scaled with the window's 16:9 frame
//! (`unit`), so the layout is the same picture in points or pixels, at
//! every size and UI scale.
//!
//! OLX: src/client/CClient.cpp:1527-1551, 1653-1744

use egui::{Rect, Vec2, pos2, vec2};

use super::layout::{MINIMAP_OLX, REFERENCE_HEIGHT, frame};

/// Split screen's most views (OLX's fork: four local players).
pub const MAX_VIEWS: usize = 4;

/// A view's shape: the solo playfield's, 16 : 7.2 (`super::playfield`).
pub const VIEW_ASPECT: f32 = 16.0 / 7.2;

// Sizes for a frame of REFERENCE_HEIGHT.
/// The seam between views: OLX's 4 pixels of a 480-pixel screen.
const SEAM: f32 = 6.0;
/// The strip under the views at its least.
const STRIP_MIN: f32 = 120.0;
/// Each column beside the views at its least (the side layout's quarter
/// of the frame's height, `super::layout::SIDE_SHARE`).
const SIDE_MIN: f32 = 180.0;
const MARGIN: f32 = 8.0;
const CLOCK_H: f32 = 26.0;
const LINE_H: f32 = 16.0;
const TEXT: f32 = 13.0;
/// The lines of notices kept under the clock.
const NOTICE_LINES: f32 = 2.0;
/// The shared minimap's largest height.
const MINIMAP_MAX_H: f32 = 200.0;

// The compact HUD in each view.
const PANEL_W: f32 = 184.0;
const PANEL_PAD: f32 = 4.0;
const PANEL_ROW: f32 = 15.0;
const PANEL_BAR: f32 = 9.0;
const PANEL_GAP: f32 = 3.0;
const PANEL_TEXT: f32 = 12.0;

/// Where the shared minimap, clock and log go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shared {
    /// The grid's empty cell (3 views in a 2 × 2 grid, as OLX's radar).
    Cell,
    /// A strip under the views, as the solo bar.
    Bottom,
    /// The columns left and right of the views: the clock and minimap
    /// left, the log right.
    Sides,
}

/// The arrangements tried for `count` views, `(columns, rows, shared)`,
/// the maintainer's first: 2 stacked top and bottom, 3 and 4 in a 2 × 2
/// grid. Each needs room for its shared space: the strip's or the side
/// columns' least, or the grid's empty cell.
pub fn candidates(count: usize) -> &'static [(usize, usize, Shared)] {
    use Shared::{Bottom, Cell, Sides};
    match count {
        0..=2 => &[(1, 2, Sides), (1, 2, Bottom), (2, 1, Bottom)],
        3 => &[(2, 2, Cell), (3, 1, Bottom), (1, 3, Sides)],
        _ => &[(2, 2, Bottom), (2, 2, Sides), (4, 1, Bottom), (1, 4, Sides)],
    }
}

/// The compact HUD of one view, at its bottom left: the player's colour
/// and name (and kills, lives) on a row, the health bar, the weapon in
/// hand with its charge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPanel {
    pub panel: Rect,
    pub name: Rect,
    pub health: Rect,
    pub weapon: Rect,
    /// The text size, in points.
    pub text: f32,
}

/// Split screen's rectangles on one screen, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitLayout {
    pub screen: Rect,
    /// The window's 16:9 frame (the UI's scale follows its height).
    pub frame: Rect,
    /// Points per reference point: the frame's height over
    /// [`REFERENCE_HEIGHT`].
    pub unit: f32,
    /// The clock's and the notices' text size, the log's, and its line.
    pub text: f32,
    pub log_text: f32,
    pub line: f32,
    /// How many views (2 to [`MAX_VIEWS`]).
    pub count: usize,
    pub cols: usize,
    pub rows: usize,
    /// The gap between neighbouring views.
    pub seam: f32,
    pub shared: Shared,
    /// The views, in order: left to right, then top to bottom.
    views: [Rect; MAX_VIEWS],
    /// The shared space's backgrounds (one, or the two side columns).
    panels: [Rect; 2],
    /// The clock and kill limit, the notices under them.
    pub clock: Rect,
    pub minimap: Rect,
    /// The event log, whole lines.
    pub log: Rect,
}

/// `v` if it is a finite size, else 0.
fn finite(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// The height of each of `cols` × `rows` views with `seam` between them
/// in `w` × `h`.
fn view_height(w: f32, h: f32, cols: usize, rows: usize, seam: f32) -> f32 {
    let (c, r) = (cols as f32, rows as f32);
    let by_w = (w - (c - 1.0) * seam) / c / VIEW_ASPECT;
    let by_h = (h - (r - 1.0) * seam) / r;
    finite(by_w.min(by_h))
}

/// Whole lines of `line` in `r`, bottom-aligned.
fn whole_lines(r: Rect, line: f32) -> Rect {
    let lines = if line > 0.0 { (finite(r.height()) / line + 1e-3).floor() } else { 0.0 };
    Rect::from_min_max(pos2(r.left(), r.bottom() - lines * line), r.max)
}

/// A minimap at OLX's proportions, at most `max_h` high, fitting `r`.
fn minimap_in(r: Rect, max_h: f32) -> Vec2 {
    let aspect = MINIMAP_OLX.x / MINIMAP_OLX.y;
    let h = finite(r.height()).min(finite(r.width()) / aspect).min(max_h);
    vec2(h * aspect, h)
}

impl SplitLayout {
    /// The layout of `count` views (clamped to 2 to [`MAX_VIEWS`]) on
    /// `screen` (points; what egui's whole window is).
    pub fn new(screen: Rect, count: usize) -> SplitLayout {
        let frame = frame(screen);
        let screen = if screen.is_finite() { screen } else { frame };
        let k = frame.height() / REFERENCE_HEIGHT;
        let count = count.clamp(2, MAX_VIEWS);
        let seam = SEAM * k;
        let (w, h) = (finite(screen.width()), finite(screen.height()));

        // The arrangement with the largest views; the earlier on a tie.
        let mut best = (-1.0, 1, 2, Shared::Sides);
        for &(cols, rows, shared) in candidates(count) {
            let (aw, ah) = match shared {
                Shared::Cell => (w, h),
                Shared::Bottom => (w, h - STRIP_MIN * k),
                Shared::Sides => (w - 2.0 * SIDE_MIN * k, h),
            };
            let vh = view_height(aw, ah, cols, rows, seam);
            if vh > best.0 * (1.0 + 1e-4) {
                best = (vh, cols, rows, shared);
            }
        }
        let (vh, cols, rows, shared) = best;
        let view = vec2(vh * VIEW_ASPECT, vh);
        let grid = vec2(
            cols as f32 * view.x + (cols - 1) as f32 * seam,
            rows as f32 * view.y + (rows - 1) as f32 * seam,
        );
        // The grid: centred; above the strip with one.
        let origin = match shared {
            Shared::Bottom => pos2(screen.center().x - grid.x / 2.0, screen.top()),
            Shared::Cell | Shared::Sides => screen.center() - grid / 2.0,
        };
        let cell = |i: usize| {
            let (c, r) = ((i % cols) as f32, (i / cols) as f32);
            Rect::from_min_size(origin + vec2(c * (view.x + seam), r * (view.y + seam)), view)
        };
        let mut views = [Rect::NOTHING; MAX_VIEWS];
        for (i, v) in views.iter_mut().enumerate().take(count) {
            *v = cell(i);
        }
        let grid = Rect::from_min_size(origin, grid);

        let (m, line) = (MARGIN * k, LINE_H * k);
        let info_h = (CLOCK_H + NOTICE_LINES * LINE_H) * k;
        let max_h = MINIMAP_MAX_H * k;
        let (panels, clock, minimap, log) = match shared {
            Shared::Cell => {
                // The clock across the top, the minimap left, the log right.
                let c = cell(count);
                let inner = c.shrink(m);
                let clock = Rect::from_min_size(inner.min, vec2(inner.width(), CLOCK_H * k));
                let body = Rect::from_min_max(pos2(inner.left(), inner.top() + info_h), inner.max);
                let half = Rect::from_min_size(body.min, vec2(body.width() / 2.0, body.height()));
                let mini = Rect::from_center_size(
                    pos2(body.left() + minimap_in(half, max_h).x / 2.0, body.center().y),
                    minimap_in(half, max_h),
                );
                let log = Rect::from_min_max(pos2(mini.right() + 2.0 * m, body.top()), body.max);
                ([c, Rect::NOTHING], clock, mini, whole_lines(log, line))
            }
            Shared::Bottom => {
                // As the solo bar: the minimap centred, the log right of it,
                // the clock left of it.
                let strip = Rect::from_min_max(pos2(screen.left(), grid.bottom()), screen.max);
                let inner = strip.shrink(m);
                let size = minimap_in(inner, max_h);
                let mini = Rect::from_center_size(pos2(screen.center().x, inner.center().y), size);
                let left = Rect::from_min_max(inner.min, pos2(mini.left() - 2.0 * m, inner.bottom()));
                let clock = Rect::from_min_size(left.min, vec2(finite(left.width()), CLOCK_H * k));
                let log = Rect::from_min_max(pos2(mini.right() + 2.0 * m, inner.top()), inner.max);
                ([strip, Rect::NOTHING], clock, mini, whole_lines(log, line))
            }
            Shared::Sides => {
                // Left: the clock, the minimap under it; right: the log.
                let left = Rect::from_min_max(screen.min, pos2(grid.left(), screen.bottom()));
                let right = Rect::from_min_max(pos2(grid.right(), screen.top()), screen.max);
                let inner = left.shrink(m);
                let clock = Rect::from_min_size(inner.min, vec2(finite(inner.width()), CLOCK_H * k));
                let below = Rect::from_min_max(pos2(inner.left(), inner.top() + info_h), inner.max);
                let size = minimap_in(below, max_h);
                let mini = Rect::from_min_size(pos2(below.center().x - size.x / 2.0, below.top()), size);
                ([left, right], clock, mini, whole_lines(right.shrink(m), line))
            }
        };
        SplitLayout {
            screen,
            frame,
            unit: k,
            text: TEXT * k,
            log_text: TEXT * k,
            line,
            count,
            cols,
            rows,
            seam,
            shared,
            views,
            panels,
            clock,
            minimap,
            log,
        }
    }

    /// The views, in order.
    pub fn views(&self) -> &[Rect] {
        &self.views[..self.count]
    }

    /// The shared space's backgrounds.
    pub fn panels(&self) -> impl Iterator<Item = Rect> + '_ {
        self.panels.iter().copied().filter(|r| r.is_positive())
    }

    /// The minimap's scale: points per OLX minimap pixel.
    pub fn minimap_scale(&self) -> f32 {
        self.minimap.width() / MINIMAP_OLX.x
    }

    /// How many of the log's lines show.
    pub fn log_lines(&self) -> usize {
        if self.line > 0.0 { (self.log.height() / self.line).round() as usize } else { 0 }
    }

    /// View `i`'s compact HUD, at its bottom left; `None` past the views.
    pub fn view_panel(&self, i: usize) -> Option<ViewPanel> {
        let v = *self.views().get(i)?;
        let k = self.unit;
        let (m, pad, gap) = (MARGIN * k, PANEL_PAD * k, PANEL_GAP * k);
        let h = 2.0 * pad + (2.0 * PANEL_ROW + PANEL_BAR + 2.0 * PANEL_GAP) * k;
        let w = (PANEL_W * k).min(finite(v.width()) / 2.0);
        let panel = Rect::from_min_size(pos2(v.left() + m, v.bottom() - m - h), vec2(w, h));
        let inner = panel.shrink(pad);
        let row = |top: f32, h: f32| Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), h));
        let name = row(inner.top(), PANEL_ROW * k);
        let health = row(name.bottom() + gap, PANEL_BAR * k);
        let weapon = row(health.bottom() + gap, PANEL_ROW * k);
        Some(ViewPanel { panel, name, health, weapon, text: PANEL_TEXT * k })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Pos2;

    #[test]
    fn the_maintainers_arrangements_at_16_9() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0));
        let two = SplitLayout::new(screen, 2);
        assert_eq!((two.cols, two.rows, two.shared), (1, 2, Shared::Sides));
        let three = SplitLayout::new(screen, 3);
        assert_eq!((three.cols, three.rows, three.shared), (2, 2, Shared::Cell));
        let four = SplitLayout::new(screen, 4);
        assert_eq!((four.cols, four.rows, four.shared), (2, 2, Shared::Bottom));
        // Two stacked are as tall as the window allows.
        assert!((2.0 * two.views()[0].height() + two.seam - 1080.0).abs() < 1e-2);
        // A window twice as wide: side by side is larger.
        let wide = SplitLayout::new(Rect::from_min_size(Pos2::ZERO, vec2(3840.0, 1080.0)), 2);
        assert_eq!((wide.cols, wide.rows), (2, 1));
        assert!(wide.views()[0].height() > two.views()[0].height());
    }
}
