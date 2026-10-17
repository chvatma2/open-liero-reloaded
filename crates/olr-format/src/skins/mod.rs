//! The skin authoring format: `skin.ron`, `skin.png` (a grid of frames,
//! aim columns by walk rows), `tint.png` (greyscale: 0 keep, 255 player
//! colour) and an optional `left.png`. Compiled to today's 672 × 18 OLX
//! sheet and lifted from it (docs/porting/olx-notes/asset-formats.md §3).

mod files;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};

pub use files::{read_skin_files, write_skin_files};

use crate::image_io::{IMAGES, check_size};
use crate::{FormatError, LiftError};

/// The largest `art_scale`.
pub const MAX_ART_SCALE: u32 = 8;
/// The largest frame side in art pixels.
pub const MAX_FRAME_SIDE: u32 = 256;
/// The most aim columns, and the most walk rows.
pub const MAX_FRAMES: u32 = 64;

/// OLX's skin layout: `art_scale`, `frame_size`, aim columns, walk rows.
pub const OLX_SKIN: (u32, (u32, u32), u32, u32) = (2, (20, 18), 7, 3);

pub(crate) const SKIN_FILE: &str = "skin.ron";
pub(crate) const IMAGE_FILE: &str = "skin.png";
pub(crate) const TINT_FILE: &str = "tint.png";
pub(crate) const LEFT_FILE: &str = "left.png";

/// OLX's sheet: 21 frames (7 aim × 3 walk), 32 px apart, 4 px of spacing
/// before each 20-px worm window.
/// Derived from [`FrameLayout::OLX`], the one definition.
const SHEET: (u32, u32) = (FrameLayout::OLX.frame_width * 21, FrameLayout::OLX.frame_height);
const FRAME_PITCH: u32 = FrameLayout::OLX.frame_width;
const FRAME_LEFT: u32 = FrameLayout::OLX.frame_spacing;

/// A skin as its files hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinSource {
    pub name: String,
    pub notes: String,
    pub art_scale: u32,
    pub frame_size: (u32, u32),
    /// Columns.
    pub aim_frames: u32,
    /// Rows.
    pub walk_frames: u32,
    /// `frame_size.0 * aim_frames` × `frame_size.1 * walk_frames`.
    pub image: RgbaImage,
    /// Greyscale, same size as `image`: 0 keep, 255 player colour.
    pub tint: Vec<u8>,
    /// Explicit left-facing frames; not supported by the 0.2 runtime.
    pub left: Option<RgbaImage>,
}

/// What `skin.ron` alone says: the layout's bounds, and the sheet size it
/// implies, which must be within what the reader decodes ([`IMAGES`]).
/// Returns the sheet size. The reader calls it before decoding any image.
pub(crate) fn check_layout(
    art_scale: u32,
    frame_size: (u32, u32),
    aim_frames: u32,
    walk_frames: u32,
) -> Result<(u32, u32), FormatError> {
    if !(1..=MAX_ART_SCALE).contains(&art_scale) {
        return Err(FormatError::invalid(
            SKIN_FILE,
            "art_scale",
            format!("{art_scale}, not within 1 to {MAX_ART_SCALE}"),
        ));
    }
    let (fw, fh) = frame_size;
    if !(1..=MAX_FRAME_SIDE).contains(&fw) || !(1..=MAX_FRAME_SIDE).contains(&fh) {
        return Err(FormatError::invalid(
            SKIN_FILE,
            "frame_size",
            format!("{fw}x{fh}, not within 1x1 to {MAX_FRAME_SIDE}x{MAX_FRAME_SIDE}"),
        ));
    }
    for (path, n) in [("aim_frames", aim_frames), ("walk_frames", walk_frames)] {
        if !(1..=MAX_FRAMES).contains(&n) {
            return Err(FormatError::invalid(SKIN_FILE, path, format!("{n}, not within 1 to {MAX_FRAMES}")));
        }
    }
    // Bounded above, so these cannot overflow.
    let (w, h) = (fw * aim_frames, fh * walk_frames);
    // The reader decodes within IMAGES: a layout above it could not reopen.
    check_size(w, h, IMAGES).map_err(|e| {
        FormatError::invalid(SKIN_FILE, "frame_size", format!("the layout needs {w}x{h}: {e}"))
    })?;
    Ok((w, h))
}

/// The checks the compiler, the writer and the reader share.
pub(crate) fn check(src: &SkinSource) -> Result<(), FormatError> {
    let (w, h) = check_layout(src.art_scale, src.frame_size, src.aim_frames, src.walk_frames)?;
    let img = &src.image;
    if (img.width(), img.height()) != (w, h) {
        return Err(FormatError::invalid(
            IMAGE_FILE,
            "",
            format!(
                "{IMAGE_FILE} is {}x{}, the layout needs {w}x{h} (frame_size × frames)",
                img.width(),
                img.height()
            ),
        ));
    }
    if src.tint.len() != w as usize * h as usize {
        return Err(FormatError::invalid(
            TINT_FILE,
            "",
            format!("{TINT_FILE} has {} pixels, {IMAGE_FILE} is {w}x{h}", src.tint.len()),
        ));
    }
    if let Some(l) = &src.left
        && (l.width(), l.height()) != (w, h)
    {
        return Err(FormatError::invalid(
            LEFT_FILE,
            "",
            format!("{LEFT_FILE} is {}x{}, {IMAGE_FILE} is {w}x{h}", l.width(), l.height()),
        ));
    }
    Ok(())
}

fn unsupported(file: &str, path: &str, what: impl std::fmt::Display) -> FormatError {
    FormatError::invalid(file, path, format!("{what} is not supported by this runtime yet"))
}

/// Makes today's OLX sheet (a skin of the OLX layout only).
pub fn compile_skin(src: &SkinSource) -> Result<Skin, FormatError> {
    check(src)?;
    let layout = (src.art_scale, src.frame_size, src.aim_frames, src.walk_frames);
    if layout != OLX_SKIN {
        return Err(unsupported(
            SKIN_FILE,
            "",
            format!(
                "a skin layout of art_scale {}, frame_size {:?}, {} aim and {} walk frames (only OLX's {:?})",
                layout.0, layout.1, layout.2, layout.3, OLX_SKIN
            ),
        ));
    }
    if src.left.is_some() {
        return Err(unsupported(LEFT_FILE, "", "a left-facing image"));
    }
    let w = src.image.width();
    if let Some(i) = src.tint.iter().position(|&t| t != 0 && t != 255) {
        let (x, y) = (i as u32 % w, i as u32 / w);
        return Err(FormatError::invalid(
            TINT_FILE,
            "",
            format!("partial tint {} at ({x}, {y}) is not supported by this runtime yet", src.tint[i]),
        ));
    }
    let mut image = RgbaImage::new(SHEET.0, SHEET.1);
    let mut mask = vec![MaskKind::Transparent; SHEET.0 as usize * SHEET.1 as usize];
    let (fw, fh) = src.frame_size;
    for w_row in 0..src.walk_frames {
        for a in 0..src.aim_frames {
            let f = w_row * src.aim_frames + a;
            for y in 0..fh {
                for x in 0..fw {
                    let (ix, iy) = (a * fw + x, w_row * fh + y);
                    let px = src.image.get(ix, iy);
                    if px.a == 0 {
                        continue;
                    }
                    let sx = f * FRAME_PITCH + FRAME_LEFT + x;
                    image.set(sx, y, px);
                    mask[y as usize * SHEET.0 as usize + sx as usize] =
                        if src.tint[iy as usize * w as usize + ix as usize] == 0 {
                            MaskKind::Keep
                        } else {
                            MaskKind::Colorize
                        };
                }
            }
        }
    }
    Ok(Skin { def: SkinDef { name: src.name.clone(), layout: FrameLayout::OLX }, image, mask })
}

/// The inverse of [`compile_skin`]; `notes` stay empty.
pub fn lift_skin(skin: &Skin) -> Result<SkinSource, LiftError> {
    let fail = |message: String| LiftError { item: format!("skin \"{}\"", skin.def.name), message };
    if skin.def.layout != FrameLayout::OLX {
        return Err(fail("a layout other than OLX's".into()));
    }
    let (sw, sh) = (skin.image.width(), skin.image.height());
    if (sw, sh) != SHEET || skin.mask.len() != sw as usize * sh as usize {
        return Err(fail(format!("a {sw}x{sh} sheet, not {}x{}", SHEET.0, SHEET.1)));
    }
    let (_, (fw, fh), aim, walk) = OLX_SKIN;
    let mut image = RgbaImage::new(fw * aim, fh * walk);
    let mut tint = vec![0u8; image.width() as usize * image.height() as usize];
    for r in 0..walk {
        for a in 0..aim {
            let f = r * aim + a;
            for y in 0..fh {
                for x in 0..fw {
                    let sx = f * FRAME_PITCH + FRAME_LEFT + x;
                    let (ix, iy) = (a * fw + x, r * fh + y);
                    let (px, t) = match skin.mask_at(sx, y) {
                        MaskKind::Transparent => (Rgba::default(), 0),
                        MaskKind::Keep => (skin.image.get(sx, y), 0),
                        MaskKind::Colorize => (skin.image.get(sx, y), 255),
                    };
                    image.set(ix, iy, px);
                    tint[iy as usize * image.width() as usize + ix as usize] = t;
                }
            }
        }
    }
    Ok(SkinSource {
        name: skin.def.name.clone(),
        notes: String::new(),
        art_scale: OLX_SKIN.0,
        frame_size: OLX_SKIN.1,
        aim_frames: aim,
        walk_frames: walk,
        image,
        tint,
        left: None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A synthetic OLX-layout skin: opaque keep and tinted pixels, a
    /// partially transparent one, and transparent elsewhere.
    pub(crate) fn sample() -> SkinSource {
        let (art_scale, frame_size, aim_frames, walk_frames) = OLX_SKIN;
        let (w, h) = (frame_size.0 * aim_frames, frame_size.1 * walk_frames);
        let mut image = RgbaImage::new(w, h);
        let mut tint = vec![0u8; (w * h) as usize];
        for f in 0..aim_frames * walk_frames {
            let (a, r) = (f % aim_frames, f / aim_frames);
            for y in 3..15 {
                for x in 4..16 {
                    let (ix, iy) = (a * 20 + x, r * 18 + y);
                    image.set(ix, iy, Rgba { r: f as u8 * 10, g: x as u8 * 8, b: y as u8 * 8, a: 255 });
                    if (x + y) % 3 == 0 {
                        tint[(iy * w + ix) as usize] = 255;
                    }
                }
            }
        }
        image.set(1, 1, Rgba { r: 9, g: 8, b: 7, a: 100 });
        SkinSource {
            name: "synthetic".into(),
            notes: String::new(),
            art_scale,
            frame_size,
            aim_frames,
            walk_frames,
            image,
            tint,
            left: None,
        }
    }

    #[test]
    fn a_skin_compiles_to_the_olx_sheet_and_lifts_back() {
        let src = sample();
        let skin = compile_skin(&src).unwrap();
        assert_eq!((skin.image.width(), skin.image.height()), (672, 18));
        for f in 0..21u32 {
            let (a, r) = (f % 7, f / 7);
            for y in 0..18 {
                for o in (0..4).chain(24..32) {
                    let sx = f * 32 + o;
                    assert_eq!(skin.mask_at(sx, y), MaskKind::Transparent);
                    assert_eq!(skin.image.get(sx, y), Rgba::default());
                }
                for x in 0..20 {
                    let sx = f * 32 + 4 + x;
                    let px = src.image.get(a * 20 + x, r * 18 + y);
                    if px.a == 0 {
                        assert_eq!(skin.mask_at(sx, y), MaskKind::Transparent);
                        assert_eq!(skin.image.get(sx, y), Rgba::default());
                    } else {
                        assert_eq!(skin.image.get(sx, y), px);
                        let t = src.tint[((r * 18 + y) * 140 + a * 20 + x) as usize];
                        let want = if t == 0 { MaskKind::Keep } else { MaskKind::Colorize };
                        assert_eq!(skin.mask_at(sx, y), want);
                    }
                }
            }
        }
        assert_eq!(skin.def, SkinDef { name: "synthetic".into(), layout: FrameLayout::OLX });
        assert_eq!(lift_skin(&skin).unwrap(), src);
    }

    #[test]
    fn a_partial_tint_names_the_pixel() {
        let mut src = sample();
        src.tint[140 + 5] = 128;
        let e = compile_skin(&src).unwrap_err().to_string();
        assert!(e.contains("tint.png") && e.contains("partial tint") && e.contains("(5, 1)"), "{e}");
    }

    #[test]
    fn a_left_image_is_not_supported() {
        let mut src = sample();
        src.left = Some(src.image.clone());
        let e = compile_skin(&src).unwrap_err().to_string();
        assert!(e.contains("left.png") && e.contains("not supported by this runtime yet"), "{e}");
    }

    #[test]
    fn another_art_scale_is_not_supported() {
        let mut src = sample();
        src.art_scale = 1;
        let e = compile_skin(&src).unwrap_err().to_string();
        assert!(e.contains("not supported by this runtime yet"), "{e}");
    }

    #[test]
    fn an_image_of_another_size_names_skin_png() {
        let mut src = sample();
        src.image = RgbaImage::new(141, 54);
        let e = compile_skin(&src).unwrap_err().to_string();
        assert!(e.contains("skin.png") && e.contains("141x54"), "{e}");
    }

    #[test]
    fn a_non_olx_layout_does_not_lift() {
        let mut skin = compile_skin(&sample()).unwrap();
        skin.def.layout.frame_spacing = 2;
        assert!(lift_skin(&skin).is_err());
        let mut skin = compile_skin(&sample()).unwrap();
        skin.image = RgbaImage::new(10, 10);
        assert!(lift_skin(&skin).is_err());
    }

    /// 64 × 64 frames of 256 px is a 16384² layout, above `IMAGES`: refused
    /// from the numbers, before the (tiny, mismatched) image is compared.
    #[test]
    fn a_layout_above_the_decode_bound_is_refused() {
        let mut s = sample();
        s.frame_size = (256, 256);
        s.aim_frames = 64;
        s.walk_frames = 64;
        let e = check(&s).unwrap_err().to_string();
        assert!(e.contains("skin.ron") && e.contains("16384x16384") && e.contains("pixels"), "{e}");
    }
}
