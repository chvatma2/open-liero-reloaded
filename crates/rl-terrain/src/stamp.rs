//! Carving stamps: a theme's hole images classified the way OLX reads them.

use alloc::vec::Vec;

use rl_data::image::{Rgba, RgbaImage};

/// What a stamp pixel does when carving (`CMap::CarveHole`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampPixel {
    /// Magenta: turn dirt into background.
    Carve,
    /// Black: leave the map alone.
    Skip,
    /// Any other colour: dirt stays, but the renderer paints this colour (the dirt edge).
    Paint(Rgba),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    width: u32,
    height: u32,
    pixels: Vec<StampPixel>,
}

impl Stamp {
    pub fn from_image(img: &RgbaImage) -> Self {
        let mut pixels = Vec::with_capacity(img.width() as usize * img.height() as usize);
        for y in 0..img.height() {
            for x in 0..img.width() {
                let c = img.get(x, y);
                pixels.push(if c.same_rgb(Rgba::MAGENTA) {
                    StampPixel::Carve
                } else if c.same_rgb(Rgba::BLACK) {
                    StampPixel::Skip
                } else {
                    StampPixel::Paint(c)
                });
            }
        }
        Self { width: img.width(), height: img.height(), pixels }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Panics if out of bounds.
    pub fn get(&self, x: u32, y: u32) -> StampPixel {
        self.pixels[(y * self.width + x) as usize]
    }
}
