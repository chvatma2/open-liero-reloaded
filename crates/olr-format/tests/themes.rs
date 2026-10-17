//! The bundled theme is format 5: read as files, written again, it gives the
//! same bytes, and compiles to the `Theme` the runtime reads, so the same
//! stamps and green mask for the simulation.

use std::path::PathBuf;

use olr_format::themes::{compile_theme, read_theme_files, write_theme_files};
use olr_terrain::{GreenMask, Stamp};
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

/// Every file below `dir` except `pack.ron`, as `(relative path, bytes)`.
fn files(dir: &std::path::Path, rel: &str, out: &mut Vec<(String, Vec<u8>)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let name = e.file_name().into_string().unwrap();
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if e.path().is_dir() {
            files(&e.path(), &path, out);
        } else if path != "pack.ron" {
            out.push((path, std::fs::read(e.path()).unwrap()));
        }
    }
}

#[test]
fn the_bundled_theme_is_canonical_format_5() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let ids = packs(&root, "olx-theme-");
    assert!(!ids.is_empty(), "no themes found");
    for id in ids {
        let dir = DirFs::new(root.join(&id));
        let src = read_theme_files(&dir).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_theme_files(&mut fs, &src).unwrap();
        let mut committed = Vec::new();
        files(&root.join(&id), "", &mut committed);
        assert!(committed.iter().any(|(p, _)| p.ends_with(".mask.png")), "{id}: no masks");
        for (name, bytes) in committed {
            let again = fs.read(&VPath::new(&name).unwrap()).unwrap_or_else(|e| panic!("{id}/{name}: {e}"));
            assert!(again == bytes, "{id}/{name}: written bytes differ from the committed file");
        }
        let back = read_theme_files(&fs).unwrap();
        assert_eq!(back, src, "{id}");
        let theme = compile_theme(&back).unwrap();
        let pack = olr_pack::read_theme(&dir).unwrap();
        assert_eq!(theme, pack.theme, "Theme");
        for (a, b) in theme.holes.iter().zip(&pack.theme.holes) {
            assert_eq!(Stamp::from_image(a), Stamp::from_image(b));
        }
        let gm = |t: &olr_data::theme::Theme| t.green_ball.as_ref().map(GreenMask::from_image);
        assert_eq!(gm(&theme), gm(&pack.theme));
    }
}
