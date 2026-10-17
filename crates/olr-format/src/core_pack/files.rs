//! The core pack's files. Other paths (`pack.ron`, ...) belong to
//! `olr-pack` and are ignored here.

use std::collections::BTreeMap;

use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::{
    CORE_FILE, CoreSource, GIBLET_PART, GIBLETS, STAINS, STAINS_PART, SpriteSource, check, check_sprite,
    size_is, sound_file, sprite_file,
};
use crate::image_io::{self, IMAGES};
use crate::ron_io::{MAX_FILE_BYTES, from_canonical, read_text, to_canonical, write_text};
use crate::{Colour, FormatError};

/// One sprite's frame grid.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpriteEntry {
    /// Columns and rows of frames; they must divide the image.
    frames: (u32, u32),
    /// Art pixels per world pixel, 1 to `MAX_ART_SCALE` (8).
    art_scale: u32,
}

/// `core.ron`: OLX's shared game assets (`CORE_LAYOUT` lists OLX's sprites).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CoreFile {
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    /// Each sprite `gfx/<id>.png` by id, such as `crosshair`, at most
    /// `MAX_SPRITES` (256).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    sprites: BTreeMap<String, SpriteEntry>,
    /// The sound files `sounds/<file>`, such as `throw.ogg`, at most `MAX_SOUNDS` (1024).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sounds: Vec<String>,
    /// OLX's `colours.cfg` values by its key, such as `RopeColour1`, at most
    /// `MAX_COLOURS` (1024).
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

/// The paths the writer writes for `src`, without encoding anything.
pub fn file_paths(src: &CoreSource) -> Vec<String> {
    let mut out = vec![CORE_FILE.to_string()];
    out.extend(src.sprites.keys().map(|id| sprite_file(id)));
    out.extend(src.sounds.keys().map(|name| sound_file(name)));
    out
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

/// Reads `core.ron` and bounds its counts; the one place both readers share.
fn read_core_ron(src: &dyn ContentSource) -> Result<CoreFile, FormatError> {
    let file: CoreFile = from_canonical(CORE_FILE, &read_text(src, CORE_FILE)?)?;
    // The counts are checked before any file is read.
    for (name, n, max) in [
        ("sprites", file.sprites.len(), super::MAX_SPRITES),
        ("sounds", file.sounds.len(), super::MAX_SOUNDS),
        ("colours", file.colours.len(), super::MAX_COLOURS),
    ] {
        if n > max {
            return Err(FormatError::invalid(CORE_FILE, name, format!("more than {max} {name}")));
        }
    }
    Ok(file)
}

fn read_sprite(src: &dyn ContentSource, id: &str, e: &SpriteEntry) -> Result<SpriteSource, FormatError> {
    // Names are checked before they become paths.
    if !super::valid_name(id) {
        return Err(FormatError::invalid(CORE_FILE, "sprites", format!("invalid sprite id {id:?}")));
    }
    let name = sprite_file(id);
    let image = image_io::decode_rgba_within(&read_bytes(src, &name)?, IMAGES)
        .map_err(|message| FormatError::Io { file: name.clone(), message })?;
    Ok(SpriteSource { image, frames: e.frames, art_scale: e.art_scale })
}

/// The core pack's files, strictly: the tolerant read's result, and its
/// first error if it had any (the errors come in a stable order).
pub fn read_core_files(src: &dyn ContentSource) -> Result<CoreSource, FormatError> {
    let (core, errors) = read_core_files_tolerant(src);
    match errors.into_iter().next() {
        Some(e) => Err(e),
        None => core,
    }
}

/// Like [`read_core_files`], but a bad sprite or a missing sound is left
/// out and reported, so the game can run with what is there. `giblets` and
/// `giblet-stains` drop together (OLX draws them from one image). The
/// `Err` is only for a `core.ron` that cannot be read (or whose counts are
/// out of bounds); the errors come in a stable order.
pub fn read_core_files_tolerant(
    src: &dyn ContentSource,
) -> (Result<CoreSource, FormatError>, Vec<FormatError>) {
    let file = match read_core_ron(src) {
        Ok(f) => f,
        Err(e) => return (Err(e), Vec::new()),
    };
    let mut errors = Vec::new();
    let mut sprites = BTreeMap::new();
    for (id, e) in &file.sprites {
        match read_sprite(src, id, e).and_then(|s| check_sprite(id, &s).map(|()| s)) {
            Ok(s) => {
                sprites.insert(id.clone(), s);
            }
            Err(e) => errors.push(e),
        }
    }
    // The pair stands or falls together, and each part must be its size.
    let pair_ok = match (sprites.get(GIBLETS), sprites.get(STAINS)) {
        (None, None) => true,
        (Some(g), Some(st)) => size_is(&g.image, GIBLET_PART) && size_is(&st.image, STAINS_PART),
        _ => false,
    };
    if !pair_ok {
        let (had_g, had_s) = (sprites.remove(GIBLETS).is_some(), sprites.remove(STAINS).is_some());
        if had_g && had_s {
            errors.push(FormatError::invalid(
                &sprite_file(GIBLETS),
                "",
                format!(
                    "{GIBLETS} and {STAINS} must be {}x{} and {}x{}",
                    GIBLET_PART.0, GIBLET_PART.1, STAINS_PART.0, STAINS_PART.1
                ),
            ));
        } else if had_g || had_s {
            let (here, gone) = if had_g { (GIBLETS, STAINS) } else { (STAINS, GIBLETS) };
            errors.push(FormatError::invalid(
                &sprite_file(here),
                "",
                format!("{here} needs {gone} beside it, so it is left out"),
            ));
        }
    }
    let mut sounds = BTreeMap::new();
    for name in file.sounds {
        if !super::valid_name(&name) {
            errors.push(FormatError::invalid(CORE_FILE, "sounds", format!("invalid sound name {name:?}")));
            continue;
        }
        match read_bytes(src, &sound_file(&name)) {
            Ok(bytes) => {
                if sounds.insert(name.clone(), bytes).is_some() {
                    errors.push(FormatError::invalid(
                        CORE_FILE,
                        "sounds",
                        format!("{name:?} is listed twice"),
                    ));
                }
            }
            Err(e) => errors.push(e),
        }
    }
    let mut colours = file.colours;
    colours.retain(|key, _| {
        let keep = !key.is_empty();
        if !keep {
            errors.push(FormatError::invalid(CORE_FILE, "colours", "an empty colour key"));
        }
        keep
    });
    let core = CoreSource { notes: file.notes, sprites, sounds, colours };
    match check(&core) {
        Ok(()) => (Ok(core), errors),
        Err(e) => (Err(e), errors),
    }
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
    fn a_thousand_sprites_are_refused_before_any_file() {
        let list: Vec<String> =
            (0..300).map(|i| format!("\"s{i}\": (frames: (1, 1), art_scale: 1)")).collect();
        let fs = MemFs::new().with("core.ron", format!("(sprites: {{{}}})\n", list.join(", ")).into_bytes());
        let e = read_core_files(&fs).unwrap_err().to_string();
        assert!(e.contains("core.ron") && e.contains("256 sprites"), "{e}");
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

    fn sample_fs() -> MemFs {
        let mut fs = MemFs::new();
        write_core_files(&mut fs, &crate::core_pack::tests::sample()).unwrap();
        fs
    }

    fn without(fs: &MemFs, gone: &[&str]) -> MemFs {
        let mut out = MemFs::new();
        for (p, d) in fs.files().filter(|(p, _)| !gone.contains(&p.as_str())) {
            out = out.with(p.as_str(), d.to_vec());
        }
        out
    }

    #[test]
    fn the_tolerant_read_of_a_good_core_is_the_strict_read() {
        let fs = sample_fs();
        let (core, errors) = read_core_files_tolerant(&fs);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(core.unwrap(), read_core_files(&fs).unwrap());
    }

    #[test]
    fn the_tolerant_read_leaves_out_a_bad_sprite_and_a_missing_sound() {
        let fs = without(&sample_fs(), &["sounds/bump.ogg"]).with("gfx/spawn.png", b"not a png".to_vec());
        let (core, errors) = read_core_files_tolerant(&fs);
        let core = core.unwrap();
        assert!(!core.sprites.contains_key("spawn") && core.sprites.contains_key("crosshair"));
        assert_eq!(core.sounds.keys().collect::<Vec<_>>(), ["throw.ogg"]);
        let text: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert_eq!(text.len(), 2, "{text:?}");
        assert!(text.iter().any(|t| t.contains("gfx/spawn.png")), "{text:?}");
        assert!(text.iter().any(|t| t.contains("sounds/bump.ogg")), "{text:?}");
    }

    #[test]
    fn the_tolerant_read_drops_the_giblets_pair_together() {
        for gone in ["gfx/giblets.png", "gfx/giblet-stains.png"] {
            let (core, errors) = read_core_files_tolerant(&without(&sample_fs(), &[gone]));
            let core = core.unwrap();
            assert!(!core.sprites.contains_key("giblets") && !core.sprites.contains_key("giblet-stains"));
            assert!(core.sprites.contains_key("crosshair"));
            assert_eq!(errors.len(), 2, "{errors:?}");
            // The result compiles.
            super::super::compile_core(&core).unwrap();
        }
    }

    #[test]
    fn the_tolerant_read_of_an_unreadable_core_ron_is_the_err() {
        let (core, errors) = read_core_files_tolerant(&MemFs::new());
        assert!(core.is_err() && errors.is_empty());
        let fs = MemFs::new().with("core.ron", b"(sprites: ".to_vec());
        assert!(matches!(read_core_files_tolerant(&fs).0, Err(FormatError::Parse { .. })));
    }

    #[test]
    fn the_tolerant_read_drops_wrong_size_giblets_together() {
        // Divisible by the sample's 4x1 frames, but not the giblets' size.
        let small = image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 2));
        let fs = sample_fs().with("gfx/giblets.png", small);
        let (core, errors) = read_core_files_tolerant(&fs);
        let core = core.unwrap();
        assert!(!core.sprites.contains_key("giblets") && !core.sprites.contains_key("giblet-stains"));
        assert!(core.sprites.contains_key("crosshair"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].to_string().contains("must be"), "{}", errors[0]);
        super::super::compile_core(&core).unwrap();
        // The strict read reports it.
        assert!(read_core_files(&fs).is_err());
    }

    #[test]
    fn the_errors_come_in_order_sprites_then_sounds_and_the_strict_read_gives_the_first() {
        let fs = without(&sample_fs(), &["sounds/bump.ogg", "gfx/crosshair.png"])
            .with("gfx/spawn.png", b"not a png".to_vec());
        let (_, errors) = read_core_files_tolerant(&fs);
        let text: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert_eq!(text.len(), 3, "{text:?}");
        // Sprites by id (crosshair, spawn), then the sounds.
        assert!(text[0].contains("gfx/crosshair.png"), "{text:?}");
        assert!(text[1].contains("gfx/spawn.png"), "{text:?}");
        assert!(text[2].contains("sounds/bump.ogg"), "{text:?}");
        assert_eq!(read_core_files(&fs).unwrap_err().to_string(), text[0]);
    }
}
