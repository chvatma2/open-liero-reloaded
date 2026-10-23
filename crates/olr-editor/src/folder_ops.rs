//! Layer folders in the document (issue #60; the format and every tree
//! computation are issue #59's, `olr_edit::folders`): group, ungroup,
//! moves in and out, a folder's fields, removal with its contents,
//! duplicate, and the clipboard's folders. Each operation checks first and
//! is one step. A structural change is computed on the composition by
//! `olr_edit::folders` and carried out as edits (`restructure`): the
//! folder list whole (`Edit::Folders`), then the moves, then each element
//! whose `folder` changed (`Edit::Element`). Locks refuse content changes,
//! not order (plan 14d R4): grouping, ungrouping and moving ignore them.

use std::collections::BTreeSet;
use std::sync::Arc;

use olr_edit::compose::Rect;
use olr_edit::element::{copy_name, new_element_id};
use olr_edit::folders::{self, FolderError, folder_chain};
use olr_format::levels::{
    Composition, Element, ElementKind, Folder, check_elements, surface_back_file, surface_file,
};

use crate::document::{DocError, Document, Item};
use crate::history::{Edit, Snapshot};
use crate::ops::{Change, NOTHING};

/// A settable field of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderField {
    Name(String),
    Visible(bool),
    Locked(bool),
    /// Folded shut in the Elements list (the editor's only).
    Collapsed(bool),
}

/// A refusal of `olr_edit::folders` as the document's.
fn refused(e: FolderError) -> DocError {
    match e {
        FolderError::NoItem(id) => DocError::NoElement(id),
        other => DocError::Bad(other.to_string()),
    }
}

impl Document {
    /// Whether `id` is one of the level's folders in use.
    pub fn is_folder(&self, id: &str) -> bool {
        self.folders().iter().any(|f| f.id == id)
    }

    /// The elements folder `id` holds, at any depth, bottom to top (none for
    /// an unknown id).
    pub fn folder_members(&self, id: &str) -> Vec<String> {
        let c = self.composition();
        folders::members(&c, id).into_iter().map(|i| c.elements[i].id.clone()).collect()
    }

    /// The folder an element moved from index `from` to `to` (the flat
    /// list's drag) lands in: the innermost folder holding both its new
    /// neighbours, or, when it lies at a folder's edge, the folder of its
    /// own it is already in there (so a reorder at the edge of its folder
    /// keeps it in). Every folder stays one block.
    pub(crate) fn landing_folder(&self, from: usize, to: usize) -> Option<String> {
        let own = self.items[from].element.folder.clone();
        if from == to {
            return own;
        }
        let view = self.folder_view();
        let rest: Vec<&Element> =
            self.items.iter().enumerate().filter(|(i, _)| *i != from).map(|(_, i)| &i.element).collect();
        let chain = |e: Option<&&Element>| -> Vec<String> {
            e.map_or_else(Vec::new, |e| {
                folder_chain(&view, e.folder.as_deref()).into_iter().map(|f| f.id.clone()).collect()
            })
        };
        let below = chain(to.checked_sub(1).and_then(|k| rest.get(k)));
        let above = chain(rest.get(to));
        // The innermost folder holding both, and the folders between it and
        // each neighbour: the places that keep every folder one block.
        let common = below.iter().find(|f| above.contains(f)).cloned();
        let upto = |c: &[String]| -> Vec<String> {
            c.iter().take_while(|f| Some(*f) != common.as_ref()).cloned().collect()
        };
        let places: Vec<String> = upto(&below).into_iter().chain(upto(&above)).collect();
        folder_chain(&view, own.as_deref())
            .into_iter()
            .map(|f| f.id.clone())
            .take_while(|f| Some(f) != common.as_ref())
            .find(|f| places.contains(f))
            .or(common)
    }

    /// Makes the document `after` (the same elements, reordered and in
    /// other folders, and other folders): the folder list, then the moves,
    /// then each element's change, applied. `after` is checked first.
    pub(crate) fn restructure(&mut self, after: Composition) -> Result<(Vec<Edit>, Rect), DocError> {
        let same = after.elements.len() == self.items.len()
            && after.elements.iter().all(|e| self.index_of(&e.id).is_some());
        if !same {
            return Err(DocError::Bad("a folder change must keep the elements".into()));
        }
        check_elements(&after, self.level.size, self.level.art_scale)
            .map_err(|e| DocError::Bad(e.to_string()))?;
        let mut edits = Vec::new();
        let mut dirty = None;
        let mut run = |doc: &mut Document, edit: Edit| {
            let r = doc.apply(&edit, true);
            if r.w > 0 && r.h > 0 {
                dirty = Some(Rect::union(dirty, r));
            }
            edits.push(edit);
        };
        if self.folders != after.folders {
            run(self, Edit::Folders { before: self.folders.clone(), after: after.folders.clone() });
        }
        for (k, e) in after.elements.iter().enumerate() {
            let Some(j) = self.index_of(&e.id) else { continue };
            if j != k {
                let folder = self.items[j].element.folder.clone();
                run(self, Edit::Move { from: j, to: k, before: folder.clone(), after: folder });
            }
        }
        for (k, e) in after.elements.into_iter().enumerate() {
            if self.items[k].element != e {
                run(self, Edit::Element { index: k, before: self.items[k].element.clone(), after: e });
            }
        }
        Ok((edits, dirty.unwrap_or(NOTHING)))
    }

    /// `op` of `olr_edit::folders` on the composition, carried out as one
    /// step (`restructure`); its result besides.
    fn tree_op<T>(
        &mut self,
        op: impl FnOnce(&mut Composition) -> Result<T, FolderError>,
    ) -> Result<(Vec<Edit>, Rect, T), DocError> {
        let mut c = self.composition();
        let out = op(&mut c).map_err(refused)?;
        let (edits, dirty) = self.restructure(c)?;
        Ok((edits, dirty, out))
    }

    /// Groups elements and folders `ids` into a new folder named `name`
    /// ("Folder n" when empty; `olr_edit::folders::group`): one step's
    /// edits (applied), the rectangle rebaked and the folder's id.
    pub fn group(&mut self, ids: &[String], name: &str) -> Result<(Vec<Edit>, Rect, String), DocError> {
        self.tree_op(|c| folders::group(c, ids, name))
    }

    /// Ungroups folder `id` (`olr_edit::folders::ungroup`): its members take
    /// its place. One step.
    pub fn ungroup(&mut self, id: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        self.tree_op(|c| folders::ungroup(c, id)).map(|(e, r, ())| (e, r))
    }

    /// Moves element or folder `id` into folder `into` (none: the top
    /// level) at `position` among its children (`olr_edit::folders::
    /// move_to`: without one, just above the folder it leaves, or on top of
    /// `into`). One step.
    pub fn move_into(
        &mut self,
        id: &str,
        into: Option<&str>,
        position: Option<usize>,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        self.tree_op(|c| folders::move_to(c, id, into, position)).map(|(e, r, ())| (e, r))
    }

    /// Sets a field of folder `id`: one step. A locked folder, or one in a
    /// locked folder, takes only `Locked` and `Collapsed` (as an element
    /// takes only `Locked`).
    pub fn set_folder(&mut self, id: &str, field: FolderField) -> Result<Change, DocError> {
        if !self.is_folder(id) {
            return Err(DocError::NoElement(id.into()));
        }
        let content = matches!(field, FolderField::Name(_) | FolderField::Visible(_));
        if content && folders::folder_locked(&self.folder_view(), id) {
            return Err(DocError::Locked(id.into()));
        }
        let mut after = self.folders.clone();
        if let Some(f) = after.iter_mut().find(|f| f.id == id) {
            match field {
                FolderField::Name(n) => f.name = n,
                FolderField::Visible(v) => f.visible = v,
                FolderField::Locked(v) => f.locked = v,
                FolderField::Collapsed(v) => f.collapsed = v,
            }
        }
        let elements = self.items.iter().map(|i| i.element.clone()).collect();
        check_elements(&crate::document::tidied(elements, &after), self.level.size, self.level.art_scale)
            .map_err(|e| DocError::Bad(e.to_string()))?;
        Ok(self.change(Edit::Folders { before: self.folders.clone(), after }))
    }

    /// Removes folder `id` with everything it holds: one step's edits
    /// (applied) and the rectangle rebaked; refused while any member is
    /// locked, naming it.
    pub fn remove_folder(&mut self, id: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        if !self.is_folder(id) {
            return Err(DocError::NoElement(id.into()));
        }
        let members = self.folder_members(id);
        let (mut edits, dirty) = self.remove_many(&members)?;
        // The folders left empty go in the same step.
        let after = self.composition().folders;
        if after != self.folders {
            edits.push(self.change(Edit::Folders { before: self.folders.clone(), after }).edit);
        }
        Ok((edits, dirty))
    }

    /// A copy of element `id` directly above it, in its folder (as
    /// `olr_edit::element::duplicate`): a new id and `copy_name`d name, its
    /// data, original, glow mask and painted surface maps. One step's edits
    /// (applied) and the copy's id.
    pub fn duplicate(&mut self, id: &str) -> Result<(Vec<Edit>, String), DocError> {
        let i = self.find(id)?;
        let from = self.items[i].element.clone();
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        let names = elements.iter().map(|o| if o.name.is_empty() { o.id.as_str() } else { o.name.as_str() });
        let name = copy_name(if from.name.is_empty() { &from.id } else { &from.name }, names);
        let new = new_element_id(&elements, &self.folders, &self.stray_files(), &name, "copy");
        let element = Element { id: new.clone(), name, ..from.clone() };
        self.check_with(|es| es.insert(i + 1, element.clone()))?;
        let original = match &from.kind {
            ElementKind::Image { source, .. } => {
                self.originals.get(source).map(|b| (source.clone(), b.clone()))
            }
            _ => None,
        };
        let snapshot = Snapshot {
            item: Item { element, decoded: self.items[i].decoded.clone() },
            original,
            glow_mask: self.glow_masks.get(id).cloned(),
        };
        let mut edits = vec![Edit::Insert { index: i + 1, item: Box::new(snapshot) }];
        for (old, file) in
            [(surface_file(id), surface_file(&new)), (surface_back_file(id), surface_back_file(&new))]
        {
            if let Some(m) = self.surface_maps.get(&old) {
                edits.push(Edit::SurfaceMap {
                    id: new.clone(),
                    file,
                    before: None,
                    after: Some(Arc::clone(m)),
                });
            }
        }
        let (edits, _) = self.apply_all(edits);
        Ok((edits, new))
    }

    /// Copies of `ids` (elements, and folders: their elements) in stack
    /// order, and the folders they fill (plan 14e R6, issue #60): a folder
    /// all of whose elements are copied is copied with its subtree. Each
    /// copy names its innermost copied folder (none when no folder holding
    /// it is copied), each folder its innermost copied parent: the copies
    /// never name a folder of the level's but the copied ones, which a
    /// paste makes anew.
    pub fn copy_tree(&self, ids: &[String]) -> (Vec<Snapshot>, Vec<Folder>) {
        let c = self.composition();
        let mut chosen: BTreeSet<usize> = BTreeSet::new();
        for id in ids {
            match self.index_of(id) {
                Some(i) => {
                    chosen.insert(i);
                }
                None => chosen.extend(folders::members(&c, id)),
            }
        }
        let filled: BTreeSet<&str> = c
            .folders
            .iter()
            .filter(|f| {
                let m = folders::members(&c, &f.id);
                !m.is_empty() && m.iter().all(|k| chosen.contains(k))
            })
            .map(|f| f.id.as_str())
            .collect();
        let innermost = |start: Option<&str>| {
            folder_chain(&c, start).into_iter().find(|f| filled.contains(f.id.as_str())).map(|f| f.id.clone())
        };
        let copied: Vec<Folder> = c
            .folders
            .iter()
            .filter(|f| filled.contains(f.id.as_str()))
            .map(|f| Folder { parent: innermost(f.parent.as_deref()), ..f.clone() })
            .collect();
        let element_ids: Vec<String> = chosen.iter().map(|&i| c.elements[i].id.clone()).collect();
        let mut items = self.copy_items(&element_ids);
        for s in &mut items {
            s.item.element.folder = innermost(s.item.element.folder.as_deref());
        }
        (items, copied)
    }

    /// Pastes `items` (copied at art scale `from`) on top, each moved by
    /// `offset` (`paste`), and `folders` (`copy_tree`'s) made anew: new ids,
    /// names made unique among the level's folders, each copy in its
    /// folder's new one, or in none. The pasted block lands on top, outside
    /// any folder of the level's (a distant one on top of the distant
    /// layers). One step's edits (applied) and the new elements' ids.
    pub fn paste_tree(
        &mut self,
        items: &[Snapshot],
        folders: &[Folder],
        from: u32,
        offset: (i32, i32),
    ) -> Result<(Vec<Edit>, Vec<String>), DocError> {
        let (mut edits, ids) = self.paste(items, from, offset)?;
        if folders.is_empty() {
            return Ok((edits, ids));
        }
        let mut c = self.composition();
        let mut made: Vec<(String, String)> = Vec::new();
        for f in folders {
            let name = if c.folders.iter().any(|o| o.name == f.name) {
                copy_name(&f.name, c.folders.iter().map(|o| o.name.as_str()))
            } else {
                f.name.clone()
            };
            let id = folders::new_folder_id(&c, &name);
            let parent =
                f.parent.as_ref().and_then(|p| made.iter().find(|(old, _)| old == p)).map(|m| m.1.clone());
            c.folders.push(Folder { id: id.clone(), name, parent, ..f.clone() });
            made.push((f.id.clone(), id));
        }
        for (snap, id) in items.iter().zip(&ids) {
            let folder =
                snap.item.element.folder.as_ref().and_then(|f| made.iter().find(|(old, _)| old == f));
            if let (Some((_, new)), Some(e)) = (folder, c.elements.iter_mut().find(|e| e.id == *id)) {
                e.folder = Some(new.clone());
            }
        }
        folders::tidy(&mut c);
        match self.restructure(c) {
            Ok((more, _)) => edits.extend(more),
            Err(e) => {
                self.roll_back(&edits);
                return Err(e);
            }
        }
        Ok((edits, ids))
    }
}
