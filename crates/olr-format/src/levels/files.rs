//! A level's files. Other paths (`pack.ron`, ...) belong to `olr-pack`
//! and are ignored here.

use olr_data::level::Material;
use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::{
    BACK_FILE, FRONT_FILE, GLOW_BACK_FILE, GLOW_FILE, GROUPS_BACK_FILE, GROUPS_FILE, LEVEL_FILE, LOOK_FILES,
    LevelHeader, LevelObject, LevelSource, MATERIAL_FILE, MAX_THUMB_BYTES, THUMB_FILE, check, check_header,
    check_thumb,
};
use crate::FormatError;
use crate::image_io::{self, IMAGES, LEVEL_ART};
use crate::look::LookSource;
use crate::ron_io::{MAX_FILE_BYTES, from_canonical, read_text, to_canonical, write_text};
use olr_terrain::GlowLayers;

fn default_scale() -> u32 {
    1
}

fn is_one(v: &u32) -> bool {
    *v == 1
}

/// `level.ron`: what the level is, besides its images.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LevelFile {
    /// Shown in menus.
    name: String,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    /// Width and height in world pixels, each 1 to `MAX_SIDE` (4096);
    /// `material.png` is this size.
    size: (u32, u32),
    /// Art pixels per world pixel, 1 to `MAX_ART_SCALE` (8): `front.png`
    /// and `back.png` are `size` × `art_scale` (at most 64 Mi pixels).
    #[serde(default = "default_scale", skip_serializing_if = "is_one")]
    art_scale: u32,
    /// The id of the theme pack whose holes and green dirt the level uses.
    theme: String,
    /// Things placed in the level, at most `MAX_OBJECTS` (4096); an editor
    /// addresses them by index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    objects: Vec<LevelObject>,
    /// The level's look (darkness, light settings, grade, glow groups); none
    /// draws as OLX did. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    look: Option<LookSource>,
}

fn write_bytes(sink: &mut dyn ContentSink, file: &str, bytes: &[u8]) -> Result<(), FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    sink.write(&VPath::new(file).map_err(|e| io(e.to_string()))?, bytes).map_err(|e| io(e.to_string()))
}

fn read_bytes(src: &dyn ContentSource, file: &str) -> Result<Vec<u8>, FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    let path = VPath::new(file).map_err(|e| io(e.to_string()))?;
    src.read_max(&path, MAX_FILE_BYTES).map_err(|e| io(e.to_string()))
}

/// The paths the writer writes for `src`, without encoding anything.
pub fn file_paths(src: &LevelSource) -> Vec<String> {
    let mut out: Vec<String> = [LEVEL_FILE, MATERIAL_FILE, FRONT_FILE, BACK_FILE].map(String::from).into();
    if src.thumb.is_some() {
        out.push(THUMB_FILE.into());
    }
    if src.glow.is_some() {
        out.extend(LOOK_FILES.map(String::from));
    }
    out
}

pub fn write_level_files(sink: &mut dyn ContentSink, src: &LevelSource) -> Result<(), FormatError> {
    check(src)?;
    let file = LevelFile {
        name: src.name.clone(),
        notes: src.notes.clone(),
        size: src.size,
        art_scale: src.art_scale,
        theme: src.theme.clone(),
        objects: src.objects.clone(),
        look: src.look.clone(),
    };
    write_text(sink, LEVEL_FILE, &to_canonical(LEVEL_FILE, &file)?)?;
    write_bytes(sink, MATERIAL_FILE, &super::encode_material(src.size, &src.material))?;
    write_bytes(sink, FRONT_FILE, &image_io::encode_rgba(&src.front))?;
    write_bytes(sink, BACK_FILE, &image_io::encode_rgba(&src.back))?;
    if let Some(t) = &src.thumb {
        write_bytes(sink, THUMB_FILE, t)?;
    }
    if let Some(g) = &src.glow {
        let (art, world) = (g.art, g.world);
        for (file, size, values) in [
            (GLOW_FILE, art, &g.front),
            (GLOW_BACK_FILE, art, &g.back),
            (GROUPS_FILE, world, &g.groups),
            (GROUPS_BACK_FILE, world, &g.groups_back),
        ] {
            write_bytes(sink, file, &image_io::encode_grey(size.0, size.1, values))?;
        }
    }
    Ok(())
}

/// Reads an 8-bit grey look image, checking its header size first.
fn read_grey(src: &dyn ContentSource, file: &str, expected: (u32, u32)) -> Result<Vec<u8>, FormatError> {
    let bytes = read_bytes(src, file)?;
    let mismatch = |(w, h): (u32, u32)| {
        FormatError::invalid(
            file,
            "",
            format!("{file} is {w}x{h}, the level needs {}x{}", expected.0, expected.1),
        )
    };
    if let Some(size) = image_io::png_size(&bytes)
        && size != expected
    {
        return Err(mismatch(size));
    }
    let (w, h, values) = image_io::decode_grey_within(&bytes, LEVEL_ART)
        .map_err(|message| FormatError::Io { file: file.into(), message })?;
    if (w, h) != expected {
        return Err(mismatch((w, h)));
    }
    Ok(values)
}

/// The four `look/` images: all or none.
fn read_glow(
    src: &dyn ContentSource,
    art: (u32, u32),
    world: (u32, u32),
) -> Result<Option<GlowLayers>, FormatError> {
    let present = |f: &str| VPath::new(f).is_ok_and(|p| src.exists(&p));
    if !LOOK_FILES.iter().any(|f| present(f)) {
        return Ok(None);
    }
    if let Some(missing) = LOOK_FILES.iter().find(|f| !present(f)) {
        return Err(FormatError::invalid(missing, "", "missing: a level's glow needs all four look/ images"));
    }
    Ok(Some(GlowLayers {
        art,
        world,
        front: read_grey(src, GLOW_FILE, art)?,
        back: read_grey(src, GLOW_BACK_FILE, art)?,
        groups: read_grey(src, GROUPS_FILE, world)?,
        groups_back: read_grey(src, GROUPS_BACK_FILE, world)?,
    }))
}

/// Reads an art PNG, checking its header size against `level.ron`'s before
/// decoding.
fn read_art(
    src: &dyn ContentSource,
    file: &str,
    expected: (u32, u32),
) -> Result<olr_data::image::RgbaImage, FormatError> {
    let bytes = read_bytes(src, file)?;
    if let Some((w, h)) = image_io::png_size(&bytes)
        && (w, h) != expected
    {
        return Err(FormatError::invalid(
            file,
            "",
            format!("{file} is {w}x{h}, {LEVEL_FILE}'s size × art_scale is {}x{}", expected.0, expected.1),
        ));
    }
    image_io::decode_rgba_within(&bytes, LEVEL_ART)
        .map_err(|message| FormatError::Io { file: file.into(), message })
}

/// Reads and validates `level.ron` only; no image is touched.
pub fn read_level_header(src: &dyn ContentSource) -> Result<LevelHeader, FormatError> {
    let file = read_level_ron(src)?;
    check_header(file.size, file.art_scale, file.objects.len())?;
    Ok(LevelHeader {
        name: file.name,
        size: file.size,
        art_scale: file.art_scale,
        theme: file.theme,
        spawns: file.objects.iter().filter_map(LevelObject::spawn_point).collect(),
        objects: file.objects,
        look: file.look,
    })
}

fn read_level_ron(src: &dyn ContentSource) -> Result<LevelFile, FormatError> {
    from_canonical(LEVEL_FILE, &read_text(src, LEVEL_FILE)?)
}

/// The thumbnail, tolerantly: absent, above [`MAX_THUMB_BYTES`] or
/// `MAX_THUMB_SIDE` (by the PNG header) or not a decodable PNG all give
/// `None`. The bytes are passed through.
fn read_thumb(src: &dyn ContentSource) -> Option<Vec<u8>> {
    let path = VPath::new(THUMB_FILE).ok()?;
    if !src.exists(&path) {
        return None;
    }
    let bytes = src.read_max(&path, MAX_THUMB_BYTES).ok()?;
    // The header says the size: a large image is never decoded.
    check_thumb(&bytes).ok()?;
    image_io::decode_rgba_within(&bytes, IMAGES).ok()?;
    Some(bytes)
}

/// The level with its thumbnail read tolerantly (never an error).
pub fn read_level_files(src: &dyn ContentSource) -> Result<LevelSource, FormatError> {
    let mut level = read_level_files_without_thumb(src)?;
    level.thumb = read_thumb(src);
    Ok(level)
}

/// The level with `thumb` always `None`: the thumbnail is never read.
pub fn read_level_files_without_thumb(src: &dyn ContentSource) -> Result<LevelSource, FormatError> {
    let file = read_level_ron(src)?;
    // level.ron is validated before any image is read.
    let art = check_header(file.size, file.art_scale, file.objects.len())?;
    let bytes = read_bytes(src, MATERIAL_FILE)?;
    if let Some((w, h)) = image_io::png_size(&bytes)
        && (w, h) != file.size
    {
        return Err(FormatError::invalid(
            MATERIAL_FILE,
            "",
            format!("{MATERIAL_FILE} is {w}x{h}, {LEVEL_FILE}'s size is {}x{}", file.size.0, file.size.1),
        ));
    }
    let (w, h, indices) = image_io::decode_indexed_within(&bytes, LEVEL_ART)
        .map_err(|message| FormatError::Io { file: MATERIAL_FILE.into(), message })?;
    if (w, h) != file.size {
        return Err(FormatError::invalid(
            MATERIAL_FILE,
            "",
            format!("{MATERIAL_FILE} is {w}x{h}, {LEVEL_FILE}'s size is {}x{}", file.size.0, file.size.1),
        ));
    }
    let mut material = Vec::with_capacity(indices.len());
    for (i, &v) in indices.iter().enumerate() {
        let m = Material::from_index(v).ok_or_else(|| {
            let (x, y) = (i as u32 % w.max(1), i as u32 / w.max(1));
            FormatError::invalid(MATERIAL_FILE, "", format!("unknown material index {v} at ({x}, {y})"))
        })?;
        material.push(m);
    }
    let front = read_art(src, FRONT_FILE, art)?;
    let back = read_art(src, BACK_FILE, art)?;
    let glow = read_glow(src, art, file.size)?;
    let level = LevelSource {
        name: file.name,
        notes: file.notes,
        size: file.size,
        art_scale: file.art_scale,
        theme: file.theme,
        objects: file.objects,
        material,
        front,
        back,
        thumb: None,
        look: file.look,
        glow,
    };
    check(&level)?;
    Ok(level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levels::MAX_THUMB_SIDE;
    use olr_data::level::SpawnPoint;
    use olr_vfs::MemFs;

    #[test]
    fn files_round_trip_byte_stably() {
        let mut src = crate::levels::tests::tiny(2);
        src.thumb = Some(image_io::encode_rgba(&olr_data::image::RgbaImage::new(2, 2)));
        let mut a = MemFs::new();
        write_level_files(&mut a, &src).unwrap();
        let names: Vec<String> = a.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(names, ["back.png", "front.png", "level.ron", "material.png", "thumb.png"]);
        let back = read_level_files(&a).unwrap();
        assert_eq!(back, src);
        let mut b = MemFs::new();
        write_level_files(&mut b, &back).unwrap();
        assert_eq!(a, b);
    }

    fn with_thumb(bytes: Vec<u8>) -> MemFs {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &crate::levels::tests::tiny(1)).unwrap();
        fs.with("thumb.png", bytes)
    }

    #[test]
    fn a_thumbnail_that_is_too_big_or_undecodable_is_left_out() {
        let good = image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4));
        assert_eq!(read_level_files(&with_thumb(good.clone())).unwrap().thumb, Some(good));
        let big = vec![0u8; 2 << 20];
        assert_eq!(read_level_files(&with_thumb(big)).unwrap().thumb, None);
        // A header past the side bound is refused without decoding (the
        // body is not even valid PNG data).
        let mut wide = image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4));
        wide[16..20].copy_from_slice(&(MAX_THUMB_SIDE + 1).to_be_bytes());
        assert!(check_thumb(&wide).is_err());
        assert_eq!(read_level_files(&with_thumb(wide)).unwrap().thumb, None);
        // The writer refuses what the reader would leave out.
        let mut src = crate::levels::tests::tiny(1);
        src.thumb = Some(b"not a png".to_vec());
        assert!(write_level_files(&mut MemFs::new(), &src).is_err());
        assert_eq!(read_level_files(&with_thumb(b"not a png".to_vec())).unwrap().thumb, None);
    }

    #[test]
    fn the_read_without_thumb_ignores_the_thumbnail() {
        let good = image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4));
        assert_eq!(read_level_files_without_thumb(&with_thumb(good)).unwrap().thumb, None);
    }

    #[test]
    fn the_header_read_touches_no_image() {
        let fs = MemFs::new().with(
            "level.ron",
            b"(name: \"x\", size: (8, 6), art_scale: 2, theme: \"t\", objects: [Spawn(at: (1, 2))])\n"
                .to_vec(),
        );
        let h = read_level_header(&fs).unwrap();
        assert_eq!((h.name.as_str(), h.size, h.art_scale, h.theme.as_str()), ("x", (8, 6), 2, "t"));
        assert_eq!(h.spawns, [SpawnPoint { x: 1, y: 2 }]);
        // It validates level.ron all the same.
        let bad = MemFs::new().with("level.ron", b"(name: \"x\", size: (0, 6), theme: \"t\")\n".to_vec());
        assert!(read_level_header(&bad).unwrap_err().to_string().contains("size"));
    }

    #[test]
    fn a_bad_material_index_names_the_pixel() {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &crate::levels::tests::tiny(1)).unwrap();
        let bad = image_io::encode_indexed(3, 2, &[0, 1, 2, 7, 1, 0], &[0; 24]);
        let fs = fs.with("material.png", bad);
        let e = read_level_files(&fs).unwrap_err().to_string();
        assert!(e.contains("material.png") && e.contains("index 7 at (0, 1)"), "{e}");
    }

    #[test]
    fn level_ron_is_validated_and_sizes_compared_before_decoding() {
        // An absurd size in level.ron is refused with no image present.
        let fs = MemFs::new()
            .with("level.ron", b"(name: \"x\", size: (4096, 4096), art_scale: 8, theme: \"t\")\n".to_vec());
        let e = read_level_files(&fs).unwrap_err().to_string();
        assert!(e.contains("level.ron") && e.contains("art_scale 8"), "{e}");
        // A bomb front.png (huge header) is refused naming both files.
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &crate::levels::tests::tiny(1)).unwrap();
        let fs = fs.with("front.png", image_io::declared_png(8, 8, false));
        let e = read_level_files(&fs).unwrap_err().to_string();
        assert!(e.contains("front.png") && e.contains("level.ron") && e.contains("8x8"), "{e}");
    }

    #[test]
    fn level_ron_is_short() {
        let mut fs = MemFs::new();
        let mut src = crate::levels::tests::tiny(1);
        src.objects.clear();
        write_level_files(&mut fs, &src).unwrap();
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec())
            .unwrap();
        assert!(
            !text.contains("art_scale") && !text.contains("objects") && !text.contains("notes"),
            "{text}"
        );
    }
}
