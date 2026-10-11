//! What a menu lists (spec §21 G1, G6): the mods with their weapon
//! restriction presets and the levels with their size and thumbnail, read
//! without loading any mod or level.

use anyhow::Context as _;
use rl_data::image::RgbaImage;
use rl_data::manifest::PackKind;
use rl_vfs::{ContentSource, VPath};

use crate::Content;

/// The widest and tallest thumbnail shown; a larger `thumb.png` is skipped
/// with a warning (a pack must not make the menu decode a huge image).
pub const MAX_THUMB_SIDE: u32 = 512;

/// The largest `thumb.png` read, in bytes.
pub const MAX_THUMB_BYTES: usize = 4 << 20;

/// A mod as the menus list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModInfo {
    pub id: String,
    /// The pack's name, such as `Liero v1.0`.
    pub name: String,
    /// Its weapon restriction presets (OLX's `WeaponRestrictionsFile`
    /// choices), sorted.
    pub presets: Vec<String>,
}

/// A level as the menus list it.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelInfo {
    pub id: String,
    /// The level's name, such as `Liero Factory`.
    pub name: String,
    /// The size in world pixels.
    pub width: u32,
    pub height: u32,
    /// `thumb.png` (§21 G6), when the pack has a readable one.
    pub thumb: Option<RgbaImage>,
}

/// Every mod and level of a content directory, by id; the packs that
/// could not be read are left out with a warning each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub mods: Vec<ModInfo>,
    pub levels: Vec<LevelInfo>,
    pub warnings: Vec<String>,
}

impl Catalog {
    pub fn level(&self, id: &str) -> Option<&LevelInfo> {
        self.levels.iter().find(|l| l.id == id)
    }

    pub fn mod_info(&self, id: &str) -> Option<&ModInfo> {
        self.mods.iter().find(|m| m.id == id)
    }
}

/// The width and height a PNG's header gives, if `bytes` starts like one.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Some((be(16), be(20)))
}

/// The level pack's thumbnail: `Ok(None)` when it has none, `Err` when it
/// is unreadable or too large.
fn read_thumb(src: &dyn ContentSource) -> Result<Option<RgbaImage>, String> {
    let path = VPath::new(rl_pack::THUMB_FILE).map_err(|e| e.to_string())?;
    if !src.exists(&path) {
        return Ok(None);
    }
    let bytes = src.read(&path).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_THUMB_BYTES {
        return Err(format!("{} bytes, above {MAX_THUMB_BYTES}", bytes.len()));
    }
    let (w, h) = png_size(&bytes).ok_or("not a PNG")?;
    if w == 0 || h == 0 || w > MAX_THUMB_SIDE || h > MAX_THUMB_SIDE {
        return Err(format!("{w}x{h}, not within {MAX_THUMB_SIDE}x{MAX_THUMB_SIDE}"));
    }
    rl_pack::image_io::decode_rgba(&bytes).map(Some)
}

impl Content {
    /// Mod pack `id`'s name and presets, reading no weapon or image.
    pub fn mod_info(&self, id: &str) -> anyhow::Result<ModInfo> {
        let src = self.pack(id, PackKind::Mod)?;
        let m = rl_pack::read_manifest(&src).with_context(|| format!("mod pack {id}"))?;
        let presets = rl_pack::read_preset_names(&src).with_context(|| format!("mod pack {id}"))?;
        Ok(ModInfo { id: id.into(), name: m.name, presets })
    }

    /// Level pack `id`'s name, size and thumbnail, decoding no level image.
    /// An unreadable or oversized thumbnail is left out with a warning.
    pub fn level_info(&self, id: &str) -> anyhow::Result<(LevelInfo, Option<String>)> {
        let src = self.pack(id, PackKind::Level)?;
        let (_, def) = rl_pack::read_level_def(&src).with_context(|| format!("level pack {id}"))?;
        let (thumb, warning) = match read_thumb(&src) {
            Ok(t) => (t, None),
            Err(e) => (None, Some(format!("level pack {id}: {}: {e}, not shown", rl_pack::THUMB_FILE))),
        };
        let info = LevelInfo { id: id.into(), name: def.name, width: def.width, height: def.height, thumb };
        Ok((info, warning))
    }

    /// Every mod and level ([`Content::mod_info`], [`Content::level_info`]),
    /// in id order.
    pub fn catalog(&self) -> Catalog {
        let mut c = Catalog::default();
        for id in self.ids(PackKind::Mod) {
            match self.mod_info(&id) {
                Ok(m) => c.mods.push(m),
                Err(e) => c.warnings.push(format!("{e:#}: left out")),
            }
        }
        for id in self.ids(PackKind::Level) {
            match self.level_info(&id) {
                Ok((l, w)) => {
                    c.levels.push(l);
                    c.warnings.extend(w);
                }
                Err(e) => c.warnings.push(format!("{e:#}: left out")),
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_size_reads_the_header() {
        let png = rl_pack::image_io::encode_rgba(&RgbaImage::new(7, 3));
        assert_eq!(png_size(&png), Some((7, 3)));
        assert_eq!(png_size(b"GIF89a"), None);
        assert_eq!(png_size(&png[..20]), None);
    }
}
