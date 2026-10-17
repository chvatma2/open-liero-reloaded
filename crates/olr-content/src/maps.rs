//! Levels, themes and skins.

use anyhow::Context as _;
use olr_data::image::RgbaImage;
use olr_data::level::{MaterialGrid, SpawnPoint};
use olr_data::manifest::PackKind;
use olr_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};
use olr_terrain::{GreenMask, Stamp};

use crate::Content;
use crate::placeholder::placeholder_image;

/// OLX's default skin pack, what a skin that cannot be matched falls back
/// to (olr-lobby's `DEFAULT_SKIN`).
pub const DEFAULT_SKIN: &str = "olx-skin-default";
/// The "content hash unknown" of a look (a bot's), restating olr-proto's
/// `NO_HASH` (olr-content is below olr-proto; a test in olr-game asserts
/// the two are equal).
pub const NO_SKIN_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

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
/// from. Images keep OLX's raw colours (see [`olr_data::theme::Theme`]).
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

    /// The green-dirt mask for [`olr_sim::World::set_green_mask`].
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
const SHEET_WIDTH: u32 = crate::SHEET_FRAMES * crate::FRAME_WIDTH;

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

/// The largest art scale a level may have (OLX's levels use 1 or 2):
/// `olr-pack`'s (`olr-format`'s), which the level reader enforces.
pub use olr_pack::MAX_ART_SCALE;

/// The largest colour image a level may need, in bytes (RGBA at art
/// scale): 256 MiB, about 12 times Castle Strike at 2×.
pub const MAX_COLOR_MAP_BYTES: u64 = 256 << 20;

/// Refuses (the reader has already bounded the art scale and the size) art that would make the colour map (`olr_terrain::ColorMap`,
/// which keeps three images of this size) allocate absurd amounts: a
/// hostile pack must fail to load, not exhaust memory.
fn check_art_size(width: u32, height: u32, art_scale: u32) -> anyhow::Result<()> {
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
    /// [`MAX_ART_SCALE`] (the reader refuses it) or [`MAX_COLOR_MAP_BYTES`] is
    /// an error.
    pub fn load_level(&self, id: &str) -> anyhow::Result<LoadedLevel> {
        let pack = olr_pack::read_level(&self.pack(id, PackKind::Level)?)
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
        let pack = olr_pack::read_theme(&self.pack(id, PackKind::Theme)?)
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

    /// Skin pack `id` (such as `olx-skin-default`). `read_skin` compiles
    /// every skin to OLX's 672 x 18 sheet, so there is no size to check.
    pub fn load_skin(&self, id: &str) -> anyhow::Result<LoadedSkin> {
        let skin = olr_pack::read_skin(&self.pack(id, PackKind::Skin)?)
            .with_context(|| format!("skin pack {id}"))?
            .skin;
        Ok(LoadedSkin { skin, warnings: vec![] })
    }

    /// Skin `id`, else OLX's default skin (`olx-skin-default`), else the
    /// placeholder; never fails. Why it fell back is in its warnings.
    pub fn skin_or_default(&self, id: &str) -> LoadedSkin {
        let err = match self.load_skin(id) {
            Ok(s) => return s,
            Err(e) => format!("{e:#}"),
        };
        let mut s = self.load_skin(DEFAULT_SKIN).unwrap_or_else(|_| LoadedSkin::placeholder());
        s.warnings.insert(0, format!("{err}: using {} instead", s.skin.def.name));
        s
    }

    /// The skin a match draws for a member's look (spec §4, plan 13c ruling
    /// 9): the pack `id` exactly (no name resolution) when it is a skin
    /// pack whose content hash is `hash`; with `hash == NO_SKIN_HASH` (a
    /// bot's) only [`DEFAULT_SKIN`]; otherwise the default skin, and the
    /// warning says why. A skin pack is never transferred, so one that is
    /// missing here or has other content is not the player's own skin.
    pub fn skin_matching(&self, id: &str, hash: &str) -> (LoadedSkin, Option<String>) {
        let is_skin = self.pack_kind(id) == Some(PackKind::Skin);
        let why = if !is_skin {
            Some(format!("{id}: no such skin pack here"))
        } else if hash == NO_SKIN_HASH {
            (id != DEFAULT_SKIN)
                .then(|| format!("{id}: its content is unknown (a bot's), not the default skin"))
        } else if !self.has(id, hash) {
            Some(format!("{id}: this skin has other content here than the player's"))
        } else {
            None
        };
        if why.is_none() {
            match self.load_skin(id) {
                Ok(s) => return (s, None),
                Err(e) => {
                    return (
                        self.skin_or_default(DEFAULT_SKIN),
                        Some(format!("{e:#}: using the default skin")),
                    );
                }
            }
        }
        (self.skin_or_default(DEFAULT_SKIN), why.map(|w| format!("{w}: using the default skin")))
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
    }
}
