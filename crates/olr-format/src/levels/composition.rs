//! A level's composition (plan 14a, spec
//! `docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md`
//! §2): `editor/composition.ron`, a stack of elements (imported images,
//! drawn layers, raster bases) and their files under `editor/`, which
//! `olr-edit` bakes into `material.png`, `front.png` and `back.png`.

use std::collections::{BTreeMap, BTreeSet};

use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::MAX_SIDE;
use crate::FormatError;
use crate::defaults as d;
use crate::image_io::{self, IMAGES, LEVEL_ART};
use crate::ron_io::{MAX_FILE_BYTES, from_canonical, read_text, to_canonical, write_text};

/// A pack's authoring folder: the editors' sources, never content.
pub const EDITOR_DIR: &str = "editor";
/// The composition's file in a level pack.
pub const COMPOSITION_FILE: &str = "editor/composition.ron";
/// The most elements a composition holds.
pub const MAX_ELEMENTS: usize = 256;
/// The largest `pixel_size` of an image element.
pub const MAX_PIXEL_SIZE: u32 = 64;
/// The most pixels a composition's elements decode to together (each
/// image's element grid, each drawn layer, each raster's front, back and
/// material): four times `LEVEL_ART`'s pixels, 256 Mi. A bake holds them
/// all at once.
pub const MAX_COMPOSED_PIXELS: u64 = 4 * LEVEL_ART.max_pixels;

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

fn one() -> u32 {
    1
}

fn is_one(v: &u32) -> bool {
    *v == 1
}

fn is_origin(v: &(i32, i32)) -> bool {
    *v == (0, 0)
}

fn threshold() -> u8 {
    128
}

fn is_threshold(v: &u8) -> bool {
    *v == 128
}

/// `editor/composition.ron`: the level's elements, bottom to top.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    /// Bottom to top; at most `MAX_ELEMENTS` (256).
    pub elements: Vec<Element>,
}

/// One element of a composition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Element {
    /// A slug unique in the level (`[a-z0-9_.-]`, not starting with `.`);
    /// names its files.
    pub id: String,
    /// Shown in the editor's element list.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Hidden elements are left out of the bake. Default true.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub visible: bool,
    /// The editor does not change a locked element.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub locked: bool,
    /// What the element is.
    pub kind: ElementKind,
    /// Its pixels glow (look system): an image or drawn element only.
    /// Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<ElementGlow>,
}

/// How an element's covered pixels glow: they show at `strength` whatever
/// the darkness, and cast light as their glow group says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementGlow {
    /// Percent, 1 to 100.
    pub strength: u8,
    /// The id of a glow group in `level.ron`'s `look`.
    pub group: String,
    /// A painted mask (`editor/glow/<id>.png`, grey, the element's box at art
    /// resolution: 0 no glow to 255 full) scales the strength per pixel.
    /// Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub mask: bool,
}

/// An element's glow mask: `editor/glow/<id>.png`.
pub fn glow_file(id: &str) -> String {
    format!("{EDITOR_DIR}/glow/{id}.png")
}

/// What an element is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ElementKind {
    /// An imported picture (`editor/images/<source>`), placed in a box and pixelated.
    Image {
        /// What its covered pixels are.
        material: ElementMaterial,
        /// The original's file name under `editor/images/` (`png`, `jpg`, `jpeg`).
        source: String,
        /// The box's top-left, world pixels; may lie outside the level.
        at: (i32, i32),
        /// The box's width and height, world pixels, each 1 to `MAX_SIDE` (4096).
        size: (u32, u32),
        /// Art pixels per element pixel, 1 to `MAX_PIXEL_SIZE` (64). Default 1.
        #[serde(default = "one", skip_serializing_if = "is_one")]
        pixel_size: u32,
        /// Colour reduction: none, or 2 to 256 colours (median cut).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        colours: Option<u16>,
        /// Dithering when colours are reduced.
        #[serde(default, skip_serializing_if = "d::is_default")]
        dither: Dither,
        /// Alpha at or above which a pixel is covered, 0 to 255. Default 128.
        #[serde(default = "threshold", skip_serializing_if = "is_threshold")]
        alpha_threshold: u8,
        /// A mask over the original (`editor/masks/<id>.png`, a grey image
        /// of the original's size as it is shown, EXIF-turned: 0 erased,
        /// anything else kept), applied before resampling. Default false.
        #[serde(default, skip_serializing_if = "d::is_false")]
        mask: bool,
    },
    /// A layer painted with brushes and shapes (`editor/layers/<id>.png`,
    /// its box × art scale; alpha 0 = not covered).
    Drawn {
        /// What its covered pixels are; on a mixed layer, what new paint is
        /// by default.
        material: ElementMaterial,
        /// The layer's top-left, world pixels; may lie outside the level.
        /// Default (0, 0).
        #[serde(default, skip_serializing_if = "is_origin")]
        at: (i32, i32),
        /// The layer's width and height, world pixels, each 1 to `MAX_SIDE`
        /// (4096); none: the level's size (it follows a resize). Default none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<(u32, u32)>,
        /// A material per world pixel of the layer
        /// (`editor/layers/<id>.material.png`, `material.png`'s palette)
        /// instead of `material`. Default false.
        #[serde(default, skip_serializing_if = "d::is_false")]
        mixed: bool,
        /// A back art (`editor/layers/<id>.back.png`, the layer's art size):
        /// what shows where its dirt or rock is carved. Default false.
        #[serde(default, skip_serializing_if = "d::is_false")]
        back: bool,
    },
    /// A whole level's own material, front and back
    /// (`editor/layers/<id>.material.png`, `.front.png`, `.back.png`).
    Raster,
}

impl ElementKind {
    /// A plain drawn layer of `material`, the level's size at the origin.
    pub fn drawn(material: ElementMaterial) -> ElementKind {
        ElementKind::Drawn { material, at: (0, 0), size: None, mixed: false, back: false }
    }
}

/// An element's material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementMaterial {
    /// Indestructible.
    Rock,
    /// Destructible.
    Dirt,
    /// Open air; a background element is the backdrop seen where dirt is carved.
    Background,
}

impl From<ElementMaterial> for olr_data::level::Material {
    fn from(m: ElementMaterial) -> Self {
        match m {
            ElementMaterial::Rock => Self::Rock,
            ElementMaterial::Dirt => Self::Dirt,
            ElementMaterial::Background => Self::Background,
        }
    }
}

/// Dithering of an image element's colour reduction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Dither {
    /// Nearest colour.
    #[default]
    None,
    /// Bayer 8 × 8.
    Ordered,
    /// Floyd-Steinberg error diffusion.
    Diffusion,
}

/// A composition with its files' bytes, by pack path (`editor/images/…`,
/// `editor/layers/…`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompositionSource {
    /// `editor/composition.ron`.
    pub composition: Composition,
    /// The elements' files, as stored (encoded PNG or JPEG bytes).
    pub files: BTreeMap<String, Vec<u8>>,
}

/// An image element's original: `editor/images/<source>`.
pub fn image_file(source: &str) -> String {
    format!("{EDITOR_DIR}/images/{source}")
}

/// A drawn element's layer: `editor/layers/<id>.png`.
pub fn layer_file(id: &str) -> String {
    format!("{EDITOR_DIR}/layers/{id}.png")
}

/// A drawn element's files: its art (`layer_file`), then its material
/// (`editor/layers/<id>.material.png`) when mixed and its back art
/// (`editor/layers/<id>.back.png`) when backed.
pub fn drawn_files(id: &str, mixed: bool, back: bool) -> Vec<String> {
    let mut out = vec![layer_file(id)];
    if mixed {
        out.push(format!("{EDITOR_DIR}/layers/{id}.material.png"));
    }
    if back {
        out.push(format!("{EDITOR_DIR}/layers/{id}.back.png"));
    }
    out
}

/// An image element's mask: `editor/masks/<id>.png`.
pub fn mask_file(id: &str) -> String {
    format!("{EDITOR_DIR}/masks/{id}.png")
}

/// A drawn element's box in world pixels, its `at` and its size (the
/// level's, `level`, when it has none); `None` for other kinds.
pub fn drawn_box(kind: &ElementKind, level: (u32, u32)) -> Option<((i32, i32), (u32, u32))> {
    match kind {
        ElementKind::Drawn { at, size, .. } => Some((*at, size.unwrap_or(level))),
        _ => None,
    }
}

/// A raster element's material, front and back:
/// `editor/layers/<id>.material.png`, `.front.png`, `.back.png`.
pub fn raster_files(id: &str) -> [String; 3] {
    ["material", "front", "back"].map(|part| format!("{EDITOR_DIR}/layers/{id}.{part}.png"))
}

/// The files `element` uses, by pack path.
pub fn element_files(element: &Element) -> Vec<String> {
    match &element.kind {
        ElementKind::Image { source, mask, .. } => {
            let mut out = vec![image_file(source)];
            if *mask {
                out.push(mask_file(&element.id));
            }
            out
        }
        ElementKind::Drawn { mixed, back, .. } => drawn_files(&element.id, *mixed, *back),
        ElementKind::Raster => raster_files(&element.id).into(),
    }
    .into_iter()
    .chain(element.glow.as_ref().is_some_and(|g| g.mask).then(|| glow_file(&element.id)))
    .collect()
}

/// An image or drawn element's box: its top-left and size in world pixels;
/// `None` for a raster (it has no box).
pub fn element_box(kind: &ElementKind, level: (u32, u32)) -> Option<((i32, i32), (u32, u32))> {
    match kind {
        ElementKind::Image { at, size, .. } => Some((*at, *size)),
        ElementKind::Drawn { .. } => drawn_box(kind, level),
        ElementKind::Raster => None,
    }
}

/// `COMPOSITION_FILE` and every element's files, each once, in element order.
pub fn composition_paths(src: &CompositionSource) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = vec![COMPOSITION_FILE.to_string()];
    for e in &src.composition.elements {
        for f in element_files(e) {
            if seen.insert(f.clone()) {
                out.push(f);
            }
        }
    }
    out
}

fn too_many(n: usize) -> Option<FormatError> {
    (n > MAX_ELEMENTS).then(|| {
        FormatError::invalid(
            COMPOSITION_FILE,
            "elements",
            format!("{n} elements: more than {MAX_ELEMENTS} elements"),
        )
    })
}

/// `editor/composition.ron` alone, without its elements' files: `None`
/// when the pack has none; more than `MAX_ELEMENTS` is an error.
pub fn read_composition(src: &dyn ContentSource) -> Result<Option<Composition>, FormatError> {
    let path = VPath::new(COMPOSITION_FILE).map_err(|e| io(COMPOSITION_FILE, e.to_string()))?;
    if !src.exists(&path) {
        return Ok(None);
    }
    let composition: Composition = from_canonical(COMPOSITION_FILE, &read_text(src, COMPOSITION_FILE)?)?;
    if let Some(e) = too_many(composition.elements.len()) {
        return Err(e);
    }
    Ok(Some(composition))
}

/// `None` when the pack has no `editor/composition.ron`. Reads every
/// referenced file (at most `MAX_FILE_BYTES` each); refuses a missing one.
pub fn read_composition_files(src: &dyn ContentSource) -> Result<Option<CompositionSource>, FormatError> {
    let Some(composition) = read_composition(src)? else { return Ok(None) };
    let mut files = BTreeMap::new();
    for e in &composition.elements {
        for file in element_files(e) {
            if files.contains_key(&file) {
                continue;
            }
            let path = VPath::new(&file).map_err(|e| io(&file, e.to_string()))?;
            let bytes = src.read_max(&path, MAX_FILE_BYTES).map_err(|e| io(&file, e.to_string()))?;
            files.insert(file, bytes);
        }
    }
    Ok(Some(CompositionSource { composition, files }))
}

fn io(file: &str, message: String) -> FormatError {
    FormatError::Io { file: file.into(), message }
}

/// `composition.ron` in canonical form, then every element's file as it is
/// (files the elements do not name are not written; a missing one is
/// skipped, and the next read refuses it).
pub fn write_composition_files(
    sink: &mut dyn ContentSink,
    src: &CompositionSource,
) -> Result<(), FormatError> {
    // Ids and sources name files: a bad one is refused before anything
    // is written.
    for (i, e) in src.composition.elements.iter().enumerate() {
        if !crate::mods::valid_id(&e.id) {
            return Err(FormatError::invalid(
                COMPOSITION_FILE,
                &format!("elements[{i}].id"),
                format!("{:?} is not a valid id", e.id),
            ));
        }
        if let ElementKind::Image { source, .. } = &e.kind
            && !valid_source(source)
        {
            return Err(FormatError::invalid(
                COMPOSITION_FILE,
                &format!("elements[{i}].source"),
                format!("{source:?} is not one portable file name ending .png, .jpg or .jpeg"),
            ));
        }
    }
    write_text(sink, COMPOSITION_FILE, &to_canonical(COMPOSITION_FILE, &src.composition)?)?;
    for file in composition_paths(src).into_iter().skip(1) {
        if let Some(bytes) = src.files.get(&file) {
            let path = VPath::new(&file).map_err(|e| io(&file, e.to_string()))?;
            sink.write(&path, bytes).map_err(|e| io(&file, e.to_string()))?;
        }
    }
    Ok(())
}

/// Whether `source` is one portable file name ending `.png`, `.jpg` or
/// `.jpeg` (any case).
fn valid_source(source: &str) -> bool {
    let ext_ok = source.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && ["png", "jpg", "jpeg"].iter().any(|e| ext.eq_ignore_ascii_case(e))
    });
    ext_ok
        && !source.contains(['/', '\\'])
        && !source.starts_with('.')
        && olr_vfs::non_portable(source).is_none()
        && VPath::new(&image_file(source)).is_ok()
}

/// Everything checkable without decoding pixels: `check_elements`, then
/// that every file is present, and each file's size from its header
/// (`size` a level of `art_scale`).
pub fn check_composition(
    src: &CompositionSource,
    size: (u32, u32),
    art_scale: u32,
) -> Result<(), FormatError> {
    let mut pixels = check_elements(&src.composition, size, art_scale)?;
    let art = (size.0.saturating_mul(art_scale), size.1.saturating_mul(art_scale));
    for (i, e) in src.composition.elements.iter().enumerate() {
        let boxed = drawn_box(&e.kind, size);
        let glow = glow_file(&e.id);
        if e.glow.as_ref().is_some_and(|g| g.mask) {
            let Some(bytes) = src.files.get(&glow) else {
                return Err(FormatError::invalid(
                    COMPOSITION_FILE,
                    &format!("elements[{i}]"),
                    format!("{glow}: missing"),
                ));
            };
            let b = element_box(&e.kind, size).map_or((0, 0), |(_, b)| b);
            let expected = (b.0.saturating_mul(art_scale), b.1.saturating_mul(art_scale));
            check_file(&glow, bytes, Some((expected, "the element's box × art_scale")))?;
        }
        for (part, file) in element_files(e).into_iter().filter(|f| *f != glow).enumerate() {
            let Some(bytes) = src.files.get(&file) else {
                return Err(FormatError::invalid(
                    COMPOSITION_FILE,
                    &format!("elements[{i}]"),
                    format!("{file}: missing"),
                ));
            };
            // A raster's files are its material, then its front and back;
            // a drawn layer's its art, then its material and back art.
            let expected = match (&e.kind, boxed) {
                (ElementKind::Image { .. }, _) => None,
                (ElementKind::Raster, _) if part == 0 => Some((size, "the level's size")),
                (ElementKind::Drawn { mixed: true, .. }, Some((_, b))) if part == 1 => {
                    Some((b, "the layer's size"))
                }
                (ElementKind::Drawn { .. }, Some((_, b))) => Some((
                    (b.0.saturating_mul(art_scale), b.1.saturating_mul(art_scale)),
                    "the layer's size × art_scale",
                )),
                _ => Some((art, "the level's size × art_scale")),
            };
            let header = check_file(&file, bytes, expected)?;
            // A mask is decoded with its element: its pixels count too.
            if part == 1 && matches!(e.kind, ElementKind::Image { .. }) {
                pixels = pixels.saturating_add(u64::from(header.0) * u64::from(header.1));
                if pixels > MAX_COMPOSED_PIXELS {
                    return Err(FormatError::invalid(
                        COMPOSITION_FILE,
                        &format!("elements[{i}]"),
                        format!(
                            "the elements and masks up to this one decode to {pixels} pixels, more than {MAX_COMPOSED_PIXELS}"
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Every glow of `c` names a group of `look` (a level's `look`): an error
/// on `composition.ron` naming the first that does not.
pub fn check_glow_groups(c: &Composition, look: Option<&crate::look::LookSource>) -> Result<(), FormatError> {
    for (i, e) in c.elements.iter().enumerate() {
        let Some(g) = &e.glow else { continue };
        if !look.is_some_and(|l| l.glow_groups.iter().any(|x| x.id == g.group)) {
            return Err(FormatError::invalid(
                COMPOSITION_FILE,
                &format!("elements[{i}].glow.group"),
                format!("no glow group {:?} in level.ron's look", g.group),
            ));
        }
    }
    Ok(())
}

/// `check_composition` without the files: the count, ids (valid, unique,
/// their layer files unshared and portable), image fields, sources unique
/// case aside, each box's art within `LEVEL_ART`, the pixel budget (its
/// total, masks aside, returned).
pub fn check_elements(c: &Composition, size: (u32, u32), art_scale: u32) -> Result<u64, FormatError> {
    let elements = &c.elements;
    if let Some(e) = too_many(elements.len()) {
        return Err(e);
    }
    let art = (size.0.saturating_mul(art_scale), size.1.saturating_mul(art_scale));
    let mut ids = BTreeSet::new();
    let mut layers = BTreeMap::new();
    // Each original's name folded to lowercase, with its spelling.
    let mut sources: BTreeMap<String, &str> = BTreeMap::new();
    let mut pixels = 0u64;
    let art_pixels = u64::from(art.0) * u64::from(art.1);
    for (i, e) in elements.iter().enumerate() {
        let at = |field: &str| format!("elements[{i}].{field}");
        let bad = |field: &str, message: String| FormatError::invalid(COMPOSITION_FILE, &at(field), message);
        if !crate::mods::valid_id(&e.id) {
            return Err(bad("id", format!("{:?} is not a valid id", e.id)));
        }
        if !ids.insert(e.id.as_str()) {
            return Err(bad("id", format!("duplicate id {:?}", e.id)));
        }
        if let ElementKind::Image { source, size, pixel_size, colours, .. } = &e.kind {
            if !valid_source(source) {
                return Err(bad(
                    "source",
                    format!("{source:?} is not one portable file name ending .png, .jpg or .jpeg"),
                ));
            }
            match sources.insert(source.to_ascii_lowercase(), source) {
                Some(other) if other != source => {
                    return Err(bad(
                        "source",
                        format!("{source:?} and {other:?} differ only in case: one file on some systems"),
                    ));
                }
                _ => {}
            }
            if !(1..=MAX_SIDE).contains(&size.0) || !(1..=MAX_SIDE).contains(&size.1) {
                return Err(bad("size", format!("{}x{}: each side 1 to {MAX_SIDE}", size.0, size.1)));
            }
            let (aw, ah) = (size.0.saturating_mul(art_scale), size.1.saturating_mul(art_scale));
            image_io::check_size(aw, ah, LEVEL_ART)
                .map_err(|why| bad("size", format!("its art at art scale {art_scale}: {why}")))?;
            if !(1..=MAX_PIXEL_SIZE).contains(pixel_size) {
                return Err(bad("pixel_size", format!("{pixel_size}: 1 to {MAX_PIXEL_SIZE}")));
            }
            if let Some(n) = colours
                && !(2..=256).contains(n)
            {
                return Err(bad("colours", format!("{n}: 2 to 256")));
            }
        }
        if let ElementKind::Drawn { size: Some(size), .. } = &e.kind {
            if !(1..=MAX_SIDE).contains(&size.0) || !(1..=MAX_SIDE).contains(&size.1) {
                return Err(bad("size", format!("{}x{}: each side 1 to {MAX_SIDE}", size.0, size.1)));
            }
            let (aw, ah) = (size.0.saturating_mul(art_scale), size.1.saturating_mul(art_scale));
            image_io::check_size(aw, ah, LEVEL_ART)
                .map_err(|why| bad("size", format!("its art at art scale {art_scale}: {why}")))?;
        }
        pixels = pixels.saturating_add(match &e.kind {
            ElementKind::Image { size, pixel_size, .. } => {
                let side = |v: u32| u64::from(v.saturating_mul(art_scale).div_ceil((*pixel_size).max(1)));
                side(size.0) * side(size.1)
            }
            ElementKind::Drawn { size: boxed, mixed, back, .. } => {
                let (w, h) = boxed.unwrap_or(size);
                let cells = u64::from(w) * u64::from(h);
                let art = cells * u64::from(art_scale) * u64::from(art_scale);
                art * (1 + u64::from(*back)) + if *mixed { cells } else { 0 }
            }
            ElementKind::Raster => 2 * art_pixels + u64::from(size.0) * u64::from(size.1),
        });
        if pixels > MAX_COMPOSED_PIXELS {
            return Err(FormatError::invalid(
                COMPOSITION_FILE,
                &format!("elements[{i}]"),
                format!(
                    "the elements up to this one decode to {pixels} pixels, more than {MAX_COMPOSED_PIXELS}"
                ),
            ));
        }
        if let Some(g) = &e.glow {
            if matches!(e.kind, ElementKind::Raster) {
                return Err(bad("glow", "a raster element cannot glow: split it into layers".into()));
            }
            if !(1..=100).contains(&g.strength) {
                return Err(bad("glow.strength", format!("{}: 1 to 100", g.strength)));
            }
            if !crate::look::valid_group_id(&g.group) {
                return Err(bad("glow.group", format!("{:?} is not a glow group id", g.group)));
            }
            let file = glow_file(&e.id);
            let name = file.rsplit('/').next().unwrap_or(&file);
            if let Some(why) = olr_vfs::non_portable(name) {
                return Err(bad("id", format!("{:?}: its file {file} {why}", e.id)));
            }
        }
        // An image's mask is named by its unique id: never shared, but its
        // name must be portable (plan 14d review).
        if let ElementKind::Image { mask: true, .. } = e.kind {
            let file = mask_file(&e.id);
            let name = file.rsplit('/').next().unwrap_or(&file);
            if let Some(why) = olr_vfs::non_portable(name) {
                return Err(bad("id", format!("{:?}: its file {file} {why}", e.id)));
            }
        }
        // Other kinds' files are checked for sharing.
        if !matches!(e.kind, ElementKind::Image { .. }) {
            for file in element_files(e) {
                // Ids are unique, their files need not be (`a` and `a.front`).
                if let Some(j) = layers.insert(file.clone(), i) {
                    return Err(bad("id", format!("{file} is used by two elements (elements[{j}])")));
                }
                let name = file.rsplit('/').next().unwrap_or(&file);
                if let Some(why) = olr_vfs::non_portable(name) {
                    return Err(bad("id", format!("{:?}: its file {file} {why}", e.id)));
                }
            }
        }
    }
    Ok(pixels)
}

/// One element file's size from its header: an original within
/// `IMAGES` (`expected` none), a layer of exactly `expected`, a size and
/// what it is (a raster's material the level's size, other layers its
/// size × `art_scale`).
fn check_file(
    file: &str,
    bytes: &[u8],
    expected: Option<((u32, u32), &str)>,
) -> Result<(u32, u32), FormatError> {
    let invalid = |message: String| FormatError::invalid(file, "", message);
    let Some(((ew, eh), what)) = expected else {
        let (w, h) = image_io::image_size(bytes).ok_or_else(|| invalid("not a PNG or JPEG".into()))?;
        image_io::check_size(w, h, IMAGES).map_err(invalid)?;
        return Ok((w, h));
    };
    let (w, h) = image_io::png_size(bytes).ok_or_else(|| invalid("not a PNG".into()))?;
    if (w, h) != (ew, eh) {
        return Err(invalid(format!("{w}x{h}, {what} is {ew}x{eh}")));
    }
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use olr_data::image::RgbaImage;
    use olr_vfs::MemFs;

    use super::*;
    use crate::levels::MATERIAL_PALETTE;

    fn sample() -> CompositionSource {
        let art = image_io::encode_rgba(&RgbaImage::new(8, 6)); // a 4 × 3 level at art scale 2
        let material = image_io::encode_indexed(4, 3, &[2; 12], &MATERIAL_PALETTE);
        let mut files = BTreeMap::new();
        files.insert(image_file("sky.png"), image_io::encode_rgba(&RgbaImage::new(10, 10)));
        files.insert(layer_file("ground"), art.clone());
        files.insert(mask_file("sky"), image_io::encode_grey(10, 10, &[255; 100]));
        let [wall_art, wall_material, wall_back] =
            [0, 1, 2].map(|i| drawn_files("wall", true, true)[i].clone());
        files.insert(wall_art, image_io::encode_rgba(&RgbaImage::new(6, 4)));
        files.insert(wall_material, image_io::encode_indexed(3, 2, &[2; 6], &MATERIAL_PALETTE));
        files.insert(wall_back, image_io::encode_rgba(&RgbaImage::new(6, 4)));
        for (f, b) in raster_files("base").into_iter().zip([material, art.clone(), art]) {
            files.insert(f, b);
        }
        let element = |id: &str, kind| Element {
            id: id.into(),
            name: String::new(),
            visible: true,
            locked: false,
            kind,
            glow: None,
        };
        CompositionSource {
            composition: Composition {
                elements: vec![
                    element("base", ElementKind::Raster),
                    element(
                        "sky",
                        ElementKind::Image {
                            material: ElementMaterial::Background,
                            source: "sky.png".into(),
                            at: (-1, 0),
                            size: (4, 3),
                            pixel_size: 1,
                            colours: Some(16),
                            dither: Dither::Ordered,
                            alpha_threshold: 128,
                            mask: true,
                        },
                    ),
                    element("ground", ElementKind::drawn(ElementMaterial::Dirt)),
                    element(
                        "wall",
                        ElementKind::Drawn {
                            material: ElementMaterial::Rock,
                            at: (1, -1),
                            size: Some((3, 2)),
                            mixed: true,
                            back: true,
                        },
                    ),
                ],
            },
            files,
        }
    }

    #[test]
    fn composition_round_trips_canonically() {
        let src = sample();
        let mut fs = MemFs::new();
        write_composition_files(&mut fs, &src).unwrap();
        let back = read_composition_files(&fs).unwrap().unwrap();
        assert_eq!(back, src);
        let text = String::from_utf8(fs.read(&VPath::new(COMPOSITION_FILE).unwrap()).unwrap()).unwrap();
        assert!(text.contains("colours: Some(16)") || text.contains("colours: 16"), "{text}");
        assert!(!text.contains("visible"), "defaults are left out: {text}");
        assert_eq!(crate::canonical_text(COMPOSITION_FILE, &text).unwrap(), text);
        let mut paths = composition_paths(&src);
        paths.sort();
        assert_eq!(paths, fs.files().map(|(p, _)| p.as_str().to_string()).collect::<Vec<_>>());
        check_composition(&src, (4, 3), 2).unwrap();
    }

    #[test]
    fn no_composition_reads_as_none() {
        assert_eq!(read_composition_files(&MemFs::new()).unwrap(), None);
    }

    #[test]
    #[allow(clippy::type_complexity)]
    fn check_composition_reports_each_problem() {
        let cases: Vec<(&str, Box<dyn Fn(&mut CompositionSource)>)> = vec![
            ("duplicate id", Box::new(|s| s.composition.elements[2].id = "sky".into())),
            ("not a valid id", Box::new(|s| s.composition.elements[2].id = "Bad Id".into())),
            (
                "missing",
                Box::new(|s| {
                    s.files.remove(&layer_file("ground"));
                }),
            ),
            (
                "art_scale",
                Box::new(|s| {
                    s.files.insert(layer_file("ground"), image_io::encode_rgba(&RgbaImage::new(4, 3)));
                }),
            ),
            (
                "pixel_size",
                Box::new(|s| {
                    if let ElementKind::Image { pixel_size, .. } = &mut s.composition.elements[1].kind {
                        *pixel_size = 65
                    }
                }),
            ),
            (
                "colours",
                Box::new(|s| {
                    if let ElementKind::Image { colours, .. } = &mut s.composition.elements[1].kind {
                        *colours = Some(1)
                    }
                }),
            ),
            (
                "size",
                Box::new(|s| {
                    if let ElementKind::Image { size, .. } = &mut s.composition.elements[1].kind {
                        *size = (0, 3)
                    }
                }),
            ),
            (
                "source",
                Box::new(|s| {
                    if let ElementKind::Image { source, .. } = &mut s.composition.elements[1].kind {
                        *source = "../x.png".into()
                    }
                }),
            ),
            ("size", Box::new(|s| set_wall_size(s, (0, 2)))),
            ("size", Box::new(|s| set_wall_size(s, (5000, 2)))),
            (
                "editor/layers/wall.material.png",
                Box::new(|s| {
                    s.files.insert(
                        drawn_files("wall", true, false)[1].clone(),
                        image_io::encode_indexed(2, 2, &[2; 4], &MATERIAL_PALETTE),
                    );
                }),
            ),
            (
                "editor/layers/wall.back.png",
                Box::new(|s| {
                    s.files.insert(
                        drawn_files("wall", false, true)[1].clone(),
                        image_io::encode_rgba(&RgbaImage::new(4, 4)),
                    );
                }),
            ),
            (
                "missing",
                Box::new(|s| {
                    s.files.remove("editor/layers/wall.material.png");
                }),
            ),
            (
                "editor/masks/sky.png",
                Box::new(|s| {
                    s.files.insert(mask_file("sky"), b"not an image".to_vec());
                }),
            ),
            (
                "more than",
                Box::new(|s| {
                    let e = s.composition.elements[0].clone();
                    s.composition.elements = vec![e; MAX_ELEMENTS + 1];
                }),
            ),
        ];
        for (want, edit) in cases {
            let mut s = sample();
            edit(&mut s);
            let e = check_composition(&s, (4, 3), 2).unwrap_err().to_string();
            assert!(e.contains(want), "{want}: {e}");
        }
    }

    fn set_wall_size(s: &mut CompositionSource, to: (u32, u32)) {
        if let ElementKind::Drawn { size, .. } = &mut s.composition.elements[3].kind {
            *size = Some(to);
        }
    }

    /// Plan 14d: a drawn layer's defaults, and an image's, are left out.
    #[test]
    fn defaults_are_left_out_of_drawn_and_image() {
        let mut s = sample();
        s.composition.elements.retain(|e| e.id == "ground");
        let text = to_canonical(COMPOSITION_FILE, &s.composition).unwrap();
        assert!(text.contains("Drawn(\n                material: Dirt,\n            )"), "{text}");
        for field in ["at:", "size:", "mixed", "back", "mask"] {
            assert!(!text.contains(field), "{field}: {text}");
        }
    }

    /// Plan 14d: a mixed layer's material file is one of its files, so
    /// another layer's art may not share its name.
    #[test]
    fn a_drawn_layers_files_never_collide() {
        let mut s = sample();
        s.composition.elements.retain(|e| e.id == "ground");
        if let ElementKind::Drawn { mixed, .. } = &mut s.composition.elements[0].kind {
            *mixed = true;
        }
        s.composition.elements[0].id = "a".into();
        let mut other = s.composition.elements[0].clone();
        other.id = "a.material".into();
        other.kind = ElementKind::drawn(ElementMaterial::Dirt);
        s.composition.elements.push(other);
        let e = check_elements(&s.composition, (4, 3), 2).unwrap_err().to_string();
        assert!(e.contains("two elements"), "{e}");
    }

    /// Review A-1: a file's size rule comes from the element, not from
    /// the file name's suffix, and two elements never share a layer file.
    #[test]
    fn layer_files_follow_their_element_and_never_collide() {
        let mut s = sample();
        s.composition.elements[2].id = "x.material".into();
        let art = s.files.remove(&layer_file("ground")).unwrap();
        s.files.insert(layer_file("x.material"), art);
        check_composition(&s, (4, 3), 2).unwrap();
        let mut s = sample();
        s.composition.elements[2].id = "base.front".into();
        let art = s.files.remove(&layer_file("ground")).unwrap();
        s.files.insert(layer_file("base.front"), art);
        let e = check_composition(&s, (4, 3), 2).unwrap_err().to_string();
        assert!(e.contains("editor/layers/base.front.png") && e.contains("two elements"), "{e}");
    }

    /// Review A-1 (re-graded): an id whose files no file system can hold
    /// on every platform (a Windows device name) is refused by the check,
    /// not only when the save writes it.
    #[test]
    fn an_id_naming_a_device_is_refused() {
        let mut s = sample();
        s.composition.elements[2].id = "aux".into();
        let art = s.files.remove(&layer_file("ground")).unwrap();
        s.files.insert(layer_file("aux"), art);
        let e = check_composition(&s, (4, 3), 2).unwrap_err().to_string();
        assert!(e.contains("elements[2].id") && e.contains("device"), "{e}");
    }

    /// Plan 14d review: an image's mask file is checked as layer files are.
    #[test]
    fn a_mask_named_for_a_device_is_refused() {
        let mut s = sample();
        s.composition.elements[1].id = "con".into();
        let mask = s.files.remove(&mask_file("sky")).unwrap();
        s.files.insert(mask_file("con"), mask);
        let e = check_composition(&s, (4, 3), 2).unwrap_err().to_string();
        assert!(e.contains("elements[1].id") && e.contains("device"), "{e}");
    }

    /// Batch B review: the elements' decoded pixels together stay within
    /// `MAX_COMPOSED_PIXELS`, so a small `composition.ron` cannot make a
    /// bake hold or process unbounded memory.
    #[test]
    fn the_elements_pixels_together_are_bounded() {
        let mut s = sample();
        s.composition.elements.retain(|e| e.id == "sky");
        if let ElementKind::Image { size, mask, .. } = &mut s.composition.elements[0].kind {
            *size = (4096, 4096);
            *mask = false;
        }
        let one = s.composition.elements[0].clone();
        s.composition.elements = (0..4).map(|i| Element { id: format!("e{i}"), ..one.clone() }).collect();
        check_composition(&s, (4, 3), 2).unwrap();
        s.composition.elements.push(Element { id: "e4".into(), ..one });
        let e = check_composition(&s, (4, 3), 2).unwrap_err().to_string();
        assert!(e.contains("elements[4]") && e.contains("pixels"), "{e}");
        // Plan 14d: a drawn layer counts its art, its back art when backed
        // and its cells when mixed. Three boxes of 4096 × 2048 at art scale
        // 2 (32 Mi art pixels each) with backs and cells come to 3 × (64 Mi
        // + 8 Mi): within 256 Mi; a fourth is not.
        let wall = |i: usize| Element {
            id: format!("w{i}"),
            name: String::new(),
            visible: true,
            locked: false,
            kind: ElementKind::Drawn {
                material: ElementMaterial::Dirt,
                at: (0, 0),
                size: Some((4096, 2048)),
                mixed: true,
                back: true,
            },
            glow: None,
        };
        // A mask's pixels count too: at the limit, a 10 × 10 mask is over.
        let mut at_limit = sample();
        at_limit.composition.elements = s.composition.elements[..4].to_vec();
        if let ElementKind::Image { mask, .. } = &mut at_limit.composition.elements[0].kind {
            *mask = true;
        }
        at_limit.files.insert(mask_file("e0"), image_io::encode_grey(10, 10, &[255; 100]));
        let e = check_composition(&at_limit, (4, 3), 2).unwrap_err().to_string();
        assert!(e.contains("masks"), "{e}");
        let mut c = Composition { elements: (0..3).map(wall).collect() };
        check_elements(&c, (4, 3), 2).unwrap();
        c.elements.push(wall(3));
        let e = check_elements(&c, (4, 3), 2).unwrap_err().to_string();
        assert!(e.contains("elements[3]") && e.contains("pixels"), "{e}");
    }

    /// Deferred minors: two originals whose names differ only in case
    /// would be one file on Windows and macOS.
    #[test]
    fn sources_alike_but_for_case_are_refused() {
        let mut s = sample();
        let mut e = s.composition.elements[1].clone();
        e.id = "sky2".into();
        if let ElementKind::Image { source, .. } = &mut e.kind {
            *source = "SKY.png".into();
        }
        s.files.insert(image_file("SKY.png"), image_io::encode_rgba(&RgbaImage::new(2, 2)));
        s.files.insert(mask_file("sky2"), image_io::encode_grey(10, 10, &[255; 100]));
        s.composition.elements.push(e.clone());
        let err = check_composition(&s, (4, 3), 2).unwrap_err().to_string();
        assert!(err.contains("elements[4].source") && err.contains("case"), "{err}");
        // The same original twice is one file: allowed.
        if let ElementKind::Image { source, .. } = &mut s.composition.elements[4].kind {
            *source = "sky.png".into();
        }
        check_composition(&s, (4, 3), 2).unwrap();
    }

    /// Deferred minors: a source that is no portable file name is refused
    /// before anything is written.
    #[test]
    fn write_refuses_a_bad_source_before_writing() {
        let mut s = sample();
        if let ElementKind::Image { source, .. } = &mut s.composition.elements[1].kind {
            *source = "a/b.png".into();
        }
        let mut fs = MemFs::new();
        let e = write_composition_files(&mut fs, &s).unwrap_err().to_string();
        assert!(e.contains("elements[1].source"), "{e}");
        assert_eq!(fs.files().count(), 0);
    }

    /// Deferred minors: a size error names its file once.
    #[test]
    fn a_size_error_names_its_file_once() {
        let mut s = sample();
        s.files.insert(layer_file("ground"), image_io::encode_rgba(&RgbaImage::new(4, 3)));
        let e = check_composition(&s, (4, 3), 2).unwrap_err().to_string();
        assert_eq!(e.matches("editor/layers/ground.png").count(), 1, "{e}");
    }

    /// `read_composition` reads `composition.ron` alone: a missing element
    /// file is not its concern.
    #[test]
    fn read_composition_reads_the_text_alone() {
        let mut fs = MemFs::new();
        let mut src = sample();
        src.files.remove(&layer_file("ground"));
        write_composition_files(&mut fs, &src).unwrap();
        assert_eq!(read_composition(&fs).unwrap(), Some(src.composition));
        assert_eq!(read_composition(&MemFs::new()).unwrap(), None);
    }

    /// Spec §2: an image's processed art (its box × art scale) is within
    /// `LEVEL_ART`, so the bake never allocates more than a level's art.
    #[test]
    fn an_image_box_beyond_level_art_is_refused() {
        let mut s = sample();
        s.composition.elements.retain(|e| e.id == "sky");
        if let ElementKind::Image { size, .. } = &mut s.composition.elements[0].kind {
            *size = (4096, 4096);
        }
        check_composition(&s, (4, 3), 2).unwrap();
        let e = check_composition(&s, (4, 3), 3).unwrap_err().to_string();
        assert!(e.contains("elements[0].size") && e.contains("art"), "{e}");
    }

    #[test]
    fn a_hostile_composition_is_an_error_never_a_panic() {
        for text in
            ["", "(", "(elements: [(id: 3)])", "(elements: [(id: \"a\", kind: Image())])", "\u{0}\u{ff}"]
        {
            let fs = MemFs::new().with(COMPOSITION_FILE, text);
            assert!(read_composition_files(&fs).is_err(), "{text:?}");
        }
    }

    #[test]
    fn a_missing_file_is_a_read_error() {
        let mut fs = MemFs::new();
        let mut src = sample();
        src.files.remove(&image_file("sky.png"));
        write_composition_files(&mut fs, &src).unwrap();
        let e = read_composition_files(&fs).unwrap_err().to_string();
        assert!(e.contains("editor/images/sky.png"), "{e}");
    }
}
