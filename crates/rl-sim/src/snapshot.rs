//! Snapshots for rollback (networking spec §3.3).

use core::mem;

use rl_terrain::{JournalError, JournalMark};

use crate::World;

/// The world at one moment: everything but the terrain, which is kept as a
/// mark in the terrain's journal when rollback is on (a full copy otherwise).
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
    Journal(JournalError),
    /// A snapshot taken without rollback restored into a world with it, or the reverse.
    NotThisWorld,
}

impl World {
    /// Turns on the terrain journal: snapshots then skip the terrain.
    pub fn enable_rollback(&mut self) {
        self.terrain.start_journal();
    }

    pub fn rollback_enabled(&self) -> bool {
        self.terrain.journal_mark().is_some()
    }

    /// A snapshot of the world as it is now.
    pub fn save(&mut self) -> Snapshot {
        match self.terrain.journal_mark() {
            Some(mark) => {
                let terrain = mem::take(&mut self.terrain);
                let world = self.clone();
                self.terrain = terrain;
                Snapshot { world, mark: Some(mark) }
            }
            None => Snapshot { world: self.clone(), mark: None },
        }
    }

    /// Puts the world back to `s`.
    pub fn restore(&mut self, s: &Snapshot) -> Result<(), RestoreError> {
        match (s.mark, self.rollback_enabled()) {
            (Some(mark), true) => {
                self.terrain.undo_to(mark).map_err(RestoreError::Journal)?;
                let terrain = mem::take(&mut self.terrain);
                self.clone_from(&s.world);
                self.terrain = terrain;
                Ok(())
            }
            (None, false) => {
                self.clone_from(&s.world);
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

    pub fn journal_bytes(&self) -> usize {
        self.terrain.journal_bytes()
    }
}
