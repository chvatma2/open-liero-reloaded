//! Core packs (spec §19 E9): `core.ron`, `gfx/<sprite>.png` and
//! `sounds/<file>`, OLX's shared game assets.

use std::collections::BTreeMap;

use rl_data::core_pack::CoreDef;
use rl_data::image::RgbaImage;
use rl_data::manifest::{PackKind, PackManifest};
use rl_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{decode_rgba, encode_rgba};
use crate::ron_io::{read_ron, write_ron};
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

/// A sprite or sound name: one lowercase file name, as the importer makes them.
fn check_name(kind: &str, name: &str) -> Result<(), PackError> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b));
    if ok { Ok(()) } else { Err(PackError::invalid(CORE_FILE, format!("invalid {kind} name {name:?}"))) }
}

fn sprite_path(name: &str) -> String {
    format!("gfx/{name}.png")
}

fn sound_path(name: &str) -> String {
    format!("sounds/{name}")
}

pub fn write_core(sink: &mut dyn ContentSink, pack: &CorePack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Core {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Core"));
    }
    let def = &pack.def;
    if !def.sprites.iter().eq(pack.sprites.keys()) || !def.sounds.iter().eq(pack.sounds.keys()) {
        return Err(PackError::invalid(CORE_FILE, "sprite or sound lists do not match the files"));
    }
    write_manifest(sink, &pack.manifest)?;
    write_ron(sink, CORE_FILE, def)?;
    for (name, img) in &pack.sprites {
        check_name("sprite", name)?;
        sink.write(&VPath::new(&sprite_path(name))?, &encode_rgba(img))?;
    }
    for (name, bytes) in &pack.sounds {
        check_name("sound", name)?;
        sink.write(&VPath::new(&sound_path(name))?, bytes)?;
    }
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
/// decode and each sound file that is missing (they are returned, in list
/// order; the pack's lists name only what was read), so one bad file costs
/// only that asset. The manifest, `core.ron`, invalid names and duplicates
/// are still errors.
pub fn read_core_tolerant(src: &dyn ContentSource) -> Result<(CorePack, Vec<PackError>), PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Core {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Core"));
    }
    let mut def: CoreDef = read_ron(src, CORE_FILE)?;
    let mut skipped = Vec::new();
    let mut sprites = BTreeMap::new();
    for name in &def.sprites {
        check_name("sprite", name)?;
        if sprites.contains_key(name) {
            return Err(PackError::invalid(CORE_FILE, format!("sprite {name:?} is listed twice")));
        }
        let file = sprite_path(name);
        let img = src
            .read(&VPath::new(&file)?)
            .map_err(PackError::from)
            .and_then(|bytes| decode_rgba(&bytes).map_err(|message| PackError::Image { file, message }));
        match img {
            Ok(img) => {
                sprites.insert(name.clone(), img);
            }
            Err(e) => skipped.push(e),
        }
    }
    let mut sounds = BTreeMap::new();
    for name in &def.sounds {
        check_name("sound", name)?;
        if sounds.contains_key(name) {
            return Err(PackError::invalid(CORE_FILE, format!("sound {name:?} is listed twice")));
        }
        match src.read(&VPath::new(&sound_path(name))?) {
            Ok(bytes) => {
                sounds.insert(name.clone(), bytes);
            }
            Err(e) => skipped.push(e.into()),
        }
    }
    def.sprites.retain(|n| sprites.contains_key(n));
    def.sounds.retain(|n| sounds.contains_key(n));
    Ok((CorePack { manifest, def, sprites, sounds }, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use rl_data::image::Rgba;
    use rl_vfs::MemFs;

    pub(crate) fn core_pack() -> CorePack {
        let mut img = RgbaImage::new(2, 1);
        img.set(1, 0, Rgba::MAGENTA);
        CorePack {
            manifest: manifest(PackKind::Core),
            def: CoreDef {
                sprites: vec!["crosshair".into(), "hook".into()],
                sounds: vec!["throw.ogg".into()],
                colours: [("RopeColour1".to_string(), Rgba::rgb(0xa0, 0x50, 0))].into_iter().collect(),
            },
            sprites: [("crosshair".to_string(), img.clone()), ("hook".to_string(), img)]
                .into_iter()
                .collect(),
            sounds: [("throw.ogg".to_string(), b"OggS".to_vec())].into_iter().collect(),
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
        // A listed file that is missing is an error when reading.
        let pack = core_pack();
        let mut fs = MemFs::new();
        write_core(&mut fs, &pack).unwrap();
        let mut def = pack.def.clone();
        def.sounds.push("bump.ogg".into());
        write_ron(&mut fs, CORE_FILE, &def).unwrap();
        assert!(read_core(&fs).is_err());
    }

    /// One undecodable sprite and one missing sound are left out with an
    /// error each; the rest of the pack is read.
    #[test]
    fn tolerant_reading_skips_only_the_bad_files() {
        let pack = core_pack();
        let mut fs = MemFs::new();
        write_core(&mut fs, &pack).unwrap();
        fs.write(&VPath::new("gfx/hook.png").unwrap(), b"not a png").unwrap();
        let mut def = pack.def.clone();
        def.sounds.push("bump.ogg".into());
        write_ron(&mut fs, CORE_FILE, &def).unwrap();
        let (read, skipped) = read_core_tolerant(&fs).unwrap();
        assert_eq!(read.sprites.keys().collect::<Vec<_>>(), ["crosshair"]);
        assert_eq!(read.def.sprites, ["crosshair"]);
        assert_eq!(read.sounds.keys().collect::<Vec<_>>(), ["throw.ogg"]);
        assert_eq!(read.def.sounds, ["throw.ogg"]);
        assert_eq!(read.def.colours, pack.def.colours);
        let files: Vec<_> = skipped.iter().map(|e| e.to_string()).collect();
        assert_eq!(files.len(), 2, "{files:?}");
        assert!(files[0].contains("gfx/hook.png") && files[1].contains("bump.ogg"), "{files:?}");
        // The strict reader refuses the same pack.
        assert!(read_core(&fs).is_err());
    }
}
