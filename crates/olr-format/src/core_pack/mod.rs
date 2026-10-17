//! The core pack's authoring format: `core.ron`, `gfx/<id>.png` (RGBA with
//! alpha), `sounds/<file>` and OLX's colours. Compiled to the runtime
//! `CoreDef` and sprites, and lifted from them
//! (docs/porting/olx-notes/asset-formats.md §5). Not named `core`: a
//! crate-root module of that name would shadow the `core` crate.

mod files;
pub(crate) use files::CoreFile;

use std::collections::BTreeMap;

use olr_data::core_pack::CoreDef;
use olr_data::image::{Rgba, RgbaImage};

pub use files::{read_core_files, read_core_files_tolerant, write_core_files};

use crate::image_io::{IMAGES, check_size};
use crate::{Colour, FormatError, LiftError};

/// The largest `art_scale`.
pub const MAX_ART_SCALE: u32 = 8;
/// The most sprites, the most sounds and the most colours.
pub const MAX_SPRITES: usize = 256;
pub const MAX_SOUNDS: usize = 1024;
pub const MAX_COLOURS: usize = 1024;

pub(crate) const CORE_FILE: &str = "core.ron";

/// Frames (columns, rows) and art scale of OLX's core sprites
/// (`docs/porting/olx-notes/asset-formats.md` §5.2). `giblets` is the
/// 32 × 8 half of OLX's image and `giblet-stains` the 16 × 4 rest.
pub const CORE_LAYOUT: [(&str, (u32, u32), u32); 12] = [
    ("bonus", (1, 1), 2),
    ("chemsmoke", (4, 1), 2),
    ("crosshair", (2, 1), 2),
    ("explosion", (16, 1), 2),
    ("giblet-stains", (4, 1), 1),
    ("giblets", (4, 1), 2),
    ("health", (1, 1), 2),
    ("hook", (1, 1), 2),
    ("muzzle", (14, 1), 1),
    ("smoke", (5, 1), 2),
    ("sparkle", (3, 1), 2),
    ("spawn", (5, 1), 2),
];

pub(crate) const GIBLETS: &str = "giblets";
pub(crate) const STAINS: &str = "giblet-stains";
/// OLX's runtime `giblets` image, and its two parts.
const GIBLETS_SIZE: (u32, u32) = (32, 12);
pub(crate) const GIBLET_PART: (u32, u32) = (32, 8);
pub(crate) const STAINS_PART: (u32, u32) = (16, 4);

/// A sprite as its file holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteSource {
    /// With alpha.
    pub image: RgbaImage,
    /// Columns, rows; must divide the image.
    pub frames: (u32, u32),
    /// 1 to [`MAX_ART_SCALE`].
    pub art_scale: u32,
}

/// A core pack as its files hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct CoreSource {
    pub notes: String,
    pub sprites: BTreeMap<String, SpriteSource>,
    /// File name (`throw.ogg`) to bytes.
    pub sounds: BTreeMap<String, Vec<u8>>,
    /// OLX's keys, such as `RopeColour1`.
    pub colours: BTreeMap<String, Colour>,
}

pub(crate) fn sprite_file(id: &str) -> String {
    format!("gfx/{id}.png")
}

pub(crate) fn sound_file(name: &str) -> String {
    format!("sounds/{name}")
}

/// A sprite id or sound file name: one lowercase file name.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
}

/// One sprite's checks: its id, art scale, size and frame grid.
pub(crate) fn check_sprite(id: &str, s: &SpriteSource) -> Result<(), FormatError> {
    if !valid_name(id) {
        return Err(FormatError::invalid(CORE_FILE, "sprites", format!("invalid sprite id {id:?}")));
    }
    let file = sprite_file(id);
    if !(1..=MAX_ART_SCALE).contains(&s.art_scale) {
        return Err(FormatError::invalid(
            &file,
            "art_scale",
            format!("{}, not within 1 to {MAX_ART_SCALE}", s.art_scale),
        ));
    }
    check_size(s.image.width(), s.image.height(), IMAGES).map_err(|e| FormatError::invalid(&file, "", e))?;
    let ((c, r), (w, h)) = (s.frames, (s.image.width(), s.image.height()));
    if c == 0 || r == 0 || w % c != 0 || h % r != 0 {
        return Err(FormatError::invalid(
            &file,
            "frames",
            format!("{c}x{r} frames do not divide the {w}x{h} image"),
        ));
    }
    Ok(())
}

/// The checks the compiler, the writer and the reader share.
pub(crate) fn check(src: &CoreSource) -> Result<(), FormatError> {
    if src.sprites.len() > MAX_SPRITES {
        return Err(FormatError::invalid(CORE_FILE, "sprites", format!("more than {MAX_SPRITES} sprites")));
    }
    if src.sounds.len() > MAX_SOUNDS {
        return Err(FormatError::invalid(CORE_FILE, "sounds", format!("more than {MAX_SOUNDS} sounds")));
    }
    if src.colours.len() > MAX_COLOURS {
        return Err(FormatError::invalid(CORE_FILE, "colours", format!("more than {MAX_COLOURS} colours")));
    }
    for (id, s) in &src.sprites {
        check_sprite(id, s)?;
    }
    for name in src.sounds.keys() {
        if !valid_name(name) {
            return Err(FormatError::invalid(CORE_FILE, "sounds", format!("invalid sound name {name:?}")));
        }
    }
    for key in src.colours.keys() {
        if key.is_empty() {
            return Err(FormatError::invalid(CORE_FILE, "colours", "an empty colour key"));
        }
    }
    Ok(())
}

pub(crate) fn size_is(img: &RgbaImage, (w, h): (u32, u32)) -> bool {
    (img.width(), img.height()) == (w, h)
}

/// Makes the runtime core pack: `giblets` and `giblet-stains` become
/// OLX's one 32 × 12 `giblets`.
#[allow(clippy::type_complexity)]
pub fn compile_core(
    src: &CoreSource,
) -> Result<(CoreDef, BTreeMap<String, RgbaImage>, BTreeMap<String, Vec<u8>>), FormatError> {
    check(src)?;
    let mut sprites = BTreeMap::new();
    for (id, s) in &src.sprites {
        if id != STAINS {
            sprites.insert(id.clone(), s.image.clone());
        }
    }
    match (src.sprites.get(GIBLETS), src.sprites.get(STAINS)) {
        (None, None) => {}
        (Some(g), Some(st)) => {
            for (id, s, want) in [(GIBLETS, g, GIBLET_PART), (STAINS, st, STAINS_PART)] {
                if !size_is(&s.image, want) {
                    return Err(FormatError::invalid(
                        &sprite_file(id),
                        "",
                        format!(
                            "{}x{}, not the {}x{} OLX's gibs need",
                            s.image.width(),
                            s.image.height(),
                            want.0,
                            want.1
                        ),
                    ));
                }
            }
            let mut out = RgbaImage::new(GIBLETS_SIZE.0, GIBLETS_SIZE.1);
            for (img, y0) in [(&g.image, 0), (&st.image, GIBLET_PART.1)] {
                for y in 0..img.height() {
                    for x in 0..img.width() {
                        out.set(x, y0 + y, img.get(x, y));
                    }
                }
            }
            sprites.insert(GIBLETS.into(), out);
        }
        (Some(_), None) => {
            return Err(FormatError::invalid(
                &sprite_file(GIBLETS),
                "",
                format!("{GIBLETS} needs {STAINS} beside it (OLX draws them from one image)"),
            ));
        }
        (None, Some(_)) => {
            return Err(FormatError::invalid(
                &sprite_file(STAINS),
                "",
                format!("{STAINS} needs {GIBLETS} beside it (OLX draws them from one image)"),
            ));
        }
    }
    let def = CoreDef {
        sprites: sprites.keys().cloned().collect(),
        sounds: src.sounds.keys().cloned().collect(),
        colours: src.colours.iter().map(|(k, c)| (k.clone(), c.0)).collect(),
    };
    Ok((def, sprites, src.sounds.clone()))
}

/// The inverse of [`compile_core`]; `notes` stay empty. Purely structural:
/// no colour keying. The sprites must already carry alpha, so the importer
/// keys the old sprites once (`image_io::olx_sprite_to_alpha`, plan 13c
/// Ruling 3) before lifting; `read_core_files(write_core_files(p)) == p`.
pub fn lift_core(
    def: &CoreDef,
    sprites: &BTreeMap<String, RgbaImage>,
    sounds: &BTreeMap<String, Vec<u8>>,
) -> Result<CoreSource, LiftError> {
    let mut out = BTreeMap::new();
    let layout = |id: &str| {
        CORE_LAYOUT.iter().find(|(n, ..)| *n == id).map_or(((1, 1), 2), |&(_, frames, scale)| (frames, scale))
    };
    let mut add = |id: &str, image: RgbaImage| {
        let (frames, art_scale) = layout(id);
        out.insert(id.to_string(), SpriteSource { image, frames, art_scale });
    };
    for (id, img) in sprites {
        let fail = |message: String| LiftError { item: format!("sprite \"{id}\""), message };
        let img = img.clone();
        if id == STAINS {
            return Err(fail(format!("{STAINS} is not a runtime sprite")));
        }
        if id != GIBLETS {
            add(id, img);
            continue;
        }
        if !size_is(&img, GIBLETS_SIZE) {
            return Err(fail(format!("{}x{}, not 32x12", img.width(), img.height())));
        }
        for y in GIBLET_PART.1..GIBLETS_SIZE.1 {
            for x in STAINS_PART.0..GIBLETS_SIZE.0 {
                if img.get(x, y).a != 0 {
                    return Err(fail(format!("a pixel at ({x}, {y}), outside the gibs and the stains")));
                }
            }
        }
        add(GIBLETS, img.crop(0, 0, GIBLET_PART.0, GIBLET_PART.1));
        add(STAINS, img.crop(0, GIBLET_PART.1, STAINS_PART.0, STAINS_PART.1));
    }
    Ok(CoreSource {
        notes: String::new(),
        sprites: out,
        sounds: sounds.clone(),
        colours: def.colours.iter().map(|(k, c): (&String, &Rgba)| (k.clone(), Colour(*c))).collect(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn filled(w: u32, h: u32, seed: u8) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                if (x + y) % 3 != 0 {
                    img.set(x, y, Rgba { r: seed, g: x as u8 + 1, b: y as u8 + 1, a: 255 });
                }
            }
        }
        img.set(1, 0, Rgba { r: seed, g: 5, b: 6, a: 90 });
        img
    }

    fn sprite(id: &str, seed: u8) -> SpriteSource {
        let (_, frames, art_scale) = *CORE_LAYOUT.iter().find(|(n, ..)| *n == id).unwrap();
        let (w, h) = match id {
            GIBLETS => GIBLET_PART,
            STAINS => STAINS_PART,
            _ => (frames.0 * 3, frames.1 * 2),
        };
        SpriteSource { image: filled(w, h, seed), frames, art_scale }
    }

    pub(crate) fn sample() -> CoreSource {
        let mut sprites = BTreeMap::new();
        for (i, id) in ["crosshair", "giblets", "giblet-stains", "spawn"].into_iter().enumerate() {
            sprites.insert(id.to_string(), sprite(id, i as u8 + 1));
        }
        sprites.insert("extra".into(), SpriteSource { image: filled(5, 4, 9), frames: (1, 1), art_scale: 2 });
        let mut sounds = BTreeMap::new();
        sounds.insert("bump.ogg".to_string(), vec![1, 2, 3]);
        sounds.insert("throw.ogg".to_string(), vec![4, 5]);
        let mut colours = BTreeMap::new();
        colours.insert("RopeColour1".to_string(), Colour(Rgba::rgb(0x94, 0x88, 0x00)));
        CoreSource { notes: String::new(), sprites, sounds, colours }
    }

    #[test]
    fn a_synthetic_core_round_trips_with_giblets_composed_and_split() {
        let src = sample();
        let (def, sprites, sounds) = compile_core(&src).unwrap();
        assert_eq!(def.sprites, ["crosshair", "extra", "giblets", "spawn"]);
        assert_eq!(def.sounds, ["bump.ogg", "throw.ogg"]);
        let g = &sprites["giblets"];
        assert_eq!((g.width(), g.height()), (32, 12));
        assert_eq!(g.crop(0, 0, 32, 8), src.sprites["giblets"].image);
        assert_eq!(g.crop(0, 8, 16, 4), src.sprites["giblet-stains"].image);
        assert_eq!(g.crop(16, 8, 16, 4), RgbaImage::new(16, 4));
        assert_eq!(sounds, src.sounds);
        assert_eq!(lift_core(&def, &sprites, &sounds).unwrap(), src);
    }

    #[test]
    fn frames_that_do_not_divide_the_image_name_the_file() {
        let mut src = sample();
        let s = src.sprites.get_mut("crosshair").unwrap();
        s.image = RgbaImage::new(10, 2);
        s.frames = (3, 1);
        let e = compile_core(&src).unwrap_err().to_string();
        assert!(e.contains("gfx/crosshair.png") && e.contains("3x1"), "{e}");
    }

    #[test]
    fn an_art_scale_out_of_range_names_the_file() {
        let mut src = sample();
        src.sprites.get_mut("spawn").unwrap().art_scale = 9;
        let e = compile_core(&src).unwrap_err().to_string();
        assert!(e.contains("gfx/spawn.png") && e.contains("art_scale"), "{e}");
    }

    #[test]
    fn giblets_without_stains_is_an_error_and_so_is_the_reverse() {
        let mut src = sample();
        src.sprites.remove("giblet-stains");
        let e = compile_core(&src).unwrap_err().to_string();
        assert!(e.contains("gfx/giblets.png") && e.contains("giblet-stains"), "{e}");
        let mut src = sample();
        src.sprites.remove("giblets");
        assert!(compile_core(&src).is_err());
    }

    #[test]
    fn giblets_of_other_sizes_are_an_error() {
        let mut src = sample();
        let s = src.sprites.get_mut("giblets").unwrap();
        s.image = RgbaImage::new(32, 12);
        s.frames = (4, 1);
        let e = compile_core(&src).unwrap_err().to_string();
        assert!(e.contains("gfx/giblets.png") && e.contains("32x12"), "{e}");
    }

    #[test]
    fn a_colour_key_round_trips() {
        let mut src = sample();
        src.colours.insert("Odd".into(), Colour(Rgba { r: 1, g: 2, b: 3, a: 4 }));
        let (def, sprites, sounds) = compile_core(&src).unwrap();
        assert_eq!(def.colours["RopeColour1"], Rgba::rgb(0x94, 0x88, 0x00));
        assert_eq!(def.colours["Odd"], Rgba { r: 1, g: 2, b: 3, a: 4 });
        assert_eq!(lift_core(&def, &sprites, &sounds).unwrap().colours, src.colours);
    }

    #[test]
    fn lifting_does_not_key() {
        let (def, mut sprites, sounds) = compile_core(&sample()).unwrap();
        let mut img = RgbaImage::new(3, 1);
        img.set(0, 0, Rgba::rgb(254, 0, 254));
        img.set(1, 0, Rgba::rgb(255, 0, 255));
        img.set(2, 0, Rgba::rgb(1, 2, 3));
        sprites.insert("extra".into(), img.clone());
        let src = lift_core(&def, &sprites, &sounds).unwrap();
        assert_eq!(src.sprites["extra"].image, img, "the importer keys; lifting keeps every pixel");
    }

    #[test]
    fn a_giblets_image_with_pixels_outside_the_parts_does_not_lift() {
        let (def, mut sprites, sounds) = compile_core(&sample()).unwrap();
        sprites.get_mut("giblets").unwrap().set(20, 10, Rgba::rgb(1, 1, 1));
        let e = lift_core(&def, &sprites, &sounds).unwrap_err().to_string();
        assert!(e.contains("giblets") && e.contains("(20, 10)"), "{e}");
        let (def, mut sprites, sounds) = compile_core(&sample()).unwrap();
        sprites.insert("giblets".into(), RgbaImage::new(32, 8));
        assert!(lift_core(&def, &sprites, &sounds).is_err());
    }

    /// 16385 × 1 is above `IMAGES`' side, and cheap to build.
    #[test]
    fn a_sprite_above_the_decode_bound_is_refused() {
        let mut src = sample();
        let s = src.sprites.get_mut("extra").unwrap();
        s.image = RgbaImage::new(IMAGES.max_side + 1, 1);
        s.frames = (1, 1);
        let e = check(&src).unwrap_err().to_string();
        assert!(e.contains("gfx/extra.png") && e.contains("a side above"), "{e}");
    }
}
