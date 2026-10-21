//! The worm skin editor (issue #14): a skin's 21 frames painted in
//! colour, player colour and glow, frame by frame, with undo, an onion
//! skin, frame copy and paste, and a live preview; as pure state a host
//! drives (`olr-game`), with egui panels, the level editor's pattern
//! (`olr-editor`).
#![forbid(unsafe_code)]

pub mod anim;
pub mod doc;
pub mod history;
pub mod paint;
pub mod state;
pub mod ui;

pub use doc::{AIM_FRAMES, FRAMES, Frame, OpenError, SkinDoc, WALK_FRAMES};
pub use paint::Target;
pub use state::{Choice, Dialog, Onion, PasteTo, SkinAction, SkinCommand, SkinEditorState, Tool};
