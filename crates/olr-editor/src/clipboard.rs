//! The editor's own clipboard (plan 14e R6; spec "Copy and paste of
//! elements"): it outlives the open document, so copies go across levels;
//! the system clipboard is not used.

use crate::history::Snapshot;

/// How far each paste moves the copies from the last, in world pixels.
pub const PASTE_STEP: (i32, i32) = (4, 4);

/// What was copied.
#[derive(Clone, Debug)]
pub enum Clipboard {
    /// Elements, at the art scale they came from; the next paste moves them
    /// by `offset` (`PASTE_STEP` more each paste, so pastes cascade).
    Elements { art_scale: u32, items: Vec<Snapshot>, offset: (i32, i32) },
}
