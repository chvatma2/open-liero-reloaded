//! The level's placed copies in the level tab (issue #82; the asset-based
//! levels design's "The level tab", calls 5 and 6): the document keeps the
//! placement (`Document::placed`) beside the ground's layers (its items),
//! each copy posed and laid (`olr_edit::placed::lay_copy`) cached until
//! the copy or its asset changes, and bakes them as `olr_edit::placed::
//! bake` does, by world rectangle: a moved copy rebakes the union of its
//! old and new boxes, a change of the gathered look (a new glow group or
//! material) the whole level. The copies' operations are `olr_edit::
//! placed`'s (`add_copy`, `move_copy`, `resize_copy`, `duplicate_copy`,
//! `remove_copy`, `raise_copy`, `lower_copy`, `set_copy`), run on a light
//! copy of the placement (the ground stood in for by its header, the
//! level by its sizes and look), each one step: `Edit::Placement`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use olr_edit::asset::{Asset, Piece, decode, decode_own};
use olr_edit::compose::{
    Decoded, GlowMasks, PaintedSurfaces, Rect, bake_distant, bake_elements_with_opacity, has_distant,
};
use olr_edit::pack::{GROUND_NAME, Pack, Source};
use olr_edit::placed::{CopyIds, LaidCopy, PlacedError, Size, gather_copy, lay_copy, same_asset};
use olr_format::assets::{AssetFile, AssetSource};
use olr_format::levels::{
    AssetOrigin, Element, ElementKind, GROUND_KEY, LevelSource, PlacedCopy, PlacedFile, PlacedSource,
    drawn_box, surface_back_file, surface_file,
};
use olr_format::look::LookSource;

use crate::document::{DocError, Document, Item};
use crate::history::{Edit, PlacementSnap};
use crate::ops::{Change, NOTHING};

/// The level tab's refusal of a change of the ground's copy (issue #81,
/// kept by #82: the ground stays the level's, at its size and place).
pub use crate::ground::GROUND_LOCKED;

/// The level tab's refusal of a copy placed under the ground.
pub const GROUND_AT_THE_BOTTOM: &str = "the ground stays at the bottom: a copy under it would not show";

/// What the level tab caches of its copies.
#[derive(Clone, Debug, Default)]
pub(crate) struct CopyCache {
    /// The library copies decoded, by key (never the ground: its layers are
    /// the items).
    pub(crate) decoded: BTreeMap<String, Arc<Asset>>,
    /// Each copy laid, by id: the copy it was laid for (unlocked), the
    /// ground's generation it was laid from (a ground copy's), and the
    /// pieces.
    laid: BTreeMap<String, (PlacedCopy, u64, Arc<LaidCopy>)>,
    /// Counts the changes of the ground's layers (every edit but a
    /// placement's).
    pub(crate) ground: u64,
    /// The glow groups' and materials' ids the bake was last made with: a
    /// change rebakes the whole level.
    baked_ids: Option<Vec<String>>,
}

/// The ids a look numbers its glow groups and materials by, in order.
fn look_ids(l: Option<&LookSource>) -> Vec<String> {
    let groups = l.iter().flat_map(|l| l.glow_groups.iter().map(|g| g.id.clone()));
    let materials = l.iter().flat_map(|l| l.materials.iter().map(|m| format!("/{}", m.id)));
    groups.chain(materials).collect()
}

/// A copy as its pieces depend on it (its lock aside).
fn unlocked(c: &PlacedCopy) -> PlacedCopy {
    PlacedCopy { locked: false, ..c.clone() }
}

/// `r` clipped to a level of `size`, from a box.
fn box_rect((at, size): ((i32, i32), (u32, u32)), level: (u32, u32)) -> Rect {
    let c = |v: i64, side: u32| v.clamp(0, i64::from(side)) as u32;
    let (x0, y0) = (i64::from(at.0), i64::from(at.1));
    let (x1, y1) = (x0 + i64::from(size.0), y0 + i64::from(size.1));
    let (x0, y0, x1, y1) = (c(x0, level.0), c(y0, level.1), c(x1, level.0), c(y1, level.1));
    Rect { x: x0, y: y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
}

/// Whether piece `e` (its data `d`) covers world pixel `p` of a level of
/// `level` at art scale `s`: shown, and its art (or back art) opaque at the
/// pixel's block centre; an image by its processed grid, a raster by the
/// level's art it lays.
pub(crate) fn covers(e: &Element, d: &Decoded, level: (u32, u32), s: u32, p: (i32, i32)) -> bool {
    if !e.visible {
        return false;
    }
    let opaque = |img: &olr_data::image::RgbaImage, (lx, ly): (i64, i64), ps: u32| {
        let s = i64::from(s);
        let (ax, ay) = ((lx * s + s / 2) / i64::from(ps.max(1)), (ly * s + s / 2) / i64::from(ps.max(1)));
        ax >= 0 && ay >= 0 && ax < i64::from(img.width()) && ay < i64::from(img.height()) && {
            img.get(ax as u32, ay as u32).a != 0
        }
    };
    let local = |at: (i32, i32), size: (u32, u32)| {
        let (lx, ly) = (i64::from(p.0) - i64::from(at.0), i64::from(p.1) - i64::from(at.1));
        ((0..i64::from(size.0)).contains(&lx) && (0..i64::from(size.1)).contains(&ly)).then_some((lx, ly))
    };
    match (&e.kind, d) {
        (ElementKind::Image { at, size, pixel_size, .. }, Decoded::Image { grid: Some(g), .. }) => {
            local(*at, *size).is_some_and(|l| opaque(g, l, *pixel_size))
        }
        (ElementKind::Drawn { .. }, Decoded::Drawn { art, back, .. }) => {
            let Some((at, size)) = drawn_box(&e.kind, level) else { return false };
            local(at, size)
                .is_some_and(|l| opaque(art, l, 1) || back.as_ref().is_some_and(|b| opaque(b, l, 1)))
        }
        (ElementKind::Raster, Decoded::Raster { front, back, .. }) => {
            local((0, 0), level).is_some_and(|l| opaque(front, l, 1) || opaque(back, l, 1))
        }
        _ => false,
    }
}

/// A flattened piece of the bake: its element, decoded art and opacity.
type Flat<'a> = (Element, &'a Decoded, u8);

/// A copy's piece as the bake takes it: its element, art, glow mask,
/// surface maps (front, back) and opacity.
type BakePiece<'a> = (Element, &'a Decoded, Option<&'a Arc<Vec<u8>>>, [Option<Arc<Vec<u8>>>; 2], u8);

impl Document {
    /// The level's placement (issue #82): `None` for a plain level until a
    /// copy is placed in it.
    pub fn placement(&self) -> Option<&PlacedSource> {
        self.placed.as_ref()
    }

    /// The level's copies, bottom to top: a plain level's ground alone.
    pub fn copies(&self) -> Vec<PlacedCopy> {
        match &self.placed {
            Some(p) => p.file.copies.clone(),
            None => vec![PlacedCopy { locked: true, ..PlacedCopy::new(GROUND_KEY, GROUND_KEY, (0, 0)) }],
        }
    }

    /// Copy `id`.
    pub fn placed_copy(&self, id: &str) -> Option<PlacedCopy> {
        self.copies().into_iter().find(|c| c.id == id)
    }

    /// Where copy `id`'s asset comes from (`Own` for the ground).
    pub fn copy_origin(&self, id: &str) -> Option<AssetOrigin> {
        let c = self.placed_copy(id)?;
        match &self.placed {
            Some(p) => p.file.assets.iter().find(|a| a.key == c.asset).map(|a| a.origin.clone()),
            None => Some(AssetOrigin::Own),
        }
    }

    /// The name of copy `id`'s asset (the ground's, `ground_name`).
    pub fn copy_name(&self, id: &str) -> Option<String> {
        let c = self.placed_copy(id)?;
        if c.asset == GROUND_KEY {
            return Some(self.ground_name());
        }
        self.placed.as_ref()?.assets.get(&c.asset).map(|a| a.file.name.clone())
    }

    /// The natural size of copy `id`'s asset, world pixels (the ground's is
    /// the level's).
    pub fn natural_size(&self, id: &str) -> Option<(u32, u32)> {
        let c = self.placed_copy(id)?;
        if c.asset == GROUND_KEY {
            return Some(self.level.size);
        }
        self.placed.as_ref()?.assets.get(&c.asset).map(|a| a.file.size)
    }

    /// Copy `id`'s box: its top-left and its placed size.
    pub fn copy_box(&self, id: &str) -> Option<((i32, i32), (u32, u32))> {
        let c = self.placed_copy(id)?;
        let natural = self.natural_size(id)?;
        Some((c.at, c.size.unwrap_or(natural)))
    }

    /// Whether copy `c` is a ground placed as the level's: at `(0, 0)`, of
    /// its size, near, in its start without values; its layers (the items)
    /// then bake as they are.
    pub(crate) fn plain_ground(&self, c: &PlacedCopy) -> bool {
        c.asset == GROUND_KEY
            && c.at == (0, 0)
            && c.size.is_none_or(|s| s == self.level.size)
            && c.distant.is_none()
            && c.state.is_none()
            && c.values.is_empty()
    }

    /// Whether the level places more than its plain ground: the bake is
    /// then the copies' (`rebake_placed`).
    pub(crate) fn placed_copies(&self) -> bool {
        self.asset.is_none()
            && !self.ground
            && self
                .placed
                .as_ref()
                .is_some_and(|p| !matches!(p.file.copies.as_slice(), [c] if self.plain_ground(c)))
    }

    /// The ground's layers as an asset (a ground copy placed otherwise
    /// than as the level's is laid from it).
    fn ground_asset(&self) -> Asset {
        let pieces = self
            .items
            .iter()
            .map(|i| {
                let e = &i.element;
                let map = |file: String, on: bool| self.surface_maps.get(&file).filter(|_| on).cloned();
                Piece {
                    element: e.clone(),
                    decoded: i.decoded.clone(),
                    original: None,
                    glow_mask: self
                        .glow_masks
                        .get(&e.id)
                        .filter(|_| e.glow.as_ref().is_some_and(|g| g.mask))
                        .cloned(),
                    surface_map: map(surface_file(&e.id), e.surface_map),
                    back_surface_map: map(surface_back_file(&e.id), e.back_surface_map),
                }
            })
            .collect();
        let (groups, materials) = self.own_look_parts();
        let mut a =
            Asset::new(&self.ground_name(), self.level.art_scale, self.level.size, pieces, groups, materials);
        a.folders = self.folders.clone();
        a
    }

    /// The level's own look's glow groups and materials (the ground's).
    fn own_look_parts(
        &self,
    ) -> (Vec<olr_format::look::GlowGroupSource>, Vec<olr_format::materials::MaterialSource>) {
        self.level.look.as_ref().map(|l| (l.glow_groups.clone(), l.materials.clone())).unwrap_or_default()
    }

    /// The asset of key `key` decoded (cached): `Own` ones as a level's
    /// own, library copies as library assets.
    fn decoded_asset(&mut self, key: &str) -> Result<Arc<Asset>, DocError> {
        if let Some(a) = self.copy_cache.decoded.get(key) {
            return Ok(a.clone());
        }
        let p = self.placed.as_ref().ok_or_else(|| DocError::Bad("no placement".into()))?;
        let src = p.assets.get(key).ok_or_else(|| DocError::Bad(format!("no asset {key:?}")))?;
        let own = p.file.assets.iter().any(|a| a.key == key && a.origin == AssetOrigin::Own);
        let asset = if own { decode_own(src) } else { decode(src) };
        let asset = Arc::new(asset.map_err(|e| DocError::Bad(format!("asset {key:?}: {e}")))?);
        self.copy_cache.decoded.insert(key.to_string(), asset.clone());
        Ok(asset)
    }

    /// Copy `c` laid (cached until it or its asset changes); `None` for a
    /// plain ground (its layers bake as they are).
    pub(crate) fn laid(&mut self, c: &PlacedCopy) -> Result<Option<Arc<LaidCopy>>, DocError> {
        if self.plain_ground(c) {
            return Ok(None);
        }
        let generation = if c.asset == GROUND_KEY { self.copy_cache.ground } else { 0 };
        let key = unlocked(c);
        if let Some((was, g, laid)) = self.copy_cache.laid.get(&c.id)
            && *was == key
            && *g == generation
        {
            return Ok(Some(laid.clone()));
        }
        let asset =
            if c.asset == GROUND_KEY { Arc::new(self.ground_asset()) } else { self.decoded_asset(&c.asset)? };
        let laid =
            Arc::new(lay_copy(c, &asset, self.level.art_scale).map_err(|e| DocError::Bad(e.to_string()))?);
        self.copy_cache.laid.insert(c.id.clone(), (key, generation, laid.clone()));
        Ok(Some(laid))
    }

    /// Copy `c`'s extent in the level: its box and its pieces' (a plain
    /// ground's, the whole level).
    fn extent(&mut self, c: &PlacedCopy) -> Rect {
        let size = self.level.size;
        if self.plain_ground(c) {
            return self.all();
        }
        let natural = if c.asset == GROUND_KEY {
            size
        } else {
            self.placed.as_ref().and_then(|p| p.assets.get(&c.asset)).map_or((1, 1), |a| a.file.size)
        };
        let mut r = box_rect((c.at, c.size.unwrap_or(natural)), size);
        if let Ok(Some(l)) = self.laid(c) {
            for p in &l.pieces {
                let b = box_rect(p.world_box(size), size);
                if b.w > 0 && b.h > 0 {
                    r = Rect::union(Some(r), b);
                }
            }
        }
        r
    }

    /// Lays every copy, refusing one that does not (an asset that does not
    /// decode, a value posing refuses): `open`'s check.
    pub(crate) fn lay_all(&mut self) -> Result<(), DocError> {
        for c in self.placed.as_ref().map(|p| p.file.copies.clone()).unwrap_or_default() {
            self.laid(&c)?;
        }
        Ok(())
    }

    /// The topmost copy with a covered pixel at world pixel `p` (its posed
    /// pieces' art: a transparent pixel falls through to what is behind),
    /// else the topmost copy whose box holds it.
    pub fn copy_at(&self, p: (i32, i32)) -> Option<String> {
        let (level, s) = (self.level.size, self.level.art_scale);
        let copies = self.copies();
        let covered = copies.iter().rev().find(|c| {
            if self.plain_ground(c) || self.placed.is_none() {
                return self
                    .items
                    .iter()
                    .enumerate()
                    .rev()
                    .any(|(i, it)| self.shown_at(i) && covers(&it.element, &it.decoded, level, s, p));
            }
            let Some((_, _, l)) = self.copy_cache.laid.get(&c.id) else { return false };
            l.pieces.iter().any(|piece| covers(&piece.element, &piece.decoded, level, s, p))
        });
        let found = covered.or_else(|| {
            copies.iter().rev().find(|c| {
                let Some((at, size)) = self.copy_box(&c.id) else { return false };
                let (dx, dy) = (i64::from(p.0) - i64::from(at.0), i64::from(p.1) - i64::from(at.1));
                (0..i64::from(size.0)).contains(&dx) && (0..i64::from(size.1)).contains(&dy)
            })
        });
        found.map(|c| c.id.clone())
    }

    /// The look the level is baked, played and saved with while it places
    /// copies (`Some`): its own settings with the copies' glow groups and
    /// materials gathered in copy order; `None` while it is its ground
    /// alone (its own look is the one).
    pub fn placed_look(&self) -> Option<&Option<LookSource>> {
        self.placed_look.as_ref()
    }

    /// The bake as Play plays it: the level with the gathered look; issue
    /// #85: with Live copies, the saved pack's (their played form, the
    /// ground without them).
    pub fn playable(&self) -> LevelSource {
        if self.placed.as_ref().is_some_and(|p| p.file.copies.iter().any(|c| c.live))
            && let Source::Level(l) = self.to_pack().source
        {
            return l;
        }
        let mut l = self.level.clone();
        if let Some(look) = &self.placed_look {
            l.look = look.clone();
        }
        l
    }

    /// The bake of `rect` from the copies (issue #82): every copy laid,
    /// the ground's layers for a plain ground, gathered and flattened as
    /// `olr_edit::placed::bake` flattens them (distant pieces first), and
    /// baked under the gathered look; the whole level when the gathered
    /// ids changed or the bake turned see-through or back.
    pub(crate) fn rebake_placed(&mut self, rect: Rect) {
        let copies = self.placed.as_ref().map(|p| p.file.copies.clone()).unwrap_or_default();
        let laid: Vec<Option<Arc<LaidCopy>>> = copies.iter().map(|c| self.laid(c).ok().flatten()).collect();
        let own = self.level.look.clone();
        let (own_groups, own_materials) = self.own_look_parts();
        let (mut groups, mut materials) = (Vec::new(), Vec::new());
        let ids: Vec<Option<CopyIds>> = copies
            .iter()
            .zip(&laid)
            .map(|(c, l)| match l {
                Some(l) => Some(gather_copy(&mut groups, &mut materials, &c.id, &l.groups, &l.materials)),
                None if self.plain_ground(c) => {
                    Some(gather_copy(&mut groups, &mut materials, &c.id, &own_groups, &own_materials))
                }
                None => None,
            })
            .collect();
        let gathered = (own.is_some() || !groups.is_empty() || !materials.is_empty()).then(|| LookSource {
            glow_groups: groups,
            materials,
            ..own.clone().unwrap_or_default()
        });
        let (size, s) = (self.level.size, self.level.art_scale);
        let all = Rect { x: 0, y: 0, w: size.0, h: size.1 };
        let key = look_ids(gathered.as_ref());
        let mut rect = if self.copy_cache.baked_ids.as_ref() != Some(&key) { all } else { rect };
        // The flattened pieces, their masks and maps by their ids.
        let view = self.folder_view();
        let (mut distant, mut near): (Vec<Flat>, Vec<Flat>) = (Vec::new(), Vec::new());
        let mut masks = GlowMasks::new();
        let mut surfaces = PaintedSurfaces::new();
        for ((c, l), ids) in copies.iter().zip(&laid).zip(&ids) {
            let Some(ids) = ids else { continue };
            let pieces: Vec<BakePiece> = match l {
                Some(l) => l
                    .pieces
                    .iter()
                    .map(|p| {
                        let maps = [p.surface_map.clone(), p.back_surface_map.clone()];
                        (p.element.clone(), &p.decoded, p.glow_mask.as_ref(), maps, p.opacity)
                    })
                    .collect(),
                None if self.plain_ground(c) => self
                    .items
                    .iter()
                    .map(|i: &Item| {
                        let mut e = olr_edit::folders::fold(&view, &i.element);
                        e.folder = None;
                        let maps = [
                            self.surface_maps.get(&surface_file(&e.id)).cloned(),
                            self.surface_maps.get(&surface_back_file(&e.id)).cloned(),
                        ];
                        (e, &i.decoded, self.glow_masks.get(&i.element.id), maps, 100)
                    })
                    .collect(),
                None => Vec::new(),
            };
            for (mut e, d, mask, [front, back], opacity) in pieces {
                ids.name(&mut e);
                if let Some(m) = mask {
                    masks.insert(e.id.clone(), m.clone());
                }
                if let Some(m) = ids.renumbered(&front) {
                    surfaces.insert(surface_file(&e.id), m);
                }
                if let Some(m) = ids.renumbered(&back) {
                    surfaces.insert(surface_back_file(&e.id), m);
                }
                if e.is_distant() { distant.push((e, d, opacity)) } else { near.push((e, d, opacity)) }
            }
        }
        let flat: Vec<(Element, &Decoded, u8)> = distant.into_iter().chain(near).collect();
        let items: Vec<(&Element, &Decoded)> = flat.iter().map(|(e, d, _)| (e, *d)).collect();
        let opacity: Vec<u8> = flat.iter().map(|f| f.2).collect();
        let see_through = has_distant(&items, size, s);
        if see_through != self.see_through {
            rect = all;
        }
        let whole = rect.x == 0 && rect.y == 0 && rect.w >= size.0 && rect.h >= size.1;
        let holes_before = if whole { 0 } else { crate::document::holes(&self.level.back, rect, s) };
        let own_look = std::mem::replace(&mut self.level.look, gathered.clone());
        bake_elements_with_opacity(
            &mut self.level,
            &items,
            &opacity,
            &masks,
            &surfaces,
            &self.back_tile,
            rect,
        );
        let distant = items.iter().filter(|(e, _)| e.distant.is_some()).copied();
        let dmasks = distant.clone().map(|(e, _)| masks.get(&e.id));
        let same_masks = dmasks.clone().eq(self.distant_masks.iter().map(Option::as_ref));
        if whole
            || !same_masks
            || !distant.clone().eq(self.distant_items.iter().map(|i| (&i.element, &i.decoded)))
        {
            if !whole {
                bake_distant(&mut self.level, &items, &masks);
            }
            self.distant_masks = dmasks.map(|m| m.cloned()).collect();
            self.distant_items =
                distant.map(|(e, d)| Item { element: e.clone(), decoded: d.clone() }).collect();
            self.distant_generation += 1;
        }
        self.level.look = own_look;
        self.see_through = see_through;
        let all_holes = crate::document::holes(&self.level.back, all, s);
        self.holes = if whole {
            all_holes
        } else {
            (self.holes + crate::document::holes(&self.level.back, rect, s)).saturating_sub(holes_before)
        };
        self.copy_cache.baked_ids = Some(key);
        self.canvas_look = gathered.clone();
        self.placed_look = Some(gathered);
        self.rebaked = true;
    }

    /// The rectangle a plain bake (the items alone, under the own look)
    /// rebakes for `rect`: the whole level when the look's ids are not
    /// those the last bake had (a placed bake's gathered ones). Notes them.
    pub(crate) fn plain_rect(&mut self, rect: Rect) -> Rect {
        let key = look_ids(self.level.look.as_ref());
        let was = self.copy_cache.baked_ids.replace(key.clone());
        if self.asset.is_none() && !self.ground {
            self.canvas_look = None;
        }
        self.placed_look = None;
        if was.is_some_and(|w| w != key) { self.all() } else { rect }
    }

    /// The placement to start an edit from: the level's, or a plain
    /// level's ground placed alone (`olr_edit::pack::ground_placement` of
    /// no files: the composition fills it when saved).
    fn placement_or_ground(&self) -> PlacedSource {
        match &self.placed {
            Some(p) => p.clone(),
            None => olr_edit::pack::ground_placement(&self.level, &Default::default()),
        }
    }

    /// The placement with the ground's asset stood in for by its header
    /// (the level's size and art scale, the own look's groups and
    /// materials, no layers): what the copy operations check cheaply.
    fn light_placement(&self) -> PlacedSource {
        let (groups, materials) = self.own_look_parts();
        // One clear world pixel stands in for the layers: an asset holds
        // one element at least (`check_own_asset`).
        let s = self.level.art_scale.max(1);
        let stand_in = olr_format::levels::Element {
            id: "stand-in".into(),
            name: String::new(),
            visible: true,
            locked: false,
            folder: None,
            kind: olr_format::levels::ElementKind::Drawn {
                material: olr_format::levels::ElementMaterial::Background,
                at: (0, 0),
                size: Some((1, 1)),
                mixed: false,
                back: false,
            },
            glow: None,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        let art = olr_format::image_io::encode_rgba(&olr_data::image::RgbaImage::new(s, s));
        let light = |a: &AssetSource| {
            let mut file = AssetFile::new(&a.file.name, self.level.size, vec![stand_in.clone()]);
            file.art_scale = self.level.art_scale;
            file.glow_groups = groups.clone();
            file.materials = materials.clone();
            let files = BTreeMap::from([("layers/stand-in.png".to_string(), art.clone())]);
            AssetSource::new(file, files)
        };
        let ground =
            AssetSource::new(AssetFile::new(GROUND_NAME, self.level.size, Vec::new()), BTreeMap::new());
        match &self.placed {
            Some(p) => PlacedSource {
                file: p.file.clone(),
                assets: p
                    .assets
                    .iter()
                    .map(|(k, a)| (k.clone(), if k == GROUND_KEY { light(a) } else { a.clone() }))
                    .collect(),
                scripts: p.scripts.clone(),
                level_script: p.level_script.clone(),
            },
            None => {
                let mut p = olr_edit::pack::ground_placement(&self.level, &Default::default());
                p.assets.insert(GROUND_KEY.into(), light(&ground));
                p
            }
        }
    }

    /// Runs `op` (one of `olr_edit::placed`'s operations) on a light pack
    /// of the placement (`light_placement`; the level's sizes and own
    /// look) and makes its change `Edit::Placement`, not applied; a
    /// refusal is `DocError::Bad` with the operation's reason.
    pub(crate) fn placement_op<T>(
        &self,
        op: impl FnOnce(&mut Pack) -> Result<T, PlacedError>,
    ) -> Result<(T, Edit), DocError> {
        self.placement_op_with(op)
    }

    /// [`Document::placement_op`] of an operation of any error (issue #83:
    /// `olr_edit::library`'s).
    pub(crate) fn placement_op_with<T, E: std::fmt::Display>(
        &self,
        op: impl FnOnce(&mut Pack) -> Result<T, E>,
    ) -> Result<(T, Edit), DocError> {
        let before = self.light_placement();
        let l = &self.level;
        let light = LevelSource {
            name: l.name.clone(),
            notes: String::new(),
            size: l.size,
            art_scale: l.art_scale,
            theme: l.theme.clone(),
            objects: Vec::new(),
            material: Vec::new(),
            front: olr_data::image::RgbaImage::new(0, 0),
            back: olr_data::image::RgbaImage::new(0, 0),
            thumb: None,
            look: l.look.clone(),
            glow: None,
            clear: None,
            glass: None,
            glow_beams: None,
            haze: None,
            distant: Vec::new(),
            surface: None,
            distant_surfaces: Vec::new(),
            live: None,
        };
        let mut pack = Pack::new(self.manifest.clone(), Source::Level(light));
        pack.placed = Some(before.clone());
        let out = op(&mut pack).map_err(|e| DocError::Bad(e.to_string()))?;
        let after = pack.placed.take().unwrap_or_default();
        Ok((out, placement_edit(&before, &after)))
    }

    /// Makes the placement `to` (an `Edit::Placement`'s side): its file and
    /// scripts, the assets it lists (those it carries put in, those it no
    /// longer lists taken out; the ground's kept). Returns the rectangle
    /// to rebake: each changed copy's old and new extent (a lock changes
    /// none), both of two copies that changed places.
    pub(crate) fn set_placement(&mut self, to: &PlacementSnap) -> Rect {
        if self.placed.is_none() {
            self.placed = Some(self.placement_or_ground());
        }
        let from = self.placed.as_ref().map(|p| p.file.copies.clone()).unwrap_or_default();
        let find = |cs: &[PlacedCopy], id: &str| cs.iter().find(|c| c.id == id).map(unlocked);
        // Assets whose content changes: their copies are laid again.
        let changed_keys: BTreeSet<String> = {
            let p = self.placed.as_ref().expect("made above");
            let listed: BTreeSet<&str> = to.file.assets.iter().map(|a| a.key.as_str()).collect();
            let gone = p.assets.keys().filter(|k| *k != GROUND_KEY && !listed.contains(k.as_str())).cloned();
            let new = to.assets.iter().filter(|(k, a)| p.assets.get(*k).is_none_or(|b| !same_asset(a, b)));
            gone.chain(new.map(|(k, _)| k.clone())).collect()
        };
        let mut changed: BTreeSet<String> = BTreeSet::new();
        for c in from.iter().chain(&to.file.copies) {
            if find(&from, &c.id) != find(&to.file.copies, &c.id) || changed_keys.contains(&c.asset) {
                changed.insert(c.id.clone());
            }
        }
        // Two copies that changed places among those both sides hold.
        let common = |cs: &[PlacedCopy], other: &[PlacedCopy]| -> Vec<String> {
            cs.iter().filter(|c| other.iter().any(|o| o.id == c.id)).map(|c| c.id.clone()).collect()
        };
        for (a, b) in common(&from, &to.file.copies).into_iter().zip(common(&to.file.copies, &from)) {
            if a != b {
                changed.insert(a);
                changed.insert(b);
            }
        }
        let mut dirty: Option<Rect> = None;
        let add = |dirty: &mut Option<Rect>, r: Rect| {
            if r.w > 0 && r.h > 0 {
                *dirty = Some(Rect::union(*dirty, r));
            }
        };
        for c in from.iter().filter(|c| changed.contains(&c.id)) {
            let r = self.extent(c);
            add(&mut dirty, r);
        }
        let p = self.placed.as_mut().expect("made above");
        p.assets.retain(|k, _| k == GROUND_KEY || to.file.assets.iter().any(|a| a.key == *k));
        for (k, a) in &to.assets {
            p.assets.insert(k.clone(), (**a).clone());
        }
        p.file = to.file.clone();
        p.scripts = to.scripts.clone();
        p.level_script = to.level_script.clone();
        for k in &changed_keys {
            self.copy_cache.decoded.remove(k);
        }
        self.copy_cache.laid.retain(|id, (c, _, _)| {
            !changed_keys.contains(&c.asset) && to.file.copies.iter().any(|x| x.id == *id)
        });
        for c in to.file.copies.clone().iter().filter(|c| changed.contains(&c.id)) {
            let r = self.extent(c);
            add(&mut dirty, r);
        }
        dirty.unwrap_or(NOTHING)
    }

    /// Whether copy `id` is the ground's (it never moves, resizes, goes or
    /// unlocks in the level tab).
    pub fn is_ground_copy(&self, id: &str) -> bool {
        self.placed_copy(id).is_some_and(|c| c.asset == GROUND_KEY)
    }

    /// Refuses a change of the ground's copies among `ids`.
    fn not_ground(&self, ids: &[String]) -> Result<(), DocError> {
        match ids.iter().any(|id| self.is_ground_copy(id)) {
            true => Err(DocError::Bad(GROUND_LOCKED.into())),
            false => Ok(()),
        }
    }

    /// Adds a copy of `asset` (from `origin`) with its top-left at `at`, on
    /// top (`olr_edit::placed::add_copy`): one step; its id.
    pub fn add_copy(
        &mut self,
        asset: &AssetSource,
        origin: AssetOrigin,
        at: (i32, i32),
    ) -> Result<(Change, olr_edit::placed::Added), DocError> {
        let (added, edit) =
            self.placement_op(|pack| olr_edit::placed::add_copy(pack, asset, origin, at, None, None))?;
        Ok((self.change(edit), added))
    }

    /// Moves each copy `(id, at)` to its place (`move_copy` each): one step.
    pub fn move_copies(&mut self, moves: &[(String, (i32, i32))]) -> Result<Change, DocError> {
        let ids: Vec<String> = moves.iter().map(|m| m.0.clone()).collect();
        self.not_ground(&ids)?;
        let ((), edit) = self.placement_op(|pack| {
            moves.iter().try_for_each(|(id, at)| olr_edit::placed::move_copy(pack, id, *at))
        })?;
        Ok(self.change(edit))
    }

    /// Resizes copy `id` to `size` (`resize_copy`) with its top-left at
    /// `at`: one step.
    pub fn resize_copy(&mut self, id: &str, size: Size, at: (i32, i32)) -> Result<Change, DocError> {
        self.not_ground(&[id.to_string()])?;
        let ((), edit) = self.placement_op(|pack| {
            olr_edit::placed::resize_copy(pack, id, size)?;
            olr_edit::placed::move_copy(pack, id, at)
        })?;
        Ok(self.change(edit))
    }

    /// Sets one of copy `id`'s settings (`set_copy`): one step.
    pub fn set_copy(
        &mut self,
        id: &str,
        property: olr_edit::placed::CopyProperty,
    ) -> Result<Change, DocError> {
        self.not_ground(&[id.to_string()])?;
        let ((), edit) = self.placement_op(|pack| olr_edit::placed::set_copy(pack, id, property))?;
        Ok(self.change(edit))
    }

    /// Removes copies `ids` (`remove_copy` each; an asset no copy uses
    /// goes with it): one step; what each took.
    pub fn remove_copies(
        &mut self,
        ids: &[String],
    ) -> Result<(Change, Vec<olr_edit::placed::Removed>), DocError> {
        self.not_ground(ids)?;
        let (removed, edit) = self.placement_op(|pack| {
            ids.iter().map(|id| olr_edit::placed::remove_copy(pack, id)).collect::<Result<Vec<_>, _>>()
        })?;
        Ok((self.change(edit), removed))
    }

    /// Duplicates copies `ids` `offset` further (`duplicate_copy` each),
    /// each directly above its own: one step; the new ids.
    pub fn duplicate_copies(
        &mut self,
        ids: &[String],
        offset: (i32, i32),
    ) -> Result<(Change, Vec<String>), DocError> {
        self.not_ground(ids)?;
        let (new, edit) = self.placement_op(|pack| {
            ids.iter()
                .map(|id| olr_edit::placed::duplicate_copy(pack, id, offset))
                .collect::<Result<Vec<_>, _>>()
        })?;
        Ok((self.change(edit), new))
    }

    /// Moves copy `id` to place `to` in the stack (bottom 0) by
    /// `raise_copy`/`lower_copy` steps: one step. Nothing goes under the
    /// ground.
    pub fn restack_copy(&mut self, id: &str, to: usize) -> Result<Change, DocError> {
        self.not_ground(&[id.to_string()])?;
        let copies = self.copies();
        let Some(from) = copies.iter().position(|c| c.id == id) else {
            return Err(DocError::Bad(format!("no copy {id:?}")));
        };
        let to = to.min(copies.len().saturating_sub(1));
        if copies.iter().position(|c| c.asset == GROUND_KEY).is_some_and(|g| to <= g && from > g) {
            return Err(DocError::Bad(GROUND_AT_THE_BOTTOM.into()));
        }
        let ((), edit) = self.placement_op(|pack| {
            for _ in to..from {
                olr_edit::placed::lower_copy(pack, id)?;
            }
            for _ in from..to {
                olr_edit::placed::raise_copy(pack, id)?;
            }
            Ok(())
        })?;
        Ok(self.change(edit))
    }

    /// Moves copies `(id, at)` live (a drag): no step, no check but the
    /// format's bound on a place; the rectangle rebaked.
    pub(crate) fn live_move(&mut self, moves: &[(String, (i32, i32))]) -> Rect {
        let Some(p) = &self.placed else { return NOTHING };
        let olds: Vec<PlacedCopy> =
            p.file.copies.iter().filter(|c| moves.iter().any(|m| m.0 == c.id)).cloned().collect();
        let mut dirty: Option<Rect> = None;
        let add = |dirty: &mut Option<Rect>, r: Rect| {
            if r.w > 0 && r.h > 0 {
                *dirty = Some(Rect::union(*dirty, r));
            }
        };
        for c in &olds {
            let r = self.extent(c);
            add(&mut dirty, r);
        }
        let bound = olr_format::levels::MAX_COPY_AT;
        let Some(p) = self.placed.as_mut() else { return NOTHING };
        for (id, at) in moves {
            if let Some(c) = p.file.copies.iter_mut().find(|c| c.id == *id) {
                c.at = (at.0.clamp(-bound, bound), at.1.clamp(-bound, bound));
            }
        }
        let news: Vec<PlacedCopy> =
            p.file.copies.iter().filter(|c| moves.iter().any(|m| m.0 == c.id)).cloned().collect();
        for c in &news {
            let r = self.extent(c);
            add(&mut dirty, r);
        }
        let dirty = dirty.unwrap_or(NOTHING);
        if dirty.w > 0 && dirty.h > 0 {
            self.rebake(dirty);
        }
        dirty
    }

    /// The level's own asset `key` (issue #82: Edit in level), not the
    /// ground's (its layers are the items).
    pub fn own_asset(&self, key: &str) -> Option<&AssetSource> {
        let p = self.placed.as_ref()?;
        let own = p.file.assets.iter().any(|a| a.key == key && a.origin == AssetOrigin::Own);
        p.assets.get(key).filter(|_| own && key != GROUND_KEY)
    }

    /// The level's own asset `key` made `source` (edited in the asset tab):
    /// one step.
    pub(crate) fn set_own_asset(&mut self, key: &str, source: AssetSource) -> Result<Change, DocError> {
        if self.own_asset(key).is_none() {
            return Err(DocError::Bad(format!("the level has no asset {key:?} of its own")));
        }
        let ((), edit) = self.placement_op(|pack| {
            if let Some(p) = pack.placed.as_mut() {
                p.assets.insert(key.to_string(), source);
            }
            Ok(())
        })?;
        Ok(self.change(edit))
    }

    /// The placement's file made `after` (its copies moved together, a
    /// level resize's): one step.
    pub(crate) fn file_change(&mut self, after: PlacedFile) -> Result<Change, DocError> {
        let ((), edit) = self.placement_op(|pack| {
            if let Some(p) = pack.placed.as_mut() {
                p.file = after;
            }
            Ok(())
        })?;
        Ok(self.change(edit))
    }

    /// The placement's file (a drag's start).
    pub(crate) fn placement_file(&self) -> PlacedFile {
        self.placement_or_ground().file
    }

    /// Ends a live move begun at `before` (the placement's file then): the
    /// moves made again from it by `move_copy` as one step (applied, the
    /// canvas showing them already); a refusal puts the copies back.
    pub(crate) fn end_move(
        &mut self,
        before: &PlacedFile,
        moves: &[(String, (i32, i32))],
    ) -> Result<Change, DocError> {
        let back: Vec<(String, (i32, i32))> = moves
            .iter()
            .filter_map(|(id, _)| Some((id.clone(), before.copies.iter().find(|c| c.id == *id)?.at)))
            .collect();
        let live = self.placed.as_ref().map(|p| p.file.clone());
        if let Some(p) = self.placed.as_mut() {
            p.file = before.clone();
        }
        let made = self.placement_op(|pack| {
            moves.iter().try_for_each(|(id, at)| olr_edit::placed::move_copy(pack, id, *at))
        });
        if let (Some(p), Some(live)) = (self.placed.as_mut(), live) {
            p.file = live;
        }
        match made {
            Ok(((), edit)) => Ok(self.change(edit)),
            Err(e) => {
                self.live_move(&back);
                Err(e)
            }
        }
    }
}

/// The step from placement `before` to `after`: each side's file and
/// scripts, and the assets it has that the other lacks or holds otherwise
/// (the ground's never: it is the items).
pub(crate) fn placement_edit(before: &PlacedSource, after: &PlacedSource) -> Edit {
    let side = |a: &PlacedSource, b: &PlacedSource| PlacementSnap {
        file: a.file.clone(),
        scripts: a.scripts.clone(),
        level_script: a.level_script.clone(),
        assets: a
            .assets
            .iter()
            .filter(|(k, _)| *k != GROUND_KEY)
            .filter(|(k, x)| {
                !b.file.assets.iter().any(|l| l.key == **k)
                    || b.assets.get(*k).is_none_or(|y| !same_asset(x, y))
            })
            .map(|(k, x)| (k.clone(), Arc::new(x.clone())))
            .collect(),
    };
    Edit::Placement { before: Arc::new(side(before, after)), after: Arc::new(side(after, before)) }
}
