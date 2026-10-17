//! A skin's files. Other paths (`pack.ron`, `mask.png`, ...) belong to
//! `olr-pack` and are ignored here.

use olr_data::image::RgbaImage;
use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::{IMAGE_FILE, LEFT_FILE, SKIN_FILE, SkinSource, TINT_FILE, check, check_layout};
use crate::FormatError;
use crate::image_io::{self, IMAGES};
use crate::ron_io::{MAX_FILE_BYTES, from_canonical, read_text, to_canonical, write_text};

/// Every field is written: the layout is the point of the file.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkinFile {
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    art_scale: u32,
    frame_size: (u32, u32),
    aim_frames: u32,
    walk_frames: u32,
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

pub fn write_skin_files(sink: &mut dyn ContentSink, src: &SkinSource) -> Result<(), FormatError> {
    check(src)?;
    let file = SkinFile {
        name: src.name.clone(),
        notes: src.notes.clone(),
        art_scale: src.art_scale,
        frame_size: src.frame_size,
        aim_frames: src.aim_frames,
        walk_frames: src.walk_frames,
    };
    write_text(sink, SKIN_FILE, &to_canonical(SKIN_FILE, &file)?)?;
    write_bytes(sink, IMAGE_FILE, &image_io::encode_rgba(&src.image))?;
    write_bytes(sink, TINT_FILE, &image_io::encode_grey(src.image.width(), src.image.height(), &src.tint))?;
    if let Some(l) = &src.left {
        write_bytes(sink, LEFT_FILE, &image_io::encode_rgba(l))?;
    }
    Ok(())
}

/// The PNG `file`'s bytes after its header size is compared with the
/// layout's, so a mismatched image is never decoded.
fn read_png(src: &dyn ContentSource, file: &str, (w, h): (u32, u32)) -> Result<Vec<u8>, FormatError> {
    let bytes = read_bytes(src, file)?;
    if let Some((pw, ph)) = image_io::png_size(&bytes)
        && (pw, ph) != (w, h)
    {
        return Err(FormatError::invalid(
            file,
            "",
            format!("{file} is {pw}x{ph}, the layout needs {w}x{h} (frame_size × frames)"),
        ));
    }
    Ok(bytes)
}

fn read_image(src: &dyn ContentSource, file: &str, size: (u32, u32)) -> Result<RgbaImage, FormatError> {
    image_io::decode_rgba_within(&read_png(src, file, size)?, IMAGES)
        .map_err(|message| FormatError::Io { file: file.into(), message })
}

pub fn read_skin_files(src: &dyn ContentSource) -> Result<SkinSource, FormatError> {
    let file: SkinFile = from_canonical(SKIN_FILE, &read_text(src, SKIN_FILE)?)?;
    // skin.ron is validated before any image is read.
    let size = check_layout(file.art_scale, file.frame_size, file.aim_frames, file.walk_frames)?;
    let image = read_image(src, IMAGE_FILE, size)?;
    let (tw, th, tint) = image_io::decode_grey_within(&read_png(src, TINT_FILE, size)?, IMAGES)
        .map_err(|message| FormatError::Io { file: TINT_FILE.into(), message })?;
    if (tw, th) != (image.width(), image.height()) {
        return Err(FormatError::invalid(
            TINT_FILE,
            "",
            format!("{TINT_FILE} is {tw}x{th}, {IMAGE_FILE} is {}x{}", image.width(), image.height()),
        ));
    }
    let left_path = VPath::new(LEFT_FILE)
        .map_err(|e| FormatError::Io { file: LEFT_FILE.into(), message: e.to_string() })?;
    let left = if src.exists(&left_path) { Some(read_image(src, LEFT_FILE, size)?) } else { None };
    let skin = SkinSource {
        name: file.name,
        notes: file.notes,
        art_scale: file.art_scale,
        frame_size: file.frame_size,
        aim_frames: file.aim_frames,
        walk_frames: file.walk_frames,
        image,
        tint,
        left,
    };
    check(&skin)?;
    Ok(skin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_vfs::MemFs;

    fn written() -> MemFs {
        let mut fs = MemFs::new();
        write_skin_files(&mut fs, &crate::skins::tests::sample()).unwrap();
        fs
    }

    #[test]
    fn files_round_trip_byte_stably() {
        let mut src = crate::skins::tests::sample();
        src.notes = "a note".into();
        src.left = Some(src.image.clone());
        let mut a = MemFs::new();
        write_skin_files(&mut a, &src).unwrap();
        let names: Vec<String> = a.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(names, ["left.png", "skin.png", "skin.ron", "tint.png"]);
        let back = read_skin_files(&a).unwrap();
        assert_eq!(back, src);
        let mut b = MemFs::new();
        write_skin_files(&mut b, &back).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn skin_ron_has_all_six_layout_fields() {
        let fs = written();
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "skin.ron").unwrap().1.to_vec())
            .unwrap();
        for f in ["name", "art_scale", "frame_size", "aim_frames", "walk_frames"] {
            assert!(text.contains(f), "{f}: {text}");
        }
        assert!(text.contains("art_scale: 2") && text.contains("(20, 18)"), "{text}");
        assert!(!text.contains("notes"), "{text}");
    }

    #[test]
    fn a_tint_of_another_size_names_the_file_and_the_layout() {
        let fs = written().with("tint.png", image_io::encode_grey(2, 2, &[0; 4]));
        let e = read_skin_files(&fs).unwrap_err().to_string();
        assert!(e.contains("tint.png") && e.contains("layout needs"), "{e}");
    }

    #[test]
    fn a_header_that_does_not_match_the_layout_is_refused_before_decoding() {
        let good = written();
        let png = good.read(&VPath::new("skin.png").unwrap()).unwrap();
        // Width 4096 in the IHDR: the body is not 4096 wide, and a decode
        // would fail differently (or allocate).
        let mut wide = png.clone();
        wide[16..20].copy_from_slice(&4096u32.to_be_bytes());
        let e = read_skin_files(&good.with("skin.png", wide)).unwrap_err().to_string();
        assert!(e.contains("skin.png") && e.contains("4096x") && e.contains("layout needs"), "{e}");
    }

    #[test]
    fn a_grey_with_alpha_tint_reads_its_grey() {
        let src = crate::skins::tests::sample();
        let (w, h) = (src.image.width(), src.image.height());
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::GrayscaleAlpha);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            let data: Vec<u8> = src.tint.iter().flat_map(|&t| [t, 77]).collect();
            wr.write_image_data(&data).unwrap();
        }
        let fs = written().with("tint.png", out);
        assert_eq!(read_skin_files(&fs).unwrap(), src);
    }
}
