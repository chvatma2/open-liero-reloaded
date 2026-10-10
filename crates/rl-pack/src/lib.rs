//! The native pack format (spec §9.1).
//!
//! A pack is a directory tree reachable through an [`rl_vfs::ContentSource`]
//! whose root holds `pack.ron` ([`rl_data::manifest::PackManifest`]).
//! Readers and writers are deterministic: writing the same value twice yields
//! byte-identical files.

mod core_pack;
mod error;
mod hash;
pub mod image_io;
mod level_pack;
mod mod_pack;
mod ron_io;
mod skin_pack;
mod theme_pack;
mod validate;

pub use core_pack::{CorePack, read_core, write_core};
pub use error::PackError;
pub use hash::content_hash;
pub use level_pack::{LevelPack, read_level, write_level};
pub use mod_pack::{ModPack, read_mod, write_mod};
pub use ron_io::{read_manifest, write_manifest};
pub use skin_pack::{SkinPack, read_skin, write_skin};
pub use theme_pack::{ThemePack, read_theme, write_theme};
pub use validate::{Diagnostic, Severity, validate};

/// Path of the manifest inside every pack.
pub const MANIFEST: &str = "pack.ron";
