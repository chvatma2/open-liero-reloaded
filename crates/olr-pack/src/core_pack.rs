//! Core packs (format 5, over `olr-format`): `core.ron`, `gfx/<sprite>.png`
//! (RGBA with alpha; the importer keys them once, nothing keys at load)
//! and `sounds/<file>`, OLX's shared game assets.

use std::collections::BTreeMap;

use olr_data::core_pack::CoreDef;
use olr_data::image::RgbaImage;
use olr_data::manifest::{PackKind, PackManifest};
use olr_format::core_pack::{compile_core, lift_core, read_core_files_tolerant, write_core_files};
use olr_vfs::{ContentSink, ContentSource};

use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const CORE_FILE: &str = "core.ron";

#[derive(Clone, Debug, PartialEq)]
pub struct CorePack {
    pub manifest: PackManifest,
    /// Its lists name exactly the keys of `sprites` and `sounds`.
    pub def: CoreDef,
    /// Decoded sprites by name, such as `crosshair`.
    pub sprites: BTreeMap<String, RgbaImage>,
    /// Encoded sound files (Ogg Vorbis) by file name, such as `throw.ogg`.
    pub sounds: BTreeMap<String, Vec<u8>>,
}

/// Writes the pack in format 5 (`olr-format`'s core files). The sprites
/// are written as they are: they must already carry alpha.
pub fn write_core(sink: &mut dyn ContentSink, pack: &CorePack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Core {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Core"));
    }
    let def = &pack.def;
    if !def.sprites.iter().eq(pack.sprites.keys()) || !def.sounds.iter().eq(pack.sounds.keys()) {
        return Err(PackError::invalid(CORE_FILE, "sprite or sound lists do not match the files"));
    }
    let src = lift_core(def, &pack.sprites, &pack.sounds)
        .map_err(|e| PackError::invalid(CORE_FILE, e.to_string()))?;
    write_manifest(sink, &pack.manifest)?;
    write_core_files(sink, &src)?;
    Ok(())
}

/// Reads a core pack; any unreadable sprite or sound file is an error
/// (what `validate` reports). Games use [`read_core_tolerant`].
pub fn read_core(src: &dyn ContentSource) -> Result<CorePack, PackError> {
    let (pack, skipped) = read_core_tolerant(src)?;
    match skipped.into_iter().next() {
        None => Ok(pack),
        Some(e) => Err(e),
    }
}

/// Reads a core pack, leaving out each sprite that is missing or does not
/// decode and each sound file that is missing (they are returned, in a
/// stable order; the pack's lists name only what was read), so one bad file
/// costs only that asset. `giblets` and `giblet-stains` drop together. The
/// manifest and `core.ron` are still errors.
pub fn read_core_tolerant(src: &dyn ContentSource) -> Result<(CorePack, Vec<PackError>), PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Core {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Core"));
    }
    let (source, errors) = read_core_files_tolerant(src);
    let (def, sprites, sounds) = compile_core(&source?)?;
    Ok((CorePack { manifest, def, sprites, sounds }, errors.into_iter().map(PackError::from).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use olr_data::image::Rgba;
    use olr_vfs::{MemFs, VPath};

    pub(crate) fn core_pack() -> CorePack {
        let mut img = RgbaImage::new(2, 1);
        img.set(1, 0, Rgba::MAGENTA);
        CorePack {
            manifest: manifest(PackKind::Core),
            def: CoreDef {
                sprites: vec!["crosshair".into(), "hook".into()],
                sounds: vec!["bump.ogg".into(), "throw.ogg".into()],
                colours: [("RopeColour1".to_string(), Rgba::rgb(0xa0, 0x50, 0))].into_iter().collect(),
            },
            sprites: [("crosshair".to_string(), img.clone()), ("hook".to_string(), img)]
                .into_iter()
                .collect(),
            sounds: [
                ("bump.ogg".to_string(), b"OggS1".to_vec()),
                ("throw.ogg".to_string(), b"OggS".to_vec()),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn roundtrip() {
        let pack = core_pack();
        let mut fs = MemFs::new();
        write_core(&mut fs, &pack).unwrap();
        assert!(fs.exists(&VPath::new("gfx/hook.png").unwrap()));
        assert!(fs.exists(&VPath::new("sounds/throw.ogg").unwrap()));
        assert_eq!(read_core(&fs).unwrap(), pack);
    }

    #[test]
    fn lists_must_match_the_files_and_names_be_plain() {
        let mut pack = core_pack();
        pack.def.sprites.pop();
        assert!(write_core(&mut MemFs::new(), &pack).is_err());
        let mut pack = core_pack();
        let img = pack.sprites.remove("hook").unwrap();
        pack.sprites.insert("../hook".into(), img);
        pack.def.sprites[1] = "../hook".into();
        assert!(write_core(&mut MemFs::new(), &pack).is_err());
    }

    /// The pack's files without `gone`.
    fn without(fs: &MemFs, gone: &str) -> MemFs {
        let mut out = MemFs::new();
        for (p, bytes) in fs.files() {
            if p.as_str() != gone {
                out.write(p, bytes).unwrap();
            }
        }
        out
    }

    /// One undecodable sprite and one missing sound are left out with an
    /// error each; the rest of the pack is read.
    #[test]
    fn tolerant_reading_skips_only_the_bad_files() {
        let pack = core_pack();
        let mut fs = MemFs::new();
        write_core(&mut fs, &pack).unwrap();
        fs.write(&VPath::new("gfx/hook.png").unwrap(), b"not a png").unwrap();
        let fs = without(&fs, "sounds/bump.ogg");
        assert!(read_core(&fs).is_err());
        let (read, skipped) = read_core_tolerant(&fs).unwrap();
        assert_eq!(read.sprites.keys().collect::<Vec<_>>(), ["crosshair"]);
        assert_eq!(read.def.sprites, ["crosshair"]);
        assert_eq!(read.sounds.keys().collect::<Vec<_>>(), ["throw.ogg"]);
        assert_eq!(read.def.sounds, ["throw.ogg"]);
        assert_eq!(read.def.colours, pack.def.colours);
        let files: Vec<_> = skipped.iter().map(|e| e.to_string()).collect();
        assert_eq!(files.len(), 2, "{files:?}");
        assert!(files[0].contains("gfx/hook.png") && files[1].contains("bump.ogg"), "{files:?}");
    }

    #[test]
    fn the_files_are_in_format_five_layout() {
        let mut fs = MemFs::new();
        write_core(&mut fs, &core_pack()).unwrap();
        let paths: Vec<String> = fs.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(
            paths,
            [
                "core.ron",
                "gfx/crosshair.png",
                "gfx/hook.png",
                "pack.ron",
                "sounds/bump.ogg",
                "sounds/throw.ogg"
            ]
        );
    }
}
