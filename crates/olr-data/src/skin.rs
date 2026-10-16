//! Worm skins: a sprite sheet plus a per-pixel colorization mask.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::{Rgba, RgbaImage};

/// What happens to a skin pixel when the player's color is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaskKind {
    /// Copy the pixel unchanged (OLX: black mask pixel).
    Keep,
    /// Tint with the player color (OLX: any other mask color).
    Colorize,
    /// Do not draw (OLX: magenta mask pixel).
    Transparent,
}

/// Frame layout of the sheet (`include/CGameSkin.h:37-41`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameLayout {
    pub frame_width: u32,
    pub frame_height: u32,
    pub frame_spacing: u32,
    pub worm_width: u32,
    pub worm_height: u32,
}

impl FrameLayout {
    /// The layout every OLX skin uses.
    pub const OLX: FrameLayout =
        FrameLayout { frame_width: 32, frame_height: 18, frame_spacing: 4, worm_width: 20, worm_height: 18 };
}

/// The `skin.ron` part of a skin pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkinDef {
    pub name: String,
    pub layout: FrameLayout,
}

/// A loaded skin. `image` and `mask` have the same size.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    pub def: SkinDef,
    pub image: RgbaImage,
    pub mask: Vec<MaskKind>,
}

impl Skin {
    /// The mask at `(x, y)`. Panics if out of bounds.
    pub fn mask_at(&self, x: u32, y: u32) -> MaskKind {
        assert!(x < self.image.width() && y < self.image.height(), "mask ({x},{y}) out of bounds");
        self.mask[y as usize * self.image.width() as usize + x as usize]
    }

    /// The sheet tinted with `player`, following OLX `CGameSkin::Colorize_Execute`
    /// (`src/client/CGameSkin.cpp:720-790`).
    pub fn colorize(&self, player: Rgba) -> RgbaImage {
        let mut out = RgbaImage::new(self.image.width(), self.image.height());
        for y in 0..self.image.height() {
            for x in 0..self.image.width() {
                let px = self.image.get(x, y);
                let c = match self.mask_at(x, y) {
                    MaskKind::Keep => px,
                    MaskKind::Transparent => Rgba::TRANSPARENT,
                    MaskKind::Colorize => colorize_pixel(px, player),
                };
                out.set(x, y, c);
            }
        }
        out
    }
}

/// OLX's tint formula: `min(255, c * player / divisor)` with divisors 96, 156, 252,
/// and pure magenta nudged to (240, 0, 240) so it does not become the color key.
/// Alpha is kept from `px`.
pub fn colorize_pixel(px: Rgba, player: Rgba) -> Rgba {
    let tint = |c: u8, p: u8, div: u32| ((c as u32 * p as u32) / div).min(255) as u8;
    let mut out = Rgba {
        r: tint(px.r, player.r, 96),
        g: tint(px.g, player.g, 156),
        b: tint(px.b, player.b, 252),
        a: px.a,
    };
    if out.same_rgb(Rgba::MAGENTA) {
        out.r = 240;
        out.b = 240;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn colorize_pixel_matches_olx_formula() {
        let white = Rgba::rgb(255, 255, 255);
        assert_eq!(colorize_pixel(white, Rgba::rgb(96, 156, 252)), white);
        assert_eq!(colorize_pixel(Rgba::rgb(100, 100, 100), Rgba::rgb(48, 78, 126)), Rgba::rgb(50, 50, 50));
        assert_eq!(colorize_pixel(white, Rgba::rgb(0, 0, 0)), Rgba::rgb(0, 0, 0));
    }

    #[test]
    fn colorize_pixel_avoids_color_key() {
        // r: 255*96/96 = 255, g: 0, b: 255*252/252 = 255 -> would be magenta.
        let out = colorize_pixel(Rgba::rgb(255, 0, 255), Rgba::rgb(96, 0, 252));
        assert_eq!(out, Rgba::rgb(240, 0, 240));
    }

    #[test]
    fn colorize_respects_mask() {
        let mut image = RgbaImage::new(3, 1);
        for x in 0..3 {
            image.set(x, 0, Rgba::rgb(200, 200, 200));
        }
        let skin = Skin {
            def: SkinDef { name: "t".into(), layout: FrameLayout::OLX },
            image,
            mask: vec![MaskKind::Keep, MaskKind::Colorize, MaskKind::Transparent],
        };
        let out = skin.colorize(Rgba::rgb(0, 0, 0));
        assert_eq!(out.get(0, 0), Rgba::rgb(200, 200, 200));
        assert_eq!(out.get(1, 0), Rgba::rgb(0, 0, 0));
        assert_eq!(out.get(2, 0), Rgba::TRANSPARENT);
    }
}
