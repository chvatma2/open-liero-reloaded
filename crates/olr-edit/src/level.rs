//! The level operations (plan 13d ruling R3) on a [`LevelSource`], in
//! world pixels; the art layers follow the art scale (spec §6).
//!
//! - [`carve`]: the material becomes background, and `front := back` on
//!   the covered art blocks.
//! - [`fill`]: the material becomes rock or dirt, and `front` is painted.
//! - [`paint`]: art only. Painting `back` also paints `front` where the
//!   material is background; painting `front` touches only solid pixels.
//! - [`stamp`]: alpha-composites an image onto an art layer, with the
//!   same layer rule as `paint`.
//! - [`resize`], and the object operations, addressed by index.
//!
//! Every world pixel an operation touches changes its whole
//! `art_scale × art_scale` art block, so `front` equals `back` wherever
//! the material is background, before and after every operation. Shapes
//! are clipped to the level before iterating: a huge shape costs at most
//! the level's area, and no arithmetic overflows (`i64`, `i128`, and 256
//! bits for a line's distance).

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_format::image_io::{LEVEL_ART, check_size};
use olr_format::levels::{
    GlassMap, GlowLayers, LevelObject, LevelSource, MAX_OBJECTS, MAX_SIDE, SurfaceMaps, level_thumbnail,
};
use std::sync::Arc;

/// A rectangle of world pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// The smallest rectangle holding both; `None` and `b` give `b`.
    pub fn union(a: Option<Rect>, b: Rect) -> Rect {
        let Some(a) = a else { return b };
        let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
        let x1 = a.x.saturating_add(a.w).max(b.x.saturating_add(b.w));
        let y1 = a.y.saturating_add(a.h).max(b.y.saturating_add(b.h));
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }
}

/// World pixels, origin top-left, y down. May extend past the level.
///
/// A shape covers the world pixels whose position `(px, py)` lies inside
/// it: for a `Rect`, `x <= px < x + w` and `y <= py < y + h` (`w <= 0` or
/// `h <= 0` covers nothing); for a `Circle`,
/// `(px - cx)² + (py - cy)² <= r²` (`r < 0` covers nothing). An `Ellipse`
/// and a `Line` cover a pixel by its centre (`px + ½, py + ½`), measured
/// in doubled coordinates so halves stay integers (plan 14b ruling R10).
/// A `Stamp` covers the world pixels of its mask's covered cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    Circle {
        cx: i32,
        cy: i32,
        r: i32,
    },
    /// The ellipse inscribed in the box; covers a pixel by its centre
    /// (`w <= 0` or `h <= 0` covers nothing).
    Ellipse {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    /// Pixels whose centre lies within `width / 2` of the segment between
    /// the centres of pixels `from` and `to`; `width` 0 covers nothing. Across
    /// a straight line that is `width` pixels for an odd width and
    /// `width + 1` for an even one, as a circle of radius `width / 2`.
    Line {
        from: (i32, i32),
        to: (i32, i32),
        width: u32,
    },
    /// `mask` centred on `(x, y)` (its top-left at `x - w·scale/2`,
    /// `y - h·scale/2`), each mask cell `scale` × `scale` world pixels
    /// (plan 14e R4). A scale above [`MAX_STAMP_SCALE`] is that; 0, or a
    /// mask whose cover does not match its size, covers nothing.
    Stamp {
        x: i32,
        y: i32,
        scale: u32,
        mask: Arc<StampMask>,
    },
}

/// The largest scale of a [`Shape::Stamp`].
pub const MAX_STAMP_SCALE: u32 = 16;

/// A stamp's covered cells, row by row (`cover.len() == w * h`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StampMask {
    pub size: (u32, u32),
    pub cover: Vec<bool>,
}

impl StampMask {
    /// A theme hole (OLX's colours: black leaves the terrain, any other
    /// colour carves or paints): covered where neither black nor
    /// transparent.
    pub fn from_hole(img: &RgbaImage) -> StampMask {
        let mut cover = Vec::with_capacity(img.width() as usize * img.height() as usize);
        for y in 0..img.height() {
            for x in 0..img.width() {
                let c = img.get(x, y);
                cover.push(c.a != 0 && !c.same_rgb(Rgba::BLACK));
            }
        }
        StampMask { size: (img.width(), img.height()), cover }
    }

    /// Whether the cover matches the size.
    fn valid(&self) -> bool {
        (self.size.0 as usize).checked_mul(self.size.1 as usize) == Some(self.cover.len())
    }
}

/// A stamp's top-left world pixel and its cell side.
fn stamp_origin(x: i32, y: i32, scale: u32, mask: &StampMask) -> (i64, i64, i64) {
    let s = i64::from(scale.min(MAX_STAMP_SCALE));
    let (w, h) = (i64::from(mask.size.0), i64::from(mask.size.1));
    (i64::from(x) - w * s / 2, i64::from(y) - h * s / 2, s)
}

/// What an art pixel is painted with, composited over the art (straight
/// alpha): a transparent pixel leaves the art as it was.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Colour(Rgba),
    /// Repeats from the art origin: art pixel `(ax, ay)` takes the tile's
    /// `(ax mod tw, ay mod th)`. An empty tile paints nothing.
    Tile(RgbaImage),
}

/// An art layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Front,
    Back,
}

/// Solid materials `fill` can lay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Solid {
    Rock,
    Dirt,
}

impl From<Solid> for Material {
    fn from(s: Solid) -> Material {
        match s {
            Solid::Rock => Material::Rock,
            Solid::Dirt => Material::Dirt,
        }
    }
}

/// Why an operation was refused; the level is unchanged.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("size {0}x{1} is not within 1x1 to {MAX_SIDE}x{MAX_SIDE}")]
    Size(u32, u32),
    #[error("art of {0}x{1} pixels is too large: {2}")]
    Art(u32, u32, String),
    #[error("a level holds at most {MAX_OBJECTS} objects")]
    TooManyObjects,
    #[error("no object {index} (the level has {len})")]
    NoObject { index: usize, len: usize },
    #[error("element {0:?} moved out of range")]
    Moved(String),
    /// A look edit the level's check refuses (look system).
    #[error("{0}")]
    Look(String),
}

impl Paint {
    /// The colour of the level's art pixel `(ax, ay)` (any position: a
    /// tile repeats from the art origin both ways); `None` for an empty tile.
    pub(crate) fn at(&self, ax: i64, ay: i64) -> Option<Rgba> {
        match self {
            Paint::Colour(c) => Some(*c),
            Paint::Tile(t) if t.width() == 0 || t.height() == 0 => None,
            Paint::Tile(t) => Some(t.get(
                ax.rem_euclid(i64::from(t.width())) as u32,
                ay.rem_euclid(i64::from(t.height())) as u32,
            )),
        }
    }
}

/// The box holding every world pixel `shape` covers, `x0..x1` × `y0..y1`
/// (unclipped, `i64`; empty when it covers nothing).
pub(crate) fn extent(shape: &Shape) -> (i64, i64, i64, i64) {
    match *shape {
        Shape::Rect { x, y, w, h } | Shape::Ellipse { x, y, w, h } => {
            let (x, y) = (i64::from(x), i64::from(y));
            (x, y, x + i64::from(w), y + i64::from(h))
        }
        // `r < 0` makes the range empty.
        Shape::Circle { cx, cy, r } => {
            let (cx, cy, r) = (i64::from(cx), i64::from(cy), i64::from(r));
            (cx - r, cy - r, cx + r + 1, cy + r + 1)
        }
        Shape::Line { width: 0, .. } => (0, 0, 0, 0),
        Shape::Line { from, to, width } => {
            // A centre within width / 2 of the segment is within
            // width / 2 + 1 pixels of an endpoint's column or row.
            let pad = i64::from(width / 2) + 1;
            let (ax, bx) = (i64::from(from.0), i64::from(to.0));
            let (ay, by) = (i64::from(from.1), i64::from(to.1));
            (ax.min(bx) - pad, ay.min(by) - pad, ax.max(bx) + pad + 1, ay.max(by) + pad + 1)
        }
        Shape::Stamp { x, y, scale, ref mask } => {
            if scale == 0 || !mask.valid() {
                return (0, 0, 0, 0);
            }
            let (x0, y0, s) = stamp_origin(x, y, scale, mask);
            (x0, y0, x0 + i64::from(mask.size.0) * s, y0 + i64::from(mask.size.1) * s)
        }
    }
}

/// `shape`'s extent clipped to a `w` × `h` level, as `x0..x1` × `y0..y1`.
fn clipped(shape: &Shape, (w, h): (u32, u32)) -> (u32, u32, u32, u32) {
    let (x0, y0, x1, y1) = extent(shape);
    let clip = |v: i64, side: u32| v.clamp(0, i64::from(side)) as u32;
    (clip(x0, w), clip(y0, h), clip(x1, w), clip(y1, h))
}

/// A rectangle holding the world pixels `shape` covers, clipped to a
/// level of `size`: `None` when that leaves nothing.
pub fn shape_bounds(shape: &Shape, size: (u32, u32)) -> Option<Rect> {
    let (x0, y0, x1, y1) = clipped(shape, size);
    (x0 < x1 && y0 < y1).then(|| Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// `a × b` as 256 bits, (high, low).
fn mul_wide(a: u128, b: u128) -> (u128, u128) {
    const M: u128 = u64::MAX as u128;
    let (a1, a0, b1, b0) = (a >> 64, a & M, b >> 64, b & M);
    let (p00, p01, p10, p11) = (a0 * b0, a0 * b1, a1 * b0, a1 * b1);
    let mid = (p00 >> 64) + (p01 & M) + (p10 & M);
    ((p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64)), (p00 & M) | (mid << 64))
}

/// Whether world pixel `(px, py)` is covered by `shape` (in its extent).
fn inside(shape: &Shape, px: i64, py: i64) -> bool {
    match *shape {
        Shape::Stamp { x, y, scale, ref mask } => {
            // Inside the extent: the cell is within the mask.
            let (x0, y0, s) = stamp_origin(x, y, scale, mask);
            let (cx, cy) = ((px - x0) / s, (py - y0) / s);
            mask.cover.get((cy * i64::from(mask.size.0) + cx) as usize).copied().unwrap_or(false)
        }
        Shape::Rect { .. } => true,
        Shape::Circle { cx, cy, r } => {
            // Inside the clipped box |dx|, |dy| <= r < 2^31.
            let (dx, dy) = (px - i64::from(cx), py - i64::from(cy));
            i128::from(dx * dx) + i128::from(dy * dy) <= i128::from(r) * i128::from(r)
        }
        Shape::Ellipse { x, y, w, h } => {
            // Doubled: the centre (2px + 1) against the box's (2x + w);
            // inside the box |dx| <= w, |dy| <= h, so every term < 2^125.
            let (w, h) = (i128::from(w), i128::from(h));
            let dx = (i128::from(2 * px + 1) - 2 * i128::from(x) - w) * h;
            let dy = (i128::from(2 * py + 1) - 2 * i128::from(y) - h) * w;
            dx * dx + dy * dy <= (w * h) * (w * h)
        }
        Shape::Line { from, to, width } => {
            let doubled = |v: i64| 2 * i128::from(v) + 1;
            let (cx, cy) = (doubled(px), doubled(py));
            let (ax, ay) = (doubled(from.0.into()), doubled(from.1.into()));
            let (bx, by) = (doubled(to.0.into()), doubled(to.1.into()));
            // Every coordinate below 2^34: squares and products below 2^69.
            let (dx, dy, vx, vy) = (bx - ax, by - ay, cx - ax, cy - ay);
            let len2 = dx * dx + dy * dy;
            let t = vx * dx + vy * dy;
            let w2 = i128::from(width) * i128::from(width);
            if t <= 0 {
                vx * vx + vy * vy <= w2
            } else if t >= len2 {
                let (qx, qy) = (cx - bx, cy - by);
                qx * qx + qy * qy <= w2
            } else {
                // The distance² is cross² / len²: compare cross² with
                // width² × len² in 256 bits.
                let cross = (vx * dy - vy * dx).unsigned_abs();
                mul_wide(cross, cross) <= mul_wide(w2.unsigned_abs(), len2.unsigned_abs())
            }
        }
    }
}

/// Whether `shape` covers world pixel `(px, py)`, anywhere (its extent,
/// then the shape's own rule).
pub(crate) fn covers(shape: &Shape, px: i64, py: i64) -> bool {
    let (x0, y0, x1, y1) = extent(shape);
    (x0..x1).contains(&px) && (y0..y1).contains(&py) && inside(shape, px, py)
}

/// Whether colours `a` and `b` are alike within `tolerance` percent (plan
/// 14d): the largest difference of their red, green, blue and alpha, × 100,
/// at most `tolerance` × 255. A tolerance above 100 is 100.
pub fn similar(a: Rgba, b: Rgba, tolerance: u8) -> bool {
    let d = [a.r.abs_diff(b.r), a.g.abs_diff(b.g), a.b.abs_diff(b.b), a.a.abs_diff(b.a)];
    let most = u32::from(d.into_iter().max().unwrap_or(0));
    most * 100 <= u32::from(tolerance.min(100)) * 255
}

/// Calls `f(px, py)` for each world pixel of `shape` inside a `w` × `h`
/// level, row by row. Only the shape's extent clipped to the level is
/// visited: a huge or far-off shape costs at most the level.
pub(crate) fn covered(shape: &Shape, size: (u32, u32), mut f: impl FnMut(u32, u32)) {
    let (x0, y0, x1, y1) = clipped(shape, size);
    for py in y0..y1 {
        for px in x0..x1 {
            if inside(shape, px.into(), py.into()) {
                f(px, py);
            }
        }
    }
}

/// Whether `art` is a level of `size` at art scale `s`.
fn fits(art: &RgbaImage, s: u32, size: (u32, u32)) -> bool {
    s > 0 && size.0.checked_mul(s) == Some(art.width()) && size.1.checked_mul(s) == Some(art.height())
}

/// Paints `shape`'s whole art blocks into a level-sized art layer, opaque:
/// each art pixel takes the paint's RGB (a transparent paint pixel leaves
/// the art). Returns the world rectangle changed (`shape_bounds`); `None`
/// and nothing painted when `art` is not `size` × `art_scale`.
pub fn draw_art(
    art: &mut RgbaImage,
    art_scale: u32,
    size: (u32, u32),
    shape: &Shape,
    paint: &Paint,
) -> Option<Rect> {
    if !fits(art, art_scale, size) {
        return None;
    }
    covered(shape, size, |x, y| {
        for (ax, ay) in art_block(art_scale, x, y) {
            if let Some(c) = paint.at(ax.into(), ay.into()).filter(|c| c.a != 0) {
                art.set(ax, ay, Rgba { a: 255, ..c });
            }
        }
    });
    shape_bounds(shape, size)
}

/// Clears `shape`'s art blocks (alpha 0); the world rectangle changed, as
/// `draw_art`.
pub fn erase_art(art: &mut RgbaImage, art_scale: u32, size: (u32, u32), shape: &Shape) -> Option<Rect> {
    if !fits(art, art_scale, size) {
        return None;
    }
    covered(shape, size, |x, y| {
        for (ax, ay) in art_block(art_scale, x, y) {
            art.set(ax, ay, Rgba::TRANSPARENT);
        }
    });
    shape_bounds(shape, size)
}

/// `img` (a layer at art scale `from`) at art scale `to`, nearest
/// neighbour: new pixel `(ax, ay)` takes old `(ax * from / to, ay * from / to)`.
/// Scales outside `1..=MAX_ART_SCALE` leave it as it is.
pub fn rescale_art(img: &RgbaImage, from: u32, to: u32) -> RgbaImage {
    let scales = 1..=olr_format::levels::MAX_ART_SCALE;
    if !scales.contains(&from) || !scales.contains(&to) || from == to {
        return img.clone();
    }
    let side = |v: u32| (u64::from(v) * u64::from(to) / u64::from(from)) as u32;
    let (w, h) = (side(img.width()), side(img.height()));
    let mut out = RgbaImage::new(w, h);
    for ay in 0..h {
        for ax in 0..w {
            let old = |v: u32| (u64::from(v) * u64::from(from) / u64::from(to)) as u32;
            out.set(ax, ay, img.get(old(ax), old(ay)));
        }
    }
    out
}

/// The art pixels of world pixel `(x, y)`'s block at art scale `s`.
pub(crate) fn art_block(s: u32, x: u32, y: u32) -> impl Iterator<Item = (u32, u32)> {
    (y * s..(y + 1) * s).flat_map(move |ay| (x * s..(x + 1) * s).map(move |ax| (ax, ay)))
}

/// The index of world pixel `(x, y)` in `level.material`.
fn cell(level: &LevelSource, x: u32, y: u32) -> usize {
    y as usize * level.size.0 as usize + x as usize
}

pub fn carve(level: &mut LevelSource, shape: &Shape) {
    let s = level.art_scale;
    covered(shape, level.size, |x, y| {
        let i = cell(level, x, y);
        level.material[i] = Material::Background;
        for (ax, ay) in art_block(s, x, y) {
            let c = level.back.get(ax, ay);
            level.front.set(ax, ay, c);
        }
    });
}

pub fn fill(level: &mut LevelSource, shape: &Shape, solid: Solid, paint: &Paint) {
    let s = level.art_scale;
    covered(shape, level.size, |x, y| {
        let i = cell(level, x, y);
        level.material[i] = solid.into();
        for (ax, ay) in art_block(s, x, y) {
            if let Some(c) = paint.at(ax.into(), ay.into()) {
                let c = over(c, level.front.get(ax, ay));
                level.front.set(ax, ay, c);
            }
        }
    });
}

pub fn paint(level: &mut LevelSource, shape: &Shape, layer: Layer, paint: &Paint) {
    let s = level.art_scale;
    covered(shape, level.size, |x, y| {
        let background = level.material[cell(level, x, y)] == Material::Background;
        for (ax, ay) in art_block(s, x, y) {
            let Some(c) = paint.at(ax.into(), ay.into()) else { continue };
            match layer {
                Layer::Back => {
                    let c = over(c, level.back.get(ax, ay));
                    level.back.set(ax, ay, c);
                    if background {
                        level.front.set(ax, ay, c);
                    }
                }
                Layer::Front if !background => {
                    let c = over(c, level.front.get(ax, ay));
                    level.front.set(ax, ay, c);
                }
                Layer::Front => {}
            }
        }
    });
}

/// `src` over `dst` (straight alpha, rounded).
fn over(src: Rgba, dst: Rgba) -> Rgba {
    match src.a {
        255 => src,
        a => {
            let (a, na) = (u32::from(a), 255 - u32::from(a));
            let mix = |s: u8, d: u8| ((u32::from(s) * a + u32::from(d) * na + 127) / 255) as u8;
            let alpha = a + (u32::from(dst.a) * na + 127) / 255;
            Rgba { r: mix(src.r, dst.r), g: mix(src.g, dst.g), b: mix(src.b, dst.b), a: alpha as u8 }
        }
    }
}

/// `image` in art pixels, its top-left at world (x, y) × art_scale.
pub fn stamp(level: &mut LevelSource, at: (i32, i32), image: &RgbaImage, layer: Layer) {
    let s = level.art_scale;
    let (ox, oy) = (i64::from(at.0) * i64::from(s), i64::from(at.1) * i64::from(s));
    let (aw, ah) = (i64::from(level.front.width()), i64::from(level.front.height()));
    let (iw, ih) = (i64::from(image.width()), i64::from(image.height()));
    for iy in (-oy).max(0)..ih.min(ah - oy) {
        for ix in (-ox).max(0)..iw.min(aw - ox) {
            let src = image.get(ix as u32, iy as u32);
            if src.a == 0 {
                continue;
            }
            // Inside the art: lossless casts.
            let (ax, ay) = ((ox + ix) as u32, (oy + iy) as u32);
            let background = level.material[cell(level, ax / s, ay / s)] == Material::Background;
            match layer {
                Layer::Back => {
                    let c = over(src, level.back.get(ax, ay));
                    level.back.set(ax, ay, c);
                    if background {
                        level.front.set(ax, ay, c);
                    }
                }
                Layer::Front if !background => {
                    let c = over(src, level.front.get(ax, ay));
                    level.front.set(ax, ay, c);
                }
                Layer::Front => {}
            }
        }
    }
}

/// The old content's (0, 0) lands at `offset`; the new area is `material`
/// painted with `paint` (front and back alike when background). Objects
/// move by `offset`; spawns now outside are removed and returned as they
/// were.
pub fn resize(
    level: &mut LevelSource,
    size: (u32, u32),
    offset: (i32, i32),
    material: Material,
    paint: &Paint,
) -> Result<Vec<LevelObject>, EditError> {
    let (w, h) = size;
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return Err(EditError::Size(w, h));
    }
    let s = level.art_scale;
    let (aw, ah) = (w * s, h * s);
    check_size(aw, ah, LEVEL_ART).map_err(|e| EditError::Art(aw, ah, e))?;
    // The new area: front and back alike, whatever the material, so the
    // background invariant holds wherever it is background.
    let mut new_area = RgbaImage::new(aw, ah);
    for ay in 0..ah {
        for ax in 0..aw {
            if let Some(c) = paint.at(ax.into(), ay.into()) {
                new_area.set(ax, ay, c);
            }
        }
    }
    let mut cells = vec![material; w as usize * h as usize];
    let (mut front, mut back) = (new_area.clone(), new_area);
    // The glow re-laid the same way; the new area glows none.
    let old_art = (level.size.0 * s, level.size.1 * s);
    // A glow that does not fit the level (never from a checked pack) is
    // dropped, never indexed.
    level.glow = level.glow.take().filter(|g| g.fits(old_art, level.size));
    let mut glow = level.glow.as_ref().map(|_| GlowLayers::empty((aw, ah), (w, h)));
    // Clear flags too; the new area is opaque.
    level.clear = level.clear.take().filter(|c| c.len() == level.material.len());
    let mut clear = level.clear.as_ref().map(|_| vec![0u8; w as usize * h as usize]);
    // The glass map too (the background glass design); the new area none.
    level.glass = level.glass.take().filter(|g| g.fits(old_art));
    let mut glass = level.glass.as_ref().map(|_| GlassMap::none((aw, ah)));
    // Surface maps too (issue #37); the new area has none.
    level.surface = level.surface.take().filter(|m| m.fits(level.size));
    let mut surface = level.surface.as_ref().map(|_| SurfaceMaps::empty((w, h)));
    let (dx, dy) = (i64::from(offset.0), i64::from(offset.1));
    let (old_w, old_h) = level.size;
    // The old pixels that land inside: x in max(0, -dx)..min(old_w, w - dx).
    for oy in (-dy).max(0)..i64::from(old_h).min(i64::from(h) - dy) {
        for ox in (-dx).max(0)..i64::from(old_w).min(i64::from(w) - dx) {
            // Both inside their levels: lossless casts.
            let (ox, oy, nx, ny) = (ox as u32, oy as u32, (ox + dx) as u32, (oy + dy) as u32);
            cells[ny as usize * w as usize + nx as usize] = level.material[cell(level, ox, oy)];
            if let (Some(c), Some(old)) = (clear.as_mut(), level.clear.as_ref()) {
                c[ny as usize * w as usize + nx as usize] = old[cell(level, ox, oy)];
            }
            if let (Some(m), Some(old)) = (surface.as_mut(), level.surface.as_ref()) {
                let (o, n) = (cell(level, ox, oy), ny as usize * w as usize + nx as usize);
                m.front[n] = old.front[o];
                m.back[n] = old.back[o];
            }
            if let (Some(g), Some(old)) = (glow.as_mut(), level.glow.as_ref()) {
                let (o, n) = (cell(level, ox, oy), ny as usize * w as usize + nx as usize);
                g.groups[n] = old.groups[o];
                g.groups_back[n] = old.groups_back[o];
            }
            for (i, (ax, ay)) in art_block(s, ox, oy).enumerate() {
                let i = i as u32;
                let (bx, by) = (nx * s + i % s, ny * s + i / s);
                front.set(bx, by, level.front.get(ax, ay));
                back.set(bx, by, level.back.get(ax, ay));
                if let (Some(g), Some(old)) = (glow.as_mut(), level.glow.as_ref()) {
                    let (o, n) = ((ay * old.art.0 + ax) as usize, (by * aw + bx) as usize);
                    g.front[n] = old.front[o];
                    g.back[n] = old.back[o];
                }
                if let (Some(g), Some(old)) = (glass.as_mut(), level.glass.as_ref()) {
                    let (o, n) = ((ay * old.art.0 + ax) as usize * 4, (by * aw + bx) as usize * 4);
                    g.texels[n..n + 4].copy_from_slice(&old.texels[o..o + 4]);
                }
            }
        }
    }
    let inside = |v: i64, side: u32| (0..i64::from(side)).contains(&v);
    // Spawns outside the new level go.
    let (kept, dropped) = std::mem::take(&mut level.objects).into_iter().partition(|o| {
        let at = o.at();
        inside(i64::from(at.0) + dx, w) && inside(i64::from(at.1) + dy, h)
    });
    level.objects = kept;
    for o in &mut level.objects {
        let at = o.at();
        let shift = |v: i32, d: i64| (i64::from(v) + d).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
        o.set_at((shift(at.0, dx), shift(at.1, dy)));
    }
    level.glow = glow;
    level.clear = clear;
    level.glass = glass;
    level.surface = surface.filter(|m| !m.is_empty());
    level.size = size;
    level.material = cells;
    level.front = front;
    level.back = back;
    Ok(dropped)
}

/// Adds a spawn point at `at` (world pixels); returns its index.
pub fn add_spawn(level: &mut LevelSource, at: (i32, i32), notes: &str) -> Result<usize, EditError> {
    if level.objects.len() >= MAX_OBJECTS {
        return Err(EditError::TooManyObjects);
    }
    level.objects.push(LevelObject::Spawn { at, notes: notes.into() });
    Ok(level.objects.len() - 1)
}

pub fn move_object(level: &mut LevelSource, index: usize, at: (i32, i32)) -> Result<(), EditError> {
    let len = level.objects.len();
    match level.objects.get_mut(index) {
        Some(o) => {
            o.set_at(at);
            Ok(())
        }
        None => Err(EditError::NoObject { index, len }),
    }
}

pub fn remove_object(level: &mut LevelSource, index: usize) -> Result<LevelObject, EditError> {
    if index >= level.objects.len() {
        return Err(EditError::NoObject { index, len: level.objects.len() });
    }
    Ok(level.objects.remove(index))
}

/// Regenerates `thumb` from `front`, lit when the level has a look
/// (`olr_format::levels::level_thumbnail`; every CLI operation calls it).
pub fn refresh_thumb(level: &mut LevelSource) {
    level.thumb = Some(olr_format::image_io::encode_rgba(&level_thumbnail(level)));
}
