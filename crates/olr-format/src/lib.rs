//! Pack authoring formats (spec `docs/superpowers/specs/2026-10-16-formats-design.md`):
//! the files editors, agents and people write, their canonical text, and
//! their compilers to `olr-data`'s runtime structs. Plan 13a: mods.

mod colour;
pub(crate) mod defaults;
mod error;
pub mod mods;
pub mod ron_io;

pub use colour::Colour;
pub use error::{FormatError, LiftError};
