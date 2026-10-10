//! The upscale pass's geometry (CPU only): where the view's texels land
//! in the output for each filter (spec §19 E3).

/// How the art-scale world is scaled to the output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UpscaleFilter {
    /// Whole multiples of a texel only, centred, black bars around: every
    /// texel is the same number of output pixels.
    IntegerNearest,
    /// Fills the output (aspect kept): each texel is a flat block whose
    /// edges blend over at most one output pixel, so pixels stay crisp
    /// at any size.
    #[default]
    SharpBilinear,
}

impl UpscaleFilter {
    pub const ALL: [UpscaleFilter; 2] = [UpscaleFilter::IntegerNearest, UpscaleFilter::SharpBilinear];

    /// The shader's code for the filter.
    pub(crate) fn code(self) -> u32 {
        match self {
            UpscaleFilter::IntegerNearest => 0,
            UpscaleFilter::SharpBilinear => 1,
        }
    }
}

/// The output rectangle `[x, y, w, h]` (output pixels) that `src` (`w`,
/// `h` in texels) is drawn into; the rest of the output is black.
pub fn dest_rect(filter: UpscaleFilter, src: (f32, f32), out: (u32, u32)) -> [f32; 4] {
    let (ow, oh) = (out.0 as f32, out.1 as f32);
    let (sw, sh) = (src.0.max(1.0), src.1.max(1.0));
    let fit = (ow / sw).min(oh / sh);
    let k = match filter {
        UpscaleFilter::IntegerNearest => fit.floor().max(1.0),
        UpscaleFilter::SharpBilinear => fit,
    };
    let (w, h) = (sw * k, sh * k);
    let centre = |o: f32, w: f32| match filter {
        UpscaleFilter::IntegerNearest => ((o - w) / 2.0).floor(),
        UpscaleFilter::SharpBilinear => (o - w) / 2.0,
    };
    [centre(ow, w), centre(oh, h), w, h]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_nearest_uses_whole_multiples_centred() {
        // 640 x 480 texels into 1920 x 1080: 2x (3x would be 1440 high).
        assert_eq!(
            dest_rect(UpscaleFilter::IntegerNearest, (640.0, 480.0), (1920, 1080)),
            [320.0, 60.0, 1280.0, 960.0]
        );
        assert_eq!(
            dest_rect(UpscaleFilter::IntegerNearest, (640.0, 480.0), (640, 480)),
            [0.0, 0.0, 640.0, 480.0]
        );
        // Too small an output still draws 1:1, centred and cropped.
        assert_eq!(
            dest_rect(UpscaleFilter::IntegerNearest, (640.0, 480.0), (320, 240)),
            [-160.0, -120.0, 640.0, 480.0]
        );
    }

    #[test]
    fn sharp_bilinear_fills_the_output_keeping_aspect() {
        assert_eq!(
            dest_rect(UpscaleFilter::SharpBilinear, (640.0, 480.0), (1440, 1080)),
            [0.0, 0.0, 1440.0, 1080.0]
        );
        assert_eq!(
            dest_rect(UpscaleFilter::SharpBilinear, (640.0, 480.0), (1920, 1080)),
            [240.0, 0.0, 1440.0, 1080.0]
        );
        let [x, y, w, h] = dest_rect(UpscaleFilter::SharpBilinear, (0.0, f32::NAN), (0, 0));
        assert!([x, y, w, h].iter().all(|v| v.is_finite()));
        assert_eq!(UpscaleFilter::default(), UpscaleFilter::SharpBilinear);
    }
}
