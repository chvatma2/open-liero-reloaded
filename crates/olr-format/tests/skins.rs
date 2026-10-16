//! Every bundled skin: lifted from today's pack, written, read and
//! compiled, it draws the same: the same frame windows (masks, and pixels
//! where visible) and the same colorized frames.

use std::path::PathBuf;

use olr_data::image::Rgba;
use olr_data::skin::MaskKind;
use olr_format::skins::{compile_skin, lift_skin, read_skin_files, write_skin_files};
use olr_vfs::{DirFs, MemFs};

#[test]
fn every_bundled_skin_draws_the_same() {
    let Some(root) = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir()) else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let colours = [Rgba::rgb(255, 0, 0), Rgba::rgb(96, 128, 252), Rgba::rgb(10, 200, 30)];
    let mut n = 0;
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        let id = path.file_name().unwrap().to_string_lossy().into_owned();
        if !id.starts_with("olx-skin-") {
            continue;
        }
        let old = olr_pack::read_skin(&DirFs::new(&path)).unwrap().skin;
        let src = lift_skin(&old).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_skin_files(&mut fs, &src).unwrap();
        let back = read_skin_files(&fs).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(back, src, "{id}");
        let new = compile_skin(&back).unwrap_or_else(|e| panic!("{id}: {e}"));
        for f in 0..21u32 {
            for y in 0..18 {
                for x in 0..20 {
                    let sx = f * 32 + 4 + x;
                    let (mo, mn) = (old.mask_at(sx, y), new.mask_at(sx, y));
                    assert_eq!(mo, mn, "{id}: mask frame {f} ({x}, {y})");
                    if mo != MaskKind::Transparent {
                        assert_eq!(
                            old.image.get(sx, y),
                            new.image.get(sx, y),
                            "{id}: pixel frame {f} ({x}, {y})"
                        );
                    }
                }
            }
        }
        for c in colours {
            let (a, b) = (old.colorize(c), new.colorize(c));
            for f in 0..21u32 {
                assert_eq!(
                    a.crop(f * 32 + 4, 0, 20, 18),
                    b.crop(f * 32 + 4, 0, 20, 18),
                    "{id}: colour {c:?} frame {f}"
                );
            }
        }
        n += 1;
    }
    assert!(n >= 700, "only {n} skins found");
}
