//! Level packs: `level.ron`, `material.png` (8-bit palette PNG whose indices are
//! [`Material`] values), and `front.png` / `back.png` at `art_scale`.

use rl_data::image::RgbaImage;
use rl_data::level::{Level, LevelDef, Material, MaterialGrid};
use rl_data::manifest::{PackKind, PackManifest};
use rl_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{decode_indexed, decode_rgba, encode_indexed, encode_rgba};
use crate::ron_io::{read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const LEVEL_FILE: &str = "level.ron";
pub const MATERIAL_FILE: &str = "material.png";
pub const FRONT_FILE: &str = "front.png";
pub const BACK_FILE: &str = "back.png";

/// Palette for `material.png`, indexed by `Material as u8`: rock is gray,
/// background black, dirt brown, so the file is readable in any image viewer.
const MATERIAL_PALETTE: [u8; 9] = [128, 128, 128, 0, 0, 0, 139, 90, 43];

#[derive(Clone, Debug, PartialEq)]
pub struct LevelPack {
    pub manifest: PackManifest,
    pub level: Level,
}

fn check_dims(level: &Level) -> Result<(), PackError> {
    let d = &level.def;
    if d.art_scale == 0 {
        return Err(PackError::invalid(LEVEL_FILE, "art_scale must be at least 1"));
    }
    if (level.material.width(), level.material.height()) != (d.width, d.height) {
        return Err(PackError::invalid(MATERIAL_FILE, format!("material must be {}x{}", d.width, d.height)));
    }
    let (Some(aw), Some(ah)) = (d.width.checked_mul(d.art_scale), d.height.checked_mul(d.art_scale)) else {
        return Err(PackError::invalid(LEVEL_FILE, "size x art_scale is too large"));
    };
    for (file, img) in [(FRONT_FILE, &level.front), (BACK_FILE, &level.back)] {
        if (img.width(), img.height()) != (aw, ah) {
            return Err(PackError::invalid(file, format!("art must be {aw}x{ah} (size x art_scale)")));
        }
    }
    Ok(())
}

pub fn write_level(sink: &mut dyn ContentSink, pack: &LevelPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Level {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Level"));
    }
    check_dims(&pack.level)?;
    let l = &pack.level;
    write_manifest(sink, &pack.manifest)?;
    write_ron(sink, LEVEL_FILE, &l.def)?;
    let indices: Vec<u8> = l.material.cells().iter().map(|m| *m as u8).collect();
    let mat_png = encode_indexed(l.material.width(), l.material.height(), &indices, &MATERIAL_PALETTE);
    sink.write(&VPath::new(MATERIAL_FILE)?, &mat_png)?;
    sink.write(&VPath::new(FRONT_FILE)?, &encode_rgba(&l.front))?;
    sink.write(&VPath::new(BACK_FILE)?, &encode_rgba(&l.back))?;
    Ok(())
}

fn read_png(src: &dyn ContentSource, file: &str) -> Result<RgbaImage, PackError> {
    let bytes = src.read(&VPath::new(file)?)?;
    decode_rgba(&bytes).map_err(|message| PackError::Image { file: file.into(), message })
}

pub fn read_level(src: &dyn ContentSource) -> Result<LevelPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Level {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Level"));
    }
    let def: LevelDef = read_ron(src, LEVEL_FILE)?;
    let bytes = src.read(&VPath::new(MATERIAL_FILE)?)?;
    let (w, h, indices) =
        decode_indexed(&bytes).map_err(|message| PackError::Image { file: MATERIAL_FILE.into(), message })?;
    let cells = indices
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            Material::from_index(v).ok_or_else(|| {
                let (x, y) = (i as u32 % w.max(1), i as u32 / w.max(1));
                PackError::invalid(MATERIAL_FILE, format!("unknown material index {v} at ({x},{y})"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let material = MaterialGrid::from_cells(w, h, cells).expect("decoder returns w*h indices");
    let level = Level { def, material, front: read_png(src, FRONT_FILE)?, back: read_png(src, BACK_FILE)? };
    check_dims(&level)?;
    Ok(LevelPack { manifest, level })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use rl_data::image::Rgba;
    use rl_vfs::MemFs;

    pub(crate) fn sample_level() -> LevelPack {
        let cells = vec![Material::Rock, Material::Background, Material::Dirt, Material::Dirt];
        let mut front = RgbaImage::new(4, 4);
        front.set(3, 3, Rgba::rgb(1, 2, 3));
        LevelPack {
            manifest: manifest(PackKind::Level),
            level: Level {
                def: LevelDef {
                    name: "Tiny".into(),
                    width: 2,
                    height: 2,
                    art_scale: 2,
                    theme: "olx-theme-dirt".into(),
                    spawns: vec![],
                },
                material: MaterialGrid::from_cells(2, 2, cells).unwrap(),
                front,
                back: RgbaImage::new(4, 4),
            },
        }
    }

    #[test]
    fn roundtrip() {
        let pack = sample_level();
        let mut fs = MemFs::new();
        write_level(&mut fs, &pack).unwrap();
        assert_eq!(read_level(&fs).unwrap(), pack);
    }

    #[test]
    fn rejects_art_of_wrong_size() {
        let mut pack = sample_level();
        pack.level.front = RgbaImage::new(3, 4);
        assert!(write_level(&mut MemFs::new(), &pack).unwrap_err().to_string().contains("front.png"));
    }

    #[test]
    fn huge_art_scale_is_an_error_not_a_panic() {
        let mut pack = sample_level();
        pack.level.def.width = 70_000;
        pack.level.def.height = 1;
        pack.level.def.art_scale = 70_000;
        pack.level.material = MaterialGrid::new(70_000, 1, Material::Dirt);
        let err = write_level(&mut MemFs::new(), &pack).unwrap_err().to_string();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn rejects_unknown_material_index() {
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        let bad = encode_indexed(2, 2, &[0, 1, 2, 7], &[0; 24]);
        let fs = fs.with(MATERIAL_FILE, bad);
        let err = read_level(&fs).unwrap_err().to_string();
        assert!(err.contains("unknown material index 7 at (1,1)"), "{err}");
    }

    #[test]
    fn rejects_rgb_material_png() {
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        let fs = fs.with(MATERIAL_FILE, encode_rgba(&RgbaImage::new(2, 2)));
        assert!(read_level(&fs).unwrap_err().to_string().contains("palette"));
    }
}
