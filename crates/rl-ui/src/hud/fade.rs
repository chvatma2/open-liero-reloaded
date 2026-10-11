//! The HUD fades where a worm is under it (the final review of plan 7,
//! Important 1). OLX draws its HUD outside the playfield; ours lies over
//! it, so the minimap and the bottom-left panel (health and weapons) turn
//! translucent while a worm (with a margin of a worm's size) is under
//! them, quickly in and out, eased. [`HudLayout::covered`] says which parts
//! a worm is under; [`HudFade`] eases their opacity over time. Both are
//! pure: the painter keeps a `HudFade` in egui's memory and feeds it egui's
//! time.

use egui::{Pos2, Rect, vec2};

use super::layout::HudLayout;

/// A part's opacity while a worm is under it.
pub const FADED: f32 = 0.25;

/// How long a fade takes, in seconds, in or out.
pub const FADE_SECONDS: f64 = 0.15;

/// The parts a worm is under.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Covered {
    pub minimap: bool,
    /// The health and the weapons, bottom left.
    pub panel: bool,
}

/// The parts' opacity, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opacity {
    pub minimap: f32,
    pub panel: f32,
}

impl HudLayout {
    /// The bottom-left panel as painted: the health and the weapons with
    /// their frames.
    pub fn panel(&self) -> Rect {
        self.health.union(self.weapons).expand(2.0 * self.unit)
    }

    /// The parts that worms at `worms` (their centres, in points) are under,
    /// each worm `size` points across with a margin of `size` around it.
    pub fn covered(&self, worms: &[Pos2], size: f32) -> Covered {
        let size = if size.is_finite() { size.max(0.0) } else { 0.0 };
        let near = |r: Rect| {
            r.width() > 0.0
                && worms.iter().any(|&p| {
                    let w = Rect::from_center_size(p, vec2(size, size) * 3.0);
                    let i = w.intersect(r);
                    i.width() > 0.0 && i.height() > 0.0
                })
        };
        Covered { minimap: near(self.minimap), panel: near(self.panel()) }
    }
}

/// One part's opacity, eased from where it was to its target over
/// [`FADE_SECONDS`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fade {
    from: f32,
    to: f32,
    /// When the fade toward `to` began (seconds).
    start: f64,
}

impl Default for Fade {
    fn default() -> Fade {
        Fade { from: 1.0, to: 1.0, start: f64::NEG_INFINITY }
    }
}

/// Smoothstep: slow at both ends.
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

impl Fade {
    fn value(&self, now: f64) -> f32 {
        let t = ((now - self.start) / FADE_SECONDS).clamp(0.0, 1.0) as f32;
        // Done (within rounding: 1.15 - 1.0 is a hair under 0.15).
        if t >= 1.0 - 1e-6 { self.to } else { self.from + (self.to - self.from) * ease(t) }
    }

    /// The opacity at `now` heading for `target`: a new target starts a
    /// fade from the opacity it has now (no jump when a fade turns).
    fn at(&mut self, target: f32, now: f64) -> f32 {
        if target != self.to {
            *self = Fade { from: self.value(now), to: target, start: now };
        }
        self.value(now)
    }

    fn moving(&self, now: f64) -> bool {
        now - self.start < FADE_SECONDS && self.from != self.to
    }
}

/// The minimap's and the panel's fades.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HudFade {
    minimap: Fade,
    panel: Fade,
}

impl HudFade {
    /// The opacities at `now` (seconds, egui's time) with `covered` under
    /// worms.
    pub fn update(&mut self, covered: Covered, now: f64) -> Opacity {
        let target = |c: bool| if c { FADED } else { 1.0 };
        Opacity {
            minimap: self.minimap.at(target(covered.minimap), now),
            panel: self.panel.at(target(covered.panel), now),
        }
    }

    /// A fade is under way at `now` (the painter asks for another frame).
    pub fn moving(&self, now: f64) -> bool {
        self.minimap.moving(now) || self.panel.moving(now)
    }
}
