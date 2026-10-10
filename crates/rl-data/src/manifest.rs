//! `pack.ron`: what every pack declares about itself.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Current native pack format. Bump it when the on-disk layout changes and add
/// a migration in `rl-pack`.
pub const PACK_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackKind {
    Mod,
    Level,
    Skin,
    Theme,
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
