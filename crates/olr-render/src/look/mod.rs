//! The look's CPU side (plan 15a): the per-texel model the composite pass
//! draws, as a reference for tests, and the light grid's description.

use olr_look::{Grade, srgb_to_linear};

/// The view's light, one texel per world pixel of the world target (the
/// view plus `MARGIN`), RGBA16Float, rebuilt every lit frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightGrid {
    /// World pixel of texel (0, 0).
    pub origin: (i32, i32),
    pub size: (u32, u32),
}

/// Linear light as an sRGB byte.
pub fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8
}

/// The grade on a linear colour: exposure (`2^(stops)` given as the
/// multiplier), contrast around 0.5, saturation around Rec. 709 luma, then
/// the tint (linear).
pub fn grade_linear(c: [f32; 3], exposure: f32, contrast: f32, saturation: f32, tint: [f32; 3]) -> [f32; 3] {
    let c = c.map(|v| v * exposure);
    let c = c.map(|v| (v - 0.5) * contrast + 0.5);
    let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let c = c.map(|v| luma + (v - luma) * saturation);
    [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2]]
}

/// One texel of the composite (spec §The picture): `albedo` (sRGB bytes)
/// lit by `light` (linear, capped at `max_light`), showing at least
/// `self_light` (0 to 1), graded; a `marker` shows its albedo as it is.
pub fn shade_texel(
    albedo: [u8; 4],
    light: [f32; 3],
    self_light: f32,
    marker: bool,
    grade: &Grade,
    max_light: f32,
) -> [u8; 4] {
    if marker {
        return [albedo[0], albedo[1], albedo[2], 255];
    }
    let a = [albedo[0], albedo[1], albedo[2]].map(srgb_to_linear);
    let l = light.map(|v| v.min(max_light).max(self_light));
    let shown = [a[0] * l[0], a[1] * l[1], a[2] * l[2]];
    let tint = grade.tint.map(srgb_to_linear);
    let exposure = 2f32.powf(f32::from(grade.exposure) / 100.0);
    let g = grade_linear(
        shown,
        exposure,
        f32::from(grade.contrast) / 100.0,
        f32::from(grade.saturation) / 100.0,
        tint,
    );
    [linear_to_srgb(g[0]), linear_to_srgb(g[1]), linear_to_srgb(g[2]), 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shade_texel_table() {
        let none = Grade::default();
        // Full light is identity for every byte.
        for v in 0..=255u8 {
            let c = [v, 255 - v, v / 3, 255];
            assert_eq!(shade_texel(c, [1.0; 3], 0.0, false, &none, 1.0), c, "{v}");
            // Self-light 1 is identity in the dark.
            assert_eq!(shade_texel(c, [0.0; 3], 1.0, false, &none, 1.0), c, "{v}");
        }
        // No light, no self-light: black.
        assert_eq!(shade_texel([200, 100, 50, 255], [0.0; 3], 0.0, false, &none, 1.0), [0, 0, 0, 255]);
        // Light is capped at max light.
        let c = [128, 128, 128, 255];
        assert_eq!(shade_texel(c, [5.0; 3], 0.0, false, &none, 1.0), c);
        let bright = shade_texel(c, [5.0; 3], 0.0, false, &none, 2.0);
        assert_eq!(bright[0], linear_to_srgb(srgb_to_linear(128) * 2.0), "over-bright doubles the light");
        // Markers ignore light and grade.
        let dark = Grade { exposure: -200, contrast: 0, saturation: 0, tint: [0, 0, 0] };
        assert_eq!(shade_texel([9, 8, 7, 255], [0.0; 3], 0.0, true, &dark, 1.0), [9, 8, 7, 255]);
        // Exposure +100 doubles a mid grey in linear.
        let up = Grade { exposure: 100, ..none };
        let grey = shade_texel([100, 100, 100, 255], [1.0; 3], 0.0, false, &up, 1.0);
        let want = linear_to_srgb(srgb_to_linear(100) * 2.0);
        assert_eq!(grey, [want, want, want, 255]);
        // Saturation 0 is grey; a tint multiplies.
        let flat =
            shade_texel([200, 40, 40, 255], [1.0; 3], 0.0, false, &Grade { saturation: 0, ..none }, 1.0);
        assert!(flat[0] == flat[1] && flat[1] == flat[2], "{flat:?}");
        let red = shade_texel(
            [200, 200, 200, 255],
            [1.0; 3],
            0.0,
            false,
            &Grade { tint: [255, 0, 0], ..none },
            1.0,
        );
        assert_eq!(red, [200, 0, 0, 255]);
    }

    #[test]
    fn srgb_round_trips_every_byte() {
        for v in 0..=255u8 {
            assert_eq!(linear_to_srgb(srgb_to_linear(v)), v);
        }
    }
}
