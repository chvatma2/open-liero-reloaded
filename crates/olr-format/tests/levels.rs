//! Bundled levels are format 5: read as files, written again, they give the
//! same bytes, and compile to the level the runtime reads (spec §6).

use std::path::PathBuf;

use olr_format::levels::{compile_level, read_level_files, write_level_files};
use olr_vfs::{ContentSource, DirFs, MemFs, VPath};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// Every `<prefix>*` pack directory of the content root, sorted.
fn packs(root: &std::path::Path, prefix: &str) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with(prefix))
        .collect();
    ids.sort();
    ids
}

/// Every file of the pack except `pack.ron`, as `(path, bytes)`.
fn files(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root).unwrap() {
        let name = e.unwrap().file_name().into_string().unwrap();
        if name != "pack.ron" {
            out.push((name.clone(), std::fs::read(root.join(&name)).unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn bundled_levels_are_canonical_format_5() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let ids = packs(&root, "olx-level-");
    assert!(ids.len() >= 4, "only {} levels found", ids.len());
    for id in ids {
        let dir = DirFs::new(root.join(&id));
        let src = read_level_files(&dir).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &src).unwrap();
        // The same bytes: the committed files are what the writer writes.
        for (name, bytes) in files(&root.join(&id)) {
            let again = fs.read(&VPath::new(&name).unwrap()).unwrap_or_else(|e| panic!("{id}/{name}: {e}"));
            assert!(again == bytes, "{id}/{name}: written bytes differ from the committed file");
        }
        let back = read_level_files(&fs).unwrap();
        assert_eq!(back, src, "{id}");
        assert_eq!(compile_level(&back).unwrap(), olr_pack::read_level(&dir).unwrap().level, "{id}: Level");
        assert!(src.thumb.is_some(), "{id}: thumbnail passes through");
    }
}
