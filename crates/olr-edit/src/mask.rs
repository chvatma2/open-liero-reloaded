//! Image masks (plan 14d R7, spec "Masks on image elements"): which pixels
//! of an image element's original are kept, in the original's pixel grid
//! as it is shown (EXIF-turned), applied before the pipeline so resizing,
//! pixelation and colour reduction still work on what is left.

use olr_data::image::RgbaImage;
use olr_format::image_io::{self, IMAGES};

use crate::compose::ComposeError;
use crate::level::{Rect, Shape, covers, extent, similar};

/// Which pixels of an original are kept: one byte per pixel, row-major,
/// 0 erased and anything else kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    pub size: (u32, u32),
    pub keep: Vec<u8>,
}

impl Mask {
    /// Everything kept.
    pub fn full(size: (u32, u32)) -> Mask {
        Mask { size, keep: vec![255; size.0 as usize * size.1 as usize] }
    }

    fn fits(&self) -> bool {
        self.keep.len() as u64 == u64::from(self.size.0) * u64::from(self.size.1)
    }

    /// `original` with its erased pixels transparent; `None` when the
    /// sizes differ.
    pub fn apply(&self, original: &RgbaImage) -> Option<RgbaImage> {
        if (original.width(), original.height()) != self.size || !self.fits() {
            return None;
        }
        let mut out = original.clone();
        for (i, &k) in self.keep.iter().enumerate() {
            if k == 0 {
                let (x, y) = (i as u32 % self.size.0, i as u32 / self.size.0);
                out.set(x, y, olr_data::image::Rgba::TRANSPARENT);
            }
        }
        Some(out)
    }

    /// The size and kept pixels' FNV-1a hash; never 0 (0 is "no mask").
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let size = [self.size.0.to_le_bytes(), self.size.1.to_le_bytes()].concat();
        for &b in size.iter().chain(self.keep.iter().map(|k| if *k == 0 { &0u8 } else { &1u8 })) {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        h.max(1)
    }

    /// A grey PNG: 0 erased, 255 kept.
    pub fn encode(&self) -> Vec<u8> {
        let values: Vec<u8> = self.keep.iter().map(|&k| if k == 0 { 0 } else { 255 }).collect();
        image_io::encode_grey(self.size.0, self.size.1, &values)
    }

    /// Mask file `file` (`bytes`) of an original of size `original`.
    pub fn decode(file: &str, bytes: &[u8], original: (u32, u32)) -> Result<Mask, ComposeError> {
        let fail = |message: String| ComposeError::File { file: file.into(), message };
        let (w, h, keep) = image_io::decode_grey_within(bytes, IMAGES).map_err(fail)?;
        if (w, h) != original {
            return Err(fail(format!("{w}x{h}, the original is {}x{}", original.0, original.1)));
        }
        Ok(Mask { size: (w, h), keep })
    }

    /// The bounding box of the pixels whose kept state differs from
    /// `other`'s; `None` when none does, or the sizes differ.
    pub fn diff(&self, other: &Mask) -> Option<Rect> {
        if self.size != other.size || !self.fits() || !other.fits() {
            return None;
        }
        let w = self.size.0;
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for (i, (a, b)) in self.keep.iter().zip(&other.keep).enumerate() {
            if (*a == 0) != (*b == 0) {
                let (x, y) = (i as u32 % w, i as u32 / w);
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
        bounds.map(|(x0, y0, x1, y1)| Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 })
    }

    /// Whether `r` lies inside the mask.
    fn holds(&self, r: Rect) -> bool {
        self.fits()
            && r.x.checked_add(r.w).is_some_and(|e| e <= self.size.0)
            && r.y.checked_add(r.h).is_some_and(|e| e <= self.size.1)
    }

    /// The bytes of `r`, row-major; empty when `r` does not fit.
    pub fn crop(&self, r: Rect) -> Vec<u8> {
        if !self.holds(r) {
            return Vec::new();
        }
        let w = self.size.0 as usize;
        (r.y..r.y + r.h)
            .flat_map(|y| {
                let row = y as usize * w;
                self.keep[row + r.x as usize..row + (r.x + r.w) as usize].iter().copied()
            })
            .collect()
    }

    /// Writes `bytes` (a `crop` of `r`) back; false, and unchanged, when it
    /// does not fit.
    pub fn patch(&mut self, r: Rect, bytes: &[u8]) -> bool {
        if !self.holds(r) || bytes.len() as u64 != u64::from(r.w) * u64::from(r.h) {
            return false;
        }
        let w = self.size.0 as usize;
        for (k, row) in bytes.chunks(r.w.max(1) as usize).enumerate() {
            let at = (r.y as usize + k) * w + r.x as usize;
            self.keep[at..at + row.len()].copy_from_slice(row);
        }
        true
    }
}

/// The world pixel (offset in the box) holding original pixel `o`'s
/// centre, along one axis: `⌊(2·o + 1) · side / (2 · original)⌋`.
fn world_of(o: u32, side: u32, original: u32) -> i64 {
    ((2 * u64::from(o) + 1) * u64::from(side) / (2 * u64::from(original).max(1))) as i64
}

/// The original pixel under world pixel `world` (at its centre) for an
/// image box `at`, `size` whose original is `original` (`None` outside
/// the box).
pub fn original_pixel(
    at: (i32, i32),
    size: (u32, u32),
    original: (u32, u32),
    world: (i32, i32),
) -> Option<(u32, u32)> {
    let axis = |w: i32, a: i32, side: u32, orig: u32| -> Option<u32> {
        let k = i64::from(w) - i64::from(a);
        if !(0..i64::from(side)).contains(&k) || orig == 0 {
            return None;
        }
        Some(((2 * k as u64 + 1) * u64::from(orig) / (2 * u64::from(side))) as u32)
    };
    Some((axis(world.0, at.0, size.0, original.0)?, axis(world.1, at.1, size.1, original.1)?))
}

/// Masks (`keep` false) or restores (`keep` true) the original pixels
/// whose centre lies in a world pixel `shape` covers, for an image box
/// `at`, `size`; only the original pixels of the shape's extent within the
/// box are visited. Returns the original-pixel rectangle visited, `None`
/// when the shape misses the box.
pub fn mask_shape(
    mask: &mut Mask,
    at: (i32, i32),
    size: (u32, u32),
    shape: &Shape,
    keep: bool,
) -> Option<Rect> {
    let (ow, oh) = mask.size;
    if !mask.fits() || ow == 0 || oh == 0 || size.0 == 0 || size.1 == 0 {
        return None;
    }
    let (x0, y0, x1, y1) = extent(shape);
    // The shape's extent in box offsets, clipped to the box.
    let clip = |v: i64, a: i32, side: u32| (v - i64::from(a)).clamp(0, i64::from(side));
    let (k0x, k1x) = (clip(x0, at.0, size.0), clip(x1, at.0, size.0));
    let (k0y, k1y) = (clip(y0, at.1, size.1), clip(y1, at.1, size.1));
    if k0x >= k1x || k0y >= k1y {
        return None;
    }
    // Original pixels whose centre maps into those offsets, one either side.
    let range = |k0: i64, k1: i64, side: u32, orig: u32| {
        let lo = (k0 as u64 * u64::from(orig) / u64::from(side)).saturating_sub(1) as u32;
        let hi = ((k1 as u64 * u64::from(orig)).div_ceil(u64::from(side)) + 1).min(u64::from(orig)) as u32;
        (lo, hi)
    };
    let (ox0, ox1) = range(k0x, k1x, size.0, ow);
    let (oy0, oy1) = range(k0y, k1y, size.1, oh);
    let value = if keep { 255 } else { 0 };
    for oy in oy0..oy1 {
        let wy = i64::from(at.1) + world_of(oy, size.1, oh);
        for ox in ox0..ox1 {
            let wx = i64::from(at.0) + world_of(ox, size.0, ow);
            if covers(shape, wx, wy) {
                mask.keep[oy as usize * ow as usize + ox as usize] = value;
            }
        }
    }
    Some(Rect { x: ox0, y: oy0, w: ox1 - ox0, h: oy1 - oy0 })
}

/// Masks the 4-connected region of `original` from `seed` whose colours
/// are `similar` to the seed's within `tolerance` (percent), whatever
/// their mask state (plan 14d R8). Returns the bounding box masked, `None`
/// when `seed` is outside or the sizes differ.
pub fn mask_colour(mask: &mut Mask, original: &RgbaImage, seed: (u32, u32), tolerance: u8) -> Option<Rect> {
    let (w, h) = mask.size;
    if (original.width(), original.height()) != (w, h) || !mask.fits() || seed.0 >= w || seed.1 >= h {
        return None;
    }
    let colour = original.get(seed.0, seed.1);
    let mut seen = vec![false; w as usize * h as usize];
    let mut stack = vec![seed];
    seen[seed.1 as usize * w as usize + seed.0 as usize] = true;
    let (mut x0, mut y0, mut x1, mut y1) = (seed.0, seed.1, seed.0, seed.1);
    while let Some((x, y)) = stack.pop() {
        mask.keep[y as usize * w as usize + x as usize] = 0;
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        let around = [
            (x.checked_sub(1), Some(y)),
            ((x + 1 < w).then_some(x + 1), Some(y)),
            (Some(x), y.checked_sub(1)),
            (Some(x), (y + 1 < h).then_some(y + 1)),
        ];
        for (nx, ny) in around {
            let (Some(nx), Some(ny)) = (nx, ny) else { continue };
            let i = ny as usize * w as usize + nx as usize;
            if !seen[i] && similar(original.get(nx, ny), colour, tolerance) {
                seen[i] = true;
                stack.push((nx, ny));
            }
        }
    }
    Some(Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 })
}
