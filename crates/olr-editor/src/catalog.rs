//! What the host knows about installed packs, and the settings it keeps
//! for the editor between sessions.

use std::path::PathBuf;
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_edit::level::StampMask;
use serde::{Deserialize, Serialize};

/// The installed themes and mods the editor offers.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub themes: Vec<ThemeEntry>,
    pub mods: Vec<ModEntry>,
    /// Where the file browser opens when no folder is remembered (the
    /// host's home directory; none: the working directory).
    pub home: Option<PathBuf>,
    /// The user's own textures (plan 14e R2), in the host's order.
    pub textures: Vec<TextureEntry>,
    /// The asset library (issue #9): the built-in assets, then the user's,
    /// each in the host's order.
    pub assets: Vec<AssetEntry>,
    /// The surface material library (issue #43;
    /// `olr_edit::material::list_materials`): the built-in materials, then
    /// the user's. A level holds copies of those it uses.
    pub materials: Vec<olr_edit::material::MaterialEntry>,
}

/// An asset of the library, by where it is: built in (shipped with the
/// game, read-only) or the user's, and its folder's name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetKey {
    pub builtin: bool,
    pub id: String,
}

/// What the Assets tab shows of an asset (`olr_edit::asset::Summary`):
/// its name, size in world pixels, what its elements are, whether it
/// glows, and its thumbnail (none when absent or bad).
#[derive(Clone, Debug, PartialEq)]
pub struct AssetEntry {
    pub key: AssetKey,
    pub name: String,
    pub size: (u32, u32),
    pub tags: Vec<&'static str>,
    pub glows: bool,
    pub thumb: Option<RgbaImage>,
}

impl AssetEntry {
    /// The tag under its thumbnail: what it is ("rock, background"), and
    /// "glows".
    pub fn tag(&self) -> String {
        let mut words: Vec<&str> = self.tags.clone();
        if self.glows {
            words.push("glows");
        }
        words.join(", ")
    }
}

/// A texture of the user's library: its name and its image.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureEntry {
    pub name: String,
    pub image: RgbaImage,
}

/// A theme pack: its id, its name, the tiles painting and the bake use,
/// and its holes' covers, the crater brushes (plan 14e R4).
#[derive(Clone, Debug)]
pub struct ThemeEntry {
    pub id: String,
    pub name: String,
    pub front_tile: RgbaImage,
    pub back_tile: RgbaImage,
    pub holes: Vec<Arc<StampMask>>,
}

/// A mod pack Play can use.
#[derive(Clone, Debug)]
pub struct ModEntry {
    pub id: String,
    pub name: String,
}

/// Remembered between sessions (the host stores it; plan 14b ruling R9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// The mod Play starts the match with.
    pub play_mod: String,
    /// Bots in a Play match, 0 to 8.
    pub play_bots: u8,
    /// The file browser's folder last chosen from (plan 14c R8).
    pub browse_dir: Option<PathBuf>,
    /// The properties' "More" is open (issue #8).
    pub more_open: bool,
    /// The lighting's "Fine tuning" is open (issue #8).
    pub fine_tuning_open: bool,
}

impl Default for EditorSettings {
    fn default() -> EditorSettings {
        EditorSettings {
            play_mod: "olx-classic".into(),
            play_bots: 1,
            browse_dir: None,
            more_open: false,
            fine_tuning_open: false,
        }
    }
}
