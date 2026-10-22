//! The asset library's document operations (issue #9): an asset made of
//! the selected elements or cut out of a world rectangle of every visible
//! layer, and an asset placed as one step. The cut and the placing are
//! `olr_edit::asset`'s, shared with `olr-cli asset`.

use std::sync::Arc;

use olr_edit::asset::{
    Asset, LevelPieces, PieceRef, cut_rect, from_elements, land_surfaces, piece_name, with_groups,
    with_materials,
};
use olr_edit::compose::Rect;
use olr_format::levels::{ElementKind, surface_back_file, surface_file};

use crate::document::{DocError, Document};

/// An item's original: its source name and bytes.
type Original = Option<(String, Arc<Vec<u8>>)>;
use crate::history::Edit;

pub use olr_edit::asset::ADDED_LIGHTING;

/// A placing done: its edits (applied: one step), the new elements' ids
/// and what the status bar says.
#[derive(Debug)]
pub struct Placed {
    pub edits: Vec<Edit>,
    pub ids: Vec<String>,
    pub note: Option<String>,
}

impl Document {
    /// The items' originals, by item (an image's source and bytes).
    fn originals_by_item(&self) -> Vec<Original> {
        self.items
            .iter()
            .map(|i| match &i.element.kind {
                ElementKind::Image { source, .. } => {
                    self.originals.get(source).map(|b| (source.clone(), b.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// The document's elements as `olr_edit::asset` cuts them (`originals`
    /// from `originals_by_item`).
    fn pieces<'a>(&'a self, originals: &'a [Original]) -> LevelPieces<'a> {
        let pieces = self
            .items
            .iter()
            .zip(originals)
            .map(|(i, o)| PieceRef {
                element: &i.element,
                decoded: &i.decoded,
                original: o.as_ref(),
                glow_mask: self
                    .glow_masks
                    .get(&i.element.id)
                    .filter(|_| i.element.glow.as_ref().is_some_and(|g| g.mask)),
                // Its painted surface maps go into an asset (issue #38).
                surface_map: self
                    .surface_maps
                    .get(&surface_file(&i.element.id))
                    .filter(|_| i.element.surface_map),
                back_surface_map: self
                    .surface_maps
                    .get(&surface_back_file(&i.element.id))
                    .filter(|_| i.element.back_surface_map),
            })
            .collect();
        LevelPieces {
            pieces,
            size: self.level.size,
            art_scale: self.level.art_scale,
            look: self.level.look.as_ref(),
        }
    }

    /// An asset of elements `ids` (`olr_edit::asset::from_elements`), named
    /// `name`.
    pub fn asset_of_elements(&self, ids: &[String], name: &str) -> Result<Asset, DocError> {
        let originals = self.originals_by_item();
        from_elements(&self.pieces(&originals), ids, name).map_err(|e| DocError::Bad(e.to_string()))
    }

    /// An asset cut out of world rectangle `rect` of every visible layer
    /// (`olr_edit::asset::cut_rect`), named `name`.
    pub fn asset_of_rect(&self, rect: Rect, name: &str) -> Result<Asset, DocError> {
        let originals = self.originals_by_item();
        cut_rect(&self.pieces(&originals), rect, None, name).map_err(|e| DocError::Bad(e.to_string()))
    }

    /// Places `asset` with its top-left at world pixel `at`, one step (issue
    /// #9): its glow groups first (`olr_edit::asset::with_groups`: a group
    /// the level has is joined, its settings kept; a missing one added with
    /// the asset's; a level without a look gets the default one, and the
    /// status bar says so) and its surface materials (issue #38,
    /// `with_materials`: the same), then each piece on top as a paste lands
    /// it (new ids and names, resampled to the level's art scale, masks
    /// kept), with its surfaces and painted maps (`land_surfaces`). A
    /// refusal undoes what it did.
    pub fn place_asset(&mut self, asset: &Asset, at: (i32, i32)) -> Result<Placed, DocError> {
        let change = with_groups(self.level.look.as_ref(), &asset.groups);
        let look = if change.changes() { Some(&change.look) } else { self.level.look.as_ref() };
        let materials = with_materials(look, &asset.materials);
        let mut edits = Vec::new();
        let look = if materials.changes() {
            Some(&materials.look)
        } else {
            change.changes().then_some(&change.look)
        };
        if let Some(look) = look {
            for edit in self.look_change(Some(look.clone()))? {
                self.apply(&edit, true);
                edits.push(edit);
            }
        }
        let mut ids = Vec::new();
        for piece in &asset.pieces {
            let name = piece_name(asset, piece);
            let done = self.landed(piece.as_ref(), asset.art_scale, at, Some(&name)).and_then(|p| {
                let id = p.element.id.clone();
                Ok((self.insert_landed(p)?, id))
            });
            match done {
                Ok((edit, id)) => {
                    edits.push(edit);
                    edits.extend(self.land_surfaces(piece, &id, &materials, &asset.materials));
                    ids.push(id);
                }
                Err(e) => {
                    self.roll_back(&edits);
                    return Err(e);
                }
            }
        }
        let note = change.added_look.then(|| ADDED_LIGHTING.to_string());
        Ok(Placed { edits, ids, note })
    }

    /// Gives landed element `id` the surface materials of asset piece
    /// `piece` (`olr_edit::asset::land_surfaces`): its painted maps, then
    /// its surfaces; their edits (applied).
    fn land_surfaces(
        &mut self,
        piece: &olr_edit::asset::Piece,
        id: &str,
        change: &olr_edit::asset::MaterialChange,
        materials: &[olr_format::materials::MaterialSource],
    ) -> Vec<Edit> {
        let Some(index) = self.index_of(id) else { return Vec::new() };
        let before = self.items[index].element.clone();
        let mut after = before.clone();
        let (front, back) = land_surfaces(piece.as_ref(), &mut after, change, materials, self.level.size);
        let mut edits = Vec::new();
        for (file, m) in [(surface_file(id), front), (surface_back_file(id), back)] {
            if let Some(m) = m {
                let edit = Edit::SurfaceMap { id: id.into(), file, before: None, after: Some(m) };
                self.apply(&edit, true);
                edits.push(edit);
            }
        }
        if after != before {
            edits.push(self.change(Edit::Element { index, before, after }).edit);
        }
        edits
    }
}
