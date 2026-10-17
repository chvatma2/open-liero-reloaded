//! Element operations on a composed level (plan 14a, spec §7): add,
//! set, remove and reorder elements, and paint into drawn layers. Each
//! works on a level [`Pack`] and leaves its composition valid for
//! `check_composition`; a refused operation changes nothing. None bakes:
//! the caller bakes (`compose::bake`) before saving.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_format::image_io::{self, IMAGES, LEVEL_ART};
use olr_format::levels::{
    CompositionSource, Dither, Element, ElementKind, ElementMaterial, LevelSource, MATERIAL_PALETTE,
    MAX_ELEMENTS, check_composition, element_files, image_file, layer_file, raster_files,
};

use crate::compose::compose_from;
use crate::level::{Paint, Shape, art_block, covered};
use crate::pack::{Pack, Source};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ElementError {
    #[error("not a level pack")]
    NotALevel,
    #[error("no element {0:?}")]
    NoElement(String),
    #[error("element {0:?} is locked")]
    Locked(String),
    #[error("element {id:?} is not {want}")]
    WrongKind { id: String, want: &'static str },
    #[error("a level holds at most {} elements", MAX_ELEMENTS)]
    TooMany,
    #[error("{0}")]
    BadValue(String),
}

/// A settable property of an element.
#[derive(Clone, Debug, PartialEq)]
pub enum Property {
    Name(String),
    Visible(bool),
    Locked(bool),
    Material(ElementMaterial),
    At((i32, i32)),
    Size((u32, u32)),
    PixelSize(u32),
    Colours(Option<u16>),
    Dither(Dither),
    Threshold(u8),
}

fn level(pack: &Pack) -> Result<&LevelSource, ElementError> {
    match &pack.source {
        Source::Level(l) => Ok(l),
        _ => Err(ElementError::NotALevel),
    }
}

/// Gives a level without a composition its raster base (`compose_from`); no-op otherwise.
pub fn ensure_composed(pack: &mut Pack) -> Result<(), ElementError> {
    let base = compose_from(level(pack)?);
    pack.composition.get_or_insert(base);
    Ok(())
}

/// Applies `edit` to a copy of the pack's composition and keeps the copy
/// only when it still checks: the composition is never left invalid. A
/// check error is `BadValue`, prefixed by `context` when given.
fn commit<T>(
    pack: &mut Pack,
    context: &str,
    edit: impl FnOnce(&mut CompositionSource) -> Result<T, ElementError>,
) -> Result<T, ElementError> {
    let l = level(pack)?;
    let (size, art_scale) = (l.size, l.art_scale);
    let mut src = pack.composition.clone().unwrap_or_else(|| compose_from(l));
    let out = edit(&mut src)?;
    check_composition(&src, size, art_scale).map_err(|e| {
        ElementError::BadValue(if context.is_empty() { e.to_string() } else { format!("{context}: {e}") })
    })?;
    pack.composition = Some(src);
    Ok(out)
}

/// The element `id`, by index.
fn find(src: &CompositionSource, id: &str) -> Result<usize, ElementError> {
    src.composition.elements.iter().position(|e| e.id == id).ok_or_else(|| ElementError::NoElement(id.into()))
}

/// `id` (found) when it is not locked.
fn unlocked(src: &CompositionSource, id: &str) -> Result<usize, ElementError> {
    let i = find(src, id)?;
    if src.composition.elements[i].locked {
        return Err(ElementError::Locked(id.into()));
    }
    Ok(i)
}

/// `base` made a slug (ruling R8): lowercase, every character outside
/// `[a-z0-9_.-]` a `-`, not starting with `.`; `fallback` when empty.
fn slug(base: &str, fallback: &str) -> String {
    let s: String = base
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-') { c } else { '-' }
        })
        .collect();
    let s = s.trim_start_matches('.').to_string();
    if s.is_empty() { fallback.into() } else { s }
}

/// `base`, or `base-2`, `base-3`, … : the first that is not `taken`.
fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    std::iter::once(base.to_string())
        .chain((2u32..).map(|i| format!("{base}-{i}")))
        .find(|c| !taken(c))
        .unwrap_or_default()
}

/// A new element id from `name` (ruling R8): not an id in use, and its
/// files (`<id>.png` and a raster's) neither another element's nor a
/// name some file system refuses.
fn new_id(src: &CompositionSource, name: &str, fallback: &str) -> String {
    let used: std::collections::BTreeSet<String> =
        src.composition.elements.iter().flat_map(element_files).chain(src.files.keys().cloned()).collect();
    unique(&slug(name, fallback), |c| {
        let mut files = vec![layer_file(c)];
        files.extend(raster_files(c));
        src.composition.elements.iter().any(|e| e.id == c)
            || files.iter().any(|f| {
                used.contains(f) || olr_vfs::non_portable(f.rsplit('/').next().unwrap_or(f)).is_some()
            })
    })
}

/// `file_name` with `-2`, `-3`, … before its extension until no stored
/// original has that name, case aside (ruling R7).
fn new_source(src: &CompositionSource, file_name: &str) -> String {
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    let taken = |c: &str| {
        let path = image_file(c).to_ascii_lowercase();
        src.files.keys().any(|k| k.to_ascii_lowercase() == path)
    };
    if !taken(file_name) {
        return file_name.into();
    }
    (2u32..).map(|i| format!("{stem}-{i}.{ext}")).find(|c| !taken(c)).unwrap_or_default()
}

fn push(src: &mut CompositionSource, element: Element) -> Result<(), ElementError> {
    if src.composition.elements.len() >= MAX_ELEMENTS {
        return Err(ElementError::TooMany);
    }
    src.composition.elements.push(element);
    Ok(())
}

/// Adds an image element on top; `file_name` is the original's (ruling R7). Returns its id.
pub fn add_image(
    pack: &mut Pack,
    file_name: &str,
    bytes: Vec<u8>,
    at: (i32, i32),
    size: (u32, u32),
    material: ElementMaterial,
    name: &str,
) -> Result<String, ElementError> {
    let bad = |why: String| ElementError::BadValue(format!("{file_name}: {why}"));
    let (w, h) = image_io::image_size(&bytes).ok_or_else(|| bad("not a PNG or JPEG".into()))?;
    image_io::check_size(w, h, IMAGES).map_err(bad)?;
    image_io::decode_image_within(&bytes, IMAGES).map_err(bad)?;
    commit(pack, file_name, |src| {
        let source = new_source(src, file_name);
        let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
        let id = new_id(src, if name.is_empty() { stem } else { name }, "image");
        src.files.insert(image_file(&source), bytes);
        push(
            src,
            Element {
                id: id.clone(),
                name: name.into(),
                visible: true,
                locked: false,
                kind: ElementKind::Image {
                    material,
                    source,
                    at,
                    size,
                    pixel_size: 1,
                    colours: None,
                    dither: Dither::None,
                    alpha_threshold: 128,
                },
            },
        )?;
        Ok(id)
    })
}

/// Adds an empty drawn layer on top. Returns its id.
pub fn add_layer(pack: &mut Pack, material: ElementMaterial, name: &str) -> Result<String, ElementError> {
    let l = level(pack)?;
    let empty = image_io::encode_rgba(&RgbaImage::new(l.size.0 * l.art_scale, l.size.1 * l.art_scale));
    commit(pack, "", |src| {
        let id = new_id(src, name, "layer");
        src.files.insert(layer_file(&id), empty);
        push(
            src,
            Element {
                id: id.clone(),
                name: name.into(),
                visible: true,
                locked: false,
                kind: ElementKind::Drawn { material },
            },
        )?;
        Ok(id)
    })
}

/// The name of a property, as `composition.ron` spells its field.
fn field(p: &Property) -> &'static str {
    match p {
        Property::Name(_) => "name",
        Property::Visible(_) => "visible",
        Property::Locked(_) => "locked",
        Property::Material(_) => "material",
        Property::At(_) => "at",
        Property::Size(_) => "size",
        Property::PixelSize(_) => "pixel_size",
        Property::Colours(_) => "colours",
        Property::Dither(_) => "dither",
        Property::Threshold(_) => "alpha_threshold",
    }
}

/// Sets `property` of element `id`, validated as `check_composition` does.
/// A locked element takes only `Locked(false)`.
pub fn set(pack: &mut Pack, id: &str, property: Property) -> Result<(), ElementError> {
    let context = field(&property);
    commit(pack, context, |src| {
        let i = find(src, id)?;
        let e = &mut src.composition.elements[i];
        if e.locked && property != Property::Locked(false) {
            return Err(ElementError::Locked(id.into()));
        }
        let wrong = |want| ElementError::WrongKind { id: id.into(), want };
        match property {
            Property::Name(n) => e.name = n,
            Property::Visible(v) => e.visible = v,
            Property::Locked(v) => e.locked = v,
            Property::Material(m) => match &mut e.kind {
                ElementKind::Image { material, .. } | ElementKind::Drawn { material } => *material = m,
                ElementKind::Raster => return Err(wrong("an image or drawn element")),
            },
            p => {
                let ElementKind::Image { at, size, pixel_size, colours, dither, alpha_threshold, .. } =
                    &mut e.kind
                else {
                    return Err(wrong("an image element"));
                };
                match p {
                    Property::At(v) => *at = v,
                    Property::Size(v) => *size = v,
                    Property::PixelSize(v) => *pixel_size = v,
                    Property::Colours(v) => *colours = v,
                    Property::Dither(v) => *dither = v,
                    Property::Threshold(v) => *alpha_threshold = v,
                    _ => {}
                }
            }
        }
        Ok(())
    })
}

/// Removes element `id` and the files no other element uses.
pub fn remove(pack: &mut Pack, id: &str) -> Result<(), ElementError> {
    commit(pack, "", |src| {
        let i = unlocked(src, id)?;
        let e = src.composition.elements.remove(i);
        for f in element_files(&e) {
            if !src.composition.elements.iter().any(|o| element_files(o).contains(&f)) {
                src.files.remove(&f);
            }
        }
        Ok(())
    })
}

/// Moves element `id` one step up; the top one stays.
pub fn raise(pack: &mut Pack, id: &str) -> Result<(), ElementError> {
    commit(pack, "", |src| {
        let i = find(src, id)?;
        if i + 1 < src.composition.elements.len() {
            src.composition.elements.swap(i, i + 1);
        }
        Ok(())
    })
}

/// Moves element `id` one step down; the bottom one stays.
pub fn lower(pack: &mut Pack, id: &str) -> Result<(), ElementError> {
    commit(pack, "", |src| {
        let i = find(src, id)?;
        if i > 0 {
            src.composition.elements.swap(i, i - 1);
        }
        Ok(())
    })
}

/// Drawn layer `id`'s art, decoded, given to `paint`, and stored again.
fn paint_layer(
    pack: &mut Pack,
    id: &str,
    paint: impl FnOnce(&mut RgbaImage, (u32, u32), u32),
) -> Result<(), ElementError> {
    let l = level(pack)?;
    let (size, s) = (l.size, l.art_scale);
    commit(pack, "", |src| {
        let i = unlocked(src, id)?;
        if !matches!(src.composition.elements[i].kind, ElementKind::Drawn { .. }) {
            return Err(ElementError::WrongKind { id: id.into(), want: "a drawn element" });
        }
        let file = layer_file(id);
        let bytes = src.files.get(&file).ok_or_else(|| ElementError::BadValue(format!("{file}: missing")))?;
        let mut art = image_io::decode_rgba_within(bytes, LEVEL_ART)
            .map_err(|why| ElementError::BadValue(format!("{file}: {why}")))?;
        if (art.width(), art.height()) != (size.0 * s, size.1 * s) {
            return Err(ElementError::BadValue(format!("{file}: not the level's art size")));
        }
        paint(&mut art, size, s);
        src.files.insert(file, image_io::encode_rgba(&art));
        Ok(())
    })
}

/// Paints `shape` (world pixels, whole art blocks) into drawn layer `id`,
/// opaque: each art pixel takes the paint's RGB (a transparent pixel of a
/// tile leaves it).
pub fn draw(pack: &mut Pack, id: &str, shape: &Shape, paint: &Paint) -> Result<(), ElementError> {
    paint_layer(pack, id, |art, size, s| {
        covered(shape, size, |x, y| {
            for (ax, ay) in art_block(s, x, y) {
                if let Some(c) = paint.at(ax, ay).filter(|c| c.a != 0) {
                    art.set(ax, ay, Rgba { a: 255, ..c });
                }
            }
        })
    })
}

/// Clears `shape` in drawn layer `id` (alpha 0).
pub fn erase(pack: &mut Pack, id: &str, shape: &Shape) -> Result<(), ElementError> {
    paint_layer(pack, id, |art, size, s| {
        covered(shape, size, |x, y| {
            for (ax, ay) in art_block(s, x, y) {
                art.set(ax, ay, Rgba::TRANSPARENT);
            }
        })
    })
}

/// A level source holding `front` and `back` (and `material`) at the
/// pack's size, for `level::resize` to re-lay.
fn as_level(l: &LevelSource, material: Vec<Material>, front: RgbaImage, back: RgbaImage) -> LevelSource {
    LevelSource { objects: Vec::new(), material, front, back, thumb: None, ..l.clone() }
}

/// Resizes the composition with the level (as `level::resize`): image boxes
/// move by `offset`; drawn layers are re-laid (the new area uncovered);
/// raster layers are re-laid with `material` and `paint` in the new area.
/// Reads the old size from the pack's level source, so callers run it
/// before `level::resize`. A level without a composition is left as it is.
pub fn resize(
    pack: &mut Pack,
    size: (u32, u32),
    offset: (i32, i32),
    material: Material,
    paint: &Paint,
) -> Result<(), ElementError> {
    let l = level(pack)?.clone();
    let Some(mut src) = pack.composition.clone() else { return Ok(()) };
    // Every layer the re-lay reads is the level's size (a hand edit may
    // have broken that).
    check_composition(&src, l.size, l.art_scale).map_err(|e| ElementError::BadValue(e.to_string()))?;
    let bad = |file: &str, why: String| ElementError::BadValue(format!("{file}: {why}"));
    let relay =
        |mut tmp: LevelSource, paint: &Paint, material: Material| -> Result<LevelSource, ElementError> {
            crate::level::resize(&mut tmp, size, offset, material, paint)
                .map_err(|e| ElementError::BadValue(e.to_string()))?;
            Ok(tmp)
        };
    let cells = vec![Material::Background; l.size.0 as usize * l.size.1 as usize];
    for e in &mut src.composition.elements {
        match &mut e.kind {
            ElementKind::Image { at, .. } => {
                let moved = (i64::from(at.0) + i64::from(offset.0), i64::from(at.1) + i64::from(offset.1));
                *at = (
                    i32::try_from(moved.0).map_err(|_| bad(&e.id, "moved out of range".into()))?,
                    i32::try_from(moved.1).map_err(|_| bad(&e.id, "moved out of range".into()))?,
                );
            }
            ElementKind::Drawn { .. } => {
                let file = layer_file(&e.id);
                let bytes = src.files.get(&file).ok_or_else(|| bad(&file, "missing".into()))?;
                let art = image_io::decode_rgba_within(bytes, LEVEL_ART).map_err(|why| bad(&file, why))?;
                let tmp = relay(
                    as_level(&l, cells.clone(), art.clone(), art),
                    &Paint::Colour(Rgba::TRANSPARENT),
                    material,
                )?;
                src.files.insert(file, image_io::encode_rgba(&tmp.front));
            }
            ElementKind::Raster => {
                let [m, f, b] = raster_files(&e.id);
                let get = |file: &str| src.files.get(file).ok_or_else(|| bad(file, "missing".into()));
                let (_, _, indices) =
                    image_io::decode_indexed_within(get(&m)?, LEVEL_ART).map_err(|why| bad(&m, why))?;
                let cells = indices
                    .iter()
                    .map(|&i| Material::from_index(i).ok_or_else(|| bad(&m, format!("palette index {i}"))))
                    .collect::<Result<Vec<_>, _>>()?;
                let front = image_io::decode_rgba_within(get(&f)?, LEVEL_ART).map_err(|why| bad(&f, why))?;
                let back = image_io::decode_rgba_within(get(&b)?, LEVEL_ART).map_err(|why| bad(&b, why))?;
                let tmp = relay(as_level(&l, cells, front, back), paint, material)?;
                let indices: Vec<u8> = tmp.material.iter().map(|c| *c as u8).collect();
                src.files.insert(m, image_io::encode_indexed(size.0, size.1, &indices, &MATERIAL_PALETTE));
                src.files.insert(f, image_io::encode_rgba(&tmp.front));
                src.files.insert(b, image_io::encode_rgba(&tmp.back));
            }
        }
    }
    check_composition(&src, size, l.art_scale).map_err(|e| ElementError::BadValue(e.to_string()))?;
    pack.composition = Some(src);
    Ok(())
}
