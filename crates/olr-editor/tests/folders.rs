//! Layer folders in the editor's document (issue #60): opened and saved as
//! they are, every folder operation one step that undoes and redoes to the
//! same files, effective visibility and locks, the clipboard and assets.

use std::path::Path;

use olr_edit::element::Property;
use olr_editor::folder_ops::FolderField;
use olr_editor::history::{Edit, History};
use olr_editor::tools::{PointerKind, Tool};
use olr_editor::{DocError, Document, EditorAction as A, Selection};
use olr_format::levels::{ElementMaterial, Folder};

use crate::common::{catalog, content, copy_pack, editor_at, files, fixture, open_fixture, run, tree};

/// One step of `edits` recorded, undone and redone: each comes back to the
/// same files. Returns the step's edits.
fn round_trip(
    doc: &mut Document,
    op: impl FnOnce(&mut Document) -> Result<Vec<Edit>, DocError>,
) -> Vec<Edit> {
    let before = files(doc);
    let edits = op(doc).unwrap();
    assert!(!edits.iter().all(Edit::is_noop), "the operation changes something");
    let after = files(doc);
    assert!(before != after, "the saved files change");
    let mut h = History::new();
    h.push(edits.clone());
    for e in h.undo().unwrap().to_vec().iter().rev() {
        doc.apply(e, false);
    }
    assert!(files(doc) == before, "undo gives back the files");
    for e in h.redo().unwrap().to_vec().iter() {
        doc.apply(e, true);
    }
    assert!(files(doc) == after, "redo gives back the files");
    edits
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn folder_of(doc: &Document, id: &str) -> Option<String> {
    doc.items()[doc.index_of(id).unwrap()].element.folder.clone()
}

fn folder(doc: &Document, id: &str) -> Folder {
    doc.folders().into_iter().find(|f| f.id == id).unwrap()
}

/// The fixture with "Outer" holding tiny and "Inner" (gradient, badge).
fn nested(root: &Path) -> (tempfile::TempDir, Document, String, String) {
    let (tmp, mut doc) = open_fixture(root);
    let (_, _, outer) = doc.group(&ids(&["tiny", "gradient", "badge"]), "Outer").unwrap();
    let (_, _, inner) = doc.group(&ids(&["gradient", "badge"]), "Inner").unwrap();
    (tmp, doc, outer, inner)
}

#[test]
fn nested_folders_open_and_save_byte_for_byte() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    let outer = olr_edit::element::group(&mut pack, &ids(&["tiny", "gradient", "badge"]), "Outer").unwrap();
    let inner = olr_edit::element::group(&mut pack, &ids(&["gradient", "badge"]), "Inner").unwrap();
    olr_edit::element::set(&mut pack, &inner, Property::Locked(true)).unwrap();
    let c = &mut pack.composition.as_mut().unwrap().composition;
    c.folders.iter_mut().find(|f| f.id == outer).unwrap().collapsed = true;
    olr_edit::pack::save(&dir, &pack).unwrap();
    let on_disk = tree(&dir);
    let doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog(&root)).unwrap();
    assert_eq!(doc.folders().len(), 2);
    assert!(files(&doc) == on_disk, "the folders come back byte for byte");
}

#[test]
fn group_and_ungroup_undo_and_redo() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    // Members not one block gather at the topmost one's place.
    round_trip(&mut doc, |d| d.group(&ids(&["tiny", "badge"]), "Pair").map(|r| r.0));
    let f = doc.folders()[0].id.clone();
    assert_eq!(folder(&doc, &f).name, "Pair");
    assert_eq!(folder_of(&doc, "tiny").as_deref(), Some(f.as_str()));
    let order: Vec<&str> = doc.items().iter().map(|i| i.element.id.as_str()).collect();
    assert_eq!(order, ["base", "gradient", "tiny", "badge", "tunnels", "wall"]);
    round_trip(&mut doc, |d| d.ungroup(&f).map(|r| r.0));
    assert!(doc.folders().is_empty() && folder_of(&doc, "tiny").is_none());
}

#[test]
fn moves_in_and_out_of_folders_undo_and_redo() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc, outer, inner) = nested(&root);
    // In: tunnels on top of Inner; out: back to the top level.
    round_trip(&mut doc, |d| d.move_into("tunnels", Some(&inner), None).map(|r| r.0));
    assert_eq!(folder_of(&doc, "tunnels").as_deref(), Some(inner.as_str()));
    round_trip(&mut doc, |d| d.move_into("tunnels", None, None).map(|r| r.0));
    assert_eq!(folder_of(&doc, "tunnels"), None);
    assert_eq!(doc.index_of("tunnels"), Some(4), "just above the folder it left");
    // A folder moves with its run.
    round_trip(&mut doc, |d| d.move_into(&inner, None, None).map(|r| r.0));
    assert_eq!(folder(&doc, &inner).parent, None);
    assert!(folder_of(&doc, "gradient").as_deref() == Some(inner.as_str()));
    assert_eq!(doc.folders().len(), 2, "{outer} keeps tiny");
}

/// The flat list's drag keeps the rules: an element dropped among a
/// folder's members joins it, one dropped outside leaves it.
#[test]
fn the_flat_move_joins_and_leaves_folders() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc, outer, inner) = nested(&root);
    // base, tiny, gradient, badge, tunnels, wall: wall between gradient
    // and badge joins Inner.
    round_trip(&mut doc, |d| d.move_to("wall", 3).map(|c| vec![c.edit]));
    assert_eq!(folder_of(&doc, "wall").as_deref(), Some(inner.as_str()));
    // Between base and tiny, at Outer's edge: Outer held it, so it stays
    // in Outer; below base it leaves every folder.
    round_trip(&mut doc, |d| d.move_to("wall", 1).map(|c| vec![c.edit]));
    assert_eq!(folder_of(&doc, "wall").as_deref(), Some(outer.as_str()));
    round_trip(&mut doc, |d| d.move_to("wall", 0).map(|c| vec![c.edit]));
    assert_eq!(folder_of(&doc, "wall"), None);
    round_trip(&mut doc, |d| d.move_to("wall", 5).map(|c| vec![c.edit]));
    // Badge to the top of its folder stays in it.
    round_trip(&mut doc, |d| d.move_to("gradient", 3).map(|c| vec![c.edit]));
    assert_eq!(folder_of(&doc, "gradient").as_deref(), Some(inner.as_str()));
    // Tiny, Outer's only element of its own, moved to the top leaves it;
    // the folders stay one block each.
    round_trip(&mut doc, |d| d.move_to("tiny", 5).map(|c| vec![c.edit]));
    assert_eq!(folder_of(&doc, "tiny"), None);
    olr_format::levels::check_elements(&doc.composition(), doc.level().size, doc.level().art_scale).unwrap();
    assert!(doc.folders().iter().any(|f| f.id == outer), "Outer holds Inner");
}

#[test]
fn folder_fields_undo_and_redo() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc, outer, inner) = nested(&root);
    round_trip(&mut doc, |d| d.set_folder(&outer, FolderField::Name("Desk".into())).map(|c| vec![c.edit]));
    round_trip(&mut doc, |d| d.set_folder(&inner, FolderField::Visible(false)).map(|c| vec![c.edit]));
    round_trip(&mut doc, |d| d.set_folder(&inner, FolderField::Collapsed(true)).map(|c| vec![c.edit]));
    round_trip(&mut doc, |d| d.set_folder(&outer, FolderField::Locked(true)).map(|c| vec![c.edit]));
    let f = folder(&doc, &inner);
    assert!(!f.visible && f.collapsed && folder(&doc, &outer).locked);
    // A folder in a locked one takes no name, only its lock and folding.
    assert!(matches!(doc.set_folder(&inner, FolderField::Name("x".into())), Err(DocError::Locked(_))));
    assert!(doc.set_folder(&inner, FolderField::Collapsed(false)).is_ok());
}

#[test]
fn removing_a_folder_takes_its_contents() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc, outer, inner) = nested(&root);
    doc.set("badge", Property::Locked(true)).unwrap();
    assert!(matches!(doc.remove_folder(&outer), Err(DocError::Locked(id)) if id == "badge"));
    doc.set("badge", Property::Locked(false)).unwrap();
    round_trip(&mut doc, |d| d.remove_folder(&inner).map(|r| r.0));
    assert!(doc.index_of("gradient").is_none() && doc.index_of("badge").is_none());
    assert_eq!(doc.folders().len(), 1);
    round_trip(&mut doc, |d| d.remove_folder(&outer).map(|r| r.0));
    assert!(doc.folders().is_empty() && doc.index_of("tiny").is_none());
}

#[test]
fn split_merge_duplicate_and_distant_in_folders() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let (_, _, f) = doc.group(&ids(&["tunnels", "wall"]), "Layers").unwrap();
    // Split: the parts take its folder.
    round_trip(&mut doc, |d| d.split("tunnels"));
    let parts: Vec<String> =
        doc.items()[4..doc.items().len() - 1].iter().map(|i| i.element.id.clone()).collect();
    assert!(!parts.is_empty() && parts.iter().all(|p| folder_of(&doc, p).as_deref() == Some(f.as_str())));
    // Merge down: the lower layer's folder.
    let below = doc.items()[doc.index_of("wall").unwrap() - 1].element.id.clone();
    round_trip(&mut doc, |d| d.merge_down("wall"));
    assert_eq!(folder_of(&doc, &below).as_deref(), Some(f.as_str()));
    // Duplicate: directly above, in the same folder.
    let (_, _, g) = doc.group(&ids(&["badge"]), "Badge").unwrap();
    let mut copy = String::new();
    round_trip(&mut doc, |d| {
        let (edits, id) = d.duplicate("badge")?;
        copy = id;
        Ok(edits)
    });
    assert_eq!(doc.index_of(&copy), doc.index_of("badge").map(|i| i + 1));
    assert_eq!(folder_of(&doc, &copy).as_deref(), Some(g.as_str()));
    // Distant: it leaves its folders.
    doc.set("badge", Property::Material(ElementMaterial::Background)).unwrap();
    round_trip(&mut doc, |d| d.set_distant("badge", Some(40)).map(|r| r.0));
    assert_eq!((folder_of(&doc, "badge"), doc.index_of("badge")), (None, Some(0)));
    olr_format::levels::check_elements(&doc.composition(), doc.level().size, doc.level().art_scale).unwrap();
}

/// A hidden folder hides its members from the bake (as their own flag
/// does) and from the canvas's hit; hiding a folder of distant layers
/// makes them again.
#[test]
fn a_hidden_folder_hides_its_members() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let (_tmp2, mut own) = open_fixture(&root);
    let shown = doc.level().front.clone();
    let (_, _, f) = doc.group(&ids(&["tiny"]), "").unwrap();
    doc.set_folder(&f, FolderField::Visible(false)).unwrap();
    own.set("tiny", Property::Visible(false)).unwrap();
    assert!(doc.level().front != shown, "tiny is gone from the bake");
    assert!(doc.level().front == own.level().front && doc.level().material == own.level().material);
    let i = doc.index_of("tiny").unwrap();
    assert!(!doc.shown_at(i) && doc.items()[i].element.visible);
    doc.set_folder(&f, FolderField::Visible(true)).unwrap();
    assert!(doc.level().front == shown);

    // Distant layers in a folder.
    doc.set("tiny", Property::Material(ElementMaterial::Background)).unwrap();
    doc.set_distant("tiny", Some(50)).unwrap();
    let (_, _, d) = doc.group(&ids(&["tiny"]), "Far").unwrap();
    let generation = doc.distant_generation();
    doc.set_folder(&d, FolderField::Visible(false)).unwrap();
    assert!(doc.distant_generation() > generation, "the distant layers are made again");

    // The canvas: tiny on top in a folder is hit until the folder hides.
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    for _ in 0..5 {
        olr_edit::element::raise(&mut pack, "tiny").unwrap();
    }
    let f = olr_edit::element::group(&mut pack, &ids(&["tiny"]), "Top").unwrap();
    olr_edit::pack::save(&dir, &pack).unwrap();
    let click = |s: &mut olr_editor::EditorState| {
        run(s, A::Tool(Tool::Select));
        run(s, A::Pointer { kind: PointerKind::Down, world: (70, 52), shift: false, toggle: false });
        run(s, A::Pointer { kind: PointerKind::Up, world: (70, 52), shift: false, toggle: false });
        s.selection().clone()
    };
    let (_t, mut s) = editor_at(&root, &dir);
    // A click selects the folder holding it (issue #61).
    assert_eq!(click(&mut s), Selection::Folder(f.clone()));
    olr_edit::element::set(&mut pack, &f, Property::Visible(false)).unwrap();
    olr_edit::pack::save(&dir, &pack).unwrap();
    let (_t, mut s) = editor_at(&root, &dir);
    assert_ne!(click(&mut s), Selection::Folder(f), "a hidden folder's member is not hit");
}

/// Plan 14d R4: a locked folder refuses its members' content changes, not
/// their order.
#[test]
fn a_locked_folder_refuses_content_but_not_order() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc, outer, inner) = nested(&root);
    doc.set_folder(&outer, FolderField::Locked(true)).unwrap();
    let locked = |r: Result<(), DocError>| matches!(r, Err(DocError::Locked(_)));
    assert!(locked(doc.set("badge", Property::Name("x".into())).map(|_| ())));
    assert!(locked(doc.set("tiny", Property::At((1, 1))).map(|_| ())));
    assert!(locked(doc.remove("gradient").map(|_| ())));
    assert!(locked(doc.begin_stroke("badge", olr_editor::ops::Stroke::Erase).map(|_| ())));
    assert!(locked(doc.set_many(&ids(&["tunnels", "tiny"]), Property::Visible(false)).map(|_| ())));
    // Order, and the lock itself, are allowed.
    doc.set("badge", Property::Locked(true)).unwrap();
    round_trip(&mut doc, |d| d.move_into("badge", None, None).map(|r| r.0));
    round_trip(&mut doc, |d| d.move_to("gradient", 0).map(|c| vec![c.edit]));
    assert!(!doc.is_folder(&inner), "emptied, it went");
    round_trip(&mut doc, |d| d.ungroup(&outer).map(|r| r.0));
    assert!(doc.set("badge", Property::Name("free".into())).is_err(), "its own lock still holds");
}

/// Copying a folder copies its subtree; the paste makes new folders, its
/// elements naming none of the originals'.
#[test]
fn copy_and_paste_of_a_folder_makes_new_folders() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc, outer, inner) = nested(&root);
    let (items, copied) = doc.copy_tree(std::slice::from_ref(&outer));
    assert_eq!((items.len(), copied.len()), (3, 2));
    let s = doc.level().art_scale;
    let mut pasted = Vec::new();
    round_trip(&mut doc, |d| {
        let (edits, new) = d.paste_tree(&items, &copied, s, (4, 4))?;
        pasted = new;
        Ok(edits)
    });
    let all = doc.folders();
    assert_eq!(all.len(), 4);
    let new: Vec<&Folder> = all.iter().filter(|f| f.id != outer && f.id != inner).collect();
    assert_eq!(new.len(), 2);
    let (top, sub) = if new[0].parent.is_none() { (new[0], new[1]) } else { (new[1], new[0]) };
    assert_eq!(sub.parent.as_deref(), Some(top.id.as_str()), "the subfolder in the new folder");
    let n = doc.items().len();
    assert_eq!(pasted.iter().map(|id| doc.index_of(id).unwrap()).collect::<Vec<_>>(), [n - 3, n - 2, n - 1]);
    for id in &pasted {
        let f = folder_of(&doc, id).unwrap();
        assert!(f == top.id || f == sub.id, "{id} names {f}");
    }
    // Copying some of a folder's elements copies them flat.
    let (items, copied) = doc.copy_tree(&ids(&["tiny", "gradient"]));
    assert!(copied.is_empty() && items.iter().all(|s| s.item.element.folder.is_none()));
    assert!(doc.copy(std::slice::from_ref(&inner)).iter().all(|s| s.item.element.folder.is_none()));
}

#[test]
fn placing_a_two_piece_asset_gives_a_folder() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let asset = doc.asset_of_elements(&ids(&["tiny", "gradient"]), "Pair").unwrap();
    assert!(asset.pieces.iter().all(|p| p.element.folder.is_none()));
    let before = files(&doc);
    let placed = doc.place_asset(&asset, (10, 10)).unwrap();
    let f = placed.folder.clone().expect("a folder");
    assert_eq!(folder(&doc, &f).name, "Pair");
    assert_eq!(doc.folder_members(&f), placed.ids);
    let mut h = History::new();
    h.push(placed.edits);
    for e in h.undo().unwrap().to_vec().iter().rev() {
        doc.apply(e, false);
    }
    assert!(files(&doc) == before, "one step");
    // A folder's elements save as an asset flat.
    for e in h.redo().unwrap().to_vec().iter() {
        doc.apply(e, true);
    }
    let again = doc.asset_of_elements(&placed.ids, "Again").unwrap();
    assert!(again.pieces.iter().all(|p| p.element.folder.is_none()));
}

/// The editor copies a folder whose elements are all selected, and selects
/// the pasted folder.
#[test]
fn the_editor_copies_a_filled_folder() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    olr_edit::element::group(&mut pack, &ids(&["gradient", "badge"]), "Two").unwrap();
    olr_edit::pack::save(&dir, &pack).unwrap();
    let (_t, mut s) = editor_at(&root, &dir);
    run(&mut s, A::Select(Selection::Element("gradient".into())));
    run(&mut s, A::SelectToggle("badge".into()));
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    let doc = s.document().unwrap();
    assert_eq!(doc.folders().len(), 2, "the folder pasted anew");
    // The new folder is selected whole (issue #61).
    let Selection::Folder(new) = s.selection().clone() else { panic!("the copy selected") };
    assert_eq!(doc.folder_members(&new).len(), 2);
}
