//! The level editor (plan 14b; spec
//! `docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md`
//! §4-§5): its document, its undo history, its tools and its egui panels,
//! as pure state a host drives (`olr-game`, plan 14c).
#![forbid(unsafe_code)]

pub mod browser;
pub mod catalog;
pub mod clipboard;
pub mod document;
pub mod history;
pub mod look_ops;
pub mod ops;
mod pointer;
pub mod state;
pub mod tools;
pub mod ui;
pub mod view;

pub use catalog::{Catalog, EditorSettings, ModEntry, ThemeEntry};
pub use document::{DocError, Document, Item, PackSnapshot, Problem};
pub use history::{Edit, History, LevelMeta};
pub use olr_pack::Severity;
pub use ops::Change;
pub use state::{
    Choice, Dialog, EditorAction, EditorCommand, EditorState, MAX_BOTS, NewLevel, PanelTab, Pending,
    Selection, Status,
};
