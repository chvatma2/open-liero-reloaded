//! Imports every OLX skin and the Dirt theme when `OLR_OLX_GAMEDIR` is set,
//! and checks the imported levels' thumbnails when `OLR_CONTENT_DIR` is.

use olr_data::manifest::Origin;
use olr_vfs::{ContentSource, DirFs, MemFs, VPath};

fn origin(path: &str) -> Origin {
    Origin { source: "OpenLieroX".into(), path: path.into(), revision: "test".into() }
}

#[test]
fn all_skins_and_dirt_theme() {
    let Ok(dir) = std::env::var("OLR_OLX_GAMEDIR") else {
        eprintln!("OLR_OLX_GAMEDIR not set; skipping real skin/theme import test");
        return;
    };
    let fs = DirFs::new(dir);
    let skins = VPath::new("skins").unwrap();
    let mut imported = 0;
    for name in fs.list(&skins).unwrap() {
        if !name.to_ascii_lowercase().ends_with(".png") {
            continue;
        }
        let s = olr_import_gfx::import_skin(&fs, &skins.join(&name).unwrap(), origin("skins")).unwrap();
        let mut out = MemFs::new();
        olr_pack::write_skin(&mut out, &s.pack).unwrap();
        assert_eq!(olr_pack::read_skin(&out).unwrap(), s.pack, "{name}");
        imported += 1;
    }
    assert_eq!(imported, 409);

    let t =
        olr_import_gfx::import_theme(&fs, &VPath::new("data/themes/Dirt").unwrap(), origin("Dirt")).unwrap();
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
    assert_eq!((t.pack.theme.stones.len(), t.pack.theme.misc.len()), (8, 8));
    let mut out = MemFs::new();
    olr_pack::write_theme(&mut out, &t.pack).unwrap();
    assert_eq!(olr_pack::validate(&out), vec![]);
}

/// Plan 7 G6: every imported level pack in `OLR_CONTENT_DIR` has its
/// thumbnail, made from its art at `THUMB_SIZE`, letterboxed.
#[test]
fn thumbnails_are_imported_for_every_level() {
    let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set; skipping the imported thumbnails");
        return;
    };
    let mut levels = 0;
    let mut dirs: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    dirs.sort();
    for path in dirs.iter().filter(|p| p.join(olr_pack::MANIFEST).is_file()) {
        let src = DirFs::new(path);
        if olr_pack::read_manifest(&src).unwrap().kind != olr_data::manifest::PackKind::Level {
            continue;
        }
        let pack = olr_pack::read_level(&src).unwrap();
        let thumb = olr_pack::read_level_thumb(&src).unwrap();
        let thumb = thumb.unwrap_or_else(|| panic!("{}: no thumb.png", path.display()));
        assert_eq!((thumb.width(), thumb.height()), olr_pack::THUMB_SIZE, "{}", path.display());
        assert_eq!(thumb, olr_import_gfx::level_thumbnail(&pack.level), "{}", path.display());
        levels += 1;
    }
    assert!(levels >= 4, "the manifest's four levels at least, found {levels}");
}
