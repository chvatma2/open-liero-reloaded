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
/// `start_journal`, with the serial of the last edit before it, so a mark
/// that the journal's history has left behind is recognised. Positions only
/// grow, so marks survive `forget_before`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct JournalMark {
    pos: u64,
    /// Serial of the edit at `pos - 1`; 0 when there is none.
    last: u64,
}

/// Why [`Terrain::undo_to`] refused a mark. Nothing is changed on an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    /// `start_journal` was never called.
    NotRecording,
    /// The mark is older than what `forget_before` kept.
    Forgotten,
    /// The mark is not in this journal's current history: it was taken after
    /// edits that a restore has since undone, or in another world.
    Stale,
}

/// One recorded edit: a clipped rectangle of the map; its old cells
/// (row-major) are in the journal's bytes.
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    /// Unique and increasing over the journal's life.
    serial: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Journal {
    edits: VecDeque<Edit>,
    bytes: VecDeque<u8>,
    /// Edits dropped from the front by `forget_before`.
    base: u64,
    /// Serial of the newest forgotten edit (0 if none), so a mark exactly at
    /// `base` can still be checked.
    forgotten: u64,
    /// The serial of the next recorded edit; never reused, even after undo.
    next_serial: u64,
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
            self.journal = Some(Box::new(Journal {
                edits: VecDeque::new(),
                bytes: VecDeque::new(),
                base: 0,
                forgotten: 0,
                next_serial: 1,
                hash,
            }));
        }
    }

    #[doc(hidden)]
    pub fn stop_journal_for_tests(&mut self) {
        self.journal = None;
    }

    /// The journal's current position, or `None` while it is off.
    pub fn journal_mark(&self) -> Option<JournalMark> {
        let j = self.journal.as_ref()?;
        let pos = j.base + j.edits.len() as u64;
        Some(JournalMark { pos, last: j.edits.back().map_or(j.forgotten, |e| e.serial) })
    }

    /// Puts back every cell changed since `mark`, newest edit first.
    pub fn undo_to(&mut self, mark: JournalMark) -> Result<(), JournalError> {
        let width = self.width as usize;
        let j = self.journal.as_mut().ok_or(JournalError::NotRecording)?;
        if mark.pos < j.base {
            return Err(JournalError::Forgotten);
        }
        let head = j.base + j.edits.len() as u64;
        let last = match mark.pos {
            p if p > head => return Err(JournalError::Stale),
            p if p == j.base => j.forgotten,
            p => j.edits[(p - j.base - 1) as usize].serial,
        };
        if last != mark.last {
            return Err(JournalError::Stale);
        }
        while j.base + j.edits.len() as u64 > mark.pos {
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
        while j.base < mark.pos {
            let Some(e) = j.edits.pop_front() else { break };
            j.forgotten = e.serial;
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
        let serial = j.next_serial;
        j.next_serial += 1;
        j.edits.push_back(Edit { x0, y0, x1, y1, serial });
    }

    /// Forgets the whole journal and counts one edit more, forgotten too, so
    /// every mark taken so far is older than the base: a change that was
    /// not recorded ([`Terrain::load_cells`]) cannot be undone across.
    pub(crate) fn seal_journal(&mut self) {
        let Some(j) = self.journal.as_mut() else { return };
        j.base += j.edits.len() as u64 + 1;
        j.edits.clear();
        j.bytes.clear();
        j.forgotten = j.next_serial;
        j.next_serial += 1;
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
    fn a_mark_after_undone_edits_is_stale() {
        let mut t = dirt(20, 20);
        t.start_journal();
        let m0 = t.journal_mark().unwrap();
        t.carve_hole(&square(4), 5, 5);
        let m1 = t.journal_mark().unwrap();
        t.undo_to(m0).unwrap();
        let after = t.clone();
        assert_eq!(t.undo_to(m1), Err(JournalError::Stale));
        assert_eq!(t, after, "a refused mark changes nothing");
    }

    #[test]
    fn a_mark_is_stale_even_when_the_journal_regrew_past_it() {
        let mut t = dirt(20, 20);
        t.start_journal();
        let m0 = t.journal_mark().unwrap();
        t.carve_hole(&square(4), 5, 5);
        let m1 = t.journal_mark().unwrap();
        t.undo_to(m0).unwrap();
        t.carve_hole(&square(4), 10, 10);
        t.carve_hole(&square(4), 14, 14);
        let after = t.clone();
        assert_eq!(t.undo_to(m1), Err(JournalError::Stale));
        assert_eq!(t, after);
        let mark = t.journal_mark().unwrap();
        t.forget_before(mark);
        t.undo_to(mark).unwrap(); // a mark exactly at the base still works
    }

    #[test]
    fn without_a_journal_nothing_is_recorded() {
        let mut t = dirt(20, 20);
        assert_eq!(t.journal_mark(), None);
        t.carve_hole(&square(4), 5, 5);
        assert_eq!(t.journal_bytes(), 0);
        assert_eq!(t.undo_to(JournalMark { pos: 0, last: 0 }), Err(JournalError::NotRecording));
    }

    #[test]
    fn a_load_refuses_every_older_mark() {
        let mut t = dirt(20, 20);
        t.start_journal();
        let m0 = t.journal_mark().unwrap();
        t.carve_hole(&square(4), 5, 5);
        let m1 = t.journal_mark().unwrap();
        let mut other = dirt(20, 20);
        other.carve_hole(&square(6), 12, 12);
        assert!(!t.load_cells(&[9; 400]), "not a material");
        assert!(!t.load_cells(&[1; 399]), "not this map's size");
        assert_eq!(t.undo_to(m1), Ok(()), "a refused load changes nothing");
        assert!(t.load_cells(other.cells()));
        assert_eq!(t, other);
        let mut fresh = other.clone();
        fresh.start_journal();
        assert_eq!(t.cell_hash(), fresh.cell_hash(), "the hash follows the load");
        let loaded = t.clone();
        // m1 is the newest mark before the load, with no edit after it.
        assert_eq!(t.undo_to(m1), Err(JournalError::Forgotten));
        assert_eq!(t.undo_to(m0), Err(JournalError::Forgotten));
        assert_eq!(t, loaded);
        let m2 = t.journal_mark().unwrap();
        t.carve_hole(&square(4), 3, 3);
        t.undo_to(m2).unwrap();
        assert_eq!(t, loaded);
        assert_eq!(t.cell_hash(), fresh.cell_hash());
        // Without a journal a load is a plain copy.
        let mut plain = dirt(20, 20);
        assert!(plain.load_cells(other.cells()));
        assert_eq!(plain, other);
    }

    #[test]
    fn equality_ignores_the_journal() {
        let a = dirt(10, 10);
        let mut b = a.clone();
        b.start_journal();
        assert_eq!(a, b);
    }
}
