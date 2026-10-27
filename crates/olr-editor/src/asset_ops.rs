//! The asset library's document operations (issue #9): an asset made of
//! the selected elements or cut out of a world rectangle of every visible
//! layer. The cut is `olr_edit::asset`'s, shared with `olr-cli asset`
//! (issue #82: a placing is a copy, `copies.rs`).

use std::sync::Arc;

use olr_edit::asset::{Asset, LevelPieces, PieceRef, cut_rect, from_elements};
use olr_edit::compose::Rect;
use olr_format::levels::{Element, ElementKind, surface_back_file, surface_file};

use crate::document::{DocError, Document};

/// An item's original: its source name and bytes.
type Original = Option<(String, Arc<Vec<u8>>)>;

pub use olr_edit::asset::ADDED_LIGHTING;

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

    /// The items' elements as cut: a hidden folder's members hidden (issue
    /// #60).
    fn seen(&self) -> Vec<Element> {
        (0..self.items.len())
            .map(|i| Element { visible: self.shown_at(i), ..self.items[i].element.clone() })
            .collect()
    }

    /// The document's elements as `olr_edit::asset` cuts them (`originals`
    /// from `originals_by_item`, `seen` from `seen`).
    fn pieces<'a>(&'a self, originals: &'a [Original], seen: &'a [Element]) -> LevelPieces<'a> {
        let pieces = self
            .items
            .iter()
            .zip(originals)
            .zip(seen)
            .map(|((i, o), element)| PieceRef {
                element,
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
        from_elements(&self.pieces(&originals, &self.seen()), ids, name)
            .map_err(|e| DocError::Bad(e.to_string()))
    }

    /// An asset cut out of world rectangle `rect` of every visible layer
    /// (`olr_edit::asset::cut_rect`), named `name`.
    pub fn asset_of_rect(&self, rect: Rect, name: &str) -> Result<Asset, DocError> {
        let originals = self.originals_by_item();
        cut_rect(&self.pieces(&originals, &self.seen()), rect, None, name)
            .map_err(|e| DocError::Bad(e.to_string()))
    }
}
