//! Levels, themes and skins.

use anyhow::Context as _;
use rl_data::image::RgbaImage;
use rl_data::level::{MaterialGrid, SpawnPoint};
use rl_data::manifest::PackKind;
use rl_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};
use rl_terrain::{GreenMask, Stamp};

use crate::Content;
use crate::placeholder::placeholder_image;

/// A level: its material grid and its art (`art_scale` art pixels per
/// material pixel, as the level pack guarantees).
#[derive(Clone, Debug)]
pub struct LoadedLevel {
    pub name: String,
    pub material: MaterialGrid,
    pub front: RgbaImage,
    pub back: RgbaImage,
    pub art_scale: u32,
    /// The theme pack's id, such as `olx-theme-dirt`.
    pub theme: String,
    pub spawns: Vec<SpawnPoint>,
}

/// A theme: the stamps the map is edited with and the tiles dirt is drawn
/// from. Images keep OLX's raw colours (see [`rl_data::theme::Theme`]).
#[derive(Clone, Debug)]
pub struct LoadedTheme {
    pub name: String,
    /// `hole1` to `hole5`.
    pub holes: Vec<RgbaImage>,
    /// The green-dirt mask (`data/gfx/greenball.png`).
    pub green_ball: Option<RgbaImage>,
    /// `PlaceDirt`'s colours (`FrontTile.png`).
    pub front_tile: RgbaImage,
    pub back_tile: RgbaImage,
    pub warnings: Vec<String>,
}

impl LoadedTheme {
    /// The holes as the simulation's stamps.
    pub fn stamps(&self) -> Vec<Stamp> {
        self.holes.iter().map(Stamp::from_image).collect()
    }

    /// The green-dirt mask for [`rl_sim::World::set_green_mask`].
    pub fn green_mask(&self) -> Option<GreenMask> {
        self.green_ball.as_ref().map(GreenMask::from_image)
    }
}

/// A worm skin (`skin.image` is OLX's 672 × 18 sheet, already keyed).
#[derive(Clone, Debug)]
pub struct LoadedSkin {
    pub skin: Skin,
    pub warnings: Vec<String>,
}

/// OLX's skin sheet width (`CGameSkin`, 21 frames of 32 px).
const SHEET_WIDTH: u32 = 672;

impl LoadedSkin {
    /// A sheet of the placeholder colour, tinted with the player's colour.
    pub fn placeholder() -> LoadedSkin {
        let layout = FrameLayout::OLX;
        let image = placeholder_image(SHEET_WIDTH, layout.frame_height);
        let mask = vec![MaskKind::Colorize; (SHEET_WIDTH * layout.frame_height) as usize];
        LoadedSkin {
            skin: Skin { def: SkinDef { name: "placeholder".into(), layout }, image, mask },
            warnings: vec![],
        }
    }
}

/// The largest art scale a level may have (OLX's levels use 1 or 2).
pub const MAX_ART_SCALE: u32 = 8;

/// The largest colour image a level may need, in bytes (RGBA at art
/// scale): 256 MiB, about 12 times Castle Strike at 2×.
pub const MAX_COLOR_MAP_BYTES: u64 = 256 << 20;

/// Refuses art that would make the colour map (`rl_terrain::ColorMap`,
/// which keeps three images of this size) allocate absurd amounts: a
/// hostile pack must fail to load, not exhaust memory.
fn check_art_size(width: u32, height: u32, art_scale: u32) -> anyhow::Result<()> {
    if art_scale > MAX_ART_SCALE {
        anyhow::bail!("art scale {art_scale} is above {MAX_ART_SCALE}");
    }
    let s = art_scale as u128;
    let bytes = width as u128 * s * height as u128 * s * 4;
    if bytes > MAX_COLOR_MAP_BYTES as u128 {
        anyhow::bail!(
            "{width}x{height} at art scale {art_scale} needs a {bytes}-byte colour image, above {MAX_COLOR_MAP_BYTES}"
        );
    }
    Ok(())
}

impl Content {
    /// Level pack `id` (such as `olx-level-castlestrike`). Art above
    /// [`MAX_ART_SCALE`] or [`MAX_COLOR_MAP_BYTES`] is an error.
    pub fn load_level(&self, id: &str) -> anyhow::Result<LoadedLevel> {
        let pack = rl_pack::read_level(&self.pack(id, PackKind::Level)?)
            .with_context(|| format!("level pack {id}"))?;
        let l = pack.level;
        check_art_size(l.material.width(), l.material.height(), l.def.art_scale)
            .with_context(|| format!("level pack {id}"))?;
        Ok(LoadedLevel {
            name: l.def.name,
            material: l.material,
            front: l.front,
            back: l.back,
            art_scale: l.def.art_scale,
            theme: l.def.theme,
            spawns: l.def.spawns,
        })
    }

    /// Theme pack `id` (such as `olx-theme-dirt`). Fewer than five holes, or
    /// no green ball, is a warning: the simulation then carves or places
    /// nothing there, as OLX does.
    pub fn load_theme(&self, id: &str) -> anyhow::Result<LoadedTheme> {
        let pack = rl_pack::read_theme(&self.pack(id, PackKind::Theme)?)
            .with_context(|| format!("theme pack {id}"))?;
        let t = pack.theme;
        let mut warnings = Vec::new();
        if t.holes.len() < 5 {
            warnings.push(format!("{id}: {} holes, not 5: the larger ones carve nothing", t.holes.len()));
        }
        if t.green_ball.is_none() {
            warnings.push(format!("{id}: no green ball: green dirt places nothing"));
        }
        Ok(LoadedTheme {
            name: t.def.name,
            holes: t.holes,
            green_ball: t.green_ball,
            front_tile: t.front_tile,
            back_tile: t.back_tile,
            warnings,
        })
    }

    /// Skin pack `id` (such as `olx-skin-default`). A sheet that is not
    /// OLX's 672 × 18 in its layout is replaced by [`LoadedSkin::placeholder`].
    pub fn load_skin(&self, id: &str) -> anyhow::Result<LoadedSkin> {
        let skin = rl_pack::read_skin(&self.pack(id, PackKind::Skin)?)
            .with_context(|| format!("skin pack {id}"))?
            .skin;
        let (w, h) = (skin.image.width(), skin.image.height());
        let layout = FrameLayout::OLX;
        if skin.def.layout != layout || (w, h) != (SHEET_WIDTH, layout.frame_height) {
            let mut p = LoadedSkin::placeholder();
            p.warnings.push(format!(
                "{id}: a {w}x{h} sheet, not OLX's {SHEET_WIDTH}x{}: a placeholder instead",
                layout.frame_height
            ));
            return Ok(p);
        }
        Ok(LoadedSkin { skin, warnings: vec![] })
    }

    /// Skin `id`, else OLX's default skin (`olx-skin-default`), else the
    /// placeholder; never fails. Why it fell back is in its warnings.
    pub fn skin_or_default(&self, id: &str) -> LoadedSkin {
        let err = match self.load_skin(id) {
            Ok(s) => return s,
            Err(e) => format!("{e:#}"),
        };
        let mut s = self.load_skin("olx-skin-default").unwrap_or_else(|_| LoadedSkin::placeholder());
        s.warnings.insert(0, format!("{err}: using {} instead", s.skin.def.name));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_map_size_is_capped() {
        // Castle Strike at 2x is fine; a 2048 x 2048 level at 8x (1 GiB) is not.
        assert!(check_art_size(1350, 550, 2).is_ok());
        assert!(check_art_size(4096, 4096, 2).is_ok());
        assert!(check_art_size(2048, 2048, 8).is_err());
        assert!(check_art_size(u32::MAX, u32::MAX, MAX_ART_SCALE).is_err());
        assert!(check_art_size(4, 4, 9).is_err());
    }
}
