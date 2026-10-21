//! Undo and redo (issue #17): one step per change, each the mod as it was
//! before it (its source and name, and its other files when the change
//! brought files: a weapon copied from another mod). Restoring swaps the
//! snapshot with the mod, so the same step redoes what it undid.

use std::collections::BTreeMap;

use olr_edit::pack::{Pack, Source};
use olr_format::mods::ModSource;
use olr_vfs::VPath;

/// The most steps kept; the oldest go first.
pub const MAX_STEPS: usize = 200;

/// The mod at one side of a step.
#[derive(Clone, Debug)]
pub struct Snapshot {
    src: ModSource,
    /// The manifest's name (a rename of the mod changes it).
    name: String,
    /// The other files, only when the step changed them.
    other: Option<BTreeMap<VPath, Vec<u8>>>,
}

impl Snapshot {
    /// `pack`'s mod as it is now; `with_files` keeps its other files too.
    pub fn of(pack: &Pack, with_files: bool) -> Option<Snapshot> {
        let Source::Mod(src) = &pack.source else { return None };
        Some(Snapshot {
            src: src.clone(),
            name: pack.manifest.name.clone(),
            other: with_files.then(|| pack.other.clone()),
        })
    }

    /// Whether `pack` is as this snapshot holds it.
    pub fn same_as(&self, pack: &Pack) -> bool {
        matches!(&pack.source, Source::Mod(s) if *s == self.src)
            && pack.manifest.name == self.name
            && self.other.as_ref().is_none_or(|o| *o == pack.other)
    }

    /// Puts the snapshot back into `pack` (a change that failed half-way).
    pub fn restore(mut self, pack: &mut Pack) {
        self.swap(pack);
    }

    /// Puts the snapshot into `pack`, keeping what it replaced in `self`.
    fn swap(&mut self, pack: &mut Pack) {
        if let Source::Mod(src) = &mut pack.source {
            std::mem::swap(src, &mut self.src);
        }
        std::mem::swap(&mut pack.manifest.name, &mut self.name);
        if let Some(other) = &mut self.other {
            std::mem::swap(&mut pack.other, other);
        }
    }
}

/// The steps done and undone, each with an id (the unsaved mark compares
/// the id at the top with the one saved).
#[derive(Default, Debug)]
pub struct History {
    /// Each step's id and the mod before it.
    done: Vec<(u64, Snapshot)>,
    /// Each undone step's id and the mod after it.
    undone: Vec<(u64, Snapshot)>,
    next_id: u64,
}

impl History {
    /// Records a step taken from `before`; a new step drops what was undone.
    pub fn push(&mut self, before: Snapshot) {
        self.next_id += 1;
        self.done.push((self.next_id, before));
        self.undone.clear();
        if self.done.len() > MAX_STEPS {
            self.done.remove(0);
        }
    }

    /// Takes the last step back, if any.
    pub fn undo(&mut self, pack: &mut Pack) -> bool {
        let Some((id, mut snap)) = self.done.pop() else { return false };
        snap.swap(pack);
        self.undone.push((id, snap));
        true
    }

    /// Makes the last undone step again, if any.
    pub fn redo(&mut self, pack: &mut Pack) -> bool {
        let Some((id, mut snap)) = self.undone.pop() else { return false };
        snap.swap(pack);
        self.done.push((id, snap));
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// The id of the step the mod is at (0 before any).
    pub fn at(&self) -> u64 {
        self.done.last().map_or(0, |s| s.0)
    }
}
