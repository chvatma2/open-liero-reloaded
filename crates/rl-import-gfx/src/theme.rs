//! Level themes (`CMap::LoadTheme`, `src/common/CMap.cpp:308-370`):
//! five holes, `NumStones` stones and `NumMisc` misc images from `theme.txt`,
//! plus the front and back tiles, and the shared green-dirt mask
//! `data/gfx/greenball.png` (`CMap.cpp:365-367`).

use rl_data::manifest::{Origin, PackKind};
use rl_data::slug::slug;
use rl_data::theme::{Theme, ThemeDef};
use rl_pack::ThemePack;
use rl_vfs::{ContentSource, VPath, find_ci};

use crate::{GfxError, decode, manifest};

const HOLES: u32 = 5;
const GREEN_BALL: &str = "data/gfx/greenball.png";

#[derive(Debug)]
pub struct ImportedTheme {
    pub pack: ThemePack,
    pub warnings: Vec<String>,
}

/// Reads an integer from the `[General]` section of an OLX ini file.
fn ini_int(text: &str, key: &str) -> Option<u32> {
    let mut in_general = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_general = line.eq_ignore_ascii_case("[General]");
        } else if in_general
            && let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return v.trim().parse().ok();
        }
    }
    None
}

/// Imports the theme in directory `dir` (such as `data/themes/Dirt`).
pub fn import_theme(olx: &dyn ContentSource, dir: &VPath, origin: Origin) -> Result<ImportedTheme, GfxError> {
    let find = |name: &str| -> Result<VPath, GfxError> {
        let p = dir.join(name)?;
        find_ci(olx, &p).ok_or_else(|| GfxError::MissingFile(p.to_string()))
    };
    let ini_path = find("theme.txt")?;
    let ini_bytes = olx.read(&ini_path)?;
    let ini = String::from_utf8_lossy(&ini_bytes);
    let mut warnings = Vec::new();
    let mut count = |key: &str| {
        ini_int(&ini, key).unwrap_or_else(|| {
            warnings.push(format!("{ini_path}: {key} missing, using 0 like OLX"));
            0
        })
    };
    let (stones, misc) = (count("NumStones"), count("NumMisc"));
    let list = |prefix: &str, n: u32| -> Result<Vec<_>, GfxError> {
        (1..=n).map(|i| decode(olx, &find(&format!("{prefix}{i}.png"))?)).collect()
    };
    let name = dir.file_name().to_string();
    let theme = Theme {
        def: ThemeDef { name: name.clone(), holes: HOLES, stones, misc },
        holes: list("Hole", HOLES)?,
        stones: list("Stone", stones)?,
        misc: list("misc", misc)?,
        front_tile: decode(olx, &find("Fronttile.png")?)?,
        back_tile: decode(olx, &find("Backtile.png")?)?,
        green_ball: match find_ci(olx, &VPath::new(GREEN_BALL)?) {
            Some(p) => Some(decode(olx, &p)?),
            None => {
                warnings.push(format!("{GREEN_BALL} missing: green dirt will place nothing, like OLX"));
                None
            }
        },
    };
    let pack = ThemePack {
        manifest: manifest(format!("olx-theme-{}", slug(&name)), name, PackKind::Theme, origin),
        theme,
    };
    Ok(ImportedTheme { pack, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::image::RgbaImage;
    use rl_vfs::MemFs;

    #[test]
    fn ini_parsing() {
        let ini = "[Other]\nNumStones=9\n[General]\nNumStones = 8\r\nNumMisc=2\n";
        assert_eq!(ini_int(ini, "NumStones"), Some(8));
        assert_eq!(ini_int(ini, "nummisc"), Some(2));
        assert_eq!(ini_int(ini, "Missing"), None);
    }

    #[test]
    fn imports_with_case_insensitive_names() {
        let png = rl_pack::image_io::encode_rgba(&RgbaImage::new(2, 2));
        let mut fs = MemFs::new()
            .with("data/themes/Dirt/theme.txt", "[General]\nNumStones = 1\nNumMisc = 0\n")
            .with("data/gfx/GreenBall.png", png.clone());
        for f in ["hole1", "hole2", "hole3", "hole4", "hole5", "Stone1", "FrontTile", "BackTile"] {
            fs = fs.with(&format!("data/themes/Dirt/{f}.png"), png.clone());
        }
        let origin = Origin { source: "t".into(), path: "Dirt".into(), revision: "0".into() };
        let t = import_theme(&fs, &VPath::new("data/themes/Dirt").unwrap(), origin).unwrap();
        assert!(t.warnings.is_empty());
        assert_eq!(t.pack.manifest.id, "olx-theme-dirt");
        assert_eq!((t.pack.theme.holes.len(), t.pack.theme.stones.len()), (5, 1));
        assert!(t.pack.theme.green_ball.is_some());
    }

    #[test]
    fn a_missing_green_ball_is_a_warning() {
        let png = rl_pack::image_io::encode_rgba(&RgbaImage::new(2, 2));
        let mut fs =
            MemFs::new().with("data/themes/Dirt/theme.txt", "[General]\nNumStones = 0\nNumMisc = 0\n");
        for f in ["Hole1", "Hole2", "Hole3", "Hole4", "Hole5", "Fronttile", "Backtile"] {
            fs = fs.with(&format!("data/themes/Dirt/{f}.png"), png.clone());
        }
        let origin = Origin { source: "t".into(), path: "Dirt".into(), revision: "0".into() };
        let t = import_theme(&fs, &VPath::new("data/themes/Dirt").unwrap(), origin).unwrap();
        assert_eq!(t.pack.theme.green_ball, None);
        assert!(t.warnings[0].contains("greenball.png missing"), "{:?}", t.warnings);
    }

    #[test]
    fn missing_image_is_an_error() {
        let fs = MemFs::new().with("data/themes/X/theme.txt", "[General]\n");
        let origin = Origin { source: "t".into(), path: "X".into(), revision: "0".into() };
        let err = import_theme(&fs, &VPath::new("data/themes/X").unwrap(), origin).unwrap_err();
        assert!(matches!(err, GfxError::MissingFile(ref f) if f.ends_with("Hole1.png")), "{err}");
    }
}
