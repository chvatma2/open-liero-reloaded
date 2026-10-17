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
//! the level's area, and no arithmetic overflows (`i64`).

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_format::image_io::{LEVEL_ART, check_size};
use olr_format::levels::{LevelObject, LevelSource, MAX_OBJECTS, MAX_SIDE, thumbnail};

/// World pixels, origin top-left, y down. May extend past the level.
///
/// A shape covers the world pixels whose position `(px, py)` lies inside
/// it: for a `Rect`, `x <= px < x + w` and `y <= py < y + h` (`w <= 0` or
/// `h <= 0` covers nothing); for a `Circle`,
/// `(px - cx)² + (py - cy)² <= r²` (`r < 0` covers nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect { x: i32, y: i32, w: i32, h: i32 },
    Circle { cx: i32, cy: i32, r: i32 },
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
}

impl Paint {
    /// The colour of art pixel `(ax, ay)`; `None` for an empty tile.
    fn at(&self, ax: u32, ay: u32) -> Option<Rgba> {
        match self {
            Paint::Colour(c) => Some(*c),
            Paint::Tile(t) if t.width() == 0 || t.height() == 0 => None,
            Paint::Tile(t) => Some(t.get(ax % t.width(), ay % t.height())),
        }
    }
}

/// Calls `f(px, py)` for each world pixel of `shape` inside a `w` × `h`
/// level, row by row.
fn covered(shape: &Shape, (w, h): (u32, u32), mut f: impl FnMut(u32, u32)) {
    let (x0, y0, x1, y1) = match *shape {
        Shape::Rect { x, y, w, h } => {
            let (x, y) = (i64::from(x), i64::from(y));
            (x, y, x + i64::from(w), y + i64::from(h))
        }
        // `r < 0` makes the range empty.
        Shape::Circle { cx, cy, r } => {
            let (cx, cy, r) = (i64::from(cx), i64::from(cy), i64::from(r));
            (cx - r, cy - r, cx + r + 1, cy + r + 1)
        }
    };
    let (x0, y0) = (x0.max(0), y0.max(0));
    let (x1, y1) = (x1.min(i64::from(w)), y1.min(i64::from(h)));
    for py in y0..y1 {
        for px in x0..x1 {
            let inside = match *shape {
                Shape::Rect { .. } => true,
                Shape::Circle { cx, cy, r } => {
                    // Inside the clipped box |dx|, |dy| <= r < 2^31.
                    let (dx, dy) = (px - i64::from(cx), py - i64::from(cy));
                    i128::from(dx * dx) + i128::from(dy * dy) <= i128::from(r) * i128::from(r)
                }
            };
            if inside {
                // Within 0..w and 0..h: the casts are lossless.
                f(px as u32, py as u32);
            }
        }
    }
}

/// The art pixels of world pixel `(x, y)`'s block at art scale `s`.
fn art_block(s: u32, x: u32, y: u32) -> impl Iterator<Item = (u32, u32)> {
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
            if let Some(c) = paint.at(ax, ay) {
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
            let Some(c) = paint.at(ax, ay) else { continue };
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
/// move by `offset`; those now outside are removed and returned as they
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
            if let Some(c) = paint.at(ax, ay) {
                new_area.set(ax, ay, c);
            }
        }
    }
    let mut cells = vec![material; w as usize * h as usize];
    let (mut front, mut back) = (new_area.clone(), new_area);
    let (dx, dy) = (i64::from(offset.0), i64::from(offset.1));
    let (old_w, old_h) = level.size;
    // The old pixels that land inside: x in max(0, -dx)..min(old_w, w - dx).
    for oy in (-dy).max(0)..i64::from(old_h).min(i64::from(h) - dy) {
        for ox in (-dx).max(0)..i64::from(old_w).min(i64::from(w) - dx) {
            // Both inside their levels: lossless casts.
            let (ox, oy, nx, ny) = (ox as u32, oy as u32, (ox + dx) as u32, (oy + dy) as u32);
            cells[ny as usize * w as usize + nx as usize] = level.material[cell(level, ox, oy)];
            for (i, (ax, ay)) in art_block(s, ox, oy).enumerate() {
                let i = i as u32;
                let (bx, by) = (nx * s + i % s, ny * s + i / s);
                front.set(bx, by, level.front.get(ax, ay));
                back.set(bx, by, level.back.get(ax, ay));
            }
        }
    }
    let inside = |v: i64, side: u32| (0..i64::from(side)).contains(&v);
    let (kept, dropped) = std::mem::take(&mut level.objects).into_iter().partition(|o| match o {
        LevelObject::Spawn { at, .. } => inside(i64::from(at.0) + dx, w) && inside(i64::from(at.1) + dy, h),
    });
    level.objects = kept;
    for o in &mut level.objects {
        match o {
            // Inside the new level: fits in i32.
            LevelObject::Spawn { at, .. } => {
                *at = ((i64::from(at.0) + dx) as i32, (i64::from(at.1) + dy) as i32)
            }
        }
    }
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
        Some(LevelObject::Spawn { at: old, .. }) => {
            *old = at;
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

/// Regenerates `thumb` from `front` (every CLI operation calls it).
pub fn refresh_thumb(level: &mut LevelSource) {
    level.thumb = Some(olr_format::image_io::encode_rgba(&thumbnail(&level.front)));
}
