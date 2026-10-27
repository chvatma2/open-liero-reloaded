//! Glow beams (the directional-glow design): a level's directional glow
//! pixels, grouped into patches, each patch one beam source. No OLX
//! counterpart. The maps come as slices, because this crate does not depend
//! on `olr-terrain`.

use alloc::vec;
use alloc::vec::Vec;

use crate::srgb_to_linear;

/// The most beams a level may have (`olr-format` refuses more).
pub const MAX_GLOW_BEAMS: usize = 64;

/// One layer's maps, front (the composite) or back (the backdrop).
#[derive(Clone, Copy, Debug)]
pub struct BeamLayer<'a> {
    /// World pixels.
    pub world: (u32, u32),
    /// Art pixels per world pixel, per side.
    pub art_scale: u32,
    /// Glow group per world pixel (`GlowLayers::groups`), row-major.
    pub groups: &'a [u8],
    /// Glow strength 0-255 per art pixel, row-major.
    pub glow: &'a [u8],
    /// The layer's art, RGBA per art pixel.
    pub art: &'a [u8],
    /// Two bytes per world pixel (`GlowBeamMap::front`/`back`).
    pub beams: &'a [u8],
}

/// One beam: a patch of directional glow pixels of one group and one
/// direction and spread, 8-connected.
#[derive(Clone, Debug, PartialEq)]
pub struct GlowBeamSource {
    /// The map value of the patch's group (1-based).
    pub group: u8,
    /// The middle of the patch's face, world pixels.
    pub at: [f32; 2],
    /// The unit direction (y down).
    pub dir: [f32; 2],
    /// Half the face's width, world pixels.
    pub half: f32,
    /// The half-angle, radians.
    pub spread: f32,
    /// The patch's mean glow strength, 0 to 1.
    pub glow: f32,
    /// The patch's mean linear art colour, brightest channel 1 (zeros for
    /// black).
    pub colour: [f32; 3],
    /// Row-major world index of the patch's first pixel (its flicker
    /// region).
    pub first: u32,
    /// Whether it is the backdrop's.
    pub back: bool,
}

/// Every patch of both layers: the front layer first, each in row-major
/// order of its first pixel. A layer whose slices do not fit its sizes makes
/// none. The caller applies [`MAX_GLOW_BEAMS`].
pub fn derive(front: &BeamLayer, back: &BeamLayer) -> Vec<GlowBeamSource> {
    let mut out = Vec::new();
    layer(front, false, &mut |s, _| out.push(s));
    layer(back, true, &mut |s, _| out.push(s));
    out
}

/// One patch of a live part's picture (issue #93): derived as the level's
/// are ([`derive`] on the picture alone, relative to its top-left) and
/// placed, turned and mirrored each frame ([`LivePatch::place`]).
#[derive(Clone, Debug, PartialEq)]
pub struct LivePatch {
    /// The patch as derived: its group is the picture's map value, its
    /// glow the mask's mean (at the part's strength 100).
    pub source: GlowBeamSource,
    /// Its own direction, whole degrees (the map's 2° steps), y down.
    pub direction: u16,
    /// Its own spread, degrees.
    pub spread: u8,
    /// Its first pixel in the picture.
    pub first: (u32, u32),
    /// Each of its rows: `(y, x0, x1)`, its pixels from `x0` to before
    /// `x1` at the row's ends (the corners every face and width is made
    /// from).
    pub rows: Vec<(u32, u32, u32)>,
}

/// How a live patch is drawn this frame: its picture's top-left in the
/// world, whether it is mirrored within the picture (the copy's `Facing`;
/// the caller mirrors the picture's place), the copy's direction and
/// spread values (`None` its own), its part's glow strength (percent) and
/// its group's map value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatchPlace {
    pub at: (i32, i32),
    pub flip: bool,
    pub direction: Option<i64>,
    pub spread: Option<i64>,
    pub strength: u8,
    pub group: u8,
    /// The level's world size (the patch's flicker region index).
    pub world: (u32, u32),
}

/// Every patch of a live part's picture (its maps in `l`, `back` for a
/// background part's): [`derive`]'s, in its order, with the rows each is
/// made of.
pub fn live_patches(l: &BeamLayer, back: bool) -> Vec<LivePatch> {
    let mut out = Vec::new();
    let w = l.world.0 as usize;
    layer(l, back, &mut |source, pixels| {
        let mut rows: Vec<(u32, u32, u32)> = Vec::new();
        for &i in pixels {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            match rows.iter_mut().find(|r| r.0 == y) {
                Some(r) => (r.1, r.2) = (r.1.min(x), r.2.max(x + 1)),
                None => rows.push((y, x, x + 1)),
            }
        }
        rows.sort_unstable();
        let first = source.first as usize;
        let (d, spread) = (l.beams[first * 2], l.beams[first * 2 + 1]);
        out.push(LivePatch {
            direction: u16::from(d.saturating_sub(1)) * 2,
            spread,
            first: ((first % w) as u32, (first / w) as u32),
            source,
            rows,
        });
    });
    out
}

/// A direction in whole degrees mirrored left to right (`(180 − d) mod
/// 360`, as `posed` mirrors a beam).
pub fn mirrored_direction(d: i64) -> u16 {
    (180 - d.rem_euclid(360)).rem_euclid(360) as u16
}

impl LivePatch {
    /// The patch as `p` draws it: its corners mirrored within the picture
    /// (width `picture`) when `p.flip`, moved to `p.at`, its direction the
    /// value (whole degrees, 0-359) or its own, mirrored when flipped, its
    /// spread the value (1-90) or its own; its face and width worked out
    /// for that direction as [`derive`] does, its glow its mean × the
    /// strength. `None` for no group or a patch without rows.
    pub fn place(&self, picture: u32, p: &PatchPlace) -> Option<GlowBeamSource> {
        if p.group == 0 || self.rows.is_empty() {
            return None;
        }
        let own = i64::from(self.direction);
        let d = p.direction.map_or(own, |d| d.rem_euclid(360));
        let d = if p.flip { i64::from(mirrored_direction(d)) } else { d };
        let spread = p.spread.map_or(i64::from(self.spread), |s| s.clamp(1, 90));
        let theta = (d as f32).to_radians();
        let dir = [libm::cosf(theta), libm::sinf(theta)];
        let perp = [-dir[1], dir[0]];
        let (mut face, mut lo, mut hi) = (f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
        let (ox, oy) = (p.at.0 as f32, p.at.1 as f32);
        let pw = picture as f32;
        for &(y, x0, x1) in &self.rows {
            for x in [x0, x1] {
                let x = if p.flip { pw - x as f32 } else { x as f32 };
                for y in [y, y + 1] {
                    let q = [ox + x, oy + y as f32];
                    face = face.max(q[0] * dir[0] + q[1] * dir[1]);
                    let side = q[0] * perp[0] + q[1] * perp[1];
                    lo = lo.min(side);
                    hi = hi.max(side);
                }
            }
        }
        let mid = (lo + hi) * 0.5;
        let fx = if p.flip { picture.saturating_sub(1).saturating_sub(self.first.0) } else { self.first.0 };
        let (wx, wy) = (i64::from(p.at.0) + i64::from(fx), i64::from(p.at.1) + i64::from(self.first.1));
        let inside = (0..i64::from(p.world.0)).contains(&wx) && (0..i64::from(p.world.1)).contains(&wy);
        let first = if inside { (wy * i64::from(p.world.0) + wx) as u32 } else { 0 };
        Some(GlowBeamSource {
            group: p.group,
            at: [dir[0] * face + perp[0] * mid, dir[1] * face + perp[1] * mid],
            dir,
            half: (hi - lo) * 0.5,
            spread: (spread as f32).to_radians(),
            glow: self.source.glow * f32::from(p.strength.min(100)) / 100.0,
            colour: self.source.colour,
            first,
            back: self.source.back,
        })
    }
}

fn fits(l: &BeamLayer) -> bool {
    let (w, h) = (l.world.0 as usize, l.world.1 as usize);
    let s = l.art_scale as usize;
    let cells = w.checked_mul(h);
    let art = w.checked_mul(s).zip(h.checked_mul(s)).and_then(|(a, b)| a.checked_mul(b));
    s >= 1
        && cells.is_some_and(|c| l.groups.len() == c && l.beams.len() == c * 2)
        && art.is_some_and(|a| l.glow.len() == a && Some(l.art.len()) == a.checked_mul(4))
}

/// Hands `out` each patch of `l` with its pixels (row-major world
/// indices).
fn layer(l: &BeamLayer, back: bool, out: &mut dyn FnMut(GlowBeamSource, &[usize])) {
    if !fits(l) {
        return;
    }
    let (w, h) = (l.world.0 as usize, l.world.1 as usize);
    let s = l.art_scale as usize;
    let aw = w * s;
    let key = |i: usize| (l.groups[i], l.beams[i * 2], l.beams[i * 2 + 1]);
    let mut seen = vec![false; w * h];
    let mut stack = Vec::new();
    let mut pixels = Vec::new();
    for first in 0..w * h {
        let (group, d, sp) = key(first);
        if seen[first] || d == 0 || group == 0 {
            continue;
        }
        let deg = f32::from(u16::from(d - 1) * 2);
        let theta = deg.to_radians();
        let dir = [libm::cosf(theta), libm::sinf(theta)];
        let perp = [-dir[1], dir[0]];
        let (mut face, mut lo, mut hi) = (f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
        let (mut glow, mut lin) = (0.0f64, [0.0f64; 3]);
        let mut n = 0usize;
        seen[first] = true;
        stack.push(first);
        pixels.clear();
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            n += 1;
            pixels.push(i);
            for (cx, cy) in [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)] {
                let p = [cx as f32, cy as f32];
                face = face.max(p[0] * dir[0] + p[1] * dir[1]);
                let side = p[0] * perp[0] + p[1] * perp[1];
                lo = lo.min(side);
                hi = hi.max(side);
            }
            for ay in y * s..(y + 1) * s {
                for ax in x * s..(x + 1) * s {
                    let a = ay * aw + ax;
                    glow += f64::from(l.glow[a]);
                    for (sum, byte) in lin.iter_mut().zip(&l.art[a * 4..a * 4 + 3]) {
                        *sum += f64::from(srgb_to_linear(*byte));
                    }
                }
            }
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] && key(j) == (group, d, sp) {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let texels = (n * s * s) as f64;
        let mean = lin.map(|c| (c / texels) as f32);
        let top = mean[0].max(mean[1]).max(mean[2]);
        let colour = if top > 0.0 { mean.map(|c| c / top) } else { [0.0; 3] };
        let mid = (lo + hi) * 0.5;
        let source = GlowBeamSource {
            group,
            at: [dir[0] * face + perp[0] * mid, dir[1] * face + perp[1] * mid],
            dir,
            half: (hi - lo) * 0.5,
            spread: f32::from(sp).to_radians(),
            glow: (glow / texels / 255.0) as f32,
            colour,
            first: first as u32,
            back,
        };
        out(source, &pixels);
    }
}
