//! A level's glass (the background glass design): what its see-through
//! glass does to the distant layers behind it, per art pixel.

use alloc::vec;
use alloc::vec::Vec;

/// Per art pixel of a level, RGBA8 row-major: the RGB is the filter the
/// distant layers behind are multiplied by (255 none), the alpha the frost
/// in tenths of a world pixel (0 none). Static: play never changes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlassMap {
    /// Art pixels.
    pub art: (u32, u32),
    /// `art.0 * art.1 * 4` bytes.
    pub texels: Vec<u8>,
}

impl GlassMap {
    /// A map of `art` that filters nothing and frosts nowhere.
    pub fn none(art: (u32, u32)) -> GlassMap {
        let n = art.0 as usize * art.1 as usize;
        let mut texels = vec![255; n * 4];
        for t in texels.as_chunks_mut::<4>().0 {
            t[3] = 0;
        }
        GlassMap { art, texels }
    }

    /// Whether it is of art size `art` with its texels' length.
    pub fn fits(&self, art: (u32, u32)) -> bool {
        self.art == art && (self.texels.len() as u64) == u64::from(art.0) * u64::from(art.1) * 4
    }
}
