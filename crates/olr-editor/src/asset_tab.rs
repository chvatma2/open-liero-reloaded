//! The asset tab's document (issue #77): a library asset opened as a
//! `Document` whose bake is a level of the asset's size and art scale under
//! the open level's theme (its tiles, for the tile paints and the craters),
//! with no spawns and the asset's look (`olr_edit::asset::preview_look`
//! with its glow groups and materials); `to_asset` makes the asset again.
//! The states, exposed properties, sounds, script and health are kept as
//! opened (issues #79 and #80 edit them); `refusal` says why an edit would
//! leave them invalid, so that the editor refuses it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::asset::{Asset, Piece, State, carried_look, preview_look, property_label};
use olr_edit::compose::{Decoded, ImageCache};
use olr_format::assets::{
    AssetFile, ExposedSource, HealthSource, MAX_ASSET_NAME, check_states, property_type,
};
use olr_format::levels::{
    Element, ElementKind, ElementMaterial, LevelSource, MAX_ART_SCALE, MAX_SIDE, element_box,
    surface_back_file, surface_file,
};

use crate::catalog::Catalog;
use crate::document::{DocError, Document, Item};

/// The theme an asset opens under when no level is open (`olr-ui`'s New
/// level default).
pub const DEFAULT_THEME: &str = "olx-theme-dirt";

/// The status line's note on a built-in asset open in the asset tab.
pub const BUILT_IN_NOTE: &str = "Built-in: saving makes your own copy";

/// A new asset's one layer (an asset holds at least one element).
pub const FIRST_LAYER: &str = "Layer 1";

/// What the asset tab keeps of an asset besides its layers and look: its
/// states (their own pictures among them), the starting one, its exposed
/// properties, sounds, script and health, as opened; and the layers states
/// own pictures of, as they were opened (their geometry is not edited
/// until issue #79 carries the pictures along).
#[derive(Clone, Debug, PartialEq)]
pub struct AssetParts {
    pub states: Vec<State>,
    pub start: usize,
    pub exposed: Vec<ExposedSource>,
    pub sounds: BTreeMap<String, Arc<Vec<u8>>>,
    pub script: Option<String>,
    pub health: Option<HealthSource>,
    /// The asset's size as opened.
    pub(crate) size: (u32, u32),
    /// The layers states own pictures of, by id: as opened.
    pub(crate) owned: BTreeMap<String, Owned>,
}

/// A layer some state paints itself, as opened: the first state owning it,
/// its box, and its picture's art size and whether it has cells and back
/// art (what every state's picture of it matches).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Owned {
    pub(crate) state: String,
    pub(crate) boxed: Option<((i32, i32), (u32, u32))>,
    pub(crate) shape: Option<((u32, u32), bool, bool)>,
}

/// A drawn layer's picture's shape: its art's size, whether it has cells
/// and back art.
fn shape(d: &Decoded) -> Option<((u32, u32), bool, bool)> {
    match d {
        Decoded::Drawn { art, cells, back } => {
            Some(((art.width(), art.height()), cells.is_some(), back.is_some()))
        }
        _ => None,
    }
}

impl AssetParts {
    /// Whether the asset has anything an edit can break: a state's own
    /// picture or value, an exposed property or health. An asset without
    /// (every built-in one today) is never refused.
    pub fn constrains(&self) -> bool {
        !self.exposed.is_empty()
            || self.health.is_some()
            || self.states.iter().any(|s| !s.own.is_empty() || !s.values.is_empty())
    }
}

/// A new asset (`Dialog::NewAsset`): its name, size and art scale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAsset {
    pub name: String,
    pub size: (u32, u32),
    pub art_scale: u32,
}

impl NewAsset {
    /// Why it cannot be made: a name of 1 to 64 characters (no control
    /// characters), each side 1 to `MAX_SIDE`, an art scale 1 to
    /// `MAX_ART_SCALE`.
    pub fn refusal(&self) -> Option<String> {
        let n = self.name.trim().chars().count();
        if n == 0 || n > MAX_ASSET_NAME || self.name.chars().any(char::is_control) {
            return Some(format!("an asset's name is 1 to {MAX_ASSET_NAME} characters"));
        }
        let side = |v: u32| (1..=MAX_SIDE).contains(&v);
        if !side(self.size.0) || !side(self.size.1) {
            return Some(format!("an asset's width and height are each 1 to {MAX_SIDE}"));
        }
        if !(1..=MAX_ART_SCALE).contains(&self.art_scale) {
            return Some(format!("an asset's art scale is 1 to {MAX_ART_SCALE}"));
        }
        None
    }

    /// The blank asset: one empty drawn Background layer, [`FIRST_LAYER`].
    pub fn asset(&self) -> Asset {
        let (w, h) = self.size;
        let s = self.art_scale;
        let id = olr_edit::element::new_element_id(&[], &[], &Default::default(), FIRST_LAYER, "layer");
        let element = Element {
            id,
            name: FIRST_LAYER.into(),
            visible: true,
            locked: false,
            folder: None,
            kind: ElementKind::drawn(ElementMaterial::Background),
            glow: None,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        let piece = Piece {
            element,
            decoded: Decoded::Drawn { art: RgbaImage::new(w * s, h * s), cells: None, back: None },
            original: None,
            glow_mask: None,
            surface_map: None,
            back_surface_map: None,
        };
        Asset::new(self.name.trim(), s, self.size, vec![piece], Vec::new(), Vec::new())
    }
}

impl Document {
    /// `asset` as the asset tab edits it (issue #77): its layers the items
    /// (the starting state's pictures), baked as a level of its size and
    /// art scale under theme `theme` (the open level's; else
    /// [`DEFAULT_THEME`], else the first installed), with no spawns and
    /// the asset's look (`preview_look` with its groups, and its
    /// materials). Its states, exposed properties, sounds, script and
    /// health are kept for [`Document::to_asset`].
    pub fn open_asset(asset: &Asset, theme: &str, catalog: &Catalog) -> Result<Document, DocError> {
        let found = |id: &str| catalog.themes.iter().find(|t| t.id == id);
        let theme = found(theme)
            .or_else(|| found(DEFAULT_THEME))
            .or(catalog.themes.first())
            .ok_or_else(|| DocError::NoTheme(theme.to_string()))?;
        let (w, h) = asset.size;
        let s = asset.art_scale;
        let side = |v: u32| (1..=MAX_SIDE).contains(&v);
        if !side(w) || !side(h) || !(1..=MAX_ART_SCALE).contains(&s) {
            return Err(DocError::Bad(format!(
                "asset {:?}: its size or art scale is out of range",
                asset.name
            )));
        }
        let mut look = preview_look(&asset.groups);
        look.materials = asset.materials.clone();
        let level = LevelSource {
            name: asset.name.clone(),
            notes: String::new(),
            size: (w, h),
            art_scale: s,
            theme: theme.id.clone(),
            objects: Vec::new(),
            material: vec![Material::Background; w as usize * h as usize],
            front: RgbaImage::new(w * s, h * s),
            back: RgbaImage::new(w * s, h * s),
            thumb: None,
            look: Some(look),
            glow: None,
            clear: None,
            glass: None,
            glow_beams: None,
            haze: None,
            distant: Vec::new(),
            surface: None,
            distant_surfaces: Vec::new(),
        };
        let mut originals = BTreeMap::new();
        let mut glow_masks = BTreeMap::new();
        let mut surface_maps = BTreeMap::new();
        for p in &asset.pieces {
            let id = &p.element.id;
            if let Some((source, bytes)) = &p.original {
                originals.insert(source.clone(), bytes.clone());
            }
            if let Some(m) = &p.glow_mask {
                glow_masks.insert(id.clone(), m.clone());
            }
            if let Some(m) = &p.surface_map {
                surface_maps.insert(surface_file(id), m.clone());
            }
            if let Some(m) = &p.back_surface_map {
                surface_maps.insert(surface_back_file(id), m.clone());
            }
        }
        let items: Vec<Item> = asset
            .pieces
            .iter()
            .map(|p| Item { element: p.element.clone(), decoded: p.decoded.clone() })
            .collect();
        let mut owned = BTreeMap::new();
        for st in &asset.states {
            for id in st.own.keys() {
                let item = items.iter().find(|i| i.element.id == *id);
                owned.entry(id.clone()).or_insert_with(|| Owned {
                    state: st.name.clone(),
                    boxed: item.and_then(|i| element_box(&i.element.kind, asset.size)),
                    shape: item.and_then(|i| shape(&i.decoded)),
                });
            }
        }
        let parts = AssetParts {
            states: asset.states.clone(),
            start: asset.start,
            exposed: asset.exposed.clone(),
            sounds: asset.sounds.clone(),
            script: asset.script.clone(),
            health: asset.health.clone(),
            size: asset.size,
            owned,
        };
        let manifest = PackManifest {
            format_version: PACK_FORMAT_VERSION,
            id: "asset".into(),
            name: asset.name.clone(),
            version: String::new(),
            kind: PackKind::Level,
            authors: Vec::new(),
            origin: None,
            license: None,
        };
        let mut doc = Document {
            stored: BTreeMap::new(),
            dir: PathBuf::new(),
            manifest,
            other: BTreeMap::new(),
            placed: None,
            verbatim: Default::default(),
            converted: false,
            level,
            items,
            folders: asset.folders.clone(),
            originals,
            cache: ImageCache::new(),
            front_tile: theme.front_tile.clone(),
            back_tile: theme.back_tile.clone(),
            themes: [(theme.id.clone(), (theme.front_tile.clone(), theme.back_tile.clone()))].into(),
            rebaked: false,
            glow_masks,
            surface_maps,
            kept: None,
            see_through: false,
            distant_items: Vec::new(),
            distant_masks: Vec::new(),
            distant_generation: 0,
            bake_generation: 0,
            holes: 0,
            asset: Some(Box::new(parts)),
        };
        doc.rebake(doc.all());
        Ok(doc)
    }

    /// Whether this is an asset tab's document (`open_asset`).
    pub fn is_asset(&self) -> bool {
        self.asset.is_some()
    }

    /// The asset's states, exposed properties, sounds, script and health
    /// as opened (an asset tab's document only).
    pub fn asset_parts(&self) -> Option<&AssetParts> {
        self.asset.as_deref()
    }

    /// The asset this document holds now (issue #77): its name (the bake's
    /// name), size and art scale, its items as its pieces (an image's
    /// original, a glow's mask and the painted surface maps with each),
    /// its folders, the glow groups and materials of its look that the
    /// layers name (`olr_edit::asset::carried_look`), and its states,
    /// exposed properties, sounds, script and health as opened. A level's
    /// document makes an asset of one state.
    pub fn to_asset(&self) -> Asset {
        let size = self.level.size;
        let mut pieces: Vec<Piece> = self
            .items
            .iter()
            .map(|i| {
                let e = &i.element;
                let original = match &e.kind {
                    ElementKind::Image { source, .. } => {
                        self.originals.get(source).map(|b| (source.clone(), b.clone()))
                    }
                    _ => None,
                };
                Piece {
                    element: e.clone(),
                    decoded: i.decoded.clone(),
                    original,
                    glow_mask: self
                        .glow_masks
                        .get(&e.id)
                        .filter(|_| e.glow.as_ref().is_some_and(|g| g.mask))
                        .cloned(),
                    surface_map: self
                        .surface_maps
                        .get(&surface_file(&e.id))
                        .filter(|_| e.surface_map)
                        .cloned(),
                    back_surface_map: self
                        .surface_maps
                        .get(&surface_back_file(&e.id))
                        .filter(|_| e.back_surface_map)
                        .cloned(),
                }
            })
            .collect();
        let (groups, materials) = carried_look(&mut pieces, self.level.look.as_ref(), size);
        let mut asset = Asset::new(&self.level.name, self.level.art_scale, size, pieces, groups, materials);
        asset.folders = self.folders();
        if let Some(p) = &self.asset {
            asset.states = p.states.clone();
            asset.start = p.start;
            asset.exposed = p.exposed.clone();
            asset.sounds = p.sounds.clone();
            asset.script = p.script.clone();
            asset.health = p.health.clone();
        }
        asset
    }

    /// `asset.ron`'s metadata as the document now is: what the state and
    /// exposure rules read (no pixels).
    fn asset_file(&self, parts: &AssetParts) -> AssetFile {
        let look = self.level.look.as_ref();
        AssetFile {
            art_scale: self.level.art_scale,
            folders: self.folders(),
            glow_groups: look.map_or_else(Vec::new, |l| l.glow_groups.clone()),
            materials: look.map_or_else(Vec::new, |l| l.materials.clone()),
            states: parts.states.iter().map(State::source).collect(),
            start: parts.states.get(parts.start).map(|s| s.name.clone()),
            exposed: parts.exposed.clone(),
            sounds: parts.sounds.keys().cloned().collect(),
            health: parts.health.clone(),
            ..AssetFile::new(
                &self.level.name,
                self.level.size,
                self.items.iter().map(|i| i.element.clone()).collect(),
            )
        }
    }

    /// Why the asset tab's document, as it now is, cannot stand (issue #77):
    /// a distant layer, a raster or a spawn (an asset carries none); a
    /// layer a state owns a picture of removed, moved, resized or re-made,
    /// or the asset resized while a state owns one (issue #79 lifts it); an
    /// exposed property or a state's value whose layer no longer offers it;
    /// else any other rule of #71's states and exposure
    /// (`olr_format::assets::check_states`). `None` for a level's document
    /// and an asset without states and exposure.
    pub(crate) fn asset_refusal(&self) -> Option<String> {
        let parts = self.asset.as_deref()?;
        if self.items.iter().any(|i| i.element.is_distant()) {
            return Some("an asset carries no distant layer".into());
        }
        if self.items.iter().any(|i| matches!(i.element.kind, ElementKind::Raster)) {
            return Some("an asset holds images and drawn layers, no raster".into());
        }
        if !self.level.objects.is_empty() {
            return Some("an asset has no spawns".into());
        }
        if !parts.constrains() {
            return None;
        }
        let own = |o: &Owned, layer: &str| {
            format!(
                "state {} has its own picture of {layer}: edit it in the states panel (issue #79)",
                o.state
            )
        };
        if let Some((id, o)) = parts.owned.iter().next()
            && self.level.size != parts.size
        {
            return Some(own(o, id));
        }
        for (id, o) in &parts.owned {
            let now = self.items.iter().find(|i| i.element.id == *id);
            let same = now.is_some_and(|i| {
                matches!(i.element.kind, ElementKind::Drawn { .. })
                    && element_box(&i.element.kind, self.level.size) == o.boxed
                    && shape(&i.decoded) == o.shape
            });
            if !same {
                return Some(own(o, id));
            }
        }
        let file = self.asset_file(parts);
        // Every exposed entry of the first layer failing, named.
        let failing: Vec<(&str, &str)> = file
            .exposed
            .iter()
            .filter(|x| property_type(&file, &x.property).is_err())
            .filter_map(|x| Some((x.property.layer()?, x.name.as_str())))
            .collect();
        if let Some(&(layer, _)) = failing.first() {
            let names: Vec<&str> = failing.iter().filter(|f| f.0 == layer).map(|f| f.1).collect();
            return Some(format!("{layer} is used by exposed {}: unexpose it first", names.join(", ")));
        }
        for s in &file.states {
            for v in &s.values {
                if let (Err(_), Some(layer)) = (property_type(&file, &v.property), v.property.layer()) {
                    return Some(format!(
                        "{layer} is set by state {} ({}): clear that value first",
                        s.name,
                        property_label(&v.property)
                    ));
                }
            }
        }
        check_states(&file).err().map(|e| format!("this would break the asset's states: {e}"))
    }
}
