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
//! Glow (plan 15a, look system §Composition) follows the same rules: a
//! front art pixel glows as the element giving its colour; a back art
//! pixel as the background element giving it (a dirt or rock layer's back
//! art never glows); a world pixel's groups follow its centre art pixel.
//! A solid world pixel is clear (`look/clear.png`) when the element giving
//! its material is.
//!
//! Distant (parallax) layers (issue #11): a distant element (a background
//! image or plain drawn layer) gives neither material nor front nor back;
//! its covered art is one of the level's distant layers (`bake_distant`).
//! When the level has a visible one, the back art pixels no other element
//! covers are see-through: the back tile's colour with alpha 0 (and so the
//! front over background blocks); without one, every back pixel is opaque,
//! as always.
//!
//! Surface materials (issue #37) follow where the pixels land: a world
//! pixel's front material (`look/surface.png`) is the surface of the
//! element giving its material when that is rock or dirt, else none; its
//! back material (`look/surface-back.png`) is that of the element giving
//! its centre art pixel's back (else the first back art pixel of its block
//! that has one): its `surface` when the back is its own art (a background
//! element, a mixed layer's background cell), its `back_surface` when it is
//! back art. A painted map's value overrides the default per pixel (0
//! keeps it). The maps are `None` when no pixel has a material; a distant
//! element's `surface` is its layer's.
//!
//! Integer arithmetic only: the same composition bakes to the same bytes
//! on every platform.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{DistantLayer, MAX_DEPTH, MIN_DEPTH, Material};
use olr_data::manifest::PackKind;
use olr_format::image_io::{self, IMAGES, LEVEL_ART};
use olr_format::levels::{
    Composition, CompositionSource, Element, ElementKind, ElementMaterial, GlowLayers, LevelSource,
    SurfaceMaps, check_composition, decode_surface_map, drawn_box, drawn_files, element_box, encode_material,
    glow_file, image_file, mask_file, raster_files, surface_back_file, surface_file, surface_map_size,
    write_level_files,
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
    /// A drawn layer's art (its box × art scale), its material per world
    /// pixel of its box when mixed, its back art when backed (plan 14d R3).
    Drawn { art: RgbaImage, cells: Option<Vec<Material>>, back: Option<RgbaImage> },
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
            Decoded::Drawn { art, cells, back } => {
                img(art) + cells.as_ref().map_or(0, Vec::len) + back.as_ref().map_or(0, img)
            }
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
    /// A drawn layer placed at `origin` (art pixels) with its art's size,
    /// covered where its alpha is not 0 (plan 14d R4). `cells` (one per
    /// world pixel of its box, `cols` wide) when mixed, `back` when backed.
    Drawn {
        material: Material,
        art: &'a RgbaImage,
        cells: Option<&'a [Material]>,
        back: Option<&'a RgbaImage>,
        origin: (i64, i64),
        cols: u32,
        s: u32,
    },
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
            (ElementKind::Drawn { material, .. }, Decoded::Drawn { art: a, cells, back }) => {
                let level = (art.0 / s.max(1), art.1 / s.max(1));
                let (at, (bw, bh)) = drawn_box(&e.kind, level)?;
                let box_art = (bw.checked_mul(s)?, bh.checked_mul(s)?);
                let fits = dims(a) == box_art
                    && cells.as_ref().is_none_or(|c| c.len() as u64 == u64::from(bw) * u64::from(bh))
                    && back.as_ref().is_none_or(|b| dims(b) == box_art);
                if !fits {
                    return None;
                }
                Layer::Drawn {
                    material: (*material).into(),
                    art: a,
                    cells: cells.as_deref(),
                    back: back.as_ref(),
                    origin: (i64::from(at.0) * i64::from(s), i64::from(at.1) * i64::from(s)),
                    cols: bw,
                    s,
                }
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
                let (lx, ly) = self.local(ax, ay)?;
                let p = art.get(lx, ly);
                (p.a != 0).then(|| opaque(p))
            }
            Layer::Raster { front, .. } => Some(front.get(ax, ay)),
        }
    }

    /// A drawn layer's own art pixel for level art pixel `(ax, ay)`, when
    /// inside its box.
    fn local(&self, ax: u32, ay: u32) -> Option<(u32, u32)> {
        let Layer::Drawn { art, origin, .. } = self else { return None };
        let (lx, ly) = (i64::from(ax) - origin.0, i64::from(ay) - origin.1);
        let inside = (0..i64::from(art.width())).contains(&lx) && (0..i64::from(art.height())).contains(&ly);
        inside.then_some((lx as u32, ly as u32))
    }

    /// A drawn layer's material at its own art pixel `(lx, ly)`: its
    /// block's cell when mixed, else its material.
    fn cell(&self, lx: u32, ly: u32) -> Option<Material> {
        let Layer::Drawn { material, cells, cols, s, .. } = self else { return None };
        Some(match cells {
            Some(c) => *c.get((ly / s) as usize * *cols as usize + (lx / s) as usize)?,
            None => *material,
        })
    }

    /// The element's backdrop at art pixel `(ax, ay)` (R4): a background
    /// image or drawn element's art, a raster element's back.
    fn back(&self, ax: u32, ay: u32) -> Option<Rgba> {
        self.backdrop(ax, ay).map(|(c, _)| c)
    }

    /// [`Layer::back`], and whether it is the element's own art (which
    /// glows with the element) rather than back art (which never does).
    fn backdrop(&self, ax: u32, ay: u32) -> Option<(Rgba, bool)> {
        match self {
            Layer::Image { material: Material::Background, .. } => self.front(ax, ay).map(|c| (c, true)),
            Layer::Image { .. } => None,
            // A covered background pixel is backdrop; under dirt or rock,
            // the back art where it is opaque (plan 14d R4).
            Layer::Drawn { back, .. } => {
                let (lx, ly) = self.local(ax, ay)?;
                if let Some(c) = self.front(ax, ay)
                    && self.cell(lx, ly) == Some(Material::Background)
                {
                    return Some((c, true));
                }
                back.map(|b| b.get(lx, ly)).filter(|p| p.a != 0).map(|p| (opaque(p), false))
            }
            // A see-through back pixel (alpha 0) covers nothing.
            Layer::Raster { back, .. } => Some((back.get(ax, ay), false)).filter(|(p, _)| p.a != 0),
        }
    }

    /// The material of world pixel `cell` (row-major) whose centre art
    /// pixel is `(ax, ay)`, when the element covers that art pixel.
    fn material(&self, cell: usize, ax: u32, ay: u32) -> Option<Material> {
        match self {
            Layer::Image { material, .. } => self.front(ax, ay).map(|_| *material),
            Layer::Drawn { .. } => {
                self.front(ax, ay)?;
                let (lx, ly) = self.local(ax, ay)?;
                self.cell(lx, ly)
            }
            Layer::Raster { material, .. } => material.get(cell).copied(),
        }
    }
}

/// The back of art pixel `(ax, ay)` (R4): see-through (the tile's colour
/// with alpha 0) where no element covers it in a level `see_through`
/// (with distant layers).
fn back_at(layers: &[Layer], back_tile: &RgbaImage, see_through: bool, ax: u32, ay: u32) -> Rgba {
    if let Some(c) = layers.iter().rev().find_map(|l| l.back(ax, ay)) {
        return c;
    }
    let t = back_tile;
    let tile = if t.width() == 0 || t.height() == 0 {
        Rgba::BLACK
    } else {
        opaque(t.get(ax % t.width(), ay % t.height()))
    };
    if see_through { Rgba { a: 0, ..tile } } else { tile }
}

/// `e`'s art as a distant layer in a level of `size` at art scale `s`:
/// its box's art, opaque where it covers and alpha 0 elsewhere; `None`
/// when it is not a visible distant element or its data does not fit.
fn distant_layer(e: &Element, d: &Decoded, size: (u32, u32), s: u32) -> Option<DistantLayer> {
    let depth = e.distant?.clamp(MIN_DEPTH, MAX_DEPTH);
    if !e.visible {
        return None;
    }
    let (at, b) = element_box(&e.kind, size)?;
    let (bw, bh) = (b.0.checked_mul(s)?, b.1.checked_mul(s)?);
    let mut image = RgbaImage::new(bw, bh);
    match (&e.kind, d) {
        (ElementKind::Image { pixel_size, .. }, Decoded::Image { grid: Some(grid), .. }) => {
            let ps = (*pixel_size).max(1);
            if (grid.width(), grid.height()) != (bw.div_ceil(ps), bh.div_ceil(ps)) {
                return None;
            }
            for y in 0..bh {
                for x in 0..bw {
                    let p = grid.get(x / ps, y / ps);
                    if p.a == 255 {
                        image.set(x, y, p);
                    }
                }
            }
        }
        (ElementKind::Drawn { .. }, Decoded::Drawn { art, .. }) => {
            if (art.width(), art.height()) != (bw, bh) {
                return None;
            }
            for y in 0..bh {
                for x in 0..bw {
                    let p = art.get(x, y);
                    if p.a != 0 {
                        image.set(x, y, opaque(p));
                    }
                }
            }
        }
        _ => return None,
    }
    Some(DistantLayer { image, at, depth })
}

/// Remakes `level`'s distant layers from `items` (bottom to top): every
/// visible distant element's (issue #11), far to near. A full bake
/// (`bake_elements_with` over the whole level) does it; a region bake
/// leaves them, so an editor calls this when a distant element changes.
pub fn bake_distant(level: &mut LevelSource, items: &[(&Element, &Decoded)]) {
    let (size, s) = (level.size, level.art_scale);
    let has = |id: &String| level.look.as_ref().is_some_and(|l| l.materials.iter().any(|m| m.id == *id));
    let (layers, mut surfaces): (Vec<DistantLayer>, Vec<Option<String>>) = items
        .iter()
        .filter_map(|(e, d)| Some((distant_layer(e, d, size, s)?, e.surface.clone().filter(has))))
        .unzip();
    while surfaces.last().is_some_and(Option::is_none) {
        surfaces.pop();
    }
    level.distant = layers;
    level.distant_surfaces = surfaces;
}

/// Whether `items` hold a visible distant element that bakes (its data
/// fitting): the level's uncovered back pixels are then see-through.
pub fn has_distant(items: &[(&Element, &Decoded)], size: (u32, u32), s: u32) -> bool {
    let Some(art) = size.0.checked_mul(s).zip(size.1.checked_mul(s)) else { return false };
    items.iter().any(|(e, d)| e.distant.is_some() && Layer::new(e, d, art, s).is_some())
}

/// The glow masks of a composition's elements, by element id: each the
/// element's box at art resolution, 0-255 (`editor/glow/<id>.png`).
pub type GlowMasks = BTreeMap<String, Arc<Vec<u8>>>;

/// A glow mask over its element's box: the values, the box's origin and
/// size in art pixels.
type BoxMask<'a> = (&'a [u8], (i64, i64), (u32, u32));

/// One element's glow as the bake reads it.
struct LayerGlow<'a> {
    /// 0-255.
    strength: u32,
    /// Its group map value: 1 + its index in the level's glow groups.
    group: u8,
    /// The mask and the element's box: origin and size in art pixels.
    mask: Option<BoxMask<'a>>,
}

impl LayerGlow<'_> {
    /// The glow at level art pixel `(ax, ay)`, 0-255.
    fn at(&self, ax: u32, ay: u32) -> u8 {
        let Some((mask, origin, size)) = self.mask else { return self.strength as u8 };
        let (lx, ly) = (i64::from(ax) - origin.0, i64::from(ay) - origin.1);
        if !(0..i64::from(size.0)).contains(&lx) || !(0..i64::from(size.1)).contains(&ly) {
            return 0;
        }
        let m = mask.get(ly as usize * size.0 as usize + lx as usize).copied().unwrap_or(0);
        ((self.strength * u32::from(m) + 127) / 255) as u8
    }
}

/// `e`'s glow in a level of `size` at art scale `s` with `groups` (ids in
/// order): `None` when it has none, names no such group, or its mask does
/// not fit.
fn layer_glow<'a>(
    e: &Element,
    masks: &'a GlowMasks,
    groups: &[&str],
    size: (u32, u32),
    s: u32,
) -> Option<LayerGlow<'a>> {
    let g = e.glow.as_ref()?;
    let group = u8::try_from(groups.iter().position(|id| *id == g.group)? + 1).ok()?;
    let strength = (u32::from(g.strength.min(100)) * 255 + 50) / 100;
    let mask = match masks.get(&e.id).filter(|_| g.mask) {
        Some(m) => {
            let (at, b) = element_box(&e.kind, size)?;
            let art = (b.0.checked_mul(s)?, b.1.checked_mul(s)?);
            if m.len() as u64 != u64::from(art.0) * u64::from(art.1) {
                return None;
            }
            Some((&m[..], (i64::from(at.0) * i64::from(s), i64::from(at.1) * i64::from(s)), art))
        }
        None => None,
    };
    Some(LayerGlow { strength, group, mask })
}

/// A world pixel's glow group: `glowing(ax, ay)`'s group (0 none) at the
/// centre art pixel `centre`, else at the first art pixel of its block
/// (`(x0, y0, s)`, row-major) that has one.
fn cell_group((x0, y0, s): (u32, u32, u32), (cx, cy): (u32, u32), glowing: &dyn Fn(u32, u32) -> u8) -> u8 {
    match glowing(cx, cy) {
        0 => (y0..y0 + s)
            .flat_map(|ay| (x0..x0 + s).map(move |ax| (ax, ay)))
            .map(|(ax, ay)| glowing(ax, ay))
            .find(|g| *g > 0)
            .unwrap_or(0),
        g => g,
    }
}

/// The painted surface maps of a composition's elements (issue #37), by
/// file (`editor/surface/<id>.png`, `editor/surface/<id>.back.png`): each
/// its element's box at world resolution ([`surface_map_size`]).
pub type PaintedSurfaces = BTreeMap<String, Arc<Vec<u8>>>;

/// A painted surface map over its element's box: the values, the box's
/// origin and size in world pixels.
type BoxMap<'a> = (&'a [u8], (i64, i64), (u32, u32));

/// One element's surface materials as the bake reads them: map values (1
/// + an index in the level's materials, 0 none).
#[derive(Default)]
struct LayerSurface<'a> {
    /// Its pixels' default.
    front: u8,
    /// Its back art's default.
    back: u8,
    front_map: Option<BoxMap<'a>>,
    back_map: Option<BoxMap<'a>>,
}

impl LayerSurface<'_> {
    /// Its pixels' material (`back`: its back art's) at world pixel
    /// `(x, y)`: the painted value, else the default.
    fn at(&self, back: bool, x: u32, y: u32, materials: usize) -> u8 {
        let (default, map) = if back { (self.back, self.back_map) } else { (self.front, self.front_map) };
        let Some((values, origin, size)) = map else { return default };
        let (lx, ly) = (i64::from(x) - origin.0, i64::from(y) - origin.1);
        if !(0..i64::from(size.0)).contains(&lx) || !(0..i64::from(size.1)).contains(&ly) {
            return default;
        }
        match values.get(ly as usize * size.0 as usize + lx as usize).copied().unwrap_or(0) {
            v if v == 0 || usize::from(v) > materials => default,
            v => v,
        }
    }

    /// Whether it names any material.
    fn any(&self) -> bool {
        self.front > 0 || self.back > 0 || self.front_map.is_some() || self.back_map.is_some()
    }
}

/// `e`'s surface materials in a level of `size` with `materials` (ids in
/// order): an id naming no material is none, a painted map not fitting
/// its box is left out.
fn layer_surface<'a>(
    e: &Element,
    painted: &'a PaintedSurfaces,
    materials: &[&str],
    size: (u32, u32),
) -> LayerSurface<'a> {
    let index = |id: &Option<String>| {
        id.as_deref()
            .and_then(|id| materials.iter().position(|m| *m == id))
            .and_then(|i| u8::try_from(i + 1).ok())
            .unwrap_or(0)
    };
    let origin = element_box(&e.kind, size).map_or((0, 0), |(at, _)| (i64::from(at.0), i64::from(at.1)));
    let b = surface_map_size(&e.kind, size);
    let map = |on: bool, file: String| -> Option<BoxMap<'a>> {
        let m = painted.get(&file).filter(|_| on)?;
        (m.len() as u64 == u64::from(b.0) * u64::from(b.1)).then_some((&m[..], origin, b))
    };
    LayerSurface {
        front: index(&e.surface),
        back: if e.has_back_art() { index(&e.back_surface) } else { 0 },
        front_map: map(e.surface_map, surface_file(&e.id)),
        back_map: map(e.back_surface_map && e.has_back_art(), surface_back_file(&e.id)),
    }
}

/// Rebakes `rect` (clipped) of `level`'s material, front and back from
/// decoded elements, bottom to top (rulings R3-R5 of plan 14a). A visible
/// image whose grid is `None`, and data of a size the level or the
/// element's box does not match, are left out; a level whose own layers
/// do not match its size is left as it is. Never fails or panics. Glow
/// masks are not given: a masked glow bakes uniform (see
/// [`bake_elements_with`]).
pub fn bake_elements(
    level: &mut LevelSource,
    items: &[(&Element, &Decoded)],
    back_tile: &RgbaImage,
    rect: Rect,
) {
    bake_elements_with(level, items, &GlowMasks::new(), &PaintedSurfaces::new(), back_tile, rect);
}

/// [`bake_elements`] with the elements' glow masks and painted surface
/// maps. The level's surface maps are baked too (issue #37): none when no
/// pixel has a material of the level's look. The level's glow
/// (`look/`) is baked too: none when no visible element glows (with a group
/// the level's look has), else its layers, made empty first when the level
/// had none. Distant elements give no material or art: the back pixels no
/// other element covers are see-through when one is visible
/// ([`has_distant`]); a bake of the whole level remakes the distant layers
/// ([`bake_distant`]).
pub fn bake_elements_with(
    level: &mut LevelSource,
    items: &[(&Element, &Decoded)],
    masks: &GlowMasks,
    painted: &PaintedSurfaces,
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
    let see_through = has_distant(items, (w, h), s);
    if rect.x == 0 && rect.y == 0 && rect.w >= w && rect.h >= h {
        bake_distant(level, items);
    }
    let groups: Vec<&str> =
        level.look.as_ref().map_or(Vec::new(), |l| l.glow_groups.iter().map(|g| g.id.as_str()).collect());
    let materials: Vec<&str> =
        level.look.as_ref().map_or(Vec::new(), |l| l.materials.iter().map(|m| m.id.as_str()).collect());
    let n = materials.len();
    let (mut layers, mut glows, mut clears, mut surfaces) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (e, d) in items.iter().filter(|(e, _)| e.distant.is_none()) {
        let Some(layer) = Layer::new(e, d, (aw, ah), s) else { continue };
        layers.push(layer);
        glows.push(layer_glow(e, masks, &groups, (w, h), s));
        clears.push(e.clear);
        surfaces.push(layer_surface(e, painted, &materials, (w, h)));
    }
    let mut surface = surfaces.iter().any(LayerSurface::any).then(|| match level.surface.take() {
        Some(m) if m.fits((w, h)) => m,
        _ => SurfaceMaps::empty((w, h)),
    });
    // The back material of the element giving art pixel (ax, ay)'s back
    // (its own art's surface, else its back art's), at world pixel (x, y).
    let back_surface = |ax: u32, ay: u32, x: u32, y: u32| {
        layers
            .iter()
            .zip(&surfaces)
            .rev()
            .find_map(|(l, m)| l.backdrop(ax, ay).map(|(_, own)| m.at(!own, x, y, n)))
            .unwrap_or(0)
    };
    let cells = w as usize * h as usize;
    let mut clear = clears.iter().any(|c| *c).then(|| match level.clear.take() {
        Some(c) if c.len() == cells => c,
        _ => vec![0; cells],
    });
    let mut glow = if glows.iter().any(Option::is_some) {
        match level.glow.take() {
            Some(g) if g.fits((aw, ah), (w, h)) => Some(g),
            _ => Some(GlowLayers::empty((aw, ah), (w, h))),
        }
    } else {
        None
    };
    // The glow and group of the topmost element giving art pixel (ax, ay)'s
    // front, and of the one giving its back (only its own art glows).
    let front_glow = |ax: u32, ay: u32| {
        layers.iter().zip(&glows).rev().find(|(l, _)| l.front(ax, ay).is_some()).and_then(|(_, g)| g.as_ref())
    };
    let back_glow = |ax: u32, ay: u32| {
        layers
            .iter()
            .zip(&glows)
            .rev()
            .find_map(|(l, g)| l.backdrop(ax, ay).map(|(_, own)| g.as_ref().filter(|_| own)))
            .flatten()
    };
    let x1 = rect.x.saturating_add(rect.w).min(w);
    let y1 = rect.y.saturating_add(rect.h).min(h);
    for y in rect.y.min(h)..y1 {
        for x in rect.x.min(w)..x1 {
            let cell = y as usize * w as usize + x as usize;
            let (cx, cy) = (x * s + s / 2, y * s + s / 2);
            let (material, is_clear, giver) = layers
                .iter()
                .zip(&clears)
                .zip(&surfaces)
                .rev()
                .find_map(|((l, c), m)| l.material(cell, cx, cy).map(|mat| (mat, *c, Some(m))))
                .unwrap_or((Material::Background, false, None));
            level.material[cell] = material;
            if let Some(c) = clear.as_mut() {
                c[cell] = u8::from(is_clear && material != Material::Background) * 255;
            }
            if let Some(m) = surface.as_mut() {
                m.front[cell] = match giver {
                    Some(g) if material != Material::Background => g.at(false, x, y, n),
                    _ => 0,
                };
                let block = (x * s, y * s, s);
                m.back[cell] = cell_group(block, (cx, cy), &|ax, ay| back_surface(ax, ay, x, y));
            }
            for ay in y * s..(y + 1) * s {
                for ax in x * s..(x + 1) * s {
                    let back = back_at(&layers, back_tile, see_through, ax, ay);
                    // A solid block is never see-through.
                    let front = if material == Material::Background {
                        back
                    } else {
                        layers.iter().rev().find_map(|l| l.front(ax, ay)).unwrap_or(opaque(back))
                    };
                    level.back.set(ax, ay, back);
                    level.front.set(ax, ay, front);
                    if let Some(g) = glow.as_mut() {
                        let i = ay as usize * aw as usize + ax as usize;
                        let b = back_glow(ax, ay).map_or(0, |l| l.at(ax, ay));
                        g.back[i] = b;
                        g.front[i] = if material == Material::Background {
                            b
                        } else {
                            front_glow(ax, ay).map_or(0, |l| l.at(ax, ay))
                        };
                    }
                }
            }
            if let Some(g) = glow.as_mut() {
                // The group of the element glowing at the centre art pixel,
                // else at the first glowing one of the block; 0 (none) when
                // nothing glows there.
                let block = (x * s, y * s, s);
                let group = |pick: &dyn Fn(u32, u32) -> u8| cell_group(block, (cx, cy), pick);
                let lit = |l: Option<&LayerGlow>, ax: u32, ay: u32| {
                    l.filter(|l| l.at(ax, ay) > 0).map_or(0, |l| l.group)
                };
                let b = group(&|ax, ay| lit(back_glow(ax, ay), ax, ay));
                g.groups_back[cell] = b;
                g.groups[cell] = if material == Material::Background {
                    b
                } else {
                    group(&|ax, ay| lit(front_glow(ax, ay), ax, ay))
                };
            }
        }
    }
    // A glow left with nothing glowing is no glow; so are clear flags.
    level.glow = glow.filter(|g| g.front.iter().chain(&g.back).any(|v| *v > 0));
    level.clear = clear.filter(|c| c.iter().any(|v| *v > 0));
    level.surface = surface.filter(|m| !m.is_empty());
}

/// Decoded elements and processed images of one composition, ready to
/// bake any region. `bake_region` never fails: everything was checked here.
pub struct Baker {
    size: (u32, u32),
    art_scale: u32,
    back_tile: RgbaImage,
    /// Every element with its decoded data, bottom to top.
    items: Vec<(Element, Decoded)>,
    /// The elements' glow masks.
    masks: GlowMasks,
    /// The elements' painted surface maps.
    painted: PaintedSurfaces,
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
        // Each element decoded by its own flags, baked by its effective
        // ones (issue #59): a folder's visibility rebakes, never re-decodes.
        let c = &src.composition;
        let items = c
            .elements
            .iter()
            .map(|e| Ok((crate::folders::fold(c, e), decode_element(src, e, size, s, cache)?)))
            .collect::<Result<_, ComposeError>>()?;
        let masks = decode_glow_masks(src, size, s)?;
        let painted = decode_surface_maps(src, size)?;
        Ok(Baker { size, art_scale, back_tile: back_tile.clone(), items, masks, painted })
    }

    /// Rebakes `rect` (clipped to the level) of `level`'s material, front
    /// and back. A level of another size or art scale than the baker's is
    /// left as it is.
    pub fn bake_region(&self, level: &mut LevelSource, rect: Rect) {
        if level.size != self.size || level.art_scale != self.art_scale {
            return;
        }
        let items: Vec<(&Element, &Decoded)> = self.items.iter().map(|(e, d)| (e, d)).collect();
        bake_elements_with(level, &items, &self.masks, &self.painted, &self.back_tile, rect);
    }
}

/// The painted surface maps of `src`'s elements in a level of `size`
/// (issue #37), each decoded grey and of its element's
/// [`surface_map_size`], by file.
pub fn decode_surface_maps(
    src: &CompositionSource,
    size: (u32, u32),
) -> Result<PaintedSurfaces, ComposeError> {
    let mut out = PaintedSurfaces::new();
    for e in &src.composition.elements {
        for (file, on) in
            [(surface_file(&e.id), e.surface_map), (surface_back_file(&e.id), e.back_surface_map)]
        {
            if on {
                let (_, values) = decode_surface_map(src, &file, surface_map_size(&e.kind, size))?;
                out.insert(file, Arc::new(values));
            }
        }
    }
    Ok(out)
}

/// The glow masks of `src`'s elements in a level of `size` at art scale
/// `s`, each decoded grey and of its element's box × `s`.
pub fn decode_glow_masks(
    src: &CompositionSource,
    size: (u32, u32),
    s: u32,
) -> Result<GlowMasks, ComposeError> {
    let mut out = GlowMasks::new();
    for e in &src.composition.elements {
        if !e.glow.as_ref().is_some_and(|g| g.mask) {
            continue;
        }
        let file = glow_file(&e.id);
        let fail = |message: String| ComposeError::File { file: file.clone(), message };
        let bytes = src.files.get(&file).ok_or_else(|| fail("missing".into()))?;
        let (_, b) = element_box(&e.kind, size).ok_or_else(|| fail("a raster element cannot glow".into()))?;
        let want = (b.0.saturating_mul(s), b.1.saturating_mul(s));
        let (mw, mh, values) = image_io::decode_grey_within(bytes, LEVEL_ART).map_err(fail)?;
        if (mw, mh) != want {
            return Err(fail(format!("{mw}x{mh}, the element's box × art scale is {}x{}", want.0, want.1)));
        }
        out.insert(e.id.clone(), Arc::new(values));
    }
    Ok(out)
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
        ElementKind::Drawn { mixed, back, .. } => {
            let (_, (bw, bh)) = drawn_box(&e.kind, size).unwrap_or(((0, 0), size));
            let box_art = (bw.saturating_mul(s), bh.saturating_mul(s));
            let files = drawn_files(&e.id, *mixed, *back);
            let cells = match mixed {
                true => Some(decode_cells(&files[1], bytes(&files[1])?, (bw, bh))?),
                false => None,
            };
            let back = match back {
                true => Some(rgba(files.last().map_or("", String::as_str), box_art)?),
                false => None,
            };
            Decoded::Drawn { art: rgba(&files[0], box_art)?, cells, back }
        }
        ElementKind::Raster => {
            let [m, f, b] = raster_files(&e.id);
            let material = decode_cells(&m, bytes(&m)?, size)?;
            Decoded::Raster { material, front: rgba(&f, art)?, back: rgba(&b, art)? }
        }
    })
}

/// Material file `file` (`bytes`, `material.png`'s palette) of exactly `size`.
fn decode_cells(file: &str, bytes: &[u8], size: (u32, u32)) -> Result<Vec<Material>, ComposeError> {
    let fail = |message: String| ComposeError::File { file: file.into(), message };
    let (mw, mh, indices) = image_io::decode_indexed_within(bytes, LEVEL_ART).map_err(fail)?;
    if (mw, mh) != size {
        return Err(fail(format!("{mw}x{mh}, the layer is {}x{}", size.0, size.1)));
    }
    indices
        .iter()
        .map(|&i| Material::from_index(i).ok_or_else(|| fail(format!("palette index {i}: not a material"))))
        .collect()
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
        // The files the element names, from data that agrees with it.
        (ElementKind::Drawn { mixed, back: backed, .. }, Decoded::Drawn { art, cells, back })
            if *mixed == cells.is_some() && *backed == back.is_some() =>
        {
            let (_, b) = drawn_box(&e.kind, size).unwrap_or(((0, 0), size));
            let files = drawn_files(&e.id, cells.is_some(), back.is_some());
            let mut out = vec![(files[0].clone(), image_io::encode_rgba(art))];
            if let Some(c) = cells {
                if c.len() as u64 != u64::from(b.0) * u64::from(b.1) {
                    return Vec::new();
                }
                out.push((files[1].clone(), encode_material(b, c)));
            }
            if let (Some(back), Some(f)) = (back, files.last()) {
                out.push((f.clone(), image_io::encode_rgba(back)));
            }
            out
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
        look: None,
        glow: None,
        clear: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
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
    // A level-sized layer moved off the origin is pinned to its box
    // first (batch A review I1): re-laid in its own frame it would lose
    // pixels the new level still shows.
    if let ElementKind::Drawn { at, size: boxed @ None, .. } = &mut e.kind
        && *at != (0, 0)
    {
        *boxed = Some(old);
    }
    // A boxed drawn layer only moves (plan 14d R2): its size is its own.
    let level_sized = !matches!(e.kind, ElementKind::Drawn { size: Some(_), .. });
    match d {
        Decoded::Drawn { art: a, .. } if level_sized && dims(a) != art => return Err(wrong(a)),
        Decoded::Drawn { back: Some(b), .. } if level_sized && dims(b) != art => return Err(wrong(b)),
        Decoded::Drawn { cells: Some(c), art: a, .. }
            if level_sized && c.len() as u64 != u64::from(old.0) * u64::from(old.1) =>
        {
            let what = format!(
                "{:?}'s cells are {}, the level has {}",
                e.id,
                c.len(),
                u64::from(old.0) * u64::from(old.1)
            );
            return Err(EditError::Art(a.width(), a.height(), what));
        }
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
    // Art re-laid with the new area uncovered; `cells` with the new area
    // of `fill` (background when none).
    let relay = |art: &RgbaImage, cells: Option<&Vec<Material>>, fill: Material| {
        let none = vec![Material::Background; old.0 as usize * old.1 as usize];
        let cells = cells.cloned().unwrap_or(none);
        let mut tmp = as_level(old, art_scale, cells, art.clone(), art.clone());
        level::resize(&mut tmp, size, offset, fill, &Paint::Colour(Rgba::TRANSPARENT))?;
        Ok::<_, EditError>((tmp.front, tmp.material))
    };
    let id = e.id.clone();
    let moved = |v: i32, by: i32| v.checked_add(by).ok_or_else(|| EditError::Moved(id.clone()));
    match (&mut e.kind, d) {
        (ElementKind::Image { at, .. }, _) | (ElementKind::Drawn { at, size: Some(_), .. }, _) => {
            *at = (moved(at.0, offset.0)?, moved(at.1, offset.1)?);
        }
        (ElementKind::Drawn { material: m, .. }, Decoded::Drawn { art, cells, back }) => {
            let (new_art, new_cells) = relay(art, cells.as_ref(), (*m).into())?;
            if cells.is_some() {
                *cells = Some(new_cells);
            }
            if let Some(b) = back {
                *b = relay(b, None, Material::Background)?.0;
            }
            *art = new_art;
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

/// A level's distant layers as distant drawn elements (`distant-1`, ...,
/// far to near), each boxed where its layer lies, with its surface: they
/// bake back to the same layers. A layer whose size is not a multiple of
/// the art scale (the format refuses one) is left out.
pub fn distant_elements(level: &LevelSource) -> Vec<(Element, Decoded)> {
    let s = level.art_scale.max(1);
    level
        .distant
        .iter()
        .zip((0..).map(|j| level.distant_surfaces.get(j).cloned().flatten()))
        .filter(|(d, _)| {
            d.image.width() % s == 0
                && d.image.height() % s == 0
                && d.image.width() > 0
                && d.image.height() > 0
        })
        .enumerate()
        .map(|(i, (d, surface))| {
            let e = Element {
                id: format!("distant-{}", i + 1),
                name: format!("Distant {}", i + 1),
                visible: true,
                locked: false,
                folder: None,
                kind: ElementKind::Drawn {
                    material: ElementMaterial::Background,
                    at: d.at,
                    size: Some((d.image.width() / s, d.image.height() / s)),
                    mixed: false,
                    back: false,
                },
                glow: None,
                clear: false,
                distant: Some(d.depth),
                surface,
                back_surface: None,
                surface_map: false,
                back_surface_map: false,
            };
            (e, Decoded::Drawn { art: d.image.clone(), cells: None, back: None })
        })
        .collect()
}

/// `level` as a composition of one raster element `base` (ruling R6): its
/// material, front and back encoded as `write_level_files` encodes them,
/// its surface maps (issue #37) as the raster's painted ones; its distant
/// layers below it ([`distant_elements`]).
pub fn compose_from(level: &LevelSource) -> CompositionSource {
    let mut element = Element {
        id: "base".into(),
        name: "Base".into(),
        visible: true,
        locked: false,
        folder: None,
        kind: ElementKind::Raster,
        glow: None,
        clear: false,
        distant: None,
        surface: None,
        back_surface: None,
        surface_map: false,
        back_surface_map: false,
    };
    let [m, f, b] = raster_files(&element.id);
    let mut files: BTreeMap<String, Vec<u8>> = [
        (m, encode_material(level.size, &level.material)),
        (f, image_io::encode_rgba(&level.front)),
        (b, image_io::encode_rgba(&level.back)),
    ]
    .into_iter()
    .collect();
    if let Some(maps) = level.surface.as_ref().filter(|m| m.fits(level.size)) {
        let (w, h) = level.size;
        (element.surface_map, element.back_surface_map) = (true, true);
        files.insert(surface_file(&element.id), image_io::encode_grey(w, h, &maps.front));
        files.insert(surface_back_file(&element.id), image_io::encode_grey(w, h, &maps.back));
    }
    let mut elements = Vec::new();
    for (e, d) in distant_elements(level) {
        files.extend(encode_decoded(&e, &d, level.size));
        elements.push(e);
    }
    elements.push(element);
    CompositionSource { composition: Composition { elements, folders: Vec::new() }, files }
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
            folder: None,
            kind: ElementKind::drawn(material),
            glow: None,
            clear: false,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        (e, Decoded::Drawn { art, cells: None, back: None })
    };
    vec![
        layer(ids[0], "Background", ElementMaterial::Background, fit(back)),
        layer(ids[1], "Dirt", ElementMaterial::Dirt, only(Material::Dirt)),
        layer(ids[2], "Rock", ElementMaterial::Rock, only(Material::Rock)),
    ]
}

/// A level without a composition as the editor opens it: its material,
/// front and back as three drawn layers (`split_layers`: `background`,
/// `dirt`, `rock`) over its distant layers ([`distant_elements`]), which
/// bake back to its files byte for byte; its surface maps (issue #37) are
/// the layers' painted ones, the back map the background's and the front
/// map the dirt's and the rock's.
pub fn split_from(level: &LevelSource) -> CompositionSource {
    let mut layers = distant_elements(level);
    layers.extend(split_layers(
        &level.material,
        &level.front,
        &level.back,
        level.size,
        level.art_scale,
        ["background", "dirt", "rock"],
    ));
    let mut files = BTreeMap::new();
    let mut elements = Vec::new();
    let maps = level.surface.as_ref().filter(|m| m.fits(level.size));
    for (mut e, d) in layers {
        files.extend(encode_decoded(&e, &d, level.size));
        if let (Some(m), false) = (maps, e.is_distant()) {
            let values = if e.id == "background" { &m.back } else { &m.front };
            e.surface_map = true;
            files.insert(surface_file(&e.id), image_io::encode_grey(level.size.0, level.size.1, values));
        }
        elements.push(e);
    }
    CompositionSource { composition: Composition { elements, folders: Vec::new() }, files }
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

/// The files a bake writes that it can leave stale (and `level.ron`'s
/// distant layers and their images, `distant/<i>.png`).
const BAKED: [&str; 11] = [
    "material.png",
    "front.png",
    "back.png",
    "thumb.png",
    "look/glow.png",
    "look/glow-back.png",
    "look/groups.png",
    "look/groups-back.png",
    "look/clear.png",
    "look/surface.png",
    "look/surface-back.png",
];

/// The baked files of `pack` (a composed level) that differ from what its
/// composition bakes to, in `BAKED`'s order: `material.png`, `front.png`,
/// `back.png`, `thumb.png`, then the four `look/` glow images,
/// `look/clear.png` and the two surface maps, then `level.ron` when its
/// distant layers (or their surfaces) differ and
/// the `distant/<i>.png` images that do, either side's. Empty for any other
/// pack.
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
    let differs = |f: &str| {
        let path = VPath::new(f).expect("a valid path");
        now.read(&path).ok() != then.read(&path).ok()
    };
    let mut out: Vec<String> = BAKED.iter().filter(|f| differs(f)).map(|f| f.to_string()).collect();
    let entries = |l: &LevelSource| l.distant.iter().map(|d| (d.at, d.depth)).collect::<Vec<_>>();
    if entries(level) != entries(&baked) || level.distant_surfaces != baked.distant_surfaces {
        out.push("level.ron".into());
    }
    let n = level.distant.len().max(baked.distant.len());
    out.extend((0..n).map(olr_format::levels::distant_file).filter(|f| differs(f)));
    Ok(out)
}
