//! Reversible edits and the bounded undo history (spec §4: a property
//! change keeps its old value, a pixel edit its rectangle's previous
//! pixels; one stroke or one drag is one step; at most 256 MiB, the
//! oldest steps dropped first; not saved).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_edit::compose::{Decoded, Rect};
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

/// Bytes an item holds besides an image's grid (shared with the image
/// cache and other steps: `History` counts each grid once).
fn item(i: &Item) -> usize {
    let decoded = match &i.decoded {
        Decoded::Image { .. } => 0,
        d => d.bytes(),
    };
    decoded + std::mem::size_of::<Element>() + i.element.name.len()
}

/// The image grids an item holds, by address, with their bytes.
fn grids<'a>(items: impl Iterator<Item = &'a Item>) -> impl Iterator<Item = (usize, usize)> {
    items.filter_map(|i| match &i.decoded {
        Decoded::Image { grid: Some(g) } => Some((Arc::as_ptr(g) as usize, image(g))),
        _ => None,
    })
}

impl Edit {
    /// Whether the edit changes nothing (a value set to itself, a move to
    /// the same place, a resize to the same level).
    pub fn is_noop(&self) -> bool {
        match self {
            Edit::Pixels { before, after, .. } => before == after,
            Edit::Element { before, after, .. } => before == after,
            Edit::Insert { .. } | Edit::Remove { .. } => false,
            Edit::Move { from, to } => from == to,
            Edit::Objects { before, after } => before == after,
            Edit::Meta { before, after } => before == after,
            Edit::Whole { before, after } => before.level == after.level && before.items == after.items,
        }
    }

    /// The memory the edit holds besides image grids, roughly (for the
    /// history's cap; `History` adds each grid it holds once).
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

    /// The image grids the edit holds, by address, with their bytes.
    fn grids(&self) -> Vec<(usize, usize)> {
        match self {
            Edit::Insert { item: s, .. } | Edit::Remove { item: s, .. } => {
                grids(std::iter::once(&s.item)).collect()
            }
            Edit::Whole { before, after } => grids(before.items.iter().chain(&after.items)).collect(),
            _ => Vec::new(),
        }
    }
}

/// One step: its edits in the order they were made.
type Step = Vec<Edit>;

/// The undo and redo lists, at most `MAX_BYTES` together (the oldest undo
/// steps dropped first; a single step larger than the cap is kept alone).
/// An image grid several steps hold counts once.
#[derive(Default)]
pub struct History {
    /// The oldest first.
    undo: VecDeque<Step>,
    redo: Vec<Step>,
    /// A running total: the steps' own bytes and each grid once.
    bytes: usize,
    /// The grids held, by address: how many edits hold each, and its bytes.
    grids: BTreeMap<usize, (usize, usize)>,
}

impl History {
    pub const MAX_BYTES: usize = 256 << 20;

    pub fn new() -> History {
        History::default()
    }

    /// Counts a step in (`add`) or out of the total.
    fn count(&mut self, step: &Step, add: bool) {
        for e in step {
            if add {
                self.bytes += e.bytes();
            } else {
                self.bytes -= e.bytes();
            }
            for (at, size) in e.grids() {
                let held = self.grids.entry(at).or_insert((0, size));
                if add {
                    if held.0 == 0 {
                        self.bytes += size;
                    }
                    held.0 += 1;
                } else {
                    held.0 -= 1;
                    if held.0 == 0 {
                        self.bytes -= size;
                        self.grids.remove(&at);
                    }
                }
            }
        }
    }

    /// Records one step (one stroke, one drag, one property change);
    /// clears redo; drops the oldest steps while over `MAX_BYTES`,
    /// keeping at least this step. An empty step, or one that changes
    /// nothing, is not recorded.
    pub fn push(&mut self, step: Vec<Edit>) {
        if step.iter().all(Edit::is_noop) {
            return;
        }
        for old in std::mem::take(&mut self.redo) {
            self.count(&old, false);
        }
        self.count(&step, true);
        self.undo.push_back(step);
        while self.bytes > History::MAX_BYTES && self.undo.len() > 1 {
            if let Some(old) = self.undo.pop_front() {
                self.count(&old, false);
            }
        }
    }

    /// The step to revert (last edit first); it moves to the redo list.
    pub fn undo(&mut self) -> Option<&[Edit]> {
        let step = self.undo.pop_back()?;
        self.redo.push(step);
        self.redo.last().map(Vec::as_slice)
    }

    /// The step to apply again; it moves back to the undo list.
    pub fn redo(&mut self) -> Option<&[Edit]> {
        let step = self.redo.pop()?;
        self.undo.push_back(step);
        self.undo.back().map(Vec::as_slice)
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
