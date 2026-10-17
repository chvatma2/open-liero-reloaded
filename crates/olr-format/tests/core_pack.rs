//! The bundled core pack and the bundled mods' gfx are format 5: OLX's
//! colour key became alpha once, at import, so what the files hold is what
//! a loader gets (spec §5, §6).

use std::path::{Path, PathBuf};

use olr_format::core_pack::{compile_core, lift_core, read_core_files, write_core_files};
use olr_format::image_io::{decode_rgba, encode_rgba};
use olr_vfs::{ContentSource, DirFs, MemFs, VPath};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// Every file under `root` except `pack.ron`, as `(path, bytes)`.
fn files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                if rel != "pack.ron" {
                    out.push((rel, std::fs::read(&path).unwrap()));
                }
            }
        }
    }
    out.sort();
    out
}

#[test]
fn the_bundled_core_pack_is_canonical_format_5() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let dir = DirFs::new(root.join("olx-core"));
    let src = read_core_files(&dir).unwrap();
    let mut fs = MemFs::new();
    write_core_files(&mut fs, &src).unwrap();
    // The same bytes: the committed files are what the writer writes.
    let committed = files(&root.join("olx-core"));
    assert_eq!(
        fs.files().map(|(p, _)| p.to_string()).filter(|p| p != "pack.ron").collect::<Vec<_>>(),
        committed.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>()
    );
    for (name, bytes) in &committed {
        let again = fs.read(&VPath::new(name).unwrap()).unwrap();
        assert!(again == *bytes, "{name}: written bytes differ from the committed file");
    }
    assert_eq!(read_core_files(&fs).unwrap(), src);
    // Compile to the runtime pack and lift back: purely structural.
    let pack = olr_pack::read_core(&dir).unwrap();
    let (def, sprites, sounds) = compile_core(&src).unwrap();
    assert_eq!(def, pack.def, "CoreDef");
    assert_eq!(sounds, pack.sounds, "sounds");
    assert_eq!(sprites, pack.sprites, "sprites: nothing is keyed at load");
    assert_eq!(lift_core(&def, &sprites, &sounds).unwrap(), src);
}

#[test]
fn the_bundled_mods_gfx_are_canonical_alpha_pngs() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    for id in ["olx-classic", "olx-mw-1-0"] {
        let gfx = root.join(id).join("gfx");
        for entry in std::fs::read_dir(&gfx).unwrap() {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            let img = decode_rgba(&bytes).unwrap();
            assert!(encode_rgba(&img) == bytes, "{}: not what the importer writes", path.display());
        }
    }
}
