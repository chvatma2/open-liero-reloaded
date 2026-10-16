use std::collections::BTreeMap;

use crate::{ContentSink, ContentSource, VPath, VfsError};

/// An in-memory file tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemFs {
    files: BTreeMap<VPath, Vec<u8>>,
}

impl MemFs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file; panics if `path` is invalid. Meant for tests and fixtures.
    pub fn with(mut self, path: &str, data: impl Into<Vec<u8>>) -> Self {
        self.files.insert(VPath::new(path).expect("valid path"), data.into());
        self
    }

    /// All files, sorted by path.
    pub fn files(&self) -> impl Iterator<Item = (&VPath, &[u8])> {
        self.files.iter().map(|(p, d)| (p, d.as_slice()))
    }
}

impl ContentSource for MemFs {
    fn read(&self, path: &VPath) -> Result<Vec<u8>, VfsError> {
        self.files.get(path).cloned().ok_or_else(|| VfsError::NotFound(path.to_string()))
    }

    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError> {
        let mut names = std::collections::BTreeSet::new();
        for p in self.files.keys() {
            if let Some(rest) = p.strip_prefix(dir) {
                if rest.is_root() {
                    continue;
                }
                let mut comps = rest.components();
                let first = comps.next().unwrap_or_default();
                if comps.next().is_some() {
                    names.insert(format!("{first}/"));
                } else {
                    names.insert(first.to_string());
                }
            }
        }
        if names.is_empty() && !dir.is_root() {
            return Err(VfsError::NotFound(dir.to_string()));
        }
        Ok(names.into_iter().collect())
    }
}

impl ContentSink for MemFs {
    fn write(&mut self, path: &VPath, data: &[u8]) -> Result<(), VfsError> {
        self.files.insert(path.clone(), data.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk;

    fn fs() -> MemFs {
        MemFs::new().with("a.txt", "1").with("d/b.txt", "2").with("d/e/c.txt", "3")
    }

    #[test]
    fn bounded_reads() {
        let fs = fs();
        let p = VPath::new("d/b.txt").unwrap();
        assert_eq!(fs.read_max(&p, 1).unwrap(), b"2");
        assert!(matches!(fs.read_max(&p, 0), Err(VfsError::TooLarge { len: 1, limit: 0, .. })));
        assert_eq!(fs.read_prefix(&p, 0).unwrap(), b"");
        assert_eq!(fs.file_len(&p).unwrap(), 1);
    }

    #[test]
    fn read_and_list() {
        let fs = fs();
        assert_eq!(fs.read(&VPath::new("d/b.txt").unwrap()).unwrap(), b"2");
        assert_eq!(fs.list(&VPath::root()).unwrap(), vec!["a.txt", "d/"]);
        assert_eq!(fs.list(&VPath::new("d").unwrap()).unwrap(), vec!["b.txt", "e/"]);
        assert!(matches!(fs.list(&VPath::new("nope").unwrap()), Err(VfsError::NotFound(_))));
    }

    #[test]
    fn walk_is_recursive_and_sorted() {
        let all: Vec<String> = walk(&fs(), &VPath::root()).unwrap().iter().map(|p| p.to_string()).collect();
        assert_eq!(all, vec!["a.txt", "d/b.txt", "d/e/c.txt"]);
    }
}
