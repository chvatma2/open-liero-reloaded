//! The images entities draw with, and the theme's default dirt colour.

use rl_content::{CorePack, LoadedTheme};
use rl_data::image::{Rgba, RgbaImage};
use rl_scene::{AtlasId, SceneAssets};

/// Where [`FxAssets`] registers its images: [`SceneAssets`], or a test's list.
pub trait Atlas {
    /// Adds `image` (already colour-keyed) and returns its id.
    fn add_image(&mut self, name: String, image: RgbaImage, px_per_world: u32) -> AtlasId;
}

impl Atlas for SceneAssets {
    fn add_image(&mut self, name: String, image: RgbaImage, px_per_world: u32) -> AtlasId {
        SceneAssets::add_image(self, name, image, px_per_world)
    }
}

/// The core sprites entities use (OLX's `data/gfx`, 2× art).
#[derive(Clone, Debug)]
pub struct FxImages {
    pub explosion: RgbaImage,
    pub smoke: RgbaImage,
    pub chemsmoke: RgbaImage,
    pub spawn: RgbaImage,
    pub sparkle: RgbaImage,
    /// `giblets.png`, before each worm's colorization.
    pub giblets: RgbaImage,
}

impl FxImages {
    /// The core pack's sprites (placeholders of OLX's sizes where missing).
    pub fn from_core(core: &CorePack) -> FxImages {
        let get = |name: &str| core.sprite(name).clone();
        FxImages {
            explosion: get("explosion"),
            smoke: get("smoke"),
            chemsmoke: get("chemsmoke"),
            spawn: get("spawn"),
            sparkle: get("sparkle"),
            giblets: get("giblets"),
        }
    }
}

/// A sprite strip in the atlas and its size in image pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FxSprite {
    pub atlas: AtlasId,
    pub w: u32,
    pub h: u32,
}

/// One worm's gibs: the atlas image and the colorized sheet the map is
/// stained from.
#[derive(Clone, Debug)]
struct Gibs {
    atlas: AtlasId,
    image: RgbaImage,
}

/// The atlas ids entities draw with, each worm's gib image, and the theme's
/// default dirt colour. Sprites are OLX's 2× art (2 image pixels a world
/// pixel), colour-keyed: magenta is transparent.
#[derive(Clone, Debug)]
pub struct FxAssets {
    /// A 1 × 1 white image: tinted, a solid pixel block.
    pub white: AtlasId,
    pub explosion: FxSprite,
    pub smoke: FxSprite,
    pub chemsmoke: FxSprite,
    pub spawn: FxSprite,
    pub sparkle: FxSprite,
    giblets: RgbaImage,
    gibs: Vec<Gibs>,
    /// For a worm without gibs of its own: the sheet as it is.
    default_gibs: Gibs,
    dirt_default: Rgba,
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

impl FxAssets {
    /// Registers `images` in `atlas`. `dirt_default` is the theme's
    /// [`default_dirt_colour`].
    pub fn new(images: FxImages, dirt_default: Rgba, atlas: &mut impl Atlas) -> FxAssets {
        let mut white = RgbaImage::new(1, 1);
        white.set(0, 0, Rgba::rgb(255, 255, 255));
        let mut add = |name: &str, image: RgbaImage| {
            let (w, h) = (image.width(), image.height());
            FxSprite { atlas: atlas.add_image(format!("fx:{name}"), keyed(image), 2), w, h }
        };
        let white = add("white", white).atlas;
        let explosion = add("explosion", images.explosion);
        let smoke = add("smoke", images.smoke);
        let chemsmoke = add("chemsmoke", images.chemsmoke);
        let spawn = add("spawn", images.spawn);
        let sparkle = add("sparkle", images.sparkle);
        let default_gibs =
            Gibs { atlas: add("gibs", images.giblets.clone()).atlas, image: images.giblets.clone() };
        FxAssets {
            white,
            explosion,
            smoke,
            chemsmoke,
            spawn,
            sparkle,
            giblets: images.giblets,
            gibs: Vec::new(),
            default_gibs,
            dirt_default,
        }
    }

    /// Adds the next worm's gibs (worms by index in [`rl_sim::World::worms`],
    /// which is their id), colorized with `colour` (`CWorm::ChangeGraphics`).
    /// Returns the worm's index.
    pub fn add_worm(&mut self, colour: Rgba, atlas: &mut impl Atlas) -> usize {
        let i = self.gibs.len();
        let image = colorize_gibs(&self.giblets, colour);
        let id = atlas.add_image(format!("fx:gibs:{i}"), keyed(image.clone()), 2);
        self.gibs.push(Gibs { atlas: id, image });
        i
    }

    /// Worm `worm`'s gib atlas id and colorized sheet (magenta kept); the
    /// plain sheet for a worm without one.
    pub fn gibs(&self, worm: i32) -> (AtlasId, &RgbaImage) {
        let g = usize::try_from(worm).ok().and_then(|i| self.gibs.get(i)).unwrap_or(&self.default_gibs);
        (g.atlas, &g.image)
    }

    /// The theme's `iDefaultColour`.
    pub fn dirt_default(&self) -> Rgba {
        self.dirt_default
    }
}

/// `CWorm::ColorizeImage("data/gfx/giblets.png")`: every pixel but the
/// colour key and the three gun greys becomes
/// `min(255, colour * c / (96, 156, 252))` in floats, truncated, and a
/// result of exactly magenta becomes `(240, 0, 240)`. Alpha is kept.
///
/// OLX: src/common/CWorm.cpp:671-757
pub fn colorize_gibs(giblets: &RgbaImage, colour: Rgba) -> RgbaImage {
    const GUNS: [Rgba; 3] = [Rgba::rgb(216, 216, 216), Rgba::rgb(180, 180, 180), Rgba::rgb(144, 144, 144)];
    let mut out = giblets.clone();
    let tint = |c: u8, k: u8, d: f32| (k as f32 * (c as f32 / d)).min(255.0) as u8;
    for y in 0..out.height() {
        for x in 0..out.width() {
            let px = out.get(x, y);
            if px.same_rgb(Rgba::MAGENTA) || GUNS.iter().any(|g| px.same_rgb(*g)) {
                continue;
            }
            let mut c = Rgba {
                r: tint(px.r, colour.r, 96.0),
                g: tint(px.g, colour.g, 156.0),
                b: tint(px.b, colour.b, 252.0),
                a: px.a,
            };
            if c.same_rgb(Rgba::MAGENTA) {
                (c.r, c.b) = (240, 240);
            }
            out.set(x, y, c);
        }
    }
    out
}

/// The theme's `iDefaultColour`: `FrontTile`'s top-left pixel, replaced by
/// the first pixel neither black nor magenta of each row of `hole1` that
/// has one (the `break` leaves only the row's loop), so the last such row
/// wins. OLX gives it to an explosion's dirt particles when it found no
/// dirt colour.
///
/// OLX: src/common/CMap.cpp:338-352
pub fn default_dirt_colour(theme: &LoadedTheme) -> Rgba {
    let tile = &theme.front_tile;
    let mut colour = if tile.width() > 0 && tile.height() > 0 { tile.get(0, 0) } else { Rgba::BLACK };
    if let Some(hole) = theme.holes.first() {
        for y in 0..hole.height() {
            if let Some(px) = (0..hole.width())
                .map(|x| hole.get(x, y))
                .find(|p| !p.same_rgb(Rgba::BLACK) && !p.same_rgb(Rgba::MAGENTA))
            {
                colour = px;
            }
        }
    }
    Rgba { a: 255, ..colour }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> Rgba) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.set(x, y, f(x, y));
            }
        }
        img
    }

    #[test]
    fn default_dirt_colour_is_the_last_rim_row_or_the_tile() {
        let tile = image(2, 2, |_, _| Rgba::rgb(1, 2, 3));
        let hole = image(3, 3, |x, y| match (x, y) {
            (1, 0) => Rgba::rgb(10, 0, 0),
            (2, 1) => Rgba::rgb(20, 0, 0),
            (0, 1) => Rgba::rgb(30, 0, 0),
            (_, 2) => Rgba::MAGENTA,
            _ => Rgba::BLACK,
        });
        let mut theme = LoadedTheme {
            name: "t".into(),
            holes: vec![hole],
            green_ball: None,
            front_tile: tile,
            back_tile: RgbaImage::new(1, 1),
            warnings: Vec::new(),
        };
        assert_eq!(default_dirt_colour(&theme), Rgba::rgb(30, 0, 0), "row 1's first rim pixel");
        theme.holes[0] = image(2, 2, |_, _| Rgba::MAGENTA);
        assert_eq!(default_dirt_colour(&theme), Rgba::rgb(1, 2, 3));
        theme.holes.clear();
        theme.front_tile = RgbaImage::new(0, 0);
        assert_eq!(default_dirt_colour(&theme), Rgba::BLACK);
    }

    #[test]
    fn colorizing_keeps_the_key_and_gun_greys_and_avoids_magenta() {
        let src = image(4, 1, |x, _| {
            [Rgba::MAGENTA, Rgba::rgb(216, 216, 216), Rgba::rgb(96, 156, 252), Rgba::rgb(255, 0, 255)]
                [x as usize]
        });
        let out = colorize_gibs(&src, Rgba::rgb(255, 0, 255));
        assert_eq!(out.get(0, 0), Rgba::MAGENTA);
        assert_eq!(out.get(1, 0), Rgba::rgb(216, 216, 216));
        assert_eq!(out.get(2, 0), Rgba::rgb(240, 0, 240), "exactly magenta is nudged");
        let out = colorize_gibs(&src, Rgba::rgb(48, 78, 126));
        assert_eq!(out.get(2, 0), Rgba::rgb(48, 78, 126));
    }
}
