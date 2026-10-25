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
    layer(front, false, &mut out);
    layer(back, true, &mut out);
    out
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

fn layer(l: &BeamLayer, back: bool, out: &mut Vec<GlowBeamSource>) {
    if !fits(l) {
        return;
    }
    let (w, h) = (l.world.0 as usize, l.world.1 as usize);
    let s = l.art_scale as usize;
    let aw = w * s;
    let key = |i: usize| (l.groups[i], l.beams[i * 2], l.beams[i * 2 + 1]);
    let mut seen = vec![false; w * h];
    let mut stack = Vec::new();
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
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            n += 1;
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
        out.push(GlowBeamSource {
            group,
            at: [dir[0] * face + perp[0] * mid, dir[1] * face + perp[1] * mid],
            dir,
            half: (hi - lo) * 0.5,
            spread: f32::from(sp).to_radians(),
            glow: (glow / texels / 255.0) as f32,
            colour,
            first: first as u32,
            back,
        });
    }
}
