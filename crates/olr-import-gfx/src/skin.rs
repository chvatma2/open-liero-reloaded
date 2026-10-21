//! Skin sheets (`CGameSkin`, `src/client/CGameSkin.cpp`): the top half is the
//! image, the bottom half the colorization mask, each `frame_height` rows tall.

use olr_data::image::Rgba;
use olr_data::manifest::{Origin, PackKind};
use olr_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};
use olr_data::slug::slug;
use olr_pack::SkinPack;
use olr_vfs::{ContentSource, VPath};

use crate::{GfxError, decode, manifest};

/// Width of a standard sheet: 21 frames of 32 pixels.
const SHEET_WIDTH: u32 = 672;

#[derive(Debug)]
pub struct ImportedSkin {
    pub pack: SkinPack,
    pub warnings: Vec<String>,
}

/// Imports the skin PNG at `path`. Sheets larger than 672x36 are cropped to it,
/// as OLX only reads that area; smaller sheets are an error.
pub fn import_skin(olx: &dyn ContentSource, path: &VPath, origin: Origin) -> Result<ImportedSkin, GfxError> {
    let sheet = decode(olx, path)?;
    let layout = FrameLayout::OLX;
    let fh = layout.frame_height;
    let (w, h) = (sheet.width(), sheet.height());
    if w < SHEET_WIDTH || h < fh * 2 {
        return Err(GfxError::Invalid {
            file: path.to_string(),
            message: format!("sheet is {w}x{h}, needs at least {SHEET_WIDTH}x{}", fh * 2),
        });
    }
    let mut warnings = Vec::new();
    if (w, h) != (SHEET_WIDTH, fh * 2) {
        warnings.push(format!("{path}: sheet is {w}x{h}, cropped to {SHEET_WIDTH}x{}", fh * 2));
    }
    let mut image = sheet.crop(0, 0, SHEET_WIDTH, fh);
    // OLX `SetColorKey`'s corner rule (`GfxPrimitives.cpp:116-150`).
    let key = sheet.olx_color_key();
    let mut mask = Vec::with_capacity((SHEET_WIDTH * fh) as usize);
    for y in 0..fh {
        for x in 0..SHEET_WIDTH {
            let px = image.get(x, y);
            let m = sheet.get(x, y + fh);
            // Same order as Colorize_Execute: black keeps, the colour key hides, anything else tints.
            let mut kind = if m.same_rgb(Rgba::BLACK) {
                MaskKind::Keep
            } else if m.same_rgb(key) {
                MaskKind::Transparent
            } else {
                MaskKind::Colorize
            };
            // A colour-key or fully transparent image pixel is never drawn.
            if px.same_rgb(key) || px.a == 0 || m.a == 0 {
                kind = MaskKind::Transparent;
            }
            // Only the worm's window of each frame is ever drawn (`frame_spacing`
            // in, `worm_width` wide); the rest of the 32-pixel pitch is hidden, so
            // the pack (which holds just the windows) reads back as written.
            let col = x % layout.frame_width;
            if !(layout.frame_spacing..layout.frame_spacing + layout.worm_width).contains(&col) {
                kind = MaskKind::Transparent;
            }
            if kind == MaskKind::Transparent {
                // Hidden pixels are fully transparent (plan 13c ruling 4), so
                // the pack reads back as written.
                image.set(x, y, Rgba::TRANSPARENT);
            } else {
                image.set(x, y, Rgba { a: 255, ..px });
            }
            mask.push(kind);
        }
    }
    let file = path.file_name();
    let name = file.rsplit_once('.').map_or(file, |(s, _)| s).to_string();
    let pack = SkinPack {
        manifest: manifest(format!("olx-skin-{}", slug(&name)), name.clone(), PackKind::Skin, origin),
        skin: Skin { def: SkinDef { name, layout, light: None }, image, mask, glow: vec![] },
    };
    Ok(ImportedSkin { pack, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::image::RgbaImage;
    use olr_vfs::MemFs;

    fn sheet(w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.set(x, y, Rgba::rgb(100, 100, 100));
            }
        }
        // Mask row 0 (sheet row 18), in frame 0's window (it starts at x=4):
        // x=4 black, x=5 magenta, x=6 white.
        img.set(4, 18, Rgba::BLACK);
        img.set(5, 18, Rgba::MAGENTA);
        img.set(6, 18, Rgba::rgb(255, 255, 255));
        // Image pixel x=7 is magenta with a colorize mask: still transparent.
        // OLX's SetColorKey_Alpha (src/client/GfxPrimitives.cpp:88-141) gives
        // key-coloured pixels alpha 0 before Colorize_Execute, which keeps
        // alpha (plan 13c ruling 5).
        img.set(7, 0, Rgba::MAGENTA);
        img
    }

    fn import(img: &RgbaImage) -> Result<ImportedSkin, GfxError> {
        let fs = MemFs::new().with("skins/Cool Worm.png", olr_pack::image_io::encode_rgba(img));
        let origin = Origin { source: "t".into(), path: "skins".into(), revision: "0".into() };
        import_skin(&fs, &VPath::new("skins/Cool Worm.png").unwrap(), origin)
    }

    #[test]
    fn mask_follows_olx_rules() {
        let s = import(&sheet(672, 36)).unwrap();
        assert!(s.warnings.is_empty());
        let skin = &s.pack.skin;
        assert_eq!(s.pack.manifest.id, "olx-skin-cool-worm");
        assert_eq!((skin.image.width(), skin.image.height()), (672, 18));
        assert_eq!(skin.mask_at(4, 0), MaskKind::Keep);
        assert_eq!(skin.mask_at(5, 0), MaskKind::Transparent);
        assert_eq!(skin.mask_at(6, 0), MaskKind::Colorize);
        assert_eq!(skin.mask_at(7, 0), MaskKind::Transparent);
        assert_eq!(skin.image.get(5, 0).a, 0);
        // Outside the worm's window of a frame nothing is drawn: hidden.
        for x in [0, 3, 24, 31, 32 + 3, 32 + 24] {
            assert_eq!(skin.mask_at(x, 0), MaskKind::Transparent, "x = {x}");
            assert_eq!(skin.image.get(x, 0), Rgba::TRANSPARENT, "x = {x}");
        }
        assert_eq!(skin.mask_at(23, 0), MaskKind::Colorize, "the window's last column");
    }

    #[test]
    fn bugged_colour_key_in_a_corner_switches_the_key() {
        // OLX SetColorKey (GfxPrimitives.cpp:116-150): a (254,0,254) corner makes
        // (254,0,254) the transparent colour of the whole sheet.
        let bugged = Rgba::rgb(254, 0, 254);
        let mut img = sheet(672, 36);
        img.set(671, 35, bugged);
        img.set(5, 18, bugged); // mask pixel
        img.set(6, 0, bugged); // image pixel, colorize mask
        let s = import(&img).unwrap();
        assert_eq!(s.pack.skin.mask_at(5, 0), MaskKind::Transparent);
        assert_eq!(s.pack.skin.mask_at(6, 0), MaskKind::Transparent);
        // Without the bugged corner, (254,0,254) is an ordinary colour.
        let mut img = sheet(672, 36);
        img.set(5, 18, bugged);
        assert_eq!(import(&img).unwrap().pack.skin.mask_at(5, 0), MaskKind::Colorize);
    }

    #[test]
    fn oversized_sheets_are_cropped_with_a_warning() {
        let s = import(&sheet(673, 51)).unwrap();
        assert_eq!(s.warnings.len(), 1);
        assert_eq!(s.pack.skin.image.width(), 672);
    }

    #[test]
    fn undersized_sheets_are_rejected() {
        assert!(matches!(import(&sheet(600, 36)), Err(GfxError::Invalid { .. })));
    }
}
