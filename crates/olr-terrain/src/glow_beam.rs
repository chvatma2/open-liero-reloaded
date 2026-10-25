//! A level's glow beams (the directional-glow design): where its glow is
//! directional, baked per world pixel for the composite (front) and for the
//! backdrop (back).

use alloc::vec;
use alloc::vec::Vec;

/// The directions of a beam map, in degrees per step.
const STEP: u16 = 2;

/// Per world pixel and layer, two bytes row-major: byte 0 is the direction
/// in 2-degree steps (0 to 179) plus 1, 0 meaning the pixel is not
/// directional; byte 1 is the spread, the half-angle in degrees (1 to 90; a
/// texel with a direction needs a spread of at least 1).
/// Directions count from pointing right, clockwise on screen (y down). An
/// odd direction bakes rounded down to the even one below. Static: play
/// never changes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlowBeamMap {
    /// World pixels.
    pub world: (u32, u32),
    /// The composite's beams: `world.0 * world.1 * 2` bytes.
    pub front: Vec<u8>,
    /// The backdrop's beams, as `front`.
    pub back: Vec<u8>,
}

impl GlowBeamMap {
    /// A map of `world` with no directional pixel.
    pub fn none(world: (u32, u32)) -> GlowBeamMap {
        let n = world.0 as usize * world.1 as usize * 2;
        GlowBeamMap { world, front: vec![0; n], back: vec![0; n] }
    }

    /// Whether it is of world size `world` with its layers' length and every
    /// byte in range (byte 0 up to 180, byte 1 up to 90, and 1 or more where
    /// byte 0 is set).
    pub fn fits(&self, world: (u32, u32)) -> bool {
        let n = u64::from(world.0) * u64::from(world.1) * 2;
        self.world == world
            && self.front.len() as u64 == n
            && self.back.len() as u64 == n
            && [&self.front, &self.back].iter().all(|l| Self::in_range(l))
    }

    /// Whether every texel of `layer` (two bytes each) is in range.
    pub fn in_range(layer: &[u8]) -> bool {
        layer.as_chunks::<2>().0.iter().all(|t| t[0] <= 180 && t[1] <= 90 && (t[0] == 0 || t[1] >= 1))
    }

    /// The beam at world pixel `(x, y)` of the backdrop (`layer_back`) or
    /// the composite: its direction in whole degrees (always even) and its
    /// spread, or `None` where the pixel is not directional or lies outside.
    pub fn get(&self, layer_back: bool, x: u32, y: u32) -> Option<(u16, u8)> {
        if x >= self.world.0 || y >= self.world.1 {
            return None;
        }
        let layer = if layer_back { &self.back } else { &self.front };
        let i = (y as usize * self.world.0 as usize + x as usize) * 2;
        let (d, s) = (*layer.get(i)?, *layer.get(i + 1)?);
        (d != 0).then(|| (u16::from(d - 1) * STEP, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texels_read_as_even_degrees_and_misfits_do_not_fit() {
        let mut m = GlowBeamMap::none((3, 2));
        assert!(m.fits((3, 2)) && !m.fits((2, 3)));
        m.front[2] = 180;
        m.front[3] = 90;
        assert_eq!(m.get(false, 1, 0), Some((358, 90)));
        assert_eq!(m.get(false, 0, 0), None);
        assert_eq!(m.get(true, 1, 0), None);
        assert_eq!(m.get(false, 3, 0), None);
        m.front[2] = 181;
        assert!(!m.fits((3, 2)));
        m.front[2] = 5;
        m.front[3] = 0;
        assert!(!m.fits((3, 2)));
        m.front.pop();
        assert!(!m.fits((3, 2)));
    }
}
