//! The bake (plan 14a, spec §3): a level's composition made into the
//! files the runtime reads, `material.png`, `front.png`, `back.png` and
//! `thumb.png`, whole or a rectangle at a time. Rulings R3-R6 of the plan:
//! - a world pixel's material is the topmost visible element covering the
//!   centre art pixel of its block (background when none covers it);
//! - an art pixel's front is the topmost visible element covering it, else
//!   its back;
//! - an art pixel's back is the topmost visible background image or drawn
//!   element covering it, or a raster element's back, else the theme's
//!   back tile repeated from the art origin;
//! - then `front := back` over every background block;
//! - baked art is opaque: an image or drawn element's colour keeps only
//!   its RGB.
//!
//! Integer arithmetic only: the same composition bakes to the same bytes
//! on every platform.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::PackKind;
use olr_format::image_io::{self, IMAGES, LEVEL_ART};
use olr_format::levels::{
    Composition, CompositionSource, Element, ElementKind, ElementMaterial, LevelSource, check_composition,
    encode_material, image_file, layer_file, mask_file, raster_files, write_level_files,
};
use olr_format::themes::read_theme_files;
use olr_vfs::{ContentSource, DirFs, MemFs, VPath};

use crate::level::{self, EditError, Paint, refresh_thumb};
use crate::mask::Mask;
use crate::pack::{Pack, Source};
use crate::pixelate::{Pixelation, process_grid};

pub use crate::level::Rect;

#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    /// An original, a layer or a raster file that does not decode, or of the wrong size.
    #[error("{file}: {message}")]
    File { file: String, message: String },
    #[error(transparent)]
    Format(#[from] olr_format::FormatError),
}

/// Originals decoded once per source, and processed grids per (source,
/// box art size, pixelation), at most `GRIDS_PER_SOURCE` per source and
/// `GRID_PIXELS` in all, the least recently used dropped (plan 14b ruling R3). Originals are kept
/// within `ORIGINAL_PIXELS`, the least recently used dropped. Each source's
/// entries carry its bytes' fingerprint: bytes that change under the same
/// name are decoded and processed afresh, never served stale.
#[derive(Default)]
pub struct ImageCache {
    /// Per source: its bytes' fingerprint, the decoded original, its last use.
    originals: BTreeMap<String, (u64, Arc<RgbaImage>, u64)>,
    /// Per source: its bytes' fingerprint and its grids, the most recently used last.
    grids: BTreeMap<String, (u64, Vec<Grid>)>,
    processed: usize,
    /// `original` and `grid` calls so far.
    lookups: usize,
    /// Uses so far: the originals' and grids' recency.
    tick: u64,
}

/// A processed grid's key besides its source: the box's art size, the
/// pixelation and the mask's fingerprint (0: none; plan 14d R7).
type GridKey = ((u32, u32), Pixelation, u64);

/// A processed grid with its key and its last use.
type Grid = (GridKey, Arc<RgbaImage>, u64);

/// Processed grids each source keeps.
pub const GRIDS_PER_SOURCE: usize = 4;

/// The decoded originals' pixels an `ImageCache` holds at most (the least
/// recently used dropped first; the newest is kept alone when larger).
pub const ORIGINAL_PIXELS: u64 = 4 * IMAGES.max_pixels;

/// The processed grids' pixels an `ImageCache` holds at most, all sources
/// together (the least recently used dropped first; the newest is kept
/// alone when larger).
pub const GRID_PIXELS: u64 = 4 * IMAGES.max_pixels;

/// `bytes`' length and FNV-1a hash, combined.
fn fingerprint(bytes: &[u8]) -> u64 {
    let hash = bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    hash ^ (bytes.len() as u64).rotate_left(32)
}

fn pixels(img: &RgbaImage) -> u64 {
    u64::from(img.width()) * u64::from(img.height())
}

impl ImageCache {
    pub fn new() -> ImageCache {
        ImageCache::default()
    }

    /// The decoded original (within `IMAGES`, EXIF-oriented) of `source`
    /// whose bytes are `bytes`, decoded on first use.
    pub fn original(&mut self, source: &str, bytes: &[u8]) -> Result<Arc<RgbaImage>, ComposeError> {
        self.lookups += 1;
        self.original_fp(source, bytes, fingerprint(bytes))
    }

    fn original_fp(&mut self, source: &str, bytes: &[u8], fp: u64) -> Result<Arc<RgbaImage>, ComposeError> {
        self.tick += 1;
        if let Some((f, img, used)) = self.originals.get_mut(source)
            && *f == fp
        {
            *used = self.tick;
            return Ok(img.clone());
        }
        let img = image_io::decode_image_within(bytes, IMAGES)
            .map_err(|message| ComposeError::File { file: image_file(source), message })?;
        let img = Arc::new(img);
        self.originals.insert(source.into(), (fp, img.clone(), self.tick));
        // Drop the least recently used others while over the budget.
        while self.original_pixels() > ORIGINAL_PIXELS {
            let oldest = self
                .originals
                .iter()
                .filter(|(k, _)| k.as_str() != source)
                .min_by_key(|(_, (_, _, used))| *used)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => self.originals.remove(&k),
                None => break,
            };
        }
        Ok(img)
    }

    /// The processed element grid (`pixelate::process_grid`) for `source`
    /// whose bytes are `original`, through `mask` when it has one (its
    /// erased pixels transparent first).
    pub fn grid(
        &mut self,
        source: &str,
        original: &[u8],
        art_box: (u32, u32),
        p: &Pixelation,
        mask: Option<&Mask>,
    ) -> Result<Arc<RgbaImage>, ComposeError> {
        self.lookups += 1;
        let fp = fingerprint(original);
        let key = (art_box, *p, mask.map_or(0, Mask::fingerprint));
        self.tick += 1;
        let tick = self.tick;
        let (f, grids) = self.grids.entry(source.into()).or_default();
        if *f != fp {
            (*f, *grids) = (fp, Vec::new());
        }
        if let Some(i) = grids.iter().position(|(k, ..)| *k == key) {
            let (k, grid, _) = grids.remove(i);
            grids.push((k, grid.clone(), tick));
            return Ok(grid);
        }
        let img = self.original_fp(source, original, fp)?;
        let grid = match mask {
            None => process_grid(&img, art_box, p),
            Some(m) => {
                let masked = m.apply(&img).ok_or_else(|| ComposeError::File {
                    file: image_file(source),
                    message: format!(
                        "a mask of {}x{} for an original of {}x{}",
                        m.size.0,
                        m.size.1,
                        img.width(),
                        img.height()
                    ),
                })?;
                process_grid(&masked, art_box, p)
            }
        };
        let grid = Arc::new(grid);
        self.processed += 1;
        let tick = self.tick;
        let (_, grids) = self.grids.entry(source.into()).or_default();
        if grids.len() >= GRIDS_PER_SOURCE {
            grids.remove(0);
        }
        grids.push((key, grid.clone(), tick));
        // Drop the least recently used others while over the budget.
        while self.grid_pixels() > GRID_PIXELS {
            let oldest = self
                .grids
                .iter()
                .flat_map(|(s, (_, g))| g.iter().map(move |(k, _, used)| (*used, s, *k)))
                .filter(|(used, ..)| *used != tick)
                .min_by_key(|(used, ..)| *used)
                .map(|(_, s, k)| (s.clone(), k));
            let Some((s, k)) = oldest else { break };
            if let Some((_, g)) = self.grids.get_mut(&s) {
                g.retain(|(gk, ..)| *gk != k);
            }
        }
        Ok(grid)
    }

    /// The processed grids' pixels held.
    pub fn grid_pixels(&self) -> u64 {
        self.grids.values().flat_map(|(_, g)| g).map(|(_, img, _)| pixels(img)).sum()
    }

    /// The decoded originals' pixels held.
    pub fn original_pixels(&self) -> u64 {
        self.originals.values().map(|(_, img, _)| pixels(img)).sum()
    }

    /// How many `original` and `grid` calls were made since `new` (each
    /// hashes the original's bytes; tests: a move looks nothing up).
    pub fn lookups(&self) -> usize {
        self.lookups
    }

    /// How many grids were processed since `new` (tests: a move never processes).
    pub fn processed(&self) -> usize {
        self.processed
    }

    /// Drops everything of sources not in `keep`.
    pub fn retain_sources(&mut self, keep: &BTreeSet<String>) {
        self.originals.retain(|k, _| keep.contains(k));
        self.grids.retain(|k, _| keep.contains(k));
    }
}

/// An element's data decoded for baking.
#[derive(Clone, Debug, PartialEq)]
pub enum Decoded {
    /// The processed element grid; `None`: hidden and not processed (its
    /// original decoded, for its errors). Its mask, when it has one.
    Image { grid: Option<Arc<RgbaImage>>, mask: Option<Arc<Mask>> },
    /// A level-sized art layer.
    Drawn { art: RgbaImage },
    /// A whole level.
    Raster { material: Vec<Material>, front: RgbaImage, back: RgbaImage },
}

impl Decoded {
    /// The memory held, in bytes (for the editor's history cap).
    pub fn bytes(&self) -> usize {
        let img = |i: &RgbaImage| i.as_raw().len();
        match self {
            Decoded::Image { grid, mask } => {
                grid.as_deref().map_or(0, img) + mask.as_deref().map_or(0, |m| m.keep.len())
            }
            Decoded::Drawn { art } => img(art),
            Decoded::Raster { material, front, back } => material.len() + img(front) + img(back),
        }
    }
}

/// One visible element as the bake reads it, borrowed from its element
/// and decoded data.
enum Layer<'a> {
    /// An image's element grid, placed at `origin` (art pixels) with
    /// `size` art pixels; art pixel `(x, y)` of the box is grid pixel
    /// `(x / pixel_size, y / pixel_size)`, covered where its alpha is 255.
    Image { material: Material, grid: &'a RgbaImage, pixel_size: u32, origin: (i64, i64), size: (i64, i64) },
    /// A level-sized art layer, covered where its alpha is not 0.
    Drawn { material: Material, art: &'a RgbaImage },
    /// A whole level: it covers everything.
    Raster { material: &'a [Material], front: &'a RgbaImage, back: &'a RgbaImage },
}

/// Opaque: the colour's RGB.
fn opaque(c: Rgba) -> Rgba {
    Rgba { a: 255, ..c }
}

impl<'a> Layer<'a> {
    /// The bake's view of a visible element, `None` when hidden, not
    /// processed, or (by a caller's mistake) of a size the level's art
    /// (`art`) or the element's box does not match: never a panic.
    fn new(e: &Element, d: &'a Decoded, art: (u32, u32), s: u32) -> Option<Layer<'a>> {
        if !e.visible {
            return None;
        }
        let dims = |i: &RgbaImage| (i.width(), i.height());
        Some(match (&e.kind, d) {
            (
                ElementKind::Image { material, at, size, pixel_size, .. },
                Decoded::Image { grid: Some(grid), .. },
            ) => {
                let ps = (*pixel_size).max(1);
                let box_art = (u64::from(size.0) * u64::from(s), u64::from(size.1) * u64::from(s));
                let need = (box_art.0.div_ceil(u64::from(ps)), box_art.1.div_ceil(u64::from(ps)));
                if (u64::from(grid.width()), u64::from(grid.height())) != need {
                    return None;
                }
                Layer::Image {
                    material: (*material).into(),
                    grid,
                    pixel_size: ps,
                    origin: (i64::from(at.0) * i64::from(s), i64::from(at.1) * i64::from(s)),
                    size: (box_art.0 as i64, box_art.1 as i64),
                }
            }
            (ElementKind::Drawn { material, .. }, Decoded::Drawn { art: a, .. }) if dims(a) == art => {
                Layer::Drawn { material: (*material).into(), art: a }
            }
            (ElementKind::Raster, Decoded::Raster { material, front, back })
                if dims(front) == art
                    && dims(back) == art
                    && material.len() as u64 == u64::from(art.0 / s) * u64::from(art.1 / s) =>
            {
                Layer::Raster { material, front, back }
            }
            _ => return None,
        })
    }

    /// The element's colour at art pixel `(ax, ay)` when it covers it.
    fn front(&self, ax: u32, ay: u32) -> Option<Rgba> {
        match self {
            Layer::Image { grid, pixel_size, origin, size, .. } => {
                let (lx, ly) = (i64::from(ax) - origin.0, i64::from(ay) - origin.1);
                if !(0..size.0).contains(&lx) || !(0..size.1).contains(&ly) {
                    return None;
                }
                let ps = i64::from(*pixel_size);
                let p = grid.get((lx / ps) as u32, (ly / ps) as u32);
                (p.a == 255).then_some(p)
            }
            Layer::Drawn { art, .. } => {
                let p = art.get(ax, ay);
                (p.a != 0).then(|| opaque(p))
            }
            Layer::Raster { front, .. } => Some(front.get(ax, ay)),
        }
    }

    /// The element's backdrop at art pixel `(ax, ay)` (R4): a background
    /// image or drawn element's art, a raster element's back.
    fn back(&self, ax: u32, ay: u32) -> Option<Rgba> {
        match self {
            Layer::Image { material: Material::Background, .. }
            | Layer::Drawn { material: Material::Background, .. } => self.front(ax, ay),
            Layer::Image { .. } | Layer::Drawn { .. } => None,
            Layer::Raster { back, .. } => Some(back.get(ax, ay)),
        }
    }

    /// The material of world pixel `cell` (row-major) whose centre art
    /// pixel is `(ax, ay)`, when the element covers that art pixel.
    fn material(&self, cell: usize, ax: u32, ay: u32) -> Option<Material> {
        match self {
            Layer::Image { material, .. } | Layer::Drawn { material, .. } => {
                self.front(ax, ay).map(|_| *material)
            }
            Layer::Raster { material, .. } => material.get(cell).copied(),
        }
    }
}

/// The back of art pixel `(ax, ay)` (R4).
fn back_at(layers: &[Layer], back_tile: &RgbaImage, ax: u32, ay: u32) -> Rgba {
    if let Some(c) = layers.iter().rev().find_map(|l| l.back(ax, ay)) {
        return c;
    }
    let t = back_tile;
    if t.width() == 0 || t.height() == 0 {
        return Rgba::BLACK;
    }
    opaque(t.get(ax % t.width(), ay % t.height()))
}

/// Rebakes `rect` (clipped) of `level`'s material, front and back from
/// decoded elements, bottom to top (rulings R3-R5 of plan 14a). A visible
/// image whose grid is `None`, and data of a size the level or the
/// element's box does not match, are left out; a level whose own layers
/// do not match its size is left as it is. Never fails or panics.
pub fn bake_elements(
    level: &mut LevelSource,
    items: &[(&Element, &Decoded)],
    back_tile: &RgbaImage,
    rect: Rect,
) {
    let (w, h) = level.size;
    let s = level.art_scale;
    let (Some(aw), Some(ah)) = (w.checked_mul(s), h.checked_mul(s)) else { return };
    let fits = s > 0
        && level.material.len() as u64 == u64::from(w) * u64::from(h)
        && (level.front.width(), level.front.height()) == (aw, ah)
        && (level.back.width(), level.back.height()) == (aw, ah);
    if !fits {
        return;
    }
    let layers: Vec<Layer> = items.iter().filter_map(|(e, d)| Layer::new(e, d, (aw, ah), s)).collect();
    let x1 = rect.x.saturating_add(rect.w).min(w);
    let y1 = rect.y.saturating_add(rect.h).min(h);
    for y in rect.y.min(h)..y1 {
        for x in rect.x.min(w)..x1 {
            let cell = y as usize * w as usize + x as usize;
            let (cx, cy) = (x * s + s / 2, y * s + s / 2);
            let material =
                layers.iter().rev().find_map(|l| l.material(cell, cx, cy)).unwrap_or(Material::Background);
            level.material[cell] = material;
            for ay in y * s..(y + 1) * s {
                for ax in x * s..(x + 1) * s {
                    let back = back_at(&layers, back_tile, ax, ay);
                    let front = if material == Material::Background {
                        back
                    } else {
                        layers.iter().rev().find_map(|l| l.front(ax, ay)).unwrap_or(back)
                    };
                    level.back.set(ax, ay, back);
                    level.front.set(ax, ay, front);
                }
            }
        }
    }
}

/// Decoded elements and processed images of one composition, ready to
/// bake any region. `bake_region` never fails: everything was checked here.
pub struct Baker {
    size: (u32, u32),
    art_scale: u32,
    back_tile: RgbaImage,
    /// Every element with its decoded data, bottom to top.
    items: Vec<(Element, Decoded)>,
}

impl Baker {
    /// Checks `src` against a level of `size` at `art_scale`
    /// (`check_composition`), then decodes its layers and originals and
    /// processes its images: originals within `IMAGES`, layers within
    /// `LEVEL_ART`. A fresh `ImageCache` each time.
    pub fn new(
        src: &CompositionSource,
        size: (u32, u32),
        art_scale: u32,
        back_tile: &RgbaImage,
    ) -> Result<Baker, ComposeError> {
        Baker::new_cached(src, size, art_scale, back_tile, &mut ImageCache::new())
    }

    /// `new` through `cache`: an image whose original and settings it
    /// holds is not processed again.
    pub fn new_cached(
        src: &CompositionSource,
        size: (u32, u32),
        art_scale: u32,
        back_tile: &RgbaImage,
        cache: &mut ImageCache,
    ) -> Result<Baker, ComposeError> {
        let s = art_scale;
        match (size.0.checked_mul(s), size.1.checked_mul(s)) {
            (Some(w), Some(h)) if image_io::check_size(w, h, LEVEL_ART).is_ok() => {}
            _ => {
                return Err(ComposeError::File {
                    file: "level.ron".into(),
                    message: format!("{}x{} at art scale {s}: art beyond the level bound", size.0, size.1),
                });
            }
        }
        check_composition(src, size, art_scale)?;
        let items = src
            .composition
            .elements
            .iter()
            .map(|e| Ok((e.clone(), decode_element(src, e, size, s, cache)?)))
            .collect::<Result<_, ComposeError>>()?;
        Ok(Baker { size, art_scale, back_tile: back_tile.clone(), items })
    }

    /// Rebakes `rect` (clipped to the level) of `level`'s material, front
    /// and back. A level of another size or art scale than the baker's is
    /// left as it is.
    pub fn bake_region(&self, level: &mut LevelSource, rect: Rect) {
        if level.size != self.size || level.art_scale != self.art_scale {
            return;
        }
        let items: Vec<(&Element, &Decoded)> = self.items.iter().map(|(e, d)| (e, d)).collect();
        bake_elements(level, &items, &self.back_tile, rect);
    }
}

/// One element's files decoded (and an image processed through `cache`
/// when visible; a hidden image's original is decoded for its errors),
/// each checked against a level of `size` at art scale `s`.
pub fn decode_element(
    src: &CompositionSource,
    e: &Element,
    size: (u32, u32),
    s: u32,
    cache: &mut ImageCache,
) -> Result<Decoded, ComposeError> {
    let bytes = |file: &str| {
        src.files.get(file).ok_or_else(|| ComposeError::File { file: file.into(), message: "missing".into() })
    };
    let fail = |file: &str, message: String| ComposeError::File { file: file.into(), message };
    let art = (size.0.saturating_mul(s), size.1.saturating_mul(s));
    let rgba = |file: &str, expected: (u32, u32)| -> Result<RgbaImage, ComposeError> {
        let img = image_io::decode_rgba_within(bytes(file)?, LEVEL_ART).map_err(|m| fail(file, m))?;
        if (img.width(), img.height()) != expected {
            return Err(fail(
                file,
                format!("{}x{}, the level needs {}x{}", img.width(), img.height(), expected.0, expected.1),
            ));
        }
        Ok(img)
    };
    Ok(match &e.kind {
        ElementKind::Image {
            source, size: (bw, bh), pixel_size, colours, dither, alpha_threshold, ..
        } => {
            let original = bytes(&image_file(source))?;
            let mask = match &e.kind {
                ElementKind::Image { mask: true, .. } => {
                    let img = cache.original(source, original)?;
                    let file = mask_file(&e.id);
                    Some(Arc::new(Mask::decode(&file, bytes(&file)?, (img.width(), img.height()))?))
                }
                _ => None,
            };
            if !e.visible {
                cache.original(source, original)?;
                return Ok(Decoded::Image { grid: None, mask });
            }
            let p = Pixelation {
                pixel_size: *pixel_size,
                colours: *colours,
                dither: *dither,
                alpha_threshold: *alpha_threshold,
            };
            let box_art = (bw.saturating_mul(s), bh.saturating_mul(s));
            let grid = cache.grid(source, original, box_art, &p, mask.as_deref())?;
            Decoded::Image { grid: Some(grid), mask }
        }
        ElementKind::Drawn { .. } => Decoded::Drawn { art: rgba(&layer_file(&e.id), art)? },
        ElementKind::Raster => {
            let [m, f, b] = raster_files(&e.id);
            let (mw, mh, indices) =
                image_io::decode_indexed_within(bytes(&m)?, LEVEL_ART).map_err(|why| fail(&m, why))?;
            if (mw, mh) != size {
                return Err(fail(&m, format!("{mw}x{mh}, the level is {}x{}", size.0, size.1)));
            }
            let material = indices
                .iter()
                .map(|&i| {
                    Material::from_index(i)
                        .ok_or_else(|| fail(&m, format!("palette index {i}: not a material")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Decoded::Raster { material, front: rgba(&f, art)?, back: rgba(&b, art)? }
        }
    })
}

/// The files a drawn or raster element's decoded data encodes to (as
/// `write_level_files` encodes art and material; a raster's material is a
/// level of `size`), and an image's mask; not an image's original (it is
/// kept), nor data of another kind than the element, nor a raster's
/// material not of `size`.
pub fn encode_decoded(e: &Element, d: &Decoded, size: (u32, u32)) -> Vec<(String, Vec<u8>)> {
    match (&e.kind, d) {
        (ElementKind::Image { mask: true, .. }, Decoded::Image { mask: Some(m), .. }) => {
            vec![(mask_file(&e.id), m.encode())]
        }
        (ElementKind::Drawn { .. }, Decoded::Drawn { art }) => {
            vec![(layer_file(&e.id), image_io::encode_rgba(art))]
        }
        (ElementKind::Raster, Decoded::Raster { material, front, back })
            if material.len() as u64 == u64::from(size.0) * u64::from(size.1) =>
        {
            let [m, f, b] = raster_files(&e.id);
            vec![
                (m, encode_material(size, material)),
                (f, image_io::encode_rgba(front)),
                (b, image_io::encode_rgba(back)),
            ]
        }
        _ => Vec::new(),
    }
}

/// A level source holding `front` and `back` (and `material`) at `size`,
/// for `level::resize` to re-lay.
fn as_level(
    size: (u32, u32),
    art_scale: u32,
    material: Vec<Material>,
    front: RgbaImage,
    back: RgbaImage,
) -> LevelSource {
    LevelSource {
        name: String::new(),
        notes: String::new(),
        size,
        art_scale,
        theme: String::new(),
        objects: Vec::new(),
        material,
        front,
        back,
        thumb: None,
    }
}

/// Re-lays a drawn or raster element's decoded data for a level resize
/// from `old` to `size` (as `level::resize`: the new area uncovered for
/// drawn layers, `material` and `paint` for rasters); moves an image's
/// `at` by `offset`. A refusal changes nothing.
#[allow(clippy::too_many_arguments)]
pub fn resize_decoded(
    e: &mut Element,
    d: &mut Decoded,
    old: (u32, u32),
    art_scale: u32,
    size: (u32, u32),
    offset: (i32, i32),
    material: Material,
    paint: &Paint,
) -> Result<(), EditError> {
    let art = (u64::from(old.0) * u64::from(art_scale), u64::from(old.1) * u64::from(art_scale));
    let dims = |i: &RgbaImage| (u64::from(i.width()), u64::from(i.height()));
    let wrong = |what: &RgbaImage| {
        EditError::Art(what.width(), what.height(), format!("{:?} is not the level's art size", e.id))
    };
    match d {
        Decoded::Drawn { art: a } if dims(a) != art => return Err(wrong(a)),
        Decoded::Raster { front, back, .. } if dims(front) != art || dims(back) != art => {
            return Err(wrong(if dims(front) != art { front } else { back }));
        }
        Decoded::Raster { material, front, .. }
            if material.len() as u64 != u64::from(old.0) * u64::from(old.1) =>
        {
            return Err(wrong(front));
        }
        _ => {}
    }
    match (&mut e.kind, d) {
        (ElementKind::Image { at, .. }, _) => {
            let moved = |v: i32, by: i32| v.checked_add(by).ok_or_else(|| EditError::Moved(e.id.clone()));
            *at = (moved(at.0, offset.0)?, moved(at.1, offset.1)?);
        }
        (ElementKind::Drawn { .. }, Decoded::Drawn { art }) => {
            let cells = vec![Material::Background; old.0 as usize * old.1 as usize];
            let mut tmp = as_level(old, art_scale, cells, art.clone(), RgbaImage::new(0, 0));
            tmp.back = tmp.front.clone();
            level::resize(&mut tmp, size, offset, material, &Paint::Colour(Rgba::TRANSPARENT))?;
            *art = tmp.front;
        }
        (ElementKind::Raster, Decoded::Raster { material: cells, front, back }) => {
            let mut tmp = as_level(old, art_scale, cells.clone(), front.clone(), back.clone());
            level::resize(&mut tmp, size, offset, material, paint)?;
            (*cells, *front, *back) = (tmp.material, tmp.front, tmp.back);
        }
        _ => {}
    }
    Ok(())
}

/// The whole bake: material, front, back and the thumbnail (`refresh_thumb`).
pub fn bake(
    level: &mut LevelSource,
    src: &CompositionSource,
    back_tile: &RgbaImage,
) -> Result<(), ComposeError> {
    let baker = Baker::new(src, level.size, level.art_scale, back_tile)?;
    baker.bake_region(level, Rect { x: 0, y: 0, w: level.size.0, h: level.size.1 });
    refresh_thumb(level);
    Ok(())
}

/// `level` as a composition of one raster element `base` (ruling R6): its
/// material, front and back encoded as `write_level_files` encodes them.
pub fn compose_from(level: &LevelSource) -> CompositionSource {
    let element = Element {
        id: "base".into(),
        name: "Base".into(),
        visible: true,
        locked: false,
        kind: ElementKind::Raster,
    };
    let [m, f, b] = raster_files(&element.id);
    let files = [
        (m, encode_material(level.size, &level.material)),
        (f, image_io::encode_rgba(&level.front)),
        (b, image_io::encode_rgba(&level.back)),
    ]
    .into_iter()
    .collect();
    CompositionSource { composition: Composition { elements: vec![element] }, files }
}

/// A level's own material, front and back as three drawn layers, bottom
/// to top: `background` (the back art, everywhere: the backdrop), `dirt`
/// and `rock` (the front art over each dirt or rock world pixel, clear
/// elsewhere). Topmost wins, so they bake back to the same material, front
/// and back (`front == back` on background), and each is painted and
/// erased like any drawn layer. `ids` names them (unique in their stack).
pub fn split_layers(
    material: &[Material],
    front: &RgbaImage,
    back: &RgbaImage,
    size: (u32, u32),
    s: u32,
    ids: [&str; 3],
) -> Vec<(Element, Decoded)> {
    let (aw, ah) = (size.0 * s, size.1 * s);
    let fit = |img: &RgbaImage| {
        let mut out = RgbaImage::new(aw, ah);
        for y in 0..ah.min(img.height()) {
            for x in 0..aw.min(img.width()) {
                out.set(x, y, img.get(x, y));
            }
        }
        out
    };
    let only = |m: Material| {
        let mut out = RgbaImage::new(aw, ah);
        for (i, cell) in material.iter().enumerate().take(size.0 as usize * size.1 as usize) {
            if *cell != m {
                continue;
            }
            let (wx, wy) = (i as u32 % size.0, i as u32 / size.0);
            for y in wy * s..(wy + 1) * s {
                for x in wx * s..(wx + 1) * s {
                    if x < front.width() && y < front.height() {
                        out.set(x, y, front.get(x, y));
                    }
                }
            }
        }
        out
    };
    let layer = |id: &str, name: &str, material: ElementMaterial, art: RgbaImage| {
        let e = Element {
            id: id.into(),
            name: name.into(),
            visible: true,
            locked: false,
            kind: ElementKind::drawn(material),
        };
        (e, Decoded::Drawn { art })
    };
    vec![
        layer(ids[0], "Background", ElementMaterial::Background, fit(back)),
        layer(ids[1], "Dirt", ElementMaterial::Dirt, only(Material::Dirt)),
        layer(ids[2], "Rock", ElementMaterial::Rock, only(Material::Rock)),
    ]
}

/// A level without a composition as the editor opens it: its material,
/// front and back as three drawn layers (`split_layers`: `background`,
/// `dirt`, `rock`), which bake back to its files byte for byte.
pub fn split_from(level: &LevelSource) -> CompositionSource {
    let layers = split_layers(
        &level.material,
        &level.front,
        &level.back,
        level.size,
        level.art_scale,
        ["background", "dirt", "rock"],
    );
    let mut files = BTreeMap::new();
    let mut elements = Vec::new();
    for (e, d) in layers {
        files.extend(encode_decoded(&e, &d, level.size));
        elements.push(e);
    }
    CompositionSource { composition: Composition { elements }, files }
}

/// The `back_tile` of the theme pack whose manifest id is `theme` among
/// `content`'s immediate subdirectories (ruling R9), taken in name order;
/// hidden directories and directories that are not packs are skipped.
pub fn theme_back_tile(content: &Path, theme: &str) -> Result<RgbaImage, ComposeError> {
    let fail = |message: String| ComposeError::File { file: content.display().to_string(), message };
    let mut dirs: Vec<_> = std::fs::read_dir(content)
        .map_err(|e| fail(e.to_string()))?
        .filter_map(Result::ok)
        .filter(|e| {
            // A linked pack counts: `metadata` follows the link.
            std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir())
                && !e.file_name().to_string_lossy().starts_with('.')
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let src = DirFs::new(&dir);
        let Ok(manifest) = olr_pack::read_manifest(&src) else { continue };
        if manifest.kind == PackKind::Theme && manifest.id == theme {
            return Ok(read_theme_files(&src)?.back_tile);
        }
    }
    Err(ComposeError::File {
        file: theme.into(),
        message: format!("no theme pack with this id in {}", content.display()),
    })
}

/// The files a bake writes that it can leave stale.
const BAKED: [&str; 4] = ["material.png", "front.png", "back.png", "thumb.png"];

/// The baked files of `pack` (a composed level) that differ from what its
/// composition bakes to: `material.png`, `front.png`, `back.png`,
/// `thumb.png`, in that order. Empty for any other pack.
pub fn stale(pack: &Pack, back_tile: &RgbaImage) -> Result<Vec<String>, ComposeError> {
    let (Source::Level(level), Some(src)) = (&pack.source, &pack.composition) else { return Ok(Vec::new()) };
    let mut baked = level.clone();
    bake(&mut baked, src, back_tile)?;
    let files = |l: &LevelSource| -> Result<MemFs, ComposeError> {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, l)?;
        Ok(fs)
    };
    let (now, then) = (files(level)?, files(&baked)?);
    Ok(BAKED
        .iter()
        .filter(|f| {
            let path = VPath::new(f).expect("a valid path");
            now.read(&path).ok() != then.read(&path).ok()
        })
        .map(|f| f.to_string())
        .collect())
}
