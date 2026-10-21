//! A synthetic classic skin, so the tests need no content.

use olr_data::image::Rgba;
use olr_format::skins::SkinSource;

/// A classic (OLX-layout) skin: in every frame a 6 × 4 block whose colour
/// says the frame, its top row tinted.
pub fn classic() -> SkinSource {
    let mut s = olr_edit::skin::blank("Sample", false);
    let (fw, fh) = s.frame_size;
    let w = s.image.width();
    for f in 0..21u32 {
        let (ox, oy) = ((f % 7) * fw, (f / 7) * fh);
        for y in 6..10 {
            for x in 7..13 {
                s.image.set(ox + x, oy + y, Rgba::rgb(10 * f as u8, 100, 200));
                if y == 6 {
                    s.tint[((oy + y) * w + ox + x) as usize] = 255;
                }
            }
        }
    }
    s
}
