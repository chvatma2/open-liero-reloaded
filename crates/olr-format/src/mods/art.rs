//! A mod's images and their glow masks (issue #19). A projectile image
//! `gfx/<name>.png` may have a mask beside it, `gfx/<name>.glow.png`: an
//! 8-bit greyscale PNG of the image's size, each value how strongly that
//! pixel glows (0 none to 255 full, drawn in quarters as a skin's glow:
//! `olr_data::skin::glow_level`). Presentation only: the simulation reads
//! an image's frame count alone.

use crate::image_io::{self, IMAGES};

/// The folder of a mod's images.
pub const GFX_DIR: &str = "gfx/";
/// The folder of a mod's sounds.
pub const SFX_DIR: &str = "sfx/";
/// A glow mask's file ending, in place of the image's `.png`.
pub const GLOW_MASK_SUFFIX: &str = ".glow.png";

/// Whether `path` is a glow mask's (`gfx/<name>.glow.png`).
pub fn is_glow_mask(path: &str) -> bool {
    path.starts_with(GFX_DIR)
        && path.len() > GFX_DIR.len() + GLOW_MASK_SUFFIX.len()
        && path.ends_with(GLOW_MASK_SUFFIX)
}

/// Whether `path` is an image a projectile may name: a `.png` in `gfx/`
/// that is not a glow mask.
pub fn is_image(path: &str) -> bool {
    path.starts_with(GFX_DIR)
        && path.ends_with(".png")
        && path.len() > GFX_DIR.len() + 4
        && !is_glow_mask(path)
}

/// The glow mask's path of image `image` (`gfx/missile.png` gives
/// `gfx/missile.glow.png`); `None` for a path that is no image.
pub fn glow_mask_of(image: &str) -> Option<String> {
    is_image(image).then(|| format!("{}{GLOW_MASK_SUFFIX}", &image[..image.len() - 4]))
}

/// The image a glow mask belongs to (`gfx/missile.glow.png` gives
/// `gfx/missile.png`); `None` for a path that is no mask.
pub fn image_of_mask(mask: &str) -> Option<String> {
    is_glow_mask(mask).then(|| format!("{}.png", &mask[..mask.len() - GLOW_MASK_SUFFIX.len()]))
}

/// The glow values of `mask` for an image of `size` (row by row), checked:
/// a greyscale PNG of exactly that size (read from its header before it is
/// decoded), else why not.
pub fn decode_glow_mask(mask: &[u8], size: (u32, u32)) -> Result<Vec<u8>, String> {
    let at = image_io::png_size(mask).ok_or("not a PNG")?;
    if at != size {
        return Err(format!("{} × {}, not its image's {} × {}", at.0, at.1, size.0, size.1));
    }
    let (_, _, values) = image_io::decode_grey_within(mask, IMAGES)?;
    Ok(values)
}

/// `values` (row by row, `size`) as a glow mask's PNG.
pub fn encode_glow_mask(size: (u32, u32), values: &[u8]) -> Vec<u8> {
    image_io::encode_grey(size.0, size.1, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_pair_with_their_images() {
        assert_eq!(glow_mask_of("gfx/missile.png").as_deref(), Some("gfx/missile.glow.png"));
        assert_eq!(image_of_mask("gfx/missile.glow.png").as_deref(), Some("gfx/missile.png"));
        assert_eq!(glow_mask_of("gfx/a/b.png").as_deref(), Some("gfx/a/b.glow.png"));
        for not_image in ["gfx/missile.glow.png", "sfx/a.png", "gfx/.png", "gfx/x.ogg", "missile.png"] {
            assert_eq!(glow_mask_of(not_image), None, "{not_image}");
        }
        for not_mask in ["gfx/missile.png", "gfx/.glow.png", "sfx/a.glow.png"] {
            assert_eq!(image_of_mask(not_mask), None, "{not_mask}");
        }
    }

    #[test]
    fn a_mask_must_be_its_images_size() {
        let mask = encode_glow_mask((3, 2), &[0, 64, 255, 0, 0, 128]);
        assert_eq!(decode_glow_mask(&mask, (3, 2)).unwrap(), vec![0, 64, 255, 0, 0, 128]);
        let e = decode_glow_mask(&mask, (2, 2)).unwrap_err();
        assert!(e.contains("3 × 2") && e.contains("2 × 2"), "{e}");
        assert!(decode_glow_mask(b"nope", (3, 2)).is_err());
        let cut = &mask[..mask.len() - 8];
        assert!(decode_glow_mask(cut, (3, 2)).is_err());
        let colour = image_io::encode_rgba(&olr_data::image::RgbaImage::new(3, 2));
        assert!(decode_glow_mask(&colour, (3, 2)).is_err());
    }
}
