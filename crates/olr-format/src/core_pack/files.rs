//! The core pack's files. Other paths (`pack.ron`, ...) belong to
//! `olr-pack` and are ignored here.

use std::collections::BTreeMap;

use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::{CORE_FILE, CoreSource, SpriteSource, check, sound_file, sprite_file};
use crate::image_io::{self, IMAGES};
use crate::ron_io::{from_canonical, read_text, to_canonical, write_text};
use crate::{Colour, FormatError};

/// The largest file read (a PNG or a sound), as `olr-pack` reads.
const MAX_FILE_BYTES: u64 = 64 << 20;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteEntry {
    frames: (u32, u32),
    art_scale: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreFile {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    sprites: BTreeMap<String, SpriteEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sounds: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    colours: BTreeMap<String, Colour>,
}

fn write_bytes(sink: &mut dyn ContentSink, file: &str, bytes: &[u8]) -> Result<(), FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    sink.write(&VPath::new(file).map_err(|e| io(e.to_string()))?, bytes).map_err(|e| io(e.to_string()))
}

fn read_bytes(src: &dyn ContentSource, file: &str) -> Result<Vec<u8>, FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    let path = VPath::new(file).map_err(|e| io(e.to_string()))?;
    src.read_max(&path, MAX_FILE_BYTES).map_err(|e| io(e.to_string()))
}

pub fn write_core_files(sink: &mut dyn ContentSink, src: &CoreSource) -> Result<(), FormatError> {
    check(src)?;
    let file = CoreFile {
        notes: src.notes.clone(),
        sprites: src
            .sprites
            .iter()
            .map(|(id, s)| (id.clone(), SpriteEntry { frames: s.frames, art_scale: s.art_scale }))
            .collect(),
        sounds: src.sounds.keys().cloned().collect(),
        colours: src.colours.clone(),
    };
    write_text(sink, CORE_FILE, &to_canonical(CORE_FILE, &file)?)?;
    for (id, s) in &src.sprites {
        write_bytes(sink, &sprite_file(id), &image_io::encode_rgba(&s.image))?;
    }
    for (name, bytes) in &src.sounds {
        write_bytes(sink, &sound_file(name), bytes)?;
    }
    Ok(())
}

pub fn read_core_files(src: &dyn ContentSource) -> Result<CoreSource, FormatError> {
    let file: CoreFile = from_canonical(CORE_FILE, &read_text(src, CORE_FILE)?)?;
    let mut sprites = BTreeMap::new();
    for (id, e) in file.sprites {
        // Names are checked before they become paths.
        if !super::valid_name(&id) {
            return Err(FormatError::invalid(CORE_FILE, "sprites", format!("invalid sprite id {id:?}")));
        }
        let name = sprite_file(&id);
        let image = image_io::decode_rgba_within(&read_bytes(src, &name)?, IMAGES)
            .map_err(|message| FormatError::Io { file: name.clone(), message })?;
        sprites.insert(id, SpriteSource { image, frames: e.frames, art_scale: e.art_scale });
    }
    let mut sounds = BTreeMap::new();
    for name in file.sounds {
        if !super::valid_name(&name) {
            return Err(FormatError::invalid(CORE_FILE, "sounds", format!("invalid sound name {name:?}")));
        }
        let bytes = read_bytes(src, &sound_file(&name))?;
        if sounds.insert(name.clone(), bytes).is_some() {
            return Err(FormatError::invalid(CORE_FILE, "sounds", format!("{name:?} is listed twice")));
        }
    }
    let core = CoreSource { notes: file.notes, sprites, sounds, colours: file.colours };
    check(&core)?;
    Ok(core)
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_vfs::MemFs;

    #[test]
    fn files_round_trip_byte_stably() {
        let mut src = crate::core_pack::tests::sample();
        src.notes = "a note".into();
        let mut a = MemFs::new();
        write_core_files(&mut a, &src).unwrap();
        let names: Vec<String> = a.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(
            names,
            [
                "core.ron",
                "gfx/crosshair.png",
                "gfx/extra.png",
                "gfx/giblet-stains.png",
                "gfx/giblets.png",
                "gfx/spawn.png",
                "sounds/bump.ogg",
                "sounds/throw.ogg"
            ]
        );
        let back = read_core_files(&a).unwrap();
        assert_eq!(back, src);
        let mut b = MemFs::new();
        write_core_files(&mut b, &back).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn core_ron_has_no_some_and_no_struct_names() {
        let mut fs = MemFs::new();
        write_core_files(&mut fs, &crate::core_pack::tests::sample()).unwrap();
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "core.ron").unwrap().1.to_vec())
            .unwrap();
        assert!(
            !text.contains("Some(") && !text.contains("SpriteEntry") && !text.contains("CoreFile"),
            "{text}"
        );
        assert!(text.contains("frames: (2, 1)") && text.contains("\"RopeColour1\": \"#948800\""), "{text}");
        assert!(!text.contains("notes"), "{text}");
    }

    #[test]
    fn a_missing_sound_file_names_it() {
        let mut fs = MemFs::new();
        write_core_files(&mut fs, &crate::core_pack::tests::sample()).unwrap();
        let mut broken = MemFs::new();
        for (p, d) in fs.files().filter(|(p, _)| p.as_str() != "sounds/bump.ogg") {
            broken = broken.with(p.as_str(), d.to_vec());
        }
        let e = read_core_files(&broken).unwrap_err().to_string();
        assert!(e.contains("sounds/bump.ogg"), "{e}");
    }
}
