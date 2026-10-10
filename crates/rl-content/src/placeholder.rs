//! Placeholders for missing or odd assets.

use rl_data::image::{Rgba, RgbaImage};

/// The colour of placeholder images: magenta, but not OLX's colour key
/// (`255, 0, 255`, transparent in sprites), so a placeholder stays visible.
pub const PLACEHOLDER: Rgba = Rgba::rgb(255, 0, 220);

/// A `w` × `h` image of [`PLACEHOLDER`] (at least 1 × 1).
pub fn placeholder_image(w: u32, h: u32) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, PLACEHOLDER);
        }
    }
    img
}

/// The side of a placeholder for an image of unknown size.
pub(crate) const PLACEHOLDER_SIZE: u32 = 8;
