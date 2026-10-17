//! What the host knows about installed packs, and the settings it keeps
//! for the editor between sessions.

use olr_data::image::RgbaImage;
use serde::{Deserialize, Serialize};

/// The installed themes and mods the editor offers.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub themes: Vec<ThemeEntry>,
    pub mods: Vec<ModEntry>,
}

/// A theme pack: its id, its name and the tiles painting and the bake use.
#[derive(Clone, Debug)]
pub struct ThemeEntry {
    pub id: String,
    pub name: String,
    pub front_tile: RgbaImage,
    pub back_tile: RgbaImage,
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
}

impl Default for EditorSettings {
    fn default() -> EditorSettings {
        EditorSettings { play_mod: "olx-classic".into(), play_bots: 1 }
    }
}
