//! Drawn layers placed in the world (plan 14d; spec "Layers with
//! per-pixel material", "Moving drawn layers"): painting and erasing a
//! layer at its box, with its material per pixel and its back art.
//! Integer only; every coordinate is an `i64` before it is clipped.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;

use crate::compose::Decoded;
use crate::level::{Paint, Rect, Shape, art_block, covers, extent};

/// Material priority (decision 4): Rock 2, Dirt 1, Background 0.
pub fn priority(m: Material) -> u8 {
    match m {
        Material::Rock => 2,
        Material::Dirt => 1,
        Material::Background => 0,
    }
}

/// A drawn layer's data, checked against its own box: its art, cells and
/// back art, and its box in world pixels (`None` when they do not match).
struct Parts<'a> {
    art: &'a mut RgbaImage,
    cells: Option<&'a mut Vec<Material>>,
    back: Option<&'a mut RgbaImage>,
    size: (u32, u32),
}

fn parts(d: &mut Decoded, s: u32) -> Option<Parts<'_>> {
    let Decoded::Drawn { art, cells, back } = d else { return None };
    if s == 0 || !art.width().is_multiple_of(s) || !art.height().is_multiple_of(s) {
        return None;
    }
    let size = (art.width() / s, art.height() / s);
    let cells_fit = cells.as_ref().is_none_or(|c| c.len() as u64 == u64::from(size.0) * u64::from(size.1));
    let back_fit = back.as_ref().is_none_or(|b| (b.width(), b.height()) == (art.width(), art.height()));
    if !cells_fit || !back_fit {
        return None;
    }
    Some(Parts { art, cells: cells.as_mut(), back: back.as_mut(), size })
}

/// The world pixels of `shape` within the box `at`, `size` and a level of
/// `level`, as `x0..x1` × `y0..y1` (`None` when empty).
fn clipped(
    shape: &Shape,
    at: (i32, i32),
    size: (u32, u32),
    level: (u32, u32),
) -> Option<(i64, i64, i64, i64)> {
    let (sx0, sy0, sx1, sy1) = extent(shape);
    let (ax, ay) = (i64::from(at.0), i64::from(at.1));
    let x0 = sx0.max(ax).max(0);
    let y0 = sy0.max(ay).max(0);
    let x1 = sx1.min(ax + i64::from(size.0)).min(i64::from(level.0));
    let y1 = sy1.min(ay + i64::from(size.1)).min(i64::from(level.1));
    (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
}

/// Calls `f(local x, local y, world x, world y)` for each world pixel
/// `shape` covers within the box and the level; returns the level
/// rectangle visited.
fn each(
    shape: &Shape,
    at: (i32, i32),
    size: (u32, u32),
    level: (u32, u32),
    mut f: impl FnMut(u32, u32, i64, i64),
) -> Option<Rect> {
    let (x0, y0, x1, y1) = clipped(shape, at, size, level)?;
    for wy in y0..y1 {
        for wx in x0..x1 {
            if covers(shape, wx, wy) {
                f((wx - i64::from(at.0)) as u32, (wy - i64::from(at.1)) as u32, wx, wy);
            }
        }
    }
    Some(Rect { x: x0 as u32, y: y0 as u32, w: (x1 - x0) as u32, h: (y1 - y0) as u32 })
}

/// Paints `shape` (world pixels) into drawn data `d` whose box starts at
/// `at` (its size its art's ÷ `s`), clipped to the box and to a level of
/// `level`: whole art blocks, opaque, the paint's phase from the level's
/// art origin. Its cells, when it has them, take `material` where a block
/// was painted (plan 14d R6). Returns the level rectangle changed; `None`
/// when nothing was, or `d` is no drawn data of a consistent size.
pub fn draw(
    d: &mut Decoded,
    at: (i32, i32),
    s: u32,
    level: (u32, u32),
    shape: &Shape,
    paint: &Paint,
    material: Material,
) -> Option<Rect> {
    let Parts { art, mut cells, size, .. } = parts(d, s)?;
    let origin = (i64::from(at.0) * i64::from(s), i64::from(at.1) * i64::from(s));
    each(shape, at, size, level, |lx, ly, _, _| {
        let mut painted = false;
        for (ax, ay) in art_block(s, lx, ly) {
            if let Some(c) = paint.at(origin.0 + i64::from(ax), origin.1 + i64::from(ay)).filter(|c| c.a != 0)
            {
                art.set(ax, ay, Rgba { a: 255, ..c });
                painted = true;
            }
        }
        if painted && let Some(cells) = cells.as_deref_mut() {
            cells[ly as usize * size.0 as usize + lx as usize] = material;
        }
    })
}

/// Erases `shape` from drawn data `d` as `draw` paints (plan 14d R5): a
/// block whose cell is dirt or rock and whose back art has an opaque pixel
/// is dug, becoming background with the back art as its art (the back
/// art cleared there); every other block is cleared (alpha 0).
pub fn erase(d: &mut Decoded, at: (i32, i32), s: u32, level: (u32, u32), shape: &Shape) -> Option<Rect> {
    let Parts { art, mut cells, mut back, size } = parts(d, s)?;
    each(shape, at, size, level, |lx, ly, _, _| {
        let i = ly as usize * size.0 as usize + lx as usize;
        let solid = cells.as_deref().is_some_and(|c| c[i] != Material::Background);
        let backed =
            back.as_deref().is_some_and(|b| art_block(s, lx, ly).any(|(ax, ay)| b.get(ax, ay).a != 0));
        if solid
            && backed
            && let (Some(cells), Some(back)) = (cells.as_deref_mut(), back.as_deref_mut())
        {
            cells[i] = Material::Background;
            for (ax, ay) in art_block(s, lx, ly) {
                art.set(ax, ay, back.get(ax, ay));
                back.set(ax, ay, Rgba::TRANSPARENT);
            }
        } else {
            for (ax, ay) in art_block(s, lx, ly) {
                art.set(ax, ay, Rgba::TRANSPARENT);
            }
        }
    })
}
