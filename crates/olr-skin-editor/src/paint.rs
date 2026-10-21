//! The paint tools on one frame (issue #14): what a dab, a line, a
//! rectangle or a fill does to the plane being painted.

use olr_data::image::Rgba;

use crate::doc::Frame;

/// What the tools paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Target {
    /// The skin's pixels.
    #[default]
    Colour,
    /// Which pixels take the player's colour (`tint.png`).
    Tint,
    /// Which pixels glow and how much (`glow.png`).
    Glow,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::Colour, Target::Tint, Target::Glow];

    pub fn name(self) -> &'static str {
        match self {
            Target::Colour => crate::ui::labels::TARGET_COLOUR,
            Target::Tint => crate::ui::labels::TARGET_TINT,
            Target::Glow => crate::ui::labels::TARGET_GLOW,
        }
    }
}

/// What one painted pixel becomes, for the target painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// A colour (the eraser's is transparent).
    Colour(Rgba),
    /// The player's colour on or off.
    Tint(bool),
    /// A glow strength, 0 none.
    Glow(u8),
}

/// Paints pixel `(x, y)` with `ink`; outside the frame nothing. Only an
/// opaque (or partly opaque) pixel takes tint or glow; a pixel made
/// transparent loses both.
pub fn put(frame: &mut Frame, x: i32, y: i32, ink: Ink) {
    let Some(i) = frame.at(x, y) else { return };
    match ink {
        Ink::Colour(c) => {
            let c = if c.a == 0 { Rgba::default() } else { c };
            frame.px[i] = c;
            if c.a == 0 {
                frame.tint[i] = 0;
                frame.glow[i] = 0;
            }
        }
        Ink::Tint(on) => frame.tint[i] = if on && frame.px[i].a != 0 { 255 } else { 0 },
        Ink::Glow(g) => frame.glow[i] = if frame.px[i].a != 0 { g } else { 0 },
    }
}

/// A dab of the brush, `size` 1 or 2 (a 2 × 2 block, its top left at
/// `(x, y)`).
pub fn dab(frame: &mut Frame, x: i32, y: i32, size: u32, ink: Ink) {
    let s = size.clamp(1, 2) as i32;
    for dy in 0..s {
        for dx in 0..s {
            put(frame, x + dx, y + dy, ink);
        }
    }
}

/// The art pixels of a line from `a` to `b`, both ends included
/// (Bresenham's).
pub fn line_points(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut x, mut y) = a;
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    let (sx, sy) = (if a.0 < b.0 { 1 } else { -1 }, if a.1 < b.1 { 1 } else { -1 });
    let mut err = dx + dy;
    let mut out = Vec::new();
    // A line across the whole frame is at most a few hundred pixels; a
    // far-off pointer is clamped by the caller.
    loop {
        out.push((x, y));
        if (x, y) == b {
            return out;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// A line of dabs.
pub fn line(frame: &mut Frame, a: (i32, i32), b: (i32, i32), size: u32, ink: Ink) {
    for (x, y) in line_points(a, b) {
        dab(frame, x, y, size, ink);
    }
}

/// A rectangle's outline of dabs between corners `a` and `b`.
pub fn rect(frame: &mut Frame, a: (i32, i32), b: (i32, i32), size: u32, ink: Ink) {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    for (p, q) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))] {
        line(frame, p, q, size, ink);
    }
}

/// Fills the pixels joined to `(x, y)` (sides touching) of the same
/// colour as it with `ink`: a colour fill recolours the area, a tint or
/// glow fill marks it.
pub fn fill(frame: &mut Frame, x: i32, y: i32, ink: Ink) {
    let Some(start) = frame.at(x, y) else { return };
    let key = frame.px[start];
    let mut seen = vec![false; frame.px.len()];
    let mut todo = vec![(x, y)];
    seen[start] = true;
    while let Some((cx, cy)) = todo.pop() {
        put(frame, cx, cy, ink);
        for (nx, ny) in [(cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)] {
            if let Some(n) = frame.at(nx, ny)
                && !seen[n]
                && frame.px[n] == key
            {
                seen[n] = true;
                todo.push((nx, ny));
            }
        }
    }
}
