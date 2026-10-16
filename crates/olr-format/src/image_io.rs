//! PNG encoding and decoding for packs and their authoring formats.

use olr_data::image::RgbaImage;

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
/// (256 MiB decoded, `olr-content`'s `MAX_COLOR_MAP_BYTES`). OLX's largest
/// level art in scope is 1350 × 550.
pub const LEVEL_ART: ImageBound = ImageBound { max_side: 4096 * 8, max_pixels: 64 << 20 };

/// `Ok` when a `w` × `h` image is within `bound`. Decoders apply it to the
/// header; each format's `check` applies it to what it would write, so a
/// saved file always reopens.
pub fn check_size(w: u32, h: u32, bound: ImageBound) -> Result<(), String> {
    if w.max(h) > bound.max_side {
        return Err(format!("{w}x{h} image: a side above {}", bound.max_side));
    }
    if u64::from(w) * u64::from(h) > bound.max_pixels {
        return Err(format!("{w}x{h} image: above {} pixels", bound.max_pixels));
    }
    Ok(())
}

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
    check_size(reader.info().width, reader.info().height, bound)?;
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

/// Unpacks `depth`-bit indices (1, 2 or 4), most significant first, of a
/// `width`-wide row.
fn unpack_row(row: &[u8], depth: u8, width: usize, out: &mut Vec<u8>) {
    let per_byte = 8 / depth as usize;
    let mask = (1u8 << depth) - 1;
    for i in 0..width {
        let byte = row[i / per_byte];
        let shift = 8 - depth as usize * (i % per_byte + 1);
        out.push((byte >> shift) & mask);
    }
}

/// Decodes a 1-, 2-, 4- or 8-bit palette PNG to `(width, height, indices)`
/// (one byte per index), without expanding the palette. Any other PNG kind
/// is an error, because the indices are the data. Within [`IMAGES`]
/// (`decode_indexed_within` for a level's material).
pub fn decode_indexed(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    decode_indexed_within(bytes, IMAGES)
}

/// [`decode_indexed`] within `bound`.
pub fn decode_indexed_within(bytes: &[u8], bound: ImageBound) -> Result<(u32, u32, Vec<u8>), String> {
    let mut reader = reader(bytes, bound, png::Transformations::IDENTITY)?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let depth = match info.bit_depth {
        png::BitDepth::One => 1u8,
        png::BitDepth::Two => 2,
        png::BitDepth::Four => 4,
        png::BitDepth::Eight => 8,
        png::BitDepth::Sixteen => 16,
    };
    if info.color_type != png::ColorType::Indexed || depth > 8 {
        return Err("not an 8-bit or lower indexed PNG (palette)".into());
    }
    buf.truncate(info.buffer_size());
    if depth == 8 {
        return Ok((info.width, info.height, buf));
    }
    let (w, h) = (info.width as usize, info.height as usize);
    let stride = info.line_size;
    if stride == 0 || buf.len() < stride * h {
        return Err("decoded size mismatch".into());
    }
    let mut out = Vec::with_capacity(w * h);
    for row in buf.chunks_exact(stride).take(h) {
        unpack_row(row, depth, w, &mut out);
    }
    Ok((info.width, info.height, out))
}

/// Encodes an 8-bit greyscale PNG.
pub fn encode_grey(width: u32, height: u32, values: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("in-memory PNG header");
        w.write_image_data(values).expect("in-memory PNG data");
    }
    out
}

/// Decodes a greyscale PNG (1-16 bit; with alpha, its grey) to
/// `(width, height, values)` of 8-bit values, within `bound`. Any other
/// colour type is an error.
pub fn decode_grey_within(bytes: &[u8], bound: ImageBound) -> Result<(u32, u32, Vec<u8>), String> {
    let mut reader = reader(bytes, bound, png::Transformations::normalize_to_color8())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let px = info.width as usize * info.height as usize;
    match info.color_type {
        png::ColorType::Grayscale if buf.len() == px => Ok((info.width, info.height, buf)),
        png::ColorType::GrayscaleAlpha if buf.len() == px * 2 => {
            let grey = buf.as_chunks::<2>().0.iter().map(|p| p[0]).collect();
            Ok((info.width, info.height, grey))
        }
        png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha => Err("decoded size mismatch".into()),
        _ => Err("not a greyscale PNG".into()),
    }
}

/// OLX's colour key applied once (`RgbaImage::key_transparent`: the corner
/// rule per image, `src/client/GfxPrimitives.cpp:114-150`): key pixels
/// become `(0, 0, 0, 0)`, everything else is unchanged. Importers use it;
/// nothing applies it at load once plan 13c switches the loaders. Apply it
/// only once: a second application to a `(254, 0, 254)`-keyed result clears
/// real pure magenta (its corners are now transparent, so the key falls
/// back to magenta).
pub fn olx_sprite_to_alpha(img: &RgbaImage) -> RgbaImage {
    let mut out = img.clone();
    out.key_transparent();
    out
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
    use olr_data::image::Rgba;

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

    /// Encoded bytes are content: packs hash them. The `png` crate is
    /// pinned and its compressors live only in Cargo.lock, so this test
    /// catches drift (a bump that changes the bytes must re-bless packs).
    #[test]
    fn encoded_bytes_are_pinned() {
        use sha2::{Digest, Sha256};
        let hex = |b: &[u8]| Sha256::digest(b).iter().map(|v| format!("{v:02x}")).collect::<String>();
        let mut img = RgbaImage::new(4, 3);
        for y in 0..3 {
            for x in 0..4 {
                img.set(
                    x,
                    y,
                    Rgba {
                        r: (x * 60) as u8,
                        g: (y * 100) as u8,
                        b: (x * y * 20) as u8,
                        a: 255 - (x * 30) as u8,
                    },
                );
            }
        }
        let rgba = encode_rgba(&img);
        let indexed = encode_indexed(4, 2, &[0, 1, 2, 1, 2, 0, 1, 2], &[0, 0, 0, 255, 0, 255, 9, 9, 9]);
        assert_eq!(
            hex(&rgba),
            "936d16d4c55a5afacb280b6d8c5aea092d7da375fa10fb1e25fcaeee8ab00995",
            "encode_rgba drifted"
        );
        assert_eq!(
            hex(&indexed),
            "530c48c7404e0348e66925fec624737aa47373f9896391cda98a695588574b47",
            "encode_indexed drifted"
        );
    }

    #[test]
    fn check_size_applies_both_limits() {
        assert!(check_size(IMAGES.max_side, 1, IMAGES).is_ok());
        assert!(check_size(IMAGES.max_side + 1, 1, IMAGES).unwrap_err().contains("a side above"));
        assert!(check_size(4097, 4096, IMAGES).unwrap_err().contains("pixels"));
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

    fn indexed_png(
        width: u32,
        height: u32,
        depth: png::BitDepth,
        packed_rows: &[u8],
        palette: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, width, height);
            enc.set_color(png::ColorType::Indexed);
            enc.set_depth(depth);
            enc.set_palette(palette.to_vec());
            enc.write_header().unwrap().write_image_data(packed_rows).unwrap();
        }
        out
    }

    #[test]
    fn low_bit_palettes_read_like_eight_bit_ones() {
        let pal = [0, 0, 0, 255, 255, 255, 255, 0, 255, 9, 9, 9];
        // 3×2 at 2 bits: rows [0,1,2] and [3,2,1], each padded to a byte.
        let png2 = indexed_png(3, 2, png::BitDepth::Two, &[0b00_01_10_00, 0b11_10_01_00], &pal);
        assert_eq!(decode_indexed_within(&png2, IMAGES).unwrap(), (3, 2, vec![0, 1, 2, 3, 2, 1]));
        // 4×1 at 1 bit: [1,0,1,1].
        let png1 = indexed_png(4, 1, png::BitDepth::One, &[0b1011_0000], &pal[..6]);
        assert_eq!(decode_indexed_within(&png1, IMAGES).unwrap(), (4, 1, vec![1, 0, 1, 1]));
        // 3×1 at 4 bits: [2,0,3].
        let png4 = indexed_png(3, 1, png::BitDepth::Four, &[0x20, 0x30], &pal);
        assert_eq!(decode_indexed_within(&png4, IMAGES).unwrap(), (3, 1, vec![2, 0, 3]));
    }

    #[test]
    fn an_rgb_png_is_not_an_indexed_one() {
        let rgb = encode_rgba(&RgbaImage::new(2, 2));
        let e = decode_indexed_within(&rgb, IMAGES).unwrap_err();
        assert!(e.contains("8-bit or lower indexed PNG"), "{e}");
    }

    #[test]
    fn grey_round_trips_and_grey_alpha_reads_its_grey() {
        let png = encode_grey(3, 1, &[0, 128, 255]);
        assert_eq!(decode_grey_within(&png, IMAGES).unwrap(), (3, 1, vec![0, 128, 255]));
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(png::ColorType::GrayscaleAlpha);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[200, 10, 7, 255]).unwrap();
        }
        assert_eq!(decode_grey_within(&out, IMAGES).unwrap(), (2, 1, vec![200, 7]));
        assert!(
            decode_grey_within(&encode_rgba(&RgbaImage::new(1, 1)), IMAGES)
                .unwrap_err()
                .contains("greyscale")
        );
        assert!(decode_grey_within(&declared_png(100_000, 100_000, false), IMAGES).is_err());
    }

    #[test]
    fn the_olx_key_becomes_alpha_once() {
        let mut img = RgbaImage::new(3, 1);
        img.set(0, 0, Rgba::MAGENTA);
        img.set(1, 0, Rgba::rgb(1, 2, 3));
        img.set(2, 0, Rgba { r: 9, g: 9, b: 9, a: 128 });
        let a = olx_sprite_to_alpha(&img);
        let first = a.clone();
        assert_eq!(a.get(0, 0), Rgba::TRANSPARENT);
        assert_eq!(a.get(1, 0), Rgba::rgb(1, 2, 3));
        assert_eq!(a.get(2, 0), Rgba { r: 9, g: 9, b: 9, a: 128 }, "partial alpha survives");
        // A (254,0,254) corner keys 254, and pure magenta stays a colour.
        let mut k = RgbaImage::new(2, 1);
        k.set(0, 0, Rgba::rgb(254, 0, 254));
        k.set(1, 0, Rgba::MAGENTA);
        let a = olx_sprite_to_alpha(&k);
        assert_eq!((a.get(0, 0), a.get(1, 0)), (Rgba::TRANSPARENT, Rgba::MAGENTA));
        // Applying OLX's key again changes nothing (for an image whose key
        // pixels are gone and that has no 254 corner).
        let mut again = first.clone();
        again.key_transparent();
        assert_eq!(again, first);
    }
}
