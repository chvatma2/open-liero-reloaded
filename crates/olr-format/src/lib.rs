//! Pack authoring formats (spec `docs/superpowers/specs/2026-10-16-formats-design.md`):
//! the files editors, agents and people write, their canonical text, and
//! their compilers to `olr-data`'s runtime structs. Plan 13a: mods; plan 13b: levels,
//! themes, skins and the core pack.

pub mod assets;
mod colour;
pub mod core_pack;
pub(crate) mod defaults;
mod error;
pub mod image_io;
pub mod levels;
pub mod look;
pub mod materials;
pub mod mods;
pub mod ron_io;
pub mod skins;
mod text;
pub mod themes;

pub use colour::Colour;
pub use error::{FormatError, LiftError};
pub use text::canonical_text;
