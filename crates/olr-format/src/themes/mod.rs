//! The theme authoring format: `theme.ron`, `holes/<i>.png` with
//! `holes/<i>.mask.png`, `green_ball.png` with `green_ball.mask.png`,
//! `stones/<i>.png`, `misc/<i>.png`, `front_tile.png` and `back_tile.png`.
//!
//! A stamp (hole or green ball) is a meaning mask plus a paint layer;
//! [`compile_theme`] writes OLX's colour-coded images back, so `olr-terrain`
//! and the simulation read them unchanged (`docs/porting/olx-notes/asset-formats.md` §4).

mod files;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::theme::{Theme, ThemeDef};

pub use files::{read_theme_files, write_theme_files};

use crate::image_io::olx_sprite_to_alpha;
use crate::{FormatError, LiftError};

/// The longest side of a hole or green-ball stamp, in world pixels.
pub const MAX_STAMP_SIDE: u32 = 64;
/// The most holes in a theme.
pub const MAX_HOLES: usize = 16;
/// The most stones, and the most misc images.
pub const MAX_THEME_IMAGES: usize = 64;
/// A hole mask's palette: untouched, carve, paint.
pub const HOLE_PALETTE: [u8; 9] = [0, 0, 0, 255, 0, 255, 255, 255, 255];
/// A green-ball mask's palette: untouched, green, rim.
pub const GREEN_PALETTE: [u8; 9] = [0, 0, 0, 0, 255, 0, 255, 255, 255];

pub(crate) const THEME_FILE: &str = "theme.ron";
pub(crate) const GREEN_FILE: &str = "green_ball.png";
pub(crate) const GREEN_MASK_FILE: &str = "green_ball.mask.png";
pub(crate) const FRONT_TILE_FILE: &str = "front_tile.png";
pub(crate) const BACK_TILE_FILE: &str = "back_tile.png";

const MAGENTA: Rgba = Rgba::MAGENTA;
const BLACK: Rgba = Rgba::BLACK;
const GREEN: Rgba = Rgba::rgb(0, 255, 0);

/// A stamp: what each pixel means, and the paint layer.
#[derive(Clone, Debug, PartialEq)]
pub struct StampImage {
    /// Row-major, `paint.width() * paint.height()`; 0, 1 or 2.
    pub mask: Vec<u8>,
    /// The paint (or rim) colour where `mask == 2`, `(0,0,0,0)` elsewhere.
    pub paint: RgbaImage,
}

/// A theme as its files hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSource {
    pub name: String,
    pub notes: String,
    /// Index = hole size (OLX's Hole1..): mask 0 untouched, 1 carve, 2 paint (the dirt edge).
    pub holes: Vec<StampImage>,
    /// Green dirt: mask 0 untouched (OLX's pink), 1 green, 2 rim.
    pub green_ball: Option<StampImage>,
    /// Alpha; OLX map-editor art, no runtime reader in 0.2.
    pub stones: Vec<RgbaImage>,
    pub misc: Vec<RgbaImage>,
    pub front_tile: RgbaImage,
    pub back_tile: RgbaImage,
}

pub(crate) fn hole_file(i: usize) -> String {
    format!("holes/{}.png", i + 1)
}

pub(crate) fn hole_mask_file(i: usize) -> String {
    format!("holes/{}.mask.png", i + 1)
}

pub(crate) fn image_file(dir: &str, i: usize) -> String {
    format!("{dir}/{}.png", i + 1)
}

/// The bounds and consistency checks one stamp needs: its side and mask.
fn check_stamp(file: &str, path: &str, s: &StampImage) -> Result<(), FormatError> {
    let (w, h) = (s.paint.width(), s.paint.height());
    if w == 0 || h == 0 || w > MAX_STAMP_SIDE || h > MAX_STAMP_SIDE {
        return Err(FormatError::invalid(
            file,
            path,
            format!("{w}x{h}, not within 1x1 to MAX_STAMP_SIDE ({MAX_STAMP_SIDE}) square"),
        ));
    }
    if s.mask.len() != w as usize * h as usize {
        return Err(FormatError::invalid(
            file,
            path,
            format!("mask has {} cells, the image is {w}x{h}", s.mask.len()),
        ));
    }
    if let Some(i) = s.mask.iter().position(|&m| m > 2) {
        let (x, y) = (i as u32 % w, i as u32 / w);
        return Err(FormatError::invalid(
            file,
            path,
            format!("mask index {} at ({x}, {y}), not 0, 1 or 2", s.mask[i]),
        ));
    }
    Ok(())
}

/// The checks `compile_theme` and the writer share.
pub(crate) fn check(src: &ThemeSource) -> Result<(), FormatError> {
    if src.holes.len() > MAX_HOLES {
        return Err(FormatError::invalid(
            THEME_FILE,
            "holes",
            format!("{} holes, above MAX_HOLES ({MAX_HOLES})", src.holes.len()),
        ));
    }
    for (name, n) in [("stones", src.stones.len()), ("misc", src.misc.len())] {
        if n > MAX_THEME_IMAGES {
            return Err(FormatError::invalid(
                THEME_FILE,
                name,
                format!("{n} images, above MAX_THEME_IMAGES ({MAX_THEME_IMAGES})"),
            ));
        }
    }
    for (i, s) in src.holes.iter().enumerate() {
        let path = format!("holes[{i}]");
        check_stamp(&hole_file(i), &path, s)?;
        for (p, &m) in s.mask.iter().enumerate() {
            if m == 2 {
                let (x, y) = (p as u32 % s.paint.width(), p as u32 / s.paint.width());
                let c = s.paint.get(x, y);
                if c.same_rgb(BLACK) || c.same_rgb(MAGENTA) {
                    return Err(FormatError::invalid(
                        &hole_file(i),
                        &path,
                        format!(
                            "paint pixel ({x}, {y}) is black or magenta, which would read back as untouched or carve"
                        ),
                    ));
                }
            }
        }
    }
    if let Some(g) = &src.green_ball {
        check_stamp(GREEN_FILE, "green_ball", g)?;
        for (p, &m) in g.mask.iter().enumerate() {
            if m == 2 {
                let (x, y) = (p as u32 % g.paint.width(), p as u32 / g.paint.width());
                let c = g.paint.get(x, y);
                if c == GREEN || c == MAGENTA {
                    return Err(FormatError::invalid(
                        GREEN_FILE,
                        "green_ball",
                        format!(
                            "rim pixel ({x}, {y}) is exactly #00ff00ff or #ff00ffff, which would read back as green or untouched"
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn compile_stamp(s: &StampImage, untouched: Rgba, one: Rgba) -> RgbaImage {
    let mut out = RgbaImage::new(s.paint.width(), s.paint.height());
    for (i, &m) in s.mask.iter().enumerate() {
        let (x, y) = (i as u32 % s.paint.width(), i as u32 / s.paint.width());
        out.set(
            x,
            y,
            match m {
                0 => untouched,
                1 => one,
                _ => s.paint.get(x, y),
            },
        );
    }
    out
}

fn compile_sprite(img: &RgbaImage) -> RgbaImage {
    let mut out = img.clone();
    for y in 0..img.height() {
        for x in 0..img.width() {
            if img.get(x, y).a == 0 {
                out.set(x, y, MAGENTA);
            }
        }
    }
    out
}

/// Makes the runtime theme, with OLX's colour-coded stamps.
pub fn compile_theme(src: &ThemeSource) -> Result<Theme, FormatError> {
    check(src)?;
    Ok(Theme {
        def: ThemeDef {
            name: src.name.clone(),
            holes: src.holes.len() as u32,
            stones: src.stones.len() as u32,
            misc: src.misc.len() as u32,
        },
        holes: src.holes.iter().map(|s| compile_stamp(s, BLACK, MAGENTA)).collect(),
        stones: src.stones.iter().map(compile_sprite).collect(),
        misc: src.misc.iter().map(compile_sprite).collect(),
        front_tile: src.front_tile.clone(),
        back_tile: src.back_tile.clone(),
        green_ball: src.green_ball.as_ref().map(|g| compile_stamp(g, MAGENTA, GREEN)),
    })
}

fn lift_err(item: String, message: impl Into<String>) -> LiftError {
    LiftError { item, message: message.into() }
}

fn lift_hole(i: usize, img: &RgbaImage) -> Result<StampImage, LiftError> {
    let item = format!("hole {}", i + 1);
    let mut paint = RgbaImage::new(img.width(), img.height());
    let mut mask = Vec::with_capacity(img.width() as usize * img.height() as usize);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let c = img.get(x, y);
            if c.same_rgb(MAGENTA) || c.same_rgb(BLACK) {
                if c.a != 255 {
                    return Err(lift_err(
                        item,
                        format!("pixel ({x}, {y}) is {c:?}: a carve or untouched pixel must be opaque"),
                    ));
                }
                mask.push(u8::from(c.same_rgb(MAGENTA)));
            } else {
                mask.push(2);
                paint.set(x, y, c);
            }
        }
    }
    Ok(StampImage { mask, paint })
}

fn lift_green(img: &RgbaImage) -> StampImage {
    let mut paint = RgbaImage::new(img.width(), img.height());
    let mut mask = Vec::with_capacity(img.width() as usize * img.height() as usize);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let c = img.get(x, y);
            mask.push(if c == MAGENTA {
                0
            } else if c == GREEN {
                1
            } else {
                paint.set(x, y, c);
                2
            });
        }
    }
    StampImage { mask, paint }
}

fn lift_sprite(item: String, img: &RgbaImage) -> Result<RgbaImage, LiftError> {
    let key = img.olx_color_key();
    if key != MAGENTA {
        return Err(lift_err(item, "keyed by (254, 0, 254); compile writes (255, 0, 255)"));
    }
    for y in 0..img.height() {
        for x in 0..img.width() {
            let c = img.get(x, y);
            if c.same_rgb(MAGENTA) && c.a != 255 {
                return Err(lift_err(item, format!("key pixel ({x}, {y}) is not opaque")));
            }
            if !c.same_rgb(MAGENTA) && c.a == 0 {
                return Err(lift_err(item, format!("pixel ({x}, {y}) is transparent but not the key")));
            }
        }
    }
    Ok(olx_sprite_to_alpha(img))
}

/// The inverse of [`compile_theme`]; `notes` stay empty.
pub fn lift_theme(t: &Theme) -> Result<ThemeSource, LiftError> {
    let holes = t.holes.iter().enumerate().map(|(i, h)| lift_hole(i, h)).collect::<Result<Vec<_>, _>>()?;
    let sprites = |kind: &str, v: &[RgbaImage]| {
        v.iter()
            .enumerate()
            .map(|(i, s)| lift_sprite(format!("{kind} {}", i + 1), s))
            .collect::<Result<Vec<_>, _>>()
    };
    let src = ThemeSource {
        name: t.def.name.clone(),
        notes: String::new(),
        holes,
        green_ball: t.green_ball.as_ref().map(lift_green),
        stones: sprites("stone", &t.stones)?,
        misc: sprites("misc", &t.misc)?,
        front_tile: t.front_tile.clone(),
        back_tile: t.back_tile.clone(),
    };
    check(&src).map_err(|e| lift_err(format!("theme {:?}", t.def.name), e.to_string()))?;
    Ok(src)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn stamp(mask: [u8; 9], paint: Rgba) -> StampImage {
        let mut img = RgbaImage::new(3, 3);
        for (i, &m) in mask.iter().enumerate() {
            if m == 2 {
                img.set(i as u32 % 3, i as u32 / 3, paint);
            }
        }
        StampImage { mask: mask.to_vec(), paint: img }
    }

    pub(crate) fn sample() -> ThemeSource {
        let mut stone = RgbaImage::new(2, 2);
        stone.set(1, 0, Rgba { r: 200, g: 10, b: 10, a: 128 });
        stone.set(0, 1, Rgba::rgb(1, 2, 3));
        stone.set(1, 1, Rgba::rgb(4, 5, 6));
        let mut tile = RgbaImage::new(2, 2);
        tile.set(0, 0, Rgba::rgb(9, 9, 9));
        ThemeSource {
            name: "Sample".into(),
            notes: "synthetic".into(),
            holes: vec![
                stamp([0, 1, 0, 1, 1, 1, 0, 2, 2], Rgba::rgb(10, 20, 30)),
                stamp([2, 1, 2, 1, 0, 1, 2, 1, 2], Rgba { r: 90, g: 80, b: 70, a: 200 }),
            ],
            green_ball: Some(stamp([0, 1, 0, 1, 1, 1, 2, 1, 2], Rgba::rgb(0, 200, 0))),
            stones: vec![stone],
            misc: vec![],
            front_tile: tile.clone(),
            back_tile: tile,
        }
    }

    #[test]
    fn compiles_to_olx_colours_and_lifts_back() {
        let src = sample();
        let t = compile_theme(&src).unwrap();
        assert_eq!(t.def, ThemeDef { name: "Sample".into(), holes: 2, stones: 1, misc: 0 });
        let h = &t.holes[0];
        assert_eq!(h.get(0, 0), BLACK);
        assert_eq!(h.get(1, 0), MAGENTA);
        assert_eq!(h.get(1, 2), Rgba::rgb(10, 20, 30));
        assert_eq!(t.holes[1].get(0, 0), Rgba { r: 90, g: 80, b: 70, a: 200 });
        let g = t.green_ball.as_ref().unwrap();
        assert_eq!(g.get(0, 0), MAGENTA);
        assert_eq!(g.get(1, 0), GREEN);
        assert_eq!(g.get(0, 2), Rgba::rgb(0, 200, 0));
        let s = &t.stones[0];
        assert_eq!(s.get(0, 0), MAGENTA);
        assert_eq!(s.get(1, 0), Rgba { r: 200, g: 10, b: 10, a: 128 });
        assert_eq!(s.get(0, 1), Rgba::rgb(1, 2, 3));
        let mut back = lift_theme(&t).unwrap();
        assert_eq!(back.notes, "");
        back.notes = src.notes.clone();
        assert_eq!(back, src);
    }

    #[test]
    fn a_black_or_magenta_paint_is_an_error_naming_the_pixel() {
        for c in [BLACK, MAGENTA] {
            let mut src = sample();
            src.holes[0].paint.set(2, 2, c);
            let e = compile_theme(&src).unwrap_err().to_string();
            assert!(e.contains("holes[0]") && e.contains("(2, 2)"), "{e}");
        }
    }

    #[test]
    fn a_green_or_pink_rim_is_an_error() {
        for c in [GREEN, MAGENTA] {
            let mut src = sample();
            src.green_ball.as_mut().unwrap().paint.set(0, 2, c);
            let e = compile_theme(&src).unwrap_err().to_string();
            assert!(e.contains("green_ball") && e.contains("(0, 2)"), "{e}");
        }
        // Not exactly those colours: allowed.
        let mut src = sample();
        src.green_ball.as_mut().unwrap().paint.set(0, 2, Rgba { r: 0, g: 255, b: 0, a: 254 });
        compile_theme(&src).unwrap();
    }

    #[test]
    fn bounds_name_the_constant() {
        let mut src = sample();
        src.holes = vec![src.holes[0].clone(); MAX_HOLES + 1];
        let e = compile_theme(&src).unwrap_err().to_string();
        assert!(e.contains("MAX_HOLES"), "{e}");
        let mut src = sample();
        src.holes[1] = StampImage { mask: vec![0; 65], paint: RgbaImage::new(65, 1) };
        let e = compile_theme(&src).unwrap_err().to_string();
        assert!(e.contains("MAX_STAMP_SIDE") && e.contains("holes[1]"), "{e}");
        let mut src = sample();
        src.stones = vec![RgbaImage::new(1, 1); MAX_THEME_IMAGES + 1];
        assert!(compile_theme(&src).unwrap_err().to_string().contains("MAX_THEME_IMAGES"));
        let mut src = sample();
        src.holes[0].mask[4] = 3;
        assert!(compile_theme(&src).unwrap_err().to_string().contains("(1, 1)"));
        src.holes[0].mask.pop();
        assert!(compile_theme(&src).is_err());
    }

    #[test]
    fn lifting_a_translucent_meaning_pixel_is_an_error() {
        let mut t = compile_theme(&sample()).unwrap();
        t.holes[0].set(0, 0, Rgba { r: 255, g: 0, b: 255, a: 128 });
        let e = lift_theme(&t).unwrap_err();
        assert!(e.item.contains("hole 1") && e.message.contains("(0, 0)"), "{e}");
        let mut t = compile_theme(&sample()).unwrap();
        t.holes[0].set(0, 0, Rgba { r: 0, g: 0, b: 0, a: 7 });
        assert!(lift_theme(&t).is_err());
    }

    #[test]
    fn lifting_a_stone_checks_its_key() {
        let base = compile_theme(&sample()).unwrap();
        // A (254,0,254) corner makes it the key.
        let mut t = base.clone();
        t.stones[0].set(0, 0, Rgba::rgb(254, 0, 254));
        assert!(lift_theme(&t).unwrap_err().message.contains("254"));
        // A translucent key-coloured pixel.
        let mut t = base.clone();
        t.stones[0].set(0, 0, Rgba { r: 255, g: 0, b: 255, a: 3 });
        assert!(lift_theme(&t).is_err());
        // A transparent pixel that is not the key.
        let mut t = base;
        t.stones[0].set(1, 1, Rgba { r: 4, g: 5, b: 6, a: 0 });
        assert!(lift_theme(&t).is_err());
    }
}
