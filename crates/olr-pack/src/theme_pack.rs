//! Theme packs (format 5, `olr-format`'s `themes`): `theme.ron`,
//! `holes/<n>.png` with `holes/<n>.mask.png`, `stones/<n>.png`,
//! `misc/<n>.png` (numbered from 1), `front_tile.png`, `back_tile.png`, and
//! optionally `green_ball.png` with `green_ball.mask.png`.

use olr_data::manifest::{PackKind, PackManifest};
use olr_data::theme::Theme;
use olr_format::themes::{compile_theme, lift_theme, read_theme_files, write_theme_files};
use olr_vfs::{ContentSink, ContentSource};

use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const THEME_FILE: &str = "theme.ron";

#[derive(Clone, Debug, PartialEq)]
pub struct ThemePack {
    pub manifest: PackManifest,
    pub theme: Theme,
}

/// Writes the pack in format 5 (`olr-format`'s theme files: meaning masks
/// beside the stamps).
pub fn write_theme(sink: &mut dyn ContentSink, pack: &ThemePack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Theme {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Theme"));
    }
    let t = &pack.theme;
    let counts = (t.holes.len() as u32, t.stones.len() as u32, t.misc.len() as u32);
    if counts != (t.def.holes, t.def.stones, t.def.misc) {
        return Err(PackError::invalid(THEME_FILE, "image counts do not match theme.ron"));
    }
    let src = lift_theme(t).map_err(|e| PackError::invalid(THEME_FILE, e.to_string()))?;
    write_manifest(sink, &pack.manifest)?;
    write_theme_files(sink, &src)?;
    Ok(())
}

pub fn read_theme(src: &dyn ContentSource) -> Result<ThemePack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Theme {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Theme"));
    }
    let theme = compile_theme(&read_theme_files(src)?)?;
    Ok(ThemePack { manifest, theme })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use olr_data::image::{Rgba, RgbaImage};
    use olr_data::theme::ThemeDef;
    use olr_vfs::{MemFs, VPath};

    fn filled(c: Rgba) -> RgbaImage {
        let mut img = RgbaImage::new(2, 2);
        for y in 0..2 {
            for x in 0..2 {
                img.set(x, y, c);
            }
        }
        img
    }

    fn pack(green: bool) -> ThemePack {
        let mut hole = filled(Rgba::rgb(0, 0, 0));
        hole.set(1, 0, Rgba::rgb(255, 0, 255));
        ThemePack {
            manifest: manifest(PackKind::Theme),
            theme: Theme {
                def: ThemeDef { name: "Dirt".into(), holes: 2, stones: 1, misc: 0 },
                holes: vec![hole.clone(), hole],
                stones: vec![filled(Rgba::rgb(1, 2, 3))],
                misc: vec![],
                front_tile: filled(Rgba::rgb(4, 5, 6)),
                back_tile: filled(Rgba::rgb(7, 8, 9)),
                green_ball: green.then(|| {
                    let mut g = filled(Rgba::rgb(0, 255, 0));
                    g.set(0, 0, Rgba::rgb(255, 0, 255));
                    g
                }),
            },
        }
    }

    /// Format 5's layout: every stamp has a meaning mask beside its image.
    #[test]
    fn roundtrip_in_format_five_layout() {
        let has = |fs: &MemFs, f: &str| fs.exists(&VPath::new(f).unwrap());
        let pack = pack(false);
        let mut fs = MemFs::new();
        write_theme(&mut fs, &pack).unwrap();
        assert!(has(&fs, "holes/2.png") && has(&fs, "holes/2.mask.png"));
        assert!(!has(&fs, "green_ball.png") && !has(&fs, "green_ball.mask.png"));
        assert_eq!(read_theme(&fs).unwrap(), pack);
        let pack = pack_with_green();
        let mut fs = MemFs::new();
        write_theme(&mut fs, &pack).unwrap();
        assert!(has(&fs, "green_ball.png") && has(&fs, "green_ball.mask.png"));
        assert_eq!(read_theme(&fs).unwrap(), pack);
    }

    fn pack_with_green() -> ThemePack {
        pack(true)
    }

    #[test]
    fn a_count_mismatch_is_refused() {
        let mut p = pack(false);
        p.theme.def.holes = 3;
        assert!(write_theme(&mut MemFs::new(), &p).unwrap_err().to_string().contains("counts"));
    }
}
