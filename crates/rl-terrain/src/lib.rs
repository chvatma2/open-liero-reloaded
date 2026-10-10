//! Terrain as the simulation sees it (spec §5.4): one material byte per world
//! pixel, OLX's pixel-flag view of it, and hole carving; and as players see
//! it (spec §19 E7): [`ColorMap`], the colour image that replays the same
//! edits.
//!
//! `#![no_std]` + `alloc`.
#![no_std]

extern crate alloc;

mod color;
mod dirt;
mod stamp;

use alloc::vec::Vec;

pub use color::{Area, ColorEdit, ColorMap, GREEN_SHADES, Rect, ThemeArt};
use rl_data::level::{Material, MaterialGrid};
pub use stamp::{GreenMask, GreenPixel, Stamp, StampPixel};

/// OLX pixel flags (`src/level/LXMapFlags.h`).
pub const PX_EMPTY: u8 = 0x01;
pub const PX_DIRT: u8 = 0x02;
pub const PX_ROCK: u8 = 0x04;

/// The material grid the simulation collides with and carves into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terrain {
    width: u32,
    height: u32,
    /// Material indices (`Material as u8`), row-major.
    cells: Vec<u8>,
}

impl Terrain {
    pub fn from_grid(grid: &MaterialGrid) -> Self {
        Self {
            width: grid.width(),
            height: grid.height(),
            cells: grid.cells().iter().map(|m| *m as u8).collect(),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The material at `(x, y)`, or `None` outside the map.
    pub fn material(&self, x: i64, y: i64) -> Option<Material> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        Material::from_index(self.cells[y as usize * self.width as usize + x as usize])
    }

    /// OLX `CMap::GetPixelFlag` (`src/game/CMap.h:291-302`) without wrap-around:
    /// outside the map is rock; inside, `Material::toLxFlags`.
    pub fn flags(&self, x: i64, y: i64) -> u8 {
        match self.material(x, y) {
            None | Some(Material::Rock) => PX_ROCK,
            Some(Material::Dirt) => PX_DIRT,
            Some(Material::Background) => PX_EMPTY,
        }
    }

    /// OLX `CMap::CarveHole` (`src/common/CMap.cpp:1005-1105`) for the material
    /// grid: centres `stamp` on `(pos_x, pos_y)` (already truncated to ints by
    /// the caller), turns dirt under magenta stamp pixels into background, and
    /// returns how many cells it carved.
    ///
    /// Kept quirk: after clipping at the map edge, OLX still reads the stamp
    /// from its top-left corner (the stamp is not offset by the clipped amount).
    pub fn carve_hole(&mut self, stamp: &Stamp, pos_x: i32, pos_y: i32) -> u32 {
        let (w, h) = (stamp.width() as i32, stamp.height() as i32);
        // C's int arithmetic; it wraps on x86 where Rust would panic.
        let (mut x, mut y, mut cw, mut ch) = (pos_x.wrapping_sub(w / 2), pos_y.wrapping_sub(h / 2), w, h);
        if !clip(&mut x, &mut cw, self.width as i32) || !clip(&mut y, &mut ch, self.height as i32) {
            return 0;
        }
        let mut carved = 0;
        for row in 0..ch {
            for col in 0..cw {
                let i = (y + row) as usize * self.width as usize + (x + col) as usize;
                if self.cells[i] == Material::Dirt as u8
                    && stamp.get(col as u32, row as u32) == StampPixel::Carve
                {
                    self.cells[i] = Material::Background as u8;
                    carved += 1;
                }
            }
        }
        carved
    }

    /// FNV-1a 64 over the material indices, row by row: the oracle's `mh`.
    pub fn material_hash(&self) -> u64 {
        self.cells
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
    }
}

/// OLX `OLXRect::clipWith` on one axis against `[0, limit)`: returns false if nothing is left.
fn clip(start: &mut i32, len: &mut i32, limit: i32) -> bool {
    let end = start.saturating_add(*len);
    *start = (*start).max(0);
    let end = end.min(limit).max(*start);
    *len = end - *start;
    *len > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use rl_data::image::{Rgba, RgbaImage};

    fn grid(w: u32, h: u32, m: Material) -> Terrain {
        Terrain::from_grid(&MaterialGrid::new(w, h, m))
    }

    fn stamp(w: u32, h: u32, carve: &[(u32, u32)]) -> Stamp {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.set(x, y, Rgba::BLACK);
            }
        }
        for &(x, y) in carve {
            img.set(x, y, Rgba::MAGENTA);
        }
        Stamp::from_image(&img)
    }

    #[test]
    fn flags_follow_olx() {
        let mut t = grid(2, 1, Material::Dirt);
        t.cells[1] = Material::Background as u8;
        assert_eq!((t.flags(0, 0), t.flags(1, 0)), (PX_DIRT, PX_EMPTY));
        assert_eq!((t.flags(-1, 0), t.flags(2, 0), t.flags(0, 1)), (PX_ROCK, PX_ROCK, PX_ROCK));
    }

    #[test]
    fn carves_only_dirt_under_magenta() {
        let mut t = grid(5, 5, Material::Dirt);
        t.cells[12 + 1] = Material::Rock as u8; // (3,2)
        let s = stamp(3, 3, &[(0, 1), (1, 1), (2, 1), (1, 0)]);
        assert_eq!(t.carve_hole(&s, 2, 2), 3); // (3,2) is rock
        assert_eq!(t.material(1, 2), Some(Material::Background));
        assert_eq!(t.material(2, 1), Some(Material::Background));
        assert_eq!(t.material(3, 2), Some(Material::Rock));
        assert_eq!(t.material(2, 3), Some(Material::Dirt));
        assert_eq!(t.carve_hole(&s, 2, 2), 0, "nothing left to carve");
    }

    #[test]
    fn edge_clipping_keeps_olx_stamp_offset_quirk() {
        // Stamp centred at x=0: the left column is clipped away, but OLX still
        // reads the stamp from its column 0, so stamp column 0 lands on map x=0.
        let mut t = grid(3, 1, Material::Dirt);
        let s = stamp(3, 1, &[(0, 0)]);
        assert_eq!(t.carve_hole(&s, 0, 0), 1);
        assert_eq!(t.material(0, 0), Some(Material::Background));
        assert_eq!(t.material(1, 0), Some(Material::Dirt));
        assert_eq!(t.carve_hole(&s, -10, 0), 0, "fully outside");
        for p in [i32::MIN, i32::MIN + 1, i32::MAX - 1, i32::MAX] {
            assert_eq!(t.carve_hole(&s, p, 0) + t.carve_hole(&s, 0, p), 0, "{p}: no panic");
        }
    }

    #[test]
    fn hash_changes_with_material() {
        let mut t = grid(2, 2, Material::Dirt);
        let before = t.material_hash();
        t.cells[0] = Material::Background as u8;
        assert_ne!(before, t.material_hash());
        assert_eq!(
            grid(1, 1, Material::Rock).material_hash(),
            (0xcbf2_9ce4_8422_2325_u64).wrapping_mul(0x100_0000_01b3)
        );
        let _ = vec![0u8; 0];
    }
}
