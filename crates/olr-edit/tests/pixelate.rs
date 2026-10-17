//! The image pipeline (plan 14a Task 4): resample, threshold, median cut, dither.

use olr_data::image::{Rgba, RgbaImage};
use olr_edit::pixelate::{Pixelation, palette_of, process};
use olr_format::levels::Dither;
use proptest::prelude::*;

fn p(pixel_size: u32, colours: Option<u16>, dither: Dither, alpha_threshold: u8) -> Pixelation {
    Pixelation { pixel_size, colours, dither, alpha_threshold }
}

fn gradient(w: u32, h: u32) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(
                x,
                y,
                Rgba { r: (x * 255 / w.max(1)) as u8, g: (y * 255 / h.max(1)) as u8, b: 99, a: 255 },
            );
        }
    }
    img
}

#[test]
fn pixel_size_makes_square_blocks() {
    let out = process(&gradient(64, 64), (12, 12), &p(4, None, Dither::None, 128));
    assert_eq!((out.width(), out.height()), (12, 12));
    for y in 0..12 {
        for x in 0..12 {
            assert_eq!(out.get(x, y), out.get(x / 4 * 4, y / 4 * 4), "({x}, {y})");
        }
    }
}

#[test]
fn the_threshold_makes_crisp_edges() {
    let mut img = RgbaImage::new(2, 1);
    img.set(0, 0, Rgba { r: 9, g: 9, b: 9, a: 127 });
    img.set(1, 0, Rgba { r: 9, g: 9, b: 9, a: 128 });
    let out = process(&img, (2, 1), &p(1, None, Dither::None, 128));
    assert_eq!(out.get(0, 0), Rgba { r: 0, g: 0, b: 0, a: 0 });
    assert_eq!(out.get(1, 0), Rgba { r: 9, g: 9, b: 9, a: 255 });
}

#[test]
fn reduction_never_exceeds_n_and_tolerates_empty_input() {
    for dither in [Dither::None, Dither::Ordered, Dither::Diffusion] {
        for n in [2u16, 3, 16, 256] {
            let out = process(&gradient(40, 30), (40, 30), &p(1, Some(n), dither, 1));
            assert!(palette_of(&out).len() <= n as usize, "{dither:?} {n}");
        }
        // Fewer distinct colours than asked: kept as they are.
        let mut two = RgbaImage::new(4, 1);
        for x in 0..4 {
            two.set(x, 0, if x < 2 { Rgba::rgb(255, 0, 0) } else { Rgba::rgb(0, 0, 255) });
        }
        assert_eq!(palette_of(&process(&two, (4, 1), &p(1, Some(16), dither, 1))).len(), 2);
        // Nothing covered, or nothing at all.
        let clear = RgbaImage::new(5, 5);
        assert!(palette_of(&process(&clear, (5, 5), &p(1, Some(4), dither, 128))).is_empty());
        let _ = process(&RgbaImage::new(0, 0), (3, 3), &p(1, Some(4), dither, 128));
    }
}

/// A grey band between black and white columns, reduced to two colours:
/// median cut keeps black and the mean of grey and white (165), so the
/// grey (120) lies between them. Without dither the band is one colour;
/// ordered and diffusion dither mix both.
#[test]
fn dither_mixes_the_two_nearest_colours() {
    let mut img = RgbaImage::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            let v = match x {
                0..4 => 0,
                12.. => 255,
                _ => 120,
            };
            img.set(x, y, Rgba::rgb(v, v, v));
        }
    }
    let interior = |out: &RgbaImage| {
        let mut seen = std::collections::BTreeSet::new();
        for y in 4..12 {
            for x in 4..12 {
                seen.insert(out.get(x, y).r);
            }
        }
        seen
    };
    let flat = process(&img, (16, 16), &p(1, Some(2), Dither::None, 1));
    assert_eq!(interior(&flat).len(), 1, "no dither: one colour");
    for dither in [Dither::Ordered, Dither::Diffusion] {
        let out = process(&img, (16, 16), &p(1, Some(2), dither, 1));
        assert_eq!(palette_of(&out).len(), 2);
        assert_eq!(interior(&out).len(), 2, "{dither:?}: both colours in the grey area");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn process_is_total_and_sized(ow in 0u32..20, oh in 0u32..20, aw in 1u32..30, ah in 1u32..30,
                                  ps in 1u32..6, n in proptest::option::of(2u16..40), d in 0u8..3, t in 0u8..=255) {
        let dither = [Dither::None, Dither::Ordered, Dither::Diffusion][d as usize];
        let out = process(&gradient(ow, oh), (aw, ah), &p(ps, n, dither, t));
        prop_assert_eq!((out.width(), out.height()), (aw, ah));
        prop_assert!(out.as_raw().chunks(4).all(|px| px[3] == 0 || px[3] == 255));
        if let Some(n) = n { prop_assert!(palette_of(&out).len() <= n as usize); }
    }
}

/// Batch B review: median cut with skewed counts (one colour far more
/// common) keeps every colour when they fit, exactly.
#[test]
fn skewed_counts_keep_every_colour_that_fits() {
    for (reds, n) in [(3u32, 2u16), (1000, 2), (1000, 4)] {
        let mut img = RgbaImage::new(reds + 1, 1);
        for x in 0..reds {
            img.set(x, 0, Rgba::rgb(255, 0, 0));
        }
        img.set(reds, 0, Rgba::rgb(0, 0, 255));
        let out = process(&img, (reds + 1, 1), &p(1, Some(n), Dither::None, 1));
        assert_eq!(
            palette_of(&out),
            [Rgba::rgb(0, 0, 255), Rgba::rgb(255, 0, 0)],
            "{reds} reds, {n} colours"
        );
        assert_eq!(out.get(reds, 0), Rgba::rgb(0, 0, 255));
    }
    // More colours than asked: no box of a split shares a channel value
    // with the other, so a rare colour is not averaged into a common one.
    let mut img = RgbaImage::new(1002, 1);
    for x in 0..1000 {
        img.set(x, 0, Rgba::rgb(255, 0, 0));
    }
    img.set(1000, 0, Rgba::rgb(0, 0, 255));
    img.set(1001, 0, Rgba::rgb(0, 255, 0));
    let out = process(&img, (1002, 1), &p(1, Some(2), Dither::None, 1));
    assert!(palette_of(&out).contains(&Rgba::rgb(255, 0, 0)), "{:?}", palette_of(&out));
}
