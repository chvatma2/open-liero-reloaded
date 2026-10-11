//! Level packs: `level.ron`, `material.png` (8-bit palette PNG whose indices are
//! [`Material`] values), `front.png` / `back.png` at `art_scale`, and the
//! optional menu thumbnail `thumb.png`.

use rl_data::image::RgbaImage;
use rl_data::level::{Level, LevelDef, Material, MaterialGrid};
use rl_data::manifest::{PackKind, PackManifest};
use rl_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{
    LEVEL_ART, decode_indexed_within, decode_rgba, decode_rgba_within, encode_indexed, encode_rgba, png_size,
};
use crate::ron_io::{read_file, read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const LEVEL_FILE: &str = "level.ron";
pub const MATERIAL_FILE: &str = "material.png";
pub const FRONT_FILE: &str = "front.png";
pub const BACK_FILE: &str = "back.png";
/// The level's thumbnail for the menus (spec §21 G6), optional: made from
/// the art at import. Presentation only: [`crate::content_hash`] leaves it
/// out (plan 7 ruling Q4), so adding or changing it keeps replays valid.
pub const THUMB_FILE: &str = "thumb.png";

/// The size importers make thumbnails at: the level letterboxed into it.
pub const THUMB_SIZE: (u32, u32) = (160, 96);

/// The widest and tallest `thumb.png` read; a larger one is refused before
/// it is decoded (a pack must not make the menus decode a huge image).
pub const MAX_THUMB_SIDE: u32 = 512;

/// The largest `thumb.png` read, in bytes. A 512 × 512 thumbnail that
/// compresses badly may exceed it and be refused (not shown), which is
/// harmless: importers write 160 × 96.
pub const MAX_THUMB_BYTES: usize = 1 << 20;

/// Palette for `material.png`, indexed by `Material as u8`: rock is gray,
/// background black, dirt brown, so the file is readable in any image viewer.
const MATERIAL_PALETTE: [u8; 9] = [128, 128, 128, 0, 0, 0, 139, 90, 43];

#[derive(Clone, Debug, PartialEq)]
pub struct LevelPack {
    pub manifest: PackManifest,
    pub level: Level,
    /// `thumb.png`, if the pack has a readable one (at most
    /// [`MAX_THUMB_SIDE`] square).
    pub thumb: Option<RgbaImage>,
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
    if let Some(t) = &pack.thumb {
        check_thumb_size(t.width(), t.height())?;
        sink.write(&VPath::new(THUMB_FILE)?, &encode_rgba(t))?;
    }
    Ok(())
}

fn check_thumb_size(w: u32, h: u32) -> Result<(), PackError> {
    if w == 0 || h == 0 || w > MAX_THUMB_SIDE || h > MAX_THUMB_SIDE {
        return Err(PackError::invalid(
            THUMB_FILE,
            format!("{w}x{h}, not within 1x1 to {MAX_THUMB_SIDE}x{MAX_THUMB_SIDE}"),
        ));
    }
    Ok(())
}

/// The level pack's `thumb.png`: `Ok(None)` when it has none, an error
/// when it is not a PNG or above [`MAX_THUMB_BYTES`] or [`MAX_THUMB_SIDE`]
/// (both checked before decoding).
pub fn read_level_thumb(src: &dyn ContentSource) -> Result<Option<RgbaImage>, PackError> {
    let path = VPath::new(THUMB_FILE)?;
    if !src.exists(&path) {
        return Ok(None);
    }
    decode_thumb(&src.read_max(&path, MAX_THUMB_BYTES as u64)?).map(Some)
}

/// Decodes `thumb.png`'s bytes after checking their length and the size the
/// PNG header gives, so an untrusted pack never makes the caller decode an
/// unbounded image.
pub fn decode_thumb(bytes: &[u8]) -> Result<RgbaImage, PackError> {
    let invalid = |message: String| PackError::invalid(THUMB_FILE, message);
    if bytes.len() > MAX_THUMB_BYTES {
        return Err(invalid(format!("{} bytes, above {MAX_THUMB_BYTES}", bytes.len())));
    }
    let (w, h) = png_size(bytes).ok_or_else(|| invalid("not a PNG".into()))?;
    check_thumb_size(w, h)?;
    decode_rgba(bytes).map_err(|message| PackError::Image { file: THUMB_FILE.into(), message })
}

/// A level's art, within [`LEVEL_ART`].
fn read_png(src: &dyn ContentSource, file: &str) -> Result<RgbaImage, PackError> {
    let bytes = read_file(src, &VPath::new(file)?)?;
    decode_rgba_within(&bytes, LEVEL_ART).map_err(|message| PackError::Image { file: file.into(), message })
}

/// The manifest and `level.ron` alone, decoding no image: what a menu
/// lists (name, size) without loading the level.
pub fn read_level_def(src: &dyn ContentSource) -> Result<(PackManifest, LevelDef), PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Level {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Level"));
    }
    Ok((manifest, read_ron(src, LEVEL_FILE)?))
}

/// Reads a level pack, every file bounded ([`crate::MAX_FILE_BYTES`]).
/// The thumbnail is presentation only and not read (`thumb: None`): a
/// match or a replay never pays for it, and a bad one never keeps the
/// level from being played. The menus read it with [`read_level_thumb`];
/// [`crate::validate`] warns about a bad one.
pub fn read_level(src: &dyn ContentSource) -> Result<LevelPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Level {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Level"));
    }
    let def: LevelDef = read_ron(src, LEVEL_FILE)?;
    let bytes = read_file(src, &VPath::new(MATERIAL_FILE)?)?;
    let (w, h, indices) = decode_indexed_within(&bytes, LEVEL_ART)
        .map_err(|message| PackError::Image { file: MATERIAL_FILE.into(), message })?;
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
    Ok(LevelPack { manifest, level, thumb: None })
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
            thumb: None,
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
    fn thumbnail_roundtrips_and_a_bad_one_is_left_out() {
        let mut pack = sample_level();
        let mut thumb = RgbaImage::new(THUMB_SIZE.0, THUMB_SIZE.1);
        thumb.set(5, 7, Rgba::rgb(9, 8, 7));
        pack.thumb = Some(thumb);
        let mut fs = MemFs::new();
        write_level(&mut fs, &pack).unwrap();
        // `read_level` leaves the thumbnail out; `read_level_thumb` reads it.
        assert_eq!(read_level_thumb(&fs).unwrap(), pack.thumb);
        let thumb = pack.thumb.take();
        assert_eq!(read_level(&fs).unwrap(), pack);
        pack.thumb = thumb;
        // Not a PNG, too large by its header or by its length: the level
        // still reads, the thumbnail does not.
        let huge = vec![0u8; MAX_THUMB_BYTES + 1];
        for bad in [b"not a png".to_vec(), encode_rgba(&RgbaImage::new(MAX_THUMB_SIDE + 1, 1)), huge.clone()]
        {
            let fs = fs.clone().with(THUMB_FILE, bad.clone());
            assert!(read_level(&fs).unwrap().thumb.is_none());
            assert!(read_level_thumb(&fs).is_err());
        }
        assert!(decode_thumb(&huge).unwrap_err().to_string().contains("bytes"));
        // The writer refuses what the reader would.
        pack.thumb = Some(RgbaImage::new(0, 3));
        assert!(write_level(&mut MemFs::new(), &pack).is_err());
    }

    #[test]
    fn level_def_reads_without_images() {
        let pack = sample_level();
        let mut fs = MemFs::new();
        write_level(&mut fs, &pack).unwrap();
        let fs = fs.with(FRONT_FILE, b"not a png".to_vec());
        assert_eq!(read_level_def(&fs).unwrap(), (pack.manifest, pack.level.def));
        let fs = fs.with(LEVEL_FILE, b"(".to_vec());
        assert!(read_level_def(&fs).is_err());
    }

    /// A level file longer than `MAX_FILE_BYTES` is refused unread, never
    /// an allocation of its size (a source that knows lengths: `DirFs`;
    /// here one that reports a huge length without holding the bytes).
    #[test]
    fn oversized_level_files_are_refused_unread() {
        struct Huge(MemFs);
        impl ContentSource for Huge {
            fn read(&self, path: &VPath) -> Result<Vec<u8>, rl_vfs::VfsError> {
                assert_ne!(path.as_str(), FRONT_FILE, "front.png must not be read");
                self.0.read(path)
            }
            fn list(&self, dir: &VPath) -> Result<Vec<String>, rl_vfs::VfsError> {
                self.0.list(dir)
            }
            fn read_max(&self, path: &VPath, limit: u64) -> Result<Vec<u8>, rl_vfs::VfsError> {
                if path.as_str() == FRONT_FILE {
                    let len = 8 << 30;
                    return Err(rl_vfs::VfsError::TooLarge { path: path.to_string(), len, limit });
                }
                self.0.read_max(path, limit)
            }
        }
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        let err = read_level(&Huge(fs)).unwrap_err().to_string();
        assert!(err.contains("front.png") && err.contains(&crate::MAX_FILE_BYTES.to_string()), "{err}");
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
