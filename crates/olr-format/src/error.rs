use thiserror::Error;

/// Why a pack's authoring files could not be read, written or compiled.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FormatError {
    /// The text is not valid RON for its type.
    #[error("{file}:{line}:{col}: {message}")]
    Parse { file: String, line: usize, col: usize, message: String },
    /// The value is valid RON but not a valid pack: `path` is the field
    /// path inside the file, such as `fire.shot.projectile.hit_worm[1]`.
    #[error("{file}: {path}: {message}")]
    Invalid { file: String, path: String, message: String },
    /// The file could not be read or written.
    #[error("{file}: {message}")]
    Io { file: String, message: String },
}

impl FormatError {
    #[allow(dead_code)] // used by the mod compiler (plan 13a)
    pub(crate) fn invalid(file: &str, path: &str, message: impl Into<String>) -> Self {
        Self::Invalid { file: file.into(), path: path.into(), message: message.into() }
    }
}

/// A runtime value the authoring format cannot express losslessly. Today's
/// imports never produce one; a hand-built `ModDef` can.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{item}: {message}")]
pub struct LiftError {
    /// Such as `weapon "bazooka"` or `projectile "bazooka.spawn"`.
    pub item: String,
    pub message: String,
}
