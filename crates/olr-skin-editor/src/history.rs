//! Undo and redo (issue #14): one step per stroke or operation, each the
//! frames it changed before and after, a settings change, or the whole
//! skin (Make double resolution). The level editor's pattern
//! (`olr_editor::history`): every change goes through [`Edit::apply`].

use olr_data::skin::SkinLight;
use olr_format::skins::SkinSource;

use crate::doc::{Frame, SkinDoc};

/// The most steps kept; the oldest go first.
pub const MAX_STEPS: usize = 400;

/// One undoable change.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Frames by index: before and after.
    Frames(Vec<(usize, Frame, Frame)>),
    /// The skin's name and light.
    Settings { before: (String, Option<SkinLight>), after: (String, Option<SkinLight>) },
    /// The whole skin.
    Whole { before: Box<SkinSource>, after: Box<SkinSource> },
}

impl Edit {
    /// Whether it changes nothing (no step).
    pub fn is_noop(&self) -> bool {
        match self {
            Edit::Frames(f) => f.iter().all(|(_, a, b)| a == b),
            Edit::Settings { before, after } => before == after,
            Edit::Whole { before, after } => before == after,
        }
    }

    /// Makes the change (`forward`) or takes it back.
    pub fn apply(&self, doc: &mut SkinDoc, forward: bool) {
        match self {
            Edit::Frames(frames) => {
                for (f, before, after) in frames {
                    doc.set_frame(*f, if forward { after } else { before });
                }
            }
            Edit::Settings { before, after } => {
                let (name, light) = if forward { after } else { before };
                doc.set_name(name.clone());
                doc.set_light(*light);
            }
            Edit::Whole { before, after } => {
                doc.set_source(if forward { (**after).clone() } else { (**before).clone() });
            }
        }
    }
}

/// The steps done and undone, each with an id (the unsaved mark compares
/// the id at the top with the one saved).
#[derive(Default, Debug)]
pub struct History {
    done: Vec<(u64, Edit)>,
    undone: Vec<(u64, Edit)>,
    next_id: u64,
}

impl History {
    /// Records `edit` (already applied) as a step; a new step drops what
    /// was undone.
    pub fn push(&mut self, edit: Edit) {
        self.next_id += 1;
        self.done.push((self.next_id, edit));
        self.undone.clear();
        if self.done.len() > MAX_STEPS {
            self.done.remove(0);
        }
    }

    /// The last step taken back, if any.
    pub fn undo(&mut self, doc: &mut SkinDoc) -> bool {
        let Some(step) = self.done.pop() else { return false };
        step.1.apply(doc, false);
        self.undone.push(step);
        true
    }

    /// The last undone step made again, if any.
    pub fn redo(&mut self, doc: &mut SkinDoc) -> bool {
        let Some(step) = self.undone.pop() else { return false };
        step.1.apply(doc, true);
        self.done.push(step);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// The id of the step the skin is at (0 before any).
    pub fn at(&self) -> u64 {
        self.done.last().map_or(0, |s| s.0)
    }
}
