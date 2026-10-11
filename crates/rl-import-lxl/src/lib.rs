//! OpenLieroX `.lxl` level importer, following `ML_LieroX`
//! (`src/common/MapLoader_LieroX.cpp`). Only image-type levels (`MPT_IMAGE`)
//! are supported; every level in scope is one.
//!
//! Imported art is stored at `art_scale` 1 because OLX's 2x surfaces are plain
//! pixel doubling (`PutPixel2x2`), unless the file carries an
//! "OLX hi-res data" chunk, which is imported at `art_scale` 2.

use std::io::Read;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Level, LevelDef, Material, MaterialGrid};
use rl_data::manifest::{Origin, PACK_FORMAT_VERSION, PackKind, PackManifest};
use rl_data::slug::slug;
use rl_pack::LevelPack;
use rl_vfs::{ContentSource, VPath};

const HEADER_LEN: usize = 148;
const MPT_IMAGE: i32 = 1;
const PX_EMPTY: u8 = 0x01;
/// OLX encodes worm positions in 12 bits, so no level is larger than this.
const MAX_SIDE: i32 = 4096;

#[derive(Debug, thiserror::Error)]
pub enum LxlError {
    #[error("file ends early at byte {0}")]
    Truncated(usize),
    #[error("not a LieroX level (id {0:?})")]
    BadId(String),
    #[error("unsupported level version {0}")]
    UnsupportedVersion(i32),
    #[error("implausible level size {0}x{1}")]
    BadSize(i32, i32),
    #[error("pixmap levels (type {0}) are not supported yet")]
    UnsupportedType(i32),
    #[error("level data does not decompress: {0}")]
    Decompress(String),
    #[error("level data has {got} bytes, needs {need}")]
    TooSmall { got: usize, need: usize },
    #[error(transparent)]
    Vfs(#[from] rl_vfs::VfsError),
}

/// The result of importing one level.
#[derive(Debug)]
pub struct ImportedLevel {
    pub pack: LevelPack,
    pub warnings: Vec<String>,
}

/// Imports the `.lxl` file at `path` of an OLX game directory.
pub fn import_lxl(olx: &dyn ContentSource, path: &VPath, origin: Origin) -> Result<ImportedLevel, LxlError> {
    let data = olx.read(path)?;
    let (level, warnings) = parse_lxl(&data)?;
    let stem = path.file_name().rsplit_once('.').map_or(path.file_name(), |(s, _)| s);
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: format!("olx-level-{}", slug(stem)),
        name: level.def.name.clone(),
        version: "1".to_string(),
        kind: PackKind::Level,
        authors: Vec::new(),
        origin: Some(origin),
        license: None,
    };
    Ok(ImportedLevel { pack: LevelPack { manifest, level, thumb: None }, warnings })
}

fn le_i32(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| c as char).collect()
}

/// Parses a complete `.lxl` file into a level plus warnings.
pub fn parse_lxl(data: &[u8]) -> Result<(Level, Vec<String>), LxlError> {
    if data.len() < HEADER_LEN + 8 {
        return Err(LxlError::Truncated(data.len()));
    }
    let id = cstr(&data[0..32]);
    let ctf = match id.as_str() {
        "LieroX Level" => false,
        "LieroX CTF Level" => true,
        _ => return Err(LxlError::BadId(id)),
    };
    let version = le_i32(data, 32);
    if version != 0 {
        return Err(LxlError::UnsupportedVersion(version));
    }
    let name = cstr(&data[36..100]);
    let (w, h) = (le_i32(data, 100), le_i32(data, 104));
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return Err(LxlError::BadSize(w, h));
    }
    let ty = le_i32(data, 108);
    if ty != MPT_IMAGE {
        return Err(LxlError::UnsupportedType(ty));
    }
    let theme = cstr(&data[112..144]);
    let (w, h) = (w as u32, h as u32);
    let n = w as usize * h as usize;
    let need = n * 7;

    let size = u32::from_le_bytes(data[148..152].try_into().expect("4 bytes")) as usize;
    let body_start = HEADER_LEN + 8;
    let body_end =
        body_start.checked_add(size).filter(|&e| e <= data.len()).ok_or(LxlError::Truncated(data.len()))?;
    let mut raw = Vec::with_capacity(need + 1);
    // Bounded read: a forged size cannot make us inflate more than one byte past what we use.
    flate2::read::ZlibDecoder::new(&data[body_start..body_end])
        .take(need as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| LxlError::Decompress(e.to_string()))?;
    if raw.len() < need {
        return Err(LxlError::TooSmall { got: raw.len(), need });
    }

    let rgb = |i: usize| Rgba::rgb(raw[i * 3], raw[i * 3 + 1], raw[i * 3 + 2]);
    let mut back = RgbaImage::new(w, h);
    let mut front = RgbaImage::new(w, h);
    let mut cells = Vec::with_capacity(n);
    for i in 0..n {
        let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
        let b = rgb(i);
        let flags = raw[n * 6 + i];
        back.set(x, y, b);
        // LoadImageFormat copies the back pixel wherever PX_EMPTY is set.
        front.set(x, y, if flags & PX_EMPTY != 0 { b } else { rgb(n + i) });
        cells.push(Material::from_lx_flags(flags));
    }
    let material = MaterialGrid::from_cells(w, h, cells).expect("n cells");

    let mut warnings = Vec::new();
    let mut rest = &data[body_end..];
    if ctf {
        warnings.push("old-format CTF data ignored".to_string());
        rest = rest.get(12..).unwrap_or(&[]);
    }
    let mut art_scale = 1;
    while !rest.is_empty() {
        if rest.len() < 20 {
            warnings.push("truncated chunk header ignored".to_string());
            break;
        }
        let chunk = cstr(&rest[..16]);
        let len = u32::from_le_bytes(rest[16..20].try_into().expect("4 bytes")) as usize;
        let Some(body) = rest.get(20..20 + len) else {
            warnings.push(format!("truncated chunk {chunk:?} ignored"));
            break;
        };
        rest = &rest[20 + len..];
        if chunk.eq_ignore_ascii_case("OLX hi-res data") {
            match hires(body, w, h) {
                Ok((mut f, b)) => {
                    // As in LoadLevelImageHiRes: show the back 2x2 block where PX_EMPTY is set.
                    for i in 0..n {
                        if raw[n * 6 + i] & PX_EMPTY != 0 {
                            let (x, y) = ((i % w as usize) as u32 * 2, (i / w as usize) as u32 * 2);
                            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                                f.set(x + dx, y + dy, b.get(x + dx, y + dy));
                            }
                        }
                    }
                    front = f;
                    back = b;
                    art_scale = 2;
                }
                Err(e) => warnings.push(format!("hi-res chunk ignored: {e}")),
            }
        } else {
            warnings.push(format!("chunk {chunk:?} ignored"));
        }
    }

    let def = LevelDef {
        name,
        width: w,
        height: h,
        art_scale,
        theme: format!("olx-theme-{}", slug(&theme)),
        spawns: Vec::new(),
    };
    Ok((Level { def, material, front, back }, warnings))
}

/// "OLX hi-res data": a PNG of 2W x 4H with the front image on top and the back
/// image below (`LoadLevelImageHiRes`, `MapLoader_LieroX.cpp:285-360`), alpha dropped.
fn hires(png: &[u8], w: u32, h: u32) -> Result<(RgbaImage, RgbaImage), String> {
    // The size from the header first: nothing else is decoded.
    let (pw, ph) = rl_pack::image_io::png_size(png).ok_or("not a PNG")?;
    if (pw, ph) != (w * 2, h * 4) {
        return Err(format!("size {pw}x{ph} is not {}x{}", w * 2, h * 4));
    }
    let img = rl_pack::image_io::decode_rgba_within(png, rl_pack::image_io::LEVEL_ART)?;
    // OLX reads only RGB from this PNG.
    let opaque = |mut part: RgbaImage| {
        for y in 0..part.height() {
            for x in 0..part.width() {
                part.set(x, y, Rgba { a: 255, ..part.get(x, y) });
            }
        }
        part
    };
    Ok((opaque(img.crop(0, 0, w * 2, h * 2)), opaque(img.crop(0, h * 2, w * 2, h * 2))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Builds an image-type `.lxl` from back/front colors and flags.
    fn lxl(w: i32, h: i32, back: &[[u8; 3]], front: &[[u8; 3]], flags: &[u8], extra: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let fixed = |s: &str, len: usize| {
            let mut b = vec![0xCCu8; len]; // garbage after NUL like real files
            b[..s.len()].copy_from_slice(s.as_bytes());
            b[s.len()] = 0;
            b
        };
        out.extend(fixed("LieroX Level", 32));
        out.extend(0i32.to_le_bytes());
        out.extend(fixed("Test Level", 64));
        out.extend(w.to_le_bytes());
        out.extend(h.to_le_bytes());
        out.extend(MPT_IMAGE.to_le_bytes());
        out.extend(fixed("dirt", 32));
        out.extend(0i32.to_le_bytes());
        let mut raw: Vec<u8> = back.iter().chain(front).flatten().copied().collect();
        raw.extend_from_slice(flags);
        raw.push(0); // real files have one extra byte
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        let z = z.finish().unwrap();
        out.extend((z.len() as u32).to_le_bytes());
        out.extend((raw.len() as u32).to_le_bytes());
        out.extend(z);
        out.extend_from_slice(extra);
        out
    }

    fn two_by_one() -> Vec<u8> {
        lxl(2, 1, &[[10, 10, 10], [20, 20, 20]], &[[1, 2, 3], [4, 5, 6]], &[0x01, 0x02], &[])
    }

    #[test]
    fn parses_header_material_and_art() {
        let (level, warnings) = parse_lxl(&two_by_one()).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(level.def.name, "Test Level");
        assert_eq!((level.def.width, level.def.height, level.def.art_scale), (2, 1, 1));
        assert_eq!(level.def.theme, "olx-theme-dirt");
        assert_eq!(level.material.cells(), &[Material::Background, Material::Dirt]);
        // Empty pixel shows the back image; dirt shows the front image.
        assert_eq!(level.front.get(0, 0), Rgba::rgb(10, 10, 10));
        assert_eq!(level.front.get(1, 0), Rgba::rgb(4, 5, 6));
        assert_eq!(level.back.get(1, 0), Rgba::rgb(20, 20, 20));
    }

    #[test]
    fn hires_chunk_replaces_art() {
        let mut hi = RgbaImage::new(4, 4);
        hi.set(0, 0, Rgba::rgb(7, 7, 7)); // front, top half
        hi.set(3, 3, Rgba::rgb(9, 9, 9)); // back, bottom half
        let png = rl_pack::image_io::encode_rgba(&hi);
        let mut chunk = b"OLX hi-res data\0".to_vec();
        chunk.extend((png.len() as u32).to_le_bytes());
        chunk.extend(&png);
        let data = lxl(2, 1, &[[0; 3]; 2], &[[0; 3]; 2], &[2, 2], &chunk);
        let (level, warnings) = parse_lxl(&data).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(level.def.art_scale, 2);
        assert_eq!(level.front.get(0, 0), Rgba::rgb(7, 7, 7));
        assert_eq!(level.back.get(3, 1), Rgba::rgb(9, 9, 9));
    }

    #[test]
    fn hires_front_shows_back_where_empty_and_drops_alpha() {
        // LoadLevelImageHiRes (MapLoader_LieroX.cpp:340-360) copies the back 2x2
        // block over the front wherever PX_EMPTY is set, and reads RGB only.
        let mut hi = RgbaImage::new(4, 4);
        for (x, y) in [(0, 0), (1, 1), (2, 0), (3, 1)] {
            hi.set(x, y, Rgba { r: 7, g: 7, b: 7, a: 0 }); // front, transparent in the PNG
        }
        for (x, y) in [(0, 2), (1, 3)] {
            hi.set(x, y, Rgba::rgb(9, 9, 9)); // back of world pixel 0
        }
        let png = rl_pack::image_io::encode_rgba(&hi);
        let mut chunk = b"OLX hi-res data\0".to_vec();
        chunk.extend((png.len() as u32).to_le_bytes());
        chunk.extend(&png);
        // World pixel 0 is empty, world pixel 1 is dirt.
        let data = lxl(2, 1, &[[0; 3]; 2], &[[0; 3]; 2], &[1, 2], &chunk);
        let (level, _) = parse_lxl(&data).unwrap();
        assert_eq!(level.front.get(0, 0), Rgba::rgb(9, 9, 9));
        assert_eq!(level.front.get(1, 1), Rgba::rgb(9, 9, 9));
        assert_eq!(level.front.get(2, 0), Rgba::rgb(7, 7, 7));
        assert_eq!(level.front.get(3, 1), Rgba::rgb(7, 7, 7));
        assert_eq!(level.back.get(0, 0).a, 255);
    }

    #[test]
    fn bad_hires_chunk_is_a_warning() {
        let mut chunk = b"OLX hi-res data\0".to_vec();
        chunk.extend(3u32.to_le_bytes());
        chunk.extend(b"bad");
        let (level, warnings) = parse_lxl(&lxl(2, 1, &[[0; 3]; 2], &[[0; 3]; 2], &[1, 1], &chunk)).unwrap();
        assert_eq!(level.def.art_scale, 1);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn rejects_bad_inputs() {
        let mut d = two_by_one();
        d[0] = b'X';
        assert!(matches!(parse_lxl(&d), Err(LxlError::BadId(_))));
        let mut d = two_by_one();
        d[108] = 0;
        assert!(matches!(parse_lxl(&d), Err(LxlError::UnsupportedType(0))));
        let d = lxl(0, 1, &[], &[], &[], &[]);
        assert!(matches!(parse_lxl(&d), Err(LxlError::BadSize(0, 1))));
        let d = lxl(3, 1, &[[0; 3]; 2], &[[0; 3]; 2], &[1, 1], &[]);
        assert!(matches!(parse_lxl(&d), Err(LxlError::TooSmall { .. })));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let d = two_by_one();
        for len in 0..d.len() {
            assert!(parse_lxl(&d[..len]).is_err(), "prefix of {len} bytes parsed");
        }
    }

    proptest::proptest! {
        #[test]
        fn never_panics(pos in 0usize..400, byte in proptest::num::u8::ANY) {
            let mut d = two_by_one();
            let pos = pos % d.len();
            d[pos] = byte;
            let _ = parse_lxl(&d);
        }
    }
}
