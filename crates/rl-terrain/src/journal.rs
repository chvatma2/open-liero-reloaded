//! Rollback support (networking spec §3.3): the terrain's edit journal and
//! its incrementally kept cell hash.
//!
//! While the journal is on, every write through `carve_hole`, `place_dirt`
//! and `place_green_dirt` first records the old bytes of the clipped
//! rectangle it may change; `undo_to` puts them back newest first, so a
//! restore costs what changed since the mark, not the map's size. The cell
//! hash is the wrapping sum of `mix((index << 8) | material)` over all
//! cells: order-independent, so one changed cell updates it in O(1).

use alloc::boxed::Box;
use alloc::collections::VecDeque;

use crate::Terrain;

/// A position in the journal: the count of edits recorded since
/// `start_journal`. Marks only grow, so they survive `forget_before`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct JournalMark(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    /// `start_journal` was never called.
    NotRecording,
    /// The mark is older than what `forget_before` kept.
    Forgotten,
}

/// One recorded edit: a clipped rectangle of the map; its old cells
/// (row-major) are in the journal's bytes.
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct Journal {
    edits: VecDeque<Edit>,
    bytes: VecDeque<u8>,
    /// Edits dropped from the front by `forget_before`.
    base: u64,
    hash: u64,
}

/// The splitmix64 finaliser.
pub(crate) fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

pub(crate) fn term(i: usize, cell: u8) -> u64 {
    mix(((i as u64) << 8) | cell as u64)
}

impl Terrain {
    /// Starts recording edits and tracking the cell hash (idempotent).
    pub fn start_journal(&mut self) {
        if self.journal.is_none() {
            let hash = self.scratch_hash();
            self.journal =
                Some(Box::new(Journal { edits: VecDeque::new(), bytes: VecDeque::new(), base: 0, hash }));
        }
    }

    #[doc(hidden)]
    pub fn stop_journal_for_tests(&mut self) {
        self.journal = None;
    }

    pub fn journal_mark(&self) -> Option<JournalMark> {
        self.journal.as_ref().map(|j| JournalMark(j.base + j.edits.len() as u64))
    }

    /// Puts back every cell changed since `mark`, newest edit first.
    pub fn undo_to(&mut self, mark: JournalMark) -> Result<(), JournalError> {
        let width = self.width as usize;
        let j = self.journal.as_mut().ok_or(JournalError::NotRecording)?;
        if mark.0 < j.base {
            return Err(JournalError::Forgotten);
        }
        while j.base + j.edits.len() as u64 > mark.0 {
            let e = j.edits.pop_back().expect("counted above");
            let (w, h) = ((e.x1 - e.x0) as usize, (e.y1 - e.y0) as usize);
            // The edit's bytes are the last w * h of the journal, row-major.
            for row in (0..h).rev() {
                for col in (0..w).rev() {
                    let old = j.bytes.pop_back().expect("an edit's bytes are recorded with it");
                    let i = (e.y0 as usize + row) * width + e.x0 as usize + col;
                    let cur = self.cells[i];
                    if cur != old {
                        j.hash = j.hash.wrapping_sub(term(i, cur)).wrapping_add(term(i, old));
                        self.cells[i] = old;
                    }
                }
            }
        }
        Ok(())
    }

    /// Drops the records older than `mark` (they can no longer be undone).
    pub fn forget_before(&mut self, mark: JournalMark) {
        let Some(j) = self.journal.as_mut() else { return };
        while j.base < mark.0 {
            let Some(e) = j.edits.pop_front() else { break };
            let n = (e.x1 - e.x0) as usize * (e.y1 - e.y0) as usize;
            j.bytes.drain(..n);
            j.base += 1;
        }
    }

    /// The cell hash: tracked while the journal is on, else computed.
    pub fn cell_hash(&self) -> u64 {
        match &self.journal {
            Some(j) => j.hash,
            None => self.scratch_hash(),
        }
    }

    /// Bytes the journal holds (pre-images), for the session's statistics.
    pub fn journal_bytes(&self) -> usize {
        self.journal.as_ref().map_or(0, |j| j.bytes.len())
    }

    fn scratch_hash(&self) -> u64 {
        self.cells.iter().enumerate().fold(0u64, |h, (i, &c)| h.wrapping_add(term(i, c)))
    }

    /// Records the old cells of `[x0, x1) x [y0, y1)` before a write (no-op
    /// without a journal or for an empty rectangle).
    pub(crate) fn record(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        let width = self.width as usize;
        let Some(j) = self.journal.as_mut() else { return };
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let row = y as usize * width;
            j.bytes.extend(&self.cells[row + x0 as usize..row + x1 as usize]);
        }
        j.edits.push_back(Edit { x0, y0, x1, y1 });
    }

    /// Writes one cell, keeping the tracked hash.
    #[inline]
    pub(crate) fn set_cell(&mut self, i: usize, v: u8) {
        let old = self.cells[i];
        if old != v {
            if let Some(j) = self.journal.as_mut() {
                j.hash = j.hash.wrapping_sub(term(i, old)).wrapping_add(term(i, v));
            }
            self.cells[i] = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::image::{Rgba, RgbaImage};
    use rl_data::level::{Material, MaterialGrid};

    use crate::Stamp;

    fn dirt(w: u32, h: u32) -> Terrain {
        Terrain::from_grid(&MaterialGrid::new(w, h, Material::Dirt))
    }

    fn square(n: u32) -> Stamp {
        let mut img = RgbaImage::new(n, n);
        for y in 0..n {
            for x in 0..n {
                img.set(x, y, Rgba::MAGENTA);
            }
        }
        Stamp::from_image(&img)
    }

    #[test]
    fn undo_restores_the_cells_and_the_hash() {
        let mut t = dirt(40, 30);
        t.start_journal();
        let before = t.clone();
        let hash = t.cell_hash();
        let mark = t.journal_mark().unwrap();
        t.carve_hole(&square(8), 10, 10);
        t.carve_hole(&square(8), 0, 0); // clipped at the corner
        t.place_dirt(&square(4), 10, 10);
        assert_ne!(t, before);
        assert_ne!(t.cell_hash(), hash);
        t.undo_to(mark).unwrap();
        assert_eq!(t, before);
        assert_eq!(t.cell_hash(), hash);
    }

    #[test]
    fn the_tracked_hash_equals_the_hash_from_scratch() {
        let mut t = dirt(40, 30);
        t.start_journal();
        t.carve_hole(&square(8), 5, 7);
        t.place_dirt(&square(4), 6, 6);
        let tracked = t.cell_hash();
        let mut fresh = t.clone();
        fresh.journal = None;
        assert_eq!(fresh.cell_hash(), tracked);
    }

    #[test]
    fn marks_survive_forgetting_and_old_marks_are_refused() {
        let mut t = dirt(20, 20);
        t.start_journal();
        let m0 = t.journal_mark().unwrap();
        t.carve_hole(&square(4), 5, 5);
        let m1 = t.journal_mark().unwrap();
        let at_m1 = t.clone();
        t.carve_hole(&square(4), 12, 12);
        t.forget_before(m1);
        assert_eq!(t.undo_to(m0), Err(JournalError::Forgotten));
        t.undo_to(m1).unwrap();
        assert_eq!(t, at_m1);
    }

    #[test]
    fn without_a_journal_nothing_is_recorded() {
        let mut t = dirt(20, 20);
        assert_eq!(t.journal_mark(), None);
        t.carve_hole(&square(4), 5, 5);
        assert_eq!(t.journal_bytes(), 0);
        assert_eq!(t.undo_to(JournalMark(0)), Err(JournalError::NotRecording));
    }

    #[test]
    fn equality_ignores_the_journal() {
        let a = dirt(10, 10);
        let mut b = a.clone();
        b.start_journal();
        assert_eq!(a, b);
    }
}
