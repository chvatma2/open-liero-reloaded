//! Glow masks (look system, plan 15a Task 5): painting an element's mask
//! by world shapes.

use crate::level::{Shape, covers, extent};

/// Sets `mask` (an element box of `size` world pixels at `at`, at art scale
/// `s`, row-major art pixels) to `value` on every art pixel of a world pixel
/// `shape` covers. Clipped to the box; never panics.
pub fn paint_mask(mask: &mut [u8], at: (i32, i32), size: (u32, u32), s: u32, shape: &Shape, value: u8) {
    let (x0, y0, x1, y1) = extent(shape);
    let clip = |v: i64, a: i32, side: u32| (v - i64::from(a)).clamp(0, i64::from(side));
    let (bx0, bx1) = (clip(x0, at.0, size.0), clip(x1, at.0, size.0));
    let (by0, by1) = (clip(y0, at.1, size.1), clip(y1, at.1, size.1));
    let cols = u64::from(size.0) * u64::from(s);
    for by in by0..by1 {
        for bx in bx0..bx1 {
            if !covers(shape, bx + i64::from(at.0), by + i64::from(at.1)) {
                continue;
            }
            for dy in 0..u64::from(s) {
                for dx in 0..u64::from(s) {
                    let i = (by as u64 * u64::from(s) + dy) * cols + bx as u64 * u64::from(s) + dx;
                    if let Some(m) = mask.get_mut(i as usize) {
                        *m = value;
                    }
                }
            }
        }
    }
}

/// The 4-connected art pixels of `art` from `seed` whose colours are
/// `similar` to the seed's within `tolerance` percent (glow by colour, the
/// user's request 2026-10-19): one flag per art pixel, row-major. `None`
/// when `seed` is outside. An explicit stack: no recursion.
pub fn colour_region(art: &olr_data::image::RgbaImage, seed: (u32, u32), tolerance: u8) -> Option<Vec<bool>> {
    let (w, h) = (art.width(), art.height());
    if seed.0 >= w || seed.1 >= h {
        return None;
    }
    let colour = art.get(seed.0, seed.1);
    let mut inside = vec![false; w as usize * h as usize];
    let mut stack = vec![seed];
    inside[seed.1 as usize * w as usize + seed.0 as usize] = true;
    while let Some((x, y)) = stack.pop() {
        let around = [
            (x.checked_sub(1), Some(y)),
            ((x + 1 < w).then_some(x + 1), Some(y)),
            (Some(x), y.checked_sub(1)),
            (Some(x), (y + 1 < h).then_some(y + 1)),
        ];
        for (nx, ny) in around {
            let (Some(nx), Some(ny)) = (nx, ny) else { continue };
            let i = ny as usize * w as usize + nx as usize;
            if !inside[i] && crate::level::similar(art.get(nx, ny), colour, tolerance) {
                inside[i] = true;
                stack.push((nx, ny));
            }
        }
    }
    Some(inside)
}

/// `mask`, the glow mask of a box `from` (world origin, size) at art scale
/// `s`, laid on box `to` by world position: what falls outside `from`
/// keeps everything (255). A crop or a level resize moves a mask this way.
pub fn relay_mask(
    mask: &[u8],
    from: ((i32, i32), (u32, u32)),
    to: ((i32, i32), (u32, u32)),
    s: u32,
) -> Vec<u8> {
    let s = s.max(1) as i64;
    let (fw, fh) = (i64::from(from.1.0) * s, i64::from(from.1.1) * s);
    let (tw, th) = (i64::from(to.1.0) * s, i64::from(to.1.1) * s);
    let mut out = vec![255u8; (tw * th).max(0) as usize];
    if mask.len() as i64 != fw * fh {
        return out;
    }
    // Art pixel (x, y) of `to` is art pixel (x + dx, y + dy) of `from`.
    let dx = (i64::from(to.0.0) - i64::from(from.0.0)) * s;
    let dy = (i64::from(to.0.1) - i64::from(from.0.1)) * s;
    for y in 0..th {
        let fy = y + dy;
        if !(0..fh).contains(&fy) {
            continue;
        }
        for x in 0..tw {
            let fx = x + dx;
            if (0..fw).contains(&fx) {
                out[(y * tw + x) as usize] = mask[(fy * fw + fx) as usize];
            }
        }
    }
    out
}

/// `mask` (`from` art pixels) resampled to `to` art pixels, nearest: a
/// resized image's, or another art scale's. One of another size than
/// `from` keeps everything (255).
pub fn resample_mask(mask: &[u8], from: (u32, u32), to: (u32, u32)) -> Vec<u8> {
    let n = to.0 as usize * to.1 as usize;
    if mask.len() != from.0 as usize * from.1 as usize || from.0 == 0 || from.1 == 0 {
        return vec![255; n];
    }
    let mut out = Vec::with_capacity(n);
    for y in 0..to.1 as u64 {
        let fy = (y * u64::from(from.1) / u64::from(to.1.max(1))) as usize;
        for x in 0..to.0 as u64 {
            let fx = (x * u64::from(from.0) / u64::from(to.0.max(1))) as usize;
            out.push(mask[fy * from.0 as usize + fx]);
        }
    }
    out
}
