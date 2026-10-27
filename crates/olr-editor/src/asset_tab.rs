//! The asset tab's document (issue #77): a library asset opened as a
//! `Document` whose bake is a level of the asset's size and art scale under
//! the open level's theme (its tiles, for the tile paints and the craters),
//! with no spawns and the asset's look (`olr_edit::asset::preview_look`
//! with its glow groups and materials); `to_asset` makes the asset again.
//! The exposed properties, sounds, script and health are kept as opened
//! (issue #80 edits them), the states edited by `state_ops` (issue #79);
//! `refusal` says why an edit would leave them invalid, so that the
//! editor refuses it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::asset::{Asset, OwnPicture, Piece, State, carried_look, preview_look, property_label};
use olr_edit::compose::{Decoded, ImageCache};
use olr_format::assets::{
    AssetFile, ExposedSource, HealthSource, MAX_ASSET_NAME, StateSource, StateValue, check_states,
    property_type,
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
/// states (their own pictures among them), the starting one and the one
/// shown (issue #79), its exposed properties, sounds, script and health.
///
/// Issue #79: while a state other than the start is shown, the document's
/// items (and glow masks) hold that state's own pictures, and its entry
/// in `states` holds the starting state's pictures of those layers in
/// their place (`Document::swap_shown` exchanges them; `to_asset` puts
/// them back), so painting, the bake and the history work on the items
/// as ever.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetParts {
    pub states: Vec<StateEntry>,
    pub start: usize,
    /// The state the canvas and previews show (not a step; the start by
    /// default).
    pub shown: usize,
    pub exposed: Vec<ExposedSource>,
    pub sounds: BTreeMap<String, Arc<Vec<u8>>>,
    pub script: Option<String>,
    pub health: Option<HealthSource>,
    /// Each state's version (the states panel's pictures are made again
    /// when it moved), parallel to `states`, and the next one.
    pub(crate) versions: Vec<u64>,
    pub(crate) clock: u64,
}

/// One of the asset's states as the asset tab keeps it (issue #79):
/// `olr_edit::asset::State` with its own pictures shared (`Arc`: the
/// history holds them by address, counted once).
#[derive(Clone, Debug, PartialEq)]
pub struct StateEntry {
    pub name: String,
    pub own: BTreeMap<String, Arc<OwnPicture>>,
    pub values: Vec<StateValue>,
    pub loop_sound: Option<String>,
    pub enter_sound: Option<String>,
}

impl StateEntry {
    pub fn of(s: &State) -> StateEntry {
        StateEntry {
            name: s.name.clone(),
            own: s.own.iter().map(|(k, v)| (k.clone(), Arc::new(v.clone()))).collect(),
            values: s.values.clone(),
            loop_sound: s.loop_sound.clone(),
            enter_sound: s.enter_sound.clone(),
        }
    }

    pub fn to_state(&self) -> State {
        State {
            name: self.name.clone(),
            own: self.own.iter().map(|(k, v)| (k.clone(), (**v).clone())).collect(),
            values: self.values.clone(),
            loop_sound: self.loop_sound.clone(),
            enter_sound: self.enter_sound.clone(),
        }
    }

    /// As `asset.ron` holds it.
    pub fn source(&self) -> StateSource {
        StateSource {
            name: self.name.clone(),
            own: self.own.keys().cloned().collect(),
            values: self.values.clone(),
            loop_sound: self.loop_sound.clone(),
            enter_sound: self.enter_sound.clone(),
        }
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
        let states: Vec<StateEntry> = asset.states.iter().map(StateEntry::of).collect();
        let n = states.len() as u64;
        let parts = AssetParts {
            states,
            start: asset.start,
            shown: asset.start,
            exposed: asset.exposed.clone(),
            sounds: asset.sounds.clone(),
            script: asset.script.clone(),
            health: asset.health.clone(),
            versions: (0..n).collect(),
            clock: n,
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
            canvas_look: None,
            ground: false,
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
            asset.states = p.states.iter().map(StateEntry::to_state).collect();
            // The shown state's own pictures are the items' (issue #79):
            // the start's go back in their place.
            if p.shown != p.start
                && let Some(st) = p.states.get(p.shown)
            {
                for (id, base) in &st.own {
                    let Some(piece) = asset.pieces.iter_mut().find(|q| q.element.id == *id) else { continue };
                    let own = OwnPicture {
                        decoded: std::mem::replace(&mut piece.decoded, base.decoded.clone()),
                        glow_mask: match &base.glow_mask {
                            Some(m) => piece.glow_mask.replace(m.clone()),
                            None => None,
                        },
                    };
                    asset.states[p.shown].own.insert(id.clone(), own);
                }
            }
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
    pub(crate) fn asset_file(&self, parts: &AssetParts) -> AssetFile {
        let look = self.level.look.as_ref();
        AssetFile {
            art_scale: self.level.art_scale,
            folders: self.folders(),
            glow_groups: look.map_or_else(Vec::new, |l| l.glow_groups.clone()),
            materials: look.map_or_else(Vec::new, |l| l.materials.clone()),
            states: parts.states.iter().map(StateEntry::source).collect(),
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

    /// The first state's own picture that no longer fits its layer (issue
    /// #79: the edits carry them along, so this is a guard): a layer gone
    /// or not drawn, art not of its box at the art scale, cells without a
    /// mixed layer or back art without the layer's, a glow mask without
    /// the layer's or of another size.
    fn misfit_picture(&self, parts: &AssetParts) -> Option<String> {
        let s = self.level.art_scale;
        for st in &parts.states {
            for (id, pic) in &st.own {
                let why = |what: &str| Some(format!("state {}'s picture of {id} {what}", st.name));
                let Some(i) = self.index_of(id) else { return why("has no layer") };
                let e = &self.items[i].element;
                let ElementKind::Drawn { mixed, back, .. } = e.kind else {
                    return why("is not of a drawn layer");
                };
                let Some((_, b)) = element_box(&e.kind, self.level.size) else { return why("has no box") };
                let Decoded::Drawn { art, cells, back: back_art } = &pic.decoded else {
                    return why("is not drawn pixels");
                };
                let art_size = (b.0 * s, b.1 * s);
                if (art.width(), art.height()) != art_size
                    || back_art.as_ref().is_some_and(|a| (a.width(), a.height()) != art_size)
                {
                    return why("is not of its box");
                }
                if mixed != cells.is_some() || cells.as_ref().is_some_and(|c| c.len() != (b.0 * b.1) as usize)
                {
                    return why("does not match its cells");
                }
                if back != back_art.is_some() {
                    return why("does not match its back art");
                }
                if let Some(m) = &pic.glow_mask {
                    let masked = e.glow.as_ref().is_some_and(|g| g.mask) && self.glow_masks.contains_key(id);
                    if !masked || m.len() != (art_size.0 * art_size.1) as usize {
                        return why("has a glow mask its layer lacks");
                    }
                }
            }
        }
        None
    }

    /// Why the asset tab's document, as it now is, cannot stand (issue #77):
    /// a distant layer, a raster or a spawn (an asset carries none); a
    /// state's own picture that no longer fits its layer (issue #79); an
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
        if let Some(why) = self.misfit_picture(parts) {
            return Some(why);
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
