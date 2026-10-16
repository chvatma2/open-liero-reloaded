//! A theme's files. Other paths (`pack.ron`, ...) belong to `olr-pack`
//! and are ignored here.

use olr_data::image::RgbaImage;
use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::{
    BACK_TILE_FILE, FRONT_TILE_FILE, GREEN_FILE, GREEN_MASK_FILE, GREEN_PALETTE, HOLE_PALETTE, MAX_HOLES,
    MAX_STAMP_SIDE, MAX_THEME_IMAGES, StampImage, THEME_FILE, ThemeSource, check, hole_file, hole_mask_file,
    image_file,
};
use crate::FormatError;
use crate::image_io::{self, IMAGES, ImageBound};
use crate::ron_io::{from_canonical, read_text, to_canonical, write_text};

/// The largest file read (a PNG), as `olr-pack` reads.
const MAX_FILE_BYTES: u64 = 64 << 20;

const STAMP_BOUND: ImageBound =
    ImageBound { max_side: MAX_STAMP_SIDE, max_pixels: (MAX_STAMP_SIDE * MAX_STAMP_SIDE) as u64 };

fn is_zero(v: &u32) -> bool {
    *v == 0
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    holes: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    stones: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    misc: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    green_ball: bool,
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

fn write_stamp(
    sink: &mut dyn ContentSink,
    file: &str,
    mask_file: &str,
    s: &StampImage,
    palette: &[u8],
) -> Result<(), FormatError> {
    // Canonical paint: transparent wherever the mask is not paint.
    let mut paint = RgbaImage::new(s.paint.width(), s.paint.height());
    for (i, &m) in s.mask.iter().enumerate() {
        if m == 2 {
            let (x, y) = (i as u32 % paint.width(), i as u32 / paint.width());
            paint.set(x, y, s.paint.get(x, y));
        }
    }
    write_bytes(sink, file, &image_io::encode_rgba(&paint))?;
    write_bytes(sink, mask_file, &image_io::encode_indexed(paint.width(), paint.height(), &s.mask, palette))
}

pub fn write_theme_files(sink: &mut dyn ContentSink, src: &ThemeSource) -> Result<(), FormatError> {
    check(src)?;
    let file = ThemeFile {
        name: src.name.clone(),
        notes: src.notes.clone(),
        holes: src.holes.len() as u32,
        stones: src.stones.len() as u32,
        misc: src.misc.len() as u32,
        green_ball: src.green_ball.is_some(),
    };
    write_text(sink, THEME_FILE, &to_canonical(THEME_FILE, &file)?)?;
    for (i, h) in src.holes.iter().enumerate() {
        write_stamp(sink, &hole_file(i), &hole_mask_file(i), h, &HOLE_PALETTE)?;
    }
    if let Some(g) = &src.green_ball {
        write_stamp(sink, GREEN_FILE, GREEN_MASK_FILE, g, &GREEN_PALETTE)?;
    }
    for (dir, imgs) in [("stones", &src.stones), ("misc", &src.misc)] {
        for (i, img) in imgs.iter().enumerate() {
            write_bytes(sink, &image_file(dir, i), &image_io::encode_rgba(img))?;
        }
    }
    write_bytes(sink, FRONT_TILE_FILE, &image_io::encode_rgba(&src.front_tile))?;
    write_bytes(sink, BACK_TILE_FILE, &image_io::encode_rgba(&src.back_tile))
}

fn read_image(src: &dyn ContentSource, file: &str, bound: ImageBound) -> Result<RgbaImage, FormatError> {
    let bytes = read_bytes(src, file)?;
    image_io::decode_rgba_within(&bytes, bound)
        .map_err(|message| FormatError::Io { file: file.into(), message })
}

fn read_stamp(src: &dyn ContentSource, file: &str, mask_file: &str) -> Result<StampImage, FormatError> {
    let paint = read_image(src, file, STAMP_BOUND)?;
    let bytes = read_bytes(src, mask_file)?;
    let (w, h, mask) = image_io::decode_indexed_within(&bytes, STAMP_BOUND)
        .map_err(|message| FormatError::Io { file: mask_file.into(), message })?;
    if (w, h) != (paint.width(), paint.height()) {
        return Err(FormatError::invalid(
            mask_file,
            "",
            format!("{mask_file} is {w}x{h}, {file} is {}x{}", paint.width(), paint.height()),
        ));
    }
    if let Some(i) = mask.iter().position(|&m| m > 2) {
        let (x, y) = (i as u32 % w, i as u32 / w);
        return Err(FormatError::invalid(
            mask_file,
            "",
            format!("mask index {} at ({x}, {y}), not 0, 1 or 2", mask[i]),
        ));
    }
    Ok(StampImage { mask, paint })
}

pub fn read_theme_files(src: &dyn ContentSource) -> Result<ThemeSource, FormatError> {
    let file: ThemeFile = from_canonical(THEME_FILE, &read_text(src, THEME_FILE)?)?;
    // The counts are checked before any image is read.
    if file.holes as usize > MAX_HOLES {
        return Err(FormatError::invalid(
            THEME_FILE,
            "holes",
            format!("{} holes, above MAX_HOLES ({MAX_HOLES})", file.holes),
        ));
    }
    for (name, n) in [("stones", file.stones), ("misc", file.misc)] {
        if n as usize > MAX_THEME_IMAGES {
            return Err(FormatError::invalid(
                THEME_FILE,
                name,
                format!("{n} images, above MAX_THEME_IMAGES ({MAX_THEME_IMAGES})"),
            ));
        }
    }
    let holes = (0..file.holes as usize)
        .map(|i| read_stamp(src, &hole_file(i), &hole_mask_file(i)))
        .collect::<Result<Vec<_>, _>>()?;
    let green_ball = if file.green_ball { Some(read_stamp(src, GREEN_FILE, GREEN_MASK_FILE)?) } else { None };
    let list = |dir: &str, n: u32| {
        (0..n as usize).map(|i| read_image(src, &image_file(dir, i), IMAGES)).collect::<Result<Vec<_>, _>>()
    };
    let theme = ThemeSource {
        name: file.name,
        notes: file.notes,
        holes,
        green_ball,
        stones: list("stones", file.stones)?,
        misc: list("misc", file.misc)?,
        front_tile: read_image(src, FRONT_TILE_FILE, IMAGES)?,
        back_tile: read_image(src, BACK_TILE_FILE, IMAGES)?,
    };
    check(&theme)?;
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_vfs::MemFs;

    fn written() -> MemFs {
        let mut fs = MemFs::new();
        write_theme_files(&mut fs, &crate::themes::tests::sample()).unwrap();
        fs
    }

    #[test]
    fn files_round_trip_byte_stably() {
        let src = crate::themes::tests::sample();
        let a = written();
        let back = read_theme_files(&a).unwrap();
        assert_eq!(back, src);
        let mut b = MemFs::new();
        write_theme_files(&mut b, &back).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn the_file_layout_is_the_documented_one() {
        let names: Vec<String> = written().files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(
            names,
            [
                "back_tile.png",
                "front_tile.png",
                "green_ball.mask.png",
                "green_ball.png",
                "holes/1.mask.png",
                "holes/1.png",
                "holes/2.mask.png",
                "holes/2.png",
                "stones/1.png",
                "theme.ron",
            ]
        );
    }

    #[test]
    fn a_mask_of_another_size_names_both_files() {
        let fs = written().with("holes/2.mask.png", image_io::encode_indexed(2, 2, &[0; 4], &HOLE_PALETTE));
        let e = read_theme_files(&fs).unwrap_err().to_string();
        assert!(e.contains("holes/2.mask.png") && e.contains("holes/2.png"), "{e}");
    }

    #[test]
    fn a_mask_index_of_three_names_the_pixel() {
        let mut m = vec![0u8; 9];
        m[7] = 3;
        let fs = written().with("holes/2.mask.png", image_io::encode_indexed(3, 3, &m, &[0; 12]));
        let e = read_theme_files(&fs).unwrap_err().to_string();
        assert!(e.contains("holes/2.mask.png") && e.contains("index 3 at (1, 2)"), "{e}");
    }

    #[test]
    fn a_two_bit_mask_reads_like_an_eight_bit_one() {
        // Rows of 3 pixels at 2 bits, each row padded to a byte: 6 bits used.
        let m = [0u8, 1, 0, 1, 1, 1, 0, 2, 2];
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 3, 3);
            enc.set_color(png::ColorType::Indexed);
            enc.set_depth(png::BitDepth::Two);
            enc.set_palette(HOLE_PALETTE.to_vec());
            let mut w = enc.write_header().unwrap();
            let rows: Vec<u8> = m.chunks(3).flat_map(|r| [(r[0] << 6) | (r[1] << 4) | (r[2] << 2)]).collect();
            w.write_image_data(&rows).unwrap();
        }
        let fs = written().with("holes/1.mask.png", out);
        assert_eq!(read_theme_files(&fs).unwrap(), crate::themes::tests::sample());
    }

    #[test]
    fn a_huge_declared_stamp_is_refused_before_decoding() {
        let fs = written().with("holes/1.png", image_io::declared_png(10_000, 10_000, false));
        let e = read_theme_files(&fs).unwrap_err().to_string();
        assert!(e.contains("holes/1.png"), "{e}");
        let fs = written().with("holes/2.mask.png", image_io::declared_png(10_000, 10_000, true));
        let e = read_theme_files(&fs).unwrap_err().to_string();
        assert!(e.contains("holes/2.mask.png"), "{e}");
    }

    #[test]
    fn a_thousand_holes_are_refused_before_any_image() {
        let fs = MemFs::new().with("theme.ron", b"(name: \"x\", holes: 1000)\n".to_vec());
        let e = read_theme_files(&fs).unwrap_err().to_string();
        assert!(e.contains("theme.ron") && e.contains("MAX_HOLES"), "{e}");
    }

    #[test]
    fn the_most_holes_write_and_read() {
        let mut src = crate::themes::tests::sample();
        src.holes = vec![src.holes[0].clone(); MAX_HOLES];
        let mut fs = MemFs::new();
        write_theme_files(&mut fs, &src).unwrap();
        assert!(fs.files().any(|(p, _)| p.as_str() == "holes/16.mask.png"));
        assert_eq!(read_theme_files(&fs).unwrap(), src);
    }

    #[test]
    fn theme_ron_is_short() {
        let mut src = crate::themes::tests::sample();
        src.notes.clear();
        src.stones.clear();
        src.green_ball = None;
        let mut fs = MemFs::new();
        write_theme_files(&mut fs, &src).unwrap();
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "theme.ron").unwrap().1.to_vec())
            .unwrap();
        assert!(
            !text.contains("stones") && !text.contains("green_ball") && !text.contains("notes"),
            "{text}"
        );
    }
}
