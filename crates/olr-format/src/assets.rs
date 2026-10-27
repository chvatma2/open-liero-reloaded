//! An asset (issue #9): a ready-made piece of a level saved once and
//! dropped into any level by the editor's Assets tab or `olr-cli level
//! copy add` (issue #75). It is a folder, `<asset-id>/`, holding `asset.ron` (its name,
//! the art scale it was made at, its size, its elements and folders, the
//! glow groups and surface materials they name, and (issue #71) its
//! states, exposed properties, sounds and health), the elements' files
//! (`images/<source>`, `layers/<id>.png`, `layers/<id>.material.png`,
//! `layers/<id>.back.png`, `masks/<id>.png`, `glow/<id>.png`,
//! `surface/<id>.png`, `surface/<id>.back.png`: a level composition's
//! `editor/` files, without `editor/`), its sounds (`sounds/<file>`, WAV or
//! Ogg, each at most 2 MiB), an optional default script (`script.rhai`,
//! UTF-8 Rhai, at most 64 KiB, read when present; not run yet) and
//! `thumb.png`, the asset drawn lit under a dark preview look. Elements
//! are a level's composition elements (see the level page), placed
//! relative to the asset's top-left corner; a library asset holds no
//! raster and no distant layer. A drawn layer owned by states stacks
//! their pictures in its own files: with its box `w` × `h` world pixels
//! at art scale `s` and owned by n states (in `states` order),
//! `layers/<id>.png` is `w·s` × `h·s·(1 + n)`, rows `0` to `h·s` the
//! starting state's picture and rows `k·h·s` to `(k + 1)·h·s` the k-th
//! owner's; `layers/<id>.material.png` (`w` × `h·(1 + n)`),
//! `layers/<id>.back.png` and `glow/<id>.png` stack the same way, while
//! surface maps and image masks are the starting state's in every state.
//! An asset without states, exposed properties, sounds, health or script
//! is written exactly as before them. A level places an asset as
//! copies of it: it holds its own copy of the folder (`editor/<key>/`,
//! issue #73), naming where it came from, never the library's folder
//! itself. An asset folder is untrusted
//! content: reading checks it with the level format's limits.

use std::collections::{BTreeMap, BTreeSet};

use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

mod states;

pub use states::*;

use crate::FormatError;
use crate::image_io;
use crate::levels::{
    CompositionSource, EDITOR_DIR, Element, ElementKind, Folder, MAX_ART_SCALE, MAX_ELEMENTS, MAX_FOLDERS,
    MAX_SIDE, MAX_THUMB_BYTES, MAX_THUMB_SIDE, check_glow_groups, check_stacked_composition,
    check_surface_maps, check_surfaces, decode_surface_map, element_files, surface_back_file, surface_file,
    surface_map_size,
};
use crate::look::{GlowGroupSource, LookSource};
use crate::materials::MaterialSource;
use crate::ron_io::{MAX_FILE_BYTES, from_canonical, read_text, to_canonical, write_text};

/// An asset's description in its folder.
pub const ASSET_FILE: &str = "asset.ron";
/// An asset's thumbnail in its folder.
pub const ASSET_THUMB_FILE: &str = "thumb.png";
/// The longest asset name, in characters.
pub const MAX_ASSET_NAME: usize = 64;
/// The most bytes an asset folder's files hold together (its sounds and
/// script included).
pub const MAX_ASSET_BYTES: u64 = 256 << 20;
/// An asset's sounds' folder.
pub const SOUNDS_DIR: &str = "sounds";
/// An asset's default script in its folder.
pub const SCRIPT_FILE: &str = "script.rhai";

/// `asset.ron`: what the asset is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetFile {
    /// Shown in the editor's Assets tab: 1 to `MAX_ASSET_NAME` (64)
    /// characters, no control characters.
    pub name: String,
    /// Art pixels per world pixel the asset was made at, 1 to
    /// `MAX_ART_SCALE` (8); placing it in a level of another art scale
    /// resamples it, as a paste does. Default 1.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub art_scale: u32,
    /// The asset's width and height, world pixels, each 1 to `MAX_SIDE`
    /// (4096): where it lands when placed, its top-left at the point.
    pub size: (u32, u32),
    /// Its elements, bottom to top, 1 to `MAX_ELEMENTS` (256), each placed
    /// relative to the asset's top-left (a drawn layer without a size is
    /// the asset's size); images and drawn layers only (a level's own
    /// asset may also hold a raster and distant layers).
    pub elements: Vec<Element>,
    /// Its layer folders (issue #59), with a level composition's folder
    /// rules (issue #71). Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<Folder>,
    /// The glow groups its elements name, each named by one at least.
    /// Placed in a level, a group the level has (by id) is joined, its own
    /// settings kept; a missing one is added with these. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub glow_groups: Vec<GlowGroupSource>,
    /// The surface materials its elements name (issue #38), at most 255,
    /// ids unique, each named by one at least (a `surface`, a
    /// `back_surface` or a painted map's value: `v` is `materials[v - 1]`
    /// here, as in a level). Placed in a level, a material the level has
    /// (by id) is joined, its own settings kept, and the painted maps
    /// follow its index there; a missing one is added with these. Default
    /// none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<MaterialSource>,
    /// Its named looks (issue #71), 1 to `MAX_STATES` (32), one of them the
    /// starting state. Default one state, `Normal`, owning and setting
    /// nothing; written `[(name: "Normal")]`.
    #[serde(default = "normal_states", skip_serializing_if = "is_normal_states")]
    pub states: Vec<StateSource>,
    /// The starting state's name. Default none: the first state (left out
    /// when it names the first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// The settable values given a name scripts use (issue #71), at most
    /// `MAX_EXPOSED` (64). The asset's states need no entry: every asset
    /// can be put in any of its states by name. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exposed: Vec<ExposedSource>,
    /// The files in `sounds/` (issue #71), at most `MAX_ASSET_SOUNDS` (16),
    /// each `[a-z0-9_-]` 1 to 32 bytes then `.wav` or `.ogg`, unique case
    /// aside. A sound no state names may be played by a script. Default
    /// none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<String>,
    /// The asset can be destroyed (issue #71). Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthSource>,
}

impl AssetFile {
    /// An asset of `elements` and nothing else: art scale 1, no folders,
    /// groups or materials, one state, nothing exposed, no sounds, no
    /// health.
    pub fn new(name: &str, size: (u32, u32), elements: Vec<Element>) -> AssetFile {
        AssetFile {
            name: name.into(),
            art_scale: 1,
            size,
            elements,
            folders: Vec::new(),
            glow_groups: Vec::new(),
            materials: Vec::new(),
            states: normal_states(),
            start: None,
            exposed: Vec::new(),
            sounds: Vec::new(),
            health: None,
        }
    }

    /// Whether it has more than one state, exposed values, sounds or
    /// health: what a stateless asset of the 0.3 format lacks.
    pub fn has_states(&self) -> bool {
        self.states.len() > 1 || !self.exposed.is_empty() || !self.sounds.is_empty() || self.health.is_some()
    }
}

fn one() -> u32 {
    1
}

fn is_one(v: &u32) -> bool {
    *v == 1
}

/// An asset folder in memory: `asset.ron`, its elements' files by path in
/// the folder (`layers/<id>.png`, ...), its thumbnail, its sounds by name
/// (`sounds/<name>`) and its default script.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetSource {
    pub file: AssetFile,
    pub files: BTreeMap<String, Vec<u8>>,
    pub thumb: Option<Vec<u8>>,
    pub sounds: BTreeMap<String, Vec<u8>>,
    pub script: Option<String>,
}

impl AssetSource {
    /// `file` with its elements' `files`, no thumbnail, sounds or script.
    pub fn new(file: AssetFile, files: BTreeMap<String, Vec<u8>>) -> AssetSource {
        AssetSource { file, files, thumb: None, sounds: BTreeMap::new(), script: None }
    }
}

/// An asset folder's name in a library (`content/assets/<id>`, the user's
/// `<data>/assets/<id>`): 1 to 64 bytes of `[a-z0-9_-]`, not starting with
/// `-` or `_` (portable everywhere, never hidden). A level's copy of a
/// library asset names its folder by it (issue #73).
pub fn valid_asset_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with(['-', '_'])
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// A sound's path in an asset folder: `sounds/<name>`.
pub fn sound_file(name: &str) -> String {
    format!("{SOUNDS_DIR}/{name}")
}

/// The files element `e` uses, by path in an asset folder (a composition's
/// `editor/` files without `editor/`).
pub fn asset_files(e: &Element) -> Vec<String> {
    element_files(e).into_iter().map(|f| strip_editor(&f).to_string()).collect()
}

fn strip_editor(path: &str) -> &str {
    path.strip_prefix(EDITOR_DIR).and_then(|p| p.strip_prefix('/')).unwrap_or(path)
}

/// `src` as a level composition (its files under `editor/`), to check and
/// decode it as one of a level of the asset's size.
pub fn as_composition(src: &AssetSource) -> CompositionSource {
    CompositionSource {
        composition: crate::levels::Composition {
            elements: src.file.elements.clone(),
            folders: src.file.folders.clone(),
        },
        files: src.files.iter().map(|(k, v)| (format!("{EDITOR_DIR}/{k}"), v.clone())).collect(),
    }
}

/// A composition's error said of the asset folder: `asset.ron` for
/// `editor/composition.ron`, the files without `editor/`.
fn of_asset(e: FormatError) -> FormatError {
    let file = |f: &str| {
        if f == crate::levels::COMPOSITION_FILE {
            ASSET_FILE.to_string()
        } else {
            strip_editor(f).to_string()
        }
    };
    let text = |m: &str| {
        m.replace(crate::levels::COMPOSITION_FILE, ASSET_FILE)
            .replace("editor/", "")
            .replace("level.ron's look", ASSET_FILE)
            .replace("the look has", "the asset has")
    };
    match e {
        FormatError::Parse { file: f, line, col, message } => {
            FormatError::Parse { file: file(&f), line, col, message: text(&message) }
        }
        FormatError::Invalid { file: f, path, message } => {
            FormatError::Invalid { file: file(&f), path, message: text(&message) }
        }
        FormatError::Io { file: f, message } => FormatError::Io { file: file(&f), message: text(&message) },
    }
}

fn invalid(path: &str, message: impl Into<String>) -> FormatError {
    FormatError::invalid(ASSET_FILE, path, message)
}

fn io(file: &str, message: String) -> FormatError {
    FormatError::Io { file: file.into(), message }
}

/// The checks that make `asset.ron`'s file paths safe to read or write
/// and bound the work: the counts (elements, folders, states, exposed
/// entries, sounds), each id a valid one, each image's source one portable
/// file name, each sound's name.
fn check_names(file: &AssetFile) -> Result<(), FormatError> {
    let n = file.elements.len();
    if n == 0 || n > MAX_ELEMENTS {
        return Err(invalid("elements", format!("{n} elements: an asset holds 1 to {MAX_ELEMENTS}")));
    }
    if file.folders.len() > MAX_FOLDERS {
        return Err(invalid("folders", format!("{} folders: at most {MAX_FOLDERS}", file.folders.len())));
    }
    states::check_counts(file)?;
    for (i, e) in file.elements.iter().enumerate() {
        if !crate::mods::valid_id(&e.id) {
            return Err(invalid(&format!("elements[{i}].id"), format!("{:?} is not a valid id", e.id)));
        }
        if let ElementKind::Image { source, .. } = &e.kind
            && !crate::levels::valid_source(source)
        {
            return Err(invalid(
                &format!("elements[{i}].source"),
                format!("{source:?} is not one portable file name ending .png, .jpg or .jpeg"),
            ));
        }
    }
    Ok(())
}

/// Everything checkable without decoding pixels: the name, art scale and
/// size; the elements and folders as a composition of a level of the
/// asset's size (`check_composition`: ids, image fields, the folder rules,
/// the pixel budget with every stacked picture, every file present, each
/// file's size from its header (a stacked sheet `1 + n` pictures tall),
/// glow masks of the box's size), no raster or distant layer; every glow
/// naming one of the asset's groups, the groups valid
/// (`compile_look_source`) and each named; every surface naming one of the
/// asset's materials, the materials valid, the painted surface maps of
/// their boxes with values within the materials, and each material named
/// (issue #38); the states, exposed properties, sounds and health (issue
/// #71), each sound present within `MAX_ASSET_SOUND_BYTES`, the script
/// within `MAX_SCRIPT_BYTES`. The library's check.
pub fn check_asset(src: &AssetSource) -> Result<(), FormatError> {
    check(src, false)
}

/// [`check_asset`] for an asset a level holds (issue #71: its ground), as a
/// level's composition and look allow: a raster and distant layers may be
/// among its elements, and glow groups and materials no element names may
/// be listed. A raster or distant layer still cannot be owned by a state
/// or named by a property.
pub fn check_own_asset(src: &AssetSource) -> Result<(), FormatError> {
    check(src, true)
}

fn check(src: &AssetSource, own: bool) -> Result<(), FormatError> {
    let f = &src.file;
    let name_len = f.name.chars().count();
    if f.name.trim().is_empty() || name_len > MAX_ASSET_NAME || f.name.chars().any(char::is_control) {
        return Err(invalid(
            "name",
            format!("{:?}: a name is 1 to {MAX_ASSET_NAME} characters, no control characters", f.name),
        ));
    }
    if !(1..=MAX_ART_SCALE).contains(&f.art_scale) {
        return Err(invalid("art_scale", format!("{}: 1 to {MAX_ART_SCALE}", f.art_scale)));
    }
    let side = |v: u32| (1..=MAX_SIDE).contains(&v);
    if !side(f.size.0) || !side(f.size.1) {
        return Err(invalid("size", format!("{} × {}: each side 1 to {MAX_SIDE}", f.size.0, f.size.1)));
    }
    check_names(f)?;
    if !own {
        if let Some(i) = f.elements.iter().position(|e| matches!(e.kind, ElementKind::Raster)) {
            return Err(invalid(
                &format!("elements[{i}].kind"),
                "an asset holds images and drawn layers, no raster",
            ));
        }
        if let Some(i) = f.elements.iter().position(|e| e.distant.is_some()) {
            return Err(invalid(&format!("elements[{i}].distant"), "an asset carries no distant layer"));
        }
    }
    let look = LookSource {
        glow_groups: f.glow_groups.clone(),
        materials: f.materials.clone(),
        ..LookSource::default()
    };
    crate::look::compile_look_source(&look, ASSET_FILE).map_err(look_paths)?;
    let composition = as_composition(src);
    check_glow_groups(&composition.composition, Some(&look)).map_err(of_asset)?;
    check_surfaces(&composition.composition, Some(&look)).map_err(of_asset)?;
    if !own {
        for (i, g) in f.glow_groups.iter().enumerate() {
            if !f.elements.iter().any(|e| e.glow.as_ref().is_some_and(|x| x.group == g.id)) {
                return Err(invalid(
                    &format!("glow_groups[{i}]"),
                    format!("no element glows in group {:?}", g.id),
                ));
            }
        }
    }
    states::check_states(f)?;
    let owners = owners(f);
    let pictures = |e: &Element| 1 + owners.get(e.id.as_str()).copied().unwrap_or(0);
    check_stacked_composition(&composition, f.size, f.art_scale, &pictures).map_err(of_asset)?;
    check_surface_maps(&composition, f.size, f.materials.len()).map_err(of_asset)?;
    if !own {
        let used = used_materials(&composition, f.size, &f.materials).map_err(of_asset)?;
        for (i, m) in f.materials.iter().enumerate() {
            if !used.contains(&i) {
                return Err(invalid(
                    &format!("materials[{i}]"),
                    format!("no element is made of material {:?}", m.id),
                ));
            }
        }
    }
    for (name, bytes) in &src.sounds {
        if !f.sounds.contains(name) {
            return Err(io(&sound_file(name), "not listed in asset.ron's sounds".into()));
        }
        if bytes.len() as u64 > MAX_ASSET_SOUND_BYTES {
            return Err(io(
                &sound_file(name),
                format!("{} bytes, more than {MAX_ASSET_SOUND_BYTES}", bytes.len()),
            ));
        }
    }
    if let Some(s) = &src.script
        && s.len() as u64 > MAX_SCRIPT_BYTES
    {
        return Err(io(SCRIPT_FILE, format!("{} bytes, more than {MAX_SCRIPT_BYTES}", s.len())));
    }
    if let Some((i, name)) = f.sounds.iter().enumerate().find(|(_, n)| !src.sounds.contains_key(*n)) {
        return Err(invalid(&format!("sounds[{i}]"), format!("{}: missing", sound_file(name))));
    }
    Ok(())
}

/// A look error said of `asset.ron`'s own fields: `look.materials[i]` is
/// `materials[i]` there.
fn look_paths(e: FormatError) -> FormatError {
    match e {
        FormatError::Invalid { file, path, message } => FormatError::Invalid {
            file,
            path: path.strip_prefix("look.").map_or(path.clone(), str::to_string),
            message,
        },
        e => e,
    }
}

/// The indices of `materials` that `src`'s elements (a level of `size`)
/// name: by `surface` or `back_surface`, or by a value of a painted map.
fn used_materials(
    src: &CompositionSource,
    size: (u32, u32),
    materials: &[MaterialSource],
) -> Result<BTreeSet<usize>, FormatError> {
    let mut used = BTreeSet::new();
    for e in &src.composition.elements {
        for id in [&e.surface, &e.back_surface].into_iter().flatten() {
            used.extend(materials.iter().position(|m| m.id == *id));
        }
        for (file, on) in
            [(surface_file(&e.id), e.surface_map), (surface_back_file(&e.id), e.back_surface_map)]
        {
            if on {
                let (_, values) = decode_surface_map(src, &file, surface_map_size(&e.kind, size))?;
                let mut seen = [false; 256];
                for v in values {
                    seen[usize::from(v)] = true;
                }
                used.extend((1..256).filter(|v| seen[*v]).map(|v| v - 1));
            }
        }
    }
    Ok(used)
}

/// The asset folder `src`: `asset.ron`, then its elements' files (each at
/// most `MAX_FILE_BYTES`), its sounds (each at most
/// `MAX_ASSET_SOUND_BYTES`) and its script (at most `MAX_SCRIPT_BYTES`,
/// UTF-8; none when absent), `MAX_ASSET_BYTES` together; counts, ids,
/// sources and sound names checked before any is read, so no path leaves
/// the folder; and its thumbnail, read tolerantly (absent, above
/// `MAX_THUMB_BYTES` or not a PNG within `MAX_THUMB_SIDE`: none). Not
/// checked further: `check_asset`.
pub fn read_asset_files(src: &dyn ContentSource) -> Result<AssetSource, FormatError> {
    let file: AssetFile = from_canonical(ASSET_FILE, &read_text(src, ASSET_FILE)?)?;
    check_names(&file)?;
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    let mut read = |f: &str, max: u64| -> Result<Vec<u8>, FormatError> {
        let path = VPath::new(f).map_err(|e| io(f, e.to_string()))?;
        let bytes = src.read_max(&path, max).map_err(|e| io(f, e.to_string()))?;
        total = total.saturating_add(bytes.len() as u64);
        if total > MAX_ASSET_BYTES {
            return Err(io(f, format!("the asset's files hold more than {MAX_ASSET_BYTES} bytes")));
        }
        Ok(bytes)
    };
    for e in &file.elements {
        for f in asset_files(e) {
            if files.contains_key(&f) {
                continue;
            }
            let bytes = read(&f, MAX_FILE_BYTES)?;
            files.insert(f, bytes);
        }
    }
    let mut sounds = BTreeMap::new();
    for name in &file.sounds {
        sounds.insert(name.clone(), read(&sound_file(name), MAX_ASSET_SOUND_BYTES)?);
    }
    let script_path = VPath::new(SCRIPT_FILE).map_err(|e| io(SCRIPT_FILE, e.to_string()))?;
    let script = match src.exists(&script_path) {
        true => {
            let bytes = read(SCRIPT_FILE, MAX_SCRIPT_BYTES)?;
            Some(String::from_utf8(bytes).map_err(|_| io(SCRIPT_FILE, "not UTF-8".into()))?)
        }
        false => None,
    };
    Ok(AssetSource { file, files, thumb: read_thumb(src), sounds, script })
}

/// The thumbnail, tolerantly.
fn read_thumb(src: &dyn ContentSource) -> Option<Vec<u8>> {
    let path = VPath::new(ASSET_THUMB_FILE).ok()?;
    let bytes = src.read_max(&path, MAX_THUMB_BYTES).ok()?;
    let (w, h) = image_io::png_size(&bytes)?;
    (w > 0 && h > 0 && w <= MAX_THUMB_SIDE && h <= MAX_THUMB_SIDE).then_some(bytes)
}

/// The files [`write_asset_files`] writes for `src`, by path in its
/// folder: `asset.ron`, the sounds `asset.ron` lists and `src` holds, the
/// script, the elements' files `src` holds and the thumbnail. Nothing is
/// encoded.
pub fn asset_paths(src: &AssetSource) -> Vec<String> {
    let mut out = vec![ASSET_FILE.to_string()];
    out.extend(src.file.sounds.iter().filter(|n| src.sounds.contains_key(*n)).map(|n| sound_file(n)));
    if src.script.is_some() {
        out.push(SCRIPT_FILE.into());
    }
    for e in &src.file.elements {
        for f in asset_files(e) {
            if src.files.contains_key(&f) && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    if src.thumb.is_some() {
        out.push(ASSET_THUMB_FILE.into());
    }
    out
}

/// Writes `asset.ron` in canonical form (`start` left out when it names
/// the first state), the elements' files, the sounds `asset.ron` lists,
/// the script and the thumbnail (counts, ids, sources and sound names
/// checked before anything is written; files no element names are not
/// written).
pub fn write_asset_files(sink: &mut dyn ContentSink, src: &AssetSource) -> Result<(), FormatError> {
    check_names(&src.file)?;
    let mut file = src.file.clone();
    if file.start.as_ref().is_some_and(|s| file.states.first().is_some_and(|f| f.name == *s)) {
        file.start = None;
    }
    write_text(sink, ASSET_FILE, &to_canonical(ASSET_FILE, &file)?)?;
    for name in &file.sounds {
        if let Some(bytes) = src.sounds.get(name) {
            let f = sound_file(name);
            let path = VPath::new(&f).map_err(|e| io(&f, e.to_string()))?;
            sink.write(&path, bytes).map_err(|e| io(&f, e.to_string()))?;
        }
    }
    if let Some(script) = &src.script {
        let path = VPath::new(SCRIPT_FILE).map_err(|e| io(SCRIPT_FILE, e.to_string()))?;
        sink.write(&path, script.as_bytes()).map_err(|e| io(SCRIPT_FILE, e.to_string()))?;
    }
    for e in &src.file.elements {
        for f in asset_files(e) {
            if let Some(bytes) = src.files.get(&f) {
                let path = VPath::new(&f).map_err(|e| io(&f, e.to_string()))?;
                sink.write(&path, bytes).map_err(|e| io(&f, e.to_string()))?;
            }
        }
    }
    if let Some(t) = &src.thumb {
        let path = VPath::new(ASSET_THUMB_FILE).map_err(|e| io(ASSET_THUMB_FILE, e.to_string()))?;
        sink.write(&path, t).map_err(|e| io(ASSET_THUMB_FILE, e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use olr_data::image::RgbaImage;
    use olr_vfs::MemFs;

    use super::*;
    use crate::Colour;
    use crate::levels::{ElementGlow, ElementMaterial};
    use crate::look::{AnimationSource, GlowColourSource, ReachSource};

    fn group(id: &str) -> GlowGroupSource {
        GlowGroupSource {
            id: id.into(),
            colour: GlowColourSource::Own,
            intensity: 300,
            reach: ReachSource::Pixels(40),
            animation: AnimationSource::Steady,
            phase_ms: None,
        }
    }

    /// A 4 × 3 asset at art scale 2: one drawn layer glowing in "flames"
    /// with a mask.
    fn sample() -> AssetSource {
        let mut files = BTreeMap::new();
        files.insert("layers/torch.png".into(), image_io::encode_rgba(&RgbaImage::new(8, 6)));
        files.insert("glow/torch.png".into(), image_io::encode_grey(8, 6, &[255; 48]));
        let element = Element {
            id: "torch".into(),
            name: "Torch".into(),
            visible: true,
            locked: false,
            folder: None,
            kind: ElementKind::Drawn {
                material: ElementMaterial::Background,
                at: (0, 0),
                size: Some((4, 3)),
                mixed: false,
                back: false,
            },
            glow: Some(ElementGlow { strength: 100, group: "flames".into(), mask: true, beam: None }),
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        let mut file = AssetFile::new("Torch", (4, 3), vec![element]);
        file.art_scale = 2;
        file.glow_groups = vec![group("flames")];
        AssetSource {
            thumb: Some(image_io::encode_rgba(&RgbaImage::new(16, 12))),
            ..AssetSource::new(file, files)
        }
    }

    #[test]
    fn an_asset_round_trips_through_its_folder() {
        let src = sample();
        check_asset(&src).unwrap();
        let mut fs = MemFs::new();
        write_asset_files(&mut fs, &src).unwrap();
        assert_eq!(read_asset_files(&fs).unwrap(), src);
    }

    #[test]
    fn bad_assets_are_refused_with_a_message() {
        let mut s = sample();
        s.file.glow_groups[0].id = "sky".into();
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.contains("asset.ron") && e.contains("flames"), "{e}");
        let mut s = sample();
        s.files.insert("glow/torch.png".into(), image_io::encode_grey(4, 3, &[255; 12]));
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.starts_with("glow/torch.png") && e.contains("8x6"), "{e}");
        let mut s = sample();
        s.file.size = (MAX_SIDE + 1, 3);
        assert!(check_asset(&s).unwrap_err().to_string().contains("size"));
        let mut s = sample();
        s.file.art_scale = MAX_ART_SCALE + 1;
        assert!(check_asset(&s).is_err());
        let mut s = sample();
        s.file.name = String::new();
        assert!(check_asset(&s).is_err());
        let mut s = sample();
        s.file.glow_groups.push(group("unused"));
        assert!(check_asset(&s).unwrap_err().to_string().contains("unused"));
        let mut s = sample();
        s.file.elements[0].kind = ElementKind::Raster;
        assert!(check_asset(&s).is_err());
        let mut s = sample();
        s.file.elements[0].glow = None;
        s.file.glow_groups.clear();
        s.file.elements[0].distant = Some(50);
        assert!(check_asset(&s).unwrap_err().to_string().contains("no distant layer"));
        let mut s = sample();
        s.files.remove("layers/torch.png");
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.contains("layers/torch.png: missing") && !e.contains("editor/"), "{e}");
    }

    /// A hostile id never names a path outside the folder: refused before
    /// any file is read or written.
    #[test]
    fn a_bad_id_is_refused_before_any_file() {
        let mut s = sample();
        s.file.elements[0].id = "../up".into();
        let mut fs = MemFs::new();
        assert!(write_asset_files(&mut fs, &s).is_err());
        let text = to_canonical(ASSET_FILE, &s.file).unwrap();
        let mut fs = MemFs::new();
        fs.write(&VPath::new(ASSET_FILE).unwrap(), text.as_bytes()).unwrap();
        assert!(read_asset_files(&fs).unwrap_err().to_string().contains("not a valid id"));
    }

    fn material(id: &str) -> MaterialSource {
        MaterialSource { id: id.into(), name: id.into(), notes: String::new(), heat: None, light_soak: None }
    }

    /// `sample()` made of stone, with a painted map naming wood at one
    /// pixel (issue #38).
    fn with_materials() -> AssetSource {
        let mut s = sample();
        let e = &mut s.file.elements[0];
        e.surface = Some("stone".into());
        e.surface_map = true;
        let mut map = [0u8; 12];
        map[5] = 2;
        s.files.insert("surface/torch.png".into(), image_io::encode_grey(4, 3, &map));
        s.file.materials = vec![material("stone"), material("wood")];
        s
    }

    #[test]
    fn an_asset_round_trips_its_materials_and_maps() {
        let src = with_materials();
        check_asset(&src).unwrap();
        let mut fs = MemFs::new();
        write_asset_files(&mut fs, &src).unwrap();
        assert!(fs.read(&VPath::new("surface/torch.png").unwrap()).is_ok());
        assert_eq!(read_asset_files(&fs).unwrap(), src);
        let text = to_canonical(ASSET_FILE, &src.file).unwrap();
        assert!(text.contains("materials: [") && text.contains("surface: \"stone\""), "{text}");
    }

    /// An asset without materials writes no `materials` (its bytes as
    /// before issue #38).
    #[test]
    fn an_asset_without_materials_names_none() {
        let text = to_canonical(ASSET_FILE, &sample().file).unwrap();
        assert!(!text.contains("materials") && !text.contains("surface"), "{text}");
    }

    #[test]
    fn bad_materials_are_refused_with_a_message() {
        let mut s = with_materials();
        s.file.elements[0].surface = Some("lava".into());
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.contains("elements[0].surface") && e.contains("lava") && !e.contains("level.ron"), "{e}");
        let mut s = with_materials();
        s.file.materials.push(material("glass"));
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.contains("materials[2]") && e.contains("glass"), "{e}");
        let mut s = with_materials();
        s.file.materials[1].id = "stone".into();
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.contains("materials[1].id") && !e.contains("look."), "{e}");
        let mut s = with_materials();
        s.file.materials[0].name = String::new();
        assert!(check_asset(&s).unwrap_err().to_string().contains("materials[0].name"));
        let mut s = with_materials();
        let mut map = [0u8; 12];
        map[2] = 3;
        s.files.insert("surface/torch.png".into(), image_io::encode_grey(4, 3, &map));
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.starts_with("surface/torch.png") && e.contains("material 3"), "{e}");
        let mut s = with_materials();
        s.files.insert("surface/torch.png".into(), image_io::encode_grey(8, 6, &[0; 48]));
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.starts_with("surface/torch.png") && e.contains("4x3"), "{e}");
        let mut s = with_materials();
        s.files.insert("surface/torch.png".into(), b"not a png".to_vec());
        assert!(check_asset(&s).is_err());
        let mut s = with_materials();
        s.files.remove("surface/torch.png");
        let e = check_asset(&s).unwrap_err().to_string();
        assert!(e.contains("surface/torch.png: missing"), "{e}");
        // A back surface needs back art.
        let mut s = with_materials();
        s.file.elements[0].back_surface = Some("wood".into());
        assert!(check_asset(&s).unwrap_err().to_string().contains("back_surface"));
        let mut s = with_materials();
        s.file.materials = (0..256).map(|i| material(&format!("m{i}"))).collect();
        assert!(check_asset(&s).unwrap_err().to_string().contains("above 255"));
    }

    /// A material named only by a painted map's value is used.
    #[test]
    fn a_painted_value_names_its_material() {
        let mut s = with_materials();
        s.file.elements[0].surface = None;
        assert!(check_asset(&s).unwrap_err().to_string().contains("materials[0]"));
        let mut map = [0u8; 12];
        (map[0], map[5]) = (1, 2);
        s.files.insert("surface/torch.png".into(), image_io::encode_grey(4, 3, &map));
        check_asset(&s).unwrap();
        s.files.insert("surface/torch.png".into(), image_io::encode_grey(4, 3, &[0; 12]));
        assert!(check_asset(&s).unwrap_err().to_string().contains("materials[0]"));
    }

    #[test]
    fn a_bad_thumbnail_is_none() {
        let mut fs = MemFs::new();
        let mut s = sample();
        s.thumb = Some(b"not a png".to_vec());
        write_asset_files(&mut fs, &s).unwrap();
        assert_eq!(read_asset_files(&fs).unwrap().thumb, None);
    }

    // Issue #71: states, exposed properties, sounds, health and a script.

    fn drawn(id: &str, material: ElementMaterial, glow: Option<ElementGlow>) -> Element {
        Element {
            id: id.into(),
            name: String::new(),
            visible: true,
            locked: false,
            folder: None,
            kind: ElementKind::Drawn { material, at: (0, 0), size: Some((4, 3)), mixed: false, back: false },
            glow,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        }
    }

    fn state(name: &str) -> StateSource {
        StateSource::new(name)
    }

    /// A 4 × 3 control desk at art scale 2: `screen` (background, glowing
    /// in `screens`, masked) and `body` (rock); states Normal, Alarm
    /// (owning the screen, setting its colour, both sounds) and Wrecked
    /// (owning the body); one exposed colour; health 200. Each owned
    /// layer's files stack two pictures.
    fn desk() -> AssetSource {
        let glow = ElementGlow { strength: 80, group: "screens".into(), mask: true, beam: None };
        let mut file = AssetFile::new(
            "Control desk",
            (4, 3),
            vec![
                drawn("body", ElementMaterial::Rock, None),
                drawn("screen", ElementMaterial::Background, Some(glow)),
            ],
        );
        file.art_scale = 2;
        let mut g = group("screens");
        g.colour = GlowColourSource::Tint(Colour::parse("#40ff80").unwrap());
        file.glow_groups = vec![g];
        let red = ValueSource::Colour(Colour::parse("#ff2020").unwrap());
        file.states = vec![
            state("Normal"),
            StateSource {
                own: vec!["screen".into()],
                values: vec![StateValue {
                    property: PropertySource::GlowColour("screen".into()),
                    value: red,
                }],
                loop_sound: Some("alarm.ogg".into()),
                enter_sound: Some("tick.wav".into()),
                ..state("Alarm")
            },
            StateSource { own: vec!["body".into()], ..state("Wrecked") },
        ];
        file.exposed = vec![ExposedSource {
            name: "screen_colour".into(),
            property: PropertySource::GlowColour("screen".into()),
            default: Some(ValueSource::Colour(Colour::parse("#40ff80").unwrap())),
            min: None,
            max: None,
        }];
        file.sounds = vec!["alarm.ogg".into(), "tick.wav".into()];
        file.health = Some(HealthSource { points: 200, at_zero: "Wrecked".into() });
        let mut files = BTreeMap::new();
        let mut tall = RgbaImage::new(8, 12);
        tall.set(1, 7, olr_data::image::Rgba::rgb(255, 0, 0));
        files.insert("layers/body.png".into(), image_io::encode_rgba(&tall));
        files.insert("layers/screen.png".into(), image_io::encode_rgba(&RgbaImage::new(8, 12)));
        files.insert("glow/screen.png".into(), image_io::encode_grey(8, 12, &[200; 96]));
        let mut sounds = BTreeMap::new();
        sounds.insert("alarm.ogg".to_string(), b"OggS alarm".to_vec());
        sounds.insert("tick.wav".to_string(), b"RIFF tick".to_vec());
        AssetSource { sounds, script: Some("fn on_enter() {}\n".into()), ..AssetSource::new(file, files) }
    }

    fn refused(s: &AssetSource) -> String {
        check_asset(s).unwrap_err().to_string()
    }

    #[test]
    fn a_stateless_asset_writes_as_before() {
        let text = to_canonical(ASSET_FILE, &sample().file).unwrap();
        for field in ["states", "start", "exposed", "sounds", "health", "folders"] {
            assert!(!text.contains(field), "{field}: {text}");
        }
        let mut fs = MemFs::new();
        write_asset_files(&mut fs, &sample()).unwrap();
        assert!(!fs.exists(&VPath::new(SCRIPT_FILE).unwrap()));
        // Every built-in asset reads and writes byte for byte.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/assets");
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        let mut seen = 0;
        for entry in entries.flatten().filter(|e| e.path().join(ASSET_FILE).is_file()) {
            let disk = olr_vfs::DirFs::new(entry.path());
            let src = read_asset_files(&disk).unwrap();
            check_asset(&src).unwrap();
            assert!(!src.file.has_states() && src.script.is_none(), "{}", entry.path().display());
            let mut fs = MemFs::new();
            write_asset_files(&mut fs, &src).unwrap();
            let mut paths = vec![ASSET_FILE.to_string()];
            paths.extend(src.files.keys().cloned());
            for p in paths {
                let v = VPath::new(&p).unwrap();
                assert_eq!(fs.read(&v).unwrap(), disk.read(&v).unwrap(), "{}: {p}", entry.path().display());
            }
            seen += 1;
        }
        assert!(seen >= 7, "{seen} built-in assets");
    }

    #[test]
    fn states_round_trip_with_stacked_pictures() {
        let src = desk();
        check_asset(&src).unwrap();
        let mut fs = MemFs::new();
        write_asset_files(&mut fs, &src).unwrap();
        let back = read_asset_files(&fs).unwrap();
        assert_eq!(back, src);
        let sheet = fs.read(&VPath::new("layers/screen.png").unwrap()).unwrap();
        assert_eq!(image_io::png_size(&sheet), Some((8, 6 * 2)));
        assert_eq!(fs.read(&VPath::new("sounds/alarm.ogg").unwrap()).unwrap(), b"OggS alarm");
        let text = to_canonical(ASSET_FILE, &src.file).unwrap();
        assert!(text.contains("GlowColour(\"screen\")") && text.contains("Colour(\"#ff2020\")"), "{text}");
        // A start naming the first state is left out.
        let mut s = desk();
        s.file.start = Some("Normal".into());
        let mut fs = MemFs::new();
        write_asset_files(&mut fs, &s).unwrap();
        assert_eq!(read_asset_files(&fs).unwrap().file.start, None);
        // Another start is kept.
        let mut s = desk();
        s.file.states.swap(0, 2);
        s.file.start = Some("Normal".into());
        check_asset(&s).unwrap();
        let mut fs = MemFs::new();
        write_asset_files(&mut fs, &s).unwrap();
        assert_eq!(read_asset_files(&fs).unwrap(), s);
    }

    #[test]
    fn state_rules_are_refused_naming_the_state() {
        let mut s = desk();
        s.file.states[2].name = "alarm".into();
        let e = refused(&s);
        assert!(e.contains("states[2].name") && e.contains("alarm"), "{e}");
        let mut s = desk();
        s.file.states[1].name = "Al/arm".into();
        assert!(refused(&s).contains("states[1].name"));
        let mut s = desk();
        s.file.start = Some("Off".into());
        let e = refused(&s);
        assert!(e.contains("start") && e.contains("Off"), "{e}");
        let mut s = desk();
        s.file.states[0].own = vec!["body".into()];
        let e = refused(&s);
        assert!(e.contains("states[0].own") && e.contains("Normal"), "{e}");
        // An image is the same in every state.
        let mut s = sample();
        s.file.elements.push(Element {
            id: "logo".into(),
            kind: ElementKind::Image {
                material: ElementMaterial::Background,
                source: "logo.png".into(),
                at: (0, 0),
                size: (4, 3),
                pixel_size: 1,
                colours: None,
                dither: crate::levels::Dither::None,
                alpha_threshold: 128,
                mask: false,
            },
            ..drawn("x", ElementMaterial::Rock, None)
        });
        s.files.insert("images/logo.png".into(), image_io::encode_rgba(&RgbaImage::new(4, 3)));
        check_asset(&s).unwrap();
        s.file.states.push(StateSource { own: vec!["logo".into()], ..state("Lit") });
        let e = refused(&s);
        assert!(e.contains("states[1].own[0]") && e.contains("Lit") && e.contains("drawn layer"), "{e}");
        let mut s = desk();
        s.file.states[1].own = vec!["lamp".into()];
        let e = refused(&s);
        assert!(e.contains("states[1].own[0]") && e.contains("Alarm") && e.contains("lamp"), "{e}");
        let mut s = desk();
        s.file.states[1].own = vec!["screen".into(), "screen".into()];
        let e = refused(&s);
        assert!(e.contains("states[1].own[1]") && e.contains("twice"), "{e}");
        let mut s = desk();
        s.file.states[1].loop_sound = Some("siren.ogg".into());
        let e = refused(&s);
        assert!(e.contains("states[1].loop_sound") && e.contains("siren.ogg"), "{e}");
        let mut s = desk();
        s.file.states[1].enter_sound = Some("bell.wav".into());
        assert!(refused(&s).contains("states[1].enter_sound"));
        let mut s = desk();
        s.file.states = (0..=MAX_STATES).map(|i| state(&format!("S{i}"))).collect();
        s.file.health = None;
        assert!(refused(&s).contains("states"));
        let mut fs = MemFs::new();
        assert!(write_asset_files(&mut fs, &s).is_err());
        let mut s = desk();
        s.file.states.clear();
        assert!(refused(&s).contains("states"));
        // A sheet of the wrong height.
        let mut s = desk();
        s.files.insert("layers/screen.png".into(), image_io::encode_rgba(&RgbaImage::new(8, 6)));
        let e = refused(&s);
        assert!(e.starts_with("layers/screen.png") && e.contains("8x12"), "{e}");
        let mut s = desk();
        s.files.insert("glow/screen.png".into(), image_io::encode_grey(8, 6, &[1; 48]));
        assert!(refused(&s).starts_with("glow/screen.png"));
        // A raster is never owned (a level's own asset may hold one).
        let mut s = desk();
        s.file
            .elements
            .insert(0, Element { kind: ElementKind::Raster, ..drawn("base", ElementMaterial::Rock, None) });
        for f in crate::levels::raster_files("base") {
            let bytes = if f.contains("material") {
                image_io::encode_indexed(4, 3, &[0; 12], &crate::levels::MATERIAL_PALETTE)
            } else {
                image_io::encode_rgba(&RgbaImage::new(8, 6))
            };
            s.files.insert(strip_editor(&f).to_string(), bytes);
        }
        check_own_asset(&s).unwrap();
        s.file.states[2].own.push("base".into());
        let e = check_own_asset(&s).unwrap_err().to_string();
        assert!(e.contains("states[2].own[1]") && e.contains("raster"), "{e}");
    }

    fn expose(name: &str, property: PropertySource, default: Option<ValueSource>) -> ExposedSource {
        ExposedSource { name: name.into(), property, default, min: None, max: None }
    }

    #[test]
    fn exposed_rules_are_refused_naming_the_entry() {
        use PropertySource as P;
        use ValueSource as V;
        let with = |x: ExposedSource| {
            let mut s = desk();
            s.file.exposed.push(x);
            s
        };
        let e = refused(&with(expose("Claw", P::Visible, None)));
        assert!(e.contains("exposed[1].name") && e.contains("Claw"), "{e}");
        assert!(refused(&with(expose("9lives", P::Visible, None))).contains("exposed[1].name"));
        let e = refused(&with(expose("screen_colour", P::Visible, None)));
        assert!(e.contains("exposed[1].name") && e.contains("twice"), "{e}");
        check_asset(&with(expose("shown", P::Visible, None))).unwrap();
        // Each property on a layer it is not valid on.
        let mut s = desk();
        s.file.elements[0].kind = ElementKind::Drawn {
            material: ElementMaterial::Dirt,
            at: (0, 0),
            size: Some((4, 3)),
            mixed: false,
            back: false,
        };
        s.file.exposed.push(expose("x", P::LayerVisible("body".into()), None));
        let e = refused(&s);
        assert!(e.contains("exposed[1].property") && e.contains("dirt"), "{e}");
        for (p, why) in [
            (P::GlowStrength("body".into()), "does not glow"),
            (P::BeamDirection("screen".into()), "no beam"),
            (P::GlassOpacity("screen".into()), "not glass"),
            (P::Opacity("body".into()), "not a background"),
            (P::OffsetX("lamp".into()), "no layer"),
            (P::Health, "no health"),
        ] {
            let mut s = with(expose("p", p.clone(), None));
            s.file.exposed[1].min = Some(0);
            s.file.exposed[1].max = Some(0);
            s.file.health = None;
            let e = refused(&s);
            assert!(e.contains("exposed[1].property") && e.contains(why), "{p:?}: {e}");
        }
        let mut s = with(expose("reach", P::LightReach("screen".into()), None));
        s.file.glow_groups[0].reach = ReachSource::Unlimited;
        assert!(refused(&s).contains("not Pixels"));
        // A position's limits.
        let e = refused(&with(expose("claw_x", P::OffsetX("body".into()), Some(V::Int(0)))));
        assert!(e.contains("exposed[1].min") && e.contains("min and a max"), "{e}");
        let mut x = expose("claw_x", P::OffsetX("body".into()), Some(V::Int(0)));
        (x.min, x.max) = (Some(0), Some(120));
        check_asset(&with(x.clone())).unwrap();
        (x.min, x.max) = (Some(10), Some(0));
        assert!(refused(&with(x.clone())).contains("exposed[1].min"));
        (x.min, x.max) = (Some(5), Some(120));
        assert!(refused(&with(x.clone())).contains("exposed[1].default"));
        (x.min, x.max) = (Some(0), Some(5000));
        assert!(refused(&with(x.clone())).contains("exposed[1].max"));
        let mut x = expose("shown", P::Visible, None);
        x.min = Some(0);
        assert!(refused(&with(x)).contains("exposed[1].min"));
        // A value of the wrong type, or out of range.
        let e = refused(&with(expose("strength", P::GlowStrength("screen".into()), Some(V::Bool(true)))));
        assert!(e.contains("exposed[1].default") && e.contains("Int"), "{e}");
        assert!(
            refused(&with(expose("strength", P::GlowStrength("screen".into()), Some(V::Int(101)))))
                .contains("0 to 100")
        );
        // One name per value, a glow group's settings once.
        let e = refused(&with(expose("colour_2", P::GlowColour("screen".into()), None)));
        assert!(e.contains("exposed[1].property") && e.contains("screen_colour"), "{e}");
        let mut s = desk();
        let glow = ElementGlow { strength: 50, group: "screens".into(), mask: false, beam: None };
        s.file.elements.push(drawn("buttons", ElementMaterial::Background, Some(glow)));
        s.files.insert("layers/buttons.png".into(), image_io::encode_rgba(&RgbaImage::new(8, 6)));
        check_asset(&s).unwrap();
        s.file.exposed.push(expose("button_colour", P::GlowColour("buttons".into()), None));
        let e = refused(&s);
        assert!(e.contains("exposed[1].property") && e.contains("screen_colour"), "{e}");
        s.file.exposed[1].property = P::GlowStrength("buttons".into());
        check_asset(&s).unwrap();
        s.file.exposed.push(expose("i1", P::LightIntensity("screen".into()), None));
        s.file.exposed.push(expose("i2", P::LightIntensity("buttons".into()), None));
        assert!(refused(&s).contains("exposed[3].property"));
        // An Own group's colour needs a default.
        let mut s = desk();
        s.file.glow_groups[0].colour = GlowColourSource::Own;
        check_asset(&s).unwrap();
        s.file.exposed[0].default = None;
        let e = refused(&s);
        assert!(e.contains("exposed[0].default") && e.contains("default colour"), "{e}");
        // Health starts at its points.
        check_asset(&with(expose("hp", P::Health, Some(V::Int(200))))).unwrap();
        check_asset(&with(expose("hp", P::Health, None))).unwrap();
        let e = refused(&with(expose("hp", P::Health, Some(V::Int(100)))));
        assert!(e.contains("exposed[1].default") && e.contains("200"), "{e}");
        let mut s = desk();
        s.file.exposed = (0..=MAX_EXPOSED).map(|i| expose(&format!("v{i}"), P::Visible, None)).collect();
        assert!(refused(&s).contains("exposed"));
    }

    #[test]
    fn state_values_are_checked() {
        use PropertySource as P;
        use ValueSource as V;
        let with = |values: Vec<(P, V)>| {
            let mut s = desk();
            s.file.states[2].values =
                values.into_iter().map(|(property, value)| StateValue { property, value }).collect();
            s
        };
        check_asset(&with(vec![(P::GlowStrength("screen".into()), V::Int(0)), (P::Visible, V::Bool(true))]))
            .unwrap();
        let e = refused(&with(vec![(P::GlowStrength("screen".into()), V::Bool(false))]));
        assert!(e.contains("states[2].values[0].value") && e.contains("Int"), "{e}");
        let e = refused(&with(vec![(P::GlowStrength("screen".into()), V::Int(-1))]));
        assert!(e.contains("states[2].values[0].value") && e.contains("0 to 100"), "{e}");
        let e = refused(&with(vec![(
            P::GlowColour("screen".into()),
            V::Colour(Colour::parse("#ff000080").unwrap()),
        )]));
        assert!(e.contains("opaque"), "{e}");
        let e = refused(&with(vec![(P::Visible, V::Bool(true)), (P::Visible, V::Bool(false))]));
        assert!(e.contains("states[2].values[1].property") && e.contains("Wrecked"), "{e}");
        let e = refused(&with(vec![(P::OffsetX("ghost".into()), V::Int(1))]));
        assert!(e.contains("states[2].values[0].property") && e.contains("ghost"), "{e}");
        let bad = AnimationSource::Flicker { speed: 0, depth: 5 };
        assert!(
            refused(&with(vec![(P::GlowAnimation("screen".into()), V::Animation(bad))])).contains("speed")
        );
        // An exposed property's limits bound a state's value.
        let mut s = with(vec![(P::OffsetX("body".into()), V::Int(50))]);
        let mut x = expose("claw_x", P::OffsetX("body".into()), Some(V::Int(0)));
        (x.min, x.max) = (Some(0), Some(40));
        s.file.exposed.push(x);
        assert!(refused(&s).contains("0 to 40"));
        // One group's property set through two layers.
        let mut s = desk();
        let glow = ElementGlow { strength: 50, group: "screens".into(), mask: false, beam: None };
        s.file.elements.push(drawn("buttons", ElementMaterial::Background, Some(glow)));
        s.files.insert("layers/buttons.png".into(), image_io::encode_rgba(&RgbaImage::new(8, 6)));
        s.file.states[2].values = vec![
            StateValue { property: P::LightIntensity("screen".into()), value: V::Int(10) },
            StateValue { property: P::LightIntensity("buttons".into()), value: V::Int(20) },
        ];
        let e = refused(&s);
        assert!(e.contains("states[2].values[1].property"), "{e}");
    }

    #[test]
    fn stacked_pictures_count_in_the_pixel_budget() {
        // Five 4096 × 4096 layers decode to 80 Mi pixels; owned by three
        // states each, their sheets to 320 Mi, over the budget.
        let big = |id: &str| Element {
            kind: ElementKind::Drawn {
                material: ElementMaterial::Rock,
                at: (0, 0),
                size: Some((4096, 4096)),
                mixed: false,
                back: false,
            },
            ..drawn(id, ElementMaterial::Rock, None)
        };
        let ids: Vec<String> = (0..5).map(|i| format!("l{i}")).collect();
        let file = AssetFile::new("Big", (4096, 4096), ids.iter().map(|i| big(i)).collect());
        let mut s = AssetSource::new(file, BTreeMap::new());
        // Without states only the files are missing.
        assert!(refused(&s).contains("missing"));
        s.file.states = vec![state("A"), state("B"), state("C"), state("D")];
        for st in &mut s.file.states[1..] {
            st.own = ids.clone();
        }
        let e = refused(&s);
        assert!(e.contains("pixels") && e.contains(&crate::levels::MAX_COMPOSED_PIXELS.to_string()), "{e}");
    }

    #[test]
    fn sounds_and_script_are_bounded() {
        let written = |s: &AssetSource| {
            let mut fs = MemFs::new();
            write_asset_files(&mut fs, s).unwrap();
            fs
        };
        // A bad name or a third extension: refused before any file is read.
        for name in
            ["../up.wav", "Alarm.ogg", "alarm.mp3", "alarm", "a.b.wav", &format!("{}.wav", "a".repeat(33))]
        {
            let mut s = desk();
            s.file.sounds[0] = name.into();
            let e = refused(&s);
            assert!(e.contains("sounds[0]"), "{name}: {e}");
            let mut fs = MemFs::new();
            assert!(write_asset_files(&mut fs, &s).is_err(), "{name}");
            let text = to_canonical(ASSET_FILE, &s.file).unwrap();
            let fs = MemFs::new().with(ASSET_FILE, text);
            assert!(read_asset_files(&fs).unwrap_err().to_string().contains("sounds[0]"), "{name}");
        }
        let mut s = desk();
        s.file.sounds.push("ALARM.ogg".into());
        assert!(refused(&s).contains("sounds[2]"));
        let mut s = desk();
        s.file.sounds = (0..=MAX_ASSET_SOUNDS).map(|i| format!("s{i}.wav")).collect();
        assert!(refused(&s).contains("sounds"));
        // An oversize sound.
        let mut fs = written(&desk());
        let huge = vec![0u8; MAX_ASSET_SOUND_BYTES as usize + 1];
        fs.write(&VPath::new("sounds/tick.wav").unwrap(), &huge).unwrap();
        let e = read_asset_files(&fs).unwrap_err().to_string();
        assert!(e.contains("sounds/tick.wav"), "{e}");
        let mut s = desk();
        s.sounds.insert("tick.wav".into(), huge);
        assert!(refused(&s).contains("sounds/tick.wav"));
        // A missing one.
        let mut s = desk();
        s.sounds.remove("tick.wav");
        assert!(refused(&s).contains("sounds/tick.wav: missing"));
        // The script: not UTF-8 or too large is an error; absent is none.
        let mut fs = written(&desk());
        fs.write(&VPath::new(SCRIPT_FILE).unwrap(), &[0xff, 0xfe, 0x00]).unwrap();
        let e = read_asset_files(&fs).unwrap_err().to_string();
        assert!(e.contains(SCRIPT_FILE) && e.contains("UTF-8"), "{e}");
        fs.write(&VPath::new(SCRIPT_FILE).unwrap(), &vec![b' '; MAX_SCRIPT_BYTES as usize + 1]).unwrap();
        assert!(read_asset_files(&fs).unwrap_err().to_string().contains(SCRIPT_FILE));
        let mut s = desk();
        s.script = None;
        assert_eq!(read_asset_files(&written(&s)).unwrap().script, None);
        s.script = Some(" ".repeat(MAX_SCRIPT_BYTES as usize + 1));
        assert!(refused(&s).contains(SCRIPT_FILE));
    }

    #[test]
    fn health_names_a_state() {
        let mut s = desk();
        s.file.health = Some(HealthSource { points: 200, at_zero: "Gone".into() });
        let e = refused(&s);
        assert!(e.contains("health.at_zero") && e.contains("Gone"), "{e}");
        for points in [0, MAX_HEALTH + 1] {
            s.file.health = Some(HealthSource { points, at_zero: "Wrecked".into() });
            assert!(refused(&s).contains("health.points"));
        }
        s.file.health = Some(HealthSource { points: MAX_HEALTH, at_zero: "Normal".into() });
        check_asset(&s).unwrap();
    }

    #[test]
    fn folders_and_glass_are_allowed_in_assets() {
        let mut s = sample();
        s.file.folders = vec![crate::levels::Folder::new("lamp", "Lamp", None)];
        s.file.elements[0].folder = Some("lamp".into());
        s.file.elements[0].glass = Some(crate::levels::ElementGlass { opacity: 35, tint: 0, frost: 0 });
        check_asset(&s).unwrap();
        let mut fs = MemFs::new();
        write_asset_files(&mut fs, &s).unwrap();
        assert_eq!(read_asset_files(&fs).unwrap(), s);
        // Issue #59's rules still apply.
        let mut t = s.clone();
        t.file.elements[0].folder = Some("shade".into());
        let e = refused(&t);
        assert!(e.contains("elements[0].folder") && e.contains("shade") && !e.contains("editor/"), "{e}");
        let mut t = s.clone();
        t.file.folders.push(crate::levels::Folder::new("empty", "", None));
        assert!(refused(&t).contains("folders[1]"));
        // Glass keeps its own rules.
        let mut t = s;
        t.file.elements[0].glass = Some(crate::levels::ElementGlass { opacity: 0, tint: 0, frost: 0 });
        assert!(refused(&t).contains("glass.opacity"));
    }

    #[test]
    fn own_assets_may_hold_a_raster_distant_layers_and_unused_groups() {
        let mut s = sample();
        s.file.glow_groups.push(group("unused"));
        s.file.materials.push(material("stone"));
        assert!(check_asset(&s).is_err());
        check_own_asset(&s).unwrap();
        let mut s = sample();
        s.file.elements[0].distant = Some(50);
        s.file.elements[0].glow = None;
        assert!(refused(&s).contains("distant"));
        check_own_asset(&s).unwrap();
        // A distant layer is never owned nor named by a property.
        s.file.states.push(StateSource { own: vec!["torch".into()], ..state("Far") });
        assert!(check_own_asset(&s).unwrap_err().to_string().contains("distant"));
        s.file.states.pop();
        s.file.exposed.push(expose("shown", PropertySource::LayerVisible("torch".into()), None));
        assert!(check_own_asset(&s).unwrap_err().to_string().contains("distant"));
        let mut s = sample();
        s.file.elements[0].kind = ElementKind::Raster;
        s.file.elements[0].glow = None;
        s.file.glow_groups.clear();
        for f in crate::levels::raster_files("torch") {
            let bytes = if f.contains("material") {
                image_io::encode_indexed(4, 3, &[0; 12], &crate::levels::MATERIAL_PALETTE)
            } else {
                image_io::encode_rgba(&RgbaImage::new(8, 6))
            };
            s.files.insert(strip_editor(&f).to_string(), bytes);
        }
        assert!(refused(&s).contains("no raster"));
        check_own_asset(&s).unwrap();
    }
}
