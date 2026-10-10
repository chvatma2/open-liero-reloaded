//! [`CorePack`]: OLX's shared game sprites, game sounds and colours, with
//! placeholders for what is missing.

use std::collections::BTreeMap;

use rl_data::image::{Rgba, RgbaImage};
use rl_sim::{GAME_SOUND_PREFIX, GAME_SOUNDS};

use crate::placeholder::{PLACEHOLDER_SIZE, placeholder_image};

/// The core pack's id (`rl-import-gfx::CORE_PACK_ID`).
pub const CORE_PACK_ID: &str = "olx-core";

/// The sprites the renderer reads from the core pack, with their sizes in
/// OLX `d1ad953f`'s `data/gfx` (`docs/porting/olx-notes/presentation.md`,
/// "Assets the remake must import"). A missing or differently sized one is
/// replaced by a placeholder of this size, so frame arithmetic still works.
pub const CORE_SPRITES: [(&str, u32, u32); 11] = [
    ("crosshair", 12, 6),
    ("muzzle", 224, 16),
    ("explosion", 512, 32),
    ("smoke", 70, 14),
    ("chemsmoke", 40, 10),
    ("spawn", 160, 32),
    ("sparkle", 30, 10),
    ("giblets", 32, 12),
    ("hook", 6, 6),
    ("bonus", 12, 8),
    ("health", 10, 10),
];

/// OLX's shared game assets, loaded from the core pack (spec §19 E9).
#[derive(Clone, Debug)]
pub struct CorePack {
    /// The pack's sprites, without odd ones.
    sprites: BTreeMap<String, RgbaImage>,
    /// Placeholders for [`CORE_SPRITES`] that are missing or odd.
    placeholders: BTreeMap<String, RgbaImage>,
    /// For any other missing sprite.
    unknown: RgbaImage,
    sounds: BTreeMap<String, Vec<u8>>,
    colours: BTreeMap<String, Rgba>,
}

impl CorePack {
    /// Checks `pack` against [`CORE_SPRITES`] and [`GAME_SOUNDS`], with a
    /// warning for each placeholder or missing sound.
    pub(crate) fn from_pack(pack: rl_pack::CorePack, warnings: &mut Vec<String>) -> Self {
        let mut core = CorePack {
            sprites: pack.sprites,
            placeholders: BTreeMap::new(),
            unknown: placeholder_image(PLACEHOLDER_SIZE, PLACEHOLDER_SIZE),
            sounds: pack.sounds,
            colours: pack.def.colours,
        };
        for (name, w, h) in CORE_SPRITES {
            let found = core.sprites.get(name).map(|s| (s.width(), s.height()));
            if found == Some((w, h)) {
                continue;
            }
            match found {
                Some((fw, fh)) => {
                    warnings
                        .push(format!("core sprite {name} is {fw}x{fh}, not {w}x{h}: a placeholder instead"));
                    core.sprites.remove(name);
                }
                None => warnings.push(format!("core sprite {name} is missing: a placeholder instead")),
            }
            core.placeholders.insert(name.to_string(), placeholder_image(w, h));
        }
        for s in GAME_SOUNDS {
            if core.game_sound(s).is_none() {
                warnings.push(format!("game sound {s} is missing from the core pack: silence instead"));
            }
        }
        core
    }

    /// No core pack: every sprite a placeholder, every sound silent (the
    /// caller warns once for the whole pack).
    pub(crate) fn empty() -> Self {
        let manifest = rl_data::manifest::PackManifest {
            format_version: rl_data::manifest::PACK_FORMAT_VERSION,
            id: CORE_PACK_ID.into(),
            name: String::new(),
            version: String::new(),
            kind: rl_data::manifest::PackKind::Core,
            authors: vec![],
            origin: None,
            license: None,
        };
        let def = rl_data::core_pack::CoreDef { sprites: vec![], sounds: vec![], colours: BTreeMap::new() };
        let pack = rl_pack::CorePack { manifest, def, sprites: BTreeMap::new(), sounds: BTreeMap::new() };
        Self::from_pack(pack, &mut Vec::new())
    }

    /// Sprite `name` (such as `crosshair`, from OLX's `data/gfx/crosshair.png`),
    /// or a placeholder: of the [`CORE_SPRITES`] size for those, 8 × 8 otherwise.
    pub fn sprite(&self, name: &str) -> &RgbaImage {
        self.sprites.get(name).or_else(|| self.placeholders.get(name)).unwrap_or(&self.unknown)
    }

    /// Whether the pack has sprite `name` (not a placeholder).
    pub fn has_sprite(&self, name: &str) -> bool {
        self.sprites.contains_key(name)
    }

    /// The bytes of sound file `name` (such as `throw.ogg`), if the pack has it.
    pub fn sound(&self, name: &str) -> Option<&[u8]> {
        self.sounds.get(name).map(Vec::as_slice)
    }

    /// The bytes for a game sound as [`rl_sim::SimMod::sounds`] names it
    /// (`olx:data/sounds/throw.wav`): the core pack's file of that name, or
    /// its `.ogg` (OLX ships the game sounds as `.ogg`). `None` for other names.
    pub fn game_sound(&self, sim_name: &str) -> Option<&[u8]> {
        let path = sim_name.strip_prefix(GAME_SOUND_PREFIX)?;
        let file = path.rsplit('/').next().unwrap_or(path);
        let ogg = file.strip_suffix(".wav").map(|stem| format!("{stem}.ogg"));
        self.sound(file).or_else(|| self.sound(ogg.as_deref()?))
    }

    /// The `colours.cfg` value `key` (such as `RopeColour1`).
    pub fn colour(&self, key: &str) -> Option<Rgba> {
        self.colours.get(key).copied()
    }
}
