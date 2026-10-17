//! Level thumbnails for the menus (spec §21 G6): the level's picture
//! scaled down into [`THUMB_SIZE`], keeping its aspect. Moved here from
//! `olr-import-gfx` (plan 13d): the importer and the level operations
//! both make them.
//!
//! OLX's level list shows the level's minimap as its preview
//! (`minimapForLevel`, `src/game/Level.cpp:33-44`; `MapLoad::getMinimap`,
//! `src/common/MapLoader.cpp:79-104`): the drawn image resampled into
//! 128 × 96 (`CMap.h:86-87`), stretched (`gusUpdateMinimap`,
//! `src/gusanos/level.cpp:486-527`). The remake letterboxes it into
//! 160 × 96 instead, so wide and tall levels keep their shape
//! (`docs/porting/STATUS.md`).

use olr_data::image::RgbaImage;

/// The size thumbnails are made at: the level letterboxed into it.
/// `olr-pack` re-exports it.
pub const THUMB_SIZE: (u32, u32) = (160, 96);

/// Samples per thumbnail pixel along each axis, at most: a box filter of
/// bounded cost however large the art (OLX's default `Game.Antialiasing`
/// resamples).
const SAMPLES: u64 = 8;

/// The rectangle `(x, y, w, h)` an `aw` × `ah` picture fills inside a
/// `tw` × `th` box at the largest scale that keeps its aspect, centred.
/// Each side is at least 1 pixel.
pub fn letterbox(aw: u32, ah: u32, tw: u32, th: u32) -> (u32, u32, u32, u32) {
    if aw == 0 || ah == 0 || tw == 0 || th == 0 {
        return (0, 0, tw, th);
    }
    // Width-bound when aw / ah >= tw / th.
    let (w, h) = if u64::from(aw) * u64::from(th) >= u64::from(ah) * u64::from(tw) {
        let h = (u64::from(tw) * u64::from(ah) + u64::from(aw) / 2) / u64::from(aw);
        (tw, (h as u32).clamp(1, th))
    } else {
        let w = (u64::from(th) * u64::from(aw) + u64::from(ah) / 2) / u64::from(ah);
        ((w as u32).clamp(1, tw), th)
    };
    ((tw - w) / 2, (th - h) / 2, w, h)
}

/// The thumbnail of a level whose visible picture is `front` (which
/// equals `back` where the material is background): box-filtered into
/// [`THUMB_SIZE`], letterboxed with transparent bars. Opaque inside;
/// never panics, whatever the art's size.
pub fn thumbnail(front: &RgbaImage) -> RgbaImage {
    let (tw, th) = THUMB_SIZE;
    scaled(front, tw, th)
}

/// `art` box-filtered into the [`letterbox`] of a `tw` × `th` image.
fn scaled(art: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let mut out = RgbaImage::new(tw, th);
    let (aw, ah) = (art.width(), art.height());
    if aw == 0 || ah == 0 || tw == 0 || th == 0 {
        return out;
    }
    let (ox, oy, w, h) = letterbox(aw, ah, tw, th);
    let (aw64, ah64, w64, h64) = (u64::from(aw), u64::from(ah), u64::from(w), u64::from(h));
    let sx = SAMPLES.min(aw64.div_ceil(w64)).max(1);
    let sy = SAMPLES.min(ah64.div_ceil(h64)).max(1);
    let raw = art.as_raw();
    for ty in 0..h64 {
        for tx in 0..w64 {
            let mut sum = [0u64; 3];
            for j in 0..sy {
                // Sample centres spread over the pixel's area.
                let y = (((ty * sy + j) * 2 + 1) * ah64 / (h64 * sy * 2)).min(ah64 - 1);
                for i in 0..sx {
                    let x = (((tx * sx + i) * 2 + 1) * aw64 / (w64 * sx * 2)).min(aw64 - 1);
                    let p = ((y * aw64 + x) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u64::from(raw[p + c]);
                    }
                }
            }
            let n = sx * sy;
            let [r, g, b] = sum.map(|s| (s / n) as u8);
            out.set(ox + tx as u32, oy + ty as u32, olr_data::image::Rgba::rgb(r, g, b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::image::Rgba;

    /// The front art of a `w` × `h` level at `art_scale`.
    fn front(w: u32, h: u32, art_scale: u32, paint: impl Fn(u32, u32) -> Rgba) -> RgbaImage {
        let mut front = RgbaImage::new(w * art_scale, h * art_scale);
        for y in 0..h * art_scale {
            for x in 0..w * art_scale {
                front.set(x, y, paint(x, y));
            }
        }
        front
    }

    /// The opaque area of `img`: `(x0, y0, x1, y1)`, exclusive.
    fn opaque(img: &RgbaImage) -> (u32, u32, u32, u32) {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..img.height() {
            for x in 0..img.width() {
                if img.get(x, y).a == 255 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        (x0, y0, x1, y1)
    }

    #[test]
    fn letterbox_keeps_the_aspect() {
        assert_eq!(letterbox(160, 96, 160, 96), (0, 0, 160, 96));
        assert_eq!(letterbox(504, 350, 160, 96), (11, 0, 138, 96), "Dirt Level");
        assert_eq!(letterbox(1350, 550, 160, 96), (0, 15, 160, 65), "Castle Strike");
        assert_eq!(letterbox(1, 1000, 160, 96), (79, 0, 1, 96), "a thread stays a pixel wide");
        assert_eq!(letterbox(1000, 1, 160, 96), (0, 47, 160, 1));
        assert_eq!(letterbox(0, 5, 160, 96), (0, 0, 160, 96));
    }

    /// Wide levels get bars above and below, tall ones beside, all of the
    /// thumbnail's size; the picture inside is the level's colours.
    #[test]
    fn thumbnails_are_letterboxed_to_a_fixed_size() {
        let red = Rgba::rgb(200, 10, 10);
        for (w, h, scale) in [(1350, 550, 1), (504, 350, 2), (200, 900, 1), (1, 1, 1), (3, 2, 8)] {
            let t = thumbnail(&front(w, h, scale, |_, _| red));
            assert_eq!((t.width(), t.height()), THUMB_SIZE);
            let (x, y, lw, lh) = letterbox(w * scale, h * scale, 160, 96);
            assert_eq!(opaque(&t), (x, y, x + lw, y + lh), "{w}x{h} x{scale}");
            assert_eq!(t.get(x + lw / 2, y + lh / 2), red);
            if lw < 160 {
                assert_eq!(t.get(0, 48).a, 0, "{w}x{h}: bars beside");
            }
            if lh < 96 {
                assert_eq!(t.get(80, 0).a, 0, "{w}x{h}: bars above");
            }
        }
    }

    /// Each pixel is the mean of its area: a fine checkerboard turns grey,
    /// and the left and right halves keep their colours.
    #[test]
    fn thumbnails_average_the_art() {
        let t = thumbnail(&front(640, 384, 1, |x, y| {
            if (x + y) % 2 == 0 { Rgba::rgb(200, 200, 200) } else { Rgba::rgb(0, 0, 0) }
        }));
        assert_eq!(t.get(80, 48), Rgba::rgb(100, 100, 100));
        let t = thumbnail(&front(640, 384, 1, |x, _| {
            if x < 320 { Rgba::rgb(0, 0, 255) } else { Rgba::rgb(255, 255, 255) }
        }));
        assert_eq!((t.get(10, 10), t.get(150, 90)), (Rgba::rgb(0, 0, 255), Rgba::rgb(255, 255, 255)));
    }
}
