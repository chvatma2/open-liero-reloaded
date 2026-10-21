//! The mod editor (issue #17): a mod's world (worm, rope, presets,
//! loadouts), its weapons and their projectiles as a tree, edited through
//! `olr_edit::mods`, with undo, a check before saving and the values of
//! the mod it was copied from; as pure state a host drives (`olr-game`),
//! with egui panels, the skin editor's pattern (`olr-skin-editor`).
#![forbid(unsafe_code)]

pub mod fields;
