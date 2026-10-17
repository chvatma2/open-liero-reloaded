//! `pack.ron`: what every pack declares about itself.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Current native pack format. Bump it when the on-disk layout changes.
/// `olr-pack` reads only this version and refuses any other ("re-import the
/// pack"); packs are imports in `content/packs` until packs are published,
/// so there are no migrations.
///
/// - 2: theme packs carry the optional `green_ball.png`.
/// - 3: mod packs carry their weapon restriction presets (`restrictions.ron`).
/// - 4: level packs carry a `thumb.png` made at import (plan 7 G6).
/// - 5: every pack kind is in the native authoring format (plan 13c;
///   `olr-format`: canonical RON, images keyed once by the importers, theme
///   stamps as a meaning mask plus a paint layer, skins as a frame grid
///   plus a tint); the version is this build's.
pub const PACK_FORMAT_VERSION: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackKind {
    Mod,
    Level,
    Skin,
    Theme,
    /// OLX's shared game assets ([`crate::core_pack::CoreDef`]).
    Core,
}

/// Where imported content came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// For example `"OpenLieroX"`.
    pub source: String,
    /// Path inside the source, for example `"share/gamedir/MW 1.0"`.
    pub path: String,
    /// Source revision, for example a git commit.
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackManifest {
    pub format_version: u32,
    /// Lowercase slug, unique among installed packs, such as `"olx-mw-1-0"`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PackKind,
    pub authors: Vec<String>,
    pub origin: Option<Origin>,
    pub license: Option<String>,
}
