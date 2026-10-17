//! The view, the tools and the canvas interaction (plan 14b Task 7).

use olr_data::image::Rgba;
use olr_edit::element::Property;
use olr_editor::tools::{Brush, BrushShape, Overlay, PaintChoice, PointerKind, Tool};
use olr_editor::view::{View, ZOOMS};
use olr_editor::{EditorAction as A, EditorState, Selection};
use olr_format::levels::{ElementKind, LevelObject};

use crate::common::{content, editor, run};

fn pointer(s: &mut EditorState, kind: PointerKind, world: (i32, i32), shift: bool) {
    run(s, A::Pointer { kind, world, shift });
}

fn drag(s: &mut EditorState, from: (i32, i32), to: (i32, i32), shift: bool) {
    pointer(s, PointerKind::Down, from, shift);
    pointer(s, PointerKind::Move, to, shift);
    pointer(s, PointerKind::Up, to, shift);
}

fn image_box(s: &EditorState, id: &str) -> ((i32, i32), (u32, u32)) {
    let d = s.document().unwrap();
    match &d.items()[d.index_of(id).unwrap()].element.kind {
        ElementKind::Image { at, size, .. } => (*at, *size),
        _ => panic!("an image"),
    }
}

fn layer(s: &EditorState, id: &str) -> olr_data::image::RgbaImage {
    s.document().unwrap().layer_art(id).unwrap().clone()
}

#[test]
fn view_maps_both_ways_and_zooms_about_the_cursor() {
    for zoom in 0..ZOOMS.len() {
        for s in 1..=3 {
            let view = View { zoom, offset: [13.0, -7.5] };
            for p in [(0, 0), (5, 9), (-3, 2), (503, 349)] {
                assert_eq!(view.to_world(view.to_screen(p, s), s), p, "zoom {zoom}, art scale {s}");
            }
        }
    }
    let mut view = View::fit([800.0, 600.0], (504, 350));
    assert_eq!(view.scale(), 1.0);
    let about = [321.0, 123.0];
    for steps in [1, 2, -1, -3, 4] {
        let world = |v: &View| {
            let s = v.scale();
            [(about[0] - v.offset[0]) / s, (about[1] - v.offset[1]) / s]
        };
        let before = world(&view);
        view.zoom_about(steps, about);
        let after = world(&view);
        let tolerance = 1.0 / view.scale();
        assert!((before[0] - after[0]).abs() <= tolerance && (before[1] - after[1]).abs() <= tolerance);
        if view.scale() >= 1.0 {
            assert_eq!(view.offset.map(f32::fract), [0.0, 0.0], "pixel-snapped");
        }
    }
}

#[test]
fn a_brush_stroke_is_one_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    let Selection::Element(id) = s.selection().clone() else { panic!("the new layer is selected") };
    run(&mut s, A::Tool(Tool::Brush));
    let before = layer(&s, &id);
    drag(&mut s, (10, 10), (40, 10), false);
    let art = layer(&s, &id);
    for x in 10..=40 {
        assert_eq!(art.get(x, 10).a, 255, "x {x}");
    }
    run(&mut s, A::Undo);
    assert!(layer(&s, &id) == before, "one undo clears the stroke");
}

#[test]
fn a_stroke_off_the_level_is_clipped() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    let Selection::Element(id) = s.selection().clone() else { panic!() };
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::Brush(Brush { shape: BrushShape::Square, size: 256 }));
    let before = layer(&s, &id);
    // The first segment passes above the level; the second crosses it.
    pointer(&mut s, PointerKind::Down, (-1000, -1000), false);
    pointer(&mut s, PointerKind::Move, (100_000, 5), false);
    pointer(&mut s, PointerKind::Move, (-1000, 100), false);
    pointer(&mut s, PointerKind::Up, (-1000, 100), false);
    assert!(layer(&s, &id) != before);
    assert!(s.can_undo());
    run(&mut s, A::Undo);
    assert!(layer(&s, &id) == before);
}

#[test]
fn moving_an_image_is_live_and_one_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let processed = s.document().unwrap().cache_processed();
    let (at, _) = image_box(&s, "tiny");
    pointer(&mut s, PointerKind::Down, (50, 40), false);
    assert_eq!(s.selection(), &Selection::Element("tiny".into()));
    pointer(&mut s, PointerKind::Move, (53, 41), false);
    assert_eq!(image_box(&s, "tiny").0, (at.0 + 3, at.1 + 1), "live");
    pointer(&mut s, PointerKind::Move, (55, 43), false);
    pointer(&mut s, PointerKind::Up, (55, 43), false);
    assert_eq!(image_box(&s, "tiny").0, (at.0 + 5, at.1 + 3));
    assert_eq!(s.document().unwrap().cache_processed(), processed);
    run(&mut s, A::Undo);
    assert_eq!(image_box(&s, "tiny").0, at);
    assert!(!s.can_undo(), "one step");
}

#[test]
fn resizing_previews_then_applies() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let (at, size) = image_box(&s, "tiny");
    run(&mut s, A::Select(Selection::Element("tiny".into())));
    let corner = (at.0 + size.0 as i32, at.1 + size.1 as i32);
    pointer(&mut s, PointerKind::Down, corner, false);
    pointer(&mut s, PointerKind::Move, (corner.0 + 10, corner.1 + 10), false);
    assert_eq!(image_box(&s, "tiny"), (at, size), "unchanged during the drag");
    let outline = olr_edit::compose::Rect { x: at.0 as u32, y: at.1 as u32, w: size.0 + 10, h: size.1 + 10 };
    assert!(s.overlays().contains(&Overlay::Outline { rect: outline }), "{:?}", s.overlays());
    pointer(&mut s, PointerKind::Up, (corner.0 + 10, corner.1 + 10), false);
    assert_eq!(image_box(&s, "tiny"), (at, (size.0 + 10, size.1 + 10)));
    run(&mut s, A::Undo);
    assert_eq!(image_box(&s, "tiny"), (at, size), "one step");

    drag(&mut s, corner, (corner.0 + 10, corner.1 + 10), true);
    let (_, (w, h)) = image_box(&s, "tiny");
    assert!(w > size.0 && h > size.1);
    let skew = (i64::from(w) * i64::from(size.1) - i64::from(h) * i64::from(size.0)).abs();
    assert!(skew <= i64::from(size.0.max(size.1)), "{w}x{h} keeps {}x{}", size.0, size.1);
}

#[test]
fn locked_elements_are_not_changed_by_the_canvas() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Set("tiny".into(), Property::Locked(true)));
    let items = s.document().unwrap().items().to_vec();
    let refused = |s: &EditorState| s.status().message.clone().unwrap_or_default().contains("locked");
    drag(&mut s, (50, 40), (60, 50), false);
    assert!(refused(&s));
    run(&mut s, A::Nudge { dx: 1, dy: 0 });
    assert!(refused(&s));
    run(&mut s, A::Remove);
    assert!(refused(&s));
    assert!(s.document().unwrap().items() == items.as_slice());
    run(&mut s, A::Undo);
    assert!(!s.can_undo(), "no step was recorded after the lock");

    run(&mut s, A::Set("tunnels".into(), Property::Locked(true)));
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Brush));
    let before = layer(&s, "tunnels");
    drag(&mut s, (200, 200), (220, 200), false);
    assert!(refused(&s));
    assert!(layer(&s, "tunnels") == before);
}

#[test]
fn spawn_and_eyedropper() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let spawns = |s: &EditorState| s.document().unwrap().level().objects.len();
    let n = spawns(&s);
    run(&mut s, A::Tool(Tool::Spawn));
    pointer(&mut s, PointerKind::Down, (50, 60), false);
    pointer(&mut s, PointerKind::Up, (50, 60), false);
    assert_eq!(spawns(&s), n + 1);
    assert!(matches!(s.document().unwrap().level().objects[n], LevelObject::Spawn { at: (50, 60), .. }));
    run(&mut s, A::Undo);
    assert_eq!(spawns(&s), n);

    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::Paint(PaintChoice::Colour(Rgba::rgb(1, 2, 3))));
    drag(&mut s, (450, 300), (450, 300), false);
    run(&mut s, A::Paint(PaintChoice::FrontTile));
    run(&mut s, A::Tool(Tool::Eyedropper));
    pointer(&mut s, PointerKind::Down, (450, 300), false);
    assert_eq!(s.paint(), PaintChoice::Colour(Rgba::rgb(1, 2, 3)));
    assert_eq!(s.tool(), Tool::Brush, "back to the previous tool");
}

#[test]
fn shapes_paint_on_release() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Ellipse));
    let before = layer(&s, "tunnels");
    pointer(&mut s, PointerKind::Down, (0, 0), false);
    pointer(&mut s, PointerKind::Move, (20, 10), false);
    assert!(layer(&s, "tunnels") == before);
    assert!(s.overlays().iter().any(|o| matches!(o, Overlay::Outline { .. })));
    pointer(&mut s, PointerKind::Up, (20, 10), false);
    let after = layer(&s, "tunnels");
    assert!(after != before);
    assert_eq!(after.get(10, 5).a, 255, "the ellipse's centre");
    run(&mut s, A::Undo);
    assert!(layer(&s, "tunnels") == before, "one step");
}

#[test]
fn pointers_report_the_cursor_and_material() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    pointer(&mut s, PointerKind::Move, (60, 50), false);
    assert_eq!(s.status().cursor, Some((60, 50)));
    assert!(s.status().material.is_some());
    pointer(&mut s, PointerKind::Move, (-5, 50), false);
    assert_eq!(s.status().material, None, "outside the level");
}

/// `cargo nextest run -p olr-editor --release --run-ignored only brush_dab_speed --no-capture`.
#[test]
#[ignore]
fn brush_dab_speed() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = crate::common::copy_pack(&root, std::path::Path::new("olx-level-castlestrike"));
    let mut s = EditorState::new(olr_editor::EditorSettings::default(), crate::common::catalog(&root));
    run(&mut s, A::Open(dir));
    run(&mut s, A::AddLayer);
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::Brush(Brush { shape: BrushShape::Round, size: 16 }));
    let start = std::time::Instant::now();
    pointer(&mut s, PointerKind::Down, (300, 200), false);
    println!("one size-16 dab and its rebake on Castle Strike: {:?}", start.elapsed());
    pointer(&mut s, PointerKind::Up, (300, 200), false);
}

/// Batch C review: an action that changes the document ends the drag
/// first, so the drag's step is recorded against the document it changed.
#[test]
fn a_remove_mid_drag_records_the_move_first() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let ids = |s: &EditorState| -> Vec<String> {
        s.document().unwrap().items().iter().map(|i| i.element.id.clone()).collect()
    };
    let start = ids(&s);
    let (at, _) = image_box(&s, "tiny");
    pointer(&mut s, PointerKind::Down, (50, 40), false);
    pointer(&mut s, PointerKind::Move, (60, 50), false);
    run(&mut s, A::Remove);
    pointer(&mut s, PointerKind::Up, (60, 50), false);
    assert!(!ids(&s).contains(&"tiny".to_string()));
    run(&mut s, A::Undo);
    assert_eq!(ids(&s), start, "the removal undone");
    assert_eq!(image_box(&s, "tiny").0, (at.0 + 10, at.1 + 10));
    run(&mut s, A::Undo);
    assert_eq!(ids(&s), start);
    assert_eq!(image_box(&s, "tiny").0, at, "the move undone");
}

#[test]
fn an_undo_mid_stroke_ends_the_stroke_first() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Brush));
    let before = layer(&s, "tunnels");
    drag(&mut s, (20, 10), (30, 10), false);
    // The second stroke overlaps the first.
    pointer(&mut s, PointerKind::Down, (25, 10), false);
    pointer(&mut s, PointerKind::Move, (30, 12), false);
    run(&mut s, A::Undo);
    pointer(&mut s, PointerKind::Move, (35, 12), false);
    pointer(&mut s, PointerKind::Up, (35, 12), false);
    run(&mut s, A::Undo);
    assert!(layer(&s, "tunnels") == before, "both strokes undone, nothing comes back");
}

#[test]
fn a_tool_change_mid_drag_keeps_the_move_undoable() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let (at, _) = image_box(&s, "tiny");
    pointer(&mut s, PointerKind::Down, (50, 40), false);
    pointer(&mut s, PointerKind::Move, (60, 50), false);
    run(&mut s, A::Tool(Tool::Brush));
    assert!(s.dirty() && s.can_undo());
    run(&mut s, A::Undo);
    assert_eq!(image_box(&s, "tiny").0, at);
}
