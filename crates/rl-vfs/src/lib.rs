//! Content I/O abstraction.
//!
//! All content is addressed by [`VPath`]: a relative, `/`-separated path with no
//! `..`, so a malicious mod or pack can never reach outside its root.
//! Backends: [`MemFs`] (tests, and later the web) and [`DirFs`] (native).

mod dir;
mod lookup;
mod mem;
mod path;

pub use dir::DirFs;
pub use lookup::{find_ci, find_ci_any};
pub use mem::MemFs;
pub use path::VPath;

/// Errors from content backends.
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid path {path:?}: {reason}")]
    InvalidPath { path: String, reason: &'static str },
    #[error("i/o error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Read-only access to a tree of files.
pub trait ContentSource {
    /// The bytes of the file at `path`.
    fn read(&self, path: &VPath) -> Result<Vec<u8>, VfsError>;

    /// Names (not full paths) of the entries directly inside `dir`, sorted.
    /// Directories are listed with a trailing `/`. The root is `VPath::root()`.
    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError>;

    /// Whether a file exists at `path`.
    fn exists(&self, path: &VPath) -> bool {
        self.read(path).is_ok()
    }
}

/// Write access to a tree of files.
pub trait ContentSink {
    /// Creates or replaces the file at `path`, creating parent directories.
    fn write(&mut self, path: &VPath, data: &[u8]) -> Result<(), VfsError>;
}

/// All file paths under `dir`, recursively, sorted.
pub fn walk(src: &dyn ContentSource, dir: &VPath) -> Result<Vec<VPath>, VfsError> {
    let mut out = Vec::new();
    for name in src.list(dir)? {
        if let Some(sub) = name.strip_suffix('/') {
            out.extend(walk(src, &dir.join(sub)?)?);
        } else {
            out.push(dir.join(&name)?);
        }
    }
    out.sort();
    Ok(out)
}
