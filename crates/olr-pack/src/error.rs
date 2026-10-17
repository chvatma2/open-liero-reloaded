use olr_format::FormatError;
use olr_vfs::VfsError;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error(transparent)]
    Vfs(#[from] VfsError),
    #[error("{file}:{line}:{col}: {message}")]
    Ron { file: String, line: usize, col: usize, message: String },
    #[error("{file}: {message}")]
    Image { file: String, message: String },
    #[error("{file}: {message}")]
    Invalid { file: String, message: String },
    /// An authoring-format error, from `olr-format`.
    #[error(transparent)]
    Format(#[from] FormatError),
}

impl PackError {
    pub(crate) fn invalid(file: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Invalid { file: file.into(), message: message.into() }
    }

    /// The file the error refers to, when there is one.
    pub fn file(&self) -> Option<&str> {
        match self {
            Self::Vfs(_) => None,
            Self::Ron { file, .. } | Self::Image { file, .. } | Self::Invalid { file, .. } => Some(file),
            Self::Format(
                FormatError::Parse { file, .. }
                | FormatError::Invalid { file, .. }
                | FormatError::Io { file, .. },
            ) => Some(file),
        }
    }
}
