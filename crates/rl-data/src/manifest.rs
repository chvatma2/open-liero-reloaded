//! `pack.ron`: what every pack declares about itself.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Current native pack format. Bump it when the on-disk layout changes.
/// `rl-pack` reads only this version and refuses any other ("re-import the
/// pack"); packs are imports in `target/content` until packs are published,
/// so there are no migrations.
///
/// - 2: theme packs carry the optional `green_ball.png`.
pub const PACK_FORMAT_VERSION: u32 = 2;

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
