//! Plain RGBA images. Encoding to and from PNG lives in `rl-pack`.

use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// An 8-bit-per-channel color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// Opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// OLX's color key: pure magenta marks "transparent" in skins, sprites and stamps.
    pub const MAGENTA: Rgba = Rgba::rgb(255, 0, 255);
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };

    /// True when red, green and blue match, ignoring alpha.
    pub fn same_rgb(self, other: Rgba) -> bool {
        self.r == other.r && self.g == other.g && self.b == other.b
    }
}

/// A row-major RGBA8 image: `pixels.len() == width * height * 4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaImage {
    /// A fully transparent image.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, pixels: vec![0; width as usize * height as usize * 4] }
    }

    /// Wraps raw RGBA8 bytes. Returns `None` if the length does not match.
    pub fn from_raw(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        (pixels.len() == width as usize * height as usize * 4).then_some(Self { width, height, pixels })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn as_raw(&self) -> &[u8] {
        &self.pixels
    }

    /// Panics if `(x, y)` is out of bounds.
    pub fn get(&self, x: u32, y: u32) -> Rgba {
        let i = self.index(x, y);
        let p = &self.pixels[i..i + 4];
        Rgba { r: p[0], g: p[1], b: p[2], a: p[3] }
    }

    /// Panics if `(x, y)` is out of bounds.
    pub fn set(&mut self, x: u32, y: u32, c: Rgba) {
        let i = self.index(x, y);
        self.pixels[i..i + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
    }

    /// Copies the `w`×`h` region at `(x, y)` into a new image. Panics if out of bounds.
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        assert!(x + w <= self.width && y + h <= self.height, "crop out of bounds");
        let mut out = RgbaImage::new(w, h);
        for row in 0..h {
            let src = self.index(x, y + row);
            let dst = out.index(0, row);
            let len = w as usize * 4;
            out.pixels[dst..dst + len].copy_from_slice(&self.pixels[src..src + len]);
        }
        out
    }

    fn index(&self, x: u32, y: u32) -> usize {
        assert!(x < self.width && y < self.height, "pixel ({x},{y}) out of bounds");
        (y as usize * self.width as usize + x as usize) * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_roundtrip() {
        let mut img = RgbaImage::new(3, 2);
        img.set(2, 1, Rgba::rgb(1, 2, 3));
        assert_eq!(img.get(2, 1), Rgba::rgb(1, 2, 3));
        assert_eq!(img.get(0, 0), Rgba::TRANSPARENT);
    }

    #[test]
    fn from_raw_rejects_wrong_length() {
        assert!(RgbaImage::from_raw(2, 2, vec![0; 15]).is_none());
        assert!(RgbaImage::from_raw(2, 2, vec![0; 16]).is_some());
    }

    #[test]
    fn crop_copies_region() {
        let mut img = RgbaImage::new(4, 4);
        img.set(1, 2, Rgba::rgb(9, 9, 9));
        let c = img.crop(1, 1, 2, 2);
        assert_eq!((c.width(), c.height()), (2, 2));
        assert_eq!(c.get(0, 1), Rgba::rgb(9, 9, 9));
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn get_out_of_bounds_panics() {
        RgbaImage::new(1, 1).get(1, 0);
    }
}
