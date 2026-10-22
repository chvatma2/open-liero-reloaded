//! The surface material editor (issue #44, `docs/superpowers/specs/
//! 2026-10-22-materials-and-heat-design.md`): the built-in materials
//! (read-only: Edit a copy) and the user's listed; New, Copy, Rename and
//! Delete; every field edited within its bounds, the colour ramp as keys
//! drawn as a strip; undo and redo; Save checked. Pure state a host drives
//! (`olr-game`), with egui panels, the skin and mod editors' pattern.
//! Issue #45: the live heat preview's controls (`preview`), the host
//! rendering its picture.
#![forbid(unsafe_code)]

pub mod preview;
pub mod state;
pub mod ui;

pub use preview::{BlastSize, PreviewAction, PreviewCommand, PreviewSettings, TimeScale};
pub use state::{
    Choice, Dialog, Edit, Key, MAX_ID_TRIES, MAX_STEPS, MaterialAction, MaterialCommand, MaterialEditorState,
    Next, Purpose, free_id, new_material, ramp_keys, ramp_of,
};
