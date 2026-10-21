//! Issue #11: distant (parallax) layers in the editor. An imported image
//! made distant and given a depth through the properties, the order kept on
//! a reorder, a paste, a merge and a split, the hint while nothing in front
//! is see-through, and the canvas showing and hit-testing them shifted by
//! the view.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_edit::element::{DISTANT_ORDER, Property};
use olr_editor::tools::{PointerKind, Tool};
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{EditorAction as A, EditorCommand as C, EditorState, Selection};
use olr_format::levels::ElementKind;

use crate::common::{content, editor, run};

struct App {
    state: EditorState,
    data: EditorData,
    commands: Vec<C>,
}

fn harness(state: EditorState) -> Harness<'static, App> {
    let app = App { state, data: EditorData::default(), commands: Vec::new() };
    Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                let c = run(&mut app.state, a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

fn act(h: &mut Harness<'static, App>, action: A) {
    let app = h.state_mut();
    let c = run(&mut app.state, action);
    app.commands.extend(c);
    h.run();
}

fn ids(state: &EditorState) -> Vec<String> {
    state.document().unwrap().items().iter().map(|i| i.element.id.clone()).collect()
}

fn depth(state: &EditorState, id: &str) -> Option<u8> {
    let d = state.document().unwrap();
    d.items()[d.index_of(id).unwrap()].element.distant
}

/// The fixture's gradient imported again: the new element's id, selected.
fn import(state: &mut EditorState) -> String {
    let bytes = std::fs::read(crate::common::fixture().join("editor/images/gradient.jpg")).unwrap();
    run(state, A::ImportImage { file_name: "gradient.jpg".into(), bytes });
    match state.selection() {
        Selection::Element(id) => id.clone(),
        s => panic!("the import is selected, not {s:?}"),
    }
}

/// The last distant layers the editor sent.
fn sent(commands: &[C]) -> Option<&Vec<olr_data::level::DistantLayer>> {
    commands.iter().rev().find_map(|c| match c {
        C::Distant(layers) => Some(layers),
        _ => None,
    })
}

#[test]
fn make_an_imported_image_distant() {
    let Some(root) = content() else { return };
    let (_tmp, mut state) = editor(&root);
    let id = import(&mut state);
    let mut h = harness(state);
    h.run();
    // Distant is offered for a background element only.
    assert!(h.query_by_label(labels::DISTANT).is_none(), "dirt is never distant");
    h.get_by_label("Background").click();
    h.run();
    h.get_by_label(labels::DISTANT).click();
    h.run();
    let state = &h.state().state;
    assert_eq!(depth(state, &id), Some(olr_editor::ops::DEFAULT_DEPTH));
    assert_eq!(ids(state)[0], id, "moved below every other element");
    let layers = sent(&h.state().commands).expect("the distant layers sent");
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].depth, 50);
    // The raster base covers the whole back: nothing shows the layer yet.
    h.get_by_label(labels::DISTANT_HINT);
    // The depth slider: a key press is one step.
    h.get_by_role_and_label(Role::Slider, labels::DEPTH).focus();
    h.run();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(depth(&h.state().state, &id), Some(51));
    assert_eq!(sent(&h.state().commands).unwrap()[0].depth, 51, "sent again");
    // Hiding the base leaves the back see-through: the hint goes.
    act(&mut h, A::Set("base".into(), Property::Visible(false)));
    let doc = h.state().state.document().unwrap();
    assert!(doc.see_through() && doc.has_holes());
    assert!(h.query_by_label(labels::DISTANT_HINT).is_none());
    // Undone, the element is back on top and no longer distant.
    for _ in 0..3 {
        act(&mut h, A::Undo);
    }
    let state = &h.state().state;
    assert_eq!(depth(state, &id), None);
    assert_eq!(ids(state).last(), Some(&id));
    assert!(sent(&h.state().commands).unwrap().is_empty(), "no distant layer left");
}

#[test]
fn distant_layers_keep_below_the_rest() {
    let Some(root) = content() else { return };
    let (_tmp, mut state) = editor(&root);
    let sky = import(&mut state);
    run(&mut state, A::Set(sky.clone(), Property::Material(olr_format::levels::ElementMaterial::Background)));
    run(&mut state, A::Set(sky.clone(), Property::Distant(Some(80))));
    assert_eq!(ids(&state)[0], sky);
    // A reorder stays within its group: the distant layer cannot rise...
    run(&mut state, A::MoveElement { id: sky.clone(), to: 4 });
    assert_eq!(ids(&state)[0], sky);
    assert_eq!(state.status().message.as_deref(), Some(DISTANT_ORDER));
    // ...and another element dropped below it lands just above it.
    run(&mut state, A::MoveElement { id: "tunnels".into(), to: 0 });
    assert_eq!(ids(&state)[..2], [sky.clone(), "tunnels".to_string()]);
    // A pasted copy lands on top of the distant layers, distant too.
    run(&mut state, A::Select(Selection::Element(sky.clone())));
    run(&mut state, A::Copy);
    run(&mut state, A::Paste);
    let pasted = ids(&state)[1].clone();
    assert_ne!(pasted, sky);
    assert_eq!(depth(&state, &pasted), Some(80));
    assert_eq!(state.document().unwrap().level().distant.len(), 2);
    // Neither merges nor splits.
    let before = ids(&state);
    run(&mut state, A::MergeDown(pasted.clone()));
    assert_eq!(state.status().message.as_deref(), Some("a distant layer does not merge"));
    run(&mut state, A::MergeDown("tunnels".into()));
    assert_eq!(state.status().message.as_deref(), Some("a distant layer does not merge"));
    run(&mut state, A::Split(sky.clone()));
    assert_eq!(state.status().message.as_deref(), Some("a distant layer does not split"));
    assert_eq!(ids(&state), before);
    // No longer distant, it goes just above the distant layers.
    run(&mut state, A::Set(sky.clone(), Property::Distant(None)));
    assert_eq!(ids(&state)[..2], [pasted, sky]);
}

#[test]
fn the_canvas_shows_and_hits_distant_layers_where_they_draw() {
    let Some(root) = content() else { return };
    let (_tmp, mut state) = editor(&root);
    let sky = import(&mut state);
    run(&mut state, A::Set(sky.clone(), Property::Material(olr_format::levels::ElementMaterial::Background)));
    run(&mut state, A::Set(sky.clone(), Property::Distant(Some(50))));
    // Nothing else to hit.
    for id in ids(&state).into_iter().filter(|i| *i != sky) {
        run(&mut state, A::Set(id, Property::Visible(false)));
    }
    run(&mut state, A::Canvas([800.0, 600.0]));
    let doc = state.document().unwrap();
    let size = doc.level().size;
    let ElementKind::Image { at, size: b, .. } = doc.items()[0].element.kind.clone() else {
        panic!("an image")
    };
    let place = |state: &EditorState| {
        let camera = state.parallax_camera().expect("a canvas");
        let d = |c: f32, side: u32| (0.5 * (c - side as f32 / 2.0)).round() as i32;
        (at.0 + d(camera.0, size.0), at.1 + d(camera.1, size.1))
    };
    for pan in [[0.0, 0.0], [-120.0, 80.0]] {
        run(&mut state, A::Pan(pan));
        let shown = place(&state);
        let boxes: Vec<_> = state
            .overlays()
            .into_iter()
            .filter_map(|o| match o {
                olr_editor::tools::Overlay::Box { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert!(
            boxes.iter().any(|r| (r.x as i32, r.y as i32) == (shown.0.max(0), shown.1.max(0))),
            "{pan:?}: the box drawn at {shown:?}, not {boxes:?}"
        );
        // A click where it shows selects it; where it would be unshifted,
        // when that is elsewhere, nothing.
        run(&mut state, A::Select(Selection::None));
        run(&mut state, A::Tool(Tool::Select));
        let centre = (shown.0 + b.0 as i32 / 2, shown.1 + b.1 as i32 / 2);
        let click = |state: &mut EditorState, p: (i32, i32)| {
            for kind in [PointerKind::Down, PointerKind::Up] {
                run(state, A::Pointer { kind, world: p, shift: false, toggle: false });
            }
        };
        click(&mut state, centre);
        assert_eq!(state.selection(), &Selection::Element(sky.clone()), "{pan:?}");
    }
    // Moved far off its place, a click on the place hits nothing.
    run(&mut state, A::Pan([-2000.0, 0.0]));
    let shown = place(&state);
    assert!((shown.0 - at.0).abs() > b.0 as i32, "the pan moves it a box away");
    run(&mut state, A::Select(Selection::None));
    for kind in [PointerKind::Down, PointerKind::Up] {
        let world = (at.0 + 2, at.1 + 2);
        run(&mut state, A::Pointer { kind, world, shift: false, toggle: false });
    }
    assert_eq!(state.selection(), &Selection::None);
}
