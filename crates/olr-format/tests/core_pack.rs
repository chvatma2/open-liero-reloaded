//! The bundled core pack, and the bundled mods' gfx: OLX's colour key
//! becomes alpha once, and nothing a loader keys changes (spec §5, §6).

use std::path::PathBuf;

use olr_format::core_pack::{compile_core, lift_core, read_core_files, write_core_files};
use olr_format::image_io::{decode_rgba, olx_sprite_to_alpha};
use olr_vfs::{DirFs, MemFs};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

fn keyed(img: &olr_data::image::RgbaImage) -> olr_data::image::RgbaImage {
    let mut k = img.clone();
    k.key_transparent();
    k
}

#[test]
fn the_bundled_core_pack_survives_lift_write_read_and_compile() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let pack = olr_pack::read_core(&DirFs::new(root.join("olx-core"))).unwrap();
    let src = lift_core(&pack.def, &pack.sprites, &pack.sounds).unwrap();
    let mut fs = MemFs::new();
    write_core_files(&mut fs, &src).unwrap();
    let back = read_core_files(&fs).unwrap();
    assert_eq!(back, src);
    let (def, sprites, sounds) = compile_core(&back).unwrap();
    assert_eq!(def, pack.def, "CoreDef");
    assert_eq!(sounds, pack.sounds, "sounds");
    assert_eq!(sprites.keys().collect::<Vec<_>>(), pack.sprites.keys().collect::<Vec<_>>());
    for (name, old) in &pack.sprites {
        let new = &sprites[name];
        assert_eq!(*new, keyed(old), "{name}: the alpha image is what a loader's key gives");
        assert_eq!(keyed(new), *new, "{name}: a loader keying it again changes nothing");
    }
}

#[test]
fn the_bundled_mods_gfx_key_to_alpha_without_change() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    for id in ["olx-classic", "olx-mw-1-0"] {
        let gfx = root.join(id).join("gfx");
        for entry in std::fs::read_dir(&gfx).unwrap() {
            let path = entry.unwrap().path();
            let img = decode_rgba(&std::fs::read(&path).unwrap()).unwrap();
            let alpha = olx_sprite_to_alpha(&img);
            assert_eq!(alpha, keyed(&img), "{}", path.display());
            assert_eq!(keyed(&alpha), alpha, "{}", path.display());
            assert_eq!(
                (alpha.width(), alpha.height()),
                (img.width(), img.height()),
                "frames stay width / height"
            );
        }
    }
}
