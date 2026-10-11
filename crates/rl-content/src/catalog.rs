//! What a menu lists (spec §21 G1, G6): the mods with their weapon
//! restriction presets and the levels with their size and thumbnail, read
//! without loading any mod or level. A thumbnail is only located and its
//! PNG header checked here; the menu decodes it when it shows it
//! ([`ThumbRef::load`]), so a long list never decodes every picture.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use rl_data::image::RgbaImage;
use rl_data::manifest::PackKind;

use crate::{Content, SkinInfo};

/// The widest and tallest thumbnail shown; a larger `thumb.png` is skipped
/// with a warning (a pack must not make the menu decode a huge image).
pub use rl_pack::{MAX_THUMB_BYTES, MAX_THUMB_SIDE};

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
    /// `thumb.png` (§21 G6), when the pack has one whose header is within
    /// the bounds; not decoded yet.
    pub thumb: Option<ThumbRef>,
}

/// A level's `thumb.png`, located and its PNG header checked against
/// [`MAX_THUMB_SIDE`] and [`MAX_THUMB_BYTES`], not decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThumbRef {
    pub path: PathBuf,
    /// The size its header gives.
    pub width: u32,
    pub height: u32,
}

impl ThumbRef {
    /// Checks `path`'s length and PNG header: `Ok(None)` when there is no
    /// such file, an error when it is not a PNG or out of bounds. Reads 24
    /// bytes at most.
    pub fn probe(path: &Path) -> Result<Option<ThumbRef>, String> {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        if len > MAX_THUMB_BYTES as u64 {
            return Err(format!("{len} bytes, above {MAX_THUMB_BYTES}"));
        }
        let mut header = Vec::with_capacity(24);
        file.take(24).read_to_end(&mut header).map_err(|e| e.to_string())?;
        let (width, height) = rl_pack::image_io::png_size(&header).ok_or("not a PNG")?;
        if width == 0 || height == 0 || width > MAX_THUMB_SIDE || height > MAX_THUMB_SIDE {
            return Err(format!("{width}x{height}, not within {MAX_THUMB_SIDE}x{MAX_THUMB_SIDE}"));
        }
        Ok(Some(ThumbRef { path: path.to_path_buf(), width, height }))
    }

    /// Reads and decodes the thumbnail, bounded again (the file may have
    /// changed since it was listed: at most [`MAX_THUMB_BYTES`] are read,
    /// and the header must still give the listed size before decoding).
    pub fn load(&self) -> Result<RgbaImage, String> {
        let file = std::fs::File::open(&self.path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(MAX_THUMB_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if rl_pack::image_io::png_size(&bytes) != Some((self.width, self.height)) {
            return Err(format!("no longer the {}x{} PNG listed", self.width, self.height));
        }
        rl_pack::decode_thumb(&bytes).map_err(|e| e.to_string())
    }
}

/// Every mod, level and skin of a content directory, by id; the packs
/// that could not be read are left out with a warning each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub mods: Vec<ModInfo>,
    pub levels: Vec<LevelInfo>,
    /// The skins (by name only: nothing is decoded).
    pub skins: Vec<SkinInfo>,
    pub warnings: Vec<String>,
}

impl Catalog {
    pub fn level(&self, id: &str) -> Option<&LevelInfo> {
        self.levels.iter().find(|l| l.id == id)
    }

    pub fn mod_info(&self, id: &str) -> Option<&ModInfo> {
        self.mods.iter().find(|m| m.id == id)
    }

    /// The skin `given` names: its id, or an OLX skin name such as
    /// `default` ([`crate::resolve_id`]).
    pub fn skin(&self, given: &str) -> Option<&SkinInfo> {
        let ids: Vec<String> = self.skins.iter().map(|s| s.id.clone()).collect();
        let id = crate::resolve_id(given, crate::skins::SKIN_PREFIX, &ids)?;
        self.skins.iter().find(|s| s.id == id)
    }
}

impl Content {
    /// Mod pack `id`'s name and presets, reading no weapon or image.
    pub fn mod_info(&self, id: &str) -> anyhow::Result<ModInfo> {
        let src = self.pack(id, PackKind::Mod)?;
        let m = rl_pack::read_manifest(&src).with_context(|| format!("mod pack {id}"))?;
        let presets = rl_pack::read_preset_names(&src).with_context(|| format!("mod pack {id}"))?;
        Ok(ModInfo { id: id.into(), name: m.name, presets })
    }

    /// Level pack `id`'s name, size and thumbnail ([`ThumbRef::probe`]),
    /// decoding no image. An unreadable or oversized thumbnail is left out
    /// with a warning.
    pub fn level_info(&self, id: &str) -> anyhow::Result<(LevelInfo, Option<String>)> {
        let src = self.pack(id, PackKind::Level)?;
        let (_, def) = rl_pack::read_level_def(&src).with_context(|| format!("level pack {id}"))?;
        let dir = self.pack_dir(id).context("level pack directory")?;
        let (thumb, warning) = match ThumbRef::probe(&dir.join(rl_pack::THUMB_FILE)) {
            Ok(t) => (t, None),
            Err(e) => (None, Some(format!("level pack {id}: {}: {e}, not shown", rl_pack::THUMB_FILE))),
        };
        let info = LevelInfo { id: id.into(), name: def.name, width: def.width, height: def.height, thumb };
        Ok((info, warning))
    }

    /// Every mod, level and skin ([`Content::mod_info`],
    /// [`Content::level_info`], [`Content::skins`]), in id order.
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
        c.skins = self.skins();
        c
    }
}
