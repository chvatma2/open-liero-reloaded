//! The view, the tools and the canvas interaction (plan 14b Task 7).

use olr_data::image::Rgba;
use olr_edit::element::Property;
use olr_editor::tools::{Brush, BrushShape, Overlay, PaintChoice, PointerKind, Tool};
use olr_editor::view::{View, ZOOMS};
use olr_editor::{EditorAction as A, EditorState, Selection};
use olr_format::levels::{ElementKind, LevelObject};

use crate::common::{content, editor, editor_at, run};

fn pointer(s: &mut EditorState, kind: PointerKind, world: (i32, i32), shift: bool) {
    run(s, A::Pointer { kind, world, shift, toggle: false });
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
    assert_eq!(s.paint(), &PaintChoice::Colour(Rgba::rgb(1, 2, 3)));
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

/// The view maps the largest levels both ways: 4096 world pixels at art
/// scale 8 and every zoom, with a fractional offset.
#[test]
fn view_maps_the_largest_levels() {
    for zoom in 0..ZOOMS.len() {
        for offset in [[0.0, 0.0], [-123_456.25, 98_765.5], [0.3, -0.7]] {
            let view = View { zoom, offset };
            for p in [(4095, 4095), (4095, 0), (-4096, 2048), (1, 4094)] {
                assert_eq!(view.to_world(view.to_screen(p, 8), 8), p, "zoom {zoom}, offset {offset:?}");
            }
        }
    }
}

/// The plain Dirt Level open: three layers.
fn dirt_level() -> Option<(tempfile::TempDir, EditorState)> {
    Some(editor_at(&content()?, std::path::Path::new("olx-level-dirt-level")))
}

/// A world pixel of `m` in the bake.
fn pixel_of(s: &EditorState, m: olr_data::level::Material) -> (i32, i32) {
    let l = s.document().unwrap().level();
    let i = l.material.iter().position(|c| *c == m).unwrap();
    ((i % l.size.0 as usize) as i32, (i / l.size.0 as usize) as i32)
}

fn material_at(s: &EditorState, (x, y): (i32, i32)) -> olr_data::level::Material {
    let l = s.document().unwrap().level();
    l.material[y as usize * l.size.0 as usize + x as usize]
}

fn bake(s: &EditorState) -> (Vec<olr_data::level::Material>, olr_data::image::RgbaImage) {
    let l = s.document().unwrap().level();
    (l.material.clone(), l.front.clone())
}

fn kind(s: &EditorState, id: &str) -> ElementKind {
    let d = s.document().unwrap();
    d.items()[d.index_of(id).unwrap()].element.kind.clone()
}

/// Plan 14d R6: "Paint as" Rock into the plain Dirt layer is one step,
/// its conversion to mixed included.
#[test]
fn a_stroke_of_rock_into_dirt_is_one_step() {
    use olr_data::level::Material;
    use olr_format::levels::ElementMaterial;
    let Some((_tmp, mut s)) = dirt_level() else { return };
    let start = bake(&s);
    let p = pixel_of(&s, Material::Dirt);
    run(&mut s, A::Select(Selection::Element("dirt".into())));
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::PaintMaterial(Some(ElementMaterial::Rock)));
    drag(&mut s, p, p, false);
    assert_eq!(material_at(&s, p), Material::Rock);
    assert!(matches!(kind(&s, "dirt"), ElementKind::Drawn { mixed: true, .. }));
    run(&mut s, A::Undo);
    assert!(bake(&s) == start, "one undo");
    assert!(matches!(kind(&s, "dirt"), ElementKind::Drawn { mixed: false, .. }), "plain again");
}

#[test]
fn the_eraser_masks_a_selected_image() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let start = bake(&s);
    run(&mut s, A::Select(Selection::Element("tiny".into())));
    run(&mut s, A::Tool(Tool::Eraser));
    drag(&mut s, (60, 45), (80, 55), false);
    assert!(bake(&s) != start, "the eraser masked the picture");
    assert!(matches!(kind(&s, "tiny"), ElementKind::Image { mask: true, .. }));
    run(&mut s, A::Restore(true));
    drag(&mut s, (60, 45), (80, 55), false);
    assert!(bake(&s) == start, "restoring the same path brings it back");
}

#[test]
fn the_wand_removes_a_colour() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let start = bake(&s);
    run(&mut s, A::Select(Selection::Element("gradient".into())));
    run(&mut s, A::Tool(Tool::Wand));
    run(&mut s, A::Tolerance(30));
    assert_eq!(s.tolerance(), 30);
    drag(&mut s, (340, 230), (340, 230), false);
    assert!(bake(&s) != start);
    run(&mut s, A::Undo);
    assert!(bake(&s) == start);
}

/// Plan 14d R9: a press on an unselected drawn layer only selects it; a
/// drag on the selected layer moves it, as one step.
#[test]
fn a_click_on_a_drawn_layer_selects_before_it_moves() {
    use olr_data::level::Material;
    let Some((_tmp, mut s)) = dirt_level() else { return };
    let p = pixel_of(&s, Material::Rock);
    let start = bake(&s);
    drag(&mut s, p, (p.0 + 5, p.1), false);
    assert_eq!(s.selection(), &Selection::Element("rock".into()));
    assert!(bake(&s) == start, "nothing moved");
    drag(&mut s, p, (p.0 + 5, p.1), false);
    assert!(matches!(kind(&s, "rock"), ElementKind::Drawn { at: (5, 0), .. }), "{:?}", kind(&s, "rock"));
    run(&mut s, A::Undo);
    assert!(matches!(kind(&s, "rock"), ElementKind::Drawn { at: (0, 0), size: None, .. }));
    assert!(bake(&s) == start);
}

#[test]
fn arrows_nudge_a_selected_drawn_layer() {
    let Some((_tmp, mut s)) = dirt_level() else { return };
    run(&mut s, A::Select(Selection::Element("rock".into())));
    run(&mut s, A::Nudge { dx: 1, dy: 0 });
    assert!(matches!(kind(&s, "rock"), ElementKind::Drawn { at: (1, 0), .. }));
    let overlays = s.overlays();
    assert!(overlays.iter().any(|o| matches!(o, Overlay::Box { selected: true, .. })), "its box is drawn");
}

/// Batch C review 1: a stroke that changes nothing leaves no step, nor
/// the conversion it began with.
#[test]
fn a_stroke_that_changes_nothing_leaves_no_step() {
    use olr_format::levels::ElementMaterial;
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(Selection::Element("tiny".into())));
    run(&mut s, A::Tool(Tool::Eraser));
    drag(&mut s, (400, 300), (410, 300), false);
    assert!(matches!(kind(&s, "tiny"), ElementKind::Image { mask: false, .. }), "no mask for a miss");
    run(&mut s, A::Restore(true));
    drag(&mut s, (60, 45), (70, 45), false);
    assert!(matches!(kind(&s, "tiny"), ElementKind::Image { mask: false, .. }), "nothing to restore");
    assert!(!s.can_undo() && !s.dirty());
    let Some((_tmp, mut s)) = dirt_level() else { return };
    run(&mut s, A::Select(Selection::Element("dirt".into())));
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::PaintMaterial(Some(ElementMaterial::Rock)));
    drag(&mut s, (-50, -50), (-40, -50), false);
    assert!(matches!(kind(&s, "dirt"), ElementKind::Drawn { mixed: false, .. }));
    assert!(!s.can_undo() && !s.dirty());
}

/// Batch C review 2: erasing a picture that lies off the level is still
/// one undoable step.
#[test]
fn masking_a_picture_off_the_level_is_recorded() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Set("tiny".into(), Property::At((-200, -200))));
    run(&mut s, A::Select(Selection::Element("tiny".into())));
    run(&mut s, A::Tool(Tool::Eraser));
    drag(&mut s, (-180, -180), (-170, -175), false);
    assert!(matches!(kind(&s, "tiny"), ElementKind::Image { mask: true, .. }));
    let erased = |s: &EditorState| {
        let d = s.document().unwrap();
        match &d.items()[d.index_of("tiny").unwrap()].decoded {
            olr_edit::compose::Decoded::Image { mask: Some(m), .. } => {
                m.keep.iter().filter(|k| **k == 0).count()
            }
            _ => 0,
        }
    };
    let n = erased(&s);
    assert!(n > 0);
    run(&mut s, A::Undo);
    assert!(
        matches!(kind(&s, "tiny"), ElementKind::Image { mask: false, at: (-200, -200), .. }),
        "{:?}",
        kind(&s, "tiny")
    );
    run(&mut s, A::Redo);
    assert_eq!(erased(&s), n, "the redone step holds the erased pixels");
}

/// Batch C review 4: moving a level-sized layer rebakes the level on each
/// pointer move. Run by hand:
/// `cargo nextest run -p olr-editor --release --run-ignored only layer_move_speed --no-capture`.
#[test]
#[ignore]
fn layer_move_speed() {
    let Some(root) = content() else { return };
    for id in ["olx-level-castlestrike", "olx-level-liero-factory"] {
        let (_tmp, mut s) = editor_at(&root, std::path::Path::new(id));
        run(&mut s, A::Select(Selection::Element("rock".into())));
        run(&mut s, A::Nudge { dx: 1, dy: 0 });
        let start = std::time::Instant::now();
        for i in 0..10 {
            run(&mut s, A::Nudge { dx: if i % 2 == 0 { 1 } else { -1 }, dy: 0 });
        }
        println!("{id}: a move of the rock layer: {:?}", start.elapsed() / 10);
    }
}

/// Plan 14d minors: the Wand on what it already removed changes nothing,
/// and says what it needs when nothing is selected.
#[test]
fn the_wand_changes_nothing_twice() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Tool(Tool::Wand));
    drag(&mut s, (340, 230), (340, 230), false);
    assert!(s.status().message.as_deref().is_some_and(|m| m.contains("picture")), "{:?}", s.status().message);
    run(&mut s, A::Select(Selection::Element("gradient".into())));
    drag(&mut s, (340, 230), (340, 230), false);
    run(&mut s, A::Save);
    assert!(!s.dirty());
    drag(&mut s, (340, 230), (340, 230), false);
    assert!(!s.dirty(), "a second click on the removed region is no change");
}

/// Plan 14e R3: a texture repeats from the level's art origin, wherever the
/// layer lies.
#[test]
fn a_texture_paints_in_step_with_the_world() {
    use olr_editor::tools::TextureRef;
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    let Selection::Element(id) = s.selection().clone() else { panic!("the new layer is selected") };
    run(&mut s, A::Set(id.clone(), Property::At((3, 2))));
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::Paint(PaintChoice::Texture(TextureRef::Builtin(3))));
    drag(&mut s, (50, 50), (60, 52), false);
    let art = layer(&s, &id);
    let sc = s.document().unwrap().level().art_scale;
    let tile = olr_edit::textures::builtin().remove(3).image;
    let mut painted = 0;
    for ay in 0..art.height() {
        for ax in 0..art.width() {
            if art.get(ax, ay).a != 0 {
                painted += 1;
                let want = tile.get((ax + 3 * sc) % 64, (ay + 2 * sc) % 64);
                assert_eq!(art.get(ax, ay), want, "art ({ax}, {ay})");
            }
        }
    }
    assert!(painted > 100, "{painted}");
}

#[test]
fn the_fill_tool_fills_one_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Fill));
    run(&mut s, A::Paint(PaintChoice::Colour(Rgba::rgb(1, 2, 3))));
    let before = layer(&s, "tunnels");
    let start = bake(&s);
    pointer(&mut s, PointerKind::Down, (2, 2), false);
    pointer(&mut s, PointerKind::Up, (2, 2), false);
    let after = layer(&s, "tunnels");
    assert!(after != before, "filled");
    assert!(after.as_raw().chunks(4).any(|p| p == [1, 2, 3, 255]));
    run(&mut s, A::Undo);
    assert!(layer(&s, "tunnels") == before, "one undo restores it");
    assert!(bake(&s) == start);
    assert!(!s.can_undo());
}

/// Plan 14e R4: a crater brush paints its hole's cover, centred on the click.
#[test]
fn a_crater_brush_paints_the_hole_shape() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let mask = s.catalog().themes[0].holes[0].clone();
    let (w, h) = mask.size;
    run(&mut s, A::AddLayer);
    let Selection::Element(id) = s.selection().clone() else { panic!() };
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::Brush(Brush { shape: BrushShape::Crater(0), size: w.max(h) }));
    let p = (100, 100);
    pointer(&mut s, PointerKind::Down, p, false);
    pointer(&mut s, PointerKind::Up, p, false);
    let art = layer(&s, &id);
    let sc = s.document().unwrap().level().art_scale;
    let (x0, y0) = (p.0 as u32 - w / 2, p.1 as u32 - h / 2);
    let mut painted = 0;
    for y in 0..art.height() / sc {
        for x in 0..art.width() / sc {
            let on = art.get(x * sc, y * sc).a != 0;
            let want = (x0..x0 + w).contains(&x)
                && (y0..y0 + h).contains(&y)
                && mask.cover[((y - y0) * w + (x - x0)) as usize];
            assert_eq!(on, want, "({x}, {y})");
            painted += u32::from(on);
        }
    }
    assert!(painted > 0);
}

#[test]
fn fill_on_a_picture_is_refused() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(Selection::Element("tiny".into())));
    run(&mut s, A::Tool(Tool::Fill));
    let start = bake(&s);
    pointer(&mut s, PointerKind::Down, (50, 40), false);
    pointer(&mut s, PointerKind::Up, (50, 40), false);
    assert!(
        s.status().message.as_deref().is_some_and(|m| m.contains("merge it into a layer")),
        "{:?}",
        s.status().message
    );
    assert!(bake(&s) == start && !s.can_undo());
}

fn ctrl_click(s: &mut EditorState, world: (i32, i32)) {
    run(s, A::Pointer { kind: PointerKind::Down, world, shift: false, toggle: true });
    run(s, A::Pointer { kind: PointerKind::Up, world, shift: false, toggle: true });
}

/// Plan 14e R8: Ctrl-clicks on the canvas build a group; a press on a
/// member drags the whole group, one step.
#[test]
fn ctrl_clicks_build_a_group_on_the_canvas() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Tool(Tool::Select));
    ctrl_click(&mut s, (50, 40));
    ctrl_click(&mut s, (310, 210));
    assert_eq!(s.selection(), &Selection::Many(vec!["tiny".into(), "gradient".into()]));
    let boxes = s.overlays().iter().filter(|o| matches!(o, Overlay::Box { selected: true, .. })).count();
    assert_eq!(boxes, 2, "each member's box is drawn selected");
    let (t, g) = (image_box(&s, "tiny").0, image_box(&s, "gradient").0);
    drag(&mut s, (50, 40), (53, 41), false);
    assert_eq!(image_box(&s, "tiny").0, (t.0 + 3, t.1 + 1));
    assert_eq!(image_box(&s, "gradient").0, (g.0 + 3, g.1 + 1));
    assert_eq!(s.selection(), &Selection::Many(vec!["tiny".into(), "gradient".into()]), "still the group");
    run(&mut s, A::Undo);
    assert_eq!((image_box(&s, "tiny").0, image_box(&s, "gradient").0), (t, g), "one step");
    ctrl_click(&mut s, (50, 40));
    assert_eq!(s.selection(), &Selection::Element("gradient".into()));
}

/// A covered pixel of `tunnels` with a 10 × 10 square from it inside the level.
fn tunnel_square(s: &EditorState) -> (i32, i32) {
    let art = layer(s, "tunnels");
    let sc = s.document().unwrap().level().art_scale;
    let (w, h) = (art.width() / sc, art.height() / sc);
    (0..h.saturating_sub(10))
        .flat_map(|y| (0..w.saturating_sub(10)).map(move |x| (x, y)))
        .find(|&(x, y)| art.get(x * sc, y * sc).a != 0)
        .map(|(x, y)| (x as i32, y as i32))
        .expect("tunnels has pixels")
}

/// Selects a 10 × 10 rectangle of `tunnels` from `p` with the pixel tool.
fn select_pixels(s: &mut EditorState, p: (i32, i32)) {
    run(s, A::Select(Selection::Element("tunnels".into())));
    run(s, A::Tool(Tool::SelectPixels));
    drag(s, p, (p.0 + 9, p.1 + 9), false);
    assert_eq!(
        s.pixel_rect().map(|(id, r)| (id.as_str(), *r)),
        Some(("tunnels", olr_edit::compose::Rect { x: p.0 as u32, y: p.1 as u32, w: 10, h: 10 }))
    );
}

/// Plan 14e R7: copied pixels paste as a new boxed layer, offset (4, 4).
#[test]
fn pixels_copied_paste_as_a_layer() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let p = tunnel_square(&s);
    select_pixels(&mut s, p);
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    let Selection::Element(id) = s.selection().clone() else { panic!("{:?}", s.selection()) };
    let d = s.document().unwrap();
    let e = &d.items().last().unwrap().element;
    assert_eq!((e.id.as_str(), e.name.as_str()), (id.as_str(), "Pasted"));
    let ElementKind::Drawn { at, size, .. } = e.kind else { panic!("{:?}", e.kind) };
    assert_eq!((at, size), ((p.0 + 4, p.1 + 4), Some((10, 10))));
    let sc = d.level().art_scale;
    let (src, out) = (layer(&s, "tunnels"), layer(&s, &id));
    for y in 0..10 * sc {
        for x in 0..10 * sc {
            assert_eq!(out.get(x, y), src.get(p.0 as u32 * sc + x, p.1 as u32 * sc + y), "({x}, {y})");
        }
    }
    run(&mut s, A::Undo);
    assert!(s.document().unwrap().index_of(&id).is_none(), "one step");
}

#[test]
fn cut_pixels_erases_them_in_one_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let p = tunnel_square(&s);
    let before = layer(&s, "tunnels");
    select_pixels(&mut s, p);
    run(&mut s, A::Cut);
    let sc = s.document().unwrap().level().art_scale;
    assert_eq!(layer(&s, "tunnels").get(p.0 as u32 * sc, p.1 as u32 * sc).a, 0, "cut out");
    run(&mut s, A::Paste);
    assert!(s.document().unwrap().items().last().unwrap().element.name == "Pasted");
    run(&mut s, A::Undo);
    run(&mut s, A::Undo);
    assert!(layer(&s, "tunnels") == before, "the cut is one step");
}

#[test]
fn delete_erases_the_pixel_rectangle() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let p = tunnel_square(&s);
    let n = s.document().unwrap().items().len();
    select_pixels(&mut s, p);
    run(&mut s, A::Remove);
    let sc = s.document().unwrap().level().art_scale;
    assert_eq!(layer(&s, "tunnels").get(p.0 as u32 * sc, p.1 as u32 * sc).a, 0);
    assert_eq!(s.document().unwrap().items().len(), n, "the layer stays");
}

#[test]
fn escape_drops_the_pixel_rectangle() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let p = tunnel_square(&s);
    select_pixels(&mut s, p);
    assert!(s.overlays().iter().any(|o| matches!(o, Overlay::Outline { .. })), "its outline is drawn");
    run(&mut s, A::DropPixels);
    assert!(s.pixel_rect().is_none());
    select_pixels(&mut s, p);
    run(&mut s, A::Tool(Tool::Brush));
    assert!(s.pixel_rect().is_none(), "a tool change drops it");
}

/// Batch C review: the pixel rectangle goes when the selection moves on or
/// its layer goes, so Delete deletes what is selected.
#[test]
fn the_pixel_rectangle_follows_the_selection() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let p = tunnel_square(&s);
    let before = layer(&s, "tunnels");
    select_pixels(&mut s, p);
    run(&mut s, A::Select(Selection::Element("tiny".into())));
    assert!(s.pixel_rect().is_none(), "another element chosen");
    run(&mut s, A::Remove);
    assert!(s.document().unwrap().index_of("tiny").is_none(), "Delete deleted the selected element");
    assert!(layer(&s, "tunnels") == before);
    // A rectangle on a pasted layer, the paste undone.
    run(&mut s, A::Undo);
    select_pixels(&mut s, p);
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    let Selection::Element(pasted) = s.selection().clone() else { panic!() };
    run(&mut s, A::Tool(Tool::SelectPixels));
    drag(&mut s, (p.0 + 4, p.1 + 4), (p.0 + 6, p.1 + 6), false);
    assert_eq!(s.pixel_rect().map(|r| r.0.clone()), Some(pasted));
    run(&mut s, A::Undo);
    assert!(s.pixel_rect().is_none(), "its layer is gone");
}

/// Minors: a stroke paints with the paint it began with, whatever is
/// chosen meanwhile (resolved once per stroke).
#[test]
fn a_stroke_keeps_its_paint() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    let Selection::Element(id) = s.selection().clone() else { panic!() };
    run(&mut s, A::Tool(Tool::Brush));
    let first = Rgba::rgb(10, 20, 30);
    run(&mut s, A::Paint(PaintChoice::Colour(first)));
    pointer(&mut s, PointerKind::Down, (20, 20), false);
    run(&mut s, A::Paint(PaintChoice::Colour(Rgba::rgb(200, 0, 0))));
    pointer(&mut s, PointerKind::Move, (60, 20), false);
    pointer(&mut s, PointerKind::Up, (60, 20), false);
    let sc = s.document().unwrap().level().art_scale;
    assert_eq!(layer(&s, &id).get(60 * sc, 20 * sc), first);
}

/// An editor on the fixture whose theme's only hole is a 9 × 9 square
/// covering its centre pixel alone.
fn sparse_crater_editor(root: &std::path::Path) -> (tempfile::TempDir, EditorState) {
    let (tmp, dir) = crate::common::copy_pack(root, &crate::common::fixture());
    let mut catalog = crate::common::catalog(root);
    let mut cover = vec![false; 81];
    cover[40] = true;
    catalog.themes[0].holes = vec![std::sync::Arc::new(olr_edit::level::StampMask { size: (9, 9), cover })];
    let mut s = EditorState::new(olr_editor::EditorSettings::default(), catalog);
    run(&mut s, A::Open(dir));
    (tmp, s)
}

/// Minors: a crater stroke lays a dab at least every half crater.
#[test]
fn crater_joins_are_half_a_crater_apart() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = sparse_crater_editor(&root);
    run(&mut s, A::AddLayer);
    let Selection::Element(id) = s.selection().clone() else { panic!() };
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::Brush(Brush { shape: BrushShape::Crater(0), size: 9 }));
    drag(&mut s, (100, 100), (107, 100), false);
    let art = layer(&s, &id);
    let sc = s.document().unwrap().level().art_scale;
    let xs: Vec<u32> = (90..120).filter(|x| art.get(x * sc, 100 * sc).a != 0).collect();
    assert_eq!((xs.first(), xs.last()), (Some(&100), Some(&107)));
    for w in xs.windows(2) {
        assert!(w[1] - w[0] <= 4, "dabs {xs:?} further apart than half a crater");
    }
}

/// Minors: the crater's scale is clamped to 16; a hole the theme lacks
/// dabs round.
#[test]
fn crater_scale_is_clamped_and_a_missing_hole_dabs_round() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = sparse_crater_editor(&root);
    run(&mut s, A::Tool(Tool::Brush));
    run(&mut s, A::Brush(Brush { shape: BrushShape::Crater(0), size: olr_editor::tools::MAX_BRUSH }));
    pointer(&mut s, PointerKind::Move, (50, 50), false);
    let size = |s: &EditorState| {
        s.overlays().iter().find_map(|o| match o {
            Overlay::Brush { size, .. } => Some(*size),
            _ => None,
        })
    };
    assert_eq!(size(&s), Some(9 * 16));
    run(&mut s, A::AddLayer);
    let Selection::Element(id) = s.selection().clone() else { panic!() };
    run(&mut s, A::Brush(Brush { shape: BrushShape::Crater(7), size: 5 }));
    assert_eq!(size(&s), Some(5), "a missing hole's outline is the round brush's");
    pointer(&mut s, PointerKind::Down, (50, 50), false);
    pointer(&mut s, PointerKind::Up, (50, 50), false);
    let art = layer(&s, &id);
    let sc = s.document().unwrap().level().art_scale;
    let painted = (40..60)
        .flat_map(|y| (40..60).map(move |x| (x, y)))
        .filter(|&(x, y)| art.get(x * sc, y * sc).a != 0)
        .count();
    assert_eq!(painted, 13, "a circle of radius 2");
}

/// Minors: the pixel rectangle is clipped to its layer's box; one off the
/// layer is no rectangle, and says so.
#[test]
fn the_pixel_rectangle_is_the_layers() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(Selection::Element("wall".into())));
    run(&mut s, A::Tool(Tool::SelectPixels));
    drag(&mut s, (0, 0), (9, 9), false);
    assert_eq!(s.pixel_rect().map(|r| r.1), Some(olr_edit::compose::Rect { x: 4, y: 0, w: 6, h: 10 }));
    drag(&mut s, (0, 0), (2, 2), false);
    assert!(s.pixel_rect().is_none());
    assert!(s.status().message.as_deref().is_some_and(|m| m.contains("wall")), "{:?}", s.status().message);
}

/// Minors: a raster in a group stays while the others move.
#[test]
fn a_raster_in_a_group_stays_put() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let t = image_box(&s, "tiny").0;
    run(&mut s, A::Select(Selection::Many(vec!["base".into(), "tiny".into()])));
    run(&mut s, A::Nudge { dx: 1, dy: 0 });
    assert_eq!(image_box(&s, "tiny").0, (t.0 + 1, t.1));
    assert!(matches!(s.document().unwrap().items()[0].element.kind, ElementKind::Raster));
    run(&mut s, A::Undo);
    assert_eq!(image_box(&s, "tiny").0, t);
}
