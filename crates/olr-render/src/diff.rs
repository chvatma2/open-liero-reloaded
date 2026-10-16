//! Comparing rendered images with goldens: a perceptual colour distance,
//! so two software rasterisers that round a blend differently still agree.

use olr_data::image::RgbaImage;

/// How far apart two images of the same size are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageDiff {
    /// Pixels whose distance is above the tolerance.
    pub differing: u64,
    pub total: u64,
    /// The largest distance, 0 (same) to about 1 (black and white).
    pub max_distance: f32,
}

impl ImageDiff {
    /// The share of differing pixels.
    pub fn fraction(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.differing as f64 / self.total as f64 }
    }
}

/// The YIQ distance of two opaque colours (pixelmatch's measure, after
/// Kotsarenko and Ramos), scaled to 0..1.
fn distance(a: &[u8; 4], b: &[u8; 4]) -> f32 {
    // Blend each over white by its alpha, as pixelmatch does.
    let rgb = |p: &[u8; 4]| {
        let a = f32::from(p[3]) / 255.0;
        [0, 1, 2].map(|i| 255.0 + (f32::from(p[i]) - 255.0) * a)
    };
    let ([r1, g1, b1], [r2, g2, b2]) = (rgb(a), rgb(b));
    let y = |r: f32, g: f32, b: f32| r * 0.298_895_3 + g * 0.586_622_5 + b * 0.114_482_2;
    let i = |r: f32, g: f32, b: f32| r * 0.595_978 - g * 0.274_176_1 - b * 0.321_801_9;
    let q = |r: f32, g: f32, b: f32| r * 0.211_470_2 - g * 0.522_617_1 + b * 0.311_146_9;
    let dy = y(r1, g1, b1) - y(r2, g2, b2);
    let di = i(r1, g1, b1) - i(r2, g2, b2);
    let dq = q(r1, g1, b1) - q(r2, g2, b2);
    ((0.5053 * dy * dy + 0.299 * di * di + 0.1957 * dq * dq) / 35215.0).sqrt()
}

/// Compares `a` with `b`: a pixel differs when its distance is above
/// `tolerance` (0..1; 0.1 hides rounding, not a changed sprite). `None`
/// when the sizes differ.
pub fn perceptual_diff(a: &RgbaImage, b: &RgbaImage, tolerance: f32) -> Option<ImageDiff> {
    if (a.width(), a.height()) != (b.width(), b.height()) {
        return None;
    }
    let mut d = ImageDiff { differing: 0, total: 0, max_distance: 0.0 };
    for (p, q) in a.as_raw().as_chunks::<4>().0.iter().zip(b.as_raw().as_chunks::<4>().0) {
        d.total += 1;
        if p == q {
            continue;
        }
        let dist = distance(p, q);
        d.max_distance = d.max_distance.max(dist);
        if dist > tolerance {
            d.differing += 1;
        }
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::image::Rgba;

    fn image(c: Rgba) -> RgbaImage {
        let mut img = RgbaImage::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                img.set(x, y, c);
            }
        }
        img
    }

    #[test]
    fn perceptual_diff_ignores_rounding_but_not_changes() {
        let a = image(Rgba::rgb(100, 120, 140));
        let mut b = image(Rgba::rgb(101, 119, 141));
        let d = perceptual_diff(&a, &b, 0.1).unwrap();
        assert_eq!((d.differing, d.total), (0, 100));
        assert!(d.max_distance > 0.0 && d.max_distance < 0.02, "{d:?}");
        b.set(3, 3, Rgba::rgb(255, 0, 255));
        let d = perceptual_diff(&a, &b, 0.1).unwrap();
        assert_eq!(d.differing, 1);
        assert!((d.fraction() - 0.01).abs() < 1e-9);
        let black_white =
            perceptual_diff(&image(Rgba::BLACK), &image(Rgba::rgb(255, 255, 255)), 0.0).unwrap();
        assert!(black_white.max_distance > 0.95, "{black_white:?}");
        assert_eq!(perceptual_diff(&a, &RgbaImage::new(9, 10), 0.1), None);
    }
}
