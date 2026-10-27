//! The document (plan 14b ruling R2): the open level pack with its
//! elements decoded. Edits change the decoded data and rebake one world
//! rectangle; PNGs are encoded only when saving (`to_pack`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_data::manifest::PackManifest;
use olr_edit::compose::{
    Decoded, GlowMasks, ImageCache, PaintedSurfaces, Rect, bake_distant, bake_elements_with_opacity,
    decode_element, decode_glow_masks, decode_surface_maps, encode_decoded, has_distant,
};
use olr_edit::folders;
use olr_edit::level::refresh_thumb;
use olr_edit::pack::{Pack, Source, Verbatim};
use olr_format::levels::{
    Composition, CompositionSource, Element, ElementKind, Folder, GlowBeamMap, GlowLayers, LevelObject,
    GROUND_KEY, LevelSource, PlacedSource, check_composition, check_elements, element_box, glow_file, image_file,
    surface_back_file, surface_file, surface_map_size,
};
use olr_pack::Severity;
use olr_vfs::VPath;

use crate::catalog::Catalog;

/// An element with its decoded data.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub element: Element,
    pub decoded: Decoded,
}

/// A live diagnostic (ruling R6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub severity: Severity,
    pub text: String,
}

/// Why an operation was refused; the document is unchanged.
#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error("not a level pack")]
    NotALevel,
    #[error("theme {0:?} is not installed")]
    NoTheme(String),
    #[error("no element {0:?}")]
    NoElement(String),
    #[error("element {0:?} is locked")]
    Locked(String),
    #[error("element {id:?} is not {want}")]
    WrongKind { id: String, want: &'static str },
    #[error("{0}")]
    Bad(String),
    #[error(transparent)]
    Compose(#[from] olr_edit::compose::ComposeError),
    #[error(transparent)]
    Edit(#[from] olr_edit::level::EditError),
}

/// The status bar's note on a level `open` converted (issue #74).
pub const CONVERTED: &str = "Converted from OLR 0.2: save to keep it";

/// The open level pack.
pub struct Document {
    pub(crate) dir: PathBuf,
    pub(crate) manifest: PackManifest,
    pub(crate) other: BTreeMap<VPath, Vec<u8>>,
    /// The level's placement as opened (issue #74): its ground placed
    /// alone, the items being the ground's layers; `None` for a plain
    /// level. Saved with the ground's layers in it.
    pub(crate) placed: Option<PlacedSource>,
    /// The library copies' files as read (`olr_edit::pack::Verbatim`).
    pub(crate) verbatim: Verbatim,
    /// Whether `open` converted an OLR 0.2 level (issue #74): it opens
    /// unsaved, the status bar saying [`CONVERTED`].
    pub(crate) converted: bool,
    /// The bake: what the canvas shows and Play plays.
    pub(crate) level: LevelSource,
    /// Bottom to top.
    pub(crate) items: Vec<Item>,
    /// The layer folders (issue #60) the items' `folder` names. A folder a
    /// removal or a move emptied may stay listed, so that its undo finds
    /// it; `composition()` leaves such folders out and lists the rest in
    /// the canonical order (`olr_edit::folders::tidy`).
    pub(crate) folders: Vec<Folder>,
    /// Image originals by source name, as stored.
    pub(crate) originals: BTreeMap<String, Arc<Vec<u8>>>,
    pub(crate) cache: ImageCache,
    pub(crate) front_tile: RgbaImage,
    pub(crate) back_tile: RgbaImage,
    /// The tiles of every theme the level has had this session, by id (an
    /// undone theme change takes its tiles back).
    pub(crate) themes: BTreeMap<String, (RgbaImage, RgbaImage)>,
    /// The drawn and raster layers' files as opened, by path: a file whose
    /// pixels did not change is saved as it was (another program's encoding
    /// is kept).
    pub(crate) stored: BTreeMap<String, Vec<u8>>,
    /// Whether anything was rebaked since `open` (the thumbnail is then stale).
    pub(crate) rebaked: bool,
    /// The elements' painted glow masks (look system), by element id: each
    /// its box at art resolution, 0-255.
    pub(crate) glow_masks: GlowMasks,
    /// The elements' painted surface maps as opened (issue #37; the editor
    /// does not paint them yet), by file: baked with, and saved while they
    /// fit their element.
    pub(crate) surface_maps: PaintedSurfaces,
    /// A plain level's glow and clear flags, made outside the editor (batch
    /// A review I2): kept as they are while no element glows or is clear.
    pub(crate) kept: Option<Kept>,
    /// Issue #11: whether the bake is see-through (a visible distant layer,
    /// `has_distant`); a change rebakes the whole level.
    pub(crate) see_through: bool,
    /// The distant items as the level's distant layers were last made from
    /// (`bake_distant` runs again only when they differ).
    pub(crate) distant_items: Vec<Item>,
    /// Their glow masks as they were made from (issue #62: a painted
    /// distant glow makes them again).
    pub(crate) distant_masks: Vec<Option<std::sync::Arc<Vec<u8>>>>,
    /// Counts the changes of the level's distant layers.
    pub(crate) distant_generation: u64,
    /// Counts the rebakes and applied edits (issue #78: the asset tab's
    /// previews are made again when it moved).
    pub(crate) bake_generation: u64,
    /// The back art's see-through pixels (alpha 0), counted.
    pub(crate) holes: u64,
    /// Issue #77: an asset tab's document keeps the asset's states,
    /// exposed properties, sounds, script and health (`open_asset`); `None`
    /// for a level's.
    pub(crate) asset: Option<Box<crate::asset_tab::AssetParts>>,
    /// Issue #79: the look the canvas is lit by when the shown state's
    /// values change its glow groups (`canvas_look`).
    pub(crate) canvas_look: Option<olr_format::look::LookSource>,
    /// Issue #81: the asset tab's copy of a level's ground
    /// (`Document::ground_of`), painted there and applied to the level.
    pub(crate) ground: bool,
    /// Issue #82: the copies laid and the assets decoded, cached.
    pub(crate) copy_cache: crate::copies::CopyCache,
    /// Issue #82: while the level places copies besides its plain ground,
    /// the look it is baked, played and saved with (its own settings, the
    /// copies' groups and materials gathered); `level.look` stays its own
    /// (the ground's groups and materials).
    pub(crate) placed_look: Option<Option<olr_format::look::LookSource>>,
}

/// What a plain level brought that its layers cannot bake.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Kept {
    pub(crate) glow: Option<GlowLayers>,
    /// The glow's beam maps (final review M1), kept with it.
    pub(crate) glow_beams: Option<GlowBeamMap>,
    pub(crate) clear: Option<Vec<u8>>,
    /// The look's group ids the glow's group map numbers (1 + index).
    pub(crate) ids: Vec<String>,
}

/// The look's glow group ids, in order.
fn group_ids(l: &LevelSource) -> Vec<String> {
    l.look.iter().flat_map(|k| k.glow_groups.iter().map(|g| g.id.clone())).collect()
}

/// The see-through (alpha 0) back art pixels within world rectangle `r`.
pub(crate) fn holes(back: &RgbaImage, r: Rect, s: u32) -> u64 {
    let (w, h) = (back.width(), back.height());
    let x1 = r.x.saturating_add(r.w).saturating_mul(s).min(w);
    let y1 = r.y.saturating_add(r.h).saturating_mul(s).min(h);
    let x0 = r.x.saturating_mul(s).min(x1);
    let mut n = 0;
    for y in r.y.saturating_mul(s).min(y1)..y1 {
        let row = &back.as_raw()
            [(y as usize * w as usize + x0 as usize) * 4..(y as usize * w as usize + x1 as usize) * 4];
        n += row.as_chunks::<4>().0.iter().filter(|p| p[3] == 0).count() as u64;
    }
    n
}

/// A document's pack, not yet made ([`Document::snapshot`]).
#[derive(Clone, Debug)]
pub struct PackSnapshot {
    manifest: PackManifest,
    other: BTreeMap<VPath, Vec<u8>>,
    placed: Option<PlacedSource>,
    verbatim: Verbatim,
    level: LevelSource,
    items: Vec<Item>,
    folders: Vec<Folder>,
    originals: BTreeMap<String, Arc<Vec<u8>>>,
    stored: BTreeMap<String, Vec<u8>>,
    rebaked: bool,
    glow_masks: GlowMasks,
    surface_maps: PaintedSurfaces,
    placed_look: Option<Option<olr_format::look::LookSource>>,
}

impl PackSnapshot {
    /// The pack the document made when the snapshot was taken.
    pub fn to_pack(&self) -> Pack {
        pack_of(
            &self.manifest,
            &self.other,
            &self.placed,
            &self.verbatim,
            &self.level,
            &self.items,
            &self.folders,
            &self.originals,
            &self.stored,
            self.rebaked,
            &self.glow_masks,
            &self.surface_maps,
            self.placed_look.as_ref(),
        )
    }
}

/// [`Document::to_pack`] of these parts.
#[allow(clippy::too_many_arguments)]
fn pack_of(
    manifest: &PackManifest,
    other: &BTreeMap<VPath, Vec<u8>>,
    placed: &Option<PlacedSource>,
    verbatim: &Verbatim,
    level: &LevelSource,
    items: &[Item],
    folders: &[Folder],
    originals: &BTreeMap<String, Arc<Vec<u8>>>,
    stored: &BTreeMap<String, Vec<u8>>,
    rebaked: bool,
    glow_masks: &GlowMasks,
    surface_maps: &PaintedSurfaces,
    placed_look: Option<&Option<olr_format::look::LookSource>>,
) -> Pack {
    let mut files = BTreeMap::new();
    for i in items {
        if let ElementKind::Image { source, .. } = &i.element.kind
            && let Some(bytes) = originals.get(source)
        {
            files.insert(image_file(source), bytes.as_ref().clone());
        }
        // Layers, rasters and masks (plan 14d).
        for (path, bytes) in encode_decoded(&i.element, &i.decoded, level.size) {
            let kept = stored.get(&path).filter(|old| same_pixels(&path, old, &bytes));
            files.insert(path, kept.cloned().unwrap_or(bytes));
        }
        // A painted glow mask (look system): its element's box × art scale.
        if i.element.glow.as_ref().is_some_and(|g| g.mask)
            && let Some(mask) = glow_masks.get(&i.element.id)
            && let Some((_, b)) = element_box(&i.element.kind, level.size)
        {
            let (w, h) = (b.0 * level.art_scale, b.1 * level.art_scale);
            if mask.len() as u64 == u64::from(w) * u64::from(h) {
                files.insert(glow_file(&i.element.id), olr_format::image_io::encode_grey(w, h, mask));
            }
        }
        // Painted surface maps (issue #37): its box at world resolution.
        for file in surface_files(&i.element) {
            if let Some(map) = surface_fit(&i.element, &file, surface_maps, level.size) {
                let (w, h) = surface_map_size(&i.element.kind, level.size);
                files.insert(file, olr_format::image_io::encode_grey(w, h, map));
            }
        }
    }
    let mut level = level.clone();
    // Issue #82: a level placing copies saves the gathered look, its
    // ground the level's own groups and materials.
    let mut placed = placed.clone();
    if let Some(look) = placed_look {
        if let Some(g) = placed.as_mut().and_then(|p| p.assets.get_mut(GROUND_KEY)) {
            g.file.glow_groups = level.look.as_ref().map(|l| l.glow_groups.clone()).unwrap_or_default();
            g.file.materials = level.look.as_ref().map(|l| l.materials.clone()).unwrap_or_default();
        }
        level.look = look.clone();
    }
    // A placement without the ground has no composition.
    let grounded = placed.as_ref().is_none_or(|p| p.file.assets.iter().any(|a| a.key == GROUND_KEY));
    // A look changes the thumbnail too (drawn lit, the user's note
    // 2026-10-19).
    if rebaked || level.look.is_some() {
        refresh_thumb(&mut level);
    }
    let fits = |e: &Element| {
        let n = element_box(&e.kind, level.size)
            .map(|(_, b)| (b.0 * level.art_scale) as usize * (b.1 * level.art_scale) as usize);
        glow_masks.get(&e.id).is_some_and(|m| Some(m.len()) == n)
    };
    let elements = items
        .iter()
        .map(|i| {
            let mut e = i.element.clone();
            if let Some(g) = e.glow.as_mut().filter(|g| g.mask && !fits(&i.element)) {
                g.mask = false;
            }
            // A surface map that no longer fits its element is dropped.
            e.surface_map &= surface_fit(&e, &surface_file(&e.id), surface_maps, level.size).is_some();
            e.back_surface_map &=
                surface_fit(&e, &surface_back_file(&e.id), surface_maps, level.size).is_some();
            e
        })
        .collect();
    let composition = tidied(elements, folders);
    Pack {
        manifest: manifest.clone(),
        source: Source::Level(level),
        composition: grounded.then_some(CompositionSource { composition, files }),
        placed,
        legacy_composition: false,
        other: other.clone(),
        verbatim: verbatim.clone(),
    }
}

/// `elements` with the folders of `folders` they use, in the canonical
/// order (`olr_edit::folders::tidy`): the composition as saved.
pub(crate) fn tidied(elements: Vec<Element>, folders: &[Folder]) -> Composition {
    let mut c = Composition { elements, folders: folders.to_vec() };
    folders::tidy(&mut c);
    c
}

/// The painted surface map files element `e` names.
fn surface_files(e: &Element) -> Vec<String> {
    [(surface_file(&e.id), e.surface_map), (surface_back_file(&e.id), e.back_surface_map)]
        .into_iter()
        .filter_map(|(f, on)| on.then_some(f))
        .collect()
}

/// Painted surface map `file` of `e` in a level of `size`, when it is in
/// `maps` and of `e`'s size.
fn surface_fit<'a>(e: &Element, file: &str, maps: &'a PaintedSurfaces, size: (u32, u32)) -> Option<&'a [u8]> {
    let (w, h) = surface_map_size(&e.kind, size);
    maps.get(file).filter(|m| m.len() as u64 == u64::from(w) * u64::from(h)).map(|m| m.as_slice())
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("dir", &self.dir)
            .field("level", &self.level.name)
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

impl Document {
    /// A level pack as the editor edits it: a plain level becomes its
    /// raster base (`compose_from`); every element decoded; the theme's
    /// tiles from `catalog`. The bake is made again from the elements; when
    /// it differs from the stored one the thumbnail is refreshed on save.
    ///
    /// Issue #74: an OLR 0.2 level (`editor/composition.ron`) is converted
    /// (`olr_edit::placed::from_composition`) and opens unsaved; a level
    /// placing assets besides its ground is refused ([`PLACED_ASSETS`]).
    pub fn open(mut pack: Pack, dir: PathBuf, catalog: &Catalog) -> Result<Document, DocError> {
        let converted = pack.legacy_composition;
        if converted {
            olr_edit::placed::from_composition(&mut pack).map_err(|e| DocError::Bad(e.to_string()))?;
        }
        let Pack { manifest, source, composition, placed, other, verbatim, .. } = pack;
        let Source::Level(stored_level) = source else { return Err(DocError::NotALevel) };
        // Issue #82: a level placing more than its ground opens whole: its
        // ground's layers are the items, its own look the ground's groups
        // and materials (the bake gathers the copies' again).
        let mut level = stored_level.clone();
        let composition = match (composition, &placed) {
            (Some(c), _) => Some(c),
            (None, Some(p)) => {
                let ground = p.assets.get(GROUND_KEY);
                let (groups, materials) =
                    ground.map(|g| (g.file.glow_groups.clone(), g.file.materials.clone())).unwrap_or_default();
                if level.look.is_some() || !groups.is_empty() || !materials.is_empty() {
                    let look = level.look.get_or_insert_with(Default::default);
                    look.glow_groups = groups;
                    look.materials = materials;
                }
                Some(ground.map(olr_edit::pack::ground_composition).unwrap_or_default())
            }
            (None, None) => None,
        };
        let theme = catalog
            .themes
            .iter()
            .find(|t| t.id == level.theme)
            .ok_or_else(|| DocError::NoTheme(level.theme.clone()))?;
        // A level without a composition opens as its three layers (the
        // playtest's request: a loaded map can be erased and painted).
        let plain = composition.is_none();
        let src = composition.unwrap_or_else(|| olr_edit::compose::split_from(&level));
        check_composition(&src, level.size, level.art_scale).map_err(|e| DocError::Bad(e.to_string()))?;
        let glow_masks = decode_glow_masks(&src, level.size, level.art_scale)?;
        let surface_maps = decode_surface_maps(&src, level.size)?;
        let kept = (plain && (level.glow.is_some() || level.clear.is_some())).then(|| Kept {
            glow: level.glow.clone(),
            glow_beams: level.glow_beams.clone(),
            clear: level.clear.clone(),
            ids: group_ids(&level),
        });
        let mut cache = ImageCache::new();
        let items = src
            .composition
            .elements
            .iter()
            .map(|e| {
                let decoded = decode_element(&src, e, level.size, level.art_scale, &mut cache)?;
                Ok(Item { element: e.clone(), decoded })
            })
            .collect::<Result<Vec<_>, DocError>>()?;
        let originals = src
            .composition
            .elements
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Image { source, .. } => {
                    let bytes = src.files.get(&image_file(source))?;
                    Some((source.clone(), Arc::new(bytes.clone())))
                }
                _ => None,
            })
            .collect();
        // Every file but the originals (kept by source name).
        let stored = src
            .composition
            .elements
            .iter()
            .flat_map(olr_format::levels::element_files)
            .filter(|f| !f.starts_with(&image_file("")))
            .filter_map(|f| Some((f.clone(), src.files.get(&f)?.clone())))
            .collect();
        let mut doc = Document {
            stored,
            dir,
            manifest,
            other,
            placed,
            verbatim,
            converted,
            level: level.clone(),
            items,
            folders: src.composition.folders.clone(),
            originals,
            cache,
            front_tile: theme.front_tile.clone(),
            back_tile: theme.back_tile.clone(),
            themes: [(theme.id.clone(), (theme.front_tile.clone(), theme.back_tile.clone()))].into(),
            rebaked: false,
            glow_masks,
            surface_maps,
            kept,
            see_through: false,
            distant_items: Vec::new(),
            distant_masks: Vec::new(),
            distant_generation: 0,
            bake_generation: 0,
            holes: 0,
            asset: None,
            canvas_look: None,
            ground: false,
            copy_cache: Default::default(),
            placed_look: None,
        };
        doc.lay_all()?;
        doc.rebake(doc.all());
        doc.rebaked = (&doc.level.material, &doc.level.front, &doc.level.back)
            != (&stored_level.material, &stored_level.front, &stored_level.back);
        // The look files are baked too (plan 15a): a change of them alone
        // does not touch the thumbnail.
        Ok(doc)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether `open` converted an OLR 0.2 level (issue #74).
    pub fn converted(&self) -> bool {
        self.converted
    }

    /// The pack's manifest (its id names the recovery copy, plan 14c).
    pub fn manifest(&self) -> &PackManifest {
        &self.manifest
    }

    pub fn level(&self) -> &LevelSource {
        &self.level
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The theme's front tile (`PaintChoice::FrontTile`).
    pub fn front_tile(&self) -> &RgbaImage {
        &self.front_tile
    }

    /// The theme's back tile (`PaintChoice::BackTile`, the bake's backdrop).
    pub fn back_tile(&self) -> &RgbaImage {
        &self.back_tile
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|i| i.element.id == id)
    }

    /// Rebakes `rect` from the items (`bake_elements_with` their glow
    /// masks) and marks the thumbnail stale. A plain level's own glow and
    /// clear flags stay while no element glows, or is clear.
    ///
    /// Issue #11: a change of whether the bake is see-through (a visible
    /// distant layer) rebakes the whole level; the level's distant layers
    /// are made again when the distant items changed
    /// (`distant_generation` counts it).
    ///
    /// Issue #60: it bakes the effective `visible` (a hidden folder hides
    /// its members), and so compares the distant items: hiding a folder of
    /// distant layers makes them again.
    pub fn rebake(&mut self, rect: Rect) {
        self.bake_generation += 1;
        // Issue #82: a level placing copies bakes them.
        if self.placed_copies() {
            return self.rebake_placed(rect);
        }
        let rect = self.plain_rect(rect);
        let mut baked = self.baked_elements();
        // Issue #79: an asset's shown state, its values applied where
        // things are.
        let pose = self.canvas_pose();
        if let Some(p) = &pose {
            let shown: Vec<bool> = match &baked {
                Some(es) => es.iter().map(|e| e.visible).collect(),
                None => vec![true; self.items.len()],
            };
            baked = Some(
                p.elements
                    .iter()
                    .zip(shown)
                    .map(|(e, v)| Element { visible: e.visible && v, ..e.clone() })
                    .collect(),
            );
        }
        if self.asset.is_some() {
            self.refresh_canvas_look(pose.as_ref());
        }
        let opacity = pose.map(|p| p.opacity).unwrap_or_default();
        let items: Vec<(&Element, &Decoded)> = match &baked {
            Some(es) => es.iter().zip(&self.items).map(|(e, i)| (e, &i.decoded)).collect(),
            None => self.items.iter().map(|i| (&i.element, &i.decoded)).collect(),
        };
        let (size, s) = (self.level.size, self.level.art_scale);
        let see_through = has_distant(&items, size, s);
        let all = Rect { x: 0, y: 0, w: size.0, h: size.1 };
        let rect = if see_through != self.see_through { all } else { rect };
        let whole = rect.x == 0 && rect.y == 0 && rect.w >= size.0 && rect.h >= size.1;
        let holes_before = if whole { 0 } else { holes(&self.level.back, rect, s) };
        bake_elements_with_opacity(
            &mut self.level,
            &items,
            &opacity,
            &self.glow_masks,
            &self.surface_maps,
            &self.back_tile,
            rect,
        );
        let distant = items.iter().filter(|(e, _)| e.distant.is_some()).copied();
        let masks = distant.clone().map(|(e, _)| self.glow_masks.get(&e.id));
        let same_masks = masks.clone().eq(self.distant_masks.iter().map(Option::as_ref));
        if whole
            || !same_masks
            || !distant.clone().eq(self.distant_items.iter().map(|i| (&i.element, &i.decoded)))
        {
            if !whole {
                bake_distant(&mut self.level, &items, &self.glow_masks);
            }
            self.distant_masks = masks.map(|m| m.cloned()).collect();
            self.distant_items =
                distant.map(|(e, d)| Item { element: e.clone(), decoded: d.clone() }).collect();
            self.distant_generation += 1;
        }
        self.see_through = see_through;
        self.holes = if whole {
            holes(&self.level.back, all, s)
        } else {
            (self.holes + holes(&self.level.back, rect, s)).saturating_sub(holes_before)
        };
        if let Some(k) = &self.kept {
            let cells = self.level.material.len();
            let art = (self.level.front.width(), self.level.front.height());
            if !self.items.iter().any(|i| i.element.glow.is_some()) {
                // Its groups follow the look by id: a group gone lights nothing.
                let now = group_ids(&self.level);
                let map: Vec<u8> = std::iter::once(0)
                    .chain(k.ids.iter().map(|id| now.iter().position(|n| n == id).map_or(0, |i| i as u8 + 1)))
                    .collect();
                let remap = |v: &mut u8| *v = map.get(usize::from(*v)).copied().unwrap_or(0);
                self.level.glow = k.glow.clone().filter(|g| g.fits(art, self.level.size)).map(|mut g| {
                    g.groups.iter_mut().for_each(remap);
                    g.groups_back.iter_mut().for_each(remap);
                    g
                });
                // Its beams with it, none without it (as a bake makes them).
                self.level.glow_beams =
                    k.glow_beams.clone().filter(|b| self.level.glow.is_some() && b.fits(self.level.size));
            }
            if !self.items.iter().any(|i| i.element.clear) {
                self.level.clear = k.clear.clone().filter(|c| c.len() == cells);
            }
        }
        self.rebaked = true;
    }

    /// After a whole change (a resize, an art scale, their undo): a plain
    /// level's kept glow and clear flags are the level's as re-laid with it.
    pub(crate) fn keep_from_level(&mut self) {
        let glows = self.items.iter().any(|i| i.element.glow.is_some());
        let clears = self.items.iter().any(|i| i.element.clear);
        if let Some(k) = &mut self.kept {
            if !glows {
                (k.glow, k.ids) = (self.level.glow.clone(), group_ids(&self.level));
                k.glow_beams = self.level.glow_beams.clone();
            }
            if !clears {
                k.clear = self.level.clear.clone();
            }
        }
    }

    /// Element `id`'s painted glow mask (its box at art resolution), if any.
    pub fn glow_mask(&self, id: &str) -> Option<&[u8]> {
        let i = self.index_of(id)?;
        self.items[i].element.glow.as_ref().filter(|g| g.mask)?;
        self.glow_masks.get(id).map(|m| m.as_slice())
    }

    /// Whether the bake is see-through: a visible distant layer shows where
    /// no other background covers the back (issue #11).
    pub fn see_through(&self) -> bool {
        self.see_through
    }

    /// Whether any back pixel is see-through (the level's distant layers
    /// show somewhere).
    pub fn has_holes(&self) -> bool {
        self.holes > 0
    }

    /// Counts the changes of the level's distant layers
    /// (`level().distant`): the canvas sends them again when it changes.
    pub fn distant_generation(&self) -> u64 {
        self.distant_generation
    }

    /// Counts the rebakes and the edits applied (issue #78): what the bake
    /// and the look show may have changed when it moved.
    pub fn bake_generation(&self) -> u64 {
        self.bake_generation
    }

    /// The whole level's rectangle.
    pub fn all(&self) -> Rect {
        Rect { x: 0, y: 0, w: self.level.size.0, h: self.level.size.1 }
    }

    /// The pack's files outside the composition (stray ones in `editor/`
    /// among them), by path: a new element's file never overwrites one.
    pub(crate) fn stray_files(&self) -> std::collections::BTreeSet<String> {
        self.other.keys().map(|p| p.as_str().to_string()).collect()
    }

    /// The composition as `composition.ron` holds it: the folders in use,
    /// in the canonical order.
    pub fn composition(&self) -> Composition {
        tidied(self.items.iter().map(|i| i.element.clone()).collect(), &self.folders)
    }

    /// The layer folders in use (issue #60), in the canonical order: by
    /// where each one's elements start, a parent before its children.
    pub fn folders(&self) -> Vec<Folder> {
        self.composition().folders
    }

    /// The folders as a composition without elements: what the effective
    /// flags read (`olr_edit::folders`' walks up the parents).
    pub(crate) fn folder_view(&self) -> Composition {
        Composition { elements: Vec::new(), folders: self.folders.clone() }
    }

    /// Whether item `i` is shown: it and every folder holding it visible
    /// (`olr_edit::folders::shown`).
    pub fn shown_at(&self, i: usize) -> bool {
        self.items.get(i).is_some_and(|it| {
            it.element.visible
                && (it.element.folder.is_none() || folders::shown(&self.folder_view(), &it.element))
        })
    }

    /// Whether item `i` is locked: it or any folder holding it
    /// (`olr_edit::folders::locked`).
    pub fn locked_at(&self, i: usize) -> bool {
        self.items.get(i).is_some_and(|it| {
            it.element.locked
                || (it.element.folder.is_some() && folders::locked(&self.folder_view(), &it.element))
        })
    }

    /// The items' elements as the bake sees them: a hidden folder hides its
    /// members (issue #60); `None` when no folder is hidden (the items' own).
    fn baked_elements(&self) -> Option<Vec<Element>> {
        if self.folders.iter().all(|f| f.visible) {
            return None;
        }
        let view = self.folder_view();
        Some(
            self.items
                .iter()
                .map(|i| Element { visible: folders::shown(&view, &i.element), ..i.element.clone() })
                .collect(),
        )
    }

    /// The pack to save: the composition (`encode_decoded`, originals as
    /// kept), the bake, the thumbnail refreshed only if anything was
    /// rebaked since `open`.
    pub fn to_pack(&self) -> Pack {
        pack_of(
            &self.manifest,
            &self.other,
            &self.placed,
            &self.verbatim,
            &self.level,
            &self.items,
            &self.folders,
            &self.originals,
            &self.stored,
            self.rebaked,
            &self.glow_masks,
            &self.surface_maps,
            self.placed_look.as_ref(),
        )
    }

    /// What `to_pack` reads, copied out: the pack can then be made away from
    /// the document (the host encodes its recovery copy on a thread).
    pub fn snapshot(&self) -> PackSnapshot {
        PackSnapshot {
            manifest: self.manifest.clone(),
            other: self.other.clone(),
            placed: self.placed.clone(),
            verbatim: self.verbatim.clone(),
            level: self.level.clone(),
            items: self.items.clone(),
            folders: self.folders.clone(),
            originals: self.originals.clone(),
            stored: self.stored.clone(),
            rebaked: self.rebaked,
            glow_masks: self.glow_masks.clone(),
            surface_maps: self.surface_maps.clone(),
            placed_look: self.placed_look.clone(),
        }
    }

    /// Ruling R6's live checks: the composition's metadata
    /// (`check_elements`), the bake's glow beams past
    /// [`MAX_GLOW_BEAMS`](olr_look::glow_beams::MAX_GLOW_BEAMS) (final
    /// review I3: the renderer then draws none, and Save refuses), spawns
    /// outside the level, a level without spawns.
    pub fn diagnostics(&self) -> Vec<Problem> {
        use olr_look::glow_beams::MAX_GLOW_BEAMS;
        let mut out = Vec::new();
        if let Err(e) = check_elements(&self.composition(), self.level.size, self.level.art_scale) {
            out.push(Problem { severity: Severity::Error, text: e.to_string() });
        }
        let beams = olr_format::levels::glow_beam_count(&self.level);
        if beams > MAX_GLOW_BEAMS {
            out.push(Problem {
                severity: Severity::Error,
                text: format!(
                    "{beams} glow beams, above {MAX_GLOW_BEAMS}: none shines until fewer directional patches \
                     glow"
                ),
            });
        }
        let (w, h) = self.level.size;
        let mut spawns = 0;
        for (i, o) in self.level.objects.iter().enumerate() {
            match o {
                LevelObject::Spawn { at, .. } => {
                    spawns += 1;
                    let inside = (0..i64::from(w)).contains(&i64::from(at.0))
                        && (0..i64::from(h)).contains(&i64::from(at.1));
                    if !inside {
                        out.push(Problem {
                            severity: Severity::Warning,
                            text: format!("spawn {} at ({}, {}) is outside the level", i + 1, at.0, at.1),
                        });
                    }
                }
            }
        }
        if self.kept.is_some() {
            out.push(Problem {
                severity: Severity::Warning,
                text: "this level's glow and clear solids were made outside the editor: they are kept as \
                       they are until an element glows or is clear"
                    .into(),
            });
        }
        // An asset has no spawns (issue #77).
        if spawns == 0 && self.asset.is_none() {
            out.push(Problem {
                severity: Severity::Warning,
                text: "the level has no spawn: worms start at free spots".into(),
            });
        }
        out
    }
}

/// Whether layer file `path`'s stored bytes `old` hold the pixels `new`
/// encodes (equal bytes, or both decoding alike: a material as indices,
/// art as RGBA).
fn same_pixels(path: &str, old: &[u8], new: &[u8]) -> bool {
    use olr_format::image_io::{LEVEL_ART, decode_indexed_within, decode_rgba_within};
    if old == new {
        return true;
    }
    if path.ends_with(".material.png") {
        matches!((decode_indexed_within(old, LEVEL_ART), decode_indexed_within(new, LEVEL_ART)), (Ok(a), Ok(b)) if a == b)
    } else {
        matches!((decode_rgba_within(old, LEVEL_ART), decode_rgba_within(new, LEVEL_ART)), (Ok(a), Ok(b)) if a == b)
    }
}
