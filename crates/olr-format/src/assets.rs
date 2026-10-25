//! An asset (issue #9): a ready-made piece of a level saved once and
//! dropped into any level by the editor's Assets tab or `olr-cli asset
//! place`. It is a folder, `<asset-id>/`, holding `asset.ron` (its name,
//! the art scale it was made at, its size, its elements and the glow
//! groups and surface materials they name), the elements' files
//! (`images/<source>`, `layers/<id>.png`, `layers/<id>.material.png`,
//! `layers/<id>.back.png`, `masks/<id>.png`, `glow/<id>.png`,
//! `surface/<id>.png`, `surface/<id>.back.png`: a level composition's
//! `editor/` files, without `editor/`) and `thumb.png`, the asset drawn lit under a
//! dark preview look. Elements are a level's composition elements (see
//! the level page), placed relative to the asset's top-left corner; an
//! asset holds no raster. A placed asset is ordinary elements: no level
//! refers to its folder. An asset folder is untrusted content: reading
//! checks it with the level format's limits.

use std::collections::{BTreeMap, BTreeSet};

use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use crate::FormatError;
use crate::image_io;
use crate::levels::{
    CompositionSource, EDITOR_DIR, Element, ElementKind, MAX_ART_SCALE, MAX_ELEMENTS, MAX_SIDE,
    MAX_THUMB_BYTES, MAX_THUMB_SIDE, check_composition, check_glow_groups, check_surface_maps,
    check_surfaces, decode_surface_map, element_files, surface_back_file, surface_file, surface_map_size,
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
/// The most bytes an asset folder's files hold together.
pub const MAX_ASSET_BYTES: u64 = 256 << 20;

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
    /// the asset's size); images and drawn layers only.
    pub elements: Vec<Element>,
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
}

fn one() -> u32 {
    1
}

fn is_one(v: &u32) -> bool {
    *v == 1
}

/// An asset folder in memory: `asset.ron`, its elements' files by path in
/// the folder (`layers/<id>.png`, ...), and its thumbnail.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetSource {
    pub file: AssetFile,
    pub files: BTreeMap<String, Vec<u8>>,
    pub thumb: Option<Vec<u8>>,
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
        composition: crate::levels::Composition { elements: src.file.elements.clone(), folders: Vec::new() },
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

/// The checks that make `asset.ron`'s file paths safe to read or write:
/// the count, each id a valid one, each image's source one portable file
/// name.
fn check_names(file: &AssetFile) -> Result<(), FormatError> {
    let n = file.elements.len();
    if n == 0 || n > MAX_ELEMENTS {
        return Err(invalid("elements", format!("{n} elements: an asset holds 1 to {MAX_ELEMENTS}")));
    }
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
/// size; the elements as a composition of a level of the asset's size
/// (`check_composition`: ids, image fields, the pixel budget, every file
/// present, each file's size from its header, glow masks of the box's
/// size), no raster; every glow naming one of the asset's groups, the
/// groups valid (`compile_look_source`) and each named; every surface
/// naming one of the asset's materials, the materials valid, the painted
/// surface maps of their boxes with values within the materials, and each
/// material named (issue #38).
pub fn check_asset(src: &AssetSource) -> Result<(), FormatError> {
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
    if let Some(i) = f.elements.iter().position(|e| matches!(e.kind, ElementKind::Raster)) {
        return Err(invalid(
            &format!("elements[{i}].kind"),
            "an asset holds images and drawn layers, no raster",
        ));
    }
    if let Some(i) = f.elements.iter().position(|e| e.distant.is_some()) {
        return Err(invalid(&format!("elements[{i}].distant"), "an asset carries no distant layer"));
    }
    if let Some(i) = f.elements.iter().position(|e| e.glass.is_some()) {
        return Err(invalid(&format!("elements[{i}].glass"), "an asset carries no glass"));
    }
    if let Some(i) = f.elements.iter().position(|e| e.folder.is_some()) {
        return Err(invalid(&format!("elements[{i}].folder"), "an asset carries no layer folder"));
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
    for (i, g) in f.glow_groups.iter().enumerate() {
        if !f.elements.iter().any(|e| e.glow.as_ref().is_some_and(|x| x.group == g.id)) {
            return Err(invalid(
                &format!("glow_groups[{i}]"),
                format!("no element glows in group {:?}", g.id),
            ));
        }
    }
    check_composition(&composition, f.size, f.art_scale).map_err(of_asset)?;
    check_surface_maps(&composition, f.size, f.materials.len()).map_err(of_asset)?;
    let used = used_materials(&composition, f.size, &f.materials).map_err(of_asset)?;
    for (i, m) in f.materials.iter().enumerate() {
        if !used.contains(&i) {
            return Err(invalid(
                &format!("materials[{i}]"),
                format!("no element is made of material {:?}", m.id),
            ));
        }
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
/// most `MAX_FILE_BYTES`, `MAX_ASSET_BYTES` together; ids and sources
/// checked before any is read, so no path leaves the folder) and its
/// thumbnail, read tolerantly (absent, above `MAX_THUMB_BYTES` or not a PNG
/// within `MAX_THUMB_SIDE`: none). Not checked further: `check_asset`.
pub fn read_asset_files(src: &dyn ContentSource) -> Result<AssetSource, FormatError> {
    let file: AssetFile = from_canonical(ASSET_FILE, &read_text(src, ASSET_FILE)?)?;
    check_names(&file)?;
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for e in &file.elements {
        for f in asset_files(e) {
            if files.contains_key(&f) {
                continue;
            }
            let path = VPath::new(&f).map_err(|e| io(&f, e.to_string()))?;
            let bytes = src.read_max(&path, MAX_FILE_BYTES).map_err(|e| io(&f, e.to_string()))?;
            total = total.saturating_add(bytes.len() as u64);
            if total > MAX_ASSET_BYTES {
                return Err(io(&f, format!("the asset's files hold more than {MAX_ASSET_BYTES} bytes")));
            }
            files.insert(f, bytes);
        }
    }
    Ok(AssetSource { file, files, thumb: read_thumb(src) })
}

/// The thumbnail, tolerantly.
fn read_thumb(src: &dyn ContentSource) -> Option<Vec<u8>> {
    let path = VPath::new(ASSET_THUMB_FILE).ok()?;
    let bytes = src.read_max(&path, MAX_THUMB_BYTES).ok()?;
    let (w, h) = image_io::png_size(&bytes)?;
    (w > 0 && h > 0 && w <= MAX_THUMB_SIDE && h <= MAX_THUMB_SIDE).then_some(bytes)
}

/// Writes `asset.ron` in canonical form, the elements' files and the
/// thumbnail (ids and sources checked before anything is written; files
/// no element names are not written).
pub fn write_asset_files(sink: &mut dyn ContentSink, src: &AssetSource) -> Result<(), FormatError> {
    check_names(&src.file)?;
    write_text(sink, ASSET_FILE, &to_canonical(ASSET_FILE, &src.file)?)?;
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
            glow: Some(ElementGlow { strength: 100, group: "flames".into(), mask: true }),
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        AssetSource {
            file: AssetFile {
                name: "Torch".into(),
                art_scale: 2,
                size: (4, 3),
                elements: vec![element],
                glow_groups: vec![group("flames")],
                materials: Vec::new(),
            },
            files,
            thumb: Some(image_io::encode_rgba(&RgbaImage::new(16, 12))),
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
        s.file.elements[0].distant = None;
        // Nor glass (the background glass design).
        s.file.elements[0].glass = Some(crate::levels::ElementGlass { opacity: 35, tint: 0, frost: 0 });
        assert!(check_asset(&s).unwrap_err().to_string().contains("no glass"));
        // Issue #59: nor any folder.
        let mut s = sample();
        s.file.elements[0].folder = Some("lamp".into());
        assert!(check_asset(&s).unwrap_err().to_string().contains("no layer folder"));
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
}
