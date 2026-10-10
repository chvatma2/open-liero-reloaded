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

pub fn read_core(src: &dyn ContentSource) -> Result<CorePack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Core {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Core"));
    }
    let def: CoreDef = read_ron(src, CORE_FILE)?;
    let mut sprites = BTreeMap::new();
    for name in &def.sprites {
        check_name("sprite", name)?;
        let file = sprite_path(name);
        let img = decode_rgba(&src.read(&VPath::new(&file)?)?)
            .map_err(|message| PackError::Image { file: file.clone(), message })?;
        if sprites.insert(name.clone(), img).is_some() {
            return Err(PackError::invalid(CORE_FILE, format!("sprite {name:?} is listed twice")));
        }
    }
    let mut sounds = BTreeMap::new();
    for name in &def.sounds {
        check_name("sound", name)?;
        let bytes = src.read(&VPath::new(&sound_path(name))?)?;
        if sounds.insert(name.clone(), bytes).is_some() {
            return Err(PackError::invalid(CORE_FILE, format!("sound {name:?} is listed twice")));
        }
    }
    Ok(CorePack { manifest, def, sprites, sounds })
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
}
