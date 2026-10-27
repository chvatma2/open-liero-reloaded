//! Issue #43: the level's surface materials in the editor: added from the
//! library, removed only when unused, an element's surfaces, the Surface
//! brush on an element's own pixels and its back art (one step a stroke),
//! the Materials view, and a save that keeps the painted maps and bakes
//! them.

use olr_edit::element::Property;
use olr_editor::tools::{PointerKind, Tool};
use olr_editor::{EditorAction as A, EditorState, Selection};
use olr_format::levels::surface_file;

use crate::common::{catalog, content, ground, run};

fn materials(s: &EditorState) -> Vec<String> {
    s.active_document().unwrap().materials().iter().map(|m| m.id.clone()).collect()
}

fn add(s: &mut EditorState, id: &str) {
    run(s, A::AddMaterial { builtin: true, id: id.into() });
}

fn element(s: &EditorState, id: &str) -> olr_format::levels::Element {
    let doc = s.active_document().unwrap();
    doc.items()[doc.index_of(id).unwrap()].element.clone()
}

/// Element `id`'s painted map (`back`: its back art's) as it bakes.
fn map(s: &EditorState, id: &str, back: bool) -> Option<Vec<u8>> {
    let doc = s.active_document().unwrap();
    doc.surface_map(&element(s, id), back).map(|m| m.as_ref().clone())
}

/// A Surface brush dab at world `at`.
fn dab(s: &mut EditorState, at: (i32, i32)) {
    for kind in [PointerKind::Down, PointerKind::Up] {
        run(s, A::Pointer { kind, world: at, shift: false, toggle: false });
    }
}

fn message(s: &EditorState) -> String {
    s.status().message.clone().unwrap_or_default()
}

#[test]
fn materials_come_from_the_library_and_go_when_unused() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    assert!(s.catalog().materials.iter().any(|m| m.builtin && m.material.id == "stone"));
    assert!(s.active_document().unwrap().level().look.is_none());
    add(&mut s, "stone");
    assert_eq!(materials(&s), ["stone"], "a copy, in a look made for it");
    assert!(message(&s).contains("Stone"), "{}", message(&s));
    add(&mut s, "stone");
    assert_eq!(materials(&s), ["stone"]);
    assert!(message(&s).contains("already"), "{}", message(&s));
    add(&mut s, "wood");
    assert_eq!(materials(&s), ["stone", "wood"]);
    // A surface names one of the level's materials only.
    run(&mut s, A::Set("tunnels".into(), Property::Surface(Some("glass".into()))));
    assert_eq!(element(&s, "tunnels").surface, None);
    assert!(message(&s).contains("glass"), "{}", message(&s));
    run(&mut s, A::Set("tunnels".into(), Property::Surface(Some("wood".into()))));
    assert_eq!(element(&s, "tunnels").surface.as_deref(), Some("wood"));
    // Wood is used: refused, naming the element; stone goes.
    assert_eq!(s.active_document().unwrap().material_users("wood"), ["Tunnels"]);
    run(&mut s, A::RemoveMaterial("wood".into()));
    assert_eq!(materials(&s), ["stone", "wood"]);
    assert!(message(&s).contains("Tunnels"), "{}", message(&s));
    run(&mut s, A::RemoveMaterial("stone".into()));
    assert_eq!(materials(&s), ["wood"]);
    assert_eq!(element(&s, "tunnels").surface.as_deref(), Some("wood"));
    run(&mut s, A::Undo);
    assert_eq!(materials(&s), ["stone", "wood"]);
    // A back surface needs back art.
    run(&mut s, A::Set("tunnels".into(), Property::BackSurface(Some("wood".into()))));
    assert_eq!(element(&s, "tunnels").back_surface, None);
    run(&mut s, A::Set("wall".into(), Property::BackSurface(Some("wood".into()))));
    assert_eq!(element(&s, "wall").back_surface.as_deref(), Some("wood"));
    // Removing the look takes the surfaces with the materials, one step.
    run(&mut s, A::SetLook(None));
    assert!(materials(&s).is_empty());
    assert_eq!((element(&s, "tunnels").surface, element(&s, "wall").back_surface), (None, None));
    run(&mut s, A::Undo);
    assert_eq!(element(&s, "wall").back_surface.as_deref(), Some("wood"));
}

#[test]
fn the_surface_brush_paints_front_and_back_one_step_a_stroke() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    add(&mut s, "stone");
    add(&mut s, "wood");
    run(&mut s, A::Tool(Tool::Surface));
    // Nothing selected: nothing painted, and why.
    dab(&mut s, (50, 50));
    assert!(message(&s).contains("select"), "{}", message(&s));
    assert!(!s.can_undo() || map(&s, "wall", false).is_none());
    run(&mut s, A::Select(Selection::Element("wall".into())));
    run(&mut s, A::Brush(olr_editor::tools::Brush { shape: olr_editor::tools::BrushShape::Square, size: 4 }));
    run(&mut s, A::SurfacePaint(Some("wood".into())));
    dab(&mut s, (50, 50));
    // The wall's box starts at (4, 0) and is 504 wide (the level's).
    let w = 504usize;
    let front = map(&s, "wall", false).expect("a front map");
    assert_eq!(front[50 * w + 46], 2, "wood under the dab");
    assert_eq!(front.iter().filter(|v| **v != 0).count(), 16, "a 4 × 4 dab");
    assert!(element(&s, "wall").surface_map);
    assert_eq!(map(&s, "wall", true), None);
    // The back art: stone.
    run(&mut s, A::SurfaceBack(true));
    run(&mut s, A::SurfacePaint(Some("stone".into())));
    dab(&mut s, (100, 100));
    let back = map(&s, "wall", true).expect("a back map");
    assert_eq!(back[100 * w + 96], 1);
    assert_eq!(map(&s, "wall", false), Some(front.clone()), "the front kept");
    // One undo a stroke.
    run(&mut s, A::Undo);
    assert_eq!(map(&s, "wall", true), None);
    assert!(!element(&s, "wall").back_surface_map);
    run(&mut s, A::Undo);
    assert_eq!(map(&s, "wall", false), None);
    assert!(!element(&s, "wall").surface_map);
    run(&mut s, A::Redo);
    run(&mut s, A::Redo);
    assert_eq!(map(&s, "wall", true).map(|m| m[100 * w + 96]), Some(1));
    // Stone is painted: not removed. Painted over with the element's
    // own (default), the map is all 0: no map, and stone may go; wood
    // moves down to 1, the front map with it.
    run(&mut s, A::RemoveMaterial("stone".into()));
    assert!(message(&s).contains("Wall"), "{}", message(&s));
    run(&mut s, A::SurfacePaint(None));
    dab(&mut s, (100, 100));
    assert_eq!(map(&s, "wall", true), None);
    assert!(!element(&s, "wall").back_surface_map);
    run(&mut s, A::RemoveMaterial("stone".into()));
    assert_eq!(materials(&s), ["wood"]);
    assert_eq!(map(&s, "wall", false).map(|m| m[50 * w + 46]), Some(1));
    run(&mut s, A::Undo);
    assert_eq!(map(&s, "wall", false).map(|m| m[50 * w + 46]), Some(2));
    // A stroke of no change is no step.
    run(&mut s, A::SurfaceBack(false));
    run(&mut s, A::SurfacePaint(Some("wood".into())));
    let before = s.can_redo();
    dab(&mut s, (50, 50));
    assert_eq!(s.can_redo(), before, "no step: redo kept");
}

#[test]
fn the_surface_brush_refuses_what_it_cannot_paint() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    add(&mut s, "stone");
    run(&mut s, A::Tool(Tool::Surface));
    run(&mut s, A::SurfacePaint(Some("stone".into())));
    // Back without back art paints the front.
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::SurfaceBack(true));
    dab(&mut s, (60, 60));
    assert!(map(&s, "tunnels", false).is_some());
    assert!(map(&s, "tunnels", true).is_none());
    // Locked.
    run(&mut s, A::Set("badge".into(), Property::Locked(true)));
    run(&mut s, A::Select(Selection::Element("badge".into())));
    dab(&mut s, (0, 110));
    assert!(message(&s).contains("locked"), "{}", message(&s));
    assert!(map(&s, "badge", false).is_none());
    // Distant: one surface for the whole layer.
    run(&mut s, A::Set("tunnels".into(), Property::Distant(Some(50))));
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    dab(&mut s, (60, 60));
    assert!(message(&s).contains("distant"), "{}", message(&s));
    run(&mut s, A::Set("tunnels".into(), Property::Surface(Some("stone".into()))));
    assert_eq!(element(&s, "tunnels").surface.as_deref(), Some("stone"));
    // The brush's material gone reads as the element's own.
    run(&mut s, A::Set("tunnels".into(), Property::Surface(None)));
    run(&mut s, A::SurfacePaint(Some("glass".into())));
    assert_eq!(s.surface_paint(), None);
}

#[test]
fn the_materials_view_and_the_collision_view_take_turns() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    run(&mut s, A::SurfaceView(true));
    assert!(s.surface_view() && !s.material_view());
    run(&mut s, A::MaterialView(true));
    assert!(!s.surface_view() && s.material_view());
    run(&mut s, A::SurfaceView(true));
    assert!(s.surface_view() && !s.material_view());
    assert!(!s.active_dirty(), "a view is no change");
    assert_eq!(olr_editor::surface_tint(0), None);
    assert_eq!(olr_editor::surface_tint(1), olr_editor::surface_tint(13), "tints cycle");
}

/// Saved, the painted maps and surfaces are the composition's, and the
/// bake's `look/surface.png` holds them; opened again, all is as it was.
#[test]
fn a_saved_level_keeps_its_materials_and_bakes_them() {
    let Some(root) = content() else { return };
    let (tmp, mut s) = ground(&root);
    let dir = s.active_document().unwrap().dir().to_path_buf();
    add(&mut s, "stone");
    add(&mut s, "metal");
    run(&mut s, A::Set("base".into(), Property::Surface(Some("stone".into()))));
    run(&mut s, A::Select(Selection::Element("base".into())));
    run(&mut s, A::Tool(Tool::Surface));
    run(&mut s, A::SurfacePaint(Some("metal".into())));
    run(&mut s, A::SurfaceBack(true));
    dab(&mut s, (200, 200));
    let level = s.active_document().unwrap().level().clone();
    let baked = level.surface.clone().expect("the bake has surfaces");
    assert!(baked.front.contains(&1), "stone in front");
    run(&mut s, A::Save);
    assert!(!s.active_dirty());
    let files = crate::common::tree(&dir);
    for f in ["look/surface.png", "look/surface-back.png", "editor/surface/base.back.png"] {
        assert!(files.contains_key(f), "{f} saved");
    }
    assert!(!files.contains_key(&surface_file("base")), "nothing painted in front");
    let pack = olr_edit::pack::load(&dir).unwrap();
    let doc = olr_editor::Document::open(pack, dir, &catalog(&root)).unwrap();
    assert_eq!(doc.level().surface.as_ref(), Some(&baked));
    assert_eq!(doc.materials(), s.active_document().unwrap().materials());
    let base = &doc.items()[doc.index_of("base").unwrap()].element;
    assert_eq!((base.surface.as_deref(), base.back_surface_map), (Some("stone"), true));
    drop(tmp);
}

/// The anti-goal: a level that uses no materials saves as it did.
#[test]
fn a_level_without_materials_saves_without_them() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = ground(&root);
    let dir = s.active_document().unwrap().dir().to_path_buf();
    let before = crate::common::tree(&dir);
    run(&mut s, A::Set("tunnels".into(), Property::Name("Tunnels 2".into())));
    run(&mut s, A::Set("tunnels".into(), Property::Name("Tunnels".into())));
    run(&mut s, A::Save);
    let after = crate::common::tree(&dir);
    assert!(after.keys().all(|f| !f.contains("surface")), "no surface files");
    assert_eq!(before.get("level.ron"), after.get("level.ron"));
}
