//! Colour terrain (spec §19 E7): the level's visible composite image
//! (OLX's `bmpDrawImage`) at art scale, kept in step with the material grid
//! by replaying the simulation's terrain edits in order, plus the stains that
//! blood and gibs leave on it (presentation note §2.1, §2.2, §7.2).
//!
//! The map keeps its own copy of the material grid and applies each edit to
//! it as well, so every colour rule reads the material **before** the edit,
//! exactly as OLX's single loop does. Replaying the same edits in the same
//! order keeps that copy equal to the simulation's grid.

use alloc::vec::Vec;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::Material;

use crate::dirt::{Clip, flags_of};
use crate::{GreenMask, GreenPixel, PX_EMPTY, PX_ROCK, Stamp, StampPixel, Terrain, clip};

/// `PlaceGreenDirt`'s shades (`CMap.cpp:1245-1248`), picked with
/// `GetRandomInt(3)` (0 to 3) per pixel.
pub const GREEN_SHADES: [Rgba; 4] =
    [Rgba::rgb(148, 136, 0), Rgba::rgb(136, 124, 0), Rgba::rgb(124, 112, 0), Rgba::rgb(116, 100, 0)];

/// Dirt's colour when the theme's `FrontTile` is empty: `rl-content`'s
/// placeholder colour.
const NO_TILE: Rgba = Rgba::rgb(255, 0, 220);

/// What a theme gives the colour map: the stamps it edits with and the tile
/// placed dirt is drawn from.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeArt {
    /// `hole1` to `hole5`, as the simulation's.
    pub holes: Vec<Stamp>,
    /// `data/gfx/greenball.png`.
    pub green_mask: Option<GreenMask>,
    /// `FrontTile.png`, tiled from the map origin in world pixels.
    pub front_tile: RgbaImage,
}

/// A terrain edit as the colour map replays it: the simulation's
/// `TerrainEdit` (`rl-content` converts). Positions are the integer
/// position OLX passes to the map function, in world pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorEdit {
    /// `CMap::CarveHole` with hole `hole` (`TerrainOp::Carve` and `Beam`).
    Carve { hole: u8, x: i32, y: i32 },
    /// `CMap::PlaceDirt` with hole `hole`.
    Dirt { hole: u8, x: i32, y: i32 },
    /// `CMap::PlaceGreenDirt`.
    GreenDirt { x: i32, y: i32 },
}

/// A rectangle of the colour image, in art pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        let x1 = (self.x + self.w).max(o.x + o.w);
        let y1 = (self.y + self.h).max(o.y + o.h);
        Rect { x, y, w: x1 - x, h: y1 - y }
    }
}

/// A map area in world pixels, `x0..x1` × `y0..y1` (exclusive ends), not
/// necessarily inside the map (such as `rl-sim`'s `LostArea`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

/// The level's colour image and the material grid it is kept in step with.
#[derive(Clone, Debug)]
pub struct ColorMap {
    scale: u32,
    /// Art pixels.
    width: u32,
    height: u32,
    /// The composite, RGBA8, always opaque.
    pixels: Vec<u8>,
    /// The level's front and back art (opaque), for carving and rebuilding.
    front: Vec<u8>,
    back: Vec<u8>,
    material: Terrain,
    theme: ThemeArt,
    /// The green shades' own generator (OLX draws them from the client's `rand()`).
    rng: u64,
    dirty: Vec<Rect>,
}

/// `img` as opaque RGBA8 of `w` × `h`: the part that fits is copied, the
/// rest is black (a level pack guarantees the size; this never panics).
fn fit(img: &RgbaImage, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    let (iw, ih) = (img.width().min(w) as usize, img.height().min(h));
    for y in 0..h {
        if y < ih {
            let row = y as usize * img.width() as usize * 4;
            out.extend_from_slice(&img.as_raw()[row..row + iw * 4]);
        }
        out.resize((y as usize + 1) * w as usize * 4, 0);
    }
    for p in out.as_chunks_mut::<4>().0 {
        p[3] = 255;
    }
    out
}

impl ColorMap {
    /// The most dirty rectangles kept between [`ColorMap::take_dirty`]
    /// calls; beyond it they are merged into their bounding box.
    pub const MAX_DIRTY: usize = 256;

    /// The composite of a level (presentation note §2.1): `front` is
    /// already the visible image (it equals `back` where the material is
    /// background), `back` is revealed by carving, both `art_scale` art
    /// pixels per world pixel of `material`. The map starts clean: the
    /// caller uploads [`ColorMap::pixels`] once, then the dirty rectangles.
    pub fn new(
        material: &Terrain,
        front: &RgbaImage,
        back: &RgbaImage,
        art_scale: u32,
        theme: ThemeArt,
    ) -> Self {
        let size = |s: u32| {
            let (w, h) = (material.width().checked_mul(s)?, material.height().checked_mul(s)?);
            (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
            Some((w, h))
        };
        let scale = art_scale.max(1);
        let (scale, (width, height)) = match size(scale) {
            Some(wh) => (scale, wh),
            None => (1, (material.width(), material.height())),
        };
        let front = fit(front, width, height);
        Self {
            scale,
            width,
            height,
            pixels: front.clone(),
            back: fit(back, width, height),
            front,
            material: material.clone(),
            theme,
            rng: 0x9e37_79b9_7f4a_7c15,
            dirty: Vec::with_capacity(Self::MAX_DIRTY),
        }
    }

    /// Width of the colour image in art pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height of the colour image in art pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Art pixels per world pixel.
    pub fn art_scale(&self) -> u32 {
        self.scale
    }

    /// The colour image, RGBA8, row-major, opaque.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The material grid as the replayed edits left it.
    pub fn material(&self) -> &Terrain {
        &self.material
    }

    /// OLX `CMap::getColorAt` (`CMap.cpp:2355-2362`): the colour of world
    /// pixel `(x, y)` (its top-left art pixel), `None` outside the map.
    pub fn color_at(&self, x: i64, y: i64) -> Option<Rgba> {
        self.material.material(x, y)?;
        let i =
            ((y as usize * self.scale as usize) * self.width as usize + x as usize * self.scale as usize) * 4;
        let p = &self.pixels[i..i + 4];
        Some(Rgba { r: p[0], g: p[1], b: p[2], a: p[3] })
    }

    /// Replays one terrain edit (after the simulation made it, in the same order).
    pub fn apply(&mut self, edit: ColorEdit) {
        match edit {
            ColorEdit::Carve { hole, x, y } => self.carve(hole as usize, x, y),
            ColorEdit::Dirt { hole, x, y } => self.place_dirt(hole as usize, x, y),
            ColorEdit::GreenDirt { x, y } => self.place_green_dirt(x, y),
        }
    }

    /// The dirty rectangles since the last call, in art pixels; together
    /// they cover every pixel that changed.
    pub fn take_dirty(&mut self) -> impl Iterator<Item = Rect> + '_ {
        self.dirty.drain(..)
    }

    /// Blood that hit rock or dirt: OLX `CMap::putColorTo` (`CMap.cpp:2365-2368`,
    /// `Entity.cpp:363-369`) paints world pixel `(x, y)` (OLX's 2×2 block)
    /// in `color`. Only the colour changes, never the material.
    pub fn stain(&mut self, x: i32, y: i32, color: Rgba) {
        if self.material.material(x as i64, y as i64).is_none() {
            return;
        }
        let (x, y) = (x as u32, y as u32);
        self.put(x, y, color);
        self.mark(x as i64, y as i64, x as i64 + 1, y as i64 + 1);
    }

    /// A gib that came to rest: OLX `CMap::putSurfaceTo` with
    /// `DrawImageStretch2` (`CMap.cpp:2370-2373`, `Entity.cpp:373-389`,
    /// `GfxPrimitives.cpp`): the `src_rect` part of `src` (the worm's
    /// colorized `giblets.png`, a 4 × 4 frame at `(rotation * 4, 8)`), one
    /// world pixel per source pixel, with its top-left at world `(x, y)`.
    /// Magenta (the colour key) and fully transparent pixels are skipped.
    /// Kept quirk: clipped at the map's left or top edge, the source is still
    /// read from its first column and row.
    pub fn stain_image(&mut self, x: i32, y: i32, src: &RgbaImage, src_rect: Rect) {
        // Source clipping first; the destination box uses the requested size.
        let (sx0, sy0) = (src_rect.x.min(src.width()), src_rect.y.min(src.height()));
        let sw = src_rect.x.saturating_add(src_rect.w).min(src.width()) - sx0;
        let sh = src_rect.y.saturating_add(src_rect.h).min(src.height()) - sy0;
        let (x, y) = (x as i64, y as i64);
        let (dx0, dy0) = (x.max(0), y.max(0));
        let dx1 = (x + src_rect.w as i64).min(self.material.width() as i64);
        let dy1 = (y + src_rect.h as i64).min(self.material.height() as i64);
        let (w, h) = ((dx1 - dx0).min(sw as i64), (dy1 - dy0).min(sh as i64));
        if w <= 0 || h <= 0 {
            return;
        }
        for row in 0..h {
            for col in 0..w {
                let c = src.get(sx0 + col as u32, sy0 + row as u32);
                if !c.same_rgb(Rgba::MAGENTA) && c.a != 0 {
                    self.put((dx0 + col) as u32, (dy0 + row) as u32, c);
                }
            }
        }
        self.mark(dx0, dy0, dx0 + w, dy0 + h);
    }

    /// Redraws `area` from `material` and takes its material there: the
    /// back image where it is background, the theme's `FrontTile` where it
    /// is dirt, the level's art where it is rock. This is the fallback for
    /// terrain edits the simulation could not report (`TickEvents::lost_area`):
    /// edits outside the area are exact, inside it only the stains and the
    /// original dirt art are lost.
    pub fn rebuild_area(&mut self, area: Area, material: &Terrain) {
        let mw = self.material.width().min(material.width()) as i64;
        let mh = self.material.height().min(material.height()) as i64;
        let (x0, y0) = ((area.x0 as i64).clamp(0, mw), (area.y0 as i64).clamp(0, mh));
        let (x1, y1) = ((area.x1 as i64).clamp(x0, mw), (area.y1 as i64).clamp(y0, mh));
        for y in y0..y1 {
            for x in x0..x1 {
                let m = material.material(x, y).unwrap_or(Material::Rock);
                let i = y as usize * self.material.width() as usize + x as usize;
                self.material.cells[i] = m as u8;
                let (x, y) = (x as u32, y as u32);
                match m {
                    Material::Background => self.copy_block(x, y, false),
                    Material::Dirt => self.put(x, y, self.tile(x, y)),
                    Material::Rock => self.copy_block(x, y, true),
                }
            }
        }
        self.mark(x0, y0, x1, y1);
    }

    /// OLX `CMap::CarveHole` (`CMap.cpp:1005-1105`), hi-res branch: under a
    /// magenta stamp pixel dirt becomes background and shows the back image
    /// (`CopyPixel2x2`); under any other non-black pixel it stays dirt,
    /// painted in that colour (the brown rim). Rock and background are left
    /// alone. Clipped like the material (the stamp is not offset).
    fn carve(&mut self, hole: usize, pos_x: i32, pos_y: i32) {
        let Some(stamp) = self.theme.holes.get(hole) else { return };
        let (w, h) = (stamp.width() as i32, stamp.height() as i32);
        let (mut x, mut y, mut cw, mut ch) = (pos_x.wrapping_sub(w / 2), pos_y.wrapping_sub(h / 2), w, h);
        let (mw, mh) = (self.material.width() as i32, self.material.height() as i32);
        if !clip(&mut x, &mut cw, mw) || !clip(&mut y, &mut ch, mh) {
            return;
        }
        let mut changed = false;
        for row in 0..ch {
            for col in 0..cw {
                let (mx, my) = ((x + col) as u32, (y + row) as u32);
                if self.material.cells[my as usize * mw as usize + mx as usize] != Material::Dirt as u8 {
                    continue;
                }
                match stamp.get(col as u32, row as u32) {
                    StampPixel::Carve => copy(&mut self.pixels, &self.back, self.width, self.scale, mx, my),
                    StampPixel::Paint(c) => fill(&mut self.pixels, self.width, self.scale, mx, my, c),
                    StampPixel::Skip => continue,
                }
                changed = true;
            }
        }
        self.material.carve_hole(stamp, pos_x, pos_y);
        if changed {
            self.mark(x as i64, y as i64, (x + cw) as i64, (y + ch) as i64);
        }
    }

    /// OLX `CMap::PlaceDirt` (`CMap.cpp:1114-1224`), hi-res branch: under a
    /// non-black stamp pixel, anything but rock becomes dirt coloured from
    /// the theme's `FrontTile` at `(x % tw, y % th)`; then, where it was
    /// background, a non-magenta stamp pixel paints its own colour.
    fn place_dirt(&mut self, hole: usize, pos_x: i32, pos_y: i32) {
        let Some(stamp) = self.theme.holes.get(hole) else { return };
        let (w, h) = (stamp.width(), stamp.height());
        let (sx, sy) = (pos_x.wrapping_sub((w >> 1) as i32), pos_y.wrapping_sub((h >> 1) as i32));
        let c = Clip::new(&self.material, sx, sy, w, h);
        let mw = self.material.width();
        let mut changed = false;
        for (i, x, y) in c.cells(mw) {
            let (mx, my) = ((i % mw as usize) as u32, (i / mw as usize) as u32);
            let (p, flag) = (stamp.get(x, y), flags_of(self.material.cells[i]));
            if p != StampPixel::Skip && flag & PX_ROCK == 0 {
                let t = tile(&self.theme.front_tile, mx, my);
                fill(&mut self.pixels, self.width, self.scale, mx, my, t);
                changed = true;
            }
            if let StampPixel::Paint(rim) = p
                && flag & PX_EMPTY != 0
            {
                fill(&mut self.pixels, self.width, self.scale, mx, my, rim);
            }
        }
        self.material.place_dirt(stamp, pos_x, pos_y);
        if changed {
            self.mark(c.x0, c.y0, c.x1, c.y1);
        }
    }

    /// OLX `CMap::PlaceGreenDirt` (`CMap.cpp:1231-1340`), hi-res branch:
    /// background under a green mask pixel becomes dirt in one of
    /// [`GREEN_SHADES`] (this map's own draw, row-major, one per pixel);
    /// under the rim colour, dirt in that colour.
    fn place_green_dirt(&mut self, pos_x: i32, pos_y: i32) {
        let Some(mask) = &self.theme.green_mask else { return };
        let (w, h) = (mask.width(), mask.height());
        let (sx, sy) = (pos_x.wrapping_sub((w >> 1) as i32), pos_y.wrapping_sub((h >> 1) as i32));
        let c = Clip::new(&self.material, sx, sy, w, h);
        let mw = self.material.width();
        let mut changed = false;
        for (i, x, y) in c.cells(mw) {
            if flags_of(self.material.cells[i]) & PX_EMPTY == 0 {
                continue;
            }
            let colour = match mask.get(x, y) {
                GreenPixel::Green => GREEN_SHADES[next_shade(&mut self.rng)],
                GreenPixel::Rim(rim) => rim,
                GreenPixel::Pink => continue,
            };
            fill(
                &mut self.pixels,
                self.width,
                self.scale,
                (i % mw as usize) as u32,
                (i / mw as usize) as u32,
                colour,
            );
            changed = true;
        }
        self.material.place_green_dirt(mask, pos_x, pos_y, &mut || 0);
        if changed {
            self.mark(c.x0, c.y0, c.x1, c.y1);
        }
    }

    fn tile(&self, x: u32, y: u32) -> Rgba {
        tile(&self.theme.front_tile, x, y)
    }

    fn put(&mut self, x: u32, y: u32, c: Rgba) {
        fill(&mut self.pixels, self.width, self.scale, x, y, c);
    }

    fn copy_block(&mut self, x: u32, y: u32, front: bool) {
        let src = if front { &self.front } else { &self.back };
        copy(&mut self.pixels, src, self.width, self.scale, x, y);
    }

    /// Marks world box `x0..x1` × `y0..y1` (inside the map) dirty.
    fn mark(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let s = self.scale;
        let r = Rect { x: x0 as u32 * s, y: y0 as u32 * s, w: (x1 - x0) as u32 * s, h: (y1 - y0) as u32 * s };
        if self.dirty.len() >= Self::MAX_DIRTY {
            let all = self.dirty.drain(..).fold(r, Rect::union);
            self.dirty.push(all);
        } else {
            self.dirty.push(r);
        }
    }
}

/// `GetRandomInt(3)` with the map's own xorshift64* generator: 0 to 3.
fn next_shade(state: &mut u64) -> usize {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 62) as usize
}

/// The theme's `FrontTile` at world pixel `(x, y)`, tiled from the origin.
fn tile(t: &RgbaImage, x: u32, y: u32) -> Rgba {
    if t.width() == 0 || t.height() == 0 {
        return NO_TILE;
    }
    t.get(x % t.width(), y % t.height())
}

/// Paints world pixel `(x, y)`'s `s` × `s` block of an image `w` art pixels wide, opaque.
fn fill(dst: &mut [u8], w: u32, s: u32, x: u32, y: u32, c: Rgba) {
    let c = [c.r, c.g, c.b, 255];
    let (w, s) = (w as usize, s as usize);
    for row in y as usize * s..(y as usize + 1) * s {
        let start = (row * w + x as usize * s) * 4;
        for p in dst[start..start + s * 4].as_chunks_mut::<4>().0 {
            p.copy_from_slice(&c);
        }
    }
}

/// Copies world pixel `(x, y)`'s block from `src` to `dst` (OLX's `CopyPixel2x2`).
fn copy(dst: &mut [u8], src: &[u8], w: u32, s: u32, x: u32, y: u32) {
    let (w, s) = (w as usize, s as usize);
    for row in y as usize * s..(y as usize + 1) * s {
        let start = (row * w + x as usize * s) * 4;
        dst[start..start + s * 4].copy_from_slice(&src[start..start + s * 4]);
    }
}

#[cfg(test)]
#[path = "color_tests.rs"]
mod tests;
