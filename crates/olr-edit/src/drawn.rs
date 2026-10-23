//! Drawn layers placed in the world (plan 14d; spec "Layers with
//! per-pixel material", "Moving drawn layers"): painting and erasing a
//! layer at its box, with its material per pixel and its back art.
//! Integer only; every coordinate is an `i64` before it is clipped.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_format::image_io::{LEVEL_ART, check_size};
use olr_format::levels::{Element, ElementKind, ElementMaterial, MAX_SIDE, drawn_box};

use crate::compose::Decoded;
use crate::level::{Paint, Rect, Shape, art_block, covers, extent, similar};

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

/// Fills the region of world pixel `seed` in drawn data `d` (box at `at`,
/// art scale `s`) within the box and a level of `level` (plan 14e R5): the
/// 4-connected world pixels sharing the seed's covered state (a block's
/// centre art pixel opaque) and, when covered, whose centre colour is
/// `similar` to the seed's within `tolerance` percent. Its blocks are
/// painted as `draw` paints them, with `material` in the cells. Returns the
/// level rectangle holding the region; `None` when the seed is outside,
/// nothing changed, or `d` is no drawn data of a consistent size.
#[allow(clippy::too_many_arguments)]
pub fn fill(
    d: &mut Decoded,
    at: (i32, i32),
    s: u32,
    level: (u32, u32),
    seed: (i32, i32),
    paint: &Paint,
    material: Material,
    tolerance: u8,
) -> Option<Rect> {
    let Parts { art, mut cells, size, .. } = parts(d, s)?;
    let (ax, ay) = (i64::from(at.0), i64::from(at.1));
    let (x0, y0) = (ax.max(0), ay.max(0));
    let x1 = (ax + i64::from(size.0)).min(i64::from(level.0));
    let y1 = (ay + i64::from(size.1)).min(i64::from(level.1));
    let (sx, sy) = (i64::from(seed.0), i64::from(seed.1));
    if !(x0..x1).contains(&sx) || !(y0..y1).contains(&sy) {
        return None;
    }
    // The region, in box ∩ level coordinates.
    let (rw, rh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let local = |i: usize| ((x0 - ax) as u32 + (i % rw) as u32, (y0 - ay) as u32 + (i / rw) as u32);
    let centre = |art: &RgbaImage, (lx, ly): (u32, u32)| art.get(lx * s + s / 2, ly * s + s / 2);
    let first = (sy - y0) as usize * rw + (sx - x0) as usize;
    let want = centre(art, local(first));
    let alike = |c: Rgba| (c.a != 0) == (want.a != 0) && (want.a == 0 || similar(c, want, tolerance));
    let mut inside = vec![false; rw * rh];
    // `u32` indices: box ∩ level holds at most the art's pixels, far below 2³².
    let mut stack = vec![first as u32];
    inside[first] = true;
    let (mut bx0, mut by0, mut bx1, mut by1) = (rw, rh, 0, 0);
    while let Some(i) = stack.pop() {
        let i = i as usize;
        let (x, y) = (i % rw, i / rw);
        (bx0, by0, bx1, by1) = (bx0.min(x), by0.min(y), bx1.max(x + 1), by1.max(y + 1));
        let mut visit = |j: usize| {
            if !inside[j] && alike(centre(art, local(j))) {
                inside[j] = true;
                stack.push(j as u32);
            }
        };
        if x > 0 {
            visit(i - 1);
        }
        if x + 1 < rw {
            visit(i + 1);
        }
        if y > 0 {
            visit(i - rw);
        }
        if y + 1 < rh {
            visit(i + rw);
        }
    }
    let origin = (ax * i64::from(s), ay * i64::from(s));
    let mut changed = false;
    for y in by0..by1 {
        for x in bx0..bx1 {
            if !inside[y * rw + x] {
                continue;
            }
            let (lx, ly) = local(y * rw + x);
            let mut painted = false;
            for (px, py) in art_block(s, lx, ly) {
                if let Some(c) =
                    paint.at(origin.0 + i64::from(px), origin.1 + i64::from(py)).filter(|c| c.a != 0)
                {
                    let c = Rgba { a: 255, ..c };
                    changed |= art.get(px, py) != c;
                    art.set(px, py, c);
                    painted = true;
                }
            }
            if painted && let Some(cells) = cells.as_deref_mut() {
                let cell = &mut cells[ly as usize * size.0 as usize + lx as usize];
                changed |= *cell != material;
                *cell = material;
            }
        }
    }
    let rect = Rect {
        x: (x0 as usize + bx0) as u32,
        y: (y0 as usize + by0) as u32,
        w: (bx1 - bx0) as u32,
        h: (by1 - by0) as u32,
    };
    changed.then_some(rect)
}

/// Erases `shape` from drawn data `d` as `draw` paints (plan 14d R5): a
/// block whose cell is dirt or rock and whose back art has an opaque pixel
/// is dug, becoming background with the back art as its art (the back
/// art cleared there); every other block is cleared (alpha 0), its back
/// art too.
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
            // Cleared, back art included: an erased background is gone
            // (batch B review 1).
            for (ax, ay) in art_block(s, lx, ly) {
                art.set(ax, ay, Rgba::TRANSPARENT);
                if let Some(back) = back.as_deref_mut() {
                    back.set(ax, ay, Rgba::TRANSPARENT);
                }
            }
        }
    })
}

/// Gives a plain layer's data cells, all of `material` (plan 14d R3), so
/// it can hold other materials; mixed or other data is left as it is.
pub fn mix(d: &mut Decoded, s: u32, material: Material) {
    if let Decoded::Drawn { art, cells: cells @ None, .. } = d {
        let n = (art.width() / s.max(1)) as usize * (art.height() / s.max(1)) as usize;
        *cells = Some(vec![material; n]);
    }
}

/// Drops the cells of drawn data `d` whose every cell is `material`: the
/// layer is plain again (a stroke of another material that painted
/// nothing; plan 14e minors).
pub fn unmix_plain(d: &mut Decoded, material: Material) {
    if let Decoded::Drawn { cells: cells @ Some(_), .. } = d
        && cells.as_ref().is_some_and(|c| c.iter().all(|m| *m == material))
    {
        *cells = None;
    }
}

/// A layer's pixels placed in the world: what merge and split work on.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The box's top-left, world pixels.
    pub at: (i32, i32),
    /// The box's size, world pixels.
    pub size: (u32, u32),
    /// `size` × art scale; alpha 0 is not covered.
    pub art: RgbaImage,
    /// One per world pixel of the box, row-major.
    pub cells: Vec<Material>,
    /// `size` × art scale, when it has back art.
    pub back: Option<RgbaImage>,
}

/// Why a merge or a split is refused.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LayerError {
    #[error("element {0:?} is not a drawn layer: merge into a drawn layer")]
    NotDrawn(String),
    #[error("element {0:?} is hidden: show it first")]
    Hidden(String),
    #[error("element {0:?} has no pixels to work on")]
    NoPixels(String),
    #[error("element {0:?} is locked")]
    Locked(String),
    #[error(
        "merging into {0:?} would make a layer larger than {MAX_SIDE} × {MAX_SIDE} pixels or its art too large"
    )]
    TooLarge(String),
}

/// Opaque: the colour's RGB.
fn opaque(c: Rgba) -> Rgba {
    Rgba { a: 255, ..c }
}

/// A visible element as placed pixels: a drawn layer (its cells from its
/// material when plain), an image's processed grid (covered where its alpha
/// is 255, made opaque; its cells its material), a raster (the level: its
/// front as art, its back as back art). `None` for a hidden element or
/// data that does not match the element.
pub fn placed(e: &Element, d: &Decoded, level: (u32, u32), s: u32) -> Option<Placed> {
    if !e.visible || s == 0 {
        return None;
    }
    let dims = |i: &RgbaImage| (i.width(), i.height());
    let cells_of = |size: (u32, u32)| size.0 as usize * size.1 as usize;
    match (&e.kind, d) {
        (ElementKind::Drawn { material, .. }, Decoded::Drawn { art, cells, back }) => {
            let (at, size) = drawn_box(&e.kind, level)?;
            let box_art = (size.0.checked_mul(s)?, size.1.checked_mul(s)?);
            let fits = dims(art) == box_art
                && cells.as_ref().is_none_or(|c| c.len() == cells_of(size))
                && back.as_ref().is_none_or(|b| dims(b) == box_art);
            fits.then(|| Placed {
                at,
                size,
                art: art.clone(),
                cells: cells.clone().unwrap_or_else(|| vec![(*material).into(); cells_of(size)]),
                back: back.clone(),
            })
        }
        (
            ElementKind::Image { material, at, size, pixel_size, .. },
            Decoded::Image { grid: Some(grid), .. },
        ) => {
            let ps = (*pixel_size).max(1);
            let box_art = (size.0.checked_mul(s)?, size.1.checked_mul(s)?);
            if dims(grid) != (box_art.0.div_ceil(ps), box_art.1.div_ceil(ps)) {
                return None;
            }
            let mut art = RgbaImage::new(box_art.0, box_art.1);
            for y in 0..box_art.1 {
                for x in 0..box_art.0 {
                    let p = grid.get(x / ps, y / ps);
                    if p.a == 255 {
                        art.set(x, y, p);
                    }
                }
            }
            Some(Placed {
                at: *at,
                size: *size,
                art,
                cells: vec![(*material).into(); cells_of(*size)],
                back: None,
            })
        }
        (ElementKind::Raster, Decoded::Raster { material, front, back }) => {
            let box_art = (level.0.checked_mul(s)?, level.1.checked_mul(s)?);
            if dims(front) != box_art || dims(back) != box_art || material.len() != cells_of(level) {
                return None;
            }
            let mut art = front.clone();
            for y in 0..art.height() {
                for x in 0..art.width() {
                    art.set(x, y, opaque(front.get(x, y)));
                }
            }
            Some(Placed { at: (0, 0), size: level, art, cells: material.clone(), back: Some(back.clone()) })
        }
        _ => None,
    }
}

impl Placed {
    /// Whether its art, cells and back art are its box's at art scale `s`
    /// (a hand-made one may not be).
    pub fn fits(&self, s: u32) -> bool {
        let art = (self.size.0.checked_mul(s), self.size.1.checked_mul(s));
        let dims = |i: &RgbaImage| (Some(i.width()), Some(i.height()));
        s > 0
            && dims(&self.art) == art
            && self.cells.len() as u64 == u64::from(self.size.0) * u64::from(self.size.1)
            && self.back.as_ref().is_none_or(|b| dims(b) == art)
    }

    /// The box offset of world pixel `(wx, wy)`, when inside the box.
    fn local(&self, wx: i64, wy: i64) -> Option<(u32, u32)> {
        let (lx, ly) = (wx - i64::from(self.at.0), wy - i64::from(self.at.1));
        let inside = (0..i64::from(self.size.0)).contains(&lx) && (0..i64::from(self.size.1)).contains(&ly);
        inside.then_some((lx as u32, ly as u32))
    }

    fn cell(&self, l: (u32, u32)) -> Material {
        self.cells[l.1 as usize * self.size.0 as usize + l.0 as usize]
    }

    /// Whether it covers the block of world pixel `(wx, wy)`: its art at
    /// the block's centre is opaque.
    fn covers(&self, wx: i64, wy: i64, s: u32) -> Option<(u32, u32)> {
        let l = self.local(wx, wy)?;
        (self.art.get(l.0 * s + s / 2, l.1 * s + s / 2).a != 0).then_some(l)
    }

    /// Its art pixel `(i, j)` of world pixel `(wx, wy)`'s block, opaque.
    fn art_at(&self, wx: i64, wy: i64, s: u32, (i, j): (u32, u32)) -> Option<Rgba> {
        let l = self.local(wx, wy)?;
        Some(self.art.get(l.0 * s + i, l.1 * s + j)).filter(|p| p.a != 0)
    }

    fn back_at(&self, wx: i64, wy: i64, s: u32, (i, j): (u32, u32)) -> Option<Rgba> {
        let l = self.local(wx, wy)?;
        Some(self.back.as_ref()?.get(l.0 * s + i, l.1 * s + j)).filter(|p| p.a != 0)
    }
}

/// `p` cropped to the world pixels it covers (plan 14e R6): those whose
/// block holds an opaque art or back-art pixel. `None` when it covers
/// none, or does not fit itself at art scale `s`.
pub fn crop(p: &Placed, s: u32) -> Option<Placed> {
    if !p.fits(s) {
        return None;
    }
    let opaque = |i: &RgbaImage, x: u32, y: u32| art_block(s, x, y).any(|(ax, ay)| i.get(ax, ay).a != 0);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..p.size.1 {
        for x in 0..p.size.0 {
            if opaque(&p.art, x, y) || p.back.as_ref().is_some_and(|b| opaque(b, x, y)) {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let cut = |i: &RgbaImage| i.crop(x0 * s, y0 * s, w * s, h * s);
    let cells = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .map(|(x, y)| p.cells[y as usize * p.size.0 as usize + x as usize])
        .collect();
    let shift = |a: i32, d: u32| (i64::from(a) + i64::from(d)).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
    Some(Placed {
        at: (shift(p.at.0, x0), shift(p.at.1, y0)),
        size: (w, h),
        art: cut(&p.art),
        cells,
        back: p.back.as_ref().map(cut),
    })
}

/// The decision-4 merge of `upper` into `lower` over the union of their
/// boxes (plan 14d Task 5, C1, C2, decision 7). Per world pixel: each layer
/// covers its block where its art at the block's centre is opaque; the
/// winner is the covering layer of higher `priority`, the upper on a tie.
/// The winner's art fills the block where opaque, the other's elsewhere;
/// the cell is the winner's (with no winner, the lower layer's, else
/// background). The back art is the winner's own, else the loser's art
/// when the loser covers there with background, else the loser's back.
/// `None` when the union is larger than `MAX_SIDE` per side or its art
/// beyond `LEVEL_ART`.
pub fn merge(lower: &Placed, upper: &Placed, s: u32) -> Option<Placed> {
    let lo = |p: &Placed| (i64::from(p.at.0), i64::from(p.at.1));
    let hi = |p: &Placed| (i64::from(p.at.0) + i64::from(p.size.0), i64::from(p.at.1) + i64::from(p.size.1));
    let (x0, y0) = (lo(lower).0.min(lo(upper).0), lo(lower).1.min(lo(upper).1));
    let (x1, y1) = (hi(lower).0.max(hi(upper).0), hi(lower).1.max(hi(upper).1));
    let (w, h) = (x1 - x0, y1 - y0);
    let max = i64::from(MAX_SIDE);
    if !lower.fits(s) || !upper.fits(s) || !(1..=max).contains(&w) || !(1..=max).contains(&h) {
        return None;
    }
    let (w, h) = (w as u32, h as u32);
    check_size(w * s, h * s, LEVEL_ART).ok()?;
    let mut art = RgbaImage::new(w * s, h * s);
    // Back art only when some pixel needs it (plan 14d minors).
    let mut back: Option<RgbaImage> = None;
    let mut cells = vec![Material::Background; w as usize * h as usize];
    for y in 0..h {
        for x in 0..w {
            let (wx, wy) = (x0 + i64::from(x), y0 + i64::from(y));
            let (lc, uc) = (lower.covers(wx, wy, s), upper.covers(wx, wy, s));
            // The winner and the other layer, when anything covers.
            let won = match (lc, uc) {
                (Some(l), Some(u)) if priority(lower.cell(l)) > priority(upper.cell(u)) => {
                    Some((lower, upper))
                }
                (_, Some(_)) => Some((upper, lower)),
                (Some(_), None) => Some((lower, upper)),
                (None, None) => None,
            };
            cells[y as usize * w as usize + x as usize] = match won {
                Some((winner, _)) => winner.local(wx, wy).map_or(Material::Background, |l| winner.cell(l)),
                None => lower.local(wx, wy).map_or(Material::Background, |l| lower.cell(l)),
            };
            let (first, second) = won.unwrap_or((upper, lower));
            // The loser shows its art as backdrop where it covers with background.
            let loser_ground = won.is_some()
                && second.covers(wx, wy, s).is_some_and(|l| second.cell(l) == Material::Background);
            for j in 0..s {
                for i in 0..s {
                    let p = first.art_at(wx, wy, s, (i, j)).or_else(|| second.art_at(wx, wy, s, (i, j)));
                    let b = match won {
                        Some(_) => first.back_at(wx, wy, s, (i, j)).or_else(|| {
                            if loser_ground {
                                second.art_at(wx, wy, s, (i, j))
                            } else {
                                second.back_at(wx, wy, s, (i, j))
                            }
                        }),
                        None => upper.back_at(wx, wy, s, (i, j)).or_else(|| lower.back_at(wx, wy, s, (i, j))),
                    };
                    let (ax, ay) = (x * s + i, y * s + j);
                    if let Some(p) = p {
                        art.set(ax, ay, opaque(p));
                    }
                    if let Some(b) = b {
                        back.get_or_insert_with(|| RgbaImage::new(w * s, h * s)).set(ax, ay, opaque(b));
                    }
                }
            }
        }
    }
    let at = (x0 as i32, y0 as i32);
    Some(Placed { at, size: (w, h), art, cells, back })
}

/// The Background, Dirt and Rock layers of `p` (plan 14d R11), each with
/// `p`'s box, holding only its material's pixels; the Background layer
/// also takes the back art under dirt and rock. `None` for one with no
/// opaque pixel.
pub fn split(p: &Placed, s: u32) -> [Option<Placed>; 3] {
    if !p.fits(s) {
        return [None, None, None];
    }
    [Material::Background, Material::Dirt, Material::Rock].map(|m| {
        let mut art = RgbaImage::new(p.art.width(), p.art.height());
        let mut any = false;
        for y in 0..p.size.1 {
            for x in 0..p.size.0 {
                let c = p.cell((x, y));
                for (ax, ay) in art_block(s, x, y) {
                    let own = (c == m).then(|| p.art.get(ax, ay));
                    let back = || p.back.as_ref().map(|b| b.get(ax, ay)).filter(|b| b.a != 0);
                    // The Background layer takes the back art wherever the art
                    // does not show (plan 14d minors).
                    let px = match m {
                        Material::Background => own.filter(|a| a.a != 0).or_else(back),
                        _ => own,
                    };
                    if let Some(px) = px {
                        any |= px.a != 0;
                        art.set(ax, ay, px);
                    }
                }
            }
        }
        any.then(|| Placed { at: p.at, size: p.size, art, cells: vec![m; p.cells.len()], back: None })
    })
}

/// `p` as a drawn element's kind and data: `material` its material, mixed
/// when a covered cell differs from it (uncovered cells set to it), backed
/// when a back pixel is opaque, `size: None` when its box is the level
/// (`level`) at the origin.
pub fn into_layer(p: Placed, material: ElementMaterial, level: (u32, u32), s: u32) -> (ElementKind, Decoded) {
    let m: Material = material.into();
    // A hand-made `Placed` that does not fit itself makes an empty layer.
    let p = match p.fits(s) {
        true => p,
        false => Placed {
            art: RgbaImage::new(p.size.0.saturating_mul(s), p.size.1.saturating_mul(s)),
            cells: vec![m; p.size.0 as usize * p.size.1 as usize],
            back: None,
            ..p
        },
    };
    let s = s.max(1);
    let Placed { at, size, art, mut cells, back } = p;
    let mut mixed = false;
    for y in 0..size.1 {
        for x in 0..size.0 {
            let i = y as usize * size.0 as usize + x as usize;
            let covered = art.get(x * s + s / 2, y * s + s / 2).a != 0;
            if !covered {
                cells[i] = m;
            }
            mixed |= cells[i] != m;
        }
    }
    let back = back.filter(|b| b.as_raw().chunks(4).any(|px| px[3] != 0));
    let boxed = (at != (0, 0) || size != level).then_some(size);
    let kind = ElementKind::Drawn { material, at, size: boxed, mixed, back: back.is_some() };
    (kind, Decoded::Drawn { art, cells: mixed.then_some(cells), back })
}

/// `upper` merged into `lower` (plan 14d R10): `lower`'s id, name and
/// material, visible and unlocked. Refused unless `lower` is a drawn layer
/// and both are visible.
pub fn merge_down(
    lower: (&Element, &Decoded),
    upper: (&Element, &Decoded),
    level: (u32, u32),
    s: u32,
) -> Result<(Element, Decoded), LayerError> {
    let (le, ld) = lower;
    let (ue, ud) = upper;
    let ElementKind::Drawn { material, .. } = le.kind else {
        return Err(LayerError::NotDrawn(le.id.clone()));
    };
    for e in [le, ue] {
        if e.locked {
            return Err(LayerError::Locked(e.id.clone()));
        }
        if !e.visible {
            return Err(LayerError::Hidden(e.id.clone()));
        }
    }
    let lp = placed(le, ld, level, s).ok_or_else(|| LayerError::NoPixels(le.id.clone()))?;
    let up = placed(ue, ud, level, s).ok_or_else(|| LayerError::NoPixels(ue.id.clone()))?;
    let merged = merge(&lp, &up, s).ok_or_else(|| LayerError::TooLarge(le.id.clone()))?;
    let (kind, decoded) = into_layer(merged, material, level, s);
    let element = Element {
        id: le.id.clone(),
        name: le.name.clone(),
        visible: true,
        locked: false,
        folder: None,
        kind,
        glow: None,
        clear: false,
        distant: None,
        surface: None,
        back_surface: None,
        surface_map: false,
        back_surface_map: false,
    };
    Ok((element, decoded))
}

/// The layers `split` makes of element `e` (plan 14d R11), bottom to top,
/// named "Background", "Dirt" and "Rock", each one's id from `id(name,
/// fallback)`. Refused for a hidden element or data that does not match.
pub fn split_element(
    e: &Element,
    d: &Decoded,
    level: (u32, u32),
    s: u32,
    mut id: impl FnMut(&str, &str) -> String,
) -> Result<Vec<(Element, Decoded)>, LayerError> {
    if e.locked {
        return Err(LayerError::Locked(e.id.clone()));
    }
    if !e.visible {
        return Err(LayerError::Hidden(e.id.clone()));
    }
    let p = placed(e, d, level, s).ok_or_else(|| LayerError::NoPixels(e.id.clone()))?;
    let names = [
        ("Background", "background", ElementMaterial::Background),
        ("Dirt", "dirt", ElementMaterial::Dirt),
        ("Rock", "rock", ElementMaterial::Rock),
    ];
    let mut out = Vec::new();
    for (part, (name, fallback, material)) in split(&p, s).into_iter().zip(names) {
        let Some(part) = part else { continue };
        let (kind, decoded) = into_layer(part, material, level, s);
        let element = Element {
            id: id(name, fallback),
            name: name.into(),
            visible: true,
            locked: false,
            folder: None,
            kind,
            glow: None,
            clear: false,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        out.push((element, decoded));
    }
    Ok(out)
}
