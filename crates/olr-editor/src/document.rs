//! The document (plan 14b ruling R2): the open level pack with its
//! elements decoded. Edits change the decoded data and rebake one world
//! rectangle; PNGs are encoded only when saving (`to_pack`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_data::manifest::PackManifest;
use olr_edit::compose::{
    Decoded, ImageCache, Rect, bake_elements, compose_from, decode_element, encode_decoded,
};
use olr_edit::level::refresh_thumb;
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{
    Composition, CompositionSource, Element, ElementKind, LevelObject, LevelSource, check_composition,
    check_elements, image_file,
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
        let src = composition.unwrap_or_else(|| compose_from(&level));
        check_composition(&src, level.size, level.art_scale).map_err(|e| DocError::Bad(e.to_string()))?;
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
        let stored = src
            .composition
            .elements
            .iter()
            .filter(|e| !matches!(e.kind, ElementKind::Image { .. }))
            .flat_map(olr_format::levels::element_files)
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
        };
        doc.rebake(doc.all());
        doc.rebaked = (&doc.level.material, &doc.level.front, &doc.level.back)
            != (&level.material, &level.front, &level.back);
        Ok(doc)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
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

    /// Rebakes `rect` from the items (`bake_elements`) and marks the thumbnail stale.
    pub fn rebake(&mut self, rect: Rect) {
        let items: Vec<(&Element, &Decoded)> = self.items.iter().map(|i| (&i.element, &i.decoded)).collect();
        bake_elements(&mut self.level, &items, &self.back_tile, rect);
        self.rebaked = true;
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
        let mut files = BTreeMap::new();
        for i in &self.items {
            match &i.element.kind {
                ElementKind::Image { source, .. } => {
                    if let Some(bytes) = self.originals.get(source) {
                        files.insert(image_file(source), bytes.as_ref().clone());
                    }
                }
                _ => {
                    for (path, bytes) in encode_decoded(&i.element, &i.decoded, self.level.size) {
                        let kept = self.stored.get(&path).filter(|old| same_pixels(&path, old, &bytes));
                        files.insert(path, kept.cloned().unwrap_or(bytes));
                    }
                }
            }
        }
        let mut level = self.level.clone();
        if self.rebaked {
            refresh_thumb(&mut level);
        }
        Pack {
            manifest: self.manifest.clone(),
            source: Source::Level(level),
            composition: Some(CompositionSource { composition: self.composition(), files }),
            other: self.other.clone(),
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
