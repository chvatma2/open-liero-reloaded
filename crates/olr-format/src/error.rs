use thiserror::Error;

/// Why a pack's authoring files could not be read, written or compiled.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FormatError {
    /// The text is not valid RON for its type.
    #[error("{file}:{line}:{col}: {message}")]
    Parse { file: String, line: usize, col: usize, message: String },
    /// The value is valid RON but not a valid pack: `path` is the field
    /// path inside the file, such as `fire.shot.projectile.hit_worm[1]`.
    #[error("{}", fmt_invalid(file, path, message))]
    Invalid { file: String, path: String, message: String },
    /// The file could not be read or written.
    #[error("{file}: {message}")]
    Io { file: String, message: String },
}

fn fmt_invalid(file: &str, path: &str, message: &str) -> String {
    if path.is_empty() { format!("{file}: {message}") } else { format!("{file}: {path}: {message}") }
}

impl FormatError {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_path_is_left_out() {
        assert_eq!(FormatError::invalid("a.ron", "", "bad").to_string(), "a.ron: bad");
        assert_eq!(FormatError::invalid("a.ron", "x.y", "bad").to_string(), "a.ron: x.y: bad");
    }
}
