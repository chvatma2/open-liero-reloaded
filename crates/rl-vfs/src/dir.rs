use std::io::Read as _;
use std::path::{Path, PathBuf};

use std::path::Component;

use crate::{ContentSink, ContentSource, VPath, VfsError, non_portable};

/// A directory on the native file system. Paths are resolved below `root` only.
#[derive(Clone, Debug)]
pub struct DirFs {
    root: PathBuf,
}

impl DirFs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The native path of `p` below the root, built only from plain
    /// components ([`plain_component`]); for a write (`write`), from
    /// portable names too ([`native_component`]). Anything else is
    /// `VfsError::InvalidPath`: content never names a file outside the root.
    fn native(&self, p: &VPath, write: bool) -> Result<PathBuf, VfsError> {
        let mut out = self.root.clone();
        for c in p.components() {
            let checked = if write { native_component(c) } else { plain_component(c) };
            checked.map_err(|reason| VfsError::InvalidPath { path: p.to_string(), reason })?;
            out.push(c);
        }
        Ok(out)
    }

    /// The regular file at `path` (following links), opened, and its
    /// length. Anything else (a directory, a FIFO, a device) is refused
    /// before it is opened, so a FIFO in a pack cannot block a read.
    fn open(&self, path: &VPath) -> Result<(std::fs::File, u64), VfsError> {
        let native = self.native(path, false)?;
        let meta = std::fs::metadata(&native).map_err(|e| io(path, e))?;
        if !meta.is_file() {
            return Err(VfsError::NotAFile(path.to_string()));
        }
        let file = std::fs::File::open(&native).map_err(|e| io(path, e))?;
        Ok((file, meta.len()))
    }
}

/// Whether `c` is one plain component on this platform: it parses as
/// exactly one `Component::Normal`, itself (on Windows, `C:` is a prefix
/// that would replace the whole path).
fn plain_component(c: &str) -> Result<(), &'static str> {
    let mut parts = Path::new(c).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(n)), None) if n == std::ffi::OsStr::new(c) => Ok(()),
        _ => Err("is not a plain path component here"),
    }
}

/// Whether `c` may name a file this backend writes: a plain component
/// and a portable name ([`non_portable`]), on every platform alike.
fn native_component(c: &str) -> Result<(), &'static str> {
    if let Some(why) = non_portable(c) {
        return Err(why);
    }
    plain_component(c)
}

fn io(path: &VPath, source: std::io::Error) -> VfsError {
    if source.kind() == std::io::ErrorKind::NotFound {
        VfsError::NotFound(path.to_string())
    } else {
        VfsError::Io { path: path.to_string(), source }
    }
}

impl ContentSource for DirFs {
    fn read(&self, path: &VPath) -> Result<Vec<u8>, VfsError> {
        let (mut file, len) = self.open(path)?;
        let mut out = Vec::with_capacity(usize::try_from(len).unwrap_or(0).min(64 << 20));
        file.read_to_end(&mut out).map_err(|e| io(path, e))?;
        Ok(out)
    }

    fn exists(&self, path: &VPath) -> bool {
        self.native(path, false).is_ok_and(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))
    }

    fn read_max(&self, path: &VPath, limit: u64) -> Result<Vec<u8>, VfsError> {
        let too_large = |len| VfsError::TooLarge { path: path.to_string(), len, limit };
        let (file, len) = self.open(path)?;
        if len > limit {
            return Err(too_large(len));
        }
        // The file may grow after its length was read: never past the limit.
        let mut out = Vec::with_capacity(len as usize);
        file.take(limit.saturating_add(1)).read_to_end(&mut out).map_err(|e| io(path, e))?;
        if out.len() as u64 > limit {
            return Err(too_large(out.len() as u64));
        }
        Ok(out)
    }

    fn read_prefix(&self, path: &VPath, n: usize) -> Result<Vec<u8>, VfsError> {
        let (file, _) = self.open(path)?;
        let mut out = Vec::with_capacity(n.min(1 << 16));
        file.take(n as u64).read_to_end(&mut out).map_err(|e| io(path, e))?;
        Ok(out)
    }

    fn file_len(&self, path: &VPath) -> Result<u64, VfsError> {
        self.open(path).map(|(_, len)| len)
    }

    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(self.native(dir, false)?).map_err(|e| io(dir, e))? {
            let entry = entry.map_err(|e| io(dir, e))?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue; // non-UTF-8 names cannot be addressed by VPath
            };
            if VPath::new(&name).is_err() {
                continue;
            }
            let is_dir = entry.file_type().map_err(|e| io(dir, e))?.is_dir();
            names.push(if is_dir { format!("{name}/") } else { name });
        }
        names.sort();
        Ok(names)
    }
}

impl ContentSink for DirFs {
    fn write(&mut self, path: &VPath, data: &[u8]) -> Result<(), VfsError> {
        let native = self.native(path, true)?;
        if let Some(parent) = native.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(path, e))?;
        }
        std::fs::write(native, data).map_err(|e| io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk;

    #[test]
    fn write_read_list() {
        let tmp = tempfile::tempdir().unwrap();
        let mut fs = DirFs::new(tmp.path());
        fs.write(&VPath::new("x/y/z.bin").unwrap(), b"hi").unwrap();
        fs.write(&VPath::new("top.txt").unwrap(), b"t").unwrap();
        assert_eq!(fs.read(&VPath::new("x/y/z.bin").unwrap()).unwrap(), b"hi");
        assert_eq!(fs.list(&VPath::root()).unwrap(), vec!["top.txt", "x/"]);
        let all: Vec<String> = walk(&fs, &VPath::root()).unwrap().iter().map(|p| p.to_string()).collect();
        assert_eq!(all, vec!["top.txt", "x/y/z.bin"]);
        assert!(matches!(fs.read(&VPath::new("missing").unwrap()), Err(VfsError::NotFound(_))));
    }

    /// Bounded reads refuse a file over the limit before reading it, read
    /// a prefix only, and report lengths; only regular files are read.
    #[test]
    fn bounded_reads_and_regular_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let mut fs = DirFs::new(tmp.path());
        let p = VPath::new("big.bin").unwrap();
        fs.write(&p, &[7u8; 100]).unwrap();
        assert_eq!(fs.read_max(&p, 100).unwrap().len(), 100);
        assert!(matches!(fs.read_max(&p, 99), Err(VfsError::TooLarge { len: 100, limit: 99, .. })));
        assert_eq!(fs.read_prefix(&p, 8).unwrap(), [7u8; 8]);
        assert_eq!(fs.read_prefix(&p, 500).unwrap().len(), 100);
        assert_eq!(fs.file_len(&p).unwrap(), 100);
        assert!(fs.exists(&p));
        // A directory is not a file to read.
        fs.write(&VPath::new("d/x").unwrap(), b"x").unwrap();
        let d = VPath::new("d").unwrap();
        assert!(!fs.exists(&d));
        assert!(matches!(fs.read(&d), Err(VfsError::NotAFile(_))));
        assert!(matches!(fs.read_max(&d, 10), Err(VfsError::NotAFile(_))));
    }

    /// A write never leaves the root, whatever the platform: a component
    /// that is not one plain portable name (a drive prefix such as `C:`, a
    /// device name, a trailing dot) is refused before anything is created.
    #[test]
    fn a_write_of_a_non_portable_path_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let mut fs = DirFs::new(&root);
        for bad in ["C:/x", "a/b:c", "NUL.png", "con", "aux.txt", "a./b", "b /c", "a/x?", "a\u{1}b"] {
            let p = VPath::new(bad).unwrap();
            assert!(matches!(fs.write(&p, b"x"), Err(VfsError::InvalidPath { .. })), "{bad:?} is written");
        }
        assert!(!root.exists(), "nothing was created");
    }

    /// The component check alone, on every platform.
    #[test]
    fn native_components_are_single_portable_names() {
        for good in ["a", "gfx", "skin.png", "MW 1.0", "x-y_z"] {
            assert!(native_component(good).is_ok(), "{good:?}");
        }
        for bad in ["C:", "c:x", "a:b", "NUL", "com1.txt", "x.", "x ", "a<b", "a*b", "\u{7f}"] {
            assert!(native_component(bad).is_err(), "{bad:?}");
        }
    }

    /// A FIFO named like a pack file is refused without opening it (which
    /// would block until a writer came).
    #[cfg(unix)]
    #[test]
    fn a_fifo_never_blocks_a_read() {
        let tmp = tempfile::tempdir().unwrap();
        let made = std::process::Command::new("mkfifo").arg(tmp.path().join("pack.ron")).status();
        if !made.is_ok_and(|s| s.success()) {
            eprintln!("mkfifo unavailable; skipping");
            return;
        }
        let fs = DirFs::new(tmp.path());
        let p = VPath::new("pack.ron").unwrap();
        assert!(!fs.exists(&p));
        assert!(matches!(fs.read(&p), Err(VfsError::NotAFile(_))));
        assert!(matches!(fs.read_max(&p, 10), Err(VfsError::NotAFile(_))));
        assert!(matches!(fs.read_prefix(&p, 10), Err(VfsError::NotAFile(_))));
    }
}
