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
