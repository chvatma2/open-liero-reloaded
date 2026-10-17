//! Helpers shared by the test modules.

use std::path::{Path, PathBuf};

/// The bundled content root, when `OLR_CONTENT_DIR` names a directory.
pub fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// Every pack directory of the content root, sorted.
pub fn packs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(root).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

/// Copies the bundled pack `id` into a fresh temporary directory, as
/// `<tmp>/<id>`.
pub fn copy_pack(root: &Path, id: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(id);
    copy_dir(&root.join(id), &dir);
    (tmp, dir)
}

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// Every file under `dir`, by `/`-separated relative path, with its bytes.
pub fn tree(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn go(base: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_str().unwrap().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    go(dir, dir, &mut out);
    out
}
