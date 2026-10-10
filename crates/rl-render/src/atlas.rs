//! Packing the scene's images into one atlas texture (CPU only).

use rl_data::image::RgbaImage;
use rl_scene::{AtlasId, AtlasImage};

/// A rectangle of atlas texels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TexRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Where each image of [`rl_scene::SceneAssets::images`] sits in the atlas.
/// Texel `(0, 0)` is opaque white, for lines and placeholder squares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AtlasLayout {
    pub width: u32,
    pub height: u32,
    /// By [`AtlasId`]; `None` for an image that did not fit.
    entries: Vec<Option<TexRect>>,
}

/// The white texel every layout has.
pub const WHITE: TexRect = TexRect { x: 0, y: 0, w: 1, h: 1 };

impl AtlasLayout {
    /// Image `id`'s rectangle; `None` for an id the atlas does not have
    /// (added after the upload, or too large): drawn as a magenta square.
    pub fn entry(&self, id: AtlasId) -> Option<TexRect> {
        self.entries.get(id.0 as usize).copied().flatten()
    }

    /// How many images the layout was packed from.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// An atlas ready to upload: its layout, RGBA8 texels and what did not fit.
#[derive(Clone, Debug)]
pub struct PackedAtlas {
    pub layout: AtlasLayout,
    pub pixels: RgbaImage,
    pub warnings: Vec<String>,
}

/// The narrowest atlas: wide enough for a skin sheet (672) and most strips.
const MIN_WIDTH: u32 = 1024;

/// Shelf-packs `images` (tallest first, ties by id, so the layout is
/// deterministic) into an atlas at most `max_dim` texels on a side. An
/// image that does not fit is left out with a warning: it draws as a
/// magenta square, never a panic.
pub fn pack(images: &[AtlasImage], max_dim: u32) -> PackedAtlas {
    let max_dim = max_dim.max(1);
    let mut warnings = Vec::new();
    let widest = images.iter().map(|i| i.image.width()).max().unwrap_or(0);
    let width = widest.clamp(MIN_WIDTH, max_dim.max(MIN_WIDTH)).min(max_dim);
    let mut order: Vec<usize> = (0..images.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(images[i].image.height()), i));
    let mut entries = vec![None; images.len()];
    // The white texel takes the first shelf's corner.
    let (mut x, mut y, mut shelf) = (WHITE.w, 0u32, WHITE.h);
    for i in order {
        let img = &images[i].image;
        let (w, h) = (img.width(), img.height());
        if w == 0 || h == 0 {
            continue;
        }
        if w > width {
            warnings.push(format!("atlas: image {} ({w}x{h}) is wider than {width}", images[i].name));
            continue;
        }
        if x + w > width {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if u64::from(y) + u64::from(h) > u64::from(max_dim) {
            warnings.push(format!("atlas: image {} ({w}x{h}) does not fit in {max_dim}", images[i].name));
            continue;
        }
        entries[i] = Some(TexRect { x, y, w, h });
        x += w;
        shelf = shelf.max(h);
    }
    let height = (y + shelf).max(1);
    let mut raw = vec![0u8; width as usize * height as usize * 4];
    raw[..4].copy_from_slice(&[255; 4]);
    for (i, e) in entries.iter().enumerate() {
        if let Some(r) = e {
            blit(&mut raw, width, &images[i].image, r.x, r.y);
        }
    }
    let pixels = RgbaImage::from_raw(width, height, raw).expect("sized above");
    PackedAtlas { layout: AtlasLayout { width, height, entries }, pixels, warnings }
}

/// Copies `src` into the RGBA rows of `dst` (`width` texels wide) at
/// `(x, y)`; the caller made room.
fn blit(dst: &mut [u8], width: u32, src: &RgbaImage, x: u32, y: u32) {
    let (dw, sw) = (width as usize, src.width() as usize * 4);
    for (row, line) in src.as_raw().chunks_exact(sw).enumerate() {
        let d = ((y as usize + row) * dw + x as usize) * 4;
        dst[d..d + sw].copy_from_slice(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::image::Rgba;

    fn img(name: &str, w: u32, h: u32, c: Rgba) -> AtlasImage {
        let mut image = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                image.set(x, y, c);
            }
        }
        AtlasImage { name: name.into(), image, px_per_world: 2 }
    }

    fn overlaps(a: TexRect, b: TexRect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn atlas_packs_every_image_without_overlap() {
        let images: Vec<AtlasImage> = (0..40)
            .map(|i| img(&format!("i{i}"), 10 + (i * 37) % 300, 1 + (i * 13) % 40, Rgba::rgb(i as u8, 1, 2)))
            .chain([img("skin", 672, 18, Rgba::rgb(9, 9, 9))])
            .collect();
        let packed = pack(&images, 4096);
        assert!(packed.warnings.is_empty(), "{:?}", packed.warnings);
        let l = &packed.layout;
        let rects: Vec<TexRect> = (0..images.len()).map(|i| l.entry(AtlasId(i as u32)).unwrap()).collect();
        for (i, r) in rects.iter().enumerate() {
            assert_eq!((r.w, r.h), (images[i].image.width(), images[i].image.height()));
            assert!(r.x + r.w <= l.width && r.y + r.h <= l.height);
            assert!(!overlaps(*r, WHITE), "{i} covers the white texel");
            for (j, s) in rects.iter().enumerate().skip(i + 1) {
                assert!(!overlaps(*r, *s), "{i} and {j} overlap");
            }
            assert_eq!(packed.pixels.get(r.x, r.y), images[i].image.get(0, 0), "{i}'s texels");
            assert_eq!(packed.pixels.get(r.x + r.w - 1, r.y + r.h - 1), images[i].image.get(0, 0));
        }
        assert_eq!(packed.pixels.get(0, 0), Rgba::rgb(255, 255, 255));
        assert_eq!(pack(&images, 4096).layout, packed.layout, "deterministic");
    }

    #[test]
    fn atlas_leaves_out_what_does_not_fit() {
        let images = [
            img("a", 10, 10, Rgba::BLACK),
            img("huge", 5000, 2, Rgba::BLACK),
            img("b", 10, 3000, Rgba::BLACK),
        ];
        let packed = pack(&images, 2048);
        assert!(packed.layout.entry(AtlasId(0)).is_some());
        assert_eq!(packed.layout.entry(AtlasId(1)), None);
        assert_eq!(packed.layout.entry(AtlasId(2)), None);
        assert_eq!(packed.layout.entry(AtlasId(7)), None, "an unknown id");
        assert_eq!(packed.warnings.len(), 2);
        assert!(packed.layout.width <= 2048 && packed.layout.height <= 2048);
        let empty = pack(&[], 0);
        assert_eq!((empty.layout.width, empty.layout.height), (1, 1));
    }
}
