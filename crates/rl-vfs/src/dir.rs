use std::path::{Path, PathBuf};

use crate::{ContentSink, ContentSource, VPath, VfsError};

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

    fn native(&self, p: &VPath) -> PathBuf {
        let mut out = self.root.clone();
        for c in p.components() {
            out.push(c);
        }
        out
    }
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
        std::fs::read(self.native(path)).map_err(|e| io(path, e))
    }

    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(self.native(dir)).map_err(|e| io(dir, e))? {
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
        let native = self.native(path);
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
}
