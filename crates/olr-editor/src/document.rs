//! The document (plan 14b ruling R2): the open level pack with its
//! elements decoded. Edits change the decoded data and rebake one world
//! rectangle; PNGs are encoded only when saving (`to_pack`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_data::manifest::PackManifest;
use olr_edit::compose::{
    Decoded, GlowMasks, ImageCache, Rect, bake_elements_with, decode_element, decode_glow_masks,
    encode_decoded,
};
use olr_edit::level::refresh_thumb;
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{
    Composition, CompositionSource, Element, ElementKind, GlowLayers, LevelObject, LevelSource,
    check_composition, check_elements, element_box, glow_file, image_file,
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

/// The open level pack.
pub struct Document {
    pub(crate) dir: PathBuf,
    pub(crate) manifest: PackManifest,
    pub(crate) other: BTreeMap<VPath, Vec<u8>>,
    /// The bake: what the canvas shows and Play plays.
    pub(crate) level: LevelSource,
    /// Bottom to top.
    pub(crate) items: Vec<Item>,
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
    /// A plain level's glow and clear flags, made outside the editor (batch
    /// A review I2): kept as they are while no element glows or is clear.
    pub(crate) kept: Option<Kept>,
}

/// What a plain level brought that its layers cannot bake.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Kept {
    pub(crate) glow: Option<GlowLayers>,
    pub(crate) clear: Option<Vec<u8>>,
    /// The look's group ids the glow's group map numbers (1 + index).
    pub(crate) ids: Vec<String>,
}

/// The look's glow group ids, in order.
fn group_ids(l: &LevelSource) -> Vec<String> {
    l.look.iter().flat_map(|k| k.glow_groups.iter().map(|g| g.id.clone())).collect()
}

/// A document's pack, not yet made ([`Document::snapshot`]).
#[derive(Clone, Debug)]
pub struct PackSnapshot {
    manifest: PackManifest,
    other: BTreeMap<VPath, Vec<u8>>,
    level: LevelSource,
    items: Vec<Item>,
    originals: BTreeMap<String, Arc<Vec<u8>>>,
    stored: BTreeMap<String, Vec<u8>>,
    rebaked: bool,
    glow_masks: GlowMasks,
}

impl PackSnapshot {
    /// The pack the document made when the snapshot was taken.
    pub fn to_pack(&self) -> Pack {
        pack_of(
            &self.manifest,
            &self.other,
            &self.level,
            &self.items,
            &self.originals,
            &self.stored,
            self.rebaked,
            &self.glow_masks,
        )
    }
}

/// [`Document::to_pack`] of these parts.
#[allow(clippy::too_many_arguments)]
fn pack_of(
    manifest: &PackManifest,
    other: &BTreeMap<VPath, Vec<u8>>,
    level: &LevelSource,
    items: &[Item],
    originals: &BTreeMap<String, Arc<Vec<u8>>>,
    stored: &BTreeMap<String, Vec<u8>>,
    rebaked: bool,
    glow_masks: &GlowMasks,
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
    }
    let mut level = level.clone();
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
            e
        })
        .collect();
    let composition = Composition { elements };
    Pack {
        manifest: manifest.clone(),
        source: Source::Level(level),
        composition: Some(CompositionSource { composition, files }),
        other: other.clone(),
    }
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
    pub fn open(pack: Pack, dir: PathBuf, catalog: &Catalog) -> Result<Document, DocError> {
        let Pack { manifest, source, composition, other } = pack;
        let Source::Level(level) = source else { return Err(DocError::NotALevel) };
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
        let kept = (plain && (level.glow.is_some() || level.clear.is_some())).then(|| Kept {
            glow: level.glow.clone(),
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
            level: level.clone(),
            items,
            originals,
            cache,
            front_tile: theme.front_tile.clone(),
            back_tile: theme.back_tile.clone(),
            themes: [(theme.id.clone(), (theme.front_tile.clone(), theme.back_tile.clone()))].into(),
            rebaked: false,
            glow_masks,
            kept,
        };
        doc.rebake(doc.all());
        doc.rebaked = (&doc.level.material, &doc.level.front, &doc.level.back)
            != (&level.material, &level.front, &level.back);
        // The look files are baked too (plan 15a): a change of them alone
        // does not touch the thumbnail.
        Ok(doc)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
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
    pub fn rebake(&mut self, rect: Rect) {
        let items: Vec<(&Element, &Decoded)> = self.items.iter().map(|i| (&i.element, &i.decoded)).collect();
        bake_elements_with(&mut self.level, &items, &self.glow_masks, &self.back_tile, rect);
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

    /// The whole level's rectangle.
    pub fn all(&self) -> Rect {
        Rect { x: 0, y: 0, w: self.level.size.0, h: self.level.size.1 }
    }

    /// The pack's files outside the composition (stray ones in `editor/`
    /// among them), by path: a new element's file never overwrites one.
    pub(crate) fn stray_files(&self) -> std::collections::BTreeSet<String> {
        self.other.keys().map(|p| p.as_str().to_string()).collect()
    }

    /// The composition as `composition.ron` holds it.
    pub(crate) fn composition(&self) -> Composition {
        Composition { elements: self.items.iter().map(|i| i.element.clone()).collect() }
    }

    /// The pack to save: the composition (`encode_decoded`, originals as
    /// kept), the bake, the thumbnail refreshed only if anything was
    /// rebaked since `open`.
    pub fn to_pack(&self) -> Pack {
        pack_of(
            &self.manifest,
            &self.other,
            &self.level,
            &self.items,
            &self.originals,
            &self.stored,
            self.rebaked,
            &self.glow_masks,
        )
    }

    /// What `to_pack` reads, copied out: the pack can then be made away from
    /// the document (the host encodes its recovery copy on a thread).
    pub fn snapshot(&self) -> PackSnapshot {
        PackSnapshot {
            manifest: self.manifest.clone(),
            other: self.other.clone(),
            level: self.level.clone(),
            items: self.items.clone(),
            originals: self.originals.clone(),
            stored: self.stored.clone(),
            rebaked: self.rebaked,
            glow_masks: self.glow_masks.clone(),
        }
    }

    /// Ruling R6's live checks: the composition's metadata
    /// (`check_elements`), spawns outside the level, a level without spawns.
    pub fn diagnostics(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        if let Err(e) = check_elements(&self.composition(), self.level.size, self.level.art_scale) {
            out.push(Problem { severity: Severity::Error, text: e.to_string() });
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
        if spawns == 0 {
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
