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
mod dirty;
mod glass;
mod glow_beam;
mod journal;
mod live;
mod stamp;

use alloc::boxed::Box;
use alloc::vec::Vec;

pub use color::{Area, ColorEdit, ColorMap, GREEN_SHADES, GlowLayers, Rect, ThemeArt};
pub use dirty::Dirty;
pub use glass::GlassMap;
pub use glow_beam::GlowBeamMap;
pub use journal::{JournalError, JournalMark};
pub use live::{LiveArt, LiveFrame, LivePicture, LivePlaced, live_glow};
use olr_data::level::{Material, MaterialGrid};
pub use stamp::{GreenMask, GreenPixel, Stamp, StampPixel};

/// OLX pixel flags (`src/level/LXMapFlags.h`).
pub const PX_EMPTY: u8 = 0x01;
pub const PX_DIRT: u8 = 0x02;
pub const PX_ROCK: u8 = 0x04;

/// The material grid the simulation collides with and carves into.
#[derive(Clone, Debug, Default)]
pub struct Terrain {
    width: u32,
    height: u32,
    /// Material indices (`Material as u8`), row-major.
    cells: Vec<u8>,
    /// Rollback journal, off by default (`journal.rs`).
    journal: Option<Box<journal::Journal>>,
    /// Areas put back or replaced since [`Terrain::take_restored_areas`].
    restored: Vec<Area>,
    /// Loose dirt (the loose dirt design): a kind per cell, 0 not loose,
    /// else 1 + the level's loose material; never on a cell that is not
    /// dirt. `None` for a level without loose dirt.
    loose: Option<Box<[u8]>>,
    /// The rectangles of carves and dirt edits since
    /// [`Terrain::take_touched`], kept only with a loose layer.
    touched: Vec<Area>,
}

/// Equality compares the terrain only: the journal is bookkeeping, not terrain.
impl PartialEq for Terrain {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.cells == other.cells
            && self.loose == other.loose
    }
}

impl Eq for Terrain {}

impl Terrain {
    /// The most areas [`Terrain::take_restored_areas`] holds; beyond it
    /// they are merged into their bounding box.
    pub const MAX_RESTORED: usize = 256;

    pub fn from_grid(grid: &MaterialGrid) -> Self {
        Self {
            width: grid.width(),
            height: grid.height(),
            cells: grid.cells().iter().map(|m| *m as u8).collect(),
            journal: None,
            restored: Vec::new(),
            loose: None,
            touched: Vec::new(),
        }
    }

    /// This terrain with a loose layer (the loose dirt design): `kinds`
    /// one per cell, row-major, 0 not loose; a kind on a cell that is not
    /// dirt is dropped. `None` when `kinds` is not one a cell.
    pub fn with_loose(mut self, kinds: Vec<u8>) -> Option<Terrain> {
        if kinds.len() != self.cells.len() {
            return None;
        }
        let mut kinds = kinds.into_boxed_slice();
        for (k, &c) in kinds.iter_mut().zip(&self.cells) {
            if c != Material::Dirt as u8 {
                *k = 0;
            }
        }
        self.loose = Some(kinds);
        self.rehash();
        Some(self)
    }

    /// Whether the terrain has a loose layer (even one with no loose cell).
    pub fn has_loose(&self) -> bool {
        self.loose.is_some()
    }

    /// The loose kind at `(x, y)`: 0 when it is not loose or outside.
    pub fn loose(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return 0;
        }
        self.kind(y as usize * self.width as usize + x as usize)
    }

    /// The loose layer, row-major, when the terrain has one.
    pub fn loose_cells(&self) -> Option<&[u8]> {
        self.loose.as_deref()
    }

    /// Replaces the loose kinds with `kinds` (a received state's, after
    /// [`Terrain::load_cells`]): all or nothing. Refused (false, nothing
    /// changed) for a terrain without a layer given `Some`, a wrong length
    /// or a kind on a cell that is not dirt; `None` clears the layer's
    /// kinds. Not undoable, as `load_cells`.
    pub fn load_loose(&mut self, kinds: Option<&[u8]>) -> bool {
        let fits = |k: &[u8]| {
            k.len() == self.cells.len()
                && k.iter().zip(&self.cells).all(|(&k, &c)| k == 0 || c == Material::Dirt as u8)
        };
        match (kinds, self.loose.is_some()) {
            (None, false) => return true,
            (Some(k), true) if fits(k) => {
                for (i, &v) in k.iter().enumerate() {
                    self.set_value(i, self.cells[i] | v << 2);
                }
            }
            (None, _) => {
                for i in 0..self.cells.len() {
                    self.set_value(i, self.cells[i]);
                }
            }
            _ => return false,
        }
        self.seal_journal();
        true
    }

    /// A grain of loose dirt moves (cell indices): `to` becomes dirt of
    /// `from`'s kind and `from` background, both through the journal.
    /// Out-of-range indices change nothing.
    pub fn move_grain(&mut self, from: u32, to: u32) {
        let n = self.cells.len();
        let (f, t) = (from as usize, to as usize);
        if f >= n || t >= n || f == t {
            return;
        }
        let w = self.width;
        self.record_quiet(from % w, from / w, from % w + 1, from / w + 1);
        self.record_quiet(to % w, to / w, to % w + 1, to / w + 1);
        let kind = self.kind(f);
        self.set_value(t, Material::Dirt as u8 | kind << 2);
        self.set_value(f, Material::Background as u8);
    }

    /// [`Terrain::move_grain`] for a grain something else moved (a live
    /// part, the factory's production line design): both cells are noted
    /// touched ([`Terrain::take_touched`]), so the loose dirt there wakes.
    pub fn shove_grain(&mut self, from: u32, to: u32) {
        let n = self.cells.len() as u32;
        if from >= n || to >= n || from == to {
            return;
        }
        self.move_grain(from, to);
        if self.loose.is_some() {
            let w = self.width;
            for c in [from, to] {
                let (x, y) = ((c % w) as i32, (c / w) as i32);
                journal::note_restored(&mut self.touched, Area { x0: x, y0: y, x1: x + 1, y1: y + 1 });
            }
        }
    }

    /// The rectangles carves and dirt edits wrote since the last call (a
    /// terrain with a loose layer only: loose dirt wakes there), at most
    /// [`Terrain::MAX_RESTORED`], merged into their bounding box beyond.
    pub fn take_touched(&mut self) -> impl Iterator<Item = Area> + '_ {
        self.touched.drain(..)
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
        self.record(x as u32, y as u32, (x + cw) as u32, (y + ch) as u32);
        let mut carved = 0;
        for row in 0..ch {
            for col in 0..cw {
                let i = (y + row) as usize * self.width as usize + (x + col) as usize;
                if self.cells[i] == Material::Dirt as u8
                    && stamp.get(col as u32, row as u32) == StampPixel::Carve
                {
                    self.set_cell(i, Material::Background as u8);
                    carved += 1;
                }
            }
        }
        carved
    }

    /// Crushes the dirt of the `n` cells from `(x, y)` rightwards (a live
    /// rock part moved there; no OLX counterpart): each dirt cell, loose or
    /// not, becomes background. The run is clipped to the map. Returns how
    /// many cells it crushed.
    pub fn crush_row(&mut self, x: i32, y: i32, n: u32) -> u32 {
        let (mut x, mut w) = (x, i32::try_from(n).unwrap_or(i32::MAX));
        let (mut y, mut h) = (y, 1);
        if !clip(&mut x, &mut w, self.width as i32) || !clip(&mut y, &mut h, self.height as i32) {
            return 0;
        }
        let row = y as usize * self.width as usize;
        let span = row + x as usize..row + (x + w) as usize;
        if !self.cells[span.clone()].contains(&(Material::Dirt as u8)) {
            return 0;
        }
        self.record(x as u32, y as u32, (x + w) as u32, y as u32 + 1);
        let mut crushed = 0;
        for i in span {
            if self.cells[i] == Material::Dirt as u8 {
                self.set_value(i, Material::Background as u8);
                crushed += 1;
            }
        }
        crushed
    }

    /// The material indices (`Material as u8`), row-major: what a world's
    /// state transfer carries (networking spec §3.5).
    pub fn cells(&self) -> &[u8] {
        &self.cells
    }

    /// Whether `cells` could be [`load_cells`](Self::load_cells)ed: one
    /// valid material index per cell of this map.
    pub fn can_load(&self, cells: &[u8]) -> bool {
        cells.len() == self.cells.len() && cells.iter().all(|&c| Material::from_index(c).is_some())
    }

    /// Replaces every cell with `cells` (a received state's), unless
    /// [`can_load`](Self::can_load) refuses them: then it returns false and
    /// changes nothing. The journal's cell hash follows the new cells, but
    /// the load is not undoable: every mark taken before it is refused
    /// afterwards ([`crate::JournalError::Forgotten`]), as if the load were
    /// one edit already forgotten. The whole map is reported restored
    /// ([`Terrain::take_restored_areas`]).
    pub fn load_cells(&mut self, cells: &[u8]) -> bool {
        if !self.can_load(cells) {
            return false;
        }
        for (i, &c) in cells.iter().enumerate() {
            self.set_cell(i, c);
        }
        self.seal_journal();
        self.restored.clear();
        let (x1, y1) =
            (i32::try_from(self.width).unwrap_or(i32::MAX), i32::try_from(self.height).unwrap_or(i32::MAX));
        self.restored.push(Area { x0: 0, y0: 0, x1, y1 });
        true
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
    use olr_data::image::{Rgba, RgbaImage};

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
