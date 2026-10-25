//! Levels, themes and skins.

use anyhow::Context as _;
use olr_data::image::RgbaImage;
use olr_data::level::{Level, Material, MaterialGrid, SpawnPoint};
use olr_data::manifest::PackKind;
use olr_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};
use olr_format::levels::LevelSource;
use olr_terrain::{GreenMask, Stamp};

use crate::Content;
use crate::placeholder::placeholder_image;

/// OLX's default skin pack, what a skin that cannot be matched falls back
/// to (olr-lobby's `DEFAULT_SKIN`).
pub const DEFAULT_SKIN: &str = olr_replay::DEFAULT_SKIN;
/// The "content hash unknown" of a look (a bot's), restating olr-proto's
/// `NO_HASH` (olr-content is below olr-proto; a test in olr-game asserts
/// the two are equal).
pub const NO_SKIN_HASH: &str = olr_replay::NO_SKIN_HASH;

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
    /// The level's look (look system); `None` draws as always.
    pub look: Option<olr_look::LevelLook>,
    /// The level's glow images.
    pub glow: Option<olr_terrain::GlowLayers>,
    /// The level's clear flags per world pixel (`look/clear.png`).
    pub clear: Option<Vec<u8>>,
    /// The level's glass map (`look/glass.png`, the background glass design).
    pub glass: Option<olr_terrain::GlassMap>,
    /// The level's glow beams (`look/glow-beam.png`, the directional-glow
    /// design).
    pub glow_beams: Option<olr_terrain::GlowBeamMap>,
    /// The level's haze map (`look/haze.png`, the directional-glow design):
    /// grey per world pixel.
    pub haze: Option<Vec<u8>>,
    /// The level's distant (parallax) layers, far to near (issue #11):
    /// its back art's alpha-0 pixels are see-through when it has any.
    pub distant: Vec<olr_data::level::DistantLayer>,
    /// The level's surface maps (issue #37), each world pixel's value in
    /// the look's `materials`, for the renderer's heat (issue #39); `None`
    /// for a level without materials.
    pub surface: Option<olr_look::SurfaceMaps>,
    /// Each distant layer's surface material as a value in the look's
    /// `materials` (issue #41, the renderer heats a layer by it), 0 none;
    /// past its end, none.
    pub distant_surface: Vec<u8>,
    /// What loading left out of a look that did not check, to log once.
    pub warnings: Vec<String>,
}

impl LoadedLevel {
    /// An edited level (the editor's `LevelSource`, compiled as a level
    /// pack's files are) as a match loads it, under the art bound
    /// [`Content::load_level`] applies.
    pub fn from_source(src: &LevelSource) -> anyhow::Result<LoadedLevel> {
        let look = olr_format::levels::compile_look(src)?;
        let extras = Extras {
            look,
            glow: src.glow.clone(),
            clear: src.clear.clone(),
            glass: src.glass.clone(),
            glow_beams: src.glow_beams.clone(),
            haze: src.haze.clone(),
            distant: src.distant.clone(),
            surface: src.surface.clone(),
            distant_surface: olr_format::levels::distant_surface_values(src),
        };
        LoadedLevel::from_level(olr_format::levels::compile_level(src)?, extras, Vec::new())
    }

    fn from_level(l: Level, extras: Extras, warnings: Vec<String>) -> anyhow::Result<LoadedLevel> {
        let Extras { look, glow, clear, glass, glow_beams, haze, distant, surface, distant_surface } = extras;
        check_art_size(l.material.width(), l.material.height(), l.def.art_scale)?;
        Ok(LoadedLevel {
            name: l.def.name,
            material: l.material,
            front: l.front,
            back: l.back,
            art_scale: l.def.art_scale,
            theme: l.def.theme,
            spawns: l.def.spawns,
            look,
            glow,
            clear,
            glass,
            glow_beams,
            haze,
            distant,
            surface,
            distant_surface,
            warnings,
        })
    }

    /// The level has a look, glows or has clear solids: the renderer's
    /// look passes draw it.
    pub fn has_look(&self) -> bool {
        self.look.is_some() || self.glow.is_some() || self.clear.is_some()
    }

    /// The first spawn point when it is a good spawn point, else `None` (a
    /// free spot): OLX's `CMap::IsGoodSpawnPoint` (`src/game/CMap.h:458`,
    /// as `olr-sim`'s `FindSpot` checks), no rock in the 6 × 6 square from
    /// `(x - 3, y - 3)` to `(x + 2, y + 2)`, outside the level counting as
    /// rock. A worm placed elsewhere would start stuck or off the map.
    pub fn first_spawn(&self) -> Option<(i32, i32)> {
        let s = self.spawns.first()?;
        let (w, h) = (i64::from(self.material.width()), i64::from(self.material.height()));
        let (x, y) = (i64::from(s.x), i64::from(s.y));
        let free = |xx: i64, yy: i64| {
            (0..w).contains(&xx)
                && (0..h).contains(&yy)
                && self.material.get(xx as u32, yy as u32) != Material::Rock
        };
        ((y - 3)..(y + 3)).all(|yy| ((x - 3)..(x + 3)).all(|xx| free(xx, yy))).then_some((s.x, s.y))
    }
}

/// What a level has besides its material and art.
struct Extras {
    look: Option<olr_look::LevelLook>,
    glow: Option<olr_terrain::GlowLayers>,
    clear: Option<Vec<u8>>,
    glass: Option<olr_terrain::GlassMap>,
    glow_beams: Option<olr_terrain::GlowBeamMap>,
    haze: Option<Vec<u8>>,
    distant: Vec<olr_data::level::DistantLayer>,
    surface: Option<olr_look::SurfaceMaps>,
    distant_surface: Vec<u8>,
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

/// A worm skin (`skin.image` is OLX's 672 × 18 sheet or, for a
/// double-resolution skin, its 1344 × 36 double; already keyed).
#[derive(Clone, Debug)]
pub struct LoadedSkin {
    pub skin: Skin,
    pub warnings: Vec<String>,
}

impl LoadedSkin {
    /// An OLX-layout sheet (`CGameSkin`'s 21 frames of 32 px) of the
    /// placeholder colour, tinted with the player's colour.
    pub fn placeholder() -> LoadedSkin {
        let layout = FrameLayout::OLX;
        let (w, h) = layout.sheet_size();
        let image = placeholder_image(w, h);
        let mask = vec![MaskKind::Colorize; (w * h) as usize];
        let def = SkinDef { name: "placeholder".into(), layout, light: None };
        LoadedSkin { skin: Skin { def, image, mask, glow: vec![] }, warnings: vec![] }
    }
}

/// The largest art scale a level may have (OLX's levels use 1 or 2):
/// `olr-pack`'s (`olr-format`'s), which the level reader enforces.
pub use olr_pack::MAX_ART_SCALE;

/// The largest colour image a level may need, in bytes (RGBA at art
/// scale): 256 MiB, about 12 times Castle Strike at 2×.
pub const MAX_COLOR_MAP_BYTES: u64 = 256 << 20;

/// Refuses (the reader has already bounded the art scale and the size)
/// art that would make the colour map (`olr_terrain::ColorMap`, which
/// keeps three images of this size) allocate absurd amounts: a hostile
/// pack must fail to load, not exhaust memory.
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
        let warnings = pack.warnings.iter().map(|w| format!("{id}: {w}")).collect();
        let extras = Extras {
            look: pack.look,
            glow: pack.glow,
            clear: pack.clear,
            glass: pack.glass,
            glow_beams: pack.glow_beams,
            haze: pack.haze,
            distant: pack.distant,
            surface: pack.surface,
            distant_surface: pack.distant_surface,
        };
        LoadedLevel::from_level(pack.level, extras, warnings).with_context(|| format!("level pack {id}"))
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
    /// every skin to OLX's 672 x 18 sheet or its 1344 x 36 double, so there
    /// is no size to check.
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
