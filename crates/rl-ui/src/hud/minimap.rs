//! The minimap's picture and marks (`docs/porting/olx-notes/presentation.md`
//! §2.4): the level's colour picture scaled to the minimap, drawn at half
//! opacity, and a dot per alive worm that brightens toward white over
//! OLX's half-second blink.

use egui::{Color32, ColorImage, Pos2, Rect, Vec2, vec2};

/// The minimap texture's size: OLX's 119 × 94 twice over, so it stays
/// sharp at the HUD's usual scales.
pub const TEXTURE_SIZE: [usize; 2] = [238, 188];

/// OLX blits the minimap with per-surface alpha 128 (`DrawMiniMap`,
/// `CMap.cpp:1774-1816`).
pub const ALPHA: u8 = 128;

/// OLX's blink period: the sawtooth restarts after 0.5 s.
pub const BLINK_PERIOD: f32 = 0.5;

/// Samples per minimap pixel along each axis, at most (a box filter, as
/// OLX's default `Game.Antialiasing` resamples).
const SAMPLES: usize = 4;

/// The `width` × `height` RGBA picture `pixels` scaled to `size`
/// (non-uniformly, as OLX's), each pixel the mean of up to 4 × 4 samples
/// of its area. A picture whose bytes do not match its size gives black.
///
/// OLX: src/common/level.cpp:486-527 (`UpdateMiniMap`'s resampling).
pub fn image(pixels: &[u8], width: u32, height: u32, size: [usize; 2]) -> ColorImage {
    let (w, h) = (width as usize, height as usize);
    let [mw, mh] = size;
    let mut out = ColorImage::filled(size, Color32::BLACK);
    if w == 0 || h == 0 || mw == 0 || mh == 0 || pixels.len() != w * h * 4 {
        return out;
    }
    let (sx, sy) = (SAMPLES.min(w.div_ceil(mw)).max(1), SAMPLES.min(h.div_ceil(mh)).max(1));
    for my in 0..mh {
        for mx in 0..mw {
            let mut sum = [0u32; 3];
            for j in 0..sy {
                // Sample centres spread over the pixel's area.
                let y = ((my * sy + j) * 2 + 1) * h / (mh * sy * 2);
                for i in 0..sx {
                    let x = ((mx * sx + i) * 2 + 1) * w / (mw * sx * 2);
                    let p = (y.min(h - 1) * w + x.min(w - 1)) * 4;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(pixels[p + c]);
                    }
                }
            }
            let n = (sx * sy) as u32;
            let [r, g, b] = sum.map(|s| (s / n) as u8);
            out.pixels[my * mw + mx] = Color32::from_rgb(r, g, b);
        }
    }
    out
}

/// A worm's dot colour at `blink` seconds into the sawtooth: `c + (255 -
/// c) * blink * 2` per channel (`drawOnMiniMap`, `CMap.cpp:1708-1770`).
pub fn dot_colour(c: Color32, blink: f32) -> Color32 {
    let t = (blink * 2.0).clamp(0.0, 1.0);
    let up = |v: u8| v.saturating_add((f32::from(255 - v) * t) as u8);
    Color32::from_rgb(up(c.r()), up(c.g()), up(c.b()))
}

/// Where a worm at `pos` (world pixels) of a `map`-sized level shows on a
/// minimap drawn in `rect`: OLX's minimap pixel `floor(pos / step)`,
/// clamped, then a 2 × 2 block (3 × 3 for the local human) from the
/// pixel up and left, scaled to `rect`.
///
/// OLX: src/common/CMap.cpp:1708-1770 (`drawOnMiniMap`).
pub fn dot_rect(pos: Pos2, map: Vec2, rect: Rect, big: bool) -> Rect {
    let olx = super::layout::MINIMAP_OLX;
    let px = |p: f32, size: f32, cells: f32| {
        let step = if size > 0.0 { size / cells } else { 1.0 };
        let v = (p / step).floor();
        if v.is_finite() { v.clamp(0.0, cells - 1.0) } else { 0.0 }
    };
    let (i, j) = (px(pos.x, map.x, olx.x), px(pos.y, map.y, olx.y));
    let k = rect.width() / olx.x;
    let (x, y) = ((i - 1.0).max(0.0), (j - 1.0).max(0.0));
    let n = if big { (i + 2.0) - x } else { 2.0 };
    let m = if big { (j + 2.0) - y } else { 2.0 };
    Rect::from_min_size(rect.min + vec2(x, y) * k, vec2(n, m) * k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn the_picture_is_box_filtered_to_the_minimap() {
        // 8 × 4: left half red, right half blue, to 2 × 1.
        let mut px = Vec::new();
        for _y in 0..4 {
            for x in 0..8 {
                px.extend_from_slice(if x < 4 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
            }
        }
        let img = image(&px, 8, 4, [2, 1]);
        assert_eq!(img.pixels, [Color32::from_rgb(255, 0, 0), Color32::from_rgb(0, 0, 255)]);
        // To 1 × 1: the mean.
        assert_eq!(image(&px, 8, 4, [1, 1]).pixels, [Color32::from_rgb(127, 0, 127)]);
        // Bad sizes give black, never a panic.
        assert_eq!(image(&px[1..], 8, 4, [2, 2]).pixels, [Color32::BLACK; 4]);
        assert_eq!(image(&[], 0, 0, [2, 1]).pixels.len(), 2);
    }

    #[test]
    fn dots_follow_olx() {
        let c = Color32::from_rgb(255, 0, 0);
        assert_eq!(dot_colour(c, 0.0), c);
        assert_eq!(dot_colour(c, 0.25), Color32::from_rgb(255, 127, 127));
        assert_eq!(dot_colour(c, 0.5), Color32::WHITE);
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(119.0, 94.0));
        let map = vec2(1190.0, 940.0);
        // OLX pixel (5, 7): a 2 × 2 block from (4, 6); big: 3 × 3.
        assert_eq!(
            dot_rect(pos2(55.0, 75.0), map, rect, false),
            Rect::from_min_size(pos2(4.0, 6.0), vec2(2.0, 2.0))
        );
        assert_eq!(
            dot_rect(pos2(55.0, 75.0), map, rect, true),
            Rect::from_min_size(pos2(4.0, 6.0), vec2(3.0, 3.0))
        );
        // Clamped to the minimap; twice the size, twice the dot.
        let r2 = Rect::from_min_size(Pos2::ZERO, vec2(238.0, 188.0));
        assert_eq!(
            dot_rect(pos2(-50.0, 1e9), map, r2, false),
            Rect::from_min_size(pos2(0.0, 184.0), vec2(4.0, 4.0))
        );
        assert_eq!(dot_rect(pos2(f32::NAN, 0.0), vec2(0.0, 0.0), rect, false).min, Pos2::ZERO);
    }
}
