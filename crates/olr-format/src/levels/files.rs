//! A level's files. Other paths (`pack.ron`, ...) belong to `olr-pack`
//! and are ignored here.

use olr_data::level::Material;
use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::{
    BACK_FILE, FRONT_FILE, LEVEL_FILE, LevelObject, LevelSource, MATERIAL_FILE, MATERIAL_PALETTE, THUMB_FILE,
    check,
};
use crate::FormatError;
use crate::image_io::{self, LEVEL_ART};
use crate::ron_io::{from_canonical, read_text, to_canonical, write_text};

/// The largest file read (a PNG or the thumbnail), as `olr-pack` reads.
const MAX_FILE_BYTES: u64 = 64 << 20;

fn default_scale() -> u32 {
    1
}

fn is_one(v: &u32) -> bool {
    *v == 1
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelFile {
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    size: (u32, u32),
    #[serde(default = "default_scale", skip_serializing_if = "is_one")]
    art_scale: u32,
    theme: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    objects: Vec<LevelObject>,
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

pub fn write_level_files(sink: &mut dyn ContentSink, src: &LevelSource) -> Result<(), FormatError> {
    check(src)?;
    let file = LevelFile {
        name: src.name.clone(),
        notes: src.notes.clone(),
        size: src.size,
        art_scale: src.art_scale,
        theme: src.theme.clone(),
        objects: src.objects.clone(),
    };
    write_text(sink, LEVEL_FILE, &to_canonical(LEVEL_FILE, &file)?)?;
    let indices: Vec<u8> = src.material.iter().map(|m| *m as u8).collect();
    write_bytes(
        sink,
        MATERIAL_FILE,
        &image_io::encode_indexed(src.size.0, src.size.1, &indices, &MATERIAL_PALETTE),
    )?;
    write_bytes(sink, FRONT_FILE, &image_io::encode_rgba(&src.front))?;
    write_bytes(sink, BACK_FILE, &image_io::encode_rgba(&src.back))?;
    if let Some(t) = &src.thumb {
        write_bytes(sink, THUMB_FILE, t)?;
    }
    Ok(())
}

fn read_art(src: &dyn ContentSource, file: &str) -> Result<olr_data::image::RgbaImage, FormatError> {
    let bytes = read_bytes(src, file)?;
    image_io::decode_rgba_within(&bytes, LEVEL_ART)
        .map_err(|message| FormatError::Io { file: file.into(), message })
}

pub fn read_level_files(src: &dyn ContentSource) -> Result<LevelSource, FormatError> {
    let file: LevelFile = from_canonical(LEVEL_FILE, &read_text(src, LEVEL_FILE)?)?;
    let bytes = read_bytes(src, MATERIAL_FILE)?;
    let (w, h, indices) = image_io::decode_indexed_within(&bytes, LEVEL_ART)
        .map_err(|message| FormatError::Io { file: MATERIAL_FILE.into(), message })?;
    if (w, h) != file.size {
        return Err(FormatError::invalid(
            MATERIAL_FILE,
            "",
            format!("{MATERIAL_FILE} is {w}x{h}, level.ron's size is {}x{}", file.size.0, file.size.1),
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
    let front = read_art(src, FRONT_FILE)?;
    let back = read_art(src, BACK_FILE)?;
    let thumb_path = VPath::new(THUMB_FILE)
        .map_err(|e| FormatError::Io { file: THUMB_FILE.into(), message: e.to_string() })?;
    let thumb = if src.exists(&thumb_path) { Some(read_bytes(src, THUMB_FILE)?) } else { None };
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
        thumb,
    };
    check(&level)?;
    Ok(level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_vfs::MemFs;

    #[test]
    fn files_round_trip_byte_stably() {
        let mut src = crate::levels::tests::tiny(2);
        src.thumb = Some(b"not even a png".to_vec());
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
