//! Carving stamps: a theme's hole images classified the way OLX reads them.

use alloc::vec::Vec;

use olr_data::image::{Rgba, RgbaImage};

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

/// What a green-dirt mask pixel does (`CMap::PlaceGreenDirt`): the mask has
/// no colour key, and its pixels compare raw (alpha included) against two
/// opaque colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GreenPixel {
    /// Green `(0, 255, 0)`: dirt with a random green colour, one `rand()` each.
    Green,
    /// Pink `(255, 0, 255)`: leave the map alone.
    Pink,
    /// Any other colour (the rim): dirt painted in this colour.
    Rim(Rgba),
}

/// The green-dirt mask, `data/gfx/greenball.png` (`CMap.cpp:365-367`).
#[derive(Clone, Debug, PartialEq)]
pub struct GreenMask {
    width: u32,
    height: u32,
    pixels: Vec<GreenPixel>,
}

impl GreenMask {
    pub fn from_image(img: &RgbaImage) -> Self {
        let mut pixels = Vec::with_capacity(img.width() as usize * img.height() as usize);
        for y in 0..img.height() {
            for x in 0..img.width() {
                let c = img.get(x, y);
                // Raw `Uint32` compares with `MakeColour`, which is opaque.
                pixels.push(if c == Rgba::rgb(0, 255, 0) {
                    GreenPixel::Green
                } else if c == Rgba::MAGENTA {
                    GreenPixel::Pink
                } else {
                    GreenPixel::Rim(c)
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
    pub fn get(&self, x: u32, y: u32) -> GreenPixel {
        self.pixels[(y * self.width + x) as usize]
    }
}
