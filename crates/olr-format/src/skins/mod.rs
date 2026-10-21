//! The skin authoring format: `skin.ron`, `skin.png` (a grid of frames,
//! aim columns by walk rows), `tint.png` (greyscale: 0 keep, 255 player
//! colour), an optional `glow.png` (greyscale: 0 no glow, 255 full) and an
//! optional `left.png`. Two layouts compile: OLX's (art scale 2, 20 × 18
//! frames, to a 672 × 18 sheet, docs/porting/olx-notes/asset-formats.md §3)
//! and double resolution (art scale 4, 40 × 36 frames, to a 1344 × 36
//! sheet; issue #12), both 7 aim columns by 3 walk rows; the worm is 10 × 9
//! world pixels either way. Lifted from either sheet.
//!
//! A glowing pixel shows bright whatever the darkness (the player's tint
//! still applies); glow on a transparent pixel is dropped. `skin.ron`'s
//! `light` makes the glowing pixels light the worm's surroundings in a lit
//! level: its colour is the frame's glowing pixels' as drawn, weighted by
//! their glow; its strength scales with how much of the frame glows.

mod files;
pub(crate) use files::SkinFile;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::skin::{FrameLayout, MaskKind, Skin, SkinDef, SkinLight};
use olr_look::engine::{MAX_SKIN_LIGHT_INTENSITY, MAX_SKIN_LIGHT_REACH};

pub use files::{file_paths, read_skin_files, write_skin_files};

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

/// The double-resolution layout (issue #12): OLX's at art scale 4.
pub const DOUBLE_SKIN: (u32, (u32, u32), u32, u32) = (4, (40, 36), 7, 3);

pub(crate) const SKIN_FILE: &str = "skin.ron";
pub(crate) const IMAGE_FILE: &str = "skin.png";
pub(crate) const TINT_FILE: &str = "tint.png";
pub(crate) const GLOW_FILE: &str = "glow.png";
pub(crate) const LEFT_FILE: &str = "left.png";

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
    /// Greyscale, same size as `image`: 0 no glow, 255 full (issue #12).
    pub glow: Option<Vec<u8>>,
    /// The glowing pixels' light (issue #12), within the engine's caps.
    pub light: Option<SkinLight>,
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

/// `skin.ron`'s `light` within the engine's caps (issue #12): an intensity
/// of 1 to [`MAX_SKIN_LIGHT_INTENSITY`] percent, a reach of 1 to
/// [`MAX_SKIN_LIGHT_REACH`] world pixels.
pub(crate) fn check_light(light: Option<SkinLight>) -> Result<(), FormatError> {
    let Some(l) = light else { return Ok(()) };
    if !(1..=MAX_SKIN_LIGHT_INTENSITY).contains(&l.intensity) {
        return Err(FormatError::invalid(
            SKIN_FILE,
            "light.intensity",
            format!("{}, not within 1 to {MAX_SKIN_LIGHT_INTENSITY} (percent)", l.intensity),
        ));
    }
    if !(1..=MAX_SKIN_LIGHT_REACH).contains(&l.reach) {
        return Err(FormatError::invalid(
            SKIN_FILE,
            "light.reach",
            format!("{}, not within 1 to {MAX_SKIN_LIGHT_REACH} (world pixels)", l.reach),
        ));
    }
    Ok(())
}

/// The checks the compiler, the writer and the reader share.
pub(crate) fn check(src: &SkinSource) -> Result<(), FormatError> {
    let (w, h) = check_layout(src.art_scale, src.frame_size, src.aim_frames, src.walk_frames)?;
    check_light(src.light)?;
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
    let px = w as usize * h as usize;
    if src.tint.len() != px {
        return Err(FormatError::invalid(
            TINT_FILE,
            "",
            format!("{TINT_FILE} has {} pixels, {IMAGE_FILE} is {w}x{h}", src.tint.len()),
        ));
    }
    if let Some(g) = &src.glow
        && g.len() != px
    {
        return Err(FormatError::invalid(
            GLOW_FILE,
            "",
            format!("{GLOW_FILE} has {} pixels, {IMAGE_FILE} is {w}x{h}", g.len()),
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

/// The runtime's sheet layout for a source layout: OLX's or double
/// resolution, else `None`.
fn sheet_layout(layout: (u32, (u32, u32), u32, u32)) -> Option<FrameLayout> {
    [(OLX_SKIN, FrameLayout::OLX), (DOUBLE_SKIN, FrameLayout::DOUBLE)]
        .into_iter()
        .find_map(|(l, f)| (l == layout).then_some(f))
}

/// Makes the runtime sheet: OLX's 672 × 18 for an OLX-layout skin, its
/// double (1344 × 36) for a double-resolution one.
pub fn compile_skin(src: &SkinSource) -> Result<Skin, FormatError> {
    check(src)?;
    let layout = (src.art_scale, src.frame_size, src.aim_frames, src.walk_frames);
    let Some(sheet) = sheet_layout(layout) else {
        return Err(unsupported(
            SKIN_FILE,
            "",
            format!(
                "a skin layout of art_scale {}, frame_size {:?}, {} aim and {} walk frames (only OLX's {:?} \
                 and double resolution's {:?})",
                layout.0, layout.1, layout.2, layout.3, OLX_SKIN, DOUBLE_SKIN
            ),
        ));
    };
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
    let (sw, sh) = sheet.sheet_size();
    let mut image = RgbaImage::new(sw, sh);
    let mut mask = vec![MaskKind::Transparent; sw as usize * sh as usize];
    let mut glow = if src.glow.is_some() { vec![0u8; mask.len()] } else { Vec::new() };
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
                    let sx = f * sheet.frame_width + sheet.frame_spacing + x;
                    let (i, si) =
                        (iy as usize * w as usize + ix as usize, y as usize * sw as usize + sx as usize);
                    image.set(sx, y, px);
                    mask[si] = if src.tint[i] == 0 { MaskKind::Keep } else { MaskKind::Colorize };
                    if let Some(g) = &src.glow {
                        glow[si] = g[i];
                    }
                }
            }
        }
    }
    let def = SkinDef { name: src.name.clone(), layout: sheet, light: src.light };
    Ok(Skin { def, image, mask, glow })
}

/// The inverse of [`compile_skin`]; `notes` stay empty.
pub fn lift_skin(skin: &Skin) -> Result<SkinSource, LiftError> {
    let fail = |message: String| LiftError { item: format!("skin \"{}\"", skin.def.name), message };
    let layout = skin.def.layout;
    let (art_scale, (fw, fh), aim, walk) = if layout == FrameLayout::OLX {
        OLX_SKIN
    } else if layout == FrameLayout::DOUBLE {
        DOUBLE_SKIN
    } else {
        return Err(fail("a layout other than OLX's or double resolution's".into()));
    };
    let (sw, sh) = (skin.image.width(), skin.image.height());
    let want = layout.sheet_size();
    if (sw, sh) != want || skin.mask.len() != sw as usize * sh as usize {
        return Err(fail(format!("a {sw}x{sh} sheet, not {}x{}", want.0, want.1)));
    }
    if !skin.glow.is_empty() && skin.glow.len() != skin.mask.len() {
        return Err(fail(format!("a glow mask of {} pixels for a {sw}x{sh} sheet", skin.glow.len())));
    }
    let mut image = RgbaImage::new(fw * aim, fh * walk);
    let n = image.width() as usize * image.height() as usize;
    let mut tint = vec![0u8; n];
    let mut glow = (!skin.glow.is_empty()).then(|| vec![0u8; n]);
    for r in 0..walk {
        for a in 0..aim {
            let f = r * aim + a;
            for y in 0..fh {
                for x in 0..fw {
                    let sx = f * layout.frame_width + layout.frame_spacing + x;
                    let (ix, iy) = (a * fw + x, r * fh + y);
                    let (px, t) = match skin.mask_at(sx, y) {
                        MaskKind::Transparent => (Rgba::default(), 0),
                        MaskKind::Keep => (skin.image.get(sx, y), 0),
                        MaskKind::Colorize => (skin.image.get(sx, y), 255),
                    };
                    image.set(ix, iy, px);
                    let i = iy as usize * image.width() as usize + ix as usize;
                    tint[i] = t;
                    if let Some(g) = &mut glow
                        && skin.mask_at(sx, y) != MaskKind::Transparent
                    {
                        g[i] = skin.glow_at(sx, y);
                    }
                }
            }
        }
    }
    Ok(SkinSource {
        name: skin.def.name.clone(),
        notes: String::new(),
        art_scale,
        frame_size: (fw, fh),
        aim_frames: aim,
        walk_frames: walk,
        image,
        tint,
        glow,
        light: skin.def.light,
        left: None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A synthetic skin of `layout` (`OLX_SKIN` or `DOUBLE_SKIN`): opaque
    /// keep and tinted pixels, a partially transparent one, and
    /// transparent elsewhere.
    pub(crate) fn sample_of(layout: (u32, (u32, u32), u32, u32)) -> SkinSource {
        let (art_scale, frame_size, aim_frames, walk_frames) = layout;
        let (fw, fh) = frame_size;
        let k = art_scale / 2;
        let (w, h) = (fw * aim_frames, fh * walk_frames);
        let mut image = RgbaImage::new(w, h);
        let mut tint = vec![0u8; (w * h) as usize];
        for f in 0..aim_frames * walk_frames {
            let (a, r) = (f % aim_frames, f / aim_frames);
            for y in 3 * k..15 * k {
                for x in 4 * k..16 * k {
                    let (ix, iy) = (a * fw + x, r * fh + y);
                    image.set(ix, iy, Rgba { r: f as u8 * 10, g: x as u8 * 4, b: y as u8 * 4, a: 255 });
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
            glow: None,
            light: None,
            left: None,
        }
    }

    /// An OLX-layout [`sample_of`].
    pub(crate) fn sample() -> SkinSource {
        sample_of(OLX_SKIN)
    }

    /// A double-resolution [`sample_of`] with glowing pixels (every frame's
    /// eyes, at two strengths) and a light.
    pub(crate) fn double_sample() -> SkinSource {
        let mut s = sample_of(DOUBLE_SKIN);
        let w = s.image.width();
        let mut glow = vec![0u8; s.tint.len()];
        for f in 0..21 {
            let (a, r) = (f % 7, f / 7);
            for (x, y, g) in [(20, 10, 255), (21, 10, 255), (24, 10, 128)] {
                glow[((r * 36 + y) * w + a * 40 + x) as usize] = g;
            }
        }
        s.glow = Some(glow);
        s.light = Some(SkinLight { intensity: 150, reach: 24 });
        s
    }

    #[test]
    fn a_skin_compiles_to_the_olx_sheet_and_lifts_back() {
        let src = sample();
        let skin = compile_skin(&src).unwrap();
        assert_eq!((skin.image.width(), skin.image.height()), (672, 18));
        assert!(skin.glow.is_empty(), "no glow.png, no glow mask");
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
        assert_eq!(skin.def, SkinDef { name: "synthetic".into(), layout: FrameLayout::OLX, light: None });
        assert_eq!(lift_skin(&skin).unwrap(), src);
    }

    /// Issue #12: a double-resolution skin compiles to the doubled sheet
    /// (frames 64 apart, 8 in), with its glow and light, and lifts back.
    #[test]
    fn a_double_skin_compiles_and_lifts_back() {
        let src = double_sample();
        let skin = compile_skin(&src).unwrap();
        assert_eq!((skin.image.width(), skin.image.height()), (1344, 36));
        assert_eq!(skin.def.layout, FrameLayout::DOUBLE);
        assert_eq!(skin.def.light, Some(SkinLight { intensity: 150, reach: 24 }));
        assert_eq!(skin.glow.len(), 1344 * 36);
        for f in 0..21u32 {
            let (a, r) = (f % 7, f / 7);
            for y in 0..36 {
                for o in (0..8).chain(48..64) {
                    assert_eq!(skin.mask_at(f * 64 + o, y), MaskKind::Transparent);
                }
                for x in 0..40 {
                    let sx = f * 64 + 8 + x;
                    let px = src.image.get(a * 40 + x, r * 36 + y);
                    assert_eq!(skin.image.get(sx, y), if px.a == 0 { Rgba::default() } else { px });
                }
            }
            assert_eq!(skin.glow_at(f * 64 + 8 + 20, 10), 255);
            assert_eq!(skin.glow_at(f * 64 + 8 + 24, 10), 128);
            assert_eq!(skin.glow_at(f * 64 + 8 + 22, 10), 0);
        }
        assert_eq!(lift_skin(&skin).unwrap(), src);
        // Without glow or light it round-trips too.
        let plain = sample_of(DOUBLE_SKIN);
        assert_eq!(lift_skin(&compile_skin(&plain).unwrap()).unwrap(), plain);
    }

    #[test]
    fn glow_on_a_transparent_pixel_is_dropped() {
        let mut src = double_sample();
        src.glow.as_mut().unwrap()[0] = 200;
        let back = lift_skin(&compile_skin(&src).unwrap()).unwrap();
        assert_eq!(back.glow.as_ref().unwrap()[0], 0);
        assert_eq!(back.glow.unwrap()[1..], src.glow.unwrap()[1..]);
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
    fn another_layout_is_not_supported() {
        let mut src = sample();
        src.art_scale = 1;
        let e = compile_skin(&src).unwrap_err().to_string();
        assert!(e.contains("not supported by this runtime yet"), "{e}");
        // Art scale 4 needs 40 × 36 frames; 2 needs 20 × 18.
        let mut src = sample();
        src.art_scale = 4;
        assert!(compile_skin(&src).unwrap_err().to_string().contains("not supported"));
        let mut src = sample_of(DOUBLE_SKIN);
        src.art_scale = 2;
        assert!(compile_skin(&src).unwrap_err().to_string().contains("not supported"));
    }

    #[test]
    fn an_image_of_another_size_names_skin_png() {
        let mut src = sample();
        src.image = RgbaImage::new(141, 54);
        let e = compile_skin(&src).unwrap_err().to_string();
        assert!(e.contains("skin.png") && e.contains("141x54"), "{e}");
    }

    #[test]
    fn a_glow_mask_of_another_size_names_glow_png() {
        let mut src = double_sample();
        src.glow = Some(vec![0; 140 * 54]);
        let e = compile_skin(&src).unwrap_err().to_string();
        assert!(e.contains("glow.png") && e.contains("280x108"), "{e}");
    }

    /// Issue #12: a light over the engine's caps (or of nothing) is refused.
    #[test]
    fn a_light_over_the_caps_is_refused() {
        for (intensity, reach, field) in [
            (0, 24, "light.intensity"),
            (201, 24, "light.intensity"),
            (100, 49, "light.reach"),
            (100, 0, "light.reach"),
        ] {
            let mut src = double_sample();
            src.light = Some(SkinLight { intensity, reach });
            let e = compile_skin(&src).unwrap_err().to_string();
            assert!(e.contains("skin.ron") && e.contains(field), "{e}");
        }
        let mut src = double_sample();
        src.light = Some(SkinLight { intensity: MAX_SKIN_LIGHT_INTENSITY, reach: MAX_SKIN_LIGHT_REACH });
        assert!(compile_skin(&src).is_ok());
    }

    #[test]
    fn a_non_drawn_layout_does_not_lift() {
        let mut skin = compile_skin(&sample()).unwrap();
        skin.def.layout.frame_spacing = 2;
        assert!(lift_skin(&skin).is_err());
        let mut skin = compile_skin(&sample()).unwrap();
        skin.image = RgbaImage::new(10, 10);
        assert!(lift_skin(&skin).is_err());
        // A double layout on an OLX-sized sheet, and a glow of the wrong size.
        let mut skin = compile_skin(&sample()).unwrap();
        skin.def.layout = FrameLayout::DOUBLE;
        assert!(lift_skin(&skin).is_err());
        let mut skin = compile_skin(&double_sample()).unwrap();
        skin.glow.pop();
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
