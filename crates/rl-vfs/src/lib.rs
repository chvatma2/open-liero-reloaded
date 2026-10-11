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
    #[error("{path} is {len} bytes, above the {limit} read at most")]
    TooLarge { path: String, len: u64, limit: u64 },
    #[error("{0} is not a regular file")]
    NotAFile(String),
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

    /// The bytes of the file at `path`, refused ([`VfsError::TooLarge`])
    /// when it is longer than `limit`: a backend that knows the length
    /// refuses it unread, so untrusted content cannot make a reader
    /// allocate without bound.
    fn read_max(&self, path: &VPath, limit: u64) -> Result<Vec<u8>, VfsError> {
        let bytes = self.read(path)?;
        let len = bytes.len() as u64;
        if len > limit {
            return Err(VfsError::TooLarge { path: path.to_string(), len, limit });
        }
        Ok(bytes)
    }

    /// The first `n` bytes of the file at `path` (all of it when shorter),
    /// such as an image header.
    fn read_prefix(&self, path: &VPath, n: usize) -> Result<Vec<u8>, VfsError> {
        let mut bytes = self.read(path)?;
        bytes.truncate(n);
        Ok(bytes)
    }

    /// The length of the file at `path`, in bytes.
    fn file_len(&self, path: &VPath) -> Result<u64, VfsError> {
        self.read(path).map(|b| b.len() as u64)
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
