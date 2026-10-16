//! Placing dirt: OLX's `CMap::PlaceDirt` and `CMap::PlaceGreenDirt` for the
//! material grid. Unlike `CarveHole`, both offset the stamp by the amount
//! clipped at the top and left edges.

use olr_data::level::Material;

use crate::{GreenMask, GreenPixel, PX_EMPTY, PX_ROCK, Stamp, StampPixel, Terrain};

/// The clipped area of a `w` × `h` stamp whose top-left corner is at
/// `(sx, sy)`: map rows and columns, and the stamp offset of the first cell
/// (`CMap.cpp:1155-1161`, `1271-1277`). Computed in `i64`, which visits the
/// same cells as C's `int` arithmetic: where that would overflow, both
/// leave nothing to visit.
#[derive(Clone, Copy)]
pub(crate) struct Clip {
    pub(crate) x0: i64,
    pub(crate) x1: i64,
    pub(crate) y0: i64,
    pub(crate) y1: i64,
    stamp_x: i64,
    stamp_y: i64,
}

impl Clip {
    pub(crate) fn new(t: &Terrain, sx: i32, sy: i32, w: u32, h: u32) -> Self {
        let (sx, sy) = (sx as i64, sy as i64);
        Self {
            x0: sx.max(0),
            x1: (sx + w as i64).min(t.width as i64),
            y0: sy.max(0),
            y1: (sy + h as i64).min(t.height as i64),
            stamp_x: -sx.min(0),
            stamp_y: -sy.min(0),
        }
    }

    /// Every visited `(map index, stamp x, stamp y)`, row-major.
    pub(crate) fn cells(self, width: u32) -> impl Iterator<Item = (usize, u32, u32)> {
        let (c, width) = (self, width as i64);
        (c.y0..c.y1).flat_map(move |dy| {
            (c.x0..c.x1).map(move |dx| {
                let i = (dy * width + dx) as usize;
                (i, (c.stamp_x + dx - c.x0) as u32, (c.stamp_y + dy - c.y0) as u32)
            })
        })
    }
}

pub(crate) fn flags_of(cell: u8) -> u8 {
    match Material::from_index(cell) {
        Some(Material::Background) => PX_EMPTY,
        Some(Material::Dirt) => crate::PX_DIRT,
        Some(Material::Rock) | None => PX_ROCK,
    }
}

impl Terrain {
    /// OLX `CMap::PlaceDirt` (`src/common/CMap.cpp:1114-1224`) for the material
    /// grid: centres `stamp` on `(pos_x, pos_y)` (already truncated by the
    /// caller) and turns background and dirt under every non-black stamp pixel
    /// into dirt, leaving rock alone. Returns OLX's count: +1 for each
    /// background cell under magenta, +2 under any other colour (both of
    /// OLX's branches count it).
    pub fn place_dirt(&mut self, stamp: &Stamp, pos_x: i32, pos_y: i32) -> u32 {
        let (w, h) = (stamp.width(), stamp.height());
        let sx = pos_x.wrapping_sub((w >> 1) as i32);
        let sy = pos_y.wrapping_sub((h >> 1) as i32);
        let clip = Clip::new(self, sx, sy, w, h);
        if clip.x0 < clip.x1 && clip.y0 < clip.y1 {
            self.record(clip.x0 as u32, clip.y0 as u32, clip.x1 as u32, clip.y1 as u32);
        }
        let mut count = 0;
        for (i, x, y) in clip.cells(self.width) {
            let p = stamp.get(x, y);
            // The flags are read once, before either branch writes.
            let flag = flags_of(self.cells[i]);
            if p != StampPixel::Skip && flag & PX_ROCK == 0 {
                if flag & PX_EMPTY != 0 {
                    count += 1;
                }
                self.set_cell(i, Material::Dirt as u8);
            }
            if matches!(p, StampPixel::Paint(_)) && flag & PX_EMPTY != 0 {
                self.set_cell(i, Material::Dirt as u8);
                count += 1;
            }
        }
        count
    }

    /// OLX `CMap::PlaceGreenDirt` (`src/common/CMap.cpp:1231-1340`) for the
    /// material grid: centres `mask` on `(pos_x, pos_y)` and turns background
    /// under green and rim pixels into dirt. Each green pixel placed draws
    /// `rand()` once, row-major (`GetRandomInt(3)`, its colour). Returns how
    /// many cells it filled.
    pub fn place_green_dirt(
        &mut self,
        mask: &GreenMask,
        pos_x: i32,
        pos_y: i32,
        rand: &mut impl FnMut() -> i32,
    ) -> u32 {
        let (w, h) = (mask.width(), mask.height());
        let sx = pos_x.wrapping_sub((w >> 1) as i32);
        let sy = pos_y.wrapping_sub((h >> 1) as i32);
        let clip = Clip::new(self, sx, sy, w, h);
        if clip.x0 < clip.x1 && clip.y0 < clip.y1 {
            self.record(clip.x0 as u32, clip.y0 as u32, clip.x1 as u32, clip.y1 as u32);
        }
        let mut count = 0;
        for (i, x, y) in clip.cells(self.width) {
            let p = mask.get(x, y);
            let flag = flags_of(self.cells[i]);
            if p == GreenPixel::Green && flag & PX_EMPTY != 0 {
                self.set_cell(i, Material::Dirt as u8);
                count += 1;
                rand();
            }
            if matches!(p, GreenPixel::Rim(_)) && flag & PX_EMPTY != 0 {
                self.set_cell(i, Material::Dirt as u8);
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use olr_data::image::{Rgba, RgbaImage};
    use olr_data::level::MaterialGrid;

    fn image(rows: &[&str], colour: impl Fn(char) -> Rgba) -> RgbaImage {
        let mut img = RgbaImage::new(rows[0].len() as u32, rows.len() as u32);
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                img.set(x as u32, y as u32, colour(c));
            }
        }
        img
    }

    /// `m` magenta, `e` the dirt edge colour, `.` black.
    fn stamp(rows: &[&str]) -> Stamp {
        Stamp::from_image(&image(rows, |c| match c {
            'm' => Rgba::MAGENTA,
            'e' => Rgba::rgb(108, 56, 0),
            _ => Rgba::BLACK,
        }))
    }

    /// `G` green, `r` the rim colour, `p` pink.
    fn mask(rows: &[&str]) -> GreenMask {
        GreenMask::from_image(&image(rows, |c| match c {
            'G' => Rgba::rgb(0, 255, 0),
            'r' => Rgba::rgb(108, 80, 0),
            _ => Rgba::MAGENTA,
        }))
    }

    /// `R` rock, `D` dirt, `.` background.
    fn map(rows: &[&str]) -> Terrain {
        let cells: Vec<Material> = rows
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| match c {
                'R' => Material::Rock,
                'D' => Material::Dirt,
                _ => Material::Background,
            })
            .collect();
        Terrain::from_grid(&MaterialGrid::from_cells(rows[0].len() as u32, rows.len() as u32, cells).unwrap())
    }

    fn rows(t: &Terrain) -> Vec<alloc::string::String> {
        (0..t.height() as i64)
            .map(|y| {
                (0..t.width() as i64)
                    .map(|x| match t.material(x, y) {
                        Some(Material::Rock) => 'R',
                        Some(Material::Dirt) => 'D',
                        _ => '.',
                    })
                    .collect()
            })
            .collect()
    }

    const EXTREMES: [i32; 8] = [i32::MIN, i32::MIN + 1, -1000, -2, 6, 1000, i32::MAX - 1, i32::MAX];

    #[test]
    fn place_dirt_at_the_edges() {
        let s = stamp(&["me.", "emm", ".me"]);
        // Top-left corner: the stamp is offset by the clipped row and column
        // (unlike CarveHole), so map (0,0) gets stamp (1,1).
        let mut t = map(&[".....", ".....", "....."]);
        assert_eq!(t.place_dirt(&s, 0, 0), 1 + 1 + 1 + 2);
        assert_eq!(rows(&t), ["DD...", "DD...", "....."]);
        // Bottom-right corner: nothing to offset.
        let mut t = map(&["...", "...", "..."]);
        assert_eq!(t.place_dirt(&s, 2, 2), 1 + 2 + 2 + 1);
        assert_eq!(rows(&t), ["...", ".DD", ".DD"]);
        // Materials: rock is never touched, dirt stays dirt without counting,
        // background counts 1 under magenta and 2 under the edge colour.
        let mut t = map(&["RDD", "...", "DR."]);
        assert_eq!(t.place_dirt(&s, 1, 1), 2 + 1 + 1 + 2);
        assert_eq!(rows(&t), ["RDD", "DDD", "DRD"]);
        // A 1x1 map: only the stamp's centre lands on it.
        let mut t = map(&["."]);
        assert_eq!(t.place_dirt(&s, 0, 0), 1);
        assert_eq!(t.place_dirt(&s, 1, 0), 0, "edge colour over dirt");
        let mut t = map(&["."]);
        assert_eq!(t.place_dirt(&s, 1, 0), 2, "stamp (0,1), the edge colour");
        // Far outside, or where C's int arithmetic would overflow: nothing, no panic.
        let mut t = map(&["..", ".."]);
        for x in EXTREMES {
            for y in EXTREMES {
                assert_eq!(t.place_dirt(&s, x, y), 0, "({x}, {y})");
            }
        }
        assert_eq!(rows(&t), ["..", ".."]);
    }

    #[test]
    fn green_dirt_at_the_edges() {
        let m = mask(&["Grp", "rGG", "pGr"]);
        let draws = core::cell::Cell::new(0);
        let mut rand = || {
            draws.set(draws.get() + 1);
            7
        };
        // Top-left corner: offset like PlaceDirt, so map (0,0) gets mask (1,1):
        // three greens (three draws) and one rim.
        let mut t = map(&["....", "....", "...."]);
        assert_eq!(t.place_green_dirt(&m, 0, 0, &mut rand), 4);
        assert_eq!(rows(&t), ["DD..", "DD..", "...."]);
        assert_eq!(draws.get(), 3);
        // Bottom-right corner: two greens, two rims.
        let mut t = map(&["...", "...", "..."]);
        assert_eq!(t.place_green_dirt(&m, 2, 2, &mut rand), 4);
        assert_eq!(rows(&t), ["...", ".DD", ".DD"]);
        assert_eq!(draws.get(), 5);
        // Only background changes; dirt and rock draw nothing.
        let mut t = map(&["RDD", "D.D", "DRD"]);
        assert_eq!(t.place_green_dirt(&m, 1, 1, &mut rand), 1);
        assert_eq!(rows(&t), ["RDD", "DDD", "DRD"]);
        assert_eq!(draws.get(), 6);
        // A 1x1 map.
        let mut t = map(&["."]);
        assert_eq!(t.place_green_dirt(&m, 0, 0, &mut rand), 1);
        assert_eq!(draws.get(), 7);
        let mut t = map(&["."]);
        assert_eq!(t.place_green_dirt(&m, 1, 1, &mut rand), 1, "mask (0,0), green");
        let mut t = map(&["."]);
        assert_eq!(t.place_green_dirt(&m, -1, 0, &mut rand), 1, "mask (2,1), green");
        let mut t = map(&["."]);
        assert_eq!(t.place_green_dirt(&m, -1, 1, &mut rand), 0, "mask (2,0), pink");
        // Far outside: nothing, no panic.
        let mut t = map(&["..", ".."]);
        let before = draws.get();
        for x in EXTREMES {
            for y in EXTREMES {
                assert_eq!(t.place_green_dirt(&m, x, y, &mut rand), 0, "({x}, {y})");
            }
        }
        assert_eq!(draws.get(), before);
    }
}
