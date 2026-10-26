//! Assets (issue #9; the format is `olr_format::assets`): ready-made
//! pieces of a level, saved once and placed in any level. The operations
//! the editor and `olr-cli asset` share: an asset made of elements
//! ([`from_elements`]) or cut out of a world rectangle of every visible
//! layer ([`cut_rect`]); a piece landed in a level with a new id and name,
//! resampled to its art scale ([`land`], which pastes use too); the glow
//! groups and surface materials a placing adds or joins ([`with_groups`],
//! [`with_materials`]); an asset encoded to
//! its folder with a lit thumbnail ([`encode`]) and decoded from one
//! ([`decode`]), and the folder read and written ([`load_dir`],
//! [`save_dir`], [`summary`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_format::Colour;
use olr_format::FormatError;
use olr_format::assets::{
    ASSET_FILE, AssetFile, AssetSource, as_composition, check_asset, read_asset_files, write_asset_files,
};
use olr_format::levels::{
    Element, ElementGlow, ElementKind, ElementMaterial, LevelSource, MAX_ART_SCALE, element_box,
    level_thumbnail, surface_back_file, surface_file, surface_map_size,
};
use olr_format::look::{AmbientSource, GlowGroupSource, LookSource};
use olr_format::materials::MaterialSource;
use olr_vfs::DirFs;

use crate::compose::{
    ComposeError, Decoded, GlowMasks, ImageCache, PaintedSurfaces, Rect, bake_elements_with, decode_element,
    decode_glow_masks, decode_surface_maps, encode_decoded,
};
use crate::drawn::{self, Placed};
use crate::element::{copy_name, new_element_id, new_source_name};
use crate::glow::{relay_mask, resample_mask};
use crate::level::rescale_art;

/// The preview look an asset's thumbnail is drawn under: dark, so a glow
/// shows (under a full-bright look a glow adds nothing, light being capped
/// at `max_light`), as the old lamps pack's ambient.
pub const PREVIEW_AMBIENT: (&str, u16) = ("#304060", 20);

/// One element with its decoded data, an image's original (its source
/// name and bytes), its glow mask (its box × art scale, 0-255) and its
/// painted surface maps (issue #38; its box at world resolution, a
/// raster's the level's size; `v` names `materials[v - 1]` of the level or
/// asset it is in, 0 its `surface` or `back_surface`): what the editor's
/// clipboard and an asset hold.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub element: Element,
    pub decoded: Decoded,
    pub original: Option<(String, Arc<Vec<u8>>)>,
    pub glow_mask: Option<Arc<Vec<u8>>>,
    pub surface_map: Option<Arc<Vec<u8>>>,
    pub back_surface_map: Option<Arc<Vec<u8>>>,
}

/// A [`Piece`] borrowed from wherever it lives (the editor's document, a
/// pack decoded).
#[derive(Clone, Copy, Debug)]
pub struct PieceRef<'a> {
    pub element: &'a Element,
    pub decoded: &'a Decoded,
    pub original: Option<&'a (String, Arc<Vec<u8>>)>,
    pub glow_mask: Option<&'a Arc<Vec<u8>>>,
    pub surface_map: Option<&'a Arc<Vec<u8>>>,
    pub back_surface_map: Option<&'a Arc<Vec<u8>>>,
}

impl Piece {
    pub fn as_ref(&self) -> PieceRef<'_> {
        PieceRef {
            element: &self.element,
            decoded: &self.decoded,
            original: self.original.as_ref(),
            glow_mask: self.glow_mask.as_ref(),
            surface_map: self.surface_map.as_ref(),
            back_surface_map: self.back_surface_map.as_ref(),
        }
    }
}

/// An asset decoded: its name, the art scale it was made at, its size in
/// world pixels, its pieces bottom to top (placed relative to its
/// top-left), the glow groups they name and the surface materials they
/// name (issue #38; their painted maps' values index these).
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub art_scale: u32,
    pub size: (u32, u32),
    pub pieces: Vec<Piece>,
    pub groups: Vec<GlowGroupSource>,
    pub materials: Vec<MaterialSource>,
}

/// Why an asset could not be made, read, written or placed.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error(transparent)]
    Format(#[from] FormatError),
    #[error(transparent)]
    Compose(#[from] ComposeError),
    #[error("{0}")]
    Bad(String),
    #[error("{0}")]
    Io(String),
}

/// A level's elements as an asset is cut from them: each piece, the
/// level's size, art scale and look (the groups' and materials'
/// settings).
pub struct LevelPieces<'a> {
    pub pieces: Vec<PieceRef<'a>>,
    pub size: (u32, u32),
    pub art_scale: u32,
    pub look: Option<&'a LookSource>,
}

/// An element as an asset holds it: shown, unlocked and in no folder
/// (asset files carry none, issue #59).
pub fn plain(e: &Element) -> Element {
    // An asset carries no glass (the background glass design).
    Element { visible: true, locked: false, folder: None, glass: None, ..e.clone() }
}

/// An element's painted surface maps, its pixels' and its back art's.
pub type SurfacePair = (Option<Arc<Vec<u8>>>, Option<Arc<Vec<u8>>>);

/// The world box element `e`'s painted surface maps cover in a level of
/// `size`: its box, a raster's the level.
fn surface_box(e: &Element, size: (u32, u32)) -> ((i32, i32), (u32, u32)) {
    element_box(&e.kind, size).unwrap_or(((0, 0), surface_map_size(&e.kind, size)))
}

/// `e`'s surface materials made to fit it with maps `front` and `back`
/// (its box at world resolution): a map not of its box's size or all 0
/// goes, so does a back surface without back art; the flags follow the
/// maps. Returns the maps kept.
fn fit_surfaces(
    e: &mut Element,
    front: Option<Arc<Vec<u8>>>,
    back: Option<Arc<Vec<u8>>>,
    size: (u32, u32),
) -> SurfacePair {
    let b = surface_map_size(&e.kind, size);
    let cells = u64::from(b.0) * u64::from(b.1);
    let keep = |on: bool, m: Option<Arc<Vec<u8>>>| {
        m.filter(|m| on && m.len() as u64 == cells && m.iter().any(|v| *v > 0))
    };
    if !e.has_back_art() || e.is_distant() {
        e.back_surface = None;
    }
    let front = keep(e.surface_map && !e.is_distant(), front);
    let back = keep(e.back_surface_map && e.has_back_art() && !e.is_distant(), back);
    (e.surface_map, e.back_surface_map) = (front.is_some(), back.is_some());
    (front, back)
}

/// `p`'s pixels within world rectangle `r` (`None` when they do not meet).
fn crop_to(p: &Placed, r: Rect, s: u32) -> Option<Placed> {
    let (bx, by) = (i64::from(p.at.0), i64::from(p.at.1));
    let x0 = bx.max(i64::from(r.x));
    let y0 = by.max(i64::from(r.y));
    let x1 = (bx + i64::from(p.size.0)).min(i64::from(r.x) + i64::from(r.w));
    let y1 = (by + i64::from(p.size.1)).min(i64::from(r.y) + i64::from(r.h));
    if x0 >= x1 || y0 >= y1 || !p.fits(s) {
        return None;
    }
    let (lx, ly, w, h) = ((x0 - bx) as u32, (y0 - by) as u32, (x1 - x0) as u32, (y1 - y0) as u32);
    let cut = |i: &RgbaImage| i.crop(lx * s, ly * s, w * s, h * s);
    let cells = (ly..ly + h)
        .flat_map(|y| (lx..lx + w).map(move |x| (x, y)))
        .map(|(x, y)| p.cells[y as usize * p.size.0 as usize + x as usize])
        .collect();
    Some(Placed {
        at: (x0 as i32, y0 as i32),
        size: (w, h),
        art: cut(&p.art),
        cells,
        back: p.back.as_ref().map(cut),
    })
}

/// The material a layer of `cells` is: the most common of them, a tie to
/// the higher priority (rock first).
fn main_material(cells: &[Material]) -> ElementMaterial {
    let mut counts = [0usize; 3];
    for c in cells {
        counts[usize::from(drawn::priority(*c))] += 1;
    }
    match (0..3).max_by_key(|&k| (counts[k], k)) {
        Some(2) => ElementMaterial::Rock,
        Some(0) => ElementMaterial::Background,
        _ => ElementMaterial::Dirt,
    }
}

/// `piece` (visible or not) as drawn pixels cropped to `within` (world)
/// and then to what it covers: its element (a drawn layer of its material),
/// decoded data and glow mask re-laid on the new box; `None` when nothing
/// is left. A glow whose mask is all zero there is dropped.
fn as_drawn(piece: PieceRef, within: Option<Rect>, size: (u32, u32), s: u32) -> Option<Piece> {
    let e = plain(piece.element);
    let p = drawn::placed(&e, piece.decoded, size, s)?;
    let p = match within {
        Some(r) => crop_to(&p, r, s)?,
        None => p,
    };
    let p = drawn::crop(&p, s)?;
    let material = match &e.kind {
        ElementKind::Image { material, .. } | ElementKind::Drawn { material, .. } => *material,
        ElementKind::Raster => main_material(&p.cells),
    };
    let new_box = (p.at, p.size);
    let (kind, decoded) = drawn::into_layer(p, material, size, s);
    // A boxed layer always: an asset's layers are placed by their box.
    let kind = match kind {
        ElementKind::Drawn { material, at, mixed, back, .. } => {
            ElementKind::Drawn { material, at, size: Some(new_box.1), mixed, back }
        }
        k => k,
    };
    let mut element = Element { kind, ..e };
    let glow_mask = relaid_mask(piece, new_box, size, s);
    let (glow, glow_mask) = kept_glow(element.glow.take(), glow_mask);
    element.glow = glow;
    // Its painted surface maps follow its pixels to the new box.
    let from = surface_box(piece.element, size);
    let relay =
        |m: Option<&Arc<Vec<u8>>>| m.map(|m| Arc::new(crate::element::relay_surface(m, from, new_box)));
    let (surface_map, back_surface_map) =
        fit_surfaces(&mut element, relay(piece.surface_map), relay(piece.back_surface_map), size);
    Some(Piece { element, decoded, original: None, glow_mask, surface_map, back_surface_map })
}

/// `piece`'s glow mask laid on box `to` (world), when it has one.
fn relaid_mask(
    piece: PieceRef,
    to: ((i32, i32), (u32, u32)),
    size: (u32, u32),
    s: u32,
) -> Option<Arc<Vec<u8>>> {
    let m = piece.glow_mask.filter(|_| piece.element.glow.as_ref().is_some_and(|g| g.mask))?;
    let from = element_box(&piece.element.kind, size)?;
    Some(Arc::new(relay_mask(m, from, to, s)))
}

/// A glow and its mask as a cut keeps them: none when the mask is all zero
/// (nothing of it glows), the mask flag following the mask.
fn kept_glow(
    glow: Option<ElementGlow>,
    mask: Option<Arc<Vec<u8>>>,
) -> (Option<ElementGlow>, Option<Arc<Vec<u8>>>) {
    let Some(mut g) = glow else { return (None, None) };
    if !g.mask {
        return (Some(g), None);
    }
    match mask {
        Some(m) if m.iter().all(|v| *v == 0) => (None, None),
        Some(m) => (Some(g), Some(m)),
        None => {
            g.mask = false;
            (Some(g), None)
        }
    }
}

/// `piece` moved by `(dx, dy)` world pixels (its box).
fn shifted(mut piece: Piece, dx: i64, dy: i64) -> Piece {
    let mv = |v: i32, d: i64| (i64::from(v) + d).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
    match &mut piece.element.kind {
        ElementKind::Image { at, .. } | ElementKind::Drawn { at, .. } => *at = (mv(at.0, dx), mv(at.1, dy)),
        ElementKind::Raster => {}
    }
    piece
}

/// The groups of `look` the pieces' glows name, in the look's order.
fn named_groups(pieces: &[Piece], look: Option<&LookSource>) -> Vec<GlowGroupSource> {
    let named: BTreeSet<&str> =
        pieces.iter().filter_map(|p| p.element.glow.as_ref()).map(|g| g.group.as_str()).collect();
    look.iter()
        .flat_map(|l| l.glow_groups.iter())
        .filter(|g| named.contains(g.id.as_str()))
        .cloned()
        .collect()
}

/// A glow naming no group of the level's look glows nowhere: it goes.
fn drop_dangling(pieces: &mut [Piece], groups: &[GlowGroupSource]) {
    for p in pieces {
        if p.element.glow.as_ref().is_some_and(|g| !groups.iter().any(|x| x.id == g.group)) {
            p.element.glow = None;
            p.glow_mask = None;
        }
    }
}

/// The materials of `look` the pieces (of a level of `size`) name, in the
/// look's order (issue #38): by `surface`, `back_surface` or a painted
/// value; their maps' values re-numbered to index them, a surface naming
/// none of the look's materials gone (it named nothing in the level).
fn carried_materials(
    pieces: &mut [Piece],
    look: Option<&LookSource>,
    size: (u32, u32),
) -> Vec<MaterialSource> {
    let all = look.map_or(&[][..], |l| &l.materials[..]);
    let mut used = vec![false; all.len()];
    for p in pieces.iter() {
        for id in [&p.element.surface, &p.element.back_surface].into_iter().flatten() {
            if let Some(i) = all.iter().position(|m| m.id == *id) {
                used[i] = true;
            }
        }
        for m in [&p.surface_map, &p.back_surface_map].into_iter().flatten() {
            for v in m.iter().map(|v| usize::from(*v)).filter(|v| (1..=all.len()).contains(v)) {
                used[v - 1] = true;
            }
        }
    }
    // The value naming old material `i` (1-based) names it here.
    let mut renumber = [0u8; 256];
    let mut next = 0u8;
    for (i, u) in used.iter().enumerate().take(255) {
        if *u {
            next += 1;
            renumber[i + 1] = next;
        }
    }
    for p in pieces.iter_mut() {
        let e = &mut p.element;
        for id in [&mut e.surface, &mut e.back_surface] {
            if id.as_ref().is_some_and(|id| !all.iter().any(|m| m.id == *id)) {
                *id = None;
            }
        }
        let remap = |m: Option<Arc<Vec<u8>>>| {
            m.map(|m| Arc::new(m.iter().map(|v| renumber[usize::from(*v)]).collect()))
        };
        let (front, back) = (remap(p.surface_map.take()), remap(p.back_surface_map.take()));
        (p.surface_map, p.back_surface_map) = fit_surfaces(e, front, back, size);
    }
    all.iter().zip(&used).filter(|(_, u)| **u).map(|(m, _)| m.clone()).take(255).collect()
}

/// An image piece as it is, its glow mask and surface maps kept (none
/// when all zero), in a level of `size`.
fn whole_image(piece: PieceRef, size: (u32, u32)) -> Option<Piece> {
    let original = piece.original?.clone();
    let mut element = plain(piece.element);
    let (glow, glow_mask) = kept_glow(element.glow.take(), piece.glow_mask.cloned());
    element.glow = glow;
    let (surface_map, back_surface_map) =
        fit_surfaces(&mut element, piece.surface_map.cloned(), piece.back_surface_map.cloned(), size);
    Some(Piece {
        element,
        decoded: piece.decoded.clone(),
        original: Some(original),
        glow_mask,
        surface_map,
        back_surface_map,
    })
}

/// An asset of the elements `ids` of `level` (the editor's Save as asset
/// with elements selected), in stack order: an image whole (its original,
/// settings and masks), a drawn layer cropped to the pixels it covers, a
/// raster as drawn pixels; each shown and unlocked, placed relative to the
/// top-left of the boxes' union (the asset's size), with the glow groups
/// they name. Distant layers are left out: an asset carries none (issue
/// #11).
pub fn from_elements(level: &LevelPieces, ids: &[String], name: &str) -> Result<Asset, AssetError> {
    let (size, s) = (level.size, level.art_scale);
    let mut pieces = Vec::new();
    for piece in level.pieces.iter().filter(|p| ids.contains(&p.element.id) && !p.element.is_distant()) {
        let made = match piece.element.kind {
            ElementKind::Image { .. } => whole_image(*piece, size),
            _ => as_drawn(*piece, None, size, s),
        };
        pieces.extend(made);
    }
    if pieces.is_empty() {
        return Err(AssetError::Bad("the elements have no pixels to save".into()));
    }
    let boxes: Vec<_> = pieces.iter().filter_map(|p| element_box(&p.element.kind, size)).collect();
    let x0 = boxes.iter().map(|b| i64::from(b.0.0)).min().unwrap_or(0);
    let y0 = boxes.iter().map(|b| i64::from(b.0.1)).min().unwrap_or(0);
    let x1 = boxes.iter().map(|b| i64::from(b.0.0) + i64::from(b.1.0)).max().unwrap_or(1);
    let y1 = boxes.iter().map(|b| i64::from(b.0.1) + i64::from(b.1.1)).max().unwrap_or(1);
    let side = |v: i64| v.clamp(1, i64::from(olr_format::levels::MAX_SIDE)) as u32;
    let mut pieces: Vec<Piece> = pieces.into_iter().map(|p| shifted(p, -x0, -y0)).collect();
    let groups = named_groups(&pieces, level.look);
    drop_dangling(&mut pieces, &groups);
    let materials = carried_materials(&mut pieces, level.look, size);
    let size = (side(x1 - x0), side(y1 - y0));
    Ok(Asset { name: name.into(), art_scale: s, size, pieces, groups, materials })
}

/// An asset cut out of world rectangle `rect` of `level` (the editor's
/// Save as asset with a pixel rectangle selected): every visible element
/// under it (only those of `layers` when given), bottom to top, each its
/// own piece placed relative to the rectangle's top-left (the asset's size
/// the rectangle's). A picture wholly inside is kept whole (its original
/// and settings); anything else is cropped as drawn pixels (a drawn layer
/// with its art, cells and back art; a picture partly inside as its baked
/// art and material), then to what it covers, its glow mask with it. A
/// piece with no pixels left is left out, so is a glow whose mask is all
/// zero there (its group not carried), and so is a piece identical in
/// pixels, material and glow to one below it (lit levels duplicate layers
/// so each copy glows in its own group). Distant layers are left out.
pub fn cut_rect(
    level: &LevelPieces,
    rect: Rect,
    layers: Option<&[String]>,
    name: &str,
) -> Result<Asset, AssetError> {
    let (size, s) = (level.size, level.art_scale);
    let rect = clip(rect, size);
    if rect.w == 0 || rect.h == 0 {
        return Err(AssetError::Bad("the rectangle holds no pixels of the level".into()));
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for piece in &level.pieces {
        let e = piece.element;
        if !e.visible || e.is_distant() || layers.is_some_and(|l| !l.contains(&e.id)) {
            continue;
        }
        let inside = match &e.kind {
            ElementKind::Image { at, size: b, .. } => {
                let (x, y) = (i64::from(at.0), i64::from(at.1));
                x >= i64::from(rect.x)
                    && y >= i64::from(rect.y)
                    && x + i64::from(b.0) <= i64::from(rect.x) + i64::from(rect.w)
                    && y + i64::from(b.1) <= i64::from(rect.y) + i64::from(rect.h)
            }
            _ => false,
        };
        let made = if inside { whole_image(*piece, size) } else { as_drawn(*piece, Some(rect), size, s) };
        let Some(made) = made else { continue };
        let same = |o: &Piece| {
            o.element.kind == made.element.kind
                && o.decoded == made.decoded
                && o.element.glow == made.element.glow
                && o.glow_mask == made.glow_mask
                && o.element.clear == made.element.clear
                && o.original == made.original
                && o.element.surface == made.element.surface
                && o.element.back_surface == made.element.back_surface
                && o.surface_map == made.surface_map
                && o.back_surface_map == made.back_surface_map
        };
        if !pieces.iter().any(same) {
            pieces.push(made);
        }
    }
    if pieces.is_empty() {
        return Err(AssetError::Bad("no visible layer has pixels in the rectangle".into()));
    }
    let mut pieces: Vec<Piece> =
        pieces.into_iter().map(|p| shifted(p, -i64::from(rect.x), -i64::from(rect.y))).collect();
    let groups = named_groups(&pieces, level.look);
    drop_dangling(&mut pieces, &groups);
    let materials = carried_materials(&mut pieces, level.look, size);
    Ok(Asset { name: name.into(), art_scale: s, size: (rect.w, rect.h), pieces, groups, materials })
}

/// `r` within a level of `size`.
fn clip(r: Rect, size: (u32, u32)) -> Rect {
    let x0 = r.x.min(size.0);
    let y0 = r.y.min(size.1);
    let x1 = r.x.saturating_add(r.w).min(size.0);
    let y1 = r.y.saturating_add(r.h).min(size.1);
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// The level a piece lands in: its elements, its stray files (never
/// overwritten), its image originals by source name, its size and art
/// scale.
pub struct Landing<'a> {
    pub elements: &'a [Element],
    /// The level's folders: a landed id is none of theirs (issue #59).
    pub folders: &'a [olr_format::levels::Folder],
    pub stray: &'a BTreeSet<String>,
    pub originals: &'a BTreeMap<String, Arc<Vec<u8>>>,
    pub size: (u32, u32),
    pub art_scale: u32,
}

/// `piece` (made at art scale `from`) as a new element of the level `to`,
/// moved by `offset` (plan 14e R6, shared by paste and placing): a new id
/// and a `copy_name`d name (its own; `name`, when given, as it is while
/// the level has no element of that name); drawn
/// and raster art resampled to the level's art scale (an image's grid left
/// for the caller to process: `Decoded::Image { grid: None, .. }`), its
/// glow mask fitted to its box there; an original whose name the level
/// holds with other bytes takes a new name (one with its bytes is shared).
/// A raster lands only in a level of its size. Its surface materials stay
/// behind (the level may lack them: [`place_in_pack`] lands them itself).
pub fn land(
    to: &Landing,
    piece: PieceRef,
    from: u32,
    offset: (i32, i32),
    name: Option<&str>,
) -> Result<Piece, AssetError> {
    let (size, s) = (to.size, to.art_scale);
    let from = from.clamp(1, MAX_ART_SCALE);
    let source_id = &piece.element.id;
    // Its surface materials stay behind (issue #37: their painted maps are
    // not a piece's, and the level landed in may lack the materials).
    let mut e = piece.element.clone().without_surfaces();
    // It lands in no folder: `place_in_pack` groups an asset's pieces.
    e.folder = None;
    let names = to.elements.iter().map(|x| if x.name.is_empty() { x.id.as_str() } else { x.name.as_str() });
    let wanted = name
        .map(str::to_string)
        .unwrap_or_else(|| if e.name.is_empty() { e.id.clone() } else { e.name.clone() });
    // A paste is a copy ("Crate 2"); a placed piece keeps its name while
    // the level has none of it.
    e.name = if name.is_some() && !names.clone().any(|n| n == wanted) {
        wanted
    } else {
        copy_name(&wanted, names)
    };
    e.id = new_element_id(to.elements, to.folders, to.stray, &e.name, "copy");
    let shift = |a: (i32, i32)| {
        let add =
            |v: i32, d: i32| (i64::from(v) + i64::from(d)).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
        (add(a.0, offset.0), add(a.1, offset.1))
    };
    let mut original = None;
    let decoded = match (&mut e.kind, piece.decoded) {
        (ElementKind::Image { at, source, .. }, Decoded::Image { mask, .. }) => {
            *at = shift(*at);
            let (name, bytes) = piece
                .original
                .cloned()
                .ok_or_else(|| AssetError::Bad(format!("{source_id:?}: its original is missing")))?;
            // Its bytes under a name here already are shared; another
            // original of its name is never overwritten (Review Focus 1).
            let same = |(n, b): (&String, &Arc<Vec<u8>>)| (**b == *bytes).then(|| n.clone());
            let shared = to
                .originals
                .get_key_value(&name)
                .and_then(same)
                .or_else(|| to.originals.iter().find_map(same));
            *source = shared.unwrap_or_else(|| {
                let dir = olr_format::levels::image_file("");
                let taken = to
                    .originals
                    .keys()
                    .map(String::as_str)
                    .chain(to.stray.iter().filter_map(|f| f.strip_prefix(dir.as_str())));
                new_source_name(taken, &name)
            });
            original = Some((source.clone(), bytes));
            Decoded::Image { grid: None, mask: mask.clone() }
        }
        (ElementKind::Drawn { at, size: boxed, .. }, Decoded::Drawn { art, cells, back }) => {
            let side = (art.width() / from, art.height() / from);
            *at = shift(*at);
            *boxed = (*at != (0, 0) || side != size).then_some(side);
            Decoded::Drawn {
                art: rescale_art(art, from, s),
                cells: cells.clone(),
                back: back.as_ref().map(|b| rescale_art(b, from, s)),
            }
        }
        (ElementKind::Raster, Decoded::Raster { material, front, back }) => {
            let side = (front.width() / from, front.height() / from);
            if side != size || material.len() as u64 != u64::from(size.0) * u64::from(size.1) {
                return Err(AssetError::Bad(format!(
                    "{source_id:?} is a raster of {} × {}: it pastes only into a level of its size ({} × {} here)",
                    side.0, side.1, size.0, size.1
                )));
            }
            Decoded::Raster {
                material: material.clone(),
                front: rescale_art(front, from, s),
                back: rescale_art(back, from, s),
            }
        }
        _ => return Err(AssetError::Bad(format!("{source_id:?}: its data does not match it"))),
    };
    // Its mask, fitted to its box here (another art scale resamples it).
    let glow_mask = piece.glow_mask.and_then(|m| {
        let (_, b) = element_box(&e.kind, size)?;
        let (_, ob) = element_box(&piece.element.kind, size)?;
        let (old, new) = ((ob.0 * from, ob.1 * from), (b.0 * s, b.1 * s));
        Some(Arc::new(resample_mask(m, old, new)))
    });
    Ok(Piece { element: e, decoded, original, glow_mask, surface_map: None, back_surface_map: None })
}

/// What a placing that gave a level its first look says (the editor's
/// status bar, `olr-cli asset place`): the default look is full bright, so
/// the glow shows only once the ambient light is lowered (the maintainer's
/// choice over darkening the level unasked).
pub const ADDED_LIGHTING: &str =
    "Added lighting for the asset's glow: lower the ambient light in the level's lighting to see it";

/// The look a placing leaves (issue #9): `look`, or the default look when
/// the level has none and the asset glows, with each of `groups` the level
/// lacks added with the asset's settings; a group it has (by id) is
/// joined, its settings kept. The ids added and joined say what happened;
/// [`GroupChange::changes`] whether the look changes at all.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupChange {
    pub look: LookSource,
    /// The level had no look: it gets `LookSource::default()`.
    pub added_look: bool,
    pub added: Vec<String>,
    pub joined: Vec<String>,
}

impl GroupChange {
    /// Whether the level's look changes: a look or a group added.
    pub fn changes(&self) -> bool {
        self.added_look || !self.added.is_empty()
    }
}

/// [`GroupChange`] of placing an asset of `groups` in a level of `look`.
pub fn with_groups(look: Option<&LookSource>, groups: &[GlowGroupSource]) -> GroupChange {
    let added_look = look.is_none() && !groups.is_empty();
    let mut look = look.cloned().unwrap_or_default();
    let (mut added, mut joined) = (Vec::new(), Vec::new());
    for g in groups {
        if look.glow_groups.iter().any(|x| x.id == g.id) {
            joined.push(g.id.clone());
        } else {
            look.glow_groups.push(g.clone());
            added.push(g.id.clone());
        }
    }
    GroupChange { look, added_look, added, joined }
}

/// The look a placing leaves for the asset's surface materials (issue
/// #38), as [`GroupChange`] for its glow groups: `look`, or the default
/// look when the level has none and the asset has materials, with each of
/// `materials` the level lacks added with the asset's settings; one it
/// has (by id) is joined, its settings kept.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialChange {
    pub look: LookSource,
    /// The level had no look: it gets `LookSource::default()`.
    pub added_look: bool,
    pub added: Vec<String>,
    pub joined: Vec<String>,
}

impl MaterialChange {
    /// Whether the level's look changes: a look or a material added.
    pub fn changes(&self) -> bool {
        self.added_look || !self.added.is_empty()
    }

    /// The value naming each of the asset's `materials` in the look left
    /// (1 + its index there), by the asset's value: `[0]` is 0.
    pub fn renumbering(&self, materials: &[MaterialSource]) -> [u8; 256] {
        let mut out = [0u8; 256];
        for (i, m) in materials.iter().enumerate().take(255) {
            let at = self.look.materials.iter().position(|x| x.id == m.id);
            out[i + 1] = at.and_then(|j| u8::try_from(j + 1).ok()).unwrap_or(0);
        }
        out
    }
}

/// The surface materials of asset piece `piece` given to `e`, the element
/// it landed as ([`land`]) in a level of `size` whose look `change` left
/// (issue #38): its `surface` and `back_surface` (by id: joined or added),
/// and its painted maps, their values re-numbered from the asset's
/// `materials` to the level's. Returns the maps; `e`'s flags follow them.
pub fn land_surfaces(
    piece: PieceRef,
    e: &mut Element,
    change: &MaterialChange,
    materials: &[MaterialSource],
    size: (u32, u32),
) -> SurfacePair {
    let renumber = change.renumbering(materials);
    let p = piece.element;
    (e.surface, e.back_surface) = (p.surface.clone(), p.back_surface.clone());
    (e.surface_map, e.back_surface_map) = (p.surface_map, p.back_surface_map);
    let remap = |m: Option<&Arc<Vec<u8>>>| {
        m.map(|m| Arc::new(m.iter().map(|v| renumber[usize::from(*v)]).collect::<Vec<u8>>()))
    };
    fit_surfaces(e, remap(piece.surface_map), remap(piece.back_surface_map), size)
}

/// [`MaterialChange`] of placing an asset of `materials` in a level of
/// `look`.
pub fn with_materials(look: Option<&LookSource>, materials: &[MaterialSource]) -> MaterialChange {
    let added_look = look.is_none() && !materials.is_empty();
    let mut look = look.cloned().unwrap_or_default();
    let (mut added, mut joined) = (Vec::new(), Vec::new());
    for m in materials {
        if look.materials.iter().any(|x| x.id == m.id) {
            joined.push(m.id.clone());
        } else {
            look.materials.push(m.clone());
            added.push(m.id.clone());
        }
    }
    MaterialChange { look, added_look, added, joined }
}

/// What a placed piece is named: the asset's name for a one-piece asset,
/// else "<asset> <piece>".
pub fn piece_name(asset: &Asset, piece: &Piece) -> String {
    let own = if piece.element.name.is_empty() { &piece.element.id } else { &piece.element.name };
    if asset.pieces.len() == 1 { asset.name.clone() } else { format!("{} {own}", asset.name) }
}

/// The dark look an asset's thumbnail is drawn under, with `groups`.
pub fn preview_look(groups: &[GlowGroupSource]) -> LookSource {
    let colour = Colour::parse(PREVIEW_AMBIENT.0).ok();
    LookSource {
        ambient: AmbientSource { colour, intensity: PREVIEW_AMBIENT.1 },
        glow_groups: groups.to_vec(),
        ..LookSource::default()
    }
}

/// The asset's folder in memory: `asset.ron` (each glow's mask flag
/// following its mask), the pieces' files and a thumbnail drawn lit under
/// [`preview_look`]; checked (`check_asset`) and decoded again, so what is
/// written reads back.
pub fn encode(asset: &Asset) -> Result<AssetSource, AssetError> {
    let s = asset.art_scale;
    let mut files = BTreeMap::new();
    let mut elements = Vec::new();
    for p in &asset.pieces {
        let mut e = p.element.clone();
        if let (ElementKind::Image { source, .. }, Some((_, bytes))) = (&e.kind, &p.original) {
            files.insert(format!("images/{source}"), bytes.as_ref().clone());
        }
        for (path, bytes) in encode_decoded(&e, &p.decoded, asset.size) {
            files.insert(strip(&path), bytes);
        }
        let fits = |m: &Arc<Vec<u8>>| {
            element_box(&e.kind, asset.size)
                .is_some_and(|(_, b)| m.len() as u64 == u64::from(b.0 * s) * u64::from(b.1 * s))
        };
        if let Some(g) = e.glow.as_mut() {
            match p.glow_mask.as_ref().filter(|m| g.mask && fits(m)) {
                Some(m) => {
                    let (_, b) = element_box(&e.kind, asset.size).unwrap_or(((0, 0), (0, 0)));
                    files.insert(
                        format!("glow/{}.png", e.id),
                        olr_format::image_io::encode_grey(b.0 * s, b.1 * s, m),
                    );
                }
                None => g.mask = false,
            }
        }
        // Its painted surface maps (issue #38), each flag following its map.
        let (w, h) = surface_map_size(&e.kind, asset.size);
        let (front, back) =
            fit_surfaces(&mut e, p.surface_map.clone(), p.back_surface_map.clone(), asset.size);
        for (file, m) in [(surface_file(&e.id), front), (surface_back_file(&e.id), back)] {
            if let Some(m) = m {
                files.insert(strip(&file), olr_format::image_io::encode_grey(w, h, &m));
            }
        }
        elements.push(e);
    }
    let file = AssetFile {
        art_scale: s,
        glow_groups: asset.groups.clone(),
        materials: asset.materials.clone(),
        ..AssetFile::new(&asset.name, asset.size, elements)
    };
    let mut src = AssetSource::new(file, files);
    let decoded = decode(&src)?;
    src.thumb = Some(olr_format::image_io::encode_rgba(&thumbnail(&decoded)));
    Ok(src)
}

fn strip(path: &str) -> String {
    path.strip_prefix("editor/").unwrap_or(path).to_string()
}

/// `asset` drawn lit under [`preview_look`] (its uncovered pixels black),
/// letterboxed into the level thumbnail's size.
pub fn thumbnail(asset: &Asset) -> RgbaImage {
    let (w, h) = asset.size;
    let s = asset.art_scale.max(1);
    let mut level = LevelSource {
        name: String::new(),
        notes: String::new(),
        size: (w, h),
        art_scale: s,
        theme: String::new(),
        objects: Vec::new(),
        material: vec![Material::Background; w as usize * h as usize],
        front: RgbaImage::new(w * s, h * s),
        back: RgbaImage::new(w * s, h * s),
        thumb: None,
        look: Some(preview_look(&asset.groups)),
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
    };
    let masks: GlowMasks =
        asset.pieces.iter().filter_map(|p| Some((p.element.id.clone(), p.glow_mask.clone()?))).collect();
    let items: Vec<(&Element, &Decoded)> = asset.pieces.iter().map(|p| (&p.element, &p.decoded)).collect();
    bake_elements_with(
        &mut level,
        &items,
        &masks,
        &PaintedSurfaces::new(),
        &RgbaImage::new(0, 0),
        Rect { x: 0, y: 0, w, h },
    );
    level_thumbnail(&level)
}

/// An asset folder's contents decoded: checked first (`check_asset`), every
/// element decoded (a truncated or misfit file refused), images processed
/// at the asset's art scale, glow masks of their boxes.
pub fn decode(src: &AssetSource) -> Result<Asset, AssetError> {
    check_asset(src)?;
    let f = &src.file;
    let comp = as_composition(src);
    let mut cache = ImageCache::new();
    let masks = decode_glow_masks(&comp, f.size, f.art_scale)?;
    let maps = decode_surface_maps(&comp, f.size)?;
    let mut pieces = Vec::new();
    for e in &f.elements {
        let decoded = decode_element(&comp, e, f.size, f.art_scale, &mut cache)?;
        let original = match &e.kind {
            ElementKind::Image { source, .. } => {
                let bytes = src
                    .files
                    .get(&format!("images/{source}"))
                    .ok_or_else(|| AssetError::Bad(format!("{ASSET_FILE}: images/{source}: missing")))?;
                Some((source.clone(), Arc::new(bytes.clone())))
            }
            _ => None,
        };
        pieces.push(Piece {
            element: e.clone(),
            decoded,
            original,
            glow_mask: masks.get(&e.id).cloned(),
            surface_map: maps.get(&surface_file(&e.id)).cloned(),
            back_surface_map: maps.get(&surface_back_file(&e.id)).cloned(),
        });
    }
    Ok(Asset {
        name: f.name.clone(),
        art_scale: f.art_scale,
        size: f.size,
        pieces,
        groups: f.glow_groups.clone(),
        materials: f.materials.clone(),
    })
}

/// The asset folder `dir` read and decoded ([`decode`]).
pub fn load_dir(dir: &Path) -> Result<Asset, AssetError> {
    decode(&read_dir(dir)?)
}

/// The asset folder `dir` read, not decoded (`check_asset` not run).
pub fn read_dir(dir: &Path) -> Result<AssetSource, AssetError> {
    if !dir.is_dir() {
        return Err(AssetError::Io(format!("{} is not a folder", dir.display())));
    }
    Ok(read_asset_files(&DirFs::new(dir))?)
}

/// Writes the asset folder `dir` from `src` (made anew: a folder there
/// already is replaced whole, `olr_pack::install::replace_dir`). Returns
/// warnings (an old copy that could not be removed).
pub fn save_dir(dir: &Path, src: &AssetSource) -> Result<Vec<String>, AssetError> {
    let io = |e: &dyn std::fmt::Display| AssetError::Io(format!("{}: {e}", dir.display()));
    olr_pack::install::replace_dir(dir, |tmp| {
        write_asset_files(&mut DirFs::new(tmp), src).map_err(|e| std::io::Error::other(e.to_string()))
    })
    .map_err(|e| io(&e))
}

/// What the Assets tab shows of an asset: its name, its size, what its
/// elements are (rock, dirt, background; a mixed layer "mixed"), whether
/// it glows, and its thumbnail (none when absent or bad).
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub name: String,
    pub size: (u32, u32),
    pub tags: Vec<&'static str>,
    pub glows: bool,
    pub thumb: Option<RgbaImage>,
}

/// The [`Summary`] of asset folder `dir`: `asset.ron` and the thumbnail
/// only (no element decoded: placing reads it whole); `asset.ron` checked
/// for its name, size and paths.
pub fn summary(dir: &Path) -> Result<Summary, AssetError> {
    let src = read_header(dir)?;
    let f = &src.file;
    let mut tags = Vec::new();
    for (tag, m) in [
        ("rock", ElementMaterial::Rock),
        ("dirt", ElementMaterial::Dirt),
        ("background", ElementMaterial::Background),
    ] {
        let has = f.elements.iter().any(|e| match &e.kind {
            ElementKind::Image { material, .. } => *material == m,
            ElementKind::Drawn { material, mixed: false, .. } => *material == m,
            _ => false,
        });
        if has {
            tags.push(tag);
        }
    }
    if f.elements.iter().any(|e| matches!(e.kind, ElementKind::Drawn { mixed: true, .. })) {
        tags.push("mixed");
    }
    let thumb = src
        .thumb
        .as_deref()
        .and_then(|t| olr_format::image_io::decode_rgba_within(t, olr_format::image_io::IMAGES).ok());
    Ok(Summary {
        name: f.name.clone(),
        size: f.size,
        tags,
        glows: f.elements.iter().any(|e| e.glow.is_some()),
        thumb,
    })
}

/// `asset.ron` and the thumbnail of folder `dir`, without the elements'
/// files; the name, art scale, size and paths checked.
fn read_header(dir: &Path) -> Result<AssetSource, AssetError> {
    use olr_format::ron_io::from_canonical;
    use olr_vfs::{ContentSource, VPath};
    let fs = DirFs::new(dir);
    let path = VPath::new(ASSET_FILE).map_err(|e| AssetError::Io(e.to_string()))?;
    let bytes = fs
        .read_max(&path, olr_format::ron_io::MAX_RON_BYTES)
        .map_err(|e| AssetError::Io(format!("{}: {e}", dir.join(ASSET_FILE).display())))?;
    let text = String::from_utf8(bytes).map_err(|_| AssetError::Bad(format!("{ASSET_FILE}: not UTF-8")))?;
    let file: AssetFile = from_canonical(ASSET_FILE, &text)?;
    // The pixel checks need the files: an empty source checks the rest.
    let thumb = {
        let p =
            VPath::new(olr_format::assets::ASSET_THUMB_FILE).map_err(|e| AssetError::Io(e.to_string()))?;
        fs.read_max(&p, olr_format::levels::MAX_THUMB_BYTES).ok().filter(|b| {
            olr_format::image_io::png_size(b).is_some_and(|(w, h)| {
                (1..=olr_format::levels::MAX_THUMB_SIDE).contains(&w)
                    && (1..=olr_format::levels::MAX_THUMB_SIDE).contains(&h)
            })
        })
    };
    let src = AssetSource { thumb, ..AssetSource::new(file, BTreeMap::new()) };
    match check_asset(&src) {
        // Only a missing file may fail here (the files are not read).
        Err(FormatError::Invalid { message, .. }) if message.ends_with(": missing") => Ok(src),
        Err(e) => Err(e.into()),
        Ok(()) => Ok(src),
    }
}

/// A level pack's composition decoded, to cut an asset from (a plain
/// level as its three layers, `compose::split_from`, as the editor opens
/// it): its pieces bottom to top, its size, art scale and look.
pub struct DecodedLevel {
    pub pieces: Vec<Piece>,
    pub size: (u32, u32),
    pub art_scale: u32,
    pub look: Option<LookSource>,
}

impl DecodedLevel {
    /// `pack`'s level decoded; refused for another kind of pack or a
    /// composition that does not check or decode.
    pub fn of_pack(pack: &crate::pack::Pack) -> Result<DecodedLevel, AssetError> {
        let crate::pack::Source::Level(level) = &pack.source else {
            return Err(AssetError::Bad("not a level pack".into()));
        };
        let (size, s) = (level.size, level.art_scale);
        let src = pack.composition.clone().unwrap_or_else(|| crate::compose::split_from(level));
        olr_format::levels::check_composition(&src, size, s)?;
        let masks = decode_glow_masks(&src, size, s)?;
        let maps = decode_surface_maps(&src, size)?;
        let mut cache = ImageCache::new();
        let mut pieces = Vec::new();
        for e in &src.composition.elements {
            let decoded = decode_element(&src, e, size, s, &mut cache)?;
            let original = match &e.kind {
                ElementKind::Image { source, .. } => src
                    .files
                    .get(&olr_format::levels::image_file(source))
                    .map(|b| (source.clone(), Arc::new(b.clone()))),
                _ => None,
            };
            pieces.push(Piece {
                // Its effective flags: a hidden folder's members are cut as
                // hidden (issue #59).
                element: crate::folders::fold(&src.composition, e),
                decoded,
                original,
                glow_mask: masks.get(&e.id).cloned(),
                surface_map: maps.get(&surface_file(&e.id)).cloned(),
                back_surface_map: maps.get(&surface_back_file(&e.id)).cloned(),
            });
        }
        Ok(DecodedLevel { pieces, size, art_scale: s, look: level.look.clone() })
    }

    /// Its pieces as an asset is cut from them.
    pub fn level(&self) -> LevelPieces<'_> {
        LevelPieces {
            pieces: self.pieces.iter().map(Piece::as_ref).collect(),
            size: self.size,
            art_scale: self.art_scale,
            look: self.look.as_ref(),
        }
    }
}

/// What a placing did: the new elements' ids (bottom to top) and the
/// look's changes (its glow groups, then its materials).
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub ids: Vec<String>,
    /// The folder holding the pieces, named after the asset (issue #59);
    /// none for a single piece.
    pub folder: Option<String>,
    pub groups: GroupChange,
    pub materials: MaterialChange,
}

/// Places `asset` in level pack `pack` with its top-left at world pixel
/// `at` (`olr-cli asset place`, as the editor's placing): its glow groups
/// first ([`with_groups`]: joined or added, the default look when the
/// level has none), then its surface materials ([`with_materials`], issue
/// #38: the same), then each piece on top ([`land`]: new ids and names,
/// resampled to the level's art scale, masks kept; its surfaces kept and
/// its painted maps' values re-numbered to the level's materials), their
/// files written
/// into the composition, two pieces or more in a new folder named after
/// the asset (issue #59) (a level without one gets its raster base first,
/// `element::ensure_composed`). Checked whole; the pack is unchanged on a
/// refusal. Not baked: the caller bakes and saves.
pub fn place_in_pack(
    pack: &mut crate::pack::Pack,
    asset: &Asset,
    at: (i32, i32),
) -> Result<Placement, AssetError> {
    let mut copy = pack.clone();
    crate::element::ensure_composed(&mut copy).map_err(|e| AssetError::Bad(e.to_string()))?;
    let stray: BTreeSet<String> = copy.other.keys().map(|p| p.as_str().to_string()).collect();
    let crate::pack::Source::Level(level) = &mut copy.source else {
        return Err(AssetError::Bad("not a level pack".into()));
    };
    let (size, s) = (level.size, level.art_scale);
    let groups = with_groups(level.look.as_ref(), &asset.groups);
    if groups.changes() {
        level.look = Some(groups.look.clone());
    }
    let materials = with_materials(level.look.as_ref(), &asset.materials);
    if materials.changes() {
        level.look = Some(materials.look.clone());
    }
    let Some(src) = copy.composition.as_mut() else { return Err(AssetError::Bad("no composition".into())) };
    let dir = olr_format::levels::image_file("");
    let mut originals: BTreeMap<String, Arc<Vec<u8>>> = src
        .files
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(dir.as_str())?.to_string(), Arc::new(v.clone()))))
        .collect();
    let mut ids = Vec::new();
    for piece in &asset.pieces {
        let taken: BTreeSet<String> = src.files.keys().chain(&stray).cloned().collect();
        let landing = Landing {
            elements: &src.composition.elements,
            folders: &src.composition.folders,
            stray: &taken,
            originals: &originals,
            size,
            art_scale: s,
        };
        let name = piece_name(asset, piece);
        let mut landed = land(&landing, piece.as_ref(), asset.art_scale, at, Some(&name))?;
        if let Some((source, bytes)) = &landed.original {
            src.files.entry(olr_format::levels::image_file(source)).or_insert_with(|| bytes.as_ref().clone());
            originals.insert(source.clone(), bytes.clone());
        }
        for (path, bytes) in encode_decoded(&landed.element, &landed.decoded, size) {
            src.files.insert(path, bytes);
        }
        let e = &mut landed.element;
        let (front, back) = land_surfaces(piece.as_ref(), e, &materials, &asset.materials, size);
        let (w, h) = surface_map_size(&e.kind, size);
        for (file, m) in [(surface_file(&e.id), front), (surface_back_file(&e.id), back)] {
            if let Some(m) = m {
                src.files.insert(file, olr_format::image_io::encode_grey(w, h, &m));
            }
        }
        if let Some(g) = e.glow.as_mut() {
            let b = element_box(&e.kind, size).map(|(_, b)| (b.0 * s, b.1 * s));
            match (landed.glow_mask.as_ref(), b) {
                (Some(m), Some((w, h))) if g.mask && m.len() as u64 == u64::from(w) * u64::from(h) => {
                    src.files.insert(
                        olr_format::levels::glow_file(&e.id),
                        olr_format::image_io::encode_grey(w, h, m),
                    );
                }
                _ => g.mask = false,
            }
        }
        if src.composition.elements.len() >= olr_format::levels::MAX_ELEMENTS {
            return Err(AssetError::Bad(format!(
                "a level holds at most {} elements",
                olr_format::levels::MAX_ELEMENTS
            )));
        }
        ids.push(e.id.clone());
        src.composition.elements.push(landed.element);
    }
    // A multi-piece asset lands as one folder named after it (issue #59);
    // one piece stays plain.
    let folder = if ids.len() > 1 {
        Some(
            crate::folders::group(&mut src.composition, &ids, &asset.name)
                .map_err(|e| AssetError::Bad(e.to_string()))?,
        )
    } else {
        None
    };
    olr_format::levels::check_composition(src, size, s)?;
    olr_format::levels::check_glow_groups(&src.composition, level.look.as_ref())?;
    olr_format::levels::check_surfaces(&src.composition, level.look.as_ref())?;
    let n = level.look.as_ref().map_or(0, |l| l.materials.len());
    olr_format::levels::check_surface_maps(src, size, n)?;
    olr_format::levels::compile_look(level)?;
    *pack = copy;
    Ok(Placement { ids, folder, groups, materials })
}

// The asset library (the editor's host and its tests): a folder of asset
// folders, the built-in one read-only.

/// The most assets a user's library holds (as `textures::MAX_USER_TEXTURES`).
pub const MAX_USER_ASSETS: usize = 256;

/// An asset folder's name in a library: 1 to 64 bytes of `[a-z0-9_-]`,
/// not starting with `-` or `_` (portable everywhere, never hidden).
pub fn valid_asset_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with(['-', '_'])
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// The asset folders of library `dir` (valid ids holding `asset.ron`),
/// sorted; none when it cannot be read.
fn asset_ids(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut ids: Vec<String> = rd
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|id| valid_asset_id(id) && dir.join(id).join(ASSET_FILE).is_file())
        .collect();
    ids.sort();
    ids
}

/// The assets of library `dir` by folder, in name order (then folder), at
/// most `max`, each [`summary`]'s; the folders that do not read as
/// warnings. A missing directory is an empty library.
pub fn list_library(dir: &Path, max: usize) -> (Vec<(String, Summary)>, Vec<String>) {
    let (mut list, mut warnings) = (Vec::new(), Vec::new());
    let mut ids = asset_ids(dir);
    if ids.len() > max {
        warnings.push(format!(
            "the asset library {} holds more than {max} assets: the first {max} are offered",
            dir.display()
        ));
        ids.truncate(max);
    }
    for id in ids {
        match summary(&dir.join(&id)) {
            Ok(s) => list.push((id, s)),
            Err(why) => warnings.push(format!("asset {id}: {why}: not offered")),
        }
    }
    list.sort_by(|a, b| a.1.name.cmp(&b.1.name).then_with(|| a.0.cmp(&b.0)));
    (list, warnings)
}

/// Writes `asset` into the user's library `dir`: over folder `replace`
/// (a valid id), else in a new folder named after it (`torch`, `torch-2`,
/// …), refused past `MAX_USER_ASSETS`. Returns its folder.
pub fn add_to_library(dir: &Path, asset: &Asset, replace: Option<&str>) -> Result<String, AssetError> {
    let src = encode(asset)?;
    let id = match replace {
        Some(id) if valid_asset_id(id) => id.to_string(),
        Some(id) => return Err(AssetError::Bad(format!("{id:?} is no asset's folder"))),
        None => {
            let have = asset_ids(dir);
            if have.len() >= MAX_USER_ASSETS {
                return Err(AssetError::Bad(format!(
                    "your asset library holds at most {MAX_USER_ASSETS} assets: remove one first"
                )));
            }
            let taken: BTreeSet<String> = std::fs::read_dir(dir)
                .map(|rd| {
                    rd.filter_map(Result::ok)
                        .filter_map(|e| e.file_name().to_str().map(str::to_lowercase))
                        .collect()
                })
                .unwrap_or_default();
            let base = asset_slug(&asset.name);
            std::iter::once(base.clone())
                .chain((2..=10_000).map(|i| format!("{base}-{i}")))
                .find(|n| !taken.contains(n))
                .ok_or_else(|| {
                    AssetError::Bad(format!("no free folder for {:?} in your assets", asset.name))
                })?
        }
    };
    std::fs::create_dir_all(dir).map_err(|e| AssetError::Io(format!("{}: {e}", dir.display())))?;
    save_dir(&dir.join(&id), &src)?;
    Ok(id)
}

/// An asset folder's name made from asset name `name`: lower case, runs of
/// anything but a letter or digit one `-`, at most 48 bytes; `asset` when
/// nothing is left.
pub fn asset_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if valid_asset_id(&out) { out } else { "asset".into() }
}

/// Imports the asset folder `from` into the user's library `dir`: read and
/// decoded (checked whole, as placing reads it), then written anew in a
/// folder of its own; a name one of the user's assets has gets a number
/// (`copy_name`). Returns its folder.
pub fn import_into_library(dir: &Path, from: &Path) -> Result<String, AssetError> {
    let mut asset = load_dir(from)?;
    let (yours, _) = list_library(dir, usize::MAX);
    if yours.iter().any(|(_, s)| s.name == asset.name) {
        asset.name = copy_name(&asset.name, yours.iter().map(|(_, s)| s.name.as_str()));
    }
    add_to_library(dir, &asset, None)
}

/// Removes asset folder `id` (a valid id only) from the user's library
/// `dir`.
pub fn remove_from_library(dir: &Path, id: &str) -> Result<(), AssetError> {
    if !valid_asset_id(id) || !dir.join(id).join(ASSET_FILE).is_file() {
        return Err(AssetError::Bad(format!("{id:?} is no asset of yours")));
    }
    std::fs::remove_dir_all(dir.join(id)).map_err(|e| AssetError::Io(format!("asset {id}: {e}")))
}
