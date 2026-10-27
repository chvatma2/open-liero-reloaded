//! Snapshots for rollback (networking spec §3.3).

use alloc::vec::Vec;
use core::mem;

use olr_terrain::{Area, JournalError, JournalMark};

use crate::World;

/// The world at one moment: everything but the terrain, which is kept as a
/// mark in the terrain's journal when rollback is on (a full copy otherwise).
///
/// Restoring a snapshot invalidates every snapshot taken after it: those are
/// refused ([`JournalError::Stale`]), never applied.
#[derive(Clone, Debug)]
pub struct Snapshot {
    world: World,
    mark: Option<JournalMark>,
}

impl Snapshot {
    /// The server frame the snapshot was taken at (before that tick runs).
    pub fn server_frame(&self) -> u64 {
        self.world.server_frame
    }
}

/// Why [`World::restore`] failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreError {
    /// The terrain journal refused the snapshot's mark (forgotten or stale).
    Journal(JournalError),
    /// A snapshot taken without rollback restored into a world with it, or the
    /// reverse, or a copy of another map (a snapshot of another world).
    NotThisWorld,
}

impl World {
    /// Turns on the terrain journal: snapshots then skip the terrain.
    pub fn enable_rollback(&mut self) {
        self.terrain.start_journal();
    }

    /// Whether the terrain journal is on ([`World::enable_rollback`]).
    pub fn rollback_enabled(&self) -> bool {
        self.terrain.journal_mark().is_some()
    }

    /// A snapshot of the world as it is now. The live parts' index (issue
    /// #90), derived from the rest, stays out of it: a restore rebuilds it.
    pub fn save(&mut self) -> Snapshot {
        let solids = self.live.as_mut().map(|l| mem::take(&mut l.solids));
        let s = match self.terrain.journal_mark() {
            Some(mark) => {
                let terrain = mem::take(&mut self.terrain);
                let world = self.clone();
                self.terrain = terrain;
                Snapshot { world, mark: Some(mark) }
            }
            None => Snapshot { world: self.clone(), mark: None },
        };
        if let (Some(l), Some(solids)) = (&mut self.live, solids) {
            l.solids = solids;
        }
        s
    }

    /// [`World::clone_from`] `s`, keeping this world's terrain and live
    /// parts' index (rebuilt from the restored copies).
    fn take_snapshot(&mut self, s: &World) {
        let terrain = mem::take(&mut self.terrain);
        let solids = self.live.as_mut().map(|l| mem::take(&mut l.solids));
        self.clone_from(s);
        self.terrain = terrain;
        if let Some(l) = &mut self.live {
            if let Some(solids) = solids {
                l.solids = solids;
            }
            l.refresh_solids();
        }
    }

    /// Puts the world back to `s`. This invalidates every snapshot taken
    /// after `s`: restoring one of those is refused with
    /// `RestoreError::Journal(JournalError::Stale)`, never applied. The world
    /// is unchanged on an error. The terrain it put back is reported by
    /// [`World::take_restored_areas`].
    pub fn restore(&mut self, s: &Snapshot) -> Result<(), RestoreError> {
        match (s.mark, self.rollback_enabled()) {
            (Some(mark), true) => {
                self.terrain.undo_to(mark).map_err(RestoreError::Journal)?;
                self.take_snapshot(&s.world);
                Ok(())
            }
            (None, false) => {
                // The snapshot's copy of the map, loaded so that the whole
                // map is reported restored.
                if !self.terrain.load_cells(s.world.terrain.cells()) {
                    return Err(RestoreError::NotThisWorld);
                }
                self.take_snapshot(&s.world);
                Ok(())
            }
            _ => Err(RestoreError::NotThisWorld),
        }
    }

    /// Drops the journal older than `s`: snapshots older than it can no
    /// longer be restored.
    pub fn forget_before(&mut self, s: &Snapshot) {
        if let Some(mark) = s.mark {
            self.terrain.forget_before(mark);
        }
    }

    /// The map areas a [`World::restore`] put back (each undone edit's
    /// rectangle, merged beyond `Terrain::MAX_RESTORED`) or a
    /// [`World::load_state`] or a restore without rollback replaced (the
    /// whole map), since the last call ([`Terrain::take_restored_areas`]).
    ///
    /// The presentation's colour map follows the edits ticks report
    /// (`EventKind::TerrainEdit`), which an undo does not: after a restore
    /// or a load, rebuild these areas from the terrain
    /// (`ColorMap::rebuild_area`) before applying the next ticks' edits,
    /// or an undone hole stays drawn.
    ///
    /// [`Terrain::take_restored_areas`]: olr_terrain::Terrain::take_restored_areas
    pub fn take_restored_areas(&mut self) -> Vec<Area> {
        self.terrain.take_restored_areas()
    }

    /// Bytes the terrain journal holds, for statistics.
    pub fn journal_bytes(&self) -> usize {
        self.terrain.journal_bytes()
    }
}
