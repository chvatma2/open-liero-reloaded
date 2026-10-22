//! Level packs (format 5, over `olr-format`): `level.ron`, `material.png` (8-bit palette PNG whose indices are
//! `Material` values), `front.png` / `back.png` at `art_scale`, and the
//! optional menu thumbnail `thumb.png`.

use olr_data::image::RgbaImage;
use olr_data::level::{Level, LevelDef};
use olr_data::manifest::{PackKind, PackManifest};
use olr_format::levels::{compile_level, lift_level, read_level_header, write_level_files};
use olr_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{decode_rgba, encode_rgba, png_size};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const LEVEL_FILE: &str = "level.ron";
/// The level's thumbnail for the menus (spec §21 G6), optional: made from
/// the art at import. Presentation only: [`crate::content_hash`] leaves it
/// out (plan 7 ruling Q4), so adding or changing it keeps replays valid.
pub use olr_format::levels::THUMB_FILE;

/// The size thumbnails are made at: the level letterboxed into it.
pub use olr_format::levels::THUMB_SIZE;

pub use olr_format::levels::{MAX_ART_SCALE, MAX_THUMB_BYTES, MAX_THUMB_SIDE};

#[derive(Clone, Debug, PartialEq)]
pub struct LevelPack {
    pub manifest: PackManifest,
    pub level: Level,
    /// `thumb.png`, if the pack has a readable one (at most
    /// [`MAX_THUMB_SIDE`] square).
    pub thumb: Option<RgbaImage>,
    /// The level's look (look system): `None` draws as always. Not
    /// written by [`write_level`] (the importers write none).
    pub look: Option<olr_look::LevelLook>,
    /// The level's glow images.
    pub glow: Option<olr_format::levels::GlowLayers>,
    /// The level's clear flags (`look/clear.png`).
    pub clear: Option<Vec<u8>>,
    /// The level's distant (parallax) layers (`distant/<i>.png`, issue
    /// #11), far to near; empty for most levels.
    pub distant: Vec<olr_data::level::DistantLayer>,
    /// The level's surface maps (`look/surface.png`, `look/surface-back.png`,
    /// issue #37), each world pixel's value in the look's `materials`; read
    /// for the renderer's heat (issue #39). Not written by [`write_level`].
    pub surface: Option<olr_format::levels::SurfaceMaps>,
    /// Each distant layer's surface material as a value in the look's
    /// `materials` (`level.ron`'s `distant[i].surface`, issue #41: the
    /// renderer heats a layer by it), 0 none; past its end, none. Not
    /// written by [`write_level`].
    pub distant_surface: Vec<u8>,
    /// What [`read_level`] left out of a look that did not check.
    pub warnings: Vec<String>,
}

/// Writes the pack in format 5 (`olr-format`'s level files), the
/// thumbnail encoded from `pack.thumb`.
pub fn write_level(sink: &mut dyn ContentSink, pack: &LevelPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Level {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Level"));
    }
    let thumb = match &pack.thumb {
        Some(t) => {
            check_thumb_size(t.width(), t.height())?;
            Some(encode_rgba(t))
        }
        None => None,
    };
    let mut src =
        lift_level(&pack.level, thumb).map_err(|e| PackError::invalid(LEVEL_FILE, e.to_string()))?;
    src.distant = pack.distant.clone();
    write_manifest(sink, &pack.manifest)?;
    write_level_files(sink, &src)?;
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
    decode_thumb(&src.read_max(&path, MAX_THUMB_BYTES)?).map(Some)
}

/// Decodes `thumb.png`'s bytes after checking their length and the size the
/// PNG header gives, so an untrusted pack never makes the caller decode an
/// unbounded image.
pub fn decode_thumb(bytes: &[u8]) -> Result<RgbaImage, PackError> {
    let invalid = |message: String| PackError::invalid(THUMB_FILE, message);
    if bytes.len() as u64 > MAX_THUMB_BYTES {
        return Err(invalid(format!("{} bytes, above {MAX_THUMB_BYTES}", bytes.len())));
    }
    let (w, h) = png_size(bytes).ok_or_else(|| invalid("not a PNG".into()))?;
    check_thumb_size(w, h)?;
    decode_rgba(bytes).map_err(|message| PackError::Image { file: THUMB_FILE.into(), message })
}

/// The manifest and `level.ron` alone, decoding no image: what a menu
/// lists (name, size) without loading the level.
pub fn read_level_def(src: &dyn ContentSource) -> Result<(PackManifest, LevelDef), PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Level {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Level"));
    }
    let h = read_level_header(src)?;
    let def = LevelDef {
        name: h.name,
        width: h.size.0,
        height: h.size.1,
        art_scale: h.art_scale,
        theme: h.theme,
        spawns: h.spawns,
    };
    Ok((manifest, def))
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
    let (src, warnings) = olr_format::levels::read_level_files_lenient(src)?;
    let level = compile_level(&src)?;
    let look = olr_format::levels::compile_look(&src)?;
    let distant_surface = olr_format::levels::distant_surface_values(&src);
    Ok(LevelPack {
        manifest,
        level,
        thumb: None,
        look,
        glow: src.glow,
        clear: src.clear,
        distant: src.distant,
        surface: src.surface,
        distant_surface,
        warnings,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::image_io::encode_indexed;
    use crate::mod_pack::tests::manifest;
    use olr_data::image::Rgba;
    use olr_data::level::{Material, MaterialGrid};
    use olr_vfs::MemFs;

    const MATERIAL_FILE: &str = "material.png";
    const FRONT_FILE: &str = "front.png";

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
            look: None,
            glow: None,
            clear: None,
            distant: Vec::new(),
            surface: None,
            distant_surface: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn roundtrip() {
        let pack = sample_level();
        let mut fs = MemFs::new();
        write_level(&mut fs, &pack).unwrap();
        assert_eq!(read_level(&fs).unwrap(), pack);
    }

    /// Issue #11: distant layers round-trip, are content (hashed) and
    /// known to `validate`.
    #[test]
    fn distant_layers_roundtrip_and_are_content() {
        let mut pack = sample_level();
        let mut image = RgbaImage::new(4, 2);
        image.set(1, 1, Rgba::rgb(4, 5, 6));
        pack.distant = vec![olr_data::level::DistantLayer { image, at: (-1, 2), depth: 70 }];
        let mut fs = MemFs::new();
        write_level(&mut fs, &pack).unwrap();
        assert_eq!(read_level(&fs).unwrap(), pack);
        assert!(crate::validate(&fs).is_empty(), "{:?}", crate::validate(&fs));
        let mut other = fs.clone();
        pack.distant[0].depth = 71;
        write_level(&mut other, &pack).unwrap();
        assert_ne!(crate::content_hash(&fs).unwrap(), crate::content_hash(&other).unwrap());
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
        let huge = vec![0u8; MAX_THUMB_BYTES as usize + 1];
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
            fn read(&self, path: &VPath) -> Result<Vec<u8>, olr_vfs::VfsError> {
                assert_ne!(path.as_str(), FRONT_FILE, "front.png must not be read");
                self.0.read(path)
            }
            fn list(&self, dir: &VPath) -> Result<Vec<String>, olr_vfs::VfsError> {
                self.0.list(dir)
            }
            fn read_max(&self, path: &VPath, limit: u64) -> Result<Vec<u8>, olr_vfs::VfsError> {
                if path.as_str() == FRONT_FILE {
                    let len = 8 << 30;
                    return Err(olr_vfs::VfsError::TooLarge { path: path.to_string(), len, limit });
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
        assert!(err.contains("not within"), "{err}");
    }

    #[test]
    fn rejects_unknown_material_index() {
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        let bad = encode_indexed(2, 2, &[0, 1, 2, 7], &[0; 24]);
        let fs = fs.with(MATERIAL_FILE, bad);
        let err = read_level(&fs).unwrap_err().to_string();
        assert!(err.contains("unknown material index 7 at (1, 1)"), "{err}");
    }

    #[test]
    fn rejects_rgb_material_png() {
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        let fs = fs.with(MATERIAL_FILE, encode_rgba(&RgbaImage::new(2, 2)));
        assert!(read_level(&fs).unwrap_err().to_string().contains("palette"));
    }
}
