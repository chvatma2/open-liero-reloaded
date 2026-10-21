//! Skin packs (format 5, over `olr-format`): `skin.ron`, `skin.png` (a grid
//! of frames), `tint.png` (greyscale: 0 keep, 255 player colour), an
//! optional `glow.png` and `left.png`, compiled to the 672 x 18 OLX sheet
//! or, for a double-resolution skin (issue #12), its 1344 x 36 double.

use olr_data::manifest::{PackKind, PackManifest};
use olr_data::skin::Skin;
use olr_format::skins::{compile_skin, lift_skin, read_skin_files, write_skin_files};
use olr_vfs::{ContentSink, ContentSource};

use crate::{MANIFEST, PackError, read_manifest, write_manifest};

const SKIN_FILE: &str = "skin.ron";

#[derive(Clone, Debug, PartialEq)]
pub struct SkinPack {
    pub manifest: PackManifest,
    pub skin: Skin,
}

/// Writes the pack in format 5 (`olr-format`'s skin files).
pub fn write_skin(sink: &mut dyn ContentSink, pack: &SkinPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Skin {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Skin"));
    }
    let src = lift_skin(&pack.skin).map_err(|e| PackError::invalid(SKIN_FILE, e.to_string()))?;
    write_manifest(sink, &pack.manifest)?;
    write_skin_files(sink, &src)?;
    Ok(())
}

pub fn read_skin(src: &dyn ContentSource) -> Result<SkinPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Skin {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Skin"));
    }
    let skin = compile_skin(&read_skin_files(src)?)?;
    Ok(SkinPack { manifest, skin })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use olr_data::image::{Rgba, RgbaImage};
    use olr_data::skin::{FrameLayout, MaskKind, SkinDef};
    use olr_vfs::{MemFs, VPath};

    fn pack() -> SkinPack {
        let mut image = RgbaImage::new(672, 18);
        let mut mask = vec![MaskKind::Transparent; 672 * 18];
        // Frame 0's window starts at x = 4.
        image.set(4, 0, Rgba::rgb(9, 8, 7));
        mask[4] = MaskKind::Keep;
        image.set(5, 0, Rgba::rgb(1, 2, 3));
        mask[5] = MaskKind::Colorize;
        SkinPack {
            manifest: manifest(PackKind::Skin),
            skin: Skin {
                def: SkinDef { name: "s".into(), layout: FrameLayout::OLX, light: None },
                image,
                mask,
                glow: vec![],
            },
        }
    }

    #[test]
    fn roundtrip_in_format_five_layout() {
        let pack = pack();
        let mut fs = MemFs::new();
        write_skin(&mut fs, &pack).unwrap();
        for f in ["skin.ron", "skin.png", "tint.png"] {
            assert!(fs.exists(&VPath::new(f).unwrap()), "{f}");
        }
        assert_eq!(read_skin(&fs).unwrap(), pack);
    }

    #[test]
    fn a_wrong_kind_is_refused() {
        let mut p = pack();
        p.manifest.kind = PackKind::Level;
        assert!(write_skin(&mut MemFs::new(), &p).is_err());
    }
}
