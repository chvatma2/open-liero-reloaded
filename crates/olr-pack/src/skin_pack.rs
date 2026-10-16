//! Skin packs: `skin.ron`, `skin.png` (RGBA sheet) and `mask.png`, an 8-bit
//! palette PNG with index 0 = keep (black), 1 = colorize (white),
//! 2 = transparent (magenta), matching OLX's mask colors.

use olr_data::manifest::{PackKind, PackManifest};
use olr_data::skin::{MaskKind, Skin, SkinDef};
use olr_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{decode_indexed, decode_rgba, encode_indexed, encode_rgba};
use crate::ron_io::{read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const SKIN_FILE: &str = "skin.ron";
pub const IMAGE_FILE: &str = "skin.png";
pub const MASK_FILE: &str = "mask.png";
const MASK_PALETTE: [u8; 9] = [0, 0, 0, 255, 255, 255, 255, 0, 255];

#[derive(Clone, Debug, PartialEq)]
pub struct SkinPack {
    pub manifest: PackManifest,
    pub skin: Skin,
}

fn mask_index(m: MaskKind) -> u8 {
    match m {
        MaskKind::Keep => 0,
        MaskKind::Colorize => 1,
        MaskKind::Transparent => 2,
    }
}

pub fn write_skin(sink: &mut dyn ContentSink, pack: &SkinPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Skin {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Skin"));
    }
    let s = &pack.skin;
    if s.mask.len() != s.image.width() as usize * s.image.height() as usize {
        return Err(PackError::invalid(MASK_FILE, "mask size does not match skin.png"));
    }
    write_manifest(sink, &pack.manifest)?;
    write_ron(sink, SKIN_FILE, &s.def)?;
    sink.write(&VPath::new(IMAGE_FILE)?, &encode_rgba(&s.image))?;
    let idx: Vec<u8> = s.mask.iter().map(|m| mask_index(*m)).collect();
    sink.write(
        &VPath::new(MASK_FILE)?,
        &encode_indexed(s.image.width(), s.image.height(), &idx, &MASK_PALETTE),
    )?;
    Ok(())
}

pub fn read_skin(src: &dyn ContentSource) -> Result<SkinPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Skin {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Skin"));
    }
    let def: SkinDef = read_ron(src, SKIN_FILE)?;
    let image = decode_rgba(&crate::ron_io::read_file(src, &VPath::new(IMAGE_FILE)?)?)
        .map_err(|message| PackError::Image { file: IMAGE_FILE.into(), message })?;
    let (w, h, idx) = decode_indexed(&crate::ron_io::read_file(src, &VPath::new(MASK_FILE)?)?)
        .map_err(|message| PackError::Image { file: MASK_FILE.into(), message })?;
    if (w, h) != (image.width(), image.height()) {
        return Err(PackError::invalid(MASK_FILE, "mask size does not match skin.png"));
    }
    let mask = idx
        .iter()
        .map(|v| match v {
            0 => Ok(MaskKind::Keep),
            1 => Ok(MaskKind::Colorize),
            2 => Ok(MaskKind::Transparent),
            v => Err(PackError::invalid(MASK_FILE, format!("unknown mask index {v}"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SkinPack { manifest, skin: Skin { def, image, mask } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use olr_data::image::{Rgba, RgbaImage};
    use olr_data::skin::FrameLayout;
    use olr_vfs::MemFs;

    #[test]
    fn roundtrip() {
        let mut image = RgbaImage::new(3, 1);
        image.set(1, 0, Rgba::rgb(9, 8, 7));
        let pack = SkinPack {
            manifest: manifest(PackKind::Skin),
            skin: Skin {
                def: SkinDef { name: "s".into(), layout: FrameLayout::OLX },
                image,
                mask: vec![MaskKind::Keep, MaskKind::Colorize, MaskKind::Transparent],
            },
        };
        let mut fs = MemFs::new();
        write_skin(&mut fs, &pack).unwrap();
        assert_eq!(read_skin(&fs).unwrap(), pack);
    }
}
