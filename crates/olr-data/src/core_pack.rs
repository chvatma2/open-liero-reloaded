//! The core pack: OpenLieroX's shared game assets (spec §19 E9).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::Rgba;

/// The `core.ron` part of a core pack: what it holds besides the files.
///
/// OLX loads these from its data directory for every game, whatever the mod:
/// the game sprites of `DeprecatedGUI::LoadGraphics`
/// (`src/client/DeprecatedGUI/Graphics.cpp:56-97`) and the gibs of
/// `CWorm::Prepare` (`src/common/CWorm.cpp:661`), the game sounds of
/// `LoadSounds` (`src/game/Sounds.cpp:25-30`, imported as `.ogg`) and the
/// colours of `data/frontend/colours.cfg` (`InitializeColors`, `Graphics.cpp:134-321`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoreDef {
    /// Sprite names, such as `crosshair`: each is the file `gfx/<name>.png`,
    /// from OLX's `data/gfx/<name>.png`.
    pub sprites: Vec<String>,
    /// Sound file names, such as `throw.ogg`: each is the file
    /// `sounds/<name>`, from OLX's `data/sounds/<name>`.
    pub sounds: Vec<String>,
    /// The `[Colours]` section of `colours.cfg`, by key (such as
    /// `RopeColour1`), as OLX's `StrToCol` reads each value.
    pub colours: BTreeMap<String, Rgba>,
}
