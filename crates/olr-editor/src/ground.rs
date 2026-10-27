//! The level's ground in the asset tab (issue #81; the asset-based levels
//! design's "The level tab", call 16): Edit in level opens a copy of the
//! level's document as the asset tab's (`Document::ground_of`: its layers,
//! folders, masks, maps and look, under the level's theme, without the
//! spawns), painted there with every layer tool, distant layers and
//! rasters included. Switching to the level tab, Play and the asset tab's
//! Save apply it to the level as one level step, "Edit Ground"
//! (`Edit::Ground`, `Document::ground_edit`).

use std::collections::BTreeMap;

use olr_edit::compose::ImageCache;
use olr_edit::pack::GROUND_NAME;
use olr_format::levels::GROUND_KEY;
use olr_format::look::LookSource;

use crate::document::Document;
use crate::history::{Edit, GroundSnap};

/// The level tab's name of the ground's copy (`Selection::Copy`).
pub const GROUND_COPY: &str = GROUND_KEY;

/// The asset tab's status after an undo or redo of "Edit Ground" took the
/// ground it showed back (issue #81).
pub const GROUND_WENT_BACK: &str = "The ground went back to before your last edits";

/// The asset tab's status after the level changed the ground (a resize, an
/// asset placed) while the asset tab held no edits of it.
pub const GROUND_FOLLOWED: &str = "The ground changed in the level: shown as it is now";

/// The step's name the level's history gives the ground's edits.
pub const EDIT_GROUND: &str = "Edit Ground";

/// How the ground opens in the asset tab (the level tab's refusals).
pub const EDIT_IN_LEVEL_HINT: &str = "select the ground, then Edit in level";

/// The level tab's refusal of a layer edit (issue #81).
pub const LAYERS_IN_ASSET_TAB: &str =
    "layers are painted in the asset tab: select the ground, then Edit in level";

/// The level tab's refusal of a change of the ground's copy itself.
pub const GROUND_LOCKED: &str = "the ground is locked: it is the level's own";

/// The asset tab's refusal of a resize or rename of the ground.
pub const GROUND_IS_THE_LEVELS: &str =
    "the ground is the level's size and name: change them in the level tab";

/// `level`'s look with `ground`'s glow groups and materials (the ground's
/// own, issue #81): the level keeps its ambient, occlusion, haze and grade.
fn merged_look(level: Option<&LookSource>, ground: Option<&LookSource>) -> Option<LookSource> {
    match (level, ground) {
        (Some(l), Some(g)) => Some(LookSource {
            glow_groups: g.glow_groups.clone(),
            materials: g.materials.clone(),
            ..l.clone()
        }),
        (Some(l), None) => Some(LookSource { glow_groups: Vec::new(), materials: Vec::new(), ..l.clone() }),
        (None, g) => g.cloned(),
    }
}

/// A look's glow groups and materials (none without a look).
fn ground_look(
    l: Option<&LookSource>,
) -> (&[olr_format::look::GlowGroupSource], &[olr_format::materials::MaterialSource]) {
    l.map_or((&[][..], &[][..]), |l| (&l.glow_groups[..], &l.materials[..]))
}

impl Document {
    /// Whether this is the asset tab's copy of a level's ground (issue #81).
    pub fn is_ground(&self) -> bool {
        self.ground
    }

    /// The ground's name: the ground asset's (`editor/asset.ron`), else
    /// `olr_edit::pack::GROUND_NAME` (a plain level's, until saved).
    pub fn ground_name(&self) -> String {
        self.placed
            .as_ref()
            .and_then(|p| p.assets.get(GROUND_KEY))
            .map_or_else(|| GROUND_NAME.to_string(), |a| a.file.name.clone())
    }

    /// The asset tab's copy of `level`'s ground (issue #81): its layers,
    /// folders, originals, glow masks, surface maps, the level's look,
    /// theme, haze and kept glow, baked as the level bakes it; no spawns
    /// (they are the level's). Never saved itself: its edits are applied
    /// to `level` (`ground_edit`).
    pub fn ground_of(level: &Document) -> Document {
        let mut l = level.level.clone();
        l.objects.clear();
        let mut doc = Document {
            stored: BTreeMap::new(),
            dir: level.dir.clone(),
            manifest: level.manifest.clone(),
            other: BTreeMap::new(),
            placed: None,
            verbatim: Default::default(),
            converted: false,
            level: l,
            items: level.items.clone(),
            folders: level.folders.clone(),
            originals: level.originals.clone(),
            cache: ImageCache::new(),
            front_tile: level.front_tile.clone(),
            back_tile: level.back_tile.clone(),
            themes: level.themes.clone(),
            rebaked: false,
            glow_masks: level.glow_masks.clone(),
            surface_maps: level.surface_maps.clone(),
            kept: level.kept.clone(),
            see_through: false,
            distant_items: Vec::new(),
            distant_masks: Vec::new(),
            distant_generation: 0,
            bake_generation: 0,
            holes: 0,
            asset: None,
            canvas_look: None,
            ground: true,
            copy_cache: Default::default(),
            placed_look: None,
        };
        doc.rebake(doc.all());
        doc
    }

    /// The ground as this document holds it (`Edit::Ground`'s half).
    pub fn ground_snap(&self) -> GroundSnap {
        GroundSnap {
            items: self.items.clone(),
            folders: self.folders.clone(),
            originals: self.originals.clone(),
            masks: self.glow_masks.clone(),
            surfaces: self.surface_maps.clone(),
            look: self.level.look.clone(),
        }
    }

    /// "Edit Ground" (issue #81): this level's ground replaced by the asset
    /// tab's copy `ground` (its layers, folders, originals, masks and maps,
    /// and its glow groups and materials in the level's look); `None` when
    /// it changes nothing.
    pub(crate) fn ground_edit(&self, ground: &Document) -> Option<Edit> {
        let before = self.ground_snap();
        let after = GroundSnap {
            look: merged_look(self.level.look.as_ref(), ground.level.look.as_ref()),
            ..ground.ground_snap()
        };
        (before != after).then(|| Edit::Ground { before: Box::new(before), after: Box::new(after) })
    }

    /// Whether `ground` (the asset tab's copy) holds this level's ground
    /// as it now is: the layers, folders, glow masks, surface maps, glow
    /// groups and materials, at the level's size, art scale and theme.
    pub(crate) fn same_ground(&self, ground: &Document) -> bool {
        self.level.size == ground.level.size
            && self.level.art_scale == ground.level.art_scale
            && self.level.theme == ground.level.theme
            && self.items == ground.items
            && self.folders == ground.folders
            && self.glow_masks == ground.glow_masks
            && self.surface_maps == ground.surface_maps
            && ground_look(self.level.look.as_ref()) == ground_look(ground.level.look.as_ref())
    }

    /// The level's own settings, as `level` now has them, in the ground's
    /// copy (`same_ground` holds): its look (the same groups and
    /// materials), haze, name and notes; no step and no bake.
    pub(crate) fn follow_level(&mut self, level: &Document) {
        self.level.look = level.level.look.clone();
        self.level.haze = level.level.haze.clone();
        self.level.name = level.level.name.clone();
        self.level.notes = level.level.notes.clone();
    }
}
