//! The images a scene draws from, decoded on the CPU: what `rl-render`
//! uploads into its atlas, and what [`crate::DrawList`] items name by
//! [`AtlasId`].

use std::collections::BTreeMap;

use rl_content::{CorePack, LoadedMod, LoadedSkin, placeholder_image};
use rl_data::image::{Rgba, RgbaImage};
use rl_data::mod_def::ProjectileGfx;

use crate::draw_list::AtlasId;

/// One atlas image. `px_per_world` is how many of its pixels make one world
/// pixel: 2 for OLX's 2× art (skins, projectile images, most core
/// sprites), 1 for the muzzle flash, which OLX stretches 2×.
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasImage {
    pub name: String,
    pub image: RgbaImage,
    pub px_per_world: u32,
}

/// How one worm is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WormLook {
    /// The colorized skin sheet (OLX's 21 frames of 32 × 18).
    pub skin: AtlasId,
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
/// worm's colorized skin. Images are colour-keyed at load (OLX's magenta
/// becomes transparent), so drawing is plain alpha blending.
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
    default_look: WormLook,
}

/// OLX's colour key, made transparent.
fn keyed(mut img: RgbaImage) -> RgbaImage {
    for y in 0..img.height() {
        for x in 0..img.width() {
            if img.get(x, y).same_rgb(Rgba::MAGENTA) {
                img.set(x, y, Rgba::TRANSPARENT);
            }
        }
    }
    img
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
        let mut sprite = |name: &str, px_per_world| add(name, keyed(core.sprite(name).clone()), px_per_world);
        let (crosshair, muzzle, hook) = (sprite("crosshair", 2), sprite("muzzle", 1), sprite("hook", 2));
        let (bonus, health) = (sprite("bonus", 2), sprite("health", 2));
        let skin =
            add("skin:placeholder", LoadedSkin::placeholder().skin.colorize(Rgba::rgb(255, 255, 255)), 2);
        let mut by_file: BTreeMap<&str, AtlasId> = BTreeMap::new();
        let mut projectiles = Vec::with_capacity(m.sim.projectiles().len());
        for p in m.sim.projectiles() {
            let ProjectileGfx::Image { file, .. } = &p.def.gfx else {
                projectiles.push(None);
                continue;
            };
            let id = *by_file.entry(file).or_insert_with(|| {
                let image = m.images.get(file).cloned().unwrap_or_else(|| placeholder_image(4, 4));
                add(&format!("mod:{file}"), keyed(image), 2)
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
            default_look: WormLook { skin, local: false, human: false },
        }
    }

    /// Adds the next worm's look (worms are drawn by index in
    /// [`rl_sim::World::worms`]): `skin` colorized with `colour`
    /// (`CGameSkin::Colorize`). Returns the worm's index.
    pub fn add_worm(&mut self, skin: &LoadedSkin, colour: Rgba, local: bool, human: bool) -> usize {
        let i = self.worms.len();
        let id = self.add_image(format!("skin:{i}"), skin.skin.colorize(colour), 2);
        self.worms.push(WormLook { skin: id, local, human });
        i
    }

    /// Adds an image (already colour-keyed) and returns its id: for other
    /// drawers such as `rl-fx`.
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

    /// The image of projectile type `proj` (a [`rl_sim::SimMod::projectiles`]
    /// index); `None` for pixel projectiles.
    pub fn projectile_image(&self, proj: usize) -> Option<AtlasId> {
        self.projectiles.get(proj).copied().flatten()
    }

    /// Worm `i`'s look; a worm without one gets a placeholder skin.
    pub fn worm(&self, i: usize) -> WormLook {
        self.worms.get(i).copied().unwrap_or(self.default_look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_content::PLACEHOLDER;

    #[test]
    fn keying_clears_only_magenta() {
        let mut img = RgbaImage::new(2, 1);
        img.set(0, 0, Rgba::MAGENTA);
        img.set(1, 0, PLACEHOLDER);
        let img = keyed(img);
        assert_eq!(img.get(0, 0), Rgba::TRANSPARENT);
        assert_eq!(img.get(1, 0), PLACEHOLDER);
    }
}
