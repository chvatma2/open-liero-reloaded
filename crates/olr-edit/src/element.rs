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
    CompositionSource, Dither, Element, ElementGlow, ElementKind, ElementMaterial, Folder, LevelSource,
    MAX_ELEMENTS, check_composition, check_elements, element_box, element_files, glow_file, image_file,
    layer_file, raster_files, surface_back_file, surface_file, surface_map_size,
};

use crate::compose::{Decoded, ImageCache, compose_from, decode_element, encode_decoded, resize_decoded};
use crate::drawn;
pub use crate::folders::new_folder_id;
use crate::folders::{self, FolderError};
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

impl From<FolderError> for ElementError {
    fn from(e: FolderError) -> Self {
        match e {
            FolderError::NoItem(id) => ElementError::NoElement(id),
            other => ElementError::BadValue(other.to_string()),
        }
    }
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
    /// A distant (parallax) layer of this depth, percent, or none (issue
    /// #11). Made distant, it moves to the top of the distant layers; made
    /// near again, to just above them.
    Distant(Option<u8>),
    /// Its pixels' surface material, the id of one of the look's
    /// materials, or none (issue #38).
    Surface(Option<String>),
    /// Its back art's surface material (an element with back art only).
    BackSurface(Option<String>),
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
    // An edit emptying a folder removes it; the folders stay in their
    // canonical order (issue #59).
    folders::tidy(&mut src.composition);
    fit_surface_maps(&mut src, size);
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

/// Keeps every element's surface materials fitting it in a level of
/// `size` (issue #37): a back surface and its map go with the back art,
/// a distant layer's painted maps go, and so does a painted map that is
/// missing or not of its element's [`surface_map_size`] (an edit the maps
/// did not follow).
pub fn fit_surface_maps(src: &mut CompositionSource, size: (u32, u32)) {
    for e in &mut src.composition.elements {
        if !e.has_back_art() {
            (e.back_surface, e.back_surface_map) = (None, false);
        }
        if e.is_distant() {
            (e.surface_map, e.back_surface_map) = (false, false);
        }
        let want = surface_map_size(&e.kind, size);
        let fits = |file: String| src.files.get(&file).and_then(|b| image_io::png_size(b)) == Some(want);
        e.surface_map &= fits(surface_file(&e.id));
        e.back_surface_map &= fits(surface_back_file(&e.id));
    }
}

/// A painted surface map (`values`, world pixels of box `from`) laid on
/// box `to`: world pixel `(x, y)` keeps its value, the rest 0 (the
/// element's default). Shared with the editor's resize.
pub fn relay_surface(values: &[u8], from: ((i32, i32), (u32, u32)), to: ((i32, i32), (u32, u32))) -> Vec<u8> {
    let (fw, fh) = (i64::from(from.1.0), i64::from(from.1.1));
    let (tw, th) = (i64::from(to.1.0), i64::from(to.1.1));
    let mut out = vec![0u8; (tw * th).max(0) as usize];
    if values.len() as i64 != fw * fh {
        return out;
    }
    let (dx, dy) = (i64::from(to.0.0) - i64::from(from.0.0), i64::from(to.0.1) - i64::from(from.0.1));
    for y in 0..th {
        let fy = y + dy;
        if !(0..fh).contains(&fy) {
            continue;
        }
        for x in 0..tw {
            let fx = x + dx;
            if (0..fw).contains(&fx) {
                out[(y * tw + x) as usize] = values[(fy * fw + fx) as usize];
            }
        }
    }
    out
}

/// The element `id`, by index.
fn find(src: &CompositionSource, id: &str) -> Result<usize, ElementError> {
    src.composition.elements.iter().position(|e| e.id == id).ok_or_else(|| ElementError::NoElement(id.into()))
}

/// `id` (found) when it is not locked, nor any folder holding it.
fn unlocked(src: &CompositionSource, id: &str) -> Result<usize, ElementError> {
    let i = find(src, id)?;
    if folders::locked(&src.composition, &src.composition.elements[i]) {
        return Err(ElementError::Locked(id.into()));
    }
    Ok(i)
}

/// The longest element id made from a name, in bytes: an id names files.
const MAX_ID: usize = 64;

/// `base` made a slug (ruling R8): lowercase, every character outside
/// `[a-z0-9_.-]` a `-`, not starting with `.`, at most `MAX_ID` bytes;
/// `fallback` when empty.
pub(crate) fn slug(base: &str, fallback: &str) -> String {
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
pub(crate) fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    std::iter::once(base.to_string())
        .chain((2u32..).map(|i| {
            let suffix = format!("-{i}");
            format!("{}{suffix}", &base[..base.len().min(MAX_ID - suffix.len())])
        }))
        .find(|c| !taken(c))
        .unwrap_or_default()
}

/// A new id from `name` (ruling R8 of plan 14a, at most 64 bytes) not
/// taken by `elements` or `folders` (issue #59: one namespace): not an id
/// in use, and its files (`<id>.png` and a raster's) neither another
/// element's, nor among `files` (the pack's others, stray ones included),
/// nor a name some file system refuses.
pub fn new_element_id(
    elements: &[Element],
    folders: &[Folder],
    files: &BTreeSet<String>,
    name: &str,
    fallback: &str,
) -> String {
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
            || folders.iter().any(|f| f.id == c)
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
    new_element_id(&src.composition.elements, &src.composition.folders, &files, name, fallback)
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
                folder: None,
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
                glass: None,
                distant: None,
                surface: None,
                back_surface: None,
                surface_map: false,
                back_surface_map: false,
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
                folder: None,
                kind: ElementKind::drawn(material),
                glow: None,
                clear: false,
                glass: None,
                distant: None,
                surface: None,
                back_surface: None,
                surface_map: false,
                back_surface_map: false,
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
        Property::Distant(_) => "distant",
        Property::Surface(_) => "surface",
        Property::BackSurface(_) => "back_surface",
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
    if pack.composition.as_ref().is_some_and(|c| c.composition.folders.iter().any(|f| f.id == id)) {
        return commit(pack, &context.join(", "), |src| {
            properties.iter().try_for_each(|p| set_folder(&mut src.composition, id, p))
        });
    }
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
        if let Property::Surface(Some(m)) | Property::BackSurface(Some(m)) = p {
            material_value(l, m).map_err(|e| ElementError::BadValue(format!("{}: {e}", field(p))))?;
        }
    }
    let s = l.art_scale;
    commit(pack, &context.join(", "), |src| {
        let was = find(src, id).ok().and_then(|i| element_box(&src.composition.elements[i].kind, size));
        for p in properties {
            apply(src, id, p.clone(), size)?;
        }
        // The new box within its bounds before any map is resampled to it.
        check_elements(&src.composition, size, s).map_err(|e| ElementError::BadValue(e.to_string()))?;
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
        // So do its painted surface maps, at world resolution (issue #37).
        if let (Some((_, from)), Some((_, to))) = (was, element_box(&e.kind, size))
            && from != to
        {
            for file in [surface_file(id), surface_back_file(id)] {
                let Some(bytes) = src.files.get(&file) else { continue };
                let Ok((w, h, values)) = image_io::decode_grey_within(bytes, olr_format::image_io::LEVEL_ART)
                else {
                    continue;
                };
                if (w, h) == from {
                    let resampled = crate::glow::resample_mask(&values, from, to);
                    src.files.insert(file, image_io::encode_grey(to.0, to.1, &resampled));
                }
            }
        }
        Ok(())
    })
}

/// Sets one property of folder `id` of `c` (issue #59): its name,
/// visibility or lock. A locked folder, or one in a locked folder, takes
/// only `Locked`.
fn set_folder(c: &mut olr_format::levels::Composition, id: &str, p: &Property) -> Result<(), ElementError> {
    if folders::folder_locked(c, id) && !matches!(p, Property::Locked(_)) {
        return Err(ElementError::Locked(id.into()));
    }
    let f = c.folders.iter_mut().find(|f| f.id == id).ok_or_else(|| ElementError::NoElement(id.into()))?;
    match p {
        Property::Name(n) => f.name = n.clone(),
        Property::Visible(v) => f.visible = *v,
        Property::Locked(v) => f.locked = *v,
        _ => {
            return Err(ElementError::WrongKind {
                id: id.into(),
                want: "an element (a folder takes a name, visibility and a lock)",
            });
        }
    }
    Ok(())
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
        let locked = folders::locked(&src.composition, &src.composition.elements[i]);
        let e = &mut src.composition.elements[i];
        if locked && !matches!(property, Property::Locked(_)) {
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
            Property::Surface(m) => e.surface = m,
            Property::BackSurface(_) if !e.has_back_art() => {
                return Err(wrong("an element with back art (a drawn layer with back art, a raster)"));
            }
            Property::BackSurface(m) => e.back_surface = m,
            Property::Distant(d) => {
                let was = e.distant.is_some();
                e.distant = d;
                if was != d.is_some() {
                    // It leaves its folders: distant layers share none with
                    // the others (issue #59).
                    e.folder = None;
                    let e = src.composition.elements.remove(i);
                    // The top of the distant layers, or just above them.
                    let at = src.composition.elements.iter().take_while(|x| x.is_distant()).count();
                    src.composition.elements.insert(at, e);
                }
            }
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

/// The map value naming material `id` of `level`'s look (1 + its index).
fn material_value(level: &LevelSource, id: &str) -> Result<u8, String> {
    level
        .look
        .iter()
        .flat_map(|l| l.materials.iter())
        .position(|m| m.id == id)
        .and_then(|i| u8::try_from(i + 1).ok())
        .ok_or_else(|| format!("no material {id:?} in level.ron's look (add it first)"))
}

/// Element `id` (unlocked, not distant; `back`: with back art) of `src`
/// and its painted surface map file (`back`: its back art's).
fn surface_target(src: &CompositionSource, id: &str, back: bool) -> Result<(usize, String), ElementError> {
    let i = unlocked(src, id)?;
    let e = &src.composition.elements[i];
    if e.is_distant() {
        return Err(ElementError::BadValue(format!(
            "element {id:?} is a distant layer: it takes one surface for the whole layer"
        )));
    }
    if back && !e.has_back_art() {
        return Err(ElementError::WrongKind {
            id: id.into(),
            want: "an element with back art (a drawn layer with back art, a raster)",
        });
    }
    Ok((i, if back { surface_back_file(id) } else { surface_file(id) }))
}

/// Writes element `i`'s painted surface map `file` (`back`: its back
/// art's) of `values` (its [`surface_map_size`]); all 0 removes it.
fn put_surface_map(
    src: &mut CompositionSource,
    i: usize,
    file: String,
    back: bool,
    values: &[u8],
    size: (u32, u32),
) {
    let e = &mut src.composition.elements[i];
    let (w, h) = surface_map_size(&e.kind, size);
    let on = values.iter().any(|v| *v > 0);
    if back {
        e.back_surface_map = on
    } else {
        e.surface_map = on
    }
    if on {
        src.files.insert(file, image_io::encode_grey(w, h, values));
    } else {
        src.files.remove(&file);
    }
}

/// Sets element `id`'s painted surface map (`back`: its back art's; issue
/// #38) to `values`, its box at world resolution (a raster's the level's
/// size), `v` naming the look's `materials[v - 1]` and 0 the element's
/// default; `None` or all 0 removes it. A value past the look's
/// materials, a map of another size, a locked or distant element, and a
/// back map without back art are refused.
pub fn set_surface_map(
    pack: &mut Pack,
    id: &str,
    back: bool,
    values: Option<&[u8]>,
) -> Result<(), ElementError> {
    let l = level(pack)?;
    let size = l.size;
    let n = l.look.as_ref().map_or(0, |l| l.materials.len());
    commit(pack, "", |src| {
        let (i, file) = surface_target(src, id, back)?;
        let want = surface_map_size(&src.composition.elements[i].kind, size);
        let values = match values {
            Some(v) => v,
            None => &[][..],
        };
        if !values.is_empty() && values.len() as u64 != u64::from(want.0) * u64::from(want.1) {
            return Err(ElementError::BadValue(format!(
                "{} values: the element's box in world pixels is {}x{}",
                values.len(),
                want.0,
                want.1
            )));
        }
        if let Some(k) = values.iter().position(|v| usize::from(*v) > n) {
            let (x, y) = (k as u32 % want.0.max(1), k as u32 / want.0.max(1));
            return Err(ElementError::BadValue(format!(
                "material {} at ({x}, {y}): the look has {n} materials",
                values[k]
            )));
        }
        put_surface_map(src, i, file, back, values, size);
        Ok(())
    })
}

/// One rule of [`surface_by_colour`]: the element's pixels of collision
/// kind `kind` (any when none) whose colour is `level::similar` to
/// `colour` within `tolerance` percent (any colour when none) are made of
/// `material` (an id of the look's materials; none: the element's
/// default).
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceRule {
    pub kind: Option<ElementMaterial>,
    pub colour: Option<olr_data::image::Rgba>,
    pub tolerance: u8,
    pub material: Option<String>,
}

/// Paints element `id`'s surface map (`back`: its back art's; issue #38)
/// by `rules`: each world pixel the element covers (its art's centre art
/// pixel not transparent) takes the material of the first rule its
/// collision kind and colour (that art pixel's; the back art's for
/// `back`) match; the others keep their value (0 where it had no map).
/// The same rules on the same element always paint the same map. Returns
/// how many pixels a rule matched.
pub fn surface_by_colour(
    pack: &mut Pack,
    id: &str,
    back: bool,
    rules: &[SurfaceRule],
) -> Result<u64, ElementError> {
    let l = level(pack)?;
    let (size, s) = (l.size, l.art_scale);
    let values = rules
        .iter()
        .map(|r| r.material.as_deref().map_or(Ok(0), |m| material_value(l, m)))
        .collect::<Result<Vec<u8>, String>>()
        .map_err(ElementError::BadValue)?;
    commit(pack, "", |src| {
        let bad = |e: String| ElementError::BadValue(e);
        let (i, file) = surface_target(src, id, back)?;
        let e = src.composition.elements[i].clone();
        let (w, h) = surface_map_size(&e.kind, size);
        let cells = w as usize * h as usize;
        let decoded =
            decode_element(src, &e, size, s, &mut ImageCache::new()).map_err(|e| bad(e.to_string()))?;
        // The art the colours are read from, and each world pixel's kind.
        let (art, kinds): (&RgbaImage, Box<dyn Fn(usize) -> Material>) = match (&e.kind, &decoded) {
            (ElementKind::Image { material, .. }, Decoded::Image { grid: Some(g), .. }) => {
                let m = Material::from(*material);
                (&**g, Box::new(move |_| m))
            }
            (ElementKind::Drawn { material, .. }, Decoded::Drawn { art, cells: c, back: b }) => {
                let m = Material::from(*material);
                let art = if back {
                    b.as_ref().ok_or_else(|| bad(format!("element {id:?} has no back art")))?
                } else {
                    art
                };
                (art, Box::new(move |k| c.as_ref().and_then(|c| c.get(k).copied()).unwrap_or(m)))
            }
            (ElementKind::Raster, Decoded::Raster { material, front, back: b }) => (
                if back { b } else { front },
                Box::new(move |k| material.get(k).copied().unwrap_or(Material::Background)),
            ),
            _ => return Err(bad(format!("element {id:?} has no pixels to read (a hidden picture?)"))),
        };
        if (u64::from(art.width()), u64::from(art.height()))
            != (u64::from(w) * u64::from(s), u64::from(h) * u64::from(s))
        {
            return Err(bad(format!("element {id:?}: its pixels are not its box's")));
        }
        let mut map =
            match src.files.get(&file).filter(|_| if back { e.back_surface_map } else { e.surface_map }) {
                Some(bytes) => {
                    let (mw, mh, v) = image_io::decode_grey_within(bytes, olr_format::image_io::LEVEL_ART)
                        .map_err(|m| bad(format!("{file}: {m}")))?;
                    if (mw, mh) != (w, h) {
                        return Err(bad(format!("{file}: {mw}x{mh}, the box is {w}x{h}")));
                    }
                    v
                }
                None => vec![0; cells],
            };
        let mut matched = 0u64;
        for y in 0..h {
            for x in 0..w {
                let c = art.get(x * s + s / 2, y * s + s / 2);
                if c.a == 0 {
                    continue;
                }
                let k = y as usize * w as usize + x as usize;
                let kind = kinds(k);
                let hit = rules.iter().position(|r| {
                    r.kind.is_none_or(|m| Material::from(m) == kind)
                        && r.colour.is_none_or(|rc| crate::level::similar(rc, c, r.tolerance))
                });
                if let Some(r) = hit {
                    map[k] = values[r];
                    matched += 1;
                }
            }
        }
        put_surface_map(src, i, file, back, &map, size);
        Ok(matched)
    })
}

/// Removes element `id` and the files no other element uses; a folder
/// with everything it holds (issue #59), refused while any of it is
/// locked.
pub fn remove(pack: &mut Pack, id: &str) -> Result<(), ElementError> {
    commit(pack, "", |src| {
        if src.composition.folders.iter().any(|f| f.id == id) {
            let c = &src.composition;
            if let Some(e) = c.elements.iter().find(|e| folders::holds(c, id, e) && folders::locked(c, e)) {
                return Err(ElementError::Locked(e.id.clone()));
            }
            for e in folders::remove_folder(&mut src.composition, id)? {
                drop_files(src, &e);
            }
            return Ok(());
        }
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

/// Why a move across the distant layers' edge is refused (issue #11).
pub const DISTANT_ORDER: &str = "distant layers stay below every other element";

/// Moves element or folder `id` one step up (`folders::raise`: a
/// folder past its neighbour, an element at a folder's top out of it);
/// the top one stays. A distant layer stays below every other element
/// (refused, [`DISTANT_ORDER`]).
pub fn raise(pack: &mut Pack, id: &str) -> Result<(), ElementError> {
    commit(pack, "", |src| Ok(folders::raise(&mut src.composition, id)?))
}

/// Moves element or folder `id` one step down (`folders::lower`); the
/// bottom one stays. An element that is not distant stays above the
/// distant layers (refused).
pub fn lower(pack: &mut Pack, id: &str) -> Result<(), ElementError> {
    commit(pack, "", |src| Ok(folders::lower(&mut src.composition, id)?))
}

/// Groups elements and folders `ids` into a new folder (issue #59,
/// `folders::group`), named `name` ("Folder n" when empty); returns its
/// id. Locked members group too.
pub fn group(pack: &mut Pack, ids: &[String], name: &str) -> Result<String, ElementError> {
    commit(pack, "", |src| Ok(folders::group(&mut src.composition, ids, name)?))
}

/// Ungroups folder `id` (`folders::ungroup`): its members go to its
/// parent, in order.
pub fn ungroup(pack: &mut Pack, id: &str) -> Result<(), ElementError> {
    commit(pack, "", |src| Ok(folders::ungroup(&mut src.composition, id)?))
}

/// Moves element or folder `id` into folder `into` (none: the top level)
/// at `position` among its children (`folders::move_to`: without one,
/// above the folder it leaves or on top of `into`).
pub fn move_to(
    pack: &mut Pack,
    id: &str,
    into: Option<&str>,
    position: Option<usize>,
) -> Result<(), ElementError> {
    commit(pack, "", |src| Ok(folders::move_to(&mut src.composition, id, into, position)?))
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
        if src.composition.elements[upper].is_distant() || src.composition.elements[lower].is_distant() {
            return Err(ElementError::BadValue("a distant layer does not merge".into()));
        }
        let (ld, ud) = (decoded(src, lower, size, s)?, decoded(src, upper, size, s)?);
        // The layers as the bake sees them: a hidden folder hides them.
        let c = &src.composition;
        let (le, ue) = (folders::fold(c, &c.elements[lower]), folders::fold(c, &c.elements[upper]));
        let (mut merged, d) = drawn::merge_down((&le, &ld), (&ue, &ud), size, s)
            .map_err(|e| ElementError::BadValue(e.to_string()))?;
        // It keeps the lower layer's folder (issue #59).
        merged.folder = le.folder.clone();
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
        if src.composition.elements[i].is_distant() {
            return Err(ElementError::BadValue("a distant layer does not split".into()));
        }
        let d = decoded(src, i, size, s)?;
        let e = folders::fold(&src.composition, &src.composition.elements[i]);
        let mut taken: Vec<Element> =
            src.composition.elements.iter().filter(|o| o.id != id).cloned().collect();
        let folder_list = src.composition.folders.clone();
        let parts = drawn::split_element(&e, &d, size, s, |name, fallback| {
            let new = new_element_id(&taken, &folder_list, &stray, name, fallback);
            // A raster's files cover a mixed layer's: none of them taken.
            taken.push(Element {
                id: new.clone(),
                name: String::new(),
                visible: true,
                locked: false,
                folder: None,
                kind: ElementKind::Raster,
                glow: None,
                clear: false,
                glass: None,
                distant: None,
                surface: None,
                back_surface: None,
                surface_map: false,
                back_surface_map: false,
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
        for (k, (mut part, data)) in parts.into_iter().enumerate() {
            // The parts take the split element's folder (issue #59).
            part.folder = e.folder.clone();
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
    // The new size within the level's bounds before any layer or map is
    // laid out at it.
    let (w, h) = size;
    if !(1..=olr_format::levels::MAX_SIDE).contains(&w) || !(1..=olr_format::levels::MAX_SIDE).contains(&h) {
        return Err(ElementError::BadValue(format!(
            "size {w}x{h}: 1 to {} a side",
            olr_format::levels::MAX_SIDE
        )));
    }
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
        // Painted surface maps (issue #37) follow it the same way at world
        // resolution, a raster's as the level's own pixels; the new area
        // takes the element's defaults.
        let from = was.unwrap_or(((0, 0), old));
        let to = element_box(&e.kind, size).unwrap_or(((0, 0), size));
        let moved = ((from.0.0.saturating_add(offset.0), from.0.1.saturating_add(offset.1)), from.1);
        for (file, on) in
            [(surface_file(&e.id), e.surface_map), (surface_back_file(&e.id), e.back_surface_map)]
        {
            let Some(bytes) = src.files.get(&file).filter(|_| on) else { continue };
            let (w, h, values) = image_io::decode_grey_within(bytes, olr_format::image_io::LEVEL_ART)
                .map_err(|m| bad(format!("{file}: {m}")))?;
            if (w, h) == from.1 {
                let relaid = relay_surface(&values, moved, to);
                src.files.insert(file, image_io::encode_grey(to.1.0, to.1.1, &relaid));
            }
        }
    }
    check_composition(&src, size, s).map_err(|e| ElementError::BadValue(e.to_string()))?;
    pack.composition = Some(src);
    Ok(())
}
