//! The mod editor (issue #17): a mod's world (worm, rope, presets,
//! loadouts), its weapons and their projectiles as a tree, edited through
//! `olr_edit::mods`, with undo, a check before saving and the values of
//! the mod it was copied from; as pure state a host drives (`olr-game`),
//! with egui panels, the skin editor's pattern (`olr-skin-editor`).
#![forbid(unsafe_code)]

pub mod art;
mod assets;
pub mod browser;
pub mod fields;
pub mod history;
pub mod state;
pub mod tree;

pub use assets::ArtTools;
pub use browser::{FileEntry, Import};
pub use state::{
    ArtAction, BrowseAction, Choice, Dialog, Event, MAX_BOTS, ModAction, ModCommand, ModEditorState,
    PlaySettings,
};
pub use tree::{Item, Node, NodeKind, SpawnAt};
pub mod ui;
