//! A skin's files. Other paths (`pack.ron`, ...) belong to
//! `olr-pack` and are ignored here.

use olr_data::image::RgbaImage;
use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use olr_data::skin::SkinLight;

use super::{
    GLOW_FILE, IMAGE_FILE, LEFT_FILE, SKIN_FILE, SkinSource, TINT_FILE, check, check_layout, check_light,
};
use crate::FormatError;
use crate::image_io::{self, IMAGES};
use crate::ron_io::{MAX_FILE_BYTES, from_canonical, read_text, to_canonical, write_text};

/// `skin.ron`: the skin's name and the layout of `skin.png`. Every layout
/// field is written: the layout is the point of the file. Two layouts
/// compile today, both 7 aim columns by 3 walk rows, with a `tint.png` of 0
/// and 255 only: OLX's (art scale 2, 20 × 18 frames) and double resolution
/// (art scale 4, 40 × 36 frames); the worm is 10 × 9 world pixels either
/// way. `glow.png` (optional, the size of `skin.png`, greyscale: 0 no glow
/// to 255 full, drawn in quarters: 32 and above glows) makes pixels show
/// bright whatever the darkness, still in the player's tint; glow on a
/// transparent pixel is dropped.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkinFile {
    /// Shown in menus.
    name: String,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    /// Art pixels per world pixel, 1 to `MAX_ART_SCALE` (8); OLX's skins are 2.
    art_scale: u32,
    /// One frame's width and height in art pixels, each 1 to
    /// `MAX_FRAME_SIDE` (256); OLX's are 20 × 18.
    frame_size: (u32, u32),
    /// Columns of `skin.png`, one per aim angle, 1 to `MAX_FRAMES` (64); OLX's skins have 7.
    aim_frames: u32,
    /// Rows of `skin.png`, one per walk frame, 1 to `MAX_FRAMES` (64); OLX's skins have 3.
    walk_frames: u32,
    /// The glowing pixels' light on the worm's surroundings, in a lit level
    /// only; none by default (`glow.png` alone only shows bright).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    light: Option<SkinLightFile>,
}

/// A skin's light (issue #12). Its colour is the frame's glowing pixels'
/// as drawn (the player's tint included), weighted by their glow; its
/// strength is `intensity` scaled by how much of the frame glows (full at
/// 3 world pixels of full glow, such as 12 art pixels at art scale 2 or 48
/// at 4). One light per frame drawn, a disc at the worm that walls stop.
/// The engine caps keep a skin from flooding a level: values above them
/// are refused. Skin light can be turned off engine-wide
/// (`olr_look::engine::SKIN_LIGHT`).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkinLightFile {
    /// Percent, 1 to `MAX_SKIN_LIGHT_INTENSITY` (200).
    intensity: u16,
    /// World pixels the light fades out over, 1 to `MAX_SKIN_LIGHT_REACH` (48).
    reach: u16,
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
pub fn file_paths(src: &SkinSource) -> Vec<String> {
    let mut out: Vec<String> = [SKIN_FILE, IMAGE_FILE, TINT_FILE].map(String::from).into();
    if src.glow.is_some() {
        out.push(GLOW_FILE.into());
    }
    if src.left.is_some() {
        out.push(LEFT_FILE.into());
    }
    out
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
        light: src.light.map(|l| SkinLightFile { intensity: l.intensity, reach: l.reach }),
    };
    write_text(sink, SKIN_FILE, &to_canonical(SKIN_FILE, &file)?)?;
    let (w, h) = (src.image.width(), src.image.height());
    write_bytes(sink, IMAGE_FILE, &image_io::encode_rgba(&src.image))?;
    write_bytes(sink, TINT_FILE, &image_io::encode_grey(w, h, &src.tint))?;
    if let Some(g) = &src.glow {
        write_bytes(sink, GLOW_FILE, &image_io::encode_grey(w, h, g))?;
    }
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

/// Greyscale `file` of the layout's `size` (`tint.png`, `glow.png`).
fn read_grey(src: &dyn ContentSource, file: &str, size: (u32, u32)) -> Result<Vec<u8>, FormatError> {
    let (w, h, v) = image_io::decode_grey_within(&read_png(src, file, size)?, IMAGES)
        .map_err(|message| FormatError::Io { file: file.into(), message })?;
    if (w, h) != size {
        return Err(FormatError::invalid(
            file,
            "",
            format!("{file} is {w}x{h}, {IMAGE_FILE} is {}x{}", size.0, size.1),
        ));
    }
    Ok(v)
}

fn exists(src: &dyn ContentSource, file: &str) -> Result<bool, FormatError> {
    let path = VPath::new(file).map_err(|e| FormatError::Io { file: file.into(), message: e.to_string() })?;
    Ok(src.exists(&path))
}

pub fn read_skin_files(src: &dyn ContentSource) -> Result<SkinSource, FormatError> {
    let file: SkinFile = from_canonical(SKIN_FILE, &read_text(src, SKIN_FILE)?)?;
    // skin.ron is validated before any image is read.
    let size = check_layout(file.art_scale, file.frame_size, file.aim_frames, file.walk_frames)?;
    let light = file.light.map(|l| SkinLight { intensity: l.intensity, reach: l.reach });
    check_light(light)?;
    let image = read_image(src, IMAGE_FILE, size)?;
    let tint = read_grey(src, TINT_FILE, size)?;
    let glow = if exists(src, GLOW_FILE)? { Some(read_grey(src, GLOW_FILE, size)?) } else { None };
    let left = if exists(src, LEFT_FILE)? { Some(read_image(src, LEFT_FILE, size)?) } else { None };
    let skin = SkinSource {
        name: file.name,
        notes: file.notes,
        art_scale: file.art_scale,
        frame_size: file.frame_size,
        aim_frames: file.aim_frames,
        walk_frames: file.walk_frames,
        image,
        tint,
        glow,
        light,
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

    /// Issue #12: a double skin's glow.png and light round-trip.
    #[test]
    fn a_double_skin_round_trips_with_its_glow_and_light() {
        let src = crate::skins::tests::double_sample();
        let mut a = MemFs::new();
        write_skin_files(&mut a, &src).unwrap();
        let names: Vec<String> = a.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(names, ["glow.png", "skin.png", "skin.ron", "tint.png"]);
        assert_eq!(file_paths(&src), ["skin.ron", "skin.png", "tint.png", "glow.png"]);
        let text = String::from_utf8(a.read(&VPath::new("skin.ron").unwrap()).unwrap()).unwrap();
        assert!(text.contains("art_scale: 4") && text.contains("(40, 36)"), "{text}");
        assert!(
            text.contains("light: (") && text.contains("intensity: 150") && text.contains("reach: 24"),
            "{text}"
        );
        let back = read_skin_files(&a).unwrap();
        assert_eq!(back, src);
        let mut b = MemFs::new();
        write_skin_files(&mut b, &back).unwrap();
        assert_eq!(a, b);
    }

    /// Bad glow masks and lights are refused with a message, never a panic.
    #[test]
    fn bad_glow_and_light_are_refused() {
        let src = crate::skins::tests::double_sample();
        let mut good = MemFs::new();
        write_skin_files(&mut good, &src).unwrap();
        // A mask of the wrong size.
        let e = read_skin_files(&good.clone().with("glow.png", image_io::encode_grey(2, 2, &[0; 4])))
            .unwrap_err()
            .to_string();
        assert!(e.contains("glow.png") && e.contains("layout needs"), "{e}");
        // A truncated PNG.
        let png = good.read(&VPath::new("glow.png").unwrap()).unwrap();
        for cut in [0, 7, 30, png.len() / 2] {
            let e =
                read_skin_files(&good.clone().with("glow.png", png[..cut].to_vec())).unwrap_err().to_string();
            assert!(e.contains("glow.png"), "{cut}: {e}");
        }
        // Values over the caps, and an unknown field.
        let ron = String::from_utf8(good.read(&VPath::new("skin.ron").unwrap()).unwrap()).unwrap();
        for (from, to, field) in [
            ("reach: 24", "reach: 49", "light.reach"),
            ("intensity: 150", "intensity: 1000", "light.intensity"),
        ] {
            let text = ron.replace(from, to);
            let e =
                read_skin_files(&good.clone().with("skin.ron", text.into_bytes())).unwrap_err().to_string();
            assert!(e.contains("skin.ron") && e.contains(field), "{e}");
        }
        let text = ron.replace("reach: 24", "reach: 24,\n        colour: 3");
        assert!(read_skin_files(&good.clone().with("skin.ron", text.into_bytes())).is_err());
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
        assert!(!text.contains("notes") && !text.contains("light"), "{text}");
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
