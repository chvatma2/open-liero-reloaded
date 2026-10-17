//! Bundled skins are format 5: read as files, written again, they give the
//! same bytes, and compile to the skin the runtime reads (spec §4).

use std::path::PathBuf;

use olr_format::skins::{compile_skin, read_skin_files, write_skin_files};
use olr_vfs::{ContentSource, DirFs, MemFs, VPath};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
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
fn bundled_skins_are_canonical_format_5() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let mut ids: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("olx-skin-"))
        .collect();
    ids.sort();
    assert!(ids.len() >= 700, "only {} skins found", ids.len());
    for id in ids {
        let dir = DirFs::new(root.join(&id));
        let src = read_skin_files(&dir).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_skin_files(&mut fs, &src).unwrap();
        // The same bytes: the committed files are what the writer writes.
        for (name, bytes) in files(&root.join(&id)) {
            let again = fs.read(&VPath::new(&name).unwrap()).unwrap_or_else(|e| panic!("{id}/{name}: {e}"));
            assert!(again == bytes, "{id}/{name}: written bytes differ from the committed file");
        }
        let back = read_skin_files(&fs).unwrap();
        assert_eq!(back, src, "{id}");
        assert_eq!(compile_skin(&back).unwrap(), olr_pack::read_skin(&dir).unwrap().skin, "{id}: Skin");
    }
}
