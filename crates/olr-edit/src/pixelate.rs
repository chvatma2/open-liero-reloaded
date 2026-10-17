//! The image pipeline of an image element (plan 14a, spec §3): resample
//! the original into the element's pixel grid by area averaging, threshold
//! its alpha, reduce its colours by median cut, dither, and scale it up by
//! `pixel_size`. Integer arithmetic only: the same settings give the same
//! pixels on every platform.

use olr_data::image::{Rgba, RgbaImage};
use olr_format::levels::Dither;

/// An image element's pixelation settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pixelation {
    /// Art pixels per element pixel, at least 1.
    pub pixel_size: u32,
    /// Colour reduction: none, or 2 to 256 colours.
    pub colours: Option<u16>,
    /// Dithering when colours are reduced.
    pub dither: Dither,
    /// Alpha at or above which a pixel is covered.
    pub alpha_threshold: u8,
}

/// The standard 8 × 8 Bayer matrix (0 to 63).
const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// `original` processed into a box of `art_size` art pixels: resampled to
/// the element grid (`ceil(art_size / pixel_size)`) by area averaging,
/// thresholded, colour-reduced and dithered, then scaled up by
/// `pixel_size` (nearest) and cropped to `art_size`. Alpha is 0 or 255.
/// Integer arithmetic only.
pub fn process(original: &RgbaImage, art_size: (u32, u32), p: &Pixelation) -> RgbaImage {
    let ps = p.pixel_size.max(1);
    let grid = process_grid(original, art_size, p);
    let mut out = RgbaImage::new(art_size.0, art_size.1);
    for y in 0..art_size.1 {
        for x in 0..art_size.0 {
            out.set(x, y, grid.get(x / ps, y / ps));
        }
    }
    out
}

/// [`process`] before the scale up: the element grid, `ceil(art_size /
/// pixel_size)` pixels. Art pixel `(x, y)` of the box is grid pixel
/// `(x / pixel_size, y / pixel_size)`.
pub(crate) fn process_grid(original: &RgbaImage, art_size: (u32, u32), p: &Pixelation) -> RgbaImage {
    let ps = p.pixel_size.max(1);
    let (gw, gh) = (art_size.0.div_ceil(ps), art_size.1.div_ceil(ps));
    if gw == 0 || gh == 0 || original.width() == 0 || original.height() == 0 {
        return RgbaImage::new(gw, gh);
    }
    let mut grid = resample(original, gw, gh);
    threshold(&mut grid, p.alpha_threshold);
    if let Some(n) = p.colours {
        let palette = median_cut(&grid, usize::from(n.max(1)));
        if !palette.is_empty() {
            match p.dither {
                Dither::None => map_nearest(&mut grid, &palette),
                Dither::Ordered => dither_ordered(&mut grid, &palette, n),
                Dither::Diffusion => dither_diffusion(&mut grid, &palette),
            }
        }
    }
    grid
}

/// The distinct opaque colours of `img` (alpha 255), sorted: for tests.
pub fn palette_of(img: &RgbaImage) -> Vec<Rgba> {
    let mut seen = std::collections::BTreeSet::new();
    for px in img.as_raw().chunks(4) {
        if px[3] == 255 {
            seen.insert((px[0], px[1], px[2]));
        }
    }
    seen.into_iter().map(|(r, g, b)| Rgba::rgb(r, g, b)).collect()
}

/// The source span `[from, to)` of grid cell `g` of `cells` over `side`
/// source pixels: at least one pixel, within the image.
fn span(g: u32, cells: u32, side: u32) -> (u32, u32) {
    let at = |g: u32| (u64::from(g) * u64::from(side) / u64::from(cells)) as u32;
    let from = at(g).min(side - 1);
    (from, at(g + 1).max(from + 1).min(side))
}

/// Area-averages `img` into `gw` × `gh`: colour weighted by alpha
/// (`Σ c·a / Σ a`, 0 when `Σ a = 0`), alpha the mean.
fn resample(img: &RgbaImage, gw: u32, gh: u32) -> RgbaImage {
    let (ow, oh) = (img.width(), img.height());
    let raw = img.as_raw();
    let mut out = RgbaImage::new(gw, gh);
    for gy in 0..gh {
        let (y0, y1) = span(gy, gh, oh);
        for gx in 0..gw {
            let (x0, x1) = span(gx, gw, ow);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                let row = (y as usize * ow as usize) * 4;
                for x in x0..x1 {
                    let i = row + x as usize * 4;
                    let pa = u64::from(raw[i + 3]);
                    r += u64::from(raw[i]) * pa;
                    g += u64::from(raw[i + 1]) * pa;
                    b += u64::from(raw[i + 2]) * pa;
                    a += pa;
                    n += 1;
                }
            }
            let c = |sum: u64| sum.checked_div(a).unwrap_or(0) as u8;
            out.set(gx, gy, Rgba { r: c(r), g: c(g), b: c(b), a: (a / n.max(1)) as u8 });
        }
    }
    out
}

/// Covered (alpha 255) when alpha is at least `t`; otherwise fully clear.
fn threshold(img: &mut RgbaImage, t: u8) {
    for y in 0..img.height() {
        for x in 0..img.width() {
            let p = img.get(x, y);
            img.set(x, y, if p.a >= t { Rgba { a: 255, ..p } } else { Rgba::TRANSPARENT });
        }
    }
}

fn covered(img: &RgbaImage) -> impl Iterator<Item = [u8; 3]> + '_ {
    img.as_raw().chunks(4).filter(|p| p[3] == 255).map(|p| [p[0], p[1], p[2]])
}

/// A median-cut box: a range of the colour list, and its widest channel
/// and that channel's range.
struct ColourBox {
    from: usize,
    to: usize,
    channel: usize,
    range: u8,
}

fn colour_box(colours: &[[u8; 3]], from: usize, to: usize) -> ColourBox {
    let (mut channel, mut range) = (0, 0);
    for c in 0..3 {
        let (lo, hi) =
            colours[from..to].iter().fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[c]), hi.max(p[c])));
        let r = hi.saturating_sub(lo);
        if r > range {
            (channel, range) = (c, r);
        }
    }
    ColourBox { from, to, channel, range }
}

/// At most `n` colours for the covered pixels of `img`. When their
/// distinct colours fit, those exactly (sorted). Otherwise median cut:
/// split the box whose widest channel range is largest (ties: the earlier
/// box) at the median of that channel, moved to the nearest boundary
/// between two values of it (ties: the lower), so no value of the channel
/// lies in both halves; until there are `n` boxes or none has a range.
/// Each box's colour is its integer mean. Empty when nothing is covered.
fn median_cut(img: &RgbaImage, n: usize) -> Vec<[u8; 3]> {
    let mut colours: Vec<[u8; 3]> = covered(img).collect();
    let distinct: std::collections::BTreeSet<[u8; 3]> = colours.iter().copied().collect();
    if distinct.len() <= n {
        return distinct.into_iter().collect();
    }
    let mut boxes = vec![colour_box(&colours, 0, colours.len())];
    while boxes.len() < n {
        let Some(i) = (0..boxes.len())
            .filter(|&i| boxes[i].range > 0)
            .max_by_key(|&i| (boxes[i].range, usize::MAX - i))
        else {
            break;
        };
        let ColourBox { from, to, channel, .. } = boxes[i];
        colours[from..to].sort_unstable_by_key(|p| (p[channel], p[0], p[1], p[2]));
        let median = from + (to - from) / 2;
        // The box has a range in `channel`, so a boundary exists.
        let mid = (from + 1..to)
            .filter(|&j| colours[j - 1][channel] != colours[j][channel])
            .min_by_key(|&j| (j.abs_diff(median), j))
            .unwrap_or(median);
        boxes[i] = colour_box(&colours, from, mid);
        boxes.insert(i + 1, colour_box(&colours, mid, to));
    }
    boxes
        .iter()
        .map(|b| {
            let len = (b.to - b.from) as u64;
            let mean =
                |c: usize| (colours[b.from..b.to].iter().map(|p| u64::from(p[c])).sum::<u64>() / len) as u8;
            [mean(0), mean(1), mean(2)]
        })
        .collect()
}

/// A palette with its entries ordered by red, for [`Palette::nearest`].
struct Palette<'a> {
    colours: &'a [[u8; 3]],
    /// `(red, index)`, sorted.
    by_red: Vec<(i64, usize)>,
}

impl<'a> Palette<'a> {
    fn new(colours: &'a [[u8; 3]]) -> Self {
        let mut by_red: Vec<(i64, usize)> =
            colours.iter().enumerate().map(|(i, c)| (i64::from(c[0]), i)).collect();
        by_red.sort_unstable();
        Palette { colours, by_red }
    }

    /// The index nearest `c` (squared RGB distance; ties: the lowest
    /// index); `c` and the palette in units of `scale`. Exact: entries
    /// are searched outward from `c`'s red, each way until the red
    /// distance alone exceeds the best distance.
    fn nearest(&self, c: [i64; 3], scale: i64) -> usize {
        let start = self.by_red.partition_point(|&(r, _)| r * scale < c[0]);
        let mut best = (i64::MAX, usize::MAX);
        let visit = |&(r, i): &(i64, usize), best: &mut (i64, usize)| {
            let dr = (c[0] - r * scale).pow(2);
            if dr > best.0 {
                return false;
            }
            let p = self.colours[i];
            let d = dr + (c[1] - i64::from(p[1]) * scale).pow(2) + (c[2] - i64::from(p[2]) * scale).pow(2);
            if (d, i) < *best {
                *best = (d, i);
            }
            true
        };
        for e in &self.by_red[start..] {
            if !visit(e, &mut best) {
                break;
            }
        }
        for e in self.by_red[..start].iter().rev() {
            if !visit(e, &mut best) {
                break;
            }
        }
        best.1
    }
}

fn set_rgb(img: &mut RgbaImage, x: u32, y: u32, c: [u8; 3]) {
    img.set(x, y, Rgba::rgb(c[0], c[1], c[2]));
}

fn map_nearest(img: &mut RgbaImage, palette: &[[u8; 3]]) {
    let index = Palette::new(palette);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let p = img.get(x, y);
            if p.a == 255 {
                let i = index.nearest([p.r, p.g, p.b].map(i64::from), 1);
                set_rgb(img, x, y, palette[i]);
            }
        }
    }
}

/// The smallest `k` with `k³ ≥ n`.
fn cbrt_ceil(n: u16) -> i64 {
    (1..).find(|k: &i64| k * k * k >= i64::from(n)).unwrap_or(1)
}

/// Bayer 8 × 8: a bias of `(B·2 − 63) · spread / 128` on each channel, with
/// `spread = 255 / cbrt_ceil(n)`, before the nearest colour.
fn dither_ordered(img: &mut RgbaImage, palette: &[[u8; 3]], n: u16) {
    let spread = 255 / cbrt_ceil(n);
    let index = Palette::new(palette);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let p = img.get(x, y);
            if p.a != 255 {
                continue;
            }
            let bias = (i64::from(BAYER8[(y % 8) as usize][(x % 8) as usize]) * 2 - 63) * spread / 128;
            let c = [p.r, p.g, p.b].map(|v| (i64::from(v) + bias).clamp(0, 255));
            set_rgb(img, x, y, palette[index.nearest(c, 1)]);
        }
    }
}

/// Floyd-Steinberg in raster order: each covered pixel's error, per
/// channel, goes 7/16 right, 3/16 down-left, 5/16 down and 1/16
/// down-right, to covered neighbours only. Errors are kept in sixteenths,
/// so small errors add up instead of rounding away; only this row's and
/// the next row's are held.
fn dither_diffusion(img: &mut RgbaImage, palette: &[[u8; 3]]) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let index = Palette::new(palette);
    // This row's errors, then the next row's.
    let mut err = [vec![[0i64; 3]; w], vec![[0i64; 3]; w]];
    for y in 0..h {
        err.swap(0, 1);
        err[1].fill([0; 3]);
        for x in 0..w {
            let p = img.get(x as u32, y as u32);
            if p.a != 255 {
                continue;
            }
            let c: [i64; 3] = std::array::from_fn(|k| {
                (i64::from([p.r, p.g, p.b][k]) * 16 + err[0][x][k]).clamp(0, 255 * 16)
            });
            let chosen = palette[index.nearest(c, 16)];
            set_rgb(img, x as u32, y as u32, chosen);
            let e: [i64; 3] = std::array::from_fn(|k| c[k] - i64::from(chosen[k]) * 16);
            for (dx, dy, weight) in [(1isize, 0usize, 7i64), (-1, 1, 3), (0, 1, 5), (1, 1, 1)] {
                let (Some(nx), ny) = (x.checked_add_signed(dx), y + dy) else { continue };
                if nx >= w || ny >= h || img.get(nx as u32, ny as u32).a != 255 {
                    continue;
                }
                for k in 0..3 {
                    err[dy][nx][k] += e[k] * weight / 16;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::Palette;

    /// The pruned search against every entry, ties on the lowest index.
    fn brute(palette: &[[u8; 3]], c: [i64; 3], scale: i64) -> usize {
        let d = |p: &[u8; 3]| (0..3).map(|k| (c[k] - i64::from(p[k]) * scale).pow(2)).sum::<i64>();
        (0..palette.len()).min_by_key(|&i| (d(&palette[i]), i)).unwrap_or(usize::MAX)
    }

    proptest! {
        #[test]
        fn nearest_is_exact(palette in prop::collection::vec(any::<[u8; 3]>(), 1..40),
                            c in any::<[u8; 3]>(), e in any::<[i8; 3]>(), sixteenths in any::<bool>()) {
            let scale = if sixteenths { 16 } else { 1 };
            let c: [i64; 3] = std::array::from_fn(|k| (i64::from(c[k]) * scale + i64::from(e[k])).clamp(0, 255 * scale));
            // Duplicates make ties.
            let mut palette = palette;
            palette.push(palette[0]);
            prop_assert_eq!(Palette::new(&palette).nearest(c, scale), brute(&palette, c, scale));
        }
    }
}
