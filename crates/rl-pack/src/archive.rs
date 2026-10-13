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
//! Hidden files are not part of a pack: a path any of whose components starts
//! with `.` (`.DS_Store`, `.git/...`) is left out by [`encode`] and by
//! [`crate::content_hash`] alike (plan 11 ruling PF11-2), and [`decode`]
//! refuses such a path.
//!
//! Every path must be portable (review C1): each component a portable name
//! ([`rl_vfs::non_portable`]: no drive or stream `:`, no `<>"|?*` or
//! control characters, no trailing dot or space, no Windows device name),
//! and no two paths of an archive (nor their directories) may be alike but
//! for ASCII case, as a case-insensitive file system would store them as
//! one. [`encode`] and [`decode`] refuse the same paths.
//!
//! What an archive unpacks to is bounded too (review C2: a 61 KB skin of
//! deep empty files made 27,218 directories): at most [`Limits::depth`]
//! components a path, at most as many directories as files and at most
//! [`MAX_DIRS`], and a disk charge ([`file_charge`], [`DIR_CHARGE`]: every
//! file and directory at least one 4 KiB block) of at most [`Limits::disk`],
//! charged before each file is written. The download cache counts its
//! packs by the same charge.
//!
//! Reading is streaming and bounded: [`decode`] holds at most one file
//! (at most [`MAX_FILE_BYTES`]) in memory, checks every length before it
//! reads the bytes, and stops at the first violation. The strictly
//! increasing order makes duplicates detectable in one pass.

use std::collections::BTreeMap;
use std::io::Read;

use rl_vfs::{ContentSink, ContentSource, VPath, non_portable, walk};

use crate::{MAX_FILE_BYTES, PackError};

/// The first bytes of every archive.
pub const MAGIC: &[u8; 6] = b"RLPK1\n";
/// The most bytes an archive may have (a skin's archive has a smaller limit,
/// the caller's [`Limits`]).
pub const MAX_ARCHIVE: u64 = 512 << 20;
/// The most files an archive may hold.
pub const MAX_ARCHIVE_FILES: u32 = 4096;
/// The longest path in an archive, in bytes.
pub const MAX_ARCHIVE_PATH: usize = 255;
/// The most components a path may have (`gfx/a.png` has 2).
pub const MAX_DEPTH: usize = 8;
/// The most distinct directories an archive may make (and never more than
/// its files).
pub const MAX_DIRS: usize = 512;
/// The most an unpacked pack may take on disk, by the charge.
pub const MAX_UNPACKED: u64 = 512 << 20;
/// A file system block: what an empty file or a directory takes at least.
pub const BLOCK: u64 = 4096;
/// The disk charge of a directory.
pub const DIR_CHARGE: u64 = BLOCK;

/// The disk charge of a file of `len` bytes: its length rounded up to
/// whole blocks, plus a block (its inode and directory entry).
pub fn file_charge(len: u64) -> u64 {
    len.div_ceil(BLOCK).saturating_mul(BLOCK).saturating_add(BLOCK)
}

/// What an archive may hold, the caller's: [`Limits::PACK`] for mods,
/// levels and themes, smaller ones for a skin (`rl-content`'s
/// `SKIN_LIMITS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The archive's bytes.
    pub bytes: u64,
    /// The unpacked pack's disk charge ([`file_charge`], [`DIR_CHARGE`]).
    pub disk: u64,
    /// Its files.
    pub files: u32,
    /// The components of a path.
    pub depth: usize,
}

impl Limits {
    /// Any pack's.
    pub const PACK: Limits =
        Limits { bytes: MAX_ARCHIVE, disk: MAX_UNPACKED, files: MAX_ARCHIVE_FILES, depth: MAX_DEPTH };
}

/// What [`decode`] wrote: the files, and their disk charge (their
/// directories' too).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unpacked {
    pub files: u32,
    pub disk: u64,
}

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
    #[error("the archive's path {0} is too deep")]
    TooDeep(String),
    #[error("the archive makes too many directories")]
    TooManyDirs,
    #[error("the archive would take more than {0} bytes on disk")]
    TooLargeUnpacked(u64),
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
/// Hidden files ([`is_hidden`]) are left out. Fails on what [`decode`] would
/// refuse with [`Limits::PACK`]: more than [`MAX_ARCHIVE_FILES`] files, a
/// path over [`MAX_ARCHIVE_PATH`] bytes or not portable, paths alike but
/// for case, a path deeper than [`MAX_DEPTH`], more directories than
/// files or [`MAX_DIRS`], a file over [`MAX_FILE_BYTES`], a total over
/// [`MAX_ARCHIVE`] or a disk charge over [`MAX_UNPACKED`].
pub fn encode(src: &dyn ContentSource) -> Result<Vec<u8>, PackError> {
    let mut paths = walk(src, &VPath::root())?;
    paths.retain(|p| !is_hidden(p.as_str()));
    if paths.len() > MAX_ARCHIVE_FILES as usize {
        return Err(PackError::invalid("archive", format!("more than {MAX_ARCHIVE_FILES} files")));
    }
    let mut out = MAGIC.to_vec();
    out.extend((paths.len() as u32).to_le_bytes());
    let mut layout = Layout::new(Limits::PACK, paths.len() as u32);
    for path in &paths {
        if let Some(why) = path_problem(path.as_str()) {
            return Err(PackError::invalid(path.as_str(), why));
        }
        let data = src.read_max(path, MAX_FILE_BYTES)?;
        if let Err(e) = layout.add(path.as_str(), data.len() as u64) {
            return Err(PackError::invalid(path.as_str(), e.to_string()));
        }
        let entry = 2 + path.as_str().len() as u64 + 8 + data.len() as u64;
        if out.len() as u64 + entry > MAX_ARCHIVE {
            return Err(PackError::invalid("archive", format!("larger than {MAX_ARCHIVE} bytes")));
        }
        out.extend((path.as_str().len() as u16).to_le_bytes());
        out.extend(path.as_str().as_bytes());
        out.extend((data.len() as u64).to_le_bytes());
        out.extend(data);
    }
    Ok(out)
}

/// Whether a component of `path` starts with `.`: not part of a pack.
pub(crate) fn is_hidden(path: &str) -> bool {
    path.split('/').any(|c| c.starts_with('.'))
}

/// Why `path` may not be in an archive, if it may not.
fn path_problem(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        Some("empty path")
    } else if path.len() > MAX_ARCHIVE_PATH {
        Some("path too long")
    } else if is_hidden(path) {
        Some("a name starts with a dot")
    } else if VPath::new(path).is_err() {
        Some("not a valid path")
    } else {
        path.split('/').find_map(non_portable)
    }
}

/// The files and directories an archive's paths make, as a
/// case-insensitive file system would see them (each by its ASCII
/// lowercase form, with its spelling and whether it is a directory), and
/// their disk charge, within the caller's [`Limits`].
struct Layout {
    limits: Limits,
    /// The archive's file count: directories may not outnumber it.
    files: u32,
    names: BTreeMap<String, (String, bool)>,
    dirs: usize,
    disk: u64,
}

impl Layout {
    fn new(limits: Limits, files: u32) -> Layout {
        Layout { limits, files, names: BTreeMap::new(), dirs: 0, disk: 0 }
    }

    /// Takes the next path (valid by [`path_problem`]) of a file of `len`
    /// bytes, before it is written. Refused when the path is deeper than
    /// the limits, when it or one of its directories is alike but for case
    /// to an earlier file or directory, or is a file where a directory is
    /// (or the reverse), when its directories pass the bound, or when the
    /// disk charge passes the limits.
    fn add(&mut self, path: &str, len: u64) -> Result<(), ArchiveError> {
        let bad = |why: &str| ArchiveError::BadPath(format!("{path} ({why})"));
        if path.split('/').count() > self.limits.depth {
            return Err(ArchiveError::TooDeep(path.to_string()));
        }
        let ends = path.match_indices('/').map(|(i, _)| (i, true)).chain([(path.len(), false)]);
        for (end, dir) in ends {
            let name = &path[..end];
            let folded = name.to_ascii_lowercase();
            match self.names.get(&folded) {
                Some((spelled, was_dir)) if spelled == name && *was_dir && dir => {}
                Some((spelled, _)) if spelled != name => {
                    return Err(bad("another path differs from it only in case"));
                }
                Some(_) => return Err(bad("a file and a directory of one name")),
                None => {
                    self.names.insert(folded, (name.to_string(), dir));
                    if dir {
                        self.dirs += 1;
                        if self.dirs > MAX_DIRS || self.dirs > self.files as usize {
                            return Err(ArchiveError::TooManyDirs);
                        }
                        self.charge(DIR_CHARGE)?;
                    }
                }
            }
        }
        self.charge(file_charge(len))
    }

    fn charge(&mut self, bytes: u64) -> Result<(), ArchiveError> {
        self.disk = self.disk.saturating_add(bytes);
        if self.disk > self.limits.disk {
            return Err(ArchiveError::TooLargeUnpacked(self.limits.disk));
        }
        Ok(())
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
/// first violation. `limits` caps the archive's bytes, its files, its
/// paths' depth and its disk charge (each file's charge counted before it
/// is written). Returns the files written and their charge; files written
/// before a violation stay in `sink`.
pub fn decode(
    r: &mut dyn Read,
    sink: &mut dyn ContentSink,
    limits: &Limits,
) -> Result<Unpacked, ArchiveError> {
    let mut r = Counted { r, taken: 0, limit: limits.bytes };
    let mut magic = [0u8; MAGIC.len()];
    match r.exact(&mut magic) {
        Err(ArchiveError::Truncated) => return Err(ArchiveError::Magic),
        other => other?,
    }
    if &magic != MAGIC {
        return Err(ArchiveError::Magic);
    }
    let count = r.u32()?;
    if count > MAX_ARCHIVE_FILES || count > limits.files {
        return Err(ArchiveError::TooManyFiles);
    }
    let mut previous: Option<String> = None;
    let mut layout = Layout::new(*limits, count);
    for _ in 0..count {
        let path_len = r.u16()? as usize;
        if path_len == 0 || path_len > MAX_ARCHIVE_PATH {
            return Err(ArchiveError::BadPath(format!("a path of {path_len} bytes")));
        }
        let mut raw = vec![0u8; path_len];
        r.exact(&mut raw)?;
        let path = String::from_utf8(raw)
            .map_err(|e| ArchiveError::BadPath(String::from_utf8_lossy(e.as_bytes()).into_owned()))?;
        if let Some(why) = path_problem(&path) {
            return Err(ArchiveError::BadPath(format!("{path} ({why})")));
        }
        if previous.as_ref().is_some_and(|p| path <= *p) {
            return Err(ArchiveError::Order(path));
        }
        let len = r.u64()?;
        if len > MAX_FILE_BYTES {
            return Err(ArchiveError::TooLarge(MAX_FILE_BYTES));
        }
        layout.add(&path, len)?;
        let data = r.bytes(len)?;
        let vpath = VPath::new(&path).map_err(|_| ArchiveError::BadPath(path.clone()))?;
        sink.write(&vpath, &data).map_err(|e| ArchiveError::Io(e.to_string()))?;
        previous = Some(path);
    }
    let mut extra = [0u8; 1];
    loop {
        match r.r.read(&mut extra) {
            Ok(0) => return Ok(Unpacked { files: count, disk: layout.disk }),
            Ok(_) => return Err(ArchiveError::Trailing),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(ArchiveError::Io(e.to_string())),
        }
    }
}
