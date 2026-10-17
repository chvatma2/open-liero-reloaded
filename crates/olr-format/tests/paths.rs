//! `file_paths` names exactly the files each writer writes (plan 13d's
//! minors: the paths without encoding a single image).

use std::path::PathBuf;

use olr_data::manifest::PackKind;
use olr_format::{core_pack, levels, mods, skins, themes};
use olr_vfs::{DirFs, MemFs};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

#[test]
fn file_paths_are_the_writers_paths() {
    let Some(root) = content() else { return };
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        let src = DirFs::new(&dir);
        let kind = olr_pack::read_manifest(&src).unwrap().kind;
        let mut written = MemFs::new();
        let paths = match kind {
            PackKind::Mod => {
                let s = mods::read_mod_files(&src).unwrap();
                mods::write_mod_files(&mut written, &s).unwrap();
                mods::file_paths(&s)
            }
            PackKind::Level => {
                let s = levels::read_level_files(&src).unwrap();
                levels::write_level_files(&mut written, &s).unwrap();
                levels::file_paths(&s)
            }
            PackKind::Skin => {
                let s = skins::read_skin_files(&src).unwrap();
                skins::write_skin_files(&mut written, &s).unwrap();
                skins::file_paths(&s)
            }
            PackKind::Theme => {
                let s = themes::read_theme_files(&src).unwrap();
                themes::write_theme_files(&mut written, &s).unwrap();
                themes::file_paths(&s)
            }
            PackKind::Core => {
                let s = core_pack::read_core_files(&src).unwrap();
                core_pack::write_core_files(&mut written, &s).unwrap();
                core_pack::file_paths(&s)
            }
        };
        let want: Vec<String> = written.files().map(|(p, _)| p.as_str().to_string()).collect();
        let mut got = paths;
        got.sort();
        assert_eq!(got, want, "{}", dir.display());
    }
}
