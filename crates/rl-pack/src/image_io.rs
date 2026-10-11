//! PNG encoding and decoding for packs.

use rl_data::image::RgbaImage;

/// Encodes an RGBA8 PNG.
pub fn encode_rgba(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width(), img.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("in-memory PNG header");
        w.write_image_data(img.as_raw()).expect("in-memory PNG data");
    }
    out
}

/// The width and height a PNG's header (`IHDR`) gives, if `bytes` starts
/// like a PNG: read without decoding, so a caller can refuse a huge image.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Some((be(16), be(20)))
}

/// Decodes any PNG (palette, gray, RGB, RGBA, 1–16 bit) to RGBA8.
pub fn decode_rgba(bytes: &[u8]) -> Result<RgbaImage, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("palette was not expanded".into()),
    };
    RgbaImage::from_raw(info.width, info.height, rgba).ok_or_else(|| "decoded size mismatch".into())
}

/// Encodes an 8-bit palette PNG. `palette` holds RGB triples; every index must be
/// smaller than `palette.len() / 3`.
pub fn encode_indexed(width: u32, height: u32, indices: &[u8], palette: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(palette.to_vec());
        let mut w = enc.write_header().expect("in-memory PNG header");
        w.write_image_data(indices).expect("in-memory PNG data");
    }
    out
}

/// Decodes an 8-bit palette PNG to `(width, height, indices)`, without expanding
/// the palette. Any other PNG kind is an error, because the indices are the data.
pub fn decode_indexed(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::IDENTITY);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.color_type != png::ColorType::Indexed || info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "expected an 8-bit palette PNG, found {:?} at {:?} bits",
            info.color_type, info.bit_depth
        ));
    }
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::image::Rgba;

    #[test]
    fn png_size_reads_the_header() {
        let png = encode_rgba(&RgbaImage::new(7, 3));
        assert_eq!(png_size(&png), Some((7, 3)));
        assert_eq!(png_size(b"GIF89a"), None);
        assert_eq!(png_size(&png[..20]), None);
    }

    #[test]
    fn rgba_roundtrip() {
        let mut img = RgbaImage::new(3, 2);
        img.set(0, 0, Rgba::rgb(1, 2, 3));
        img.set(2, 1, Rgba { r: 4, g: 5, b: 6, a: 7 });
        assert_eq!(decode_rgba(&encode_rgba(&img)).unwrap(), img);
    }

    #[test]
    fn rgba_encoding_is_deterministic() {
        let img = RgbaImage::new(8, 8);
        assert_eq!(encode_rgba(&img), encode_rgba(&img));
    }

    #[test]
    fn indexed_roundtrip_and_rgba_expansion() {
        let png = encode_indexed(2, 1, &[0, 1], &[10, 20, 30, 40, 50, 60]);
        assert_eq!(decode_indexed(&png).unwrap(), (2, 1, vec![0, 1]));
        let rgba = decode_rgba(&png).unwrap();
        assert_eq!(rgba.get(1, 0), Rgba::rgb(40, 50, 60));
    }

    #[test]
    fn indexed_rejects_rgba_png() {
        let png = encode_rgba(&RgbaImage::new(1, 1));
        assert!(decode_indexed(&png).unwrap_err().contains("palette"));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_rgba(b"not a png").is_err());
        assert!(decode_indexed(&[0x89, b'P', b'N', b'G']).is_err());
    }
}
