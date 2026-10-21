//! Element operations on a composed level (plan 14a, spec §7): add,
//! set, remove and reorder elements, and paint into drawn layers. Each
//! works on a level [`Pack`] and leaves its composition valid for
//! `check_composition`; a refused operation changes nothing. None bakes:
//! the caller bakes (`compose::bake`) before saving.

use std::collections::BTreeSet;

use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_format::image_io::{self, IMAGES};
use olr_format::levels::{
    CompositionSource, Dither, Element, ElementGlow, ElementKind, ElementMaterial, LevelSource, MAX_ELEMENTS,
    check_composition, element_box, element_files, glow_file, image_file, layer_file, raster_files,
};

use crate::compose::{Decoded, ImageCache, compose_from, decode_element, encode_decoded, resize_decoded};
use crate::drawn;
use crate::level::{Paint, Shape};
use crate::mask::{Mask, mask_colour, mask_shape, original_pixel};
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
    /// The element's glow (an image or drawn element; look system).
    Glow(Option<ElementGlow>),
    /// Its rock and dirt let light through (look system §Clear solids).
    Clear(bool),
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
/// check error is `BadValue`, prefixed by `context` when given and the
/// composition checked before the edit (an error already there is not the
/// edit's; an edit that repairs it still applies).
fn commit<T>(
    pack: &mut Pack,
    context: &str,
    edit: impl FnOnce(&mut CompositionSource) -> Result<T, ElementError>,
) -> Result<T, ElementError> {
    let l = level(pack)?;
    let (size, art_scale) = (l.size, l.art_scale);
    let was_valid = pack.composition.as_ref().is_none_or(|c| check_composition(c, size, art_scale).is_ok());
    let mut src = pack.composition.clone().unwrap_or_else(|| compose_from(l));
    let out = edit(&mut src)?;
    check_composition(&src, size, art_scale).map_err(|e| {
        ElementError::BadValue(if context.is_empty() || !was_valid {
            e.to_string()
        } else {
            format!("{context}: {e}")
        })
    })?;
    // Files no element names any more (a layer made plain) go.
    let named: BTreeSet<String> = olr_format::levels::composition_paths(&src).into_iter().collect();
    src.files.retain(|f, _| named.contains(f));
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

/// The longest element id made from a name, in bytes: an id names files.
const MAX_ID: usize = 64;

/// `base` made a slug (ruling R8): lowercase, every character outside
/// `[a-z0-9_.-]` a `-`, not starting with `.`, at most `MAX_ID` bytes;
/// `fallback` when empty.
fn slug(base: &str, fallback: &str) -> String {
    let s: String = base
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-') { c } else { '-' }
        })
        .collect();
    let mut s = s.trim_start_matches('.').to_string();
    s.truncate(MAX_ID);
    if s.is_empty() { fallback.into() } else { s }
}

/// `base`, or `base-2`, `base-3`, … (`base` shortened so the whole stays
/// within `MAX_ID` bytes): the first that is not `taken`.
fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    std::iter::once(base.to_string())
        .chain((2u32..).map(|i| {
            let suffix = format!("-{i}");
            format!("{}{suffix}", &base[..base.len().min(MAX_ID - suffix.len())])
        }))
        .find(|c| !taken(c))
        .unwrap_or_default()
}

/// A new id from `name` (ruling R8 of plan 14a, at most 64 bytes) not
/// taken by `elements`: not an id in use, and its files (`<id>.png` and a
/// raster's) neither another element's, nor among `files` (the pack's
/// others, stray ones included), nor a name some file system refuses.
pub fn new_element_id(elements: &[Element], files: &BTreeSet<String>, name: &str, fallback: &str) -> String {
    // Every file an element may come to hold: a drawn layer's material and
    // back art, an image's mask (plan 14d).
    let may_hold = |e: &Element| -> Vec<String> {
        let mut out = element_files(e);
        match &e.kind {
            ElementKind::Drawn { .. } => out.extend(olr_format::levels::drawn_files(&e.id, true, true)),
            ElementKind::Image { .. } => out.push(olr_format::levels::mask_file(&e.id)),
            ElementKind::Raster => {}
        }
        out
    };
    let used: BTreeSet<String> = elements.iter().flat_map(may_hold).chain(files.iter().cloned()).collect();
    unique(&slug(name, fallback), |c| {
        let mut files = vec![layer_file(c), olr_format::levels::mask_file(c)];
        files.extend(raster_files(c));
        elements.iter().any(|e| e.id == c)
            || files.iter().any(|f| {
                used.contains(f) || olr_vfs::non_portable(f.rsplit('/').next().unwrap_or(f)).is_some()
            })
    })
}

/// The pack's files outside the composition (in `other`): a stray file in
/// `editor/` a new element's file must not overwrite.
fn stray(pack: &Pack) -> BTreeSet<String> {
    pack.other.keys().map(|p| p.as_str().to_string()).collect()
}

fn new_id(src: &CompositionSource, stray: &BTreeSet<String>, name: &str, fallback: &str) -> String {
    let files: BTreeSet<String> = src.files.keys().chain(stray).cloned().collect();
    new_element_id(&src.composition.elements, &files, name, fallback)
}

/// `file_name`, or `-2`, `-3`, … before its extension, until it is not
/// among `sources` (the originals' names), case aside, and not a name some
/// file system refuses (a Windows device: `CON.png` becomes `CON-2.png`;
/// ruling R7 of plan 14a). A name no suffix makes portable (a colon) is
/// kept, for the check to refuse.
pub fn new_source_name<'a>(sources: impl Iterator<Item = &'a str>, file_name: &str) -> String {
    let (stem, ext) =
        file_name.rsplit_once('.').map_or((file_name, String::new()), |(s, e)| (s, format!(".{e}")));
    let names: BTreeSet<String> = sources.map(str::to_ascii_lowercase).collect();
    let used = |c: &str| names.contains(&c.to_ascii_lowercase());
    let taken = |c: &str| olr_vfs::non_portable(c).is_some() || used(c);
    if !taken(file_name) {
        return file_name.into();
    }
    // One more suffix than there are names is always free (a pack's
    // strays never make it run out, batch C review).
    let free =
        |base: &str| (2..=names.len() as u64 + 2).map(|i| format!("{base}-{i}{ext}")).find(|c| !taken(c));
    match free(stem) {
        Some(name) => name,
        // No suffix makes it portable: a name only unportable is kept, for
        // the check to refuse; a taken one is never returned (a device's
        // stem, `con.x.png`, takes a plain one).
        None if !used(file_name) => file_name.into(),
        None => free("image").unwrap_or_else(|| format!("image-{}{ext}", names.len() + 3)),
    }
}

fn new_source(src: &CompositionSource, stray: &BTreeSet<String>, file_name: &str) -> String {
    let dir = image_file("");
    new_source_name(src.files.keys().chain(stray).filter_map(|k| k.strip_prefix(dir.as_str())), file_name)
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
    add_image_with(pack, file_name, bytes, at, size, material, name, &[])
}

/// [`add_image`] with `settings` applied to the new element in the same
/// step, the result checked once: a pixel size that keeps the
/// composition within its pixel budget counts from the start.
#[allow(clippy::too_many_arguments)]
pub fn add_image_with(
    pack: &mut Pack,
    file_name: &str,
    bytes: Vec<u8>,
    at: (i32, i32),
    size: (u32, u32),
    material: ElementMaterial,
    name: &str,
    settings: &[Property],
) -> Result<String, ElementError> {
    let bad = |why: String| ElementError::BadValue(format!("{file_name}: {why}"));
    let (w, h) = image_io::image_size(&bytes).ok_or_else(|| bad("not a PNG or JPEG".into()))?;
    image_io::check_size(w, h, IMAGES).map_err(bad)?;
    image_io::decode_image_within(&bytes, IMAGES).map_err(bad)?;
    let stray = stray(pack);
    let level_size = level(pack)?.size;
    commit(pack, file_name, |src| {
        let source = new_source(src, &stray, file_name);
        let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
        let id = new_id(src, &stray, if name.is_empty() { stem } else { name }, "image");
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
                    mask: false,
                },
                glow: None,
                clear: false,
                distant: None,
            },
        )?;
        for p in settings {
            apply(src, &id, p.clone(), level_size)?;
        }
        Ok(id)
    })
}

/// Adds an empty drawn layer on top. Returns its id.
pub fn add_layer(pack: &mut Pack, material: ElementMaterial, name: &str) -> Result<String, ElementError> {
    let l = level(pack)?;
    let empty = image_io::encode_rgba(&RgbaImage::new(l.size.0 * l.art_scale, l.size.1 * l.art_scale));
    let stray = stray(pack);
    commit(pack, "", |src| {
        let id = new_id(src, &stray, name, "layer");
        src.files.insert(layer_file(&id), empty);
        push(
            src,
            Element {
                id: id.clone(),
                name: name.into(),
                visible: true,
                locked: false,
                kind: ElementKind::drawn(material),
                glow: None,
                clear: false,
                distant: None,
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
        Property::Glow(_) => "glow",
        Property::Clear(_) => "clear",
    }
}

/// Sets `property` of element `id`, validated as `check_composition` does.
/// A locked element takes only `Locked` (unlocking, or no change).
pub fn set(pack: &mut Pack, id: &str, property: Property) -> Result<(), ElementError> {
    set_all(pack, id, &[property])
}

/// Sets `properties` of element `id` in order, in one step: the result is
/// checked once, as `set` checks one.
pub fn set_all(pack: &mut Pack, id: &str, properties: &[Property]) -> Result<(), ElementError> {
    let context: Vec<&str> = properties.iter().map(field).collect();
    let l = level(pack)?;
    let size = l.size;
    for p in properties {
        if let Property::Glow(Some(g)) = p
            && !l.look.as_ref().is_some_and(|look| look.glow_groups.iter().any(|x| x.id == g.group))
        {
            return Err(ElementError::BadValue(format!(
                "glow: no glow group {:?} in level.ron's look (add it first)",
                g.group
            )));
        }
    }
    let s = l.art_scale;
    commit(pack, &context.join(", "), |src| {
        let was = find(src, id).ok().and_then(|i| element_box(&src.composition.elements[i].kind, size));
        for p in properties {
            apply(src, id, p.clone(), size)?;
        }
        // A painted glow mask follows a resized image, resampled as the
        // editor does (plan 15a's final review).
        let i = find(src, id)?;
        let e = &src.composition.elements[i];
        let file = glow_file(id);
        if let (Some(true), Some((_, from)), Some((_, to)), Some(bytes)) =
            (e.glow.as_ref().map(|g| g.mask), was, element_box(&e.kind, size), src.files.get(&file))
            && from != to
        {
            let (w, h, mask) = image_io::decode_grey_within(bytes, olr_format::image_io::LEVEL_ART)
                .map_err(|m| ElementError::BadValue(format!("{file}: {m}")))?;
            let (tw, th) = (to.0.saturating_mul(s), to.1.saturating_mul(s));
            let resampled = crate::glow::resample_mask(&mask, (w, h), (tw, th));
            src.files.insert(file, image_io::encode_grey(tw, th, &resampled));
        }
        Ok(())
    })
}

/// Sets one property of element `id` in `src` (a level of `size`), unchecked.
fn apply(
    src: &mut CompositionSource,
    id: &str,
    property: Property,
    size: (u32, u32),
) -> Result<(), ElementError> {
    {
        let i = find(src, id)?;
        let e = &mut src.composition.elements[i];
        if e.locked && !matches!(property, Property::Locked(_)) {
            return Err(ElementError::Locked(id.into()));
        }
        let wrong = |want| ElementError::WrongKind { id: id.into(), want };
        match property {
            Property::Glow(_) if matches!(e.kind, ElementKind::Raster) => {
                return Err(wrong("an image or drawn element"));
            }
            Property::Glow(g) => e.glow = g,
            Property::Clear(_) if matches!(e.kind, ElementKind::Raster) => {
                return Err(wrong("an image or drawn element (clear)"));
            }
            Property::Clear(c) => e.clear = c,
            Property::Name(n) => e.name = n,
            Property::Visible(v) => e.visible = v,
            Property::Locked(v) => e.locked = v,
            Property::Material(m) => match &mut e.kind {
                ElementKind::Image { material, .. } => *material = m,
                // A mixed layer becomes plain, all of `m` (plan 14d C4).
                ElementKind::Drawn { material, mixed, .. } => (*material, *mixed) = (m, false),
                ElementKind::Raster => return Err(wrong("an image or drawn element")),
            },
            // Moving a level-sized layer pins its box (batch A review I1).
            Property::At(v) if matches!(e.kind, ElementKind::Drawn { .. }) => {
                if let ElementKind::Drawn { at, size: boxed, .. } = &mut e.kind {
                    *at = v;
                    let b = boxed.unwrap_or(size);
                    // Back at the origin with the level's size, it follows
                    // the level again (batch B review 2).
                    *boxed = (v != (0, 0) || b != size).then_some(b);
                }
            }
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
    }
}

/// Paints element `id`'s glow mask over world `shape` (look system):
/// `strength` percent, 0 erasing. A first mask keeps everything (255). The
/// element must glow already.
pub fn paint_glow(pack: &mut Pack, id: &str, shape: &Shape, strength: u8) -> Result<(), ElementError> {
    let l = level(pack)?;
    let (size, s) = (l.size, l.art_scale);
    if strength > 100 {
        return Err(ElementError::BadValue(format!("strength {strength}: 0 to 100")));
    }
    commit(pack, "", |src| {
        let i = unlocked(src, id)?;
        let e = &src.composition.elements[i];
        let Some(glow) = e.glow.clone() else {
            return Err(ElementError::BadValue(format!("element {id:?} has no glow: set its glow first")));
        };
        let Some((at, b)) = element_box(&e.kind, size) else {
            return Err(ElementError::WrongKind { id: id.into(), want: "an image or drawn element" });
        };
        let art = (b.0.saturating_mul(s), b.1.saturating_mul(s));
        let file = glow_file(id);
        let mut mask = match src.files.get(&file).filter(|_| glow.mask) {
            Some(bytes) => {
                let (w, h, v) = image_io::decode_grey_within(bytes, olr_format::image_io::LEVEL_ART)
                    .map_err(|m| ElementError::BadValue(format!("{file}: {m}")))?;
                if (w, h) != art {
                    return Err(ElementError::BadValue(format!(
                        "{file}: {w}x{h}, the box is {}x{}",
                        art.0, art.1
                    )));
                }
                v
            }
            None => vec![255; art.0 as usize * art.1 as usize],
        };
        let value = ((u32::from(strength) * 255 + 50) / 100) as u8;
        crate::glow::paint_mask(&mut mask, at, b, s, shape, value);
        src.files.insert(file, image_io::encode_grey(art.0, art.1, &mask));
        src.composition.elements[i].glow = Some(ElementGlow { mask: true, ..glow });
        Ok(())
    })
}

/// Lights the connected pixels of element `id` alike in colour to the one
/// at world pixel `at` (`tolerance` percent) at `strength` percent of its
/// glow (the user's request, 2026-10-19). A glow without a mask starts
/// dark: only the region lights.
pub fn glow_colour(
    pack: &mut Pack,
    id: &str,
    at: (i32, i32),
    tolerance: u8,
    strength: u8,
) -> Result<(), ElementError> {
    let l = level(pack)?;
    let (size, s) = (l.size, l.art_scale);
    if strength > 100 || tolerance > 100 {
        return Err(ElementError::BadValue(format!("strength {strength}, tolerance {tolerance}: 0 to 100")));
    }
    commit(pack, "", |src| {
        let bad = |e: String| ElementError::BadValue(e);
        let i = unlocked(src, id)?;
        let e = src.composition.elements[i].clone();
        let Some(glow) = e.glow.clone() else {
            return Err(bad(format!("element {id:?} has no glow: set its glow first")));
        };
        let Some((origin, b)) = element_box(&e.kind, size) else {
            return Err(ElementError::WrongKind { id: id.into(), want: "an image or drawn element" });
        };
        let decoded =
            decode_element(src, &e, size, s, &mut ImageCache::new()).map_err(|e| bad(e.to_string()))?;
        let art = match &decoded {
            Decoded::Drawn { art, .. } => art,
            Decoded::Image { grid: Some(g), .. } => &**g,
            _ => return Err(bad(format!("element {id:?} has no pixels to pick"))),
        };
        let local = |v: i32, o: i32, side: u32| {
            let d = i64::from(v) - i64::from(o);
            (0..i64::from(side)).contains(&d).then(|| d as u32 * s + s / 2)
        };
        let seed = local(at.0, origin.0, b.0)
            .zip(local(at.1, origin.1, b.1))
            .ok_or_else(|| bad(format!("({}, {}) is outside {id:?}", at.0, at.1)))?;
        let region = crate::glow::colour_region(art, seed, tolerance)
            .ok_or_else(|| bad(format!("({}, {}) is outside {id:?}", at.0, at.1)))?;
        let (aw, ah) = (b.0.saturating_mul(s), b.1.saturating_mul(s));
        if (art.width(), art.height()) != (aw, ah) {
            return Err(bad(format!("element {id:?}: its pixels are not its box's")));
        }
        let file = glow_file(id);
        let mut mask = match src.files.get(&file).filter(|_| glow.mask) {
            Some(bytes) => {
                let (w, h, v) = image_io::decode_grey_within(bytes, olr_format::image_io::LEVEL_ART)
                    .map_err(|m| bad(format!("{file}: {m}")))?;
                if (w, h) != (aw, ah) {
                    return Err(bad(format!("{file}: {w}x{h}, the box is {aw}x{ah}")));
                }
                v
            }
            None => vec![0; aw as usize * ah as usize],
        };
        let value = ((u32::from(strength) * 255 + 50) / 100) as u8;
        for (m, _) in mask.iter_mut().zip(&region).filter(|(_, r)| **r) {
            *m = value;
        }
        src.files.insert(file, image_io::encode_grey(aw, ah, &mask));
        src.composition.elements[i].glow = Some(ElementGlow { mask: true, ..glow });
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

/// Element `i`'s data decoded (a level of `size` at art scale `s`).
fn decoded(src: &CompositionSource, i: usize, size: (u32, u32), s: u32) -> Result<Decoded, ElementError> {
    decode_element(src, &src.composition.elements[i], size, s, &mut ImageCache::new())
        .map_err(|e| ElementError::BadValue(e.to_string()))
}

/// Stores `element` with data `d` at index `i` (a level of `size`): its
/// flags follow the data (a drawn layer's `mixed` and `back`, an image's
/// `mask`), and its files are written.
fn store(src: &mut CompositionSource, i: usize, mut element: Element, d: &Decoded, size: (u32, u32)) {
    match (&mut element.kind, d) {
        (ElementKind::Drawn { mixed, back, .. }, Decoded::Drawn { cells, back: art, .. }) => {
            (*mixed, *back) = (cells.is_some(), art.is_some());
        }
        (ElementKind::Image { mask, .. }, Decoded::Image { mask: m, .. }) => *mask = m.is_some(),
        _ => {}
    }
    for (path, bytes) in encode_decoded(&element, d, size) {
        src.files.insert(path, bytes);
    }
    src.composition.elements[i] = element;
}

/// Removes the files of `e` (just taken out of `src`) no other element uses.
fn drop_files(src: &mut CompositionSource, e: &Element) {
    for f in element_files(e) {
        if !src.composition.elements.iter().any(|o| element_files(o).contains(&f)) {
            src.files.remove(&f);
        }
    }
}

/// Drawn layer `id` decoded, given to `paint` with its box's top-left,
/// the level's size, the art scale and the layer's material, and stored
/// again; an error from `paint` keeps the pack as it was.
fn paint_layer(
    pack: &mut Pack,
    id: &str,
    paint: impl FnOnce(&mut Decoded, (i32, i32), (u32, u32), u32, ElementMaterial) -> Result<(), ElementError>,
) -> Result<(), ElementError> {
    let l = level(pack)?;
    let (size, s) = (l.size, l.art_scale);
    commit(pack, "", |src| {
        let i = unlocked(src, id)?;
        let e = src.composition.elements[i].clone();
        let ElementKind::Drawn { material, .. } = e.kind else {
            return Err(ElementError::WrongKind { id: id.into(), want: "a drawn element" });
        };
        let mut d = decoded(src, i, size, s)?;
        let (at, _) = olr_format::levels::drawn_box(&e.kind, size).unwrap_or(((0, 0), size));
        paint(&mut d, at, size, s, material)?;
        store(src, i, e, &d, size);
        Ok(())
    })
}

/// Paints `shape` (world pixels, whole art blocks) into drawn layer `id`,
/// opaque: each art pixel takes the paint's RGB (a transparent pixel of a
/// tile leaves it). `material` other than a plain layer's makes it mixed
/// first (plan 14d R6); none is the layer's.
pub fn draw(
    pack: &mut Pack,
    id: &str,
    shape: &Shape,
    paint: &Paint,
    material: Option<ElementMaterial>,
) -> Result<(), ElementError> {
    paint_layer(pack, id, |d, at, size, s, layer| {
        let m = material.unwrap_or(layer);
        let plain = matches!(d, Decoded::Drawn { cells: None, .. });
        if m != layer {
            drawn::mix(d, s, layer.into());
        }
        drawn::draw(d, at, s, size, shape, paint, m.into());
        if plain {
            drawn::unmix_plain(d, layer.into());
        }
        Ok(())
    })
}

/// Flood-fills drawn layer `id` from world pixel `seed` (plan 14e R5,
/// `drawn::fill`) with `paint`; `material` as `draw` takes it. A seed
/// outside the layer's box or the level is refused.
pub fn fill(
    pack: &mut Pack,
    id: &str,
    seed: (i32, i32),
    paint: &Paint,
    tolerance: u8,
    material: Option<ElementMaterial>,
) -> Result<(), ElementError> {
    paint_layer(pack, id, |d, at, size, s, layer| {
        let (box_size, inside) = match d {
            Decoded::Drawn { art, .. } => ((art.width() / s.max(1), art.height() / s.max(1)), true),
            _ => ((0, 0), false),
        };
        let within = |v: i32, from: i32, side: u32, level: u32| {
            let v = i64::from(v);
            v >= i64::from(from).max(0) && v < (i64::from(from) + i64::from(side)).min(i64::from(level))
        };
        if !inside || !within(seed.0, at.0, box_size.0, size.0) || !within(seed.1, at.1, box_size.1, size.1) {
            return Err(ElementError::BadValue(format!("({}, {}) is outside {id:?}", seed.0, seed.1)));
        }
        let m = material.unwrap_or(layer);
        let plain = matches!(d, Decoded::Drawn { cells: None, .. });
        if m != layer {
            drawn::mix(d, s, layer.into());
        }
        drawn::fill(d, at, s, size, seed, paint, m.into(), tolerance);
        if plain {
            drawn::unmix_plain(d, layer.into());
        }
        Ok(())
    })
}

/// `name` with the next free ` N` suffix (N from 2) among `names`: a
/// trailing ` N` of `name` is dropped first ("Crate" → "Crate 2", "Crate 2"
/// → "Crate 3").
pub fn copy_name<'a>(name: &str, names: impl Iterator<Item = &'a str> + Clone) -> String {
    let base = match name.rsplit_once(' ') {
        Some((b, n)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => b,
        _ => name,
    };
    let with = |i: u32| if base.is_empty() { i.to_string() } else { format!("{base} {i}") };
    (2u32..).map(with).find(|c| !names.clone().any(|n| n == c)).unwrap_or_default()
}

/// Copies element `id` directly above it, at the same place (plan 14e
/// R10): a new id and a `copy_name`d name; an image shares its original,
/// and every file of its own (a layer's art, a mask) is copied under the
/// new id. Returns the new id.
pub fn duplicate(pack: &mut Pack, id: &str) -> Result<String, ElementError> {
    let stray = stray(pack);
    commit(pack, "", |src| {
        let i = find(src, id)?;
        if src.composition.elements.len() >= MAX_ELEMENTS {
            return Err(ElementError::TooMany);
        }
        let e = src.composition.elements[i].clone();
        let names = src
            .composition
            .elements
            .iter()
            .map(|o| if o.name.is_empty() { o.id.as_str() } else { o.name.as_str() });
        let name = copy_name(if e.name.is_empty() { &e.id } else { &e.name }, names);
        let new = new_id(src, &stray, &name, "copy");
        let copy = Element { id: new.clone(), name, ..e.clone() };
        for (from, to) in element_files(&e).into_iter().zip(element_files(&copy)) {
            if from != to {
                let bytes = src
                    .files
                    .get(&from)
                    .cloned()
                    .ok_or_else(|| ElementError::BadValue(format!("{from}: missing")))?;
                src.files.insert(to, bytes);
            }
        }
        src.composition.elements.insert(i + 1, copy);
        Ok(new)
    })
}

/// Erases `shape` in drawn layer `id` (plan 14d R5: dirt and rock over back
/// art are dug to it; the rest cleared).
pub fn erase(pack: &mut Pack, id: &str, shape: &Shape) -> Result<(), ElementError> {
    paint_layer(pack, id, |d, at, size, s, layer| {
        if matches!(d, Decoded::Drawn { back: Some(_), .. }) {
            drawn::mix(d, s, layer.into());
        }
        drawn::erase(d, at, s, size, shape);
        Ok(())
    })
}

/// Merges element `id` into the drawn layer directly below it (plan 14d
/// R10): one layer with the lower one's id, name and place.
pub fn merge_down(pack: &mut Pack, id: &str) -> Result<(), ElementError> {
    let l = level(pack)?;
    let (size, s) = (l.size, l.art_scale);
    commit(pack, "", |src| {
        let upper = unlocked(src, id)?;
        if upper == 0 {
            return Err(ElementError::BadValue(format!(
                "{id:?} is the bottom element: nothing to merge into"
            )));
        }
        let lower = upper - 1;
        unlocked(src, &src.composition.elements[lower].id.clone())?;
        let (ld, ud) = (decoded(src, lower, size, s)?, decoded(src, upper, size, s)?);
        let (le, ue) = (src.composition.elements[lower].clone(), src.composition.elements[upper].clone());
        let (merged, d) = drawn::merge_down((&le, &ld), (&ue, &ud), size, s)
            .map_err(|e| ElementError::BadValue(e.to_string()))?;
        let gone = src.composition.elements.remove(upper);
        drop_files(src, &gone);
        store(src, lower, merged, &d, size);
        Ok(())
    })
}

/// Splits element `id` into Background, Dirt and Rock layers in its place
/// (plan 14d R11); returns their ids, bottom to top.
pub fn split(pack: &mut Pack, id: &str) -> Result<Vec<String>, ElementError> {
    let l = level(pack)?;
    let (size, s) = (l.size, l.art_scale);
    let stray = stray(pack);
    commit(pack, "", |src| {
        let i = unlocked(src, id)?;
        let d = decoded(src, i, size, s)?;
        let e = src.composition.elements[i].clone();
        let mut taken: Vec<Element> =
            src.composition.elements.iter().filter(|o| o.id != id).cloned().collect();
        let parts = drawn::split_element(&e, &d, size, s, |name, fallback| {
            let new = new_element_id(&taken, &stray, name, fallback);
            // A raster's files cover a mixed layer's: none of them taken.
            taken.push(Element {
                id: new.clone(),
                name: String::new(),
                visible: true,
                locked: false,
                kind: ElementKind::Raster,
                glow: None,
                clear: false,
                distant: None,
            });
            new
        })
        .map_err(|e| ElementError::BadValue(e.to_string()))?;
        if parts.is_empty() {
            return Err(ElementError::BadValue(format!("{id:?} has no pixels to split")));
        }
        let gone = src.composition.elements.remove(i);
        drop_files(src, &gone);
        let mut ids = Vec::new();
        for (k, (part, data)) in parts.into_iter().enumerate() {
            ids.push(part.id.clone());
            src.composition.elements.insert(i + k, part.clone());
            store(src, i + k, part, &data, size);
        }
        Ok(ids)
    })
}

/// How `mask` masks an image.
#[derive(Clone, Debug, PartialEq)]
pub enum MaskOp {
    /// The original pixels under a world shape.
    Shape(Shape),
    /// The connected region of similar colour around world pixel `at`
    /// (`tolerance` percent, plan 14d R8).
    ColourAt { at: (i32, i32), tolerance: u8 },
}

/// Masks (or with `restore`, unmasks: shapes only) part of image element
/// `id`'s original (plan 14d R7, R8); its first mask keeps everything.
pub fn mask(pack: &mut Pack, id: &str, op: &MaskOp, restore: bool) -> Result<(), ElementError> {
    let size = level(pack)?.size;
    if restore && matches!(op, MaskOp::ColourAt { .. }) {
        return Err(ElementError::BadValue("--restore takes a shape".into()));
    }
    commit(pack, "", |src| {
        let i = unlocked(src, id)?;
        let e = src.composition.elements[i].clone();
        let ElementKind::Image { source, at, size: bsize, mask: has, .. } = &e.kind else {
            return Err(ElementError::WrongKind { id: id.into(), want: "an image element" });
        };
        let bad = |e: String| ElementError::BadValue(e);
        let file = image_file(source);
        let bytes = src.files.get(&file).ok_or_else(|| bad(format!("{file}: missing")))?;
        let original = ImageCache::new().original(source, bytes).map_err(|e| bad(e.to_string()))?;
        let osize = (original.width(), original.height());
        let mut m = match has {
            true => {
                let mf = olr_format::levels::mask_file(id);
                let mb = src.files.get(&mf).ok_or_else(|| bad(format!("{mf}: missing")))?;
                Mask::decode(&mf, mb, osize).map_err(|e| bad(e.to_string()))?
            }
            false => Mask::full(osize),
        };
        match op {
            MaskOp::Shape(shape) => {
                if mask_shape(&mut m, *at, *bsize, shape, restore).is_none() {
                    return Err(bad(format!("the shape misses {id:?}")));
                }
            }
            MaskOp::ColourAt { at: world, tolerance } => {
                let seed = original_pixel(*at, *bsize, osize, *world)
                    .ok_or_else(|| bad(format!("({}, {}) is outside the image", world.0, world.1)))?;
                mask_colour(&mut m, &original, seed, *tolerance);
            }
        }
        let d = Decoded::Image { grid: None, mask: Some(std::sync::Arc::new(m)) };
        let mut e = e.clone();
        if let ElementKind::Image { mask, .. } = &mut e.kind {
            *mask = true;
        }
        store(src, i, e, &d, size);
        Ok(())
    })
}

/// Resizes the composition with the level (as `level::resize`): image boxes
/// move by `offset`; drawn layers are re-laid (the new area uncovered);
/// raster layers are re-laid with `material` and `paint` in the new area
/// (`compose::resize_decoded`). Reads the old size from the pack's level
/// source, so callers run it before `level::resize`. A level without a
/// composition is left as it is.
pub fn resize(
    pack: &mut Pack,
    size: (u32, u32),
    offset: (i32, i32),
    material: Material,
    paint: &Paint,
) -> Result<(), ElementError> {
    let l = level(pack)?;
    let (old, s) = (l.size, l.art_scale);
    let Some(mut src) = pack.composition.clone() else { return Ok(()) };
    // Every layer the re-lay reads is the level's size (a hand edit may
    // have broken that).
    check_composition(&src, old, s).map_err(|e| ElementError::BadValue(e.to_string()))?;
    let bad = |e: String| ElementError::BadValue(e);
    let mut cache = ImageCache::new();
    let before = src.clone();
    for e in &mut src.composition.elements {
        let was = element_box(&e.kind, old);
        // An image's pixels are not re-laid: only its box moves.
        let mut d = match e.kind {
            ElementKind::Image { .. } => Decoded::Image { grid: None, mask: None },
            _ => decode_element(&before, e, old, s, &mut cache).map_err(|e| bad(e.to_string()))?,
        };
        resize_decoded(e, &mut d, old, s, size, offset, material, paint).map_err(|e| bad(e.to_string()))?;
        for (path, bytes) in encode_decoded(e, &d, size) {
            src.files.insert(path, bytes);
        }
        // A painted glow mask follows its layer's content as the editor's
        // does (plan 15a's final review): a level-sized layer's moves by
        // `offset` into its new box, a box keeps its own.
        let file = glow_file(&e.id);
        if let (Some(true), Some((at, b)), Some(new), Some(bytes)) =
            (e.glow.as_ref().map(|g| g.mask), was, element_box(&e.kind, size), src.files.get(&file))
        {
            let (w, h, mask) = image_io::decode_grey_within(bytes, olr_format::image_io::LEVEL_ART)
                .map_err(|m| bad(format!("{file}: {m}")))?;
            if (w, h) == (b.0.saturating_mul(s), b.1.saturating_mul(s)) {
                let moved = ((at.0.saturating_add(offset.0), at.1.saturating_add(offset.1)), b);
                let relaid = crate::glow::relay_mask(&mask, moved, new, s);
                let (nw, nh) = (new.1.0.saturating_mul(s), new.1.1.saturating_mul(s));
                src.files.insert(file, image_io::encode_grey(nw, nh, &relaid));
            }
        }
    }
    check_composition(&src, size, s).map_err(|e| ElementError::BadValue(e.to_string()))?;
    pack.composition = Some(src);
    Ok(())
}
