//! Worm skins for the game and the menus: the frame layouts (OLX's and
//! double resolution's, issue #12), the sheet colorized as worms are drawn,
//! one frame's sprite, the glowing pixels and the skin's light per frame,
//! and the skin list. The game (`olr-scene`) and the profiles' preview
//! (`olr-ui`) both draw from here, so the preview shows the in-game pixels.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::manifest::PackKind;
use olr_data::skin::{FrameLayout, MaskKind};
use olr_sim::Dir;

use crate::{Content, LoadedSkin};

/// The frames of a sheet: three walk frames of seven aim angles.
pub use olr_data::skin::SHEET_FRAMES;

/// Where a frame's sprite is on the sheet, and whether it is drawn
/// mirrored (facing left).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinSource {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub mirrored: bool,
}

/// Frame `f`'s sprite facing `face` on a sheet of `layout` (OLX's
/// `include/CGameSkin.h:36-41`: frames 32 pixels apart, each a 20 × 18
/// sprite 4 pixels in; a double-resolution sheet's are twice that).
/// Facing left is the exact mirror of the right-facing rectangle. OLX
/// blits from its mirrored sheet one column to the right (`w − sx − 20 −
/// 1`), showing a column of the frame's spacing and losing one of the
/// sprite; that offset is dropped (spec 2026-10-16 formats §4).
///
/// OLX: src/client/CGameSkin.cpp:584-601
pub fn skin_source(layout: &FrameLayout, f: u32, face: Dir) -> SkinSource {
    let sx = f.saturating_mul(layout.frame_width).saturating_add(layout.frame_spacing);
    let (x, mirrored) = match face {
        Dir::Right => (sx, false),
        Dir::Left => (sx, true),
    };
    SkinSource { x, y: 0, w: layout.worm_width, h: layout.worm_height, mirrored }
}

/// The glow strengths drawn (issue #12), shared with `olr-edit`'s skin
/// preview.
pub use olr_data::skin::{GLOW_LEVELS, glow_level};

/// How much glow makes a skin's light full: 3 world pixels of full glow
/// in a frame (12 art pixels at art scale 2, 48 at 4); less glow scales
/// the light down.
pub const FULL_LIGHT_GLOW: f32 = 3.0;

/// A skin's light in one frame (issue #12): its colour (sRGB) and
/// intensity in percent, already scaled by how much of the frame glows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinFrameLight {
    pub colour: [u8; 3],
    pub intensity: u16,
}

/// A worm's skin light: its reach (world pixels) and its light in each
/// frame (by sheet frame index; `None` where the frame has no glow).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinLights {
    pub reach: u16,
    pub frames: Vec<Option<SkinFrameLight>>,
}

impl LoadedSkin {
    /// The sheet in the worm's `colour`, as the game draws worms from it
    /// (`olr_scene::SceneAssets::add_worm`).
    ///
    /// OLX: src/client/CGameSkin.cpp:720-790 (`Colorize_Execute`)
    pub fn colorized(&self, colour: Rgba) -> RgbaImage {
        self.skin.colorize(colour)
    }

    /// The sheet's frame layout.
    pub fn layout(&self) -> FrameLayout {
        self.skin.def.layout
    }

    /// Art pixels per world pixel: 2 for OLX's skins, 4 for double
    /// resolution's.
    pub fn art_scale(&self) -> u32 {
        self.skin.def.layout.art_scale()
    }

    /// The glowing pixels of the sheet in `colour` (issue #12), one image
    /// per glow strength that has any: `(strength, image)`, the strength
    /// 1 to [`GLOW_LEVELS`] (quarters of full glow), each image the
    /// colorized sheet's pixels of that strength (transparent elsewhere).
    /// Drawn over the worm with that self-light, they show bright whatever
    /// the darkness. Empty without a glow mask.
    pub fn glow_overlays(&self, colour: Rgba) -> Vec<(usize, RgbaImage)> {
        let s = &self.skin;
        if s.glow.is_empty() {
            return Vec::new();
        }
        let sheet = self.colorized(colour);
        let (w, h) = (sheet.width(), sheet.height());
        let mut out: Vec<(usize, RgbaImage)> = Vec::new();
        for level in 1..=GLOW_LEVELS {
            let mut img: Option<RgbaImage> = None;
            for y in 0..h {
                for x in 0..w {
                    if s.mask_at(x, y) != MaskKind::Transparent && glow_level(s.glow_at(x, y)) == level {
                        img.get_or_insert_with(|| RgbaImage::new(w, h)).set(x, y, sheet.get(x, y));
                    }
                }
            }
            out.extend(img.map(|i| (level, i)));
        }
        out
    }

    /// The skin's light in each frame for a worm of `colour` (issue #12):
    /// `None` without a `light` or a glowing pixel. A frame's colour is its
    /// glowing pixels' as drawn (`colour`'s tint included), weighted by
    /// their glow; its intensity is the light's, scaled by the frame's glow
    /// in world pixels over [`FULL_LIGHT_GLOW`] (at most 1). Mirroring
    /// does not change it.
    pub fn frame_lights(&self, colour: Rgba) -> Option<SkinLights> {
        let s = &self.skin;
        let light = s.def.light?;
        if s.glow.is_empty() {
            return None;
        }
        let sheet = self.colorized(colour);
        let layout = self.layout();
        let k = self.art_scale() as f32;
        let n = (sheet.width() / layout.frame_width.max(1)).max(1);
        let frames: Vec<Option<SkinFrameLight>> = (0..n)
            .map(|f| {
                let src = skin_source(&layout, f, Dir::Right);
                let (mut sum, mut weight) = ([0.0f32; 3], 0.0f32);
                for y in src.y..src.y.saturating_add(src.h).min(sheet.height()) {
                    for x in src.x..src.x.saturating_add(src.w).min(sheet.width()) {
                        if s.mask_at(x, y) == MaskKind::Transparent {
                            continue;
                        }
                        let g = f32::from(s.glow_at(x, y)) / 255.0;
                        let p = sheet.get(x, y);
                        for (c, v) in sum.iter_mut().zip([p.r, p.g, p.b]) {
                            *c += g * f32::from(v);
                        }
                        weight += g;
                    }
                }
                if weight <= 0.0 {
                    return None;
                }
                let colour = sum.map(|c| (c / weight).round().clamp(0.0, 255.0) as u8);
                let share = (weight / (k * k) / FULL_LIGHT_GLOW).min(1.0);
                let intensity = (f32::from(light.intensity) * share).round() as u16;
                (intensity > 0).then_some(SkinFrameLight { colour, intensity })
            })
            .collect();
        frames.iter().any(Option::is_some).then_some(SkinLights { reach: light.reach, frames })
    }
}

/// Frame `f` of a (colorized) `sheet` of `layout` facing `face`, as the
/// renderer draws it: [`skin_source`]'s rectangle, mirrored facing left.
/// Pixels outside the sheet are transparent (an odd sheet never panics).
pub fn skin_sprite(sheet: &RgbaImage, layout: &FrameLayout, f: u32, face: Dir) -> RgbaImage {
    let s = skin_source(layout, f, face);
    let mut out = RgbaImage::new(s.w, s.h);
    for y in 0..s.h {
        for x in 0..s.w {
            let sx = s.x.saturating_add(if s.mirrored { s.w - 1 - x } else { x });
            let sy = s.y.saturating_add(y);
            if sx < sheet.width() && sy < sheet.height() {
                out.set(x, y, sheet.get(sx, sy));
            }
        }
    }
    out
}

/// A skin as the menus list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinInfo {
    pub id: String,
    /// The pack's name: OLX's file name without `.png` (such as `default`).
    pub name: String,
}

/// The pack id among `ids` that `given` names: the id itself, else the
/// id OLX's import gives the name (`prefix` and its slug, with or without
/// an extension, such as `default.png` → `olx-skin-default`).
pub fn resolve_id(given: &str, prefix: &str, ids: &[String]) -> Option<String> {
    let stem = given.rsplit_once('.').map_or(given, |(s, _)| s);
    let slug = |s: &str| format!("{prefix}{}", olr_data::slug::slug(s));
    [given.to_string(), slug(given), slug(stem)].into_iter().find(|c| ids.contains(c))
}

/// The prefix of OLX's skins' pack ids (`olr-cli import`).
pub(crate) const SKIN_PREFIX: &str = "olx-skin-";

impl Content {
    /// Every skin pack with its name, in id order (nothing is decoded).
    pub fn skins(&self) -> Vec<SkinInfo> {
        self.ids(PackKind::Skin)
            .into_iter()
            .map(|id| {
                let name = self.pack_name(&id).unwrap_or(&id).to_string();
                SkinInfo { id, name }
            })
            .collect()
    }

    /// The skin pack `given` names ([`resolve_id`]), if there is one.
    pub fn skin_id(&self, given: &str) -> Option<String> {
        resolve_id(given, SKIN_PREFIX, &self.ids(PackKind::Skin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A left-facing frame is the exact mirror of the right-facing one
    /// (OLX's one-column offset, `src/client/CGameSkin.cpp:584-601`,
    /// dropped: spec 2026-10-16 formats §4).
    #[test]
    fn left_is_the_mirror_of_right() {
        let mut sheet = RgbaImage::new(672, 18);
        for x in 0..672 {
            for y in 0..18 {
                sheet.set(x, y, Rgba { r: (x % 251) as u8, g: y as u8, b: (x / 251) as u8, a: 255 });
            }
        }
        let olx = FrameLayout::OLX;
        for f in 0..21 {
            let (r, l) = (skin_sprite(&sheet, &olx, f, Dir::Right), skin_sprite(&sheet, &olx, f, Dir::Left));
            for y in 0..18 {
                for x in 0..20 {
                    assert_eq!(l.get(x, y), r.get(19 - x, y), "frame {f} ({x}, {y})");
                }
            }
        }
        // Off the sheet: transparent, no panic.
        let small = RgbaImage::new(64, 18);
        assert_eq!(skin_sprite(&small, &olx, 5, Dir::Left).get(0, 0), Rgba::TRANSPARENT);
        assert_eq!(skin_sprite(&small, &olx, u32::MAX, Dir::Left).get(0, 0), Rgba::TRANSPARENT);
    }

    /// Issue #12: a double-resolution sheet's frames are 64 apart, 8 in,
    /// 40 × 36.
    #[test]
    fn double_frames_are_cut_at_their_layout() {
        let d = FrameLayout::DOUBLE;
        assert_eq!(skin_source(&d, 0, Dir::Right), SkinSource { x: 8, y: 0, w: 40, h: 36, mirrored: false });
        assert_eq!(
            skin_source(&d, 20, Dir::Left),
            SkinSource { x: 1288, y: 0, w: 40, h: 36, mirrored: true }
        );
        assert_eq!(skin_source(&FrameLayout::OLX, 20, Dir::Right).x, 644);
    }

    #[test]
    fn glow_rounds_to_quarters() {
        let levels: Vec<usize> = [0u8, 31, 32, 95, 96, 159, 160, 223, 224, 255].map(glow_level).into();
        assert_eq!(levels, [0, 0, 1, 1, 2, 2, 3, 3, 4, 4]);
    }

    #[test]
    fn names_resolve_to_ids() {
        let ids = vec!["olx-skin-default".to_string(), "mine".to_string()];
        assert_eq!(resolve_id("default", SKIN_PREFIX, &ids).as_deref(), Some("olx-skin-default"));
        assert_eq!(resolve_id("default.png", SKIN_PREFIX, &ids).as_deref(), Some("olx-skin-default"));
        assert_eq!(resolve_id("mine", SKIN_PREFIX, &ids).as_deref(), Some("mine"));
        assert_eq!(resolve_id("other", SKIN_PREFIX, &ids), None);
    }
}
