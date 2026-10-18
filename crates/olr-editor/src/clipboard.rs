//! The editor's own clipboard (plan 14e R6; spec "Copy and paste of
//! elements"): it outlives the open document, so copies go across levels;
//! the system clipboard is not used.

use olr_format::levels::LevelObject;

use crate::history::Snapshot;
use crate::ops::PixelClip;

/// How far each paste moves the copies from the last, in world pixels.
pub const PASTE_STEP: (i32, i32) = (4, 4);

/// What was copied.
#[derive(Clone, Debug)]
pub enum Clipboard {
    /// Elements, at the art scale they came from; the next paste moves them
    /// by `offset` (`PASTE_STEP` more each paste, so pastes cascade).
    Elements { art_scale: u32, items: Vec<Snapshot>, offset: (i32, i32) },
    /// A rectangle of a drawn layer's pixels copied at world pixel `at`
    /// (plan 14e R7); pasted as a new layer at `at + offset`.
    Pixels { art_scale: u32, at: (i32, i32), clip: PixelClip, offset: (i32, i32) },
    /// Level objects (spawns, lights; the user's request 2026-10-19),
    /// pasted at their places plus `offset`.
    Objects { objects: Vec<LevelObject>, offset: (i32, i32) },
}
