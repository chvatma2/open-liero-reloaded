//! The `.rlpk` pack archive (plan 11 decision N29): a pack as one byte
//! stream, for download.
//!
//! Format, all integers little-endian:
//!
//! - the magic [`MAGIC`] (`RLPK1\n`);
//! - a `u32` file count;
//! - per file, sorted by path in strictly increasing byte order: a `u16`
//!   path length, the UTF-8 [`VPath`], a `u64` length and the bytes.
//!
//! There is no compression (packs are mostly PNG and OGG). The output is
//! deterministic: the same pack always gives the same bytes, so a resumed
//! download continues byte-exactly. Every file of the pack travels, `pack.ron`
//! and the presentation files included; the receiver identifies the pack by
//! [`crate::content_hash`], a partial download by the archive's own hash.
//!
//! Reading is streaming and bounded: [`decode`] holds at most one file
//! (at most [`MAX_FILE_BYTES`]) in memory, checks every length before it
//! reads the bytes, and stops at the first violation. The strictly
//! increasing order makes duplicates detectable in one pass.

use std::io::Read;

use rl_vfs::{ContentSink, ContentSource, VPath, walk};

use crate::{MAX_FILE_BYTES, PackError};

/// The first bytes of every archive.
pub const MAGIC: &[u8; 6] = b"RLPK1\n";
/// The most bytes an archive may have (a skin's archive has a smaller limit,
/// the caller's `limit` of [`decode`]).
pub const MAX_ARCHIVE: u64 = 512 << 20;
/// The most files an archive may hold.
pub const MAX_ARCHIVE_FILES: u32 = 4096;
/// The longest path in an archive, in bytes.
pub const MAX_ARCHIVE_PATH: usize = 255;

/// Why an archive is refused.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("not a pack archive")]
    Magic,
    #[error("the archive is cut short")]
    Truncated,
    #[error("the archive has bytes after its last file")]
    Trailing,
    #[error("the archive is larger than {0} bytes")]
    TooLarge(u64),
    #[error("the archive has too many files")]
    TooManyFiles,
    #[error("bad path in the archive: {0}")]
    BadPath(String),
    #[error("the archive lists {0} twice or out of order")]
    Order(String),
    #[error("{0}")]
    Io(String),
}

/// The archive of every file under `src`'s root (sorted by path, `pack.ron`
/// and presentation files included), deterministic.
///
/// Fails on what [`decode`] would refuse: more than [`MAX_ARCHIVE_FILES`]
/// files, a path over [`MAX_ARCHIVE_PATH`] bytes or with a name starting with
/// `.`, a file over [`MAX_FILE_BYTES`], or a total over [`MAX_ARCHIVE`].
pub fn encode(src: &dyn ContentSource) -> Result<Vec<u8>, PackError> {
    let paths = walk(src, &VPath::root())?;
    if paths.len() > MAX_ARCHIVE_FILES as usize {
        return Err(PackError::invalid("archive", format!("more than {MAX_ARCHIVE_FILES} files")));
    }
    let mut out = MAGIC.to_vec();
    out.extend((paths.len() as u32).to_le_bytes());
    for path in &paths {
        if let Some(why) = path_problem(path.as_str()) {
            return Err(PackError::invalid(path.as_str(), why));
        }
        let data = src.read_max(path, MAX_FILE_BYTES)?;
        out.extend((path.as_str().len() as u16).to_le_bytes());
        out.extend(path.as_str().as_bytes());
        out.extend((data.len() as u64).to_le_bytes());
        out.extend(data);
        if out.len() as u64 > MAX_ARCHIVE {
            return Err(PackError::invalid("archive", format!("larger than {MAX_ARCHIVE} bytes")));
        }
    }
    Ok(out)
}

/// Why `path` may not be in an archive, if it may not.
fn path_problem(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        Some("empty path")
    } else if path.len() > MAX_ARCHIVE_PATH {
        Some("path too long")
    } else if path.split('/').any(|c| c.starts_with('.')) {
        Some("a name starts with a dot")
    } else if VPath::new(path).is_err() {
        Some("not a valid path")
    } else {
        None
    }
}

/// Counts the bytes taken from the reader and refuses to take more than `limit`.
struct Counted<'a> {
    r: &'a mut dyn Read,
    taken: u64,
    limit: u64,
}

impl Counted<'_> {
    fn too_large(&self) -> ArchiveError {
        ArchiveError::TooLarge(self.limit)
    }

    /// Takes exactly `buf.len()` bytes.
    fn exact(&mut self, buf: &mut [u8]) -> Result<(), ArchiveError> {
        if self.taken.saturating_add(buf.len() as u64) > self.limit {
            return Err(self.too_large());
        }
        self.r.read_exact(buf).map_err(io_error)?;
        self.taken += buf.len() as u64;
        Ok(())
    }

    fn u16(&mut self) -> Result<u16, ArchiveError> {
        let mut b = [0u8; 2];
        self.exact(&mut b)?;
        Ok(u16::from_le_bytes(b))
    }

    fn u32(&mut self) -> Result<u32, ArchiveError> {
        let mut b = [0u8; 4];
        self.exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn u64(&mut self) -> Result<u64, ArchiveError> {
        let mut b = [0u8; 8];
        self.exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    /// Takes `len` bytes; the buffer grows with what arrives, so a lying
    /// length never allocates ahead of the data.
    fn bytes(&mut self, len: u64) -> Result<Vec<u8>, ArchiveError> {
        if len > MAX_FILE_BYTES {
            return Err(ArchiveError::TooLarge(MAX_FILE_BYTES));
        }
        if self.taken.saturating_add(len) > self.limit {
            return Err(self.too_large());
        }
        let mut data = Vec::new();
        let got = (&mut *self.r).take(len).read_to_end(&mut data).map_err(io_error)?;
        if got as u64 != len {
            return Err(ArchiveError::Truncated);
        }
        self.taken += len;
        Ok(data)
    }
}

fn io_error(e: std::io::Error) -> ArchiveError {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        ArchiveError::Truncated
    } else {
        ArchiveError::Io(e.to_string())
    }
}

/// Reads an archive from `r`, writing each file into `sink`; stops at the
/// first violation. Returns the files written. `limit` caps the archive's
/// total bytes; files written before a violation stay in `sink`.
pub fn decode(r: &mut dyn Read, sink: &mut dyn ContentSink, limit: u64) -> Result<u32, ArchiveError> {
    let mut r = Counted { r, taken: 0, limit };
    let mut magic = [0u8; MAGIC.len()];
    match r.exact(&mut magic) {
        Err(ArchiveError::Truncated) => return Err(ArchiveError::Magic),
        other => other?,
    }
    if &magic != MAGIC {
        return Err(ArchiveError::Magic);
    }
    let count = r.u32()?;
    if count > MAX_ARCHIVE_FILES {
        return Err(ArchiveError::TooManyFiles);
    }
    let mut previous: Option<String> = None;
    for _ in 0..count {
        let path_len = r.u16()? as usize;
        if path_len == 0 || path_len > MAX_ARCHIVE_PATH {
            return Err(ArchiveError::BadPath(format!("a path of {path_len} bytes")));
        }
        let mut raw = vec![0u8; path_len];
        r.exact(&mut raw)?;
        let path = String::from_utf8(raw)
            .map_err(|e| ArchiveError::BadPath(String::from_utf8_lossy(e.as_bytes()).into_owned()))?;
        if path_problem(&path).is_some() {
            return Err(ArchiveError::BadPath(path));
        }
        if previous.as_ref().is_some_and(|p| path <= *p) {
            return Err(ArchiveError::Order(path));
        }
        let len = r.u64()?;
        let data = r.bytes(len)?;
        let vpath = VPath::new(&path).map_err(|_| ArchiveError::BadPath(path.clone()))?;
        sink.write(&vpath, &data).map_err(|e| ArchiveError::Io(e.to_string()))?;
        previous = Some(path);
    }
    let mut extra = [0u8; 1];
    loop {
        match r.r.read(&mut extra) {
            Ok(0) => return Ok(count),
            Ok(_) => return Err(ArchiveError::Trailing),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(ArchiveError::Io(e.to_string())),
        }
    }
}
