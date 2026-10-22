//! The colour map's dirty tiles (issue #35): what changed since the last
//! [`crate::ColorMap::take_dirty`], kept as a set of square tiles rather
//! than a list of rectangles, so a spray of blood stains is a few tiles
//! instead of one upload each, and no number of edits ever folds them into
//! a box of most of the map. Each tile also knows whether only its colour
//! changed (a stain): the renderer then uploads the colour alone.

use alloc::vec;
use alloc::vec::Vec;

use crate::color::Rect;

/// A tile's flags: clean, its colour changed, or more (material or glow).
const CLEAN: u8 = 0;
const COLOUR: u8 = 1;
const ALL: u8 = 3;

/// A changed part of the colour map: an art rectangle, and whether only its
/// colour changed (blood and gib stains), not its material or glow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dirty {
    pub rect: Rect,
    pub colour_only: bool,
}

/// The dirty tiles of a map of `size` art pixels, `side` art pixels a tile.
#[derive(Clone, Debug)]
pub(crate) struct DirtyTiles {
    side: u32,
    size: (u32, u32),
    /// Tiles across.
    across: u32,
    /// Each tile's flags, row-major.
    flags: Vec<u8>,
    /// The tile rows holding a dirty tile, `first..end` (empty when clean).
    rows: (u32, u32),
}

impl DirtyTiles {
    pub(crate) fn new(size: (u32, u32), side: u32) -> DirtyTiles {
        let side = side.max(1);
        let (across, down) = (size.0.div_ceil(side), size.1.div_ceil(side));
        DirtyTiles { side, size, across, flags: vec![CLEAN; across as usize * down as usize], rows: (0, 0) }
    }

    /// Marks the tiles under art rectangle `r` (clipped to the map).
    pub(crate) fn mark(&mut self, r: Rect, colour_only: bool) {
        let (x1, y1) = (r.x.saturating_add(r.w).min(self.size.0), r.y.saturating_add(r.h).min(self.size.1));
        if x1 <= r.x || y1 <= r.y {
            return;
        }
        let kind = if colour_only { COLOUR } else { ALL };
        let (tx0, tx1) = (r.x / self.side, x1.div_ceil(self.side));
        let (ty0, ty1) = (r.y / self.side, y1.div_ceil(self.side));
        for ty in ty0..ty1 {
            let row = (ty * self.across) as usize;
            for f in &mut self.flags[row + tx0 as usize..row + tx1 as usize] {
                *f |= kind;
            }
        }
        self.rows = if self.rows.0 == self.rows.1 {
            (ty0, ty1)
        } else {
            (self.rows.0.min(ty0), self.rows.1.max(ty1))
        };
    }

    /// Drains the dirty tiles: each row's runs of tiles of one kind, top to
    /// bottom, as art rectangles clipped to the map. Dropped early, it
    /// still forgets them all.
    pub(crate) fn drain(&mut self) -> Drain<'_> {
        let y = self.rows.0;
        Drain { tiles: self, x: 0, y }
    }
}

/// [`DirtyTiles::drain`]'s iterator.
pub(crate) struct Drain<'a> {
    tiles: &'a mut DirtyTiles,
    x: u32,
    y: u32,
}

impl Iterator for Drain<'_> {
    type Item = Dirty;

    fn next(&mut self) -> Option<Dirty> {
        let t = &mut *self.tiles;
        while self.y < t.rows.1 {
            let row = (self.y * t.across) as usize;
            while self.x < t.across && t.flags[row + self.x as usize] == CLEAN {
                self.x += 1;
            }
            if self.x == t.across {
                self.x = 0;
                self.y += 1;
                continue;
            }
            let (start, kind) = (self.x, t.flags[row + self.x as usize]);
            while self.x < t.across && t.flags[row + self.x as usize] == kind {
                t.flags[row + self.x as usize] = CLEAN;
                self.x += 1;
            }
            let (x0, y0) = (start * t.side, self.y * t.side);
            let (x1, y1) = ((self.x * t.side).min(t.size.0), ((self.y + 1) * t.side).min(t.size.1));
            return Some(Dirty {
                rect: Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 },
                colour_only: kind == COLOUR,
            });
        }
        None
    }
}

impl Drop for Drain<'_> {
    fn drop(&mut self) {
        let t = &mut *self.tiles;
        let rows = (t.rows.0 * t.across) as usize..(t.rows.1 * t.across) as usize;
        t.flags[rows].fill(CLEAN);
        t.rows = (0, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn runs_of_one_kind_per_row() {
        // 10 × 5 art pixels, tiles of 4: 3 × 2 tiles, the last column and row cut.
        let mut t = DirtyTiles::new((10, 5), 4);
        t.mark(rect(1, 1, 1, 1), true);
        t.mark(rect(5, 0, 1, 1), true);
        t.mark(rect(9, 4, 1, 1), false);
        let got: Vec<Dirty> = t.drain().collect();
        assert_eq!(
            got,
            [
                Dirty { rect: rect(0, 0, 8, 4), colour_only: true },
                Dirty { rect: rect(8, 4, 2, 1), colour_only: false },
            ]
        );
        assert_eq!(t.drain().count(), 0, "drained");
    }

    #[test]
    fn a_change_of_more_than_colour_wins() {
        let mut t = DirtyTiles::new((8, 4), 4);
        t.mark(rect(0, 0, 8, 4), true);
        t.mark(rect(4, 0, 1, 1), false);
        let got: Vec<Dirty> = t.drain().collect();
        assert_eq!(
            got,
            [
                Dirty { rect: rect(0, 0, 4, 4), colour_only: true },
                Dirty { rect: rect(4, 0, 4, 4), colour_only: false }
            ]
        );
    }

    #[test]
    fn dropped_early_it_forgets_them_all() {
        let mut t = DirtyTiles::new((64, 64), 4);
        t.mark(rect(0, 0, 64, 64), false);
        t.mark(rect(0, 0, 1, 1), true);
        assert!(t.drain().next().is_some());
        assert_eq!(t.drain().count(), 0);
        // Past the map, empty or huge: clipped, never a panic.
        t.mark(rect(70, 70, 5, 5), false);
        t.mark(rect(3, 3, 0, 9), false);
        t.mark(rect(60, 60, u32::MAX, u32::MAX), true);
        assert_eq!(t.drain().collect::<Vec<_>>(), [Dirty { rect: rect(60, 60, 4, 4), colour_only: true }]);
    }
}
