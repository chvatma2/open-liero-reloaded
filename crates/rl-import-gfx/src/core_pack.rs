//! The core pack (spec §19 E9): OLX's shared game sprites (`data/gfx`), game
//! sounds (`data/sounds`) and frontend colours (`data/frontend/colours.cfg`,
//! read like `DeprecatedGUI::InitializeColors`, `src/client/DeprecatedGUI/Graphics.cpp:134-321`).

use std::collections::BTreeMap;

use rl_data::core_pack::CoreDef;
use rl_data::image::Rgba;
use rl_data::manifest::{Origin, PackKind};
use rl_pack::CorePack;
use rl_vfs::{ContentSource, VPath, find_ci};

use crate::{GfxError, decode, manifest};

/// The core pack's id.
pub const CORE_PACK_ID: &str = "olx-core";

#[derive(Debug)]
pub struct ImportedCore {
    pub pack: CorePack,
    pub warnings: Vec<String>,
}

/// `StrToCol` (`src/common/StringUtils.cpp:409-447`) for the forms
/// `colours.cfg` uses: `#rrggbb` (`HexToCol`) and `r,g,b` (each clamped to
/// 255, alpha opaque). A trailing `;` is ignored. Other forms are `None`.
fn parse_colour(v: &str) -> Option<Rgba> {
    let v = v.trim();
    let v = v.strip_suffix(';').unwrap_or(v).trim();
    if let Some(hex) = v.strip_prefix('#') {
        let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        return match hex.len() {
            6 => Some(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?)),
            _ => None,
        };
    }
    let parts: Vec<&str> = v.split(',').map(str::trim).collect();
    if parts.len() == 3 {
        let c = |s: &str| s.parse::<i64>().ok().map(|n| n.clamp(0, 255) as u8);
        return Some(Rgba::rgb(c(parts[0])?, c(parts[1])?, c(parts[2])?));
    }
    None
}

/// The `[Colours]` section of an OLX ini file: every `Key = value` line whose
/// value is a colour. Comments (`#` at the start of a line) are skipped.
fn parse_colours(text: &str, file: &str, warnings: &mut Vec<String>) -> BTreeMap<String, Rgba> {
    let mut out = BTreeMap::new();
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line.eq_ignore_ascii_case("[Colours]");
        } else if in_section
            && !line.starts_with('#')
            && let Some((k, v)) = line.split_once('=')
        {
            match parse_colour(v) {
                Some(c) => {
                    out.insert(k.trim().to_string(), c);
                }
                None => warnings.push(format!("{file}: {}: unreadable colour {:?}", k.trim(), v.trim())),
            }
        }
    }
    out
}

/// Imports `files` (paths in the game directory, such as
/// `data/gfx/crosshair.png`) into one core pack: each `.png` becomes the
/// sprite named by its lowercase stem, each `.ogg` the sound named by its
/// lowercase file name, and `colours.cfg` the colours. A missing or
/// unreadable file is a warning (the game draws a placeholder), so the rest
/// still import.
pub fn import_core(
    olx: &dyn ContentSource,
    files: &[VPath],
    origin: Origin,
) -> Result<ImportedCore, GfxError> {
    let mut warnings = Vec::new();
    let (mut sprites, mut sounds, mut colours) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    for file in files {
        let Some(path) = find_ci(olx, file) else {
            warnings.push(format!("{file} missing: the game uses a placeholder"));
            continue;
        };
        let name = path.file_name().to_ascii_lowercase();
        if let Some(stem) = name.strip_suffix(".png") {
            match decode(olx, &path) {
                Ok(img) if !sprites.contains_key(stem) => {
                    sprites.insert(stem.to_string(), img);
                }
                Ok(_) => warnings.push(format!("{path}: a second sprite {stem:?}, skipped")),
                Err(e) => warnings.push(format!("{e}: skipped")),
            }
        } else if name.ends_with(".ogg") {
            sounds.entry(name).or_insert(olx.read(&path)?);
        } else if name == "colours.cfg" {
            let text = String::from_utf8_lossy(&olx.read(&path)?).into_owned();
            colours.extend(parse_colours(&text, path.as_str(), &mut warnings));
        } else {
            warnings.push(format!("{path}: not a sprite, sound or colours.cfg, skipped"));
        }
    }
    let def = CoreDef {
        sprites: sprites.keys().cloned().collect(),
        sounds: sounds.keys().cloned().collect(),
        colours,
    };
    let manifest = manifest(CORE_PACK_ID.into(), "OpenLieroX core".into(), PackKind::Core, origin);
    Ok(ImportedCore { pack: CorePack { manifest, def, sprites, sounds }, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::image::RgbaImage;
    use rl_vfs::MemFs;

    fn origin() -> Origin {
        Origin { source: "t".into(), path: "data".into(), revision: "0".into() }
    }

    #[test]
    fn colours_parse_like_str_to_col() {
        assert_eq!(parse_colour(" #A05000 "), Some(Rgba::rgb(0xa0, 0x50, 0)));
        assert_eq!(parse_colour("#c86400;"), Some(Rgba::rgb(0xc8, 0x64, 0)));
        assert_eq!(parse_colour("10, 300, -4"), Some(Rgba::rgb(10, 255, 0)));
        assert_eq!(parse_colour("#12345"), None);
        assert_eq!(parse_colour("red"), None);
        let mut w = Vec::new();
        let text = "[Other]\nX = #000000\n[Colours]\n# Rope\nRopeColour1 = #A05000\nBad = what\n";
        let c = parse_colours(text, "colours.cfg", &mut w);
        assert_eq!(
            c.into_iter().collect::<Vec<_>>(),
            [("RopeColour1".to_string(), Rgba::rgb(0xa0, 0x50, 0))]
        );
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn imports_sprites_sounds_and_colours() {
        let png = rl_pack::image_io::encode_rgba(&RgbaImage::new(6, 6));
        let fs = MemFs::new()
            .with("data/gfx/Hook.png", png)
            .with("data/gfx/broken.png", "not a png")
            .with("data/sounds/throw.ogg", "OggS")
            .with("data/frontend/colours.cfg", "[Colours]\nLaserSightColour1 = #BE0000\n");
        let files =
            ["data/gfx/hook.png", "data/gfx/broken.png", "data/gfx/none.png", "data/sounds/throw.ogg"]
                .into_iter()
                .chain(["data/frontend/colours.cfg"])
                .map(|f| VPath::new(f).unwrap())
                .collect::<Vec<_>>();
        let c = import_core(&fs, &files, origin()).unwrap();
        assert_eq!(c.pack.manifest.id, CORE_PACK_ID);
        assert_eq!(c.pack.def.sprites, ["hook"]);
        assert_eq!(c.pack.def.sounds, ["throw.ogg"]);
        assert_eq!(c.pack.sounds["throw.ogg"], b"OggS");
        assert_eq!(c.pack.def.colours["LaserSightColour1"], Rgba::rgb(0xbe, 0, 0));
        assert_eq!(c.warnings.len(), 2, "the broken and the missing sprite: {:?}", c.warnings);
        let mut out = MemFs::new();
        rl_pack::write_core(&mut out, &c.pack).unwrap();
        assert_eq!(rl_pack::read_core(&out).unwrap(), c.pack);
    }
}
