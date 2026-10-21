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
