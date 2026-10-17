//! The native pack format (spec §9.1).
//!
//! A pack is a directory tree reachable through an [`olr_vfs::ContentSource`]
//! whose root holds `pack.ron` ([`olr_data::manifest::PackManifest`]).
//! Readers and writers are deterministic: writing the same value twice yields
//! byte-identical files.

pub mod archive;
mod core_pack;
mod error;
mod hash;
pub mod image_io;
pub mod install;
mod level_pack;
mod mod_pack;
mod ron_io;
mod skin_pack;
mod theme_pack;
mod validate;

pub use core_pack::{CorePack, read_core, read_core_tolerant, write_core};
pub use error::PackError;
pub use hash::{PRESENTATION_FILES, content_hash};
pub use level_pack::{
    LevelPack, MAX_ART_SCALE, MAX_THUMB_BYTES, MAX_THUMB_SIDE, THUMB_FILE, THUMB_SIZE, decode_thumb,
    read_level, read_level_def, read_level_thumb, write_level,
};
pub use mod_pack::{ModPack, RESTRICTIONS_FILE, read_mod, read_preset_names, write_mod};
pub use ron_io::{MAX_FILE_BYTES, MAX_RON_BYTES, read_manifest, write_manifest};
pub use skin_pack::{SkinPack, read_skin, write_skin};
pub use theme_pack::{ThemePack, read_theme, write_theme};
pub use validate::{Diagnostic, Severity, validate};

/// Whether `path` lies under a pack's authoring folder (`editor/`): the
/// editors' sources, never content (not hashed, never in an archive; plan
/// 14a ruling R2).
pub fn is_authoring(path: &str) -> bool {
    path.split('/').next() == Some(olr_format::levels::EDITOR_DIR)
}

/// Path of the manifest inside every pack.
pub const MANIFEST: &str = "pack.ron";
