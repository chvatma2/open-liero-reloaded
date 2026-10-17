//! Reversible edits and the bounded undo history (spec §4: a property
//! change keeps its old value, a pixel edit its rectangle's previous
//! pixels; one stroke or one drag is one step; at most 256 MiB, the
//! oldest steps dropped first; not saved).

use std::collections::VecDeque;
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_edit::compose::Rect;
use olr_format::levels::{Element, LevelObject, LevelSource};

use crate::document::Item;

/// A reversible change of the document.
#[derive(Clone, Debug)]
pub enum Edit {
    /// A drawn layer's art over `rect` (world), before and after (the art
    /// pixels of the rect).
    Pixels {
        id: String,
        rect: Rect,
        before: RgbaImage,
        after: RgbaImage,
    },
    /// An element's metadata (a property, a rename, a move of its box).
    Element {
        index: usize,
        before: Element,
        after: Element,
    },
    Insert {
        index: usize,
        item: Box<Snapshot>,
    },
    Remove {
        index: usize,
        item: Box<Snapshot>,
    },
    Move {
        from: usize,
        to: usize,
    },
    Objects {
        before: Vec<LevelObject>,
        after: Vec<LevelObject>,
    },
    Meta {
        before: LevelMeta,
        after: LevelMeta,
    },
    /// A resize or an art-scale change: the whole document.
    Whole {
        before: Box<Whole>,
        after: Box<Whole>,
    },
}

/// An element with its decoded data and, for an image, its original's
/// name and bytes.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub item: Item,
    pub original: Option<(String, Arc<Vec<u8>>)>,
}

/// The level's settings besides its size and art scale.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelMeta {
    pub name: String,
    pub notes: String,
    pub theme: String,
}

/// The whole document: the level (with its bake) and the items.
#[derive(Clone, Debug)]
pub struct Whole {
    pub level: LevelSource,
    pub items: Vec<Item>,
}

/// Bytes an image holds.
fn image(i: &RgbaImage) -> usize {
    i.as_raw().len()
}

impl Whole {
    fn bytes(&self) -> usize {
        let l = &self.level;
        l.material.len() + image(&l.front) + image(&l.back) + self.items.iter().map(item).sum::<usize>()
    }
}

/// Bytes an item holds, roughly: its decoded data and its metadata.
fn item(i: &Item) -> usize {
    i.decoded.bytes() + std::mem::size_of::<Element>() + i.element.name.len()
}

impl Edit {
    /// The memory the edit holds, roughly (for the history's cap).
    pub fn bytes(&self) -> usize {
        let base = std::mem::size_of::<Edit>();
        base + match self {
            Edit::Pixels { before, after, .. } => image(before) + image(after),
            Edit::Element { before, after, .. } => before.name.len() + after.name.len(),
            Edit::Insert { item: s, .. } | Edit::Remove { item: s, .. } => {
                item(&s.item) + s.original.as_ref().map_or(0, |(_, b)| b.len())
            }
            Edit::Move { .. } => 0,
            Edit::Objects { before, after } => {
                (before.len() + after.len()) * std::mem::size_of::<LevelObject>()
            }
            Edit::Meta { before, after } => {
                before.name.len() + before.notes.len() + after.name.len() + after.notes.len()
            }
            Edit::Whole { before, after } => before.bytes() + after.bytes(),
        }
    }
}

/// One step: its edits in the order they were made.
type Step = Vec<Edit>;

fn step_bytes(step: &Step) -> usize {
    step.iter().map(Edit::bytes).sum()
}

/// The undo and redo lists, at most `MAX_BYTES` together (the oldest undo
/// steps dropped first; a single step larger than the cap is kept alone).
#[derive(Default)]
pub struct History {
    /// The oldest first.
    undo: VecDeque<Step>,
    redo: Vec<Step>,
    bytes: usize,
}

impl History {
    pub const MAX_BYTES: usize = 256 << 20;

    pub fn new() -> History {
        History::default()
    }

    /// Records one step (one stroke, one drag, one property change);
    /// clears redo; drops the oldest steps while over `MAX_BYTES`,
    /// keeping at least this step. An empty step is not recorded.
    pub fn push(&mut self, step: Vec<Edit>) {
        if step.is_empty() {
            return;
        }
        self.redo.clear();
        self.bytes = self.undo.iter().map(step_bytes).sum::<usize>() + step_bytes(&step);
        self.undo.push_back(step);
        while self.bytes > History::MAX_BYTES && self.undo.len() > 1 {
            if let Some(old) = self.undo.pop_front() {
                self.bytes -= step_bytes(&old);
            }
        }
    }

    /// The step to revert (last edit first); it moves to the redo list.
    pub fn undo(&mut self) -> Option<Vec<Edit>> {
        let step = self.undo.pop_back()?;
        self.redo.push(step.clone());
        Some(step)
    }

    /// The step to apply again; it moves back to the undo list.
    pub fn redo(&mut self) -> Option<Vec<Edit>> {
        let step = self.redo.pop()?;
        self.undo.push_back(step.clone());
        Some(step)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The bytes the steps hold.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}
