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

/// How large an image a decoder accepts, from its header, before it
/// allocates anything: untrusted packs and OLX files must not make a
/// small file allocate gigabytes (a 64 MiB PNG of zeros can declare 17 GB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageBound {
    /// The widest or tallest side.
    pub max_side: u32,
    /// The most pixels (4 bytes each decoded).
    pub max_pixels: u64,
}

/// The widest or tallest image [`decode_rgba`] takes: above OLX's widest
/// legitimate strip (`data/gfx/out_font.png`, 11563 × 19).
pub const MAX_IMAGE_SIDE: u32 = 16384;

/// Sprites, skins, theme images, sounds' art: at most 4096 × 4096 pixels'
/// worth (64 MiB decoded), in any shape up to [`MAX_IMAGE_SIDE`]. OLX's
/// largest sprite is far below (mod and core gfx are under 1 Mpx).
pub const IMAGES: ImageBound = ImageBound { max_side: MAX_IMAGE_SIDE, max_pixels: 4096 * 4096 };

/// A level's art and material grid (`front.png`, `back.png`,
/// `material.png`, an `.lxl`'s hi-res chunk): OLX's largest level side
/// (4096, 12-bit positions) at art scale 8, and at most 64 Mi pixels
/// (256 MiB decoded, `rl-content`'s `MAX_COLOR_MAP_BYTES`). OLX's largest
/// level art in scope is 1350 × 550.
pub const LEVEL_ART: ImageBound = ImageBound { max_side: 4096 * 8, max_pixels: 64 << 20 };

/// A decoder for `bytes` whose header is within `bound`: refused from
/// the header, before the image is allocated.
fn reader(
    bytes: &[u8],
    bound: ImageBound,
    t: png::Transformations,
) -> Result<png::Reader<std::io::Cursor<&[u8]>>, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(t);
    let reader = dec.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w.max(h) > bound.max_side {
        return Err(format!("{w}x{h} image: a side above {}", bound.max_side));
    }
    if u64::from(w) * u64::from(h) > bound.max_pixels {
        return Err(format!("{w}x{h} image: above {} pixels", bound.max_pixels));
    }
    Ok(reader)
}

/// Decodes any PNG (palette, gray, RGB, RGBA, 1–16 bit) to RGBA8, within
/// [`IMAGES`].
pub fn decode_rgba(bytes: &[u8]) -> Result<RgbaImage, String> {
    decode_rgba_within(bytes, IMAGES)
}

/// [`decode_rgba`] within `bound` (a level's art: [`LEVEL_ART`]). The
/// image is decoded into one buffer of its RGBA size, expanded in place,
/// so the memory used is 4 bytes a pixel.
pub fn decode_rgba_within(bytes: &[u8], bound: ImageBound) -> Result<RgbaImage, String> {
    let mut reader = reader(bytes, bound, png::Transformations::normalize_to_color8())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let (w, h) = (reader.info().width, reader.info().height);
    let rgba_len = w as usize * h as usize * 4;
    let mut buf = Vec::with_capacity(size.max(rgba_len));
    buf.resize(size, 0);
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let px = info.width as usize * info.height as usize;
    // Expand from the end, so no pixel is overwritten before it is read.
    let expand = |buf: &mut Vec<u8>, from: usize, pixel: fn(&[u8]) -> [u8; 4]| -> Result<(), String> {
        if buf.len() != px * from {
            return Err("decoded size mismatch".into());
        }
        buf.resize(px * 4, 0);
        for i in (0..px).rev() {
            let p = pixel(&buf[i * from..i * from + from]);
            buf[i * 4..i * 4 + 4].copy_from_slice(&p);
        }
        Ok(())
    };
    match info.color_type {
        png::ColorType::Rgba => {}
        png::ColorType::Rgb => expand(&mut buf, 3, |p| [p[0], p[1], p[2], 255])?,
        png::ColorType::GrayscaleAlpha => expand(&mut buf, 2, |p| [p[0], p[0], p[0], p[1]])?,
        png::ColorType::Grayscale => expand(&mut buf, 1, |p| [p[0], p[0], p[0], 255])?,
        png::ColorType::Indexed => return Err("palette was not expanded".into()),
    }
    RgbaImage::from_raw(info.width, info.height, buf).ok_or_else(|| "decoded size mismatch".into())
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
/// Within [`IMAGES`] (`decode_indexed_within` for a level's material).
pub fn decode_indexed(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    decode_indexed_within(bytes, IMAGES)
}

/// [`decode_indexed`] within `bound`.
pub fn decode_indexed_within(bytes: &[u8], bound: ImageBound) -> Result<(u32, u32, Vec<u8>), String> {
    let mut reader = reader(bytes, bound, png::Transformations::IDENTITY)?;
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

/// A PNG whose header declares `width` × `height` (8-bit RGBA, or palette
/// when `indexed`) but that holds no image: what a decompression bomb looks
/// like to a decoder's size check. For the tests of the bounds.
#[doc(hidden)]
pub fn declared_png(width: u32, height: u32, indexed: bool) -> Vec<u8> {
    let color = if indexed { png::ColorType::Indexed } else { png::ColorType::Rgba };
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(color);
        enc.set_depth(png::BitDepth::Eight);
        if color == png::ColorType::Indexed {
            enc.set_palette(vec![0; 3]);
        }
        let mut w = enc.write_header().expect("in-memory PNG header");
        w.write_chunk(png::chunk::IDAT, &[0x78, 0x9c, 0x03, 0x00]).expect("in-memory PNG chunk");
    }
    out
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

    /// A small file declaring a huge image (a decompression bomb) is
    /// refused from its header, before anything is allocated for it; a
    /// legitimate wide strip (OLX's `out_font.png` is 11563 × 19) is not.
    #[test]
    fn huge_images_are_refused_before_decoding() {
        let bomb = declared_png(12000, 12000, false);
        assert!(bomb.len() < 100);
        let e = decode_rgba(&bomb).unwrap_err();
        assert!(e.contains("above"), "{e}");
        let e = decode_indexed(&declared_png(12000, 12000, true)).unwrap_err();
        assert!(e.contains("above"), "{e}");
        let e = decode_rgba(&declared_png(MAX_IMAGE_SIDE + 1, 1, false)).unwrap_err();
        assert!(e.contains("above"), "{e}");
        let strip = encode_rgba(&RgbaImage::new(11563, 19));
        assert_eq!(decode_rgba(&strip).unwrap().width(), 11563);
        // Level art has a bound of its own (its art scale), still finite.
        let art = declared_png(8192, 8192, false);
        assert!(decode_rgba(&art).unwrap_err().contains("above"));
        assert!(!decode_rgba_within(&art, LEVEL_ART).unwrap_err().contains("above"));
        let e = decode_rgba_within(&declared_png(8192, 8193, false), LEVEL_ART).unwrap_err();
        assert!(e.contains("above"), "{e}");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_rgba(b"not a png").is_err());
        assert!(decode_indexed(&[0x89, b'P', b'N', b'G']).is_err());
    }
}
