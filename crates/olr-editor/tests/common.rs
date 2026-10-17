//! Helpers shared by the test modules.

use std::path::{Path, PathBuf};

use olr_editor::{Catalog, ModEntry, ThemeEntry};

/// The bundled content root, when `OLR_CONTENT_DIR` names a directory.
pub fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// `fixtures/edit/composed-level` (plan 14a Task 9).
pub fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edit/composed-level")
}

/// The installed packs the editor knows: `olx-theme-dirt` and the two mods.
pub fn catalog(root: &Path) -> Catalog {
    let theme = olr_format::themes::read_theme_files(&olr_vfs::DirFs::new(root.join("olx-theme-dirt"))).unwrap();
    Catalog {
        themes: vec![ThemeEntry {
            id: "olx-theme-dirt".into(),
            name: "Dirt".into(),
            front_tile: theme.front_tile,
            back_tile: theme.back_tile,
        }],
        mods: vec![
            ModEntry { id: "olx-classic".into(), name: "Liero v1.0".into() },
            ModEntry { id: "olx-mw-1-0".into(), name: "Modern Warfare1.0".into() },
        ],
    }
}

/// Copies the pack at `root.join(path)` (`path` may be absolute) into a
/// fresh temporary directory; returns it and the copy's path.
pub fn copy_pack(root: &Path, path: &Path) -> (tempfile::TempDir, PathBuf) {
    let from = root.join(path);
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(from.file_name().unwrap());
    copy_dir(&from, &dir);
    (tmp, dir)
}

fn copy_dir(from: &Path, to: &Path) {
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

/// The fixture, copied and opened.
pub fn open_fixture(root: &Path) -> (tempfile::TempDir, olr_editor::Document) {
    let (tmp, dir) = copy_pack(root, &fixture());
    let pack = olr_edit::pack::load(&dir).unwrap();
    let doc = olr_editor::Document::open(pack, dir, &catalog(root)).unwrap();
    (tmp, doc)
}

/// `pack::files` of the document's pack, by path string.
pub fn files(doc: &olr_editor::Document) -> std::collections::BTreeMap<String, Vec<u8>> {
    olr_edit::pack::files(&doc.to_pack()).unwrap().into_iter().map(|(p, b)| (p.as_str().to_string(), b)).collect()
}
