//! OpenLieroX `script.lgs` importer.
//!
//! Reads gamescript version 7 (LX 0.56), the version of every mod in scope,
//! following `CGameScript::Load` and `CGameScript::LoadProjectile`
//! (`src/common/CGameScript.cpp:582-1152`), and produces a self-contained
//! [`olr_pack::ModPack`] including the image and sound files the mod uses.

mod convert;
mod parse;
mod reader;
mod wps;

pub use convert::{ImportedMod, import_mod};
pub use parse::{ProjNode, RawMod, RawWeapon, parse_lgs};
pub use wps::{PRESET_LOADING_TIMES, import_presets, parse_wps, shipped_loadouts};

/// Everything that can go wrong while importing a mod.
#[derive(Debug, thiserror::Error)]
pub enum LgsError {
    #[error("file ends early at byte {offset} while reading {what}")]
    Truncated { offset: usize, what: &'static str },
    #[error("not a Liero Game Script (bad header id)")]
    BadId,
    #[error("unsupported gamescript version {0} (only version 7 is supported)")]
    UnsupportedVersion(u32),
    #[error("implausible {what}: {value}")]
    Implausible { what: &'static str, value: i64 },
    #[error("projectiles nested deeper than 128 levels")]
    TooDeep,
    #[error("unknown projectile type {0}")]
    UnknownProjectileType(i32),
    #[error("unknown weapon type {0} in weapon {1:?}")]
    UnknownWeaponType(i32, String),
    #[error("{0} not found")]
    MissingFile(String),
    #[error("image {0:?} not found in the mod's gfx/ or in data/gfx/")]
    MissingImage(String),
    #[error("two different files map to {0}")]
    AssetConflict(String),
    #[error(transparent)]
    Vfs(#[from] olr_vfs::VfsError),
}
