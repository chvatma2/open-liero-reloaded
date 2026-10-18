//! The editor's panels driven by label (`egui_kittest`, plan 14b Task 8):
//! the flows of spec §8, headless, with the test host carrying out the
//! commands each frame.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_editor::tools::Tool;
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{EditorAction as A, EditorCommand as C, EditorState, Selection};
use olr_format::levels::ElementKind;

use crate::common::{content, editor, run};

/// The editor and what it asked the host to do.
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

/// The fixture open in a harness, run until it settles.
fn opened() -> Option<(tempfile::TempDir, Harness<'static, App>)> {
    let root = content()?;
    let (tmp, state) = editor(&root);
    let mut h = harness(state);
    h.run();
    Some((tmp, h))
}

fn act(h: &mut Harness<'static, App>, action: A) {
    let app = h.state_mut();
    let c = run(&mut app.state, action);
    app.commands.extend(c);
    h.run();
}

fn pixel_size(h: &Harness<'static, App>, id: &str) -> u32 {
    let d = h.state().state.document().unwrap();
    match &d.items()[d.index_of(id).unwrap()].element.kind {
        ElementKind::Image { pixel_size, .. } => *pixel_size,
        _ => panic!("an image"),
    }
}

fn ids(h: &Harness<'static, App>) -> Vec<String> {
    h.state().state.document().unwrap().items().iter().map(|i| i.element.id.clone()).collect()
}

/// A point of the canvas, `d` points from its centre.
fn canvas_point(h: &Harness<'static, App>, d: (f32, f32)) -> egui::Pos2 {
    let rect = h.state().data.canvas_rect().expect("the canvas was drawn");
    rect.center() + egui::vec2(d.0, d.1)
}

fn press(h: &mut Harness<'static, App>, pos: egui::Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

fn hover(h: &mut Harness<'static, App>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.run();
}

#[test]
fn import_change_undo_redo() {
    let Some((_tmp, mut h)) = opened() else { return };
    let bytes = std::fs::read(crate::common::fixture().join("editor/images/gradient.jpg")).unwrap();
    act(&mut h, A::ImportImage { file_name: "gradient.jpg".into(), bytes });
    h.get_by_label("gradient");
    let Selection::Element(id) = h.state().state.selection().clone() else {
        panic!("the import is selected")
    };
    h.get_by_role_and_label(Role::SpinButton, labels::PIXEL_SIZE).focus();
    h.run();
    h.get_by_label(labels::PIXEL_SIZE).type_text("4");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(pixel_size(&h, &id), 4);
    h.get_by_label(labels::UNDO).click();
    h.run();
    assert_eq!(pixel_size(&h, &id), 1);
    h.get_by_label(labels::REDO).click();
    h.run();
    assert_eq!(pixel_size(&h, &id), 4);
}

#[test]
fn paint_with_the_brush() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label("Tunnels").click();
    h.run();
    assert_eq!(h.state().state.selection(), &Selection::Element("tunnels".into()));
    h.key_press(egui::Key::B);
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Brush);
    let before = h.state().state.document().unwrap().layer_art("tunnels").unwrap().clone();
    let start = canvas_point(&h, (-40.0, 0.0));
    hover(&mut h, start);
    press(&mut h, start, true);
    for dx in [-20.0, 0.0, 20.0, 40.0] {
        let p = canvas_point(&h, (dx, 5.0));
        hover(&mut h, p);
    }
    let end = canvas_point(&h, (40.0, 5.0));
    press(&mut h, end, false);
    let after = h.state().state.document().unwrap().layer_art("tunnels").unwrap().clone();
    assert!(after != before, "the stroke painted");
    h.get_by_label(labels::UNDO).click();
    h.run();
    assert!(*h.state().state.document().unwrap().layer_art("tunnels").unwrap() == before, "one undo");
}

#[test]
fn place_a_spawn() {
    let Some((_tmp, mut h)) = opened() else { return };
    let before = h.query_all_by_label_contains(" at (").count();
    h.key_press(egui::Key::P);
    h.run();
    let p = canvas_point(&h, (10.0, 10.0));
    hover(&mut h, p);
    press(&mut h, p, true);
    press(&mut h, p, false);
    h.get_by_label(labels::OBJECTS);
    assert_eq!(h.query_all_by_label_contains(" at (").count(), before + 1);
}

/// Drags the list row labelled `from` onto the row labelled `onto`.
fn drag_row(h: &mut Harness<'static, App>, from: &str, onto: &str) {
    let a = h.get_by_label(from).rect().center();
    let b = h.get_by_label(onto).rect().center();
    hover(h, a);
    press(h, a, true);
    for t in [0.25, 0.5, 0.75, 1.0] {
        hover(h, a + (b - a) * t);
    }
    press(h, b, false);
}

#[test]
fn reorder_by_drag() {
    let Some((_tmp, mut h)) = opened() else { return };
    assert_eq!(ids(&h), ["base", "tiny", "gradient", "badge", "tunnels"]);
    drag_row(&mut h, "Badge", "Gradient");
    assert_eq!(ids(&h), ["base", "tiny", "badge", "gradient", "tunnels"]);
    act(&mut h, A::Set("tiny".into(), olr_edit::element::Property::Locked(true)));
    drag_row(&mut h, "Tiny", "Tunnels");
    assert_eq!(
        ids(&h),
        ["base", "badge", "gradient", "tunnels", "tiny"],
        "a locked element still moves (R4)"
    );
}

#[test]
fn save_then_check_the_files() {
    let Some((_tmp, mut h)) = opened() else { return };
    let dir = h.state().state.document().unwrap().dir().to_path_buf();
    h.get_by_label(labels::ADD_LAYER).click();
    h.run();
    assert!(h.state().state.dirty());
    h.get_by_label(labels::SAVE).click();
    h.run();
    assert!(!h.state().state.dirty());
    let ron = std::fs::read_to_string(dir.join("editor/composition.ron")).unwrap();
    assert!(ron.contains("id: \"layer\""), "{ron}");
    let errors: Vec<_> = olr_pack::validate(&olr_vfs::DirFs::new(&dir))
        .into_iter()
        .filter(|d| d.severity == olr_pack::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    use egui_kittest::kittest::NodeT;
    assert!(!h.get_by_label(labels::SAVE).accesskit_node().is_disabled());
}

#[test]
fn close_with_unsaved_changes() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label(labels::ADD_LAYER).click();
    h.run();
    act(&mut h, A::Close);
    h.get_by_label(labels::DONT_SAVE).click();
    h.run();
    assert!(h.state().commands.contains(&C::Close));
}

#[test]
fn shortcuts_select_tools() {
    let Some((_tmp, mut h)) = opened() else { return };
    for tool in Tool::ALL {
        let key = egui::Key::from_name(&tool.key().to_string()).unwrap();
        h.key_press(key);
        h.run();
        assert_eq!(h.state().state.tool(), tool, "{}", tool.key());
    }
    h.get_by_label(labels::ADD_LAYER).click();
    h.run();
    assert_eq!(ids(&h).len(), 6);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert_eq!(ids(&h).len(), 5, "Ctrl+Z undoes");
}

/// Final review: a mouse release during a Space pan still ends the stroke;
/// hovering afterwards paints nothing.
#[test]
fn a_release_while_panning_ends_the_stroke() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Element("tunnels".into())));
    act(&mut h, A::Tool(Tool::Brush));
    let p = canvas_point(&h, (0.0, 0.0));
    hover(&mut h, p);
    press(&mut h, p, true);
    h.key_down(egui::Key::Space);
    h.run();
    press(&mut h, p, false);
    h.key_up(egui::Key::Space);
    h.run();
    assert!(h.state().state.can_undo(), "the stroke was recorded on its release");
    let after_stroke = h.state().state.document().unwrap().layer_art("tunnels").unwrap().clone();
    let q = canvas_point(&h, (60.0, 30.0));
    hover(&mut h, q);
    assert!(
        *h.state().state.document().unwrap().layer_art("tunnels").unwrap() == after_stroke,
        "a hover paints nothing"
    );
}

/// Final review: a field being typed into commits before the toolbar's
/// click that ended it.
#[test]
fn a_typed_value_commits_before_the_toolbar_click() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Element("gradient".into())));
    act(&mut h, A::Nudge { dx: 1, dy: 0 });
    let at = |h: &Harness<'static, App>| {
        let d = h.state().state.document().unwrap();
        match &d.items()[d.index_of("gradient").unwrap()].element.kind {
            ElementKind::Image { at, .. } => *at,
            _ => panic!(),
        }
    };
    let nudged = at(&h);
    let before = pixel_size(&h, "gradient");
    h.get_by_role_and_label(Role::SpinButton, labels::PIXEL_SIZE).focus();
    h.run();
    h.get_by_label(labels::PIXEL_SIZE).type_text("4");
    h.run();
    h.get_by_label(labels::UNDO).click();
    h.run();
    assert_eq!(pixel_size(&h, "gradient"), before, "the typed value is what Undo undid");
    assert_eq!(at(&h), nudged, "the nudge stays");
}

/// Final review: a value draft whose field is no longer drawn is dropped,
/// never committed later to the element showing that field again.
#[test]
fn a_hidden_fields_draft_is_dropped() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Element("gradient".into())));
    let x = h.get_by_role_and_label(Role::SpinButton, "X").rect().center();
    hover(&mut h, x);
    press(&mut h, x, true);
    for d in [10.0, 30.0, 60.0] {
        hover(&mut h, x + egui::vec2(d, 0.0));
    }
    act(&mut h, A::Select(Selection::None));
    press(&mut h, x + egui::vec2(60.0, 0.0), false);
    let can_undo = h.state().state.can_undo();
    act(&mut h, A::Select(Selection::Element("gradient".into())));
    h.run();
    assert_eq!(h.state().state.can_undo(), can_undo, "nothing committed on reselecting");
}

/// Final review: shortcuts wait while a widget is dragged.
#[test]
fn delete_waits_while_a_field_is_dragged() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Element("gradient".into())));
    let x = h.get_by_role_and_label(Role::SpinButton, "X").rect().center();
    hover(&mut h, x);
    press(&mut h, x, true);
    hover(&mut h, x + egui::vec2(20.0, 0.0));
    h.key_press(egui::Key::Delete);
    h.run();
    press(&mut h, x + egui::vec2(20.0, 0.0), false);
    assert!(h.state().state.document().unwrap().index_of("gradient").is_some());
}

/// The resize fields follow the level's size (after a resize elsewhere, or
/// an undo): Resize with nothing changed changes nothing.
#[test]
fn the_resize_fields_follow_the_level() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::None));
    let black = olr_data::image::Rgba::BLACK;
    act(
        &mut h,
        A::Resize {
            size: (600, 400),
            offset: (0, 0),
            material: olr_data::level::Material::Rock,
            colour: black,
        },
    );
    h.get_by_label("Resize").click();
    h.run();
    assert_eq!(h.state().state.document().unwrap().level().size, (600, 400));
}

/// Each wheel event zooms one step, however many arrive in a frame.
#[test]
fn each_wheel_event_zooms_one_step() {
    let Some((_tmp, mut h)) = opened() else { return };
    let p = canvas_point(&h, (0.0, 0.0));
    hover(&mut h, p);
    let zoom = h.state().state.view().zoom;
    // Both in one frame (`Harness::event` would run a frame per event).
    for _ in 0..2 {
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
    assert_eq!(h.state().state.view().zoom, zoom + 2);
}

/// Escape cancels a dialog.
#[test]
fn escape_cancels_a_dialog() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::AddLayer);
    act(&mut h, A::Close);
    assert!(h.state().state.dialog().is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().state.dialog().is_none());
    assert!(!h.state().commands.contains(&C::Close));
}

/// Escape in a text field drops what was typed.
#[test]
fn escape_reverts_a_text_field() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Element("tiny".into())));
    h.get_by_role_and_label(Role::TextInput, labels::NAME).focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, labels::NAME).type_text("zzz");
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    let d = h.state().state.document().unwrap();
    assert_eq!(d.items()[d.index_of("tiny").unwrap()].element.name, "Tiny");
}

/// A Base (raster) layer's properties offer "Split into layers": one step,
/// undone in one.
#[test]
fn a_raster_splits_by_its_button() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label("Base").click();
    h.run();
    h.get_by_label(labels::SPLIT).click();
    h.run();
    assert_eq!(ids(&h)[..3], ["background", "dirt", "rock"]);
    assert!(h.state().state.dirty());
    h.get_by_label(labels::UNDO).click();
    h.run();
    assert_eq!(ids(&h)[0], "base", "one undo puts the raster back");
}

/// The playtest: the toolbar's controls moved as things changed (the
/// unsaved mark, a longer mod name), and the mod picker and the bots sat
/// past the window's edge. Each stays inside and where it is.
#[test]
fn the_toolbar_stays_in_the_window_and_in_place() {
    let Some(root) = content() else { return };
    let names = [
        "Select",
        "Brush",
        "Eraser",
        "Rectangle",
        "Ellipse",
        "Line",
        "Spawn",
        "Eyedropper",
        "Round",
        "Square",
        "Front tile",
        "Back tile",
        labels::UNDO,
        labels::REDO,
        labels::OPEN,
        labels::SAVE,
        labels::CLOSE,
        labels::PLAY,
        "Mod",
        "Bots",
        "Material view",
        "Grid",
    ];
    for width in [1280.0, 1024.0] {
        let (_tmp, state) = editor(&root);
        let app = App { state, data: EditorData::default(), commands: Vec::new() };
        let mut h = Harness::builder().with_size(egui::vec2(width, 720.0)).build_ui_state(
            |ui, app: &mut App| {
                for a in ui::show(ui, &app.state, &mut app.data) {
                    let c = run(&mut app.state, a);
                    app.commands.extend(c);
                }
            },
            app,
        );
        h.run();
        let rects = |h: &Harness<'static, App>| -> Vec<egui::Rect> {
            names.iter().map(|n| h.get_by_label(n).rect()).collect()
        };
        let first = rects(&h);
        for (n, r) in names.iter().zip(&first) {
            assert!(r.min.x >= 0.0 && r.max.x <= width, "{n} at {r:?} leaves a {width}-point window");
        }
        act(&mut h, A::AddLayer);
        act(&mut h, A::PlayMod("olx-mw-1-0".into()));
        act(&mut h, A::Tool(Tool::Spawn));
        let brush = h.state().state.brush();
        act(&mut h, A::Brush(olr_editor::tools::Brush { size: 64, ..brush }));
        assert_eq!(rects(&h), first, "nothing moved at {width} points");
    }
}
