//! Layer folders (issue #59): `olr_edit::folders`' tree operations on a
//! composition, and folders through the pack operations and the bake.

use std::collections::BTreeSet;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::compose::{Baker, ImageCache, bake};
use olr_edit::element::{self, ElementError, Property};
use olr_edit::folders::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::image_io;
use olr_format::levels::{Composition, Element, ElementKind, ElementMaterial, LevelSource};
use proptest::prelude::*;

fn layer(id: &str) -> Element {
    Element {
        id: id.into(),
        name: String::new(),
        visible: true,
        locked: false,
        folder: None,
        kind: ElementKind::drawn(ElementMaterial::Dirt),
        glow: None,
        clear: false,
        glass: None,
        distant: None,
        surface: None,
        back_surface: None,
        surface_map: false,
        back_surface_map: false,
    }
}

/// Elements `ids`, bottom to top, in no folder.
fn stack(ids: &[&str]) -> Composition {
    Composition { elements: ids.iter().map(|id| layer(id)).collect(), folders: Vec::new() }
}

/// The stack as `id` or `id@folder`, bottom to top.
fn shape(c: &Composition) -> Vec<String> {
    c.elements
        .iter()
        .map(|e| match &e.folder {
            Some(f) => format!("{}@{f}", e.id),
            None => e.id.clone(),
        })
        .collect()
}

/// The folders as `id` or `id<parent`, in their order.
fn folders(c: &Composition) -> Vec<String> {
    c.folders
        .iter()
        .map(|f| match &f.parent {
            Some(p) => format!("{}<{p}", f.id),
            None => f.id.clone(),
        })
        .collect()
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn group_a_contiguous_block_in_place() {
    let mut c = stack(&["a", "b", "c", "d"]);
    let f = group(&mut c, &ids(&["c", "b"]), "").unwrap();
    assert_eq!(f, "folder-1");
    assert_eq!(c.folders[0].name, "Folder 1");
    assert_eq!(shape(&c), ["a", "b@folder-1", "c@folder-1", "d"]);
    // The next default name is the next free number.
    let g = group(&mut c, &ids(&["d"]), "").unwrap();
    assert_eq!((g.as_str(), c.folders[1].name.as_str()), ("folder-2", "Folder 2"));
    check_folders(&c).unwrap();
}

#[test]
fn group_moves_scattered_members_to_the_topmost_ones_place() {
    let mut c = stack(&["a", "b", "c", "d", "e"]);
    group(&mut c, &ids(&["a", "d"]), "Desk").unwrap();
    assert_eq!(shape(&c), ["b", "c", "a@desk", "d@desk", "e"]);
    assert_eq!(c.folders[0].name, "Desk");
    // Folder ids are never an element's, nor an element's a folder's.
    let mut c = stack(&["desk", "b"]);
    assert_eq!(group(&mut c, &ids(&["b"]), "Desk").unwrap(), "desk-2");
    assert_eq!(new_folder_id(&c, "Desk"), "desk-3");
    let files = BTreeSet::new();
    assert_eq!(element::new_element_id(&c.elements, &c.folders, &files, "Desk 2", "x"), "desk-2-2");
}

#[test]
fn nested_groups_share_the_innermost_folder() {
    let mut c = stack(&["a", "b", "c", "d"]);
    let inner = group(&mut c, &ids(&["b", "c"]), "Inner").unwrap();
    let outer = group(&mut c, &ids(&[&inner, "d"]), "Outer").unwrap();
    assert_eq!(shape(&c), ["a", "b@inner", "c@inner", "d@outer"]);
    assert_eq!(folders(&c), ["outer", "inner<outer"], "a parent before its children");
    // Within `inner`: a third folder inside it.
    let deep = group(&mut c, &ids(&["c"]), "Deep").unwrap();
    assert_eq!(folders(&c), ["outer", "inner<outer", "deep<inner"]);
    assert_eq!(shape(&c), ["a", "b@inner", "c@deep", "d@outer"]);
    // A member and its own folder: the folder goes, holding it.
    let all = group(&mut c, &ids(&["c", &outer]), "All").unwrap();
    assert_eq!(folders(&c), ["all", "outer<all", "inner<outer", "deep<inner"]);
    // Members in different folders: the new one lies in what they share.
    let mut c = stack(&["a", "b", "c", "d"]);
    group(&mut c, &ids(&["a", "b", "c"]), "P").unwrap();
    group(&mut c, &ids(&["a"]), "Q").unwrap();
    group(&mut c, &ids(&["c"]), "R").unwrap();
    group(&mut c, &ids(&["a", "c"]), "S").unwrap();
    assert_eq!(shape(&c), ["b@p", "a@s", "c@s", "d"]);
    assert_eq!(folders(&c), ["p", "s<p"], "emptied Q and R go");
    assert!(group(&mut c, &ids(&["zz"]), "").is_err());
    assert!(group(&mut c, &[], "").is_err());
    let _ = (deep, all);
}

#[test]
fn ungroup_hands_the_members_to_the_parent_in_order() {
    let mut c = stack(&["a", "b", "c", "d"]);
    let inner = group(&mut c, &ids(&["b", "c"]), "Inner").unwrap();
    let outer = group(&mut c, &ids(&["a", &inner]), "Outer").unwrap();
    ungroup(&mut c, &outer).unwrap();
    assert_eq!(shape(&c), ["a", "b@inner", "c@inner", "d"]);
    assert_eq!(folders(&c), ["inner"]);
    ungroup(&mut c, &inner).unwrap();
    assert_eq!(c, stack(&["a", "b", "c", "d"]));
    assert_eq!(ungroup(&mut c, "a"), Err(FolderError::NotAFolder("a".into())));
}

#[test]
fn move_in_and_out() {
    let mut c = stack(&["a", "b", "c", "d"]);
    let f = group(&mut c, &ids(&["b", "c"]), "F").unwrap();
    // In: on top of the folder.
    move_to(&mut c, "a", Some(&f), None).unwrap();
    assert_eq!(shape(&c), ["b@f", "c@f", "a@f", "d"]);
    // Out: just above the folder it leaves.
    move_to(&mut c, "b", None, None).unwrap();
    assert_eq!(shape(&c), ["c@f", "a@f", "b", "d"]);
    // At a position among the folder's children.
    move_to(&mut c, "d", Some(&f), Some(0)).unwrap();
    assert_eq!(shape(&c), ["d@f", "c@f", "a@f", "b"]);
    // Already there: nothing changes.
    let before = c.clone();
    move_to(&mut c, "c", Some(&f), None).unwrap();
    assert_eq!(c, before);
    // A folder never goes into itself, nor into its own subfolder.
    let g = group(&mut c, &ids(&["c"]), "G").unwrap();
    assert!(move_to(&mut c, &f, Some(&g), None).is_err());
    assert!(move_to(&mut c, &f, Some(&f), None).is_err());
    assert_eq!(move_to(&mut c, "a", Some("b"), None), Err(FolderError::NotAFolder("b".into())));
    // Its last member gone, a folder goes too.
    move_to(&mut c, "c", None, None).unwrap();
    assert_eq!(folders(&c), ["f"]);
}

#[test]
fn raise_and_lower_at_folder_edges() {
    let mut c = stack(&["a", "b", "c", "d"]);
    let f = group(&mut c, &ids(&["b", "c"]), "F").unwrap();
    // An element at a folder's top leaves it, the stack unchanged.
    raise(&mut c, "c").unwrap();
    assert_eq!(shape(&c), ["a", "b@f", "c", "d"]);
    lower(&mut c, "c").unwrap();
    assert_eq!(shape(&c), ["a", "c", "b@f", "d"], "below it is the folder: c jumps past its block");
    let mut c = stack(&["a", "b", "c", "d"]);
    group(&mut c, &ids(&["b", "c"]), "F").unwrap();
    // An element below a folder jumps past its whole block.
    raise(&mut c, "a").unwrap();
    assert_eq!(shape(&c), ["b@f", "c@f", "a", "d"]);
    // A folder moves its block past a neighbour.
    raise(&mut c, &f).unwrap();
    assert_eq!(shape(&c), ["a", "b@f", "c@f", "d"]);
    raise(&mut c, &f).unwrap();
    assert_eq!(shape(&c), ["a", "d", "b@f", "c@f"]);
    raise(&mut c, &f).unwrap();
    assert_eq!(shape(&c), ["a", "d", "b@f", "c@f"], "the top one stays");
    // At a folder's bottom, lower leaves it.
    lower(&mut c, "b").unwrap();
    assert_eq!(shape(&c), ["a", "d", "b", "c@f"]);
    // Inside a folder, siblings swap as before.
    let mut c = stack(&["a", "b", "c"]);
    group(&mut c, &ids(&["a", "b", "c"]), "F").unwrap();
    raise(&mut c, "a").unwrap();
    assert_eq!(shape(&c), ["b@f", "a@f", "c@f"]);
    // A subfolder at its parent's top leaves it as an element does.
    let g = group(&mut c, &ids(&["c"]), "G").unwrap();
    raise(&mut c, &g).unwrap();
    assert_eq!(shape(&c), ["b@f", "a@f", "c@g"]);
    assert_eq!(folders(&c), ["f", "g"]);
}

#[test]
fn distant_layers_keep_below_and_apart() {
    let mut c = stack(&["sky", "a", "b"]);
    c.elements[0].kind = ElementKind::drawn(ElementMaterial::Background);
    c.elements[0].distant = Some(50);
    let f = group(&mut c, &ids(&["a", "b"]), "F").unwrap();
    assert_eq!(lower(&mut c, &f), Err(FolderError::Bad(element::DISTANT_ORDER.into())));
    let e = group(&mut c, &ids(&["sky", "a"]), "Mixed").unwrap_err().to_string();
    assert!(e.contains("distant"), "{e}");
    assert!(move_to(&mut c, "sky", Some(&f), Some(0)).is_err());
    // A folder of distant layers is fine.
    let sky = group(&mut c, &ids(&["sky"]), "Sky").unwrap();
    assert_eq!(raise(&mut c, &sky), Err(FolderError::Bad(element::DISTANT_ORDER.into())));
    check_folders(&c).unwrap();
}

#[test]
fn removing_a_folder_removes_its_contents() {
    let mut c = stack(&["a", "b", "c", "d"]);
    let inner = group(&mut c, &ids(&["b", "c"]), "Inner").unwrap();
    let outer = group(&mut c, &ids(&[&inner, "d"]), "Outer").unwrap();
    let gone = remove_folder(&mut c, &outer).unwrap();
    assert_eq!(gone.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["b", "c", "d"]);
    assert_eq!(c, stack(&["a"]));
    assert_eq!(remove_folder(&mut c, "a"), Err(FolderError::NotAFolder("a".into())));
    assert_eq!(remove_folder(&mut c, "x"), Err(FolderError::NoItem("x".into())));
}

#[test]
fn tidy_removes_empty_folders_and_orders_the_rest() {
    let mut c = stack(&["a", "b", "c", "d"]);
    let p = group(&mut c, &ids(&["c", "d"]), "P").unwrap();
    group(&mut c, &ids(&["a"]), "Q").unwrap();
    group(&mut c, &ids(&["d"]), "R").unwrap();
    let canonical = c.clone();
    assert_eq!(folders(&c), ["q", "p", "r<p"]);
    c.folders.reverse();
    assert!(check_folders(&c).is_err(), "out of order");
    tidy(&mut c);
    assert_eq!(c, canonical);
    // An element removed leaves its folder empty: it goes, so do parents
    // left empty.
    c.elements.retain(|e| e.id != "a");
    tidy(&mut c);
    assert_eq!(folders(&c), ["p", "r<p"]);
    c.elements.retain(|e| e.folder.as_deref() != Some("r") && e.folder.as_deref() != Some(&p));
    tidy(&mut c);
    assert!(c.folders.is_empty());
    // A composition without folders is left as it is.
    let mut plain = stack(&["a", "b"]);
    tidy(&mut plain);
    assert_eq!(plain, stack(&["a", "b"]));
}

#[test]
fn equal_trees_write_alike() {
    // One tree made two ways.
    let mut one = stack(&["a", "b", "c", "d"]);
    group(&mut one, &ids(&["c", "d"]), "Lamp").unwrap();
    group(&mut one, &ids(&["d"]), "Glow").unwrap();
    let mut two = stack(&["a", "b", "c", "d"]);
    group(&mut two, &ids(&["d"]), "Glow").unwrap();
    group(&mut two, &ids(&["glow", "c"]), "Lamp").unwrap();
    assert_eq!(one, two);
    let text = |c: &Composition| olr_format::ron_io::to_canonical("editor/composition.ron", c).unwrap();
    assert_eq!(text(&one), text(&two));
    assert_eq!(flatten(tree(&one).unwrap()), one);
}

#[test]
fn effective_flags_and_walks_are_safe_on_cycles() {
    let mut c = stack(&["a", "b"]);
    let f = group(&mut c, &ids(&["a"]), "F").unwrap();
    let g = group(&mut c, &ids(&[&f]), "G").unwrap();
    c.folders.iter_mut().find(|x| x.id == g).unwrap().visible = false;
    c.folders.iter_mut().find(|x| x.id == f).unwrap().locked = true;
    assert!(!shown(&c, &c.elements[0]) && locked(&c, &c.elements[0]));
    assert!(shown(&c, &c.elements[1]) && !locked(&c, &c.elements[1]));
    let flat = folded(&c);
    assert!(!flat[0].visible && flat[0].locked && flat[1].visible);
    assert!(c.elements[0].visible, "the element's own flag is kept");
    // A cycle: g's parent is f. Every walk still ends.
    c.folders.iter_mut().find(|x| x.id == g).unwrap().parent = Some(f.clone());
    assert!(!shown(&c, &c.elements[0]));
    assert!(locked(&c, &c.elements[0]));
    assert_eq!(folder_chain(&c, Some(&f)).len(), 2);
    assert!(tree(&c).is_err());
    assert!(raise(&mut c, "a").is_err());
    let mut t = c.clone();
    tidy(&mut t);
    assert_eq!(t.folders.len(), 2);
    assert_eq!(depth(&c, &f), 2);
}

/// One operation of the property test, on indices into the current ids.
#[derive(Clone, Debug)]
enum Op {
    Group(Vec<usize>),
    Ungroup(usize),
    Move(usize, Option<usize>, Option<usize>),
    Raise(usize),
    Lower(usize),
    RemoveFolder(usize),
    RemoveElement(usize),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => prop::collection::vec(0usize..64, 1..4).prop_map(Op::Group),
        1 => (0usize..64).prop_map(Op::Ungroup),
        2 => (0usize..64, prop::option::of(0usize..64), prop::option::of(0usize..6))
            .prop_map(|(a, b, p)| Op::Move(a, b, p)),
        2 => (0usize..64).prop_map(Op::Raise),
        2 => (0usize..64).prop_map(Op::Lower),
        1 => (0usize..64).prop_map(Op::RemoveFolder),
        1 => (0usize..64).prop_map(Op::RemoveElement),
    ]
}

fn all_ids(c: &Composition) -> Vec<String> {
    c.elements.iter().map(|e| e.id.clone()).chain(c.folders.iter().map(|f| f.id.clone())).collect()
}

fn folder_at(c: &Composition, i: usize) -> Option<String> {
    (!c.folders.is_empty()).then(|| c.folders[i % c.folders.len()].id.clone())
}

proptest! {
    /// Every operation keeps the rules, writes the canonical tree and
    /// keeps its elements; a refused one changes nothing.
    #[test]
    fn every_operation_keeps_the_rules(ops in prop::collection::vec(op(), 1..24)) {
        let mut c = stack(&["sky", "far", "a", "b", "c", "d", "e", "f"]);
        for e in &mut c.elements[..2] {
            e.kind = ElementKind::drawn(ElementMaterial::Background);
            e.distant = Some(40);
        }
        for op in ops {
            // A removed folder may take every element with it.
            if c.elements.is_empty() {
                break;
            }
            let before = c.clone();
            let n = all_ids(&c);
            let pick = |i: usize| n[i % n.len()].clone();
            let mut removed: Vec<String> = Vec::new();
            let result = match &op {
                Op::Group(v) => group(&mut c, &v.iter().map(|i| pick(*i)).collect::<Vec<_>>(), "").map(|_| ()),
                Op::Ungroup(i) => match folder_at(&c, *i) {
                    Some(f) => ungroup(&mut c, &f),
                    None => Ok(()),
                },
                Op::Move(a, into, p) => {
                    let into = into.and_then(|i| folder_at(&c, i));
                    move_to(&mut c, &pick(*a), into.as_deref(), *p)
                }
                Op::Raise(i) => raise(&mut c, &pick(*i)),
                Op::Lower(i) => lower(&mut c, &pick(*i)),
                Op::RemoveFolder(i) => match folder_at(&c, *i) {
                    Some(f) => remove_folder(&mut c, &f).map(|gone| {
                        removed = gone.into_iter().map(|e| e.id).collect();
                    }),
                    None => Ok(()),
                },
                Op::RemoveElement(i) => {
                    if c.elements.len() > 1 {
                        let e = c.elements.remove(i % c.elements.len());
                        removed.push(e.id);
                        tidy(&mut c);
                    }
                    Ok(())
                }
            };
            match result {
                Ok(()) => {
                    prop_assert!(check_folders(&c).is_ok(), "{op:?}: {:?}", check_folders(&c));
                    prop_assert_eq!(&flatten(tree(&c).unwrap()), &c, "{:?}: canonical", op);
                    let mut t = c.clone();
                    tidy(&mut t);
                    prop_assert_eq!(&t, &c);
                    let want: BTreeSet<String> = before
                        .elements
                        .iter()
                        .map(|e| e.id.clone())
                        .filter(|id| !removed.contains(id))
                        .collect();
                    let have: BTreeSet<String> = c.elements.iter().map(|e| e.id.clone()).collect();
                    prop_assert_eq!(have, want, "{:?}: the elements kept", op);
                    // Distant layers stay below the others.
                    let near = c.elements.iter().position(|e| !e.is_distant()).unwrap_or(c.elements.len());
                    prop_assert!(c.elements[near..].iter().all(|e| !e.is_distant()), "{:?}", op);
                }
                Err(_) => prop_assert_eq!(&c, &before, "{:?}: refused, unchanged", op),
            }
        }
    }
}

// Folders through the pack operations and the bake.

const RED: Rgba = Rgba::rgb(255, 0, 0);

/// A plain 8 × 6 level pack at art scale 1 (all dirt), in memory.
fn pack() -> Pack {
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: "t".into(),
        name: "T".into(),
        version: "1".into(),
        kind: PackKind::Level,
        authors: vec![],
        origin: None,
        license: None,
    };
    let (w, h) = (8, 6);
    let level = LevelSource {
        name: "T".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: 1,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Dirt; (w * h) as usize],
        front: RgbaImage::new(w, h),
        back: RgbaImage::new(w, h),
        thumb: None,
        look: None,
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
        live: None,
    };
    Pack::new(manifest, Source::Level(level))
}

/// A rock layer `wall` painted red over (0, 0, 4, 2), in folder `lamp`.
fn walled() -> Pack {
    let mut p = pack();
    let id = element::add_layer(&mut p, ElementMaterial::Rock, "Wall").unwrap();
    element::draw(&mut p, &id, &Shape::Rect { x: 0, y: 0, w: 4, h: 2 }, &Paint::Colour(RED), None).unwrap();
    assert_eq!(element::group(&mut p, &[id], "Lamp").unwrap(), "lamp");
    p
}

fn baked(p: &Pack) -> LevelSource {
    let Source::Level(l) = &p.source else { panic!("a level") };
    let mut l = l.clone();
    bake(&mut l, p.composition.as_ref().unwrap(), &RgbaImage::new(4, 4)).unwrap();
    l
}

fn composition(p: &Pack) -> &Composition {
    &p.composition.as_ref().unwrap().composition
}

#[test]
fn a_hidden_folder_hides_its_visible_members() {
    let mut p = walled();
    assert_eq!(baked(&p).material[0], Material::Rock);
    element::set(&mut p, "lamp", Property::Visible(false)).unwrap();
    assert!(composition(&p).elements[1].visible, "the member's own flag stays");
    assert_eq!(baked(&p).material[0], Material::Dirt);
    element::set(&mut p, "lamp", Property::Visible(true)).unwrap();
    assert_eq!(baked(&p).material[0], Material::Rock);
    // A hidden member of a shown folder stays hidden.
    element::set(&mut p, "wall", Property::Visible(false)).unwrap();
    assert_eq!(baked(&p).material[0], Material::Dirt);
}

#[test]
fn a_folders_visibility_rebakes_without_decoding_again() {
    let mut p = pack();
    let mut picture = RgbaImage::new(4, 4);
    for (x, y) in (0..4).flat_map(|x| (0..4).map(move |y| (x, y))) {
        picture.set(x, y, RED);
    }
    let bytes = image_io::encode_rgba(&picture);
    let id = element::add_image(&mut p, "pic.png", bytes, (0, 0), (4, 4), ElementMaterial::Rock, "").unwrap();
    element::group(&mut p, std::slice::from_ref(&id), "Pics").unwrap();
    element::set(&mut p, "pics", Property::Visible(false)).unwrap();
    let tile = RgbaImage::new(4, 4);
    let (size, s) = ((8, 6), 1);
    // Hidden by its folder, the picture is still processed: showing the
    // folder needs a rebake only.
    let mut cache = ImageCache::new();
    let src = p.composition.clone().unwrap();
    let hidden = Baker::new_cached(&src, size, s, &tile, &mut cache).unwrap();
    assert_eq!(cache.processed(), 1);
    element::set(&mut p, "pics", Property::Visible(true)).unwrap();
    let src = p.composition.clone().unwrap();
    let shown = Baker::new_cached(&src, size, s, &tile, &mut cache).unwrap();
    assert_eq!(cache.processed(), 1, "nothing decoded again");
    let Source::Level(l) = &p.source else { panic!() };
    let rect = olr_edit::compose::Rect { x: 0, y: 0, w: 8, h: 6 };
    let (mut a, mut b) = (l.clone(), l.clone());
    hidden.bake_region(&mut a, rect);
    shown.bake_region(&mut b, rect);
    assert_eq!((a.material[0], b.material[0]), (Material::Dirt, Material::Rock));
    // Its own flag hidden, it is not processed (as before).
    element::set(&mut p, &id, Property::Visible(false)).unwrap();
    let mut fresh = ImageCache::new();
    Baker::new_cached(p.composition.as_ref().unwrap(), size, s, &tile, &mut fresh).unwrap();
    assert_eq!(fresh.processed(), 0);
}

#[test]
fn a_locked_folder_locks_its_members_but_not_their_order() {
    let mut p = walled();
    element::add_layer(&mut p, ElementMaterial::Dirt, "Top").unwrap();
    element::set(&mut p, "lamp", Property::Locked(true)).unwrap();
    let rect = Shape::Rect { x: 0, y: 0, w: 1, h: 1 };
    let locked = Err(ElementError::Locked("wall".into()));
    assert_eq!(element::draw(&mut p, "wall", &rect, &Paint::Colour(RED), None), locked);
    assert_eq!(element::set(&mut p, "wall", Property::Name("X".into())), locked);
    assert_eq!(element::remove(&mut p, "wall"), locked);
    assert_eq!(element::remove(&mut p, "lamp"), locked);
    assert_eq!(
        element::set(&mut p, "lamp", Property::Name("X".into())),
        Err(ElementError::Locked("lamp".into()))
    );
    // Order changes are allowed.
    element::raise(&mut p, "lamp").unwrap();
    assert_eq!(shape(composition(&p)), ["base", "top", "wall@lamp"]);
    let f = element::group(&mut p, &ids(&["top", "lamp"]), "").unwrap();
    element::ungroup(&mut p, &f).unwrap();
    element::move_to(&mut p, "wall", None, None).unwrap();
    assert!(composition(&p).folders.is_empty(), "its only member out, the folder goes");
}

#[test]
fn pack_operations_keep_the_folder() {
    let mut p = walled();
    // Duplicate: directly above, in the same folder.
    let copy = element::duplicate(&mut p, "wall").unwrap();
    assert_eq!(shape(composition(&p)), ["base", "wall@lamp", &format!("{copy}@lamp")]);
    // Merge down: the lower layer's folder.
    element::merge_down(&mut p, &copy).unwrap();
    assert_eq!(shape(composition(&p)), ["base", "wall@lamp"]);
    // Split: the parts take the folder.
    let parts = element::split(&mut p, "wall").unwrap();
    let want: Vec<String> = parts.iter().map(|id| format!("{id}@lamp")).collect();
    assert_eq!(shape(composition(&p))[1..], want[..]);
    // A layer made distant leaves its folder.
    let top = element::add_layer(&mut p, ElementMaterial::Background, "Sky").unwrap();
    element::move_to(&mut p, &top, Some("lamp"), None).unwrap();
    element::set(&mut p, &top, Property::Distant(Some(30))).unwrap();
    let c = composition(&p);
    assert_eq!(c.elements[0].id, top);
    assert_eq!(c.elements[0].folder, None);
    // Removing a folder removes its members and their files.
    element::remove(&mut p, "lamp").unwrap();
    let src = p.composition.as_ref().unwrap();
    assert!(src.composition.folders.is_empty());
    assert_eq!(src.composition.elements.len(), 2);
    assert!(
        src.files.keys().all(|f| !parts.iter().any(|id| f.contains(id.as_str()))),
        "{:?}",
        src.files.keys()
    );
}

#[test]
fn a_folder_takes_a_name_visibility_and_lock_only() {
    let mut p = walled();
    element::set_all(&mut p, "lamp", &[Property::Name("Desk lamp".into()), Property::Locked(true)]).unwrap();
    let f = &composition(&p).folders[0];
    assert_eq!((f.name.as_str(), f.locked), ("Desk lamp", true));
    assert!(matches!(
        element::set(&mut p, "lamp", Property::Material(ElementMaterial::Dirt)),
        Err(ElementError::Locked(_))
    ));
    element::set(&mut p, "lamp", Property::Locked(false)).unwrap();
    assert!(matches!(
        element::set(&mut p, "lamp", Property::Material(ElementMaterial::Dirt)),
        Err(ElementError::WrongKind { .. })
    ));
}
