//! Worm skins for the game and the menus: OLX's frame layout, the sheet
//! colorized as worms are drawn, one frame's sprite, and the skin list.
//! The game (`rl-scene`) and the profiles' preview (`rl-ui`) both draw
//! from here, so the preview shows the in-game pixels.

use rl_data::image::{Rgba, RgbaImage};
use rl_data::manifest::PackKind;
use rl_sim::Dir;

use crate::{Content, LoadedSkin};

/// OLX's skin layout (`include/CGameSkin.h:36-41`): frames 32 pixels
/// apart, each a 20 × 18 sprite 4 pixels in.
pub const FRAME_WIDTH: u32 = 32;
pub const FRAME_SPACING: u32 = 4;
pub const SKIN_WIDTH: u32 = 20;
pub const SKIN_HEIGHT: u32 = 18;

/// The frames of an OLX sheet: three walk frames of seven aim angles.
pub const SHEET_FRAMES: u32 = 21;

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

/// Frame `f`'s sprite facing `face`. Facing left, OLX blits from its
/// mirrored sheet at `w − sx − 20 − 1`, which shows the source columns one
/// to the right of the right-facing sprite (an OLX quirk, kept).
///
/// OLX: src/client/CGameSkin.cpp:584-600
pub fn skin_source(f: u32, face: Dir) -> SkinSource {
    let sx = f * FRAME_WIDTH + FRAME_SPACING;
    let (x, mirrored) = match face {
        Dir::Right => (sx, false),
        Dir::Left => (sx + 1, true),
    };
    SkinSource { x, y: 0, w: SKIN_WIDTH, h: SKIN_HEIGHT, mirrored }
}

impl LoadedSkin {
    /// The sheet in the worm's `colour`, as the game draws worms from it
    /// (`rl_scene::SceneAssets::add_worm`).
    ///
    /// OLX: src/client/CGameSkin.cpp:720-790 (`Colorize_Execute`)
    pub fn colorized(&self, colour: Rgba) -> RgbaImage {
        self.skin.colorize(colour)
    }
}

/// Frame `f` of a (colorized) `sheet` facing `face`, as the renderer draws
/// it: [`skin_source`]'s rectangle, mirrored facing left. Pixels outside
/// the sheet are transparent (an odd sheet never panics).
pub fn skin_sprite(sheet: &RgbaImage, f: u32, face: Dir) -> RgbaImage {
    let s = skin_source(f, face);
    let mut out = RgbaImage::new(s.w, s.h);
    for y in 0..s.h {
        for x in 0..s.w {
            let sx = if s.mirrored { s.x + s.w - 1 - x } else { s.x + x };
            let sy = s.y + y;
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
    let slug = |s: &str| format!("{prefix}{}", rl_data::slug::slug(s));
    [given.to_string(), slug(given), slug(stem)].into_iter().find(|c| ids.contains(c))
}

/// The prefix of OLX's skins' pack ids (`rl-cli import`).
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

    #[test]
    fn left_shows_the_columns_one_to_the_right_mirrored() {
        let mut sheet = RgbaImage::new(64, 18);
        for x in 0..64 {
            sheet.set(x, 0, Rgba::rgb(x as u8, 0, 0));
        }
        let right = skin_sprite(&sheet, 1, Dir::Right);
        let left = skin_sprite(&sheet, 1, Dir::Left);
        assert_eq!(right.get(0, 0).r, 36);
        assert_eq!(left.get(0, 0).r, 36 + 20);
        assert_eq!(left.get(19, 0).r, 37);
        // Off the sheet: transparent, no panic.
        assert_eq!(skin_sprite(&sheet, 5, Dir::Left).get(0, 0), Rgba::TRANSPARENT);
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
