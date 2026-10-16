//! Plain RGBA images. Encoding to and from PNG lives in `olr-pack`.

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

    /// The image's colour key as OLX's `SetColorKey` picks it: some editors
    /// saved magenta as (254,0,254), so when any corner pixel has that
    /// colour it is the key of the whole image (and pure magenta an
    /// ordinary colour); otherwise the key is [`Rgba::MAGENTA`].
    ///
    /// OLX: src/client/GfxPrimitives.cpp:116-150
    pub fn olx_color_key(&self) -> Rgba {
        let bugged = Rgba::rgb(254, 0, 254);
        let (Some(w), Some(h)) = (self.width.checked_sub(1), self.height.checked_sub(1)) else {
            return Rgba::MAGENTA;
        };
        let corners = [(0, 0), (w, 0), (0, h), (w, h)];
        if corners.iter().any(|&(x, y)| self.get(x, y).same_rgb(bugged)) { bugged } else { Rgba::MAGENTA }
    }

    /// Makes every pixel of [`RgbaImage::olx_color_key`] transparent, as
    /// OLX draws a colour-keyed surface.
    pub fn key_transparent(&mut self) {
        let key = self.olx_color_key();
        for p in self.pixels.as_chunks_mut::<4>().0 {
            if p[..3] == [key.r, key.g, key.b] {
                *p = [0; 4];
            }
        }
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
    fn olx_color_key_follows_the_corner_rule() {
        let bugged = Rgba::rgb(254, 0, 254);
        let mut img = RgbaImage::new(3, 2);
        img.set(1, 0, Rgba::MAGENTA);
        img.set(1, 1, bugged);
        assert_eq!(img.olx_color_key(), Rgba::MAGENTA, "(254,0,254) inside only");
        for (x, y) in [(0, 0), (2, 0), (0, 1), (2, 1)] {
            let mut corner = img.clone();
            corner.set(x, y, bugged);
            assert_eq!(corner.olx_color_key(), bugged, "corner ({x},{y})");
        }
        assert_eq!(RgbaImage::new(0, 0).olx_color_key(), Rgba::MAGENTA, "an empty image never panics");
    }

    #[test]
    fn keying_clears_only_the_key() {
        let bugged = Rgba::rgb(254, 0, 254);
        let mut img = RgbaImage::new(3, 1);
        img.set(0, 0, Rgba::MAGENTA);
        img.set(1, 0, bugged);
        let mut plain = img.clone();
        plain.key_transparent();
        assert_eq!(plain.get(0, 0), Rgba::TRANSPARENT);
        assert_eq!(plain.get(1, 0), bugged, "(254,0,254) is a colour without a bugged corner");
        // With a bugged corner, (254,0,254) is the key and magenta a colour.
        let mut img = RgbaImage::new(2, 2);
        img.set(0, 0, Rgba::MAGENTA);
        img.set(1, 1, bugged);
        img.key_transparent();
        assert_eq!(img.get(0, 0), Rgba::MAGENTA);
        assert_eq!(img.get(1, 1), Rgba::TRANSPARENT);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn get_out_of_bounds_panics() {
        RgbaImage::new(1, 1).get(1, 0);
    }
}
