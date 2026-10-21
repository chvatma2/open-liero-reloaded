//! The images a scene draws from, decoded on the CPU: what `olr-render`
//! uploads into its atlas, and what [`crate::DrawList`] items name by
//! [`AtlasId`].

use std::collections::BTreeMap;

use olr_content::{
    CorePack, GLOW_LEVELS, LoadedMod, LoadedSkin, SkinFrameLight, SkinLights, placeholder_image,
};
use olr_data::image::{Rgba, RgbaImage};
use olr_data::mod_def::ProjectileGfx;
use olr_data::skin::FrameLayout;

use crate::draw_list::AtlasId;

/// One atlas image. `px_per_world` is how many of its pixels make one world
/// pixel: 2 for OLX's 2× art (skins, projectile images, most core
/// sprites), 1 for the muzzle flash, which OLX stretches 2×, 4 for a
/// double-resolution skin (issue #12).
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasImage {
    pub name: String,
    pub image: RgbaImage,
    pub px_per_world: u32,
}

/// How one worm is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WormLook {
    /// The colorized skin sheet (21 frames of `layout`).
    pub skin: AtlasId,
    /// The sheet's frame layout: OLX's, or double resolution's (issue #12).
    pub layout: FrameLayout,
    /// The sheet's glowing pixels per strength (issue #12): `glow[k]` holds
    /// those of strength `k + 1` quarters, drawn over the body with that
    /// self-light in a lit match; `None` where none glow.
    pub glow: [Option<AtlasId>; GLOW_LEVELS],
    /// Played on this machine: OLX draws its crosshair.
    pub local: bool,
    /// A human player: OLX never draws a local human's health bar.
    pub human: bool,
}

/// OLX's colours for lines (`data/frontend/colours.cfg`).
const ROPE_COLOURS: [(&str, Rgba); 2] =
    [("RopeColour1", Rgba::rgb(0xa0, 0x50, 0)), ("RopeColour2", Rgba::rgb(0xc8, 0x64, 0))];
const LASER_COLOURS: [(&str, Rgba); 2] =
    [("LaserSightColour1", Rgba::rgb(0xbe, 0, 0)), ("LaserSightColour2", Rgba::rgb(0xa0, 0, 0))];

/// The scene's images: core sprites, the mod's projectile images and each
/// worm's colorized skin. The packs are colour-keyed at import (OLX's
/// magenta is already transparent in them), so nothing keys at load and
/// drawing is plain alpha blending.
#[derive(Clone, Debug)]
pub struct SceneAssets {
    images: Vec<AtlasImage>,
    /// A 1 × 1 white image: tinted, a solid pixel block.
    pub white: AtlasId,
    pub crosshair: AtlasId,
    pub muzzle: AtlasId,
    pub hook: AtlasId,
    /// The weapon bonus crate.
    pub bonus: AtlasId,
    /// The health bonus.
    pub health: AtlasId,
    /// `RopeColour1`, `RopeColour2`.
    pub rope_colours: [Rgba; 2],
    /// `LaserSightColour1`, `LaserSightColour2`.
    pub laser_colours: [Rgba; 2],
    projectiles: Vec<Option<AtlasId>>,
    worms: Vec<WormLook>,
    /// Each worm's skin light (issue #12), by worm index.
    worm_lights: Vec<Option<SkinLights>>,
    default_look: WormLook,
}

impl SceneAssets {
    /// The core sprites and `m`'s projectile images. A projectile image the
    /// mod lacks gets a one-frame placeholder, so nothing ever panics.
    pub fn new(core: &CorePack, m: &LoadedMod) -> SceneAssets {
        let mut images = Vec::new();
        let mut add = |name: &str, image: RgbaImage, px_per_world: u32| {
            images.push(AtlasImage { name: name.into(), image, px_per_world });
            AtlasId(images.len() as u32 - 1)
        };
        let mut white_img = RgbaImage::new(1, 1);
        white_img.set(0, 0, Rgba::rgb(255, 255, 255));
        let white = add("white", white_img, 1);
        let mut sprite = |name: &str, px_per_world| add(name, core.sprite(name).clone(), px_per_world);
        let (crosshair, muzzle, hook) = (sprite("crosshair", 2), sprite("muzzle", 1), sprite("hook", 2));
        let (bonus, health) = (sprite("bonus", 2), sprite("health", 2));
        let skin = add("skin:placeholder", LoadedSkin::placeholder().colorized(Rgba::rgb(255, 255, 255)), 2);
        let mut by_file: BTreeMap<&str, AtlasId> = BTreeMap::new();
        let mut projectiles = Vec::with_capacity(m.sim.projectiles().len());
        for p in m.sim.projectiles() {
            let ProjectileGfx::Image { file, .. } = &p.def.gfx else {
                projectiles.push(None);
                continue;
            };
            let id = *by_file.entry(file).or_insert_with(|| {
                let image = m.images.get(file).cloned().unwrap_or_else(|| placeholder_image(4, 4));
                add(&format!("mod:{file}"), image, 2)
            });
            projectiles.push(Some(id));
        }
        let colours = |keys: [(&str, Rgba); 2]| keys.map(|(k, d)| core.colour(k).unwrap_or(d));
        SceneAssets {
            images,
            white,
            crosshair,
            muzzle,
            hook,
            bonus,
            health,
            rope_colours: colours(ROPE_COLOURS),
            laser_colours: colours(LASER_COLOURS),
            projectiles,
            worms: Vec::new(),
            worm_lights: Vec::new(),
            default_look: WormLook {
                skin,
                layout: FrameLayout::OLX,
                glow: [None; GLOW_LEVELS],
                local: false,
                human: false,
            },
        }
    }

    /// Adds the next worm's look (worms are drawn by index in
    /// [`olr_sim::World::worms`]): `skin` colorized with `colour`
    /// (`LoadedSkin::colorized`, which the profiles' preview draws from
    /// too) at its art scale, its glowing pixels per strength and its light
    /// per frame (issue #12). Returns the worm's index.
    pub fn add_worm(&mut self, skin: &LoadedSkin, colour: Rgba, local: bool, human: bool) -> usize {
        let i = self.worms.len();
        let scale = skin.art_scale();
        let id = self.add_image(format!("skin:{i}"), skin.colorized(colour), scale);
        let mut glow = [None; GLOW_LEVELS];
        for (level, image) in skin.glow_overlays(colour) {
            if let Some(slot) = level.checked_sub(1).and_then(|k| glow.get_mut(k)) {
                *slot = Some(self.add_image(format!("skin:{i}:glow{level}"), image, scale));
            }
        }
        self.worms.push(WormLook { skin: id, layout: skin.layout(), glow, local, human });
        self.worm_lights.push(skin.frame_lights(colour));
        i
    }

    /// Worm `i`'s skin light in sheet frame `frame` and its reach (issue
    /// #12): `None` for a skin without a light, a frame without glow, or
    /// an unknown worm.
    pub fn worm_light(&self, i: usize, frame: u32) -> Option<(SkinFrameLight, u16)> {
        let l = self.worm_lights.get(i)?.as_ref()?;
        Some((*l.frames.get(frame as usize)?.as_ref()?, l.reach))
    }

    /// Adds an image (already carrying alpha) and returns its id: for other
    /// drawers such as `olr-fx`.
    pub fn add_image(&mut self, name: impl Into<String>, image: RgbaImage, px_per_world: u32) -> AtlasId {
        self.images.push(AtlasImage { name: name.into(), image, px_per_world: px_per_world.max(1) });
        AtlasId(self.images.len() as u32 - 1)
    }

    /// Every image, indexed by [`AtlasId`].
    pub fn images(&self) -> &[AtlasImage] {
        &self.images
    }

    /// Image `id`; an unknown id is the white pixel, never a panic.
    pub fn image(&self, id: AtlasId) -> &AtlasImage {
        self.images.get(id.0 as usize).unwrap_or(&self.images[self.white.0 as usize])
    }

    /// The image of projectile type `proj` (a [`olr_sim::SimMod::projectiles`]
    /// index); `None` for pixel projectiles.
    pub fn projectile_image(&self, proj: usize) -> Option<AtlasId> {
        self.projectiles.get(proj).copied().flatten()
    }

    /// Worm `i`'s look; a worm without one gets a placeholder skin.
    pub fn worm(&self, i: usize) -> WormLook {
        self.worms.get(i).copied().unwrap_or(self.default_look)
    }
}
