//! The bundled theme: today's `olr_pack::read_theme`, lifted, written, read
//! and compiled, gives the same `Theme` and so the same stamps and green
//! mask for the simulation.

use std::path::PathBuf;

use olr_format::themes::{compile_theme, lift_theme, read_theme_files, write_theme_files};
use olr_terrain::{GreenMask, Stamp};
use olr_vfs::{DirFs, MemFs};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

#[test]
fn the_bundled_theme_survives_lift_write_read_and_compile() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let pack = olr_pack::read_theme(&DirFs::new(root.join("olx-theme-dirt"))).unwrap();
    let src = lift_theme(&pack.theme).unwrap();
    let mut fs = MemFs::new();
    write_theme_files(&mut fs, &src).unwrap();
    let back = read_theme_files(&fs).unwrap();
    assert_eq!(back, src);
    let theme = compile_theme(&back).unwrap();
    assert_eq!(theme, pack.theme, "Theme");
    for (a, b) in theme.holes.iter().zip(&pack.theme.holes) {
        assert_eq!(Stamp::from_image(a), Stamp::from_image(b));
    }
    let gm = |t: &olr_data::theme::Theme| t.green_ball.as_ref().map(GreenMask::from_image);
    assert_eq!(gm(&theme), gm(&pack.theme));
}
