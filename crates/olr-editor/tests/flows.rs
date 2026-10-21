//! The editor's panels driven by label (`egui_kittest`, plan 14b Task 8):
//! the flows of spec §8, headless, with the test host carrying out the
//! commands each frame.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_editor::tools::{PaintChoice, TextureRef, Tool};
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{EditorAction as A, EditorCommand as C, EditorState, PanelTab, Selection};
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
    open_more(&mut h);
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
    assert_eq!(h.state().state.tab(), PanelTab::Spawns, "the tab follows the new spawn");
    assert_eq!(h.query_all_by_label_contains(" at (").count(), before + 1);
}

/// Opens the properties' "More" (folded at first).
fn open_more(h: &mut Harness<'static, App>) {
    assert!(!h.state().state.settings().more_open);
    h.get_by_label(labels::MORE).click();
    h.run();
}

/// Clicks `item` of the File menu.
fn file(h: &mut Harness<'static, App>, item: &str) {
    h.get_by_label(labels::FILE).click();
    h.run();
    h.get_by_label(item).click();
    h.run();
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
    assert_eq!(ids(&h), ["base", "tiny", "gradient", "badge", "tunnels", "wall"]);
    drag_row(&mut h, "Badge", "Gradient");
    assert_eq!(ids(&h), ["base", "tiny", "badge", "gradient", "tunnels", "wall"]);
    act(&mut h, A::Set("tiny".into(), olr_edit::element::Property::Locked(true)));
    drag_row(&mut h, "Tiny", "Tunnels");
    assert_eq!(
        ids(&h),
        ["base", "badge", "gradient", "tunnels", "tiny", "wall"],
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
    file(&mut h, labels::SAVE);
    assert!(!h.state().state.dirty());
    let ron = std::fs::read_to_string(dir.join("editor/composition.ron")).unwrap();
    assert!(ron.contains("id: \"layer\""), "{ron}");
    let errors: Vec<_> = olr_pack::validate(&olr_vfs::DirFs::new(&dir))
        .into_iter()
        .filter(|d| d.severity == olr_pack::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    h.get_by_label(labels::FILE).click();
    h.run();
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
    assert_eq!(ids(&h).len(), 7);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert_eq!(ids(&h).len(), 6, "Ctrl+Z undoes");
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
    open_more(&mut h);
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
    open_more(&mut h);
    h.get_by_label(labels::SPLIT).click();
    h.run();
    assert_eq!(ids(&h)[..3], ["background", "dirt", "rock"]);
    assert!(h.state().state.dirty());
    h.get_by_label(labels::UNDO).click();
    h.run();
    assert_eq!(ids(&h)[0], "base", "one undo puts the raster back");
}

/// The game's style (`olr_ui::theme::apply`): larger buttons than labels.
fn theme(h: &mut Harness<'static, App>) {
    h.ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
            (TextStyle::Button, FontId::proportional(16.0)),
            (TextStyle::Heading, FontId::proportional(26.0)),
        ]
        .into();
    });
    h.run();
}

/// The session bar's and the rail's controls: always shown.
const BARS: [&str; 12] = [
    labels::FILE,
    labels::UNDO,
    labels::REDO,
    labels::VIEW,
    labels::PLAY,
    labels::PLAY_OPTIONS,
    labels::IMPORT_IMAGE,
    "Select",
    labels::DRAW,
    labels::PIXELS,
    labels::TOOL_GLOW,
    "Spawn",
];

/// The tool options bar's controls for `tool` (issue #8): the brush for the
/// tools that dab with it, the paint for those that lay it, then its own.
fn options_of(tool: Tool) -> Vec<&'static str> {
    let mut v = Vec::new();
    if tool.uses_brush() {
        v.extend(["Round", "Square", labels::CRATER, "Size"]);
    }
    if tool.lays_paint() {
        v.extend([labels::COLOUR, labels::FRONT_TILE, labels::BACK_TILE, labels::TEXTURES]);
    }
    v.extend(match tool {
        Tool::Brush | Tool::Rect | Tool::Ellipse | Tool::Line => &[labels::PAINT_AS][..],
        Tool::Eraser => &[labels::RESTORE][..],
        Tool::Wand => &[labels::TOLERANCE][..],
        Tool::Fill => &[labels::PAINT_AS, labels::TOLERANCE][..],
        Tool::SelectPixels => &[labels::BY_COLOUR, labels::GLOW_SELECTION, labels::GLOW_STRENGTH_BRUSH][..],
        Tool::Glow => &[labels::BY_COLOUR, labels::GLOW_STRENGTH_BRUSH][..],
        Tool::Select | Tool::Spawn | Tool::Eyedropper => &[][..],
    });
    v
}

/// The rects of the widgets named `names` (each named once; a number
/// field's, not its label's).
fn rects(h: &Harness<'static, App>, names: &[&str]) -> Vec<egui::Rect> {
    names
        .iter()
        .map(|n| match h.query_by_role_and_label(Role::SpinButton, n) {
            Some(field) => field.rect(),
            None => h.get_by_label(n).rect(),
        })
        .collect()
}

/// The playtest: the toolbar's controls moved as things changed (the
/// unsaved mark, a longer mod name), and the mod picker and the bots sat
/// past the window's edge. Issue #8's bars and rail keep both rules: the
/// session bar and the rail never move, each tool's options sit in one
/// place whatever they show, all inside a 1024-point window under the
/// game's theme too, and the canvas stays usable there.
#[test]
fn the_toolbar_stays_in_the_window_and_in_place() {
    let Some(root) = content() else { return };
    for (width, themed) in [(1280.0, false), (1024.0, false), (1024.0, true)] {
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
        if themed {
            theme(&mut h);
        }
        h.run();
        let at = format!("at {width} points{}", if themed { ", themed" } else { "" });
        let inside = |h: &Harness<'static, App>, names: &[&str]| {
            for (n, r) in names.iter().zip(rects(h, names)) {
                assert!(r.min.x >= 0.0 && r.max.x <= width, "{n} at {r:?} leaves the window {at}");
            }
        };
        let bars = rects(&h, &BARS);
        inside(&h, &BARS);
        let canvas = h.state().data.canvas_rect().unwrap();
        assert!(canvas.width() >= 500.0 && canvas.height() >= 400.0, "a usable canvas {at}: {canvas:?}");
        // Each tool's options, where they first showed.
        let mut first = Vec::new();
        for tool in Tool::ALL {
            act(&mut h, A::Tool(tool));
            let names = options_of(tool);
            inside(&h, &names);
            first.push(rects(&h, &names));
            assert_eq!(rects(&h, &BARS), bars, "the bars moved {at} with {}", tool.name());
        }
        // Nothing moves with the unsaved mark, a mod, a size, a texture, a
        // crater, nor "By colour"'s tolerance shown.
        act(&mut h, A::AddLayer);
        act(&mut h, A::PlayMod("olx-mw-1-0".into()));
        let brush = h.state().state.brush();
        act(&mut h, A::Brush(olr_editor::tools::Brush { size: 64, ..brush }));
        act(&mut h, A::Paint(PaintChoice::Texture(TextureRef::Builtin(3))));
        let brush = h.state().state.brush();
        act(
            &mut h,
            A::Brush(olr_editor::tools::Brush { shape: olr_editor::tools::BrushShape::Crater(2), ..brush }),
        );
        act(&mut h, A::SelectByColour(true));
        act(&mut h, A::GlowByColour(true));
        for (tool, first) in Tool::ALL.into_iter().zip(&first) {
            act(&mut h, A::Tool(tool));
            assert_eq!(&rects(&h, &options_of(tool)), first, "{}'s options moved {at}", tool.name());
            assert_eq!(rects(&h, &BARS), bars, "the bars moved {at} with {}", tool.name());
        }
        // The brush sits in one place for every tool that dabs with it.
        let brushes: Vec<Vec<egui::Rect>> = Tool::ALL
            .into_iter()
            .zip(&first)
            .filter(|(t, _)| t.uses_brush())
            .map(|(_, r)| r[..4].to_vec())
            .collect();
        assert!(brushes.windows(2).all(|w| w[0] == w[1]), "the brush moved between tools {at}");
    }
}

/// The playtest after the two rows: Play, the mod picker and the bots sat
/// at different heights. Every control of a bar shares one centre line,
/// under the game's theme.
#[test]
fn a_toolbar_row_is_one_straight_line() {
    let Some((_tmp, mut h)) = opened() else { return };
    theme(&mut h);
    let line = |h: &Harness<'static, App>, names: &[&str]| {
        let c: Vec<(String, f32)> = names
            .iter()
            .flat_map(|n| h.query_all_by_label(n).map(move |node| (n.to_string(), node.rect().center().y)))
            .collect();
        let first = c[0].1;
        for (name, y) in &c {
            assert!((y - first).abs() <= 1.0, "{name} centred at {y}, not {first}: {c:?}");
        }
    };
    line(&h, &BARS[..6]);
    for tool in Tool::ALL {
        act(&mut h, A::Tool(tool));
        let names = options_of(tool);
        if !names.is_empty() {
            line(&h, &names);
        }
    }
}

/// Issue #8: a group's flyout lists its tools with their shortcuts; a pick
/// chooses the tool and closes it, and so does Escape, sparing the pixel
/// selection.
#[test]
fn choose_a_tool_from_a_flyout() {
    let Some((_tmp, mut h)) = opened() else { return };
    assert!(h.query_by_label("Line").is_none(), "no flyout at first");
    h.get_by_label(labels::DRAW).click();
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Brush, "one click: the group's last tool");
    assert!(h.query_by_label("Line").is_none(), "and no flyout");
    h.get_by_label(labels::DRAW).click();
    h.run();
    for tool in olr_editor::tools::ToolGroup::Draw.tools() {
        assert!(h.query_by_label(tool.name()).is_some(), "{} listed", tool.name());
    }
    h.get_by_label("Line").click();
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Line);
    assert!(h.query_by_label("Eraser").is_none(), "a pick closes the flyout");
    // Escape closes a flyout and leaves the pixel selection.
    act(&mut h, A::Select(Selection::Element("tunnels".into())));
    h.get_by_label(labels::PIXELS).click();
    h.run();
    for (kind, world) in
        [(olr_editor::tools::PointerKind::Down, (10, 10)), (olr_editor::tools::PointerKind::Up, (20, 20))]
    {
        act(&mut h, A::Pointer { kind, world, shift: false, toggle: false });
    }
    assert!(h.state().state.pixel_rect().is_some());
    h.get_by_label(labels::PIXELS).click();
    h.run();
    assert!(h.query_by_label("Eyedropper").is_some(), "the Pixels flyout");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Eyedropper").is_none(), "Escape closed it");
    assert!(h.state().state.pixel_rect().is_some(), "and kept the selection");
    // A stroke on the canvas closes it too, and paints.
    h.get_by_label(labels::DRAW).click();
    h.run();
    h.get_by_label(labels::DRAW).click();
    h.run();
    assert!(h.query_by_label("Eraser").is_some());
    let (p, q) = (canvas_point(&h, (0.0, 0.0)), canvas_point(&h, (30.0, 10.0)));
    hover(&mut h, p);
    press(&mut h, p, true);
    hover(&mut h, q);
    press(&mut h, q, false);
    assert!(h.query_by_label("Eraser").is_none(), "a stroke on the canvas closed it");
    assert!(h.state().state.can_undo(), "and the canvas took the line");
}

/// Issue #8: a group's button brings back the tool chosen last in it, by
/// the flyout or by its shortcut; its icon and hover follow, its label
/// never changes.
#[test]
fn a_group_button_brings_back_its_last_tool() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label(labels::DRAW).click();
    h.run();
    h.get_by_label(labels::DRAW).click();
    h.run();
    h.get_by_label("Fill").click();
    h.run();
    h.key_press(egui::Key::V);
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Select);
    h.get_by_label(labels::DRAW).click();
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Fill, "the last of Draw");
    h.key_press(egui::Key::I);
    h.run();
    h.key_press(egui::Key::E);
    h.run();
    assert_eq!(h.state().state.last_tool(olr_editor::tools::ToolGroup::Pixels), Tool::Eyedropper);
    h.get_by_label(labels::PIXELS).click();
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Eyedropper, "a shortcut counts");
    h.get_by_label(labels::DRAW).click();
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Eraser);
}

/// Issue #8: the chosen tool's button is highlighted, whichever way it was
/// chosen; no two of the rail's and the bars' widgets share a name, nor the
/// Glow brush and the element's Glow.
#[test]
fn the_rail_highlights_the_chosen_tool() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Element("tunnels".into())));
    let button = |tool: Tool| match tool.group() {
        Some(g) => g.name(),
        None => tool.name(),
    };
    for tool in Tool::ALL {
        let key = egui::Key::from_name(&tool.key().to_string()).unwrap();
        h.key_press(key);
        h.run();
        assert_eq!(h.state().state.tool(), tool);
        for name in BARS {
            let node = h.get_by_label(name);
            if let Some(selected) = node.accesskit_node().is_selected() {
                assert_eq!(selected, name == button(tool), "{name} with {}", tool.name());
            }
        }
        assert!(h.get_by_label(labels::GLOW).accesskit_node().is_selected().is_none(), "the checkbox");
    }
}

#[test]
fn merge_down_from_the_properties() {
    let Some((_tmp, mut h)) = opened() else { return };
    let before = ids(&h).len();
    act(&mut h, A::Select(Selection::Element("wall".into())));
    open_more(&mut h);
    h.get_by_label(labels::MERGE_DOWN).click();
    h.run();
    assert_eq!(ids(&h).len(), before - 1);
}

/// Plan 14d R13, issue #8: the options bar shows the chosen tool's options
/// and no other's.
#[test]
fn the_tool_options_follow_the_tool() {
    let Some((_tmp, mut h)) = opened() else { return };
    let every: std::collections::BTreeSet<&str> = Tool::ALL.into_iter().flat_map(options_of).collect();
    for tool in Tool::ALL {
        act(&mut h, A::Tool(tool));
        let shown = options_of(tool);
        for name in &every {
            let want = shown.contains(name);
            assert_eq!(h.query_all_by_label(name).next().is_some(), want, "{name} with {}", tool.name());
        }
    }
    // The bar keeps its height without options: the canvas never jumps.
    act(&mut h, A::Tool(Tool::Brush));
    let canvas = h.state().data.canvas_rect().unwrap();
    act(&mut h, A::Tool(Tool::Select));
    assert_eq!(h.state().data.canvas_rect().unwrap(), canvas);
}

/// Plan 14e: the palette offers the theme's tiles, the built-in textures
/// and the user's, each a swatch labelled by name.
#[test]
fn the_palette_offers_every_texture() {
    let Some((_tmp, mut h)) = opened() else { return };
    let mine = olr_editor::catalog::TextureEntry {
        name: "stripes".into(),
        image: olr_data::image::RgbaImage::new(8, 8),
    };
    act(&mut h, A::TexturesChanged(vec![mine]));
    // The palette shows with a tool that paints (issue #8).
    act(&mut h, A::Tool(Tool::Brush));
    h.get_by_label(labels::TEXTURES).click();
    h.run();
    for name in [
        "Dirt",
        "Rock",
        "Grass",
        "Brick",
        "Metal",
        "Wood",
        labels::THEME_FRONT,
        labels::THEME_BACK,
        "stripes",
        labels::IMPORT_TEXTURE,
    ] {
        assert!(h.query_by_label(name).is_some(), "{name} is offered");
    }
    h.get_by_label("Brick").click();
    h.run();
    assert_eq!(h.state().state.paint(), &PaintChoice::Texture(TextureRef::Builtin(3)));
    h.get_by_label(labels::TEXTURES).click();
    h.run();
    h.get_by_label("stripes").click();
    h.run();
    assert_eq!(h.state().state.paint(), &PaintChoice::Texture(TextureRef::User("stripes".into())));
    h.get_by_label(labels::TEXTURES).click();
    h.run();
    h.get_by_label(labels::THEME_BACK).click();
    h.run();
    assert_eq!(h.state().state.paint(), &PaintChoice::BackTile);
}

#[test]
fn import_texture_opens_the_browser_for_images() {
    let Some((_tmp, mut h)) = opened() else { return };
    // The palette shows with a tool that paints (issue #8).
    act(&mut h, A::Tool(Tool::Brush));
    h.get_by_label(labels::TEXTURES).click();
    h.run();
    h.get_by_label(labels::IMPORT_TEXTURE).click();
    h.run();
    let b = h.state().state.browser().expect("the browser opens");
    assert_eq!(b.purpose(), olr_editor::browser::Purpose::ImportTexture);
}

#[test]
fn the_crater_picker_lists_the_themes_holes() {
    let Some((_tmp, mut h)) = opened() else { return };
    let n = h.state().state.holes().len();
    assert!(n >= 2, "the Dirt theme has holes");
    // The palette shows with a tool that paints (issue #8).
    act(&mut h, A::Tool(Tool::Brush));
    h.get_by_label(labels::CRATER).click();
    h.run();
    for i in 1..=n {
        assert!(h.query_by_label(&format!("Crater {i}")).is_some(), "Crater {i}");
    }
    h.get_by_label("Crater 2").click();
    h.run();
    assert_eq!(h.state().state.brush().shape, olr_editor::tools::BrushShape::Crater(1));
}

/// Plan 14e R8: Shift selects the range from the single selection.
#[test]
fn shift_click_selects_a_range_in_the_list() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label("Tiny").click();
    h.run();
    h.get_by_label("Badge").click_modifiers(egui::Modifiers::SHIFT);
    h.run();
    let want = Selection::Many(vec!["tiny".into(), "gradient".into(), "badge".into()]);
    assert_eq!(h.state().state.selection(), &want);
    h.get_by_label("Gradient").click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    assert_eq!(h.state().state.selection(), &Selection::Many(vec!["tiny".into(), "badge".into()]));
}

#[test]
fn the_group_panel_shows_the_count() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Many(vec!["tiny".into(), "gradient".into()])));
    assert!(h.query_by_label("2 elements").is_some());
    // Hiding from the panel hides both, one step.
    h.get_by_label(labels::VISIBLE).click();
    h.run();
    let d = h.state().state.document().unwrap();
    for id in ["tiny", "gradient"] {
        assert!(!d.items()[d.index_of(id).unwrap()].element.visible, "{id} hidden");
    }
    assert!(h.query_by_label(labels::MERGE_DOWN).is_none() && h.query_by_label(labels::SPLIT).is_none());
}

/// Batch B review: removing one of the user's textures is reachable in the
/// palette (a context menu inside it never showed).
#[test]
fn a_users_texture_is_removed_from_the_palette() {
    let Some((_tmp, mut h)) = opened() else { return };
    let mine = olr_editor::catalog::TextureEntry {
        name: "stripes".into(),
        image: olr_data::image::RgbaImage::new(8, 8),
    };
    act(&mut h, A::TexturesChanged(vec![mine]));
    // The palette shows with a tool that paints (issue #8).
    act(&mut h, A::Tool(Tool::Brush));
    h.get_by_label(labels::TEXTURES).click();
    h.run();
    h.get_by_label("stripes").click();
    h.run();
    h.get_by_label(labels::TEXTURES).click();
    h.run();
    h.get_by_label(labels::REMOVE).click();
    h.run();
    assert!(h.state().commands.contains(&C::RemoveTexture("stripes".into())), "{:?}", h.state().commands);
}

/// Plan 14e R9: Ctrl+C and Ctrl+V as key presses, and as the window's
/// copy and paste events.
#[test]
fn ctrl_c_and_ctrl_v_copy_and_paste() {
    let Some((_tmp, mut h)) = opened() else { return };
    let n = ids(&h).len();
    act(&mut h, A::Select(Selection::Element("tiny".into())));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::C);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::V);
    h.run();
    assert_eq!(ids(&h).len(), n + 1);
    h.event(egui::Event::Copy);
    h.run();
    h.event(egui::Event::Paste(String::new()));
    h.run();
    assert_eq!(ids(&h).len(), n + 2, "one paste per event");
    h.event(egui::Event::Cut);
    h.run();
    assert_eq!(ids(&h).len(), n + 1);
}

/// Plan 14e R9: Escape drops the pixel rectangle.
#[test]
fn escape_drops_the_pixel_rectangle_by_key() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Element("tunnels".into())));
    act(&mut h, A::Tool(Tool::SelectPixels));
    for (kind, world) in
        [(olr_editor::tools::PointerKind::Down, (10, 10)), (olr_editor::tools::PointerKind::Up, (20, 20))]
    {
        act(&mut h, A::Pointer { kind, world, shift: false, toggle: false });
    }
    assert!(h.state().state.pixel_rect().is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().state.pixel_rect().is_none());
}

/// Minors: no clipboard shortcut while a text field has the keyboard; a
/// key and the window's event in one frame are one action.
#[test]
fn clipboard_shortcuts_spare_text_fields_and_fire_once() {
    let Some((_tmp, mut h)) = opened() else { return };
    let n = ids(&h).len();
    act(&mut h, A::Select(Selection::Element("tiny".into())));
    act(&mut h, A::Copy);
    h.get_by_label(labels::NAME).click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::V);
    h.event(egui::Event::Paste(String::new()));
    h.run();
    assert_eq!(ids(&h).len(), n, "the name field had the keyboard");
    h.key_press(egui::Key::Escape);
    h.run();
    act(&mut h, A::Select(Selection::Element("tiny".into())));
    // Both in one frame (kittest's queue runs a frame per event).
    let key = egui::Event::Key {
        key: egui::Key::V,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    h.input_mut().events.extend([key, egui::Event::Paste(String::new())]);
    h.run();
    assert_eq!(ids(&h).len(), n + 1, "one paste");
}

/// Minors: the theme's swatches follow a theme change.
#[test]
fn theme_swatches_follow_the_theme() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = crate::common::copy_pack(&root, &crate::common::fixture());
    let mut catalog = crate::common::catalog(&root);
    let mut other = catalog.themes[0].clone();
    other.id = "olx-theme-castle".into();
    catalog.themes.push(other);
    let mut state = EditorState::new(olr_editor::EditorSettings::default(), catalog);
    run(&mut state, A::Open(dir));
    let mut h = harness(state);
    h.run();
    // The palette shows with a tool that paints (issue #8).
    act(&mut h, A::Tool(Tool::Brush));
    h.get_by_label(labels::TEXTURES).click();
    h.run();
    let keys = |h: &Harness<'static, App>| h.state().data.swatch_keys();
    assert!(keys(&h).iter().any(|k| k == "theme/olx-theme-dirt/front"), "{:?}", keys(&h));
    let mut meta = h.state().state.document().unwrap().meta();
    meta.theme = "olx-theme-castle".into();
    act(&mut h, A::Meta(meta));
    h.get_by_label(labels::TEXTURES).click();
    h.run();
    let k = keys(&h);
    assert!(!k.iter().any(|k| k.starts_with("theme/olx-theme-dirt/")), "{k:?}");
}

/// Types `value` into the number field labelled `label` and commits it.
fn type_number(h: &mut Harness<'static, App>, label: &str, value: &str) {
    h.get_by_role_and_label(Role::SpinButton, label).focus();
    h.run();
    h.get_by_label(label).type_text(value);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
}

#[test]
fn glow_an_element_and_save() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label("Tunnels").click();
    h.run();
    h.get_by_label(labels::GLOW).click();
    h.run();
    let doc = h.state().state.document().unwrap();
    let e = &doc.items()[doc.index_of("tunnels").unwrap()].element;
    assert_eq!(e.glow.as_ref().map(|g| g.group.as_str()), Some("tunnels"));
    let dir = doc.dir().to_path_buf();
    file(&mut h, labels::SAVE);
    let ron = std::fs::read_to_string(dir.join("level.ron")).unwrap();
    assert!(ron.contains("glow_groups") && ron.contains("id: \"tunnels\""), "{ron}");
    assert!(dir.join("look/glow.png").is_file());
}

#[test]
fn paint_glow_with_the_brush() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label("Tunnels").click();
    h.run();
    h.get_by_label(labels::GLOW).click();
    h.run();
    h.get_by_label(labels::TOOL_GLOW).click();
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Glow);
    let start = canvas_point(&h, (-20.0, 0.0));
    hover(&mut h, start);
    press(&mut h, start, true);
    let end = canvas_point(&h, (20.0, 0.0));
    hover(&mut h, end);
    press(&mut h, end, false);
    assert!(h.state().state.document().unwrap().glow_mask("tunnels").is_some());
    assert!(h.state().state.can_undo());
}

/// The user's request (2026-10-19): pixels picked by colour with Select
/// pixels take the glow % from Glow selection, in Select pixels' options
/// (issue #8: a tool change drops the pixel selection).
#[test]
fn glow_pixels_picked_by_colour() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label("Tunnels").click();
    h.run();
    h.get_by_label(labels::PIXELS).click();
    h.run();
    assert_eq!(h.state().state.tool(), Tool::SelectPixels, "the Pixels button's first tool");
    h.get_by_label(labels::BY_COLOUR).click();
    h.run();
    assert!(h.state().state.select_by_colour());
    let at = canvas_point(&h, (0.0, 0.0));
    hover(&mut h, at);
    press(&mut h, at, true);
    press(&mut h, at, false);
    assert!(h.state().state.pixel_region().is_some(), "picked by colour");
    h.get_by_label(labels::GLOW_SELECTION).click();
    h.run();
    assert!(h.state().state.document().unwrap().glow_mask("tunnels").is_some());
    assert!(h.state().state.can_undo());
}

#[test]
fn edit_the_look_panel() {
    let Some((_tmp, mut h)) = opened() else { return };
    // Tall enough that the level panel's Look section is in view.
    h.set_size(egui::vec2(1280.0, 1800.0));
    act(&mut h, A::Select(Selection::None));
    h.get_by_label(labels::ADD_LOOK).click();
    h.run();
    assert!(h.state().state.document().unwrap().level().look.is_some());
    type_number(&mut h, labels::LOOK_AMBIENT, "40");
    let look = h.state().state.document().unwrap().level().look.clone().unwrap();
    assert_eq!(look.ambient.intensity, 40);
    h.get_by_label(labels::ADD_GROUP).click();
    h.run();
    let look = h.state().state.document().unwrap().level().look.clone().unwrap();
    assert_eq!(look.glow_groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["group-1"]);
    type_number(&mut h, labels::GROUP_INTENSITY, "300");
    let look = h.state().state.document().unwrap().level().look.clone().unwrap();
    assert_eq!(look.glow_groups[0].intensity, 300);
    h.get_by_label(labels::UNDO).click();
    h.run();
    let look = h.state().state.document().unwrap().level().look.clone().unwrap();
    assert_eq!(look.glow_groups[0].intensity, 100);
}

#[test]
fn toggle_views_and_scrub_the_clock() {
    let Some((_tmp, mut h)) = opened() else { return };
    assert!(h.query_by_label(labels::VIEW_LIGHT).is_none(), "under View");
    h.get_by_label(labels::VIEW).click();
    h.run();
    h.get_by_label(labels::VIEW_LIGHT).click();
    h.run();
    assert_eq!(h.state().state.view_mode(), olr_look::ViewMode::LightOnly);
    h.get_by_label(labels::VIEW_FULL).click();
    h.run();
    assert_eq!(h.state().state.view_mode(), olr_look::ViewMode::FullBright);
    h.get_by_label(labels::CLOCK_PLAY).click();
    h.run();
    assert!(h.state().state.clock_running());
    h.get_by_label(labels::CLOCK_PAUSE).click();
    h.run();
    assert!(!h.state().state.clock_running());
    type_number(&mut h, labels::CLOCK_SCRUB, "750");
    assert_eq!(h.state().state.clock_ms(), 750.0);
    assert!(!h.state().state.dirty());
}

/// Issue #8: "More" and the lighting's "Fine tuning" are folded at first,
/// open by a click, and remembered in the settings.
#[test]
fn more_and_fine_tuning_fold_and_are_remembered() {
    let Some((_tmp, mut h)) = opened() else { return };
    act(&mut h, A::Select(Selection::Element("gradient".into())));
    for shown in
        [labels::NAME, "Rock", "Dirt", "Background", labels::GLOW, labels::MAKE_PARTS_GLOW, "X", "Width"]
    {
        assert!(h.query_all_by_label(shown).next().is_some(), "{shown} shows");
    }
    for folded in
        [labels::VISIBLE, labels::LOCKED, labels::CLEAR, labels::SPLIT, labels::PIXEL_SIZE, labels::DITHER]
    {
        assert!(h.query_all_by_label(folded).next().is_none(), "{folded} is under More");
    }
    open_more(&mut h);
    assert!(h.state().state.settings().more_open);
    assert!(
        h.state().commands.iter().any(|c| matches!(c, C::SaveSettings(s) if s.more_open)),
        "saved with the settings"
    );
    for shown in [labels::VISIBLE, labels::LOCKED, labels::CLEAR, labels::SPLIT, labels::PIXEL_SIZE] {
        assert!(h.query_all_by_label(shown).next().is_some(), "{shown} shows under More");
    }
    act(&mut h, A::Select(Selection::Element("tunnels".into())));
    assert!(h.query_by_label(labels::LOCKED).is_some(), "More stays open");
    // The level's lighting: ambient and groups shown, the rest folded.
    act(&mut h, A::Select(Selection::None));
    h.get_by_label(labels::ADD_LOOK).click();
    h.run();
    assert!(
        h.query_by_label(labels::LOOK_AMBIENT).is_some() && h.query_by_label(labels::ADD_GROUP).is_some()
    );
    assert!(h.query_by_label(labels::LOOK_MAX_LIGHT).is_none(), "under Fine tuning");
    assert!(!h.state().state.settings().fine_tuning_open);
    h.get_by_label(labels::FINE_TUNING).click();
    h.run();
    assert!(h.state().state.settings().fine_tuning_open);
    for shown in [labels::LOOK_MAX_LIGHT, labels::LOOK_STEPS, labels::LOOK_SOFTNESS, labels::LOOK_TINT] {
        assert!(h.query_by_label(shown).is_some(), "{shown} under Fine tuning");
    }
}

/// Issue #8: "Make parts glow…" chooses the Glow brush with By colour on;
/// a click on the canvas lights the pixels of that colour.
#[test]
fn make_parts_glow() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label("Tunnels").click();
    h.run();
    h.get_by_label(labels::MAKE_PARTS_GLOW).click();
    h.run();
    assert_eq!(h.state().state.tool(), Tool::Glow);
    assert!(h.state().state.glow_by_colour());
    let at = canvas_point(&h, (0.0, 0.0));
    hover(&mut h, at);
    press(&mut h, at, true);
    press(&mut h, at, false);
    let d = h.state().state.document().unwrap();
    assert!(d.items()[d.index_of("tunnels").unwrap()].element.glow.is_some(), "the element glows");
    assert!(d.glow_mask("tunnels").is_some(), "in parts");
    // What the e2e script asserts.
    assert!(h.query_by_label("painted with the Glow brush (H)").is_some());
    h.get_by_label(labels::UNDO).click();
    h.run();
    let d = h.state().state.document().unwrap();
    assert!(d.items()[d.index_of("tunnels").unwrap()].element.glow.is_none(), "one step");
}

/// Issue #8: the right panel's tab follows the selection; otherwise it
/// changes by hand; Assets is a placeholder.
#[test]
fn the_tab_follows_the_selection() {
    let Some((_tmp, mut h)) = opened() else { return };
    assert_eq!(h.state().state.tab(), PanelTab::Elements);
    h.get_by_label(labels::ASSETS).click();
    h.run();
    assert!(h.query_by_label("Saved assets will appear here.").is_some());
    h.get_by_label(labels::SPAWNS).click();
    h.run();
    assert_eq!(h.state().state.tab(), PanelTab::Spawns);
    assert!(h.query_by_label("Tunnels").is_none(), "the element list is hidden");
    // An element selected on the canvas shows Elements.
    act(&mut h, A::Tool(Tool::Select));
    let d = h.state().state.document().unwrap();
    let ElementKind::Image { at, .. } = d.items()[d.index_of("gradient").unwrap()].element.kind else {
        panic!("an image")
    };
    for kind in [olr_editor::tools::PointerKind::Down, olr_editor::tools::PointerKind::Up] {
        act(&mut h, A::Pointer { kind, world: (at.0 + 1, at.1 + 1), shift: false, toggle: false });
    }
    assert_eq!(h.state().state.selection(), &Selection::Element("gradient".into()));
    assert_eq!(h.state().state.tab(), PanelTab::Elements);
    let at = canvas_point(&h, (0.0, 0.0));
    // A spawn placed shows Spawns; the selection dropped changes nothing.
    act(&mut h, A::Tool(Tool::Spawn));
    hover(&mut h, at);
    press(&mut h, at, true);
    press(&mut h, at, false);
    assert_eq!(h.state().state.tab(), PanelTab::Spawns);
    act(&mut h, A::Select(Selection::None));
    assert_eq!(h.state().state.tab(), PanelTab::Spawns);
    // An element selected otherwise (the list, a shortcut's action) shows Elements.
    act(&mut h, A::Select(Selection::Element("tiny".into())));
    assert_eq!(h.state().state.tab(), PanelTab::Elements);
}
