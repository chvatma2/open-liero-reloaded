//! The level authoring format: `level.ron`, `material.png`, `front.png`,
//! `back.png` and an optional `thumb.png` (spec §6), and an optional
//! composition under `editor/` that bakes into them (plan 14a).

mod composition;
pub use composition::*;
mod files;
pub(crate) use files::LevelFile;
mod thumb;

use olr_data::image::RgbaImage;
use olr_data::level::{Level, LevelDef, Material, MaterialGrid, SpawnPoint};
use serde::{Deserialize, Serialize};

pub use files::{
    file_paths, read_level_files, read_level_files_without_thumb, read_level_header, write_level_files,
};
pub use thumb::{THUMB_SIZE, letterbox, thumbnail};

use crate::image_io::{LEVEL_ART, check_size};
use crate::{FormatError, LiftError};

/// The largest `art_scale` (a level's art pixels per world pixel).
pub const MAX_ART_SCALE: u32 = 8;
/// The most objects in `level.ron`.
pub const MAX_OBJECTS: usize = 4096;
/// The largest level side in world pixels (OLX's 12-bit positions).
pub const MAX_SIDE: u32 = 4096;
/// `material.png`'s palette, indexed by `Material as u8`: rock grey,
/// background black, dirt brown.
pub const MATERIAL_PALETTE: [u8; 9] = [128, 128, 128, 0, 0, 0, 139, 90, 43];

pub(crate) const LEVEL_FILE: &str = "level.ron";
pub(crate) const MATERIAL_FILE: &str = "material.png";
pub(crate) const FRONT_FILE: &str = "front.png";
pub(crate) const BACK_FILE: &str = "back.png";
/// The thumbnail's file name; `olr-pack` re-exports it.
pub const THUMB_FILE: &str = "thumb.png";
/// The largest `thumb.png` read, in bytes. The reader leaves a larger or
/// undecodable one out (presentation only); the writer refuses it.
/// `olr-pack` re-exports it.
pub const MAX_THUMB_BYTES: u64 = 1 << 20;
/// The widest and tallest `thumb.png` read, checked on the PNG header
/// before any decoding (a pack must not make the menus decode a huge
/// image). `olr-pack` re-exports it.
pub const MAX_THUMB_SIDE: u32 = 512;

/// `material` (`size`, row-major) as `material.png` holds it: an 8-bit
/// palette PNG of `MATERIAL_PALETTE`, indexed by `Material as u8`. The
/// writer and a raster element's material (`olr_edit::compose`) share it,
/// so a level composed of its raster base bakes back to its own bytes.
pub fn encode_material(size: (u32, u32), material: &[Material]) -> Vec<u8> {
    let indices: Vec<u8> = material.iter().map(|m| *m as u8).collect();
    crate::image_io::encode_indexed(size.0, size.1, &indices, &MATERIAL_PALETTE)
}

/// What `level.ron` alone says, read without touching any image.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelHeader {
    pub name: String,
    /// World pixels.
    pub size: (u32, u32),
    pub art_scale: u32,
    /// Theme pack id.
    pub theme: String,
    pub spawns: Vec<SpawnPoint>,
    /// `level.ron`'s objects as written (with their notes).
    pub objects: Vec<LevelObject>,
}

/// A level as its files hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelSource {
    pub name: String,
    pub notes: String,
    /// World pixels.
    pub size: (u32, u32),
    /// `1..=MAX_ART_SCALE`.
    pub art_scale: u32,
    /// Theme pack id.
    pub theme: String,
    pub objects: Vec<LevelObject>,
    /// `size.0 * size.1`, row-major.
    pub material: Vec<Material>,
    /// `size` × `art_scale`.
    pub front: RgbaImage,
    /// `size` × `art_scale`.
    pub back: RgbaImage,
    /// `thumb.png`'s bytes, passed through (presentation only).
    pub thumb: Option<Vec<u8>>,
}

/// A thing placed in a level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum LevelObject {
    /// A spawn point (world pixels). The 0.2 runtime does not read spawns yet.
    Spawn {
        /// World pixels (x, y), origin top-left, y down; outside the level
        /// is a warning of `olr-cli pack check`.
        at: (i32, i32),
        /// Free text for authors; ignored by the game.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        notes: String,
    },
}

/// What `level.ron` alone says: size, art scale, objects, and the art size
/// they imply, which must be within what the reader decodes
/// ([`LEVEL_ART`]). Returns the art size.
pub(crate) fn check_header(
    size: (u32, u32),
    art_scale: u32,
    objects: usize,
) -> Result<(u32, u32), FormatError> {
    let (w, h) = size;
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "size",
            format!("{w}x{h}, not within 1x1 to {MAX_SIDE}x{MAX_SIDE}"),
        ));
    }
    if !(1..=MAX_ART_SCALE).contains(&art_scale) {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "art_scale",
            format!("{art_scale}, not within 1 to {MAX_ART_SCALE}"),
        ));
    }
    if objects > MAX_OBJECTS {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "objects",
            format!("{objects} objects, above {MAX_OBJECTS}"),
        ));
    }
    // Both are bounded above, so this cannot overflow.
    let (aw, ah) = (w * art_scale, h * art_scale);
    check_size(aw, ah, LEVEL_ART).map_err(|e| {
        FormatError::invalid(
            LEVEL_FILE,
            "size",
            format!("size {w}x{h} at art_scale {art_scale} makes art of {aw}x{ah}: {e}"),
        )
    })?;
    Ok((aw, ah))
}

/// Whether `thumb.png`'s bytes are within [`MAX_THUMB_BYTES`] and
/// [`MAX_THUMB_SIDE`], from the PNG header alone (nothing is decoded).
pub(crate) fn check_thumb(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_THUMB_BYTES {
        return Err(format!("{} bytes, above {MAX_THUMB_BYTES}", bytes.len()));
    }
    let (w, h) = crate::image_io::png_size(bytes).ok_or("not a PNG")?;
    if w == 0 || h == 0 || w > MAX_THUMB_SIDE || h > MAX_THUMB_SIDE {
        return Err(format!("{w}x{h}, not within 1x1 to {MAX_THUMB_SIDE}x{MAX_THUMB_SIDE}"));
    }
    Ok(())
}

/// The checks `compile_level` and the writer share.
pub(crate) fn check(src: &LevelSource) -> Result<(), FormatError> {
    let (aw, ah) = check_header(src.size, src.art_scale, src.objects.len())?;
    let (w, h) = src.size;
    let cells = w as usize * h as usize;
    if src.material.len() != cells {
        return Err(FormatError::invalid(
            MATERIAL_FILE,
            "material",
            format!("{} cells, the level needs {cells} ({w}x{h})", src.material.len()),
        ));
    }
    if let Some(t) = &src.thumb {
        check_thumb(t).map_err(|m| FormatError::invalid(THUMB_FILE, "", m))?;
    }
    for (file, img) in [(FRONT_FILE, &src.front), (BACK_FILE, &src.back)] {
        if (img.width(), img.height()) != (aw, ah) {
            return Err(FormatError::invalid(
                file,
                "",
                format!(
                    "{file} is {}x{}, the level needs {aw}x{ah} (size × art_scale)",
                    img.width(),
                    img.height()
                ),
            ));
        }
    }
    Ok(())
}

/// Makes the runtime level.
pub fn compile_level(src: &LevelSource) -> Result<Level, FormatError> {
    check(src)?;
    let spawns = src
        .objects
        .iter()
        .map(|o| match o {
            LevelObject::Spawn { at, .. } => SpawnPoint { x: at.0, y: at.1 },
        })
        .collect();
    let def = LevelDef {
        name: src.name.clone(),
        width: src.size.0,
        height: src.size.1,
        art_scale: src.art_scale,
        theme: src.theme.clone(),
        spawns,
    };
    let material = MaterialGrid::from_cells(src.size.0, src.size.1, src.material.clone())
        .ok_or_else(|| FormatError::invalid(MATERIAL_FILE, "material", "wrong cell count"))?;
    Ok(Level { def, material, front: src.front.clone(), back: src.back.clone() })
}

/// The inverse of [`compile_level`]; `notes` stay empty.
pub fn lift_level(level: &Level, thumb: Option<Vec<u8>>) -> Result<LevelSource, LiftError> {
    let d = &level.def;
    let src = LevelSource {
        name: d.name.clone(),
        notes: String::new(),
        size: (d.width, d.height),
        art_scale: d.art_scale,
        theme: d.theme.clone(),
        objects: d
            .spawns
            .iter()
            .map(|s| LevelObject::Spawn { at: (s.x, s.y), notes: String::new() })
            .collect(),
        material: level.material.cells().to_vec(),
        front: level.front.clone(),
        back: level.back.clone(),
        thumb,
    };
    if (level.material.width(), level.material.height()) != src.size {
        return Err(LiftError {
            item: format!("level {:?}", d.name),
            message: "the material grid is not the level's size".into(),
        });
    }
    check(&src).map_err(|e| LiftError { item: format!("level {:?}", d.name), message: e.to_string() })?;
    Ok(src)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use olr_data::image::Rgba;

    pub(crate) fn tiny(art_scale: u32) -> LevelSource {
        let (w, h) = (3, 2);
        let mut front = RgbaImage::new(w * art_scale, h * art_scale);
        front.set(0, 0, Rgba::rgb(1, 2, 3));
        LevelSource {
            name: "Tiny".into(),
            notes: String::new(),
            size: (w, h),
            art_scale,
            theme: "olx-theme-dirt".into(),
            objects: vec![LevelObject::Spawn { at: (1, 1), notes: "here".into() }],
            material: vec![
                Material::Rock,
                Material::Background,
                Material::Dirt,
                Material::Dirt,
                Material::Background,
                Material::Rock,
            ],
            front,
            back: RgbaImage::new(w * art_scale, h * art_scale),
            thumb: None,
        }
    }

    #[test]
    fn compile_and_lift_are_inverse() {
        for scale in [1, 2] {
            let src = tiny(scale);
            let level = compile_level(&src).unwrap();
            assert_eq!(level.def.spawns, vec![olr_data::level::SpawnPoint { x: 1, y: 1 }]);
            assert_eq!(level.material.get(2, 0), Material::Dirt);
            let mut back = lift_level(&level, None).unwrap();
            assert_eq!(back.objects, vec![LevelObject::Spawn { at: (1, 1), notes: String::new() }]);
            back.objects = src.objects.clone(); // notes are authoring-only
            assert_eq!(back, src);
        }
    }

    #[test]
    fn sizes_must_agree() {
        let mut s = tiny(1);
        s.material.pop();
        assert!(compile_level(&s).unwrap_err().to_string().contains("material"));
        let mut s = tiny(2);
        s.front = RgbaImage::new(6, 3);
        let e = compile_level(&s).unwrap_err().to_string();
        assert!(e.contains("front.png") && e.contains("6x4"), "{e}");
        let mut s = tiny(1);
        s.art_scale = 9;
        assert!(compile_level(&s).is_err());
        let mut s = tiny(1);
        s.size = (5000, 1);
        assert!(compile_level(&s).is_err());
    }

    /// A saved file must reopen: the reader decodes art within `LEVEL_ART`,
    /// so the check refuses what it would refuse (the art is a mismatch
    /// here, but the bound is reported first, so no big image is built).
    #[test]
    fn art_above_the_decode_bound_is_refused() {
        let mut s = tiny(8);
        s.size = (1100, 1000);
        s.material = vec![Material::Rock; 1_100_000];
        let e = compile_level(&s).unwrap_err().to_string();
        assert!(e.contains("level.ron") && e.contains("1100x1000") && e.contains("art_scale 8"), "{e}");
        assert!(e.contains("pixels"), "{e}");
        let mut out = olr_vfs::MemFs::new();
        assert!(write_level_files(&mut out, &s).is_err());
        // Just inside: 1000 x 1000 at 8 is 64 Mpx, exactly the bound.
        assert!(check_header((1000, 1000), 8, 0).is_ok());
    }
}
