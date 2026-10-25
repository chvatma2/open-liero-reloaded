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

/// Whether `bytes` starts like a JPEG (an SOI marker, then another marker).
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// The width and height a PNG's `IHDR` or a JPEG's frame header gives,
/// read without decoding pixels; `None` for anything else.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if !is_jpeg(bytes) {
        return png_size(bytes);
    }
    let (i, marker) = jpeg_marker(bytes, |m| is_sof(m) || matches!(m, 0xD9 | 0xDA))?;
    if !is_sof(marker) {
        // The end of the image or the scan before any frame header.
        return None;
    }
    let be = |at: usize| Some(u32::from(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?])));
    Some((be(i + 7)?, be(i + 5)?))
}

/// A JPEG start-of-frame marker (not DHT `C4`, JPG `C8` or DAC `CC`).
fn is_sof(m: u8) -> bool {
    matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
}

/// The offset and code of the first marker after SOI for which `stop`
/// holds, walking the marker segments; every index is checked.
fn jpeg_marker(bytes: &[u8], stop: impl Fn(u8) -> bool) -> Option<(usize, u8)> {
    jpeg_marker_from(bytes, 2, stop)
}

/// `jpeg_marker` from offset `i`, a marker's start.
fn jpeg_marker_from(bytes: &[u8], mut i: usize, stop: impl Fn(u8) -> bool) -> Option<(usize, u8)> {
    loop {
        if *bytes.get(i)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(i + 1)?;
        if stop(marker) {
            return Some((i, marker));
        }
        match marker {
            // Fill bytes before a marker.
            0xFF => i += 1,
            // Markers without a segment.
            0x01 | 0xD0..=0xD7 => i += 2,
            0xD9 => return None,
            _ => {
                let len = usize::from(u16::from_be_bytes([*bytes.get(i + 2)?, *bytes.get(i + 3)?]));
                if len < 2 {
                    return None;
                }
                i += 2 + len;
            }
        }
    }
}

/// The EXIF `Orientation` (1-8) of a JPEG's APP1 segment; `None` when absent,
/// out of range, or not a JPEG. Reads only the first IFD; every index checked.
pub fn jpeg_orientation(bytes: &[u8]) -> Option<u8> {
    if !is_jpeg(bytes) {
        return None;
    }
    let mut from = 2;
    loop {
        // Metadata segments come before the first scan.
        let (i, marker) = jpeg_marker_from(bytes, from, |m| matches!(m, 0xE1 | 0xDA))?;
        if marker != 0xE1 {
            return None;
        }
        let len = usize::from(u16::from_be_bytes([*bytes.get(i + 2)?, *bytes.get(i + 3)?]));
        let data = bytes.get(i + 4..(i + 2).checked_add(len)?)?;
        if let Some(tiff) = data.strip_prefix(b"Exif\0\0") {
            return exif_orientation(tiff);
        }
        from = i + 2 + len;
    }
}

/// The `Orientation` tag (0x0112, a SHORT) of a TIFF header's IFD0.
fn exif_orientation(tiff: &[u8]) -> Option<u8> {
    let big = match tiff.get(..2)? {
        b"II" => false,
        b"MM" => true,
        _ => return None,
    };
    let u16_at = |at: usize| {
        let b = [*tiff.get(at)?, *tiff.get(at.checked_add(1)?)?];
        Some(if big { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
    };
    let u32_at = |at: usize| {
        let b: [u8; 4] = tiff.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    };
    if u16_at(2)? != 0x2A {
        return None;
    }
    let ifd = usize::try_from(u32_at(4)?).ok()?;
    let count = usize::from(u16_at(ifd)?);
    for k in 0..count {
        let entry = ifd.checked_add(2 + 12 * k)?;
        if u16_at(entry)? == 0x0112 {
            if u16_at(entry + 2)? != 3 {
                return None;
            }
            return u8::try_from(u16_at(entry + 8)?).ok().filter(|o| (1..=8).contains(o));
        }
    }
    None
}

/// `img` turned as EXIF orientation `o` asks (1 unchanged; 5-8 swap width and
/// height); an orientation outside 1-8 leaves it unchanged.
pub fn orient(img: &RgbaImage, o: u8) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    if !(2..=8).contains(&o) {
        return img.clone();
    }
    let (nw, nh) = if o >= 5 { (h, w) } else { (w, h) };
    let mut out = RgbaImage::new(nw, nh);
    for y in 0..nh {
        for x in 0..nw {
            let (ox, oy) = match o {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            out.set(x, y, img.get(ox, oy));
        }
    }
    out
}

/// Whether the JPEG's data holds an end-of-image marker after its first
/// scan: in entropy-coded data `FF` is stuffed (`FF 00`), so `FF D9`
/// there is the real end. A file cut inside its scans has none (the
/// decoder would fill the rest in grey); bytes after the end are allowed.
fn jpeg_complete(bytes: &[u8]) -> bool {
    jpeg_marker(bytes, |m| m == 0xDA)
        .is_some_and(|(sos, _)| bytes[sos..].windows(2).any(|w| w == [0xFF, 0xD9]))
}

/// A PNG or a JPEG (by its first bytes: `\x89PNG` or `FF D8 FF`) decoded
/// to RGBA8 within `bound`, the size checked from the header before any
/// pixel is decoded. A JPEG is opaque (alpha 255) and turned by its EXIF
/// `Orientation` (`jpeg_orientation`, `orient`).
pub fn decode_image_within(bytes: &[u8], bound: ImageBound) -> Result<RgbaImage, String> {
    if png_size(bytes).is_some() {
        return decode_rgba_within(bytes, bound);
    }
    if !is_jpeg(bytes) {
        return Err("not a PNG or JPEG".into());
    }
    let (w, h) = image_size(bytes).ok_or("a JPEG without a frame header")?;
    check_size(w, h, bound)?;
    if !jpeg_complete(bytes) {
        return Err("a JPEG cut short: no end of image after its scan".into());
    }
    let options = zune_core::options::DecoderOptions::default()
        .jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGB)
        .set_max_width(w as usize)
        .set_max_height(h as usize);
    let mut dec =
        zune_jpeg::JpegDecoder::new_with_options(zune_core::bytestream::ZCursor::new(bytes), options);
    let rgb = dec.decode().map_err(|e| format!("{e:?}"))?;
    let info = dec.info().ok_or("a JPEG without a frame header")?;
    if (u32::from(info.width), u32::from(info.height)) != (w, h) || rgb.len() != w as usize * h as usize * 3 {
        return Err("decoded size mismatch".into());
    }
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for p in rgb.as_chunks::<3>().0 {
        rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
    }
    let img = RgbaImage::from_raw(w, h, rgba).ok_or("decoded size mismatch")?;
    Ok(match jpeg_orientation(bytes) {
        Some(o) => orient(&img, o),
        None => img,
    })
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

/// Encodes an 8-bit grey-alpha PNG from `values`, two bytes per pixel
/// (grey, alpha), row-major.
pub fn encode_grey_alpha(width: u32, height: u32, values: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::GrayscaleAlpha);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("in-memory PNG header");
        w.write_image_data(values).expect("in-memory PNG data");
    }
    out
}

/// Decodes a grey-alpha PNG (8 or 16 bit) to `(width, height, values)`:
/// two 8-bit bytes per pixel (grey, alpha), within `bound`. Any other
/// colour type is an error.
pub fn decode_grey_alpha_within(bytes: &[u8], bound: ImageBound) -> Result<(u32, u32, Vec<u8>), String> {
    let mut reader = reader(bytes, bound, png::Transformations::normalize_to_color8())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let px = info.width as usize * info.height as usize;
    match info.color_type {
        png::ColorType::GrayscaleAlpha if buf.len() == px * 2 => Ok((info.width, info.height, buf)),
        png::ColorType::GrayscaleAlpha => Err("decoded size mismatch".into()),
        _ => Err("not a grey-alpha PNG".into()),
    }
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
/// nothing applies it at load (plan 13c). Apply it
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

    const TINY_JPG: &[u8] = include_bytes!("../tests/data/tiny.jpg");
    const TRUNCATED_JPG: &[u8] = include_bytes!("../tests/data/truncated.jpg");
    const GRADIENT_JPG: &[u8] = include_bytes!("../tests/data/gradient.jpg");

    /// `jpeg` with an APP1 segment right after SOI holding one IFD0 entry,
    /// `Orientation` (0x0112) = `o`.
    fn with_orientation(jpeg: &[u8], o: u8, big_endian: bool) -> Vec<u8> {
        let u16b = |v: u16| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
        let u32b = |v: u32| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
        let mut tiff = Vec::new();
        tiff.extend_from_slice(if big_endian { b"MM" } else { b"II" });
        tiff.extend_from_slice(&u16b(0x2A));
        tiff.extend_from_slice(&u32b(8));
        tiff.extend_from_slice(&u16b(1));
        tiff.extend_from_slice(&u16b(0x0112));
        tiff.extend_from_slice(&u16b(3));
        tiff.extend_from_slice(&u32b(1));
        tiff.extend_from_slice(&u16b(u16::from(o)));
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&u32b(0));
        let mut data = b"Exif\0\0".to_vec();
        data.extend_from_slice(&tiff);
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&u16::try_from(data.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(&data);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn jpeg_orientation_is_read_in_both_byte_orders() {
        assert_eq!(jpeg_orientation(&with_orientation(GRADIENT_JPG, 6, false)), Some(6));
        assert_eq!(jpeg_orientation(&with_orientation(GRADIENT_JPG, 6, true)), Some(6));
        assert_eq!(jpeg_orientation(GRADIENT_JPG), None);
        assert_eq!(jpeg_orientation(&with_orientation(GRADIENT_JPG, 9, false)), None);
        let full = with_orientation(GRADIENT_JPG, 3, true);
        for n in 0..=full.len() {
            let _ = jpeg_orientation(&full[..n]);
        }
    }

    #[test]
    fn decoding_applies_every_orientation() {
        let plain = decode_image_within(GRADIENT_JPG, IMAGES).unwrap();
        let (w, h) = (plain.width(), plain.height());
        assert_eq!((w, h), (64, 48));
        // The old pixel a new (x, y) takes, per orientation (the plan's table).
        let old = |o: u8, x: u32, y: u32| match o {
            1 => (x, y),
            2 => (w - 1 - x, y),
            3 => (w - 1 - x, h - 1 - y),
            4 => (x, h - 1 - y),
            5 => (y, x),
            6 => (y, h - 1 - x),
            7 => (w - 1 - y, h - 1 - x),
            _ => (w - 1 - y, x),
        };
        for o in 1..=8u8 {
            let turned = decode_image_within(&with_orientation(GRADIENT_JPG, o, false), IMAGES).unwrap();
            assert_eq!(turned, orient(&plain, o), "orientation {o}");
            let want = if o >= 5 { (h, w) } else { (w, h) };
            assert_eq!((turned.width(), turned.height()), want, "orientation {o}");
            for (x, y) in [(0, 0), (1, 0)] {
                let (ox, oy) = old(o, x, y);
                assert_eq!(turned.get(x, y), plain.get(ox, oy), "orientation {o} at ({x}, {y})");
            }
        }
    }

    #[test]
    fn decodes_png_and_jpeg() {
        let png = encode_rgba(&RgbaImage::new(5, 2));
        assert_eq!(image_size(&png), Some((5, 2)));
        let img = decode_image_within(&png, IMAGES).unwrap();
        assert_eq!((img.width(), img.height()), (5, 2));
        assert_eq!(image_size(TINY_JPG), Some((4, 3)));
        let img = decode_image_within(TINY_JPG, IMAGES).unwrap();
        assert_eq!((img.width(), img.height()), (4, 3));
        let p = img.get(1, 1);
        assert!(p.r > 200 && p.g < 40 && p.b < 40 && p.a == 255, "{p:?}");
    }

    #[test]
    fn a_hostile_image_is_refused_before_decoding() {
        // A PNG declaring 100 000 × 100 000 (no pixel data).
        let huge = declared_png(100_000, 100_000, false);
        assert!(decode_image_within(&huge, IMAGES).unwrap_err().contains("100000"));
        // A JPEG whose frame header declares 60 000 × 60 000: patch tiny.jpg's SOF0 size.
        let mut big = TINY_JPG.to_vec();
        let sof = big.windows(2).position(|w| w == [0xFF, 0xC0]).expect("baseline SOF0");
        big[sof + 5..sof + 9].copy_from_slice(&[0xEA, 0x60, 0xEA, 0x60]);
        assert_eq!(image_size(&big), Some((60_000, 60_000)));
        assert!(decode_image_within(&big, IMAGES).is_err());
        assert!(decode_image_within(TRUNCATED_JPG, IMAGES).is_err());
        // Cut inside its scan data: the decoder would fill the rest in grey.
        assert!(decode_image_within(GRADIENT_JPG, IMAGES).is_ok());
        let half = &GRADIENT_JPG[..GRADIENT_JPG.len() / 2];
        assert!(decode_image_within(half, IMAGES).unwrap_err().contains("cut short"));
        assert!(decode_image_within(b"GIF89a....", IMAGES).unwrap_err().contains("not a PNG or JPEG"));
        assert_eq!(image_size(b"nope"), None);
    }

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
