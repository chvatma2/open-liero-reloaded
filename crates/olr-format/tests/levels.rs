//! Bundled levels: today's `olr_pack::read_level`, lifted, written, read
//! and compiled, give the same `Level` (spec §6).

use std::path::PathBuf;

use olr_format::levels::{compile_level, lift_level, read_level_files, write_level_files};
use olr_vfs::{DirFs, MemFs};

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

#[test]
fn bundled_levels_survive_lift_write_read_and_compile() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let ids = packs(&root, "olx-level-");
    assert!(ids.len() >= 4, "only {} levels found", ids.len());
    for id in ids {
        let dir = DirFs::new(root.join(&id));
        let pack = olr_pack::read_level(&dir).unwrap();
        let thumb = std::fs::read(root.join(&id).join("thumb.png")).ok();
        let src = lift_level(&pack.level, thumb.clone()).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &src).unwrap();
        let back = read_level_files(&fs).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(back, src, "{id}");
        assert_eq!(compile_level(&back).unwrap(), pack.level, "{id}: Level");
        assert_eq!(back.thumb, thumb, "{id}: thumbnail passes through");
    }
}
