//! Loads content packs (`rl-pack`) into ready-to-run inputs: a [`SimMod`]
//! with its decoded images and sound bytes, levels, themes, skins, OLX's
//! core pack, and whole sandbox [`World`]s.
//!
//! **Never panics on content** (spec §19, Review Focus 3): a missing or odd
//! asset gets a placeholder ([`PLACEHOLDER`]-coloured images of the size the
//! renderer expects, silence for sounds) and a warning. Warnings are
//! collected, never printed: [`Content::warnings`] for the directory and the
//! core pack, and a `warnings` list on each loaded item, so the caller logs
//! each once. A missing pack or an unreadable definition is an error.
//!
//! The oracle loads mods strictly ([`Content::sim_mod`]): OLX's client
//! knows every projectile image's frame count, so a missing one is an error.

mod catalog;
mod color;
mod core_assets;
mod maps;
mod matches;
mod mods;
mod placeholder;
mod replay;
mod sandbox;
mod sounds;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use rl_data::manifest::PackKind;
use rl_vfs::DirFs;

pub use catalog::{Catalog, LevelInfo, MAX_THUMB_BYTES, MAX_THUMB_SIDE, ModInfo};
pub use color::{apply_edits, color_edit, lost_area};
pub use core_assets::{CORE_PACK_ID, CORE_SPRITES, CorePack};
pub use maps::{LoadedLevel, LoadedSkin, LoadedTheme, MAX_ART_SCALE, MAX_COLOR_MAP_BYTES};
pub use matches::{BuiltMatch, MatchAssets, MatchOptions, Player, build_match};
pub use mods::{LoadedMod, check_frames};
pub use placeholder::{PLACEHOLDER, placeholder_image};
pub use replay::replay_assets_from;
pub use sandbox::{SANDBOX_START, SandboxSetup, SpawnSpec, match_setup, new_world_from};
pub use sounds::{MAX_SOUND_BYTES, MAX_SOUND_SECONDS, check_sound};

#[doc(no_inline)]
pub use rl_data::restrictions::{DEFAULT_PRESET, WeaponRules};
#[doc(no_inline)]
pub use rl_sim::{SimMod, World};

/// The pack id `rl-cli import` gives OLX mod directory `dir` (such as `MW 1.0`).
pub fn olx_mod_id(dir: &str) -> String {
    format!("olx-{}", rl_data::slug::slug(dir))
}

/// The pack id `rl-cli import` gives OLX level file `file` (such as
/// `CastleStrike.lxl`, with or without its `levels/` directory).
pub fn olx_level_id(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    format!("olx-level-{}", rl_data::slug::slug(stem))
}

/// A content directory (such as `target/content`): its packs by id, and the
/// core pack, loaded once.
pub struct Content {
    packs: BTreeMap<String, (PackKind, PathBuf)>,
    core: CorePack,
    warnings: Vec<String>,
}

impl Content {
    /// Lists the packs in `root`'s subdirectories (each with a `pack.ron`)
    /// and loads the core pack [`CORE_PACK_ID`]. A pack this build cannot
    /// read, a second pack with the same id or a missing core pack is a
    /// warning; only an unreadable `root` is an error.
    pub fn open(root: &Path) -> anyhow::Result<Content> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
            .with_context(|| format!("content directory {}", root.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join(rl_pack::MANIFEST).is_file())
            .collect();
        dirs.sort();
        let mut packs = BTreeMap::new();
        let mut warnings = Vec::new();
        for dir in dirs {
            match rl_pack::read_manifest(&DirFs::new(&dir)) {
                Ok(m) => {
                    if let Some((_, first)) = packs.get(&m.id) {
                        let first: &PathBuf = first;
                        warnings.push(format!(
                            "{}: pack {} is also {}, skipped",
                            dir.display(),
                            m.id,
                            first.display()
                        ));
                    } else {
                        packs.insert(m.id, (m.kind, dir));
                    }
                }
                Err(e) => warnings.push(format!("{}: {e}, skipped", dir.display())),
            }
        }
        let core = match packs.get(CORE_PACK_ID) {
            Some((PackKind::Core, dir)) => match rl_pack::read_core_tolerant(&DirFs::new(dir)) {
                Ok((pack, skipped)) => {
                    for e in skipped {
                        warnings.push(format!("core pack {CORE_PACK_ID}: {e}: left out"));
                    }
                    CorePack::from_pack(pack, &mut warnings)
                }
                Err(e) => {
                    warnings.push(format!("core pack {CORE_PACK_ID}: {e}: placeholders and silence instead"));
                    CorePack::empty()
                }
            },
            _ => {
                warnings.push(format!("no core pack {CORE_PACK_ID}: placeholders and silence instead"));
                CorePack::empty()
            }
        };
        Ok(Content { packs, core, warnings })
    }

    /// Problems found when opening: unreadable packs, duplicate ids, and the
    /// core pack's missing or odd assets.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// OLX's shared game assets (spec §19 E9).
    pub fn core(&self) -> &CorePack {
        &self.core
    }

    /// The ids of the packs of `kind`, sorted.
    pub fn ids(&self, kind: PackKind) -> Vec<String> {
        self.packs.iter().filter(|(_, (k, _))| *k == kind).map(|(id, _)| id.clone()).collect()
    }

    /// The pack `id`, which must be of `kind`.
    fn pack(&self, id: &str, kind: PackKind) -> anyhow::Result<DirFs> {
        match self.packs.get(id) {
            Some((k, dir)) if *k == kind => Ok(DirFs::new(dir)),
            Some((k, _)) => anyhow::bail!("pack {id} is a {k:?} pack, not {kind:?}"),
            None => anyhow::bail!("no {kind:?} pack {id}"),
        }
    }
}
