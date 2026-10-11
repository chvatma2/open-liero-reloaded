//! Windows bitmaps in OLX's image files. OLX loads every image with
//! SDL_image's `IMG_Load`, which looks at the bytes, not the name, so a
//! `.png` that is really a BMP worked there: OLX 0.58's own release ships
//! two skins like that (`skins/Teletubbie.png`, `skins/capworm.png`).
//! Uncompressed 1, 4, 8, 24 and 32-bit bitmaps are read; compressed ones
//! (RLE, bit fields) are refused with a message.
//!
//! OLX: src/client/GfxPrimitives.cpp:2915 (`IMG_Load`).

use rl_data::image::RgbaImage;
use rl_pack::image_io::ImageBound;

/// Whether `bytes` starts like a Windows bitmap.
pub fn is_bmp(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BM")
}

/// Decodes a Windows bitmap to RGBA8 (opaque), within `bound`.
pub fn decode(bytes: &[u8], bound: ImageBound) -> Result<RgbaImage, String> {
    let u16_at = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let short = || "BMP header cut short".to_string();
    if !is_bmp(bytes) {
        return Err("not a BMP".into());
    }
    let data_at = u32_at(10).ok_or_else(short)? as usize;
    let header = u32_at(14).ok_or_else(short)?;
    if header < 40 {
        return Err(format!("BMP header of {header} bytes (OS/2 bitmaps are not read)"));
    }
    let width = u32_at(18).ok_or_else(short)? as i32;
    let height = u32_at(22).ok_or_else(short)? as i32;
    let bpp = u16_at(28).ok_or_else(short)?;
    let compression = u32_at(30).ok_or_else(short)?;
    if compression != 0 {
        return Err(format!("compressed BMP (method {compression}) is not read"));
    }
    if width <= 0 || height == 0 {
        return Err(format!("BMP of {width}x{height} pixels"));
    }
    let (w, h) = (width as u32, height.unsigned_abs());
    if w.max(h) > bound.max_side {
        return Err(format!("{w}x{h} image: a side above {}", bound.max_side));
    }
    if u64::from(w) * u64::from(h) > bound.max_pixels {
        return Err(format!("{w}x{h} image: above {} pixels", bound.max_pixels));
    }
    let palette: Vec<[u8; 3]> = if bpp <= 8 {
        let declared = u32_at(46).ok_or_else(short)? as usize;
        let n = if declared == 0 { 1usize << bpp } else { declared.min(256) };
        let at = 14 + header as usize;
        (0..n)
            .map(|i| bytes.get(at + i * 4..at + i * 4 + 3).map(|c| [c[2], c[1], c[0]]))
            .collect::<Option<_>>()
            .ok_or("BMP palette cut short")?
    } else {
        Vec::new()
    };
    if !matches!(bpp, 1 | 4 | 8 | 24 | 32) {
        return Err(format!("{bpp}-bit BMP is not read"));
    }
    let stride = (w as usize * bpp as usize).div_ceil(32) * 4;
    let rows = bytes.get(data_at..).ok_or("BMP pixel data missing")?;
    if rows.len() < stride * h as usize {
        return Err("BMP pixel data cut short".into());
    }
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        // Bottom-up unless the height is negative.
        let src_row = if height > 0 { h as usize - 1 - y } else { y };
        let row = &rows[src_row * stride..(src_row + 1) * stride];
        for x in 0..w as usize {
            let rgb = match bpp {
                24 | 32 => {
                    let i = x * (bpp as usize / 8);
                    [row[i + 2], row[i + 1], row[i]]
                }
                _ => {
                    let bit = x * bpp as usize;
                    let byte = row[bit / 8];
                    let index = (byte >> (8 - bpp as usize - bit % 8)) & ((1u16 << bpp) - 1) as u8;
                    *palette.get(index as usize).ok_or("BMP index past its palette")?
                }
            };
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| "decoded size mismatch".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_pack::image_io::IMAGES;

    /// A BMP file of `w`×`h` with `bpp` bits, `palette` (RGB) and the
    /// rows as stored (each padded by the caller).
    fn bmp(w: i32, h: i32, bpp: u16, palette: &[[u8; 3]], rows: &[u8]) -> Vec<u8> {
        let data_at = 14 + 40 + palette.len() as u32 * 4;
        let mut b = Vec::new();
        b.extend_from_slice(b"BM");
        b.extend_from_slice(&(data_at + rows.len() as u32).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&data_at.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&bpp.to_le_bytes());
        b.extend_from_slice(&[0; 4]); // BI_RGB
        b.extend_from_slice(&(rows.len() as u32).to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&(palette.len() as u32).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        for c in palette {
            b.extend_from_slice(&[c[2], c[1], c[0], 0]);
        }
        b.extend_from_slice(rows);
        b
    }

    fn pixels(img: &RgbaImage) -> Vec<[u8; 4]> {
        img.as_raw().chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect()
    }

    #[test]
    fn reads_24_bit_bottom_up_and_top_down() {
        // 2×2: stored bottom row first, BGR, each row padded to 8 bytes.
        let rows = [
            0, 0, 255, 0, 255, 0, 0, 0, // bottom: red, green
            255, 0, 0, 255, 255, 255, 0, 0, // top: blue, white
        ];
        let img = decode(&bmp(2, 2, 24, &[], &rows), IMAGES).unwrap();
        let want = [[0, 0, 255, 255], [255, 255, 255, 255], [255, 0, 0, 255], [0, 255, 0, 255]];
        assert_eq!(pixels(&img), want);
        let img = decode(&bmp(2, -2, 24, &[], &rows), IMAGES).unwrap();
        assert_eq!(pixels(&img)[0], [255, 0, 0, 255], "a negative height is top-down");
    }

    #[test]
    fn reads_palette_bitmaps() {
        let pal = [[0, 0, 0], [255, 0, 255], [10, 20, 30]];
        // 8-bit, 3×1, padded to 4.
        let img = decode(&bmp(3, 1, 8, &pal, &[2, 1, 0, 0]), IMAGES).unwrap();
        assert_eq!(pixels(&img), [[10, 20, 30, 255], [255, 0, 255, 255], [0, 0, 0, 255]]);
        // 4-bit and 1-bit: high bits first.
        let img = decode(&bmp(2, 1, 4, &pal, &[0x21, 0, 0, 0]), IMAGES).unwrap();
        assert_eq!(pixels(&img), [[10, 20, 30, 255], [255, 0, 255, 255]]);
        let img = decode(&bmp(3, 1, 1, &pal[..2], &[0b0100_0000, 0, 0, 0]), IMAGES).unwrap();
        assert_eq!(pixels(&img), [[0, 0, 0, 255], [255, 0, 255, 255], [0, 0, 0, 255]]);
    }

    /// Untrusted files: every broken one is an error, never a panic or a
    /// huge allocation.
    #[test]
    fn refuses_broken_bitmaps() {
        let good = bmp(2, 2, 24, &[], &[0; 16]);
        for cut in 0..good.len() {
            assert!(decode(&good[..cut], IMAGES).is_err(), "cut at {cut}");
        }
        let mut huge = good.clone();
        huge[18..22].copy_from_slice(&100_000i32.to_le_bytes());
        assert!(decode(&huge, IMAGES).unwrap_err().contains("above"));
        let mut rle = good.clone();
        rle[30] = 1;
        assert!(decode(&rle, IMAGES).unwrap_err().contains("compressed"));
        let past = bmp(1, 1, 8, &[[1, 2, 3]], &[5, 0, 0, 0]);
        assert!(decode(&past, IMAGES).unwrap_err().contains("palette"));
        assert!(decode(&bmp(0, 1, 24, &[], &[]), IMAGES).is_err());
        assert!(decode(&bmp(1, 1, 16, &[], &[0; 4]), IMAGES).is_err());
    }
}
