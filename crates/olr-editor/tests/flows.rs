//! The editor's panels driven by label (`egui_kittest`, plan 14b Task 8):
//! the flows of spec §8, headless, with the test host carrying out the
//! commands each frame.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_editor::tools::{PaintChoice, TextureRef, Tool};
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
        "Remove colour",
        "Fill",
        "Round",
        "Square",
        "Front tile",
        "Back tile",
        labels::TEXTURES,
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
        // Plan 14d R13: a tool's options never move the others.
        for tool in Tool::ALL {
            act(&mut h, A::Tool(tool));
            assert_eq!(rects(&h), first, "nothing moved at {width} points with {}", tool.name());
        }
        // Plan 14e: nor a texture or a crater chosen.
        act(&mut h, A::Paint(PaintChoice::Texture(TextureRef::Builtin(3))));
        assert_eq!(rects(&h), first, "nothing moved at {width} points with a texture");
        let brush = h.state().state.brush();
        act(
            &mut h,
            A::Brush(olr_editor::tools::Brush { shape: olr_editor::tools::BrushShape::Crater(2), ..brush }),
        );
        assert_eq!(rects(&h), first, "nothing moved at {width} points with a crater");
    }
}

/// The playtest after the two rows: Play, the mod picker and the bots sat
/// at different heights. Every control of a toolbar row shares one centre
/// line.
#[test]
fn a_toolbar_row_is_one_straight_line() {
    let Some((_tmp, mut h)) = opened() else { return };
    // The game's style (`olr_ui::theme::apply`): larger buttons than labels.
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
    let centres = |names: &[&str]| -> Vec<(String, f32)> {
        names
            .iter()
            .flat_map(|n| h.query_all_by_label(n).map(move |node| (n.to_string(), node.rect().center().y)))
            .collect()
    };
    for row in [
        &["Select", "Eyedropper", "Remove colour", "Fill"][..],
        &["Round", labels::CRATER, "Size", "Colour", "Back tile", labels::TEXTURES][..],
        &[labels::UNDO, labels::OPEN, labels::CLOSE, labels::PLAY, "Mod", "Bots", "Material view", "Grid"][..],
    ] {
        let c = centres(row);
        let first = c[0].1;
        for (name, y) in &c {
            assert!((y - first).abs() <= 1.0, "{name} centred at {y}, not {first}: {c:?}");
        }
    }
    // Plan 14d: a tool's options share the tools' row's line.
    let options = [
        (Tool::Brush, labels::PAINT_AS),
        (Tool::Eraser, labels::RESTORE),
        (Tool::Wand, labels::TOLERANCE),
        (Tool::Fill, labels::TOLERANCE),
    ];
    for (tool, label) in options {
        act(&mut h, A::Tool(tool));
        let y = |n: &str| h.get_by_label(n).rect().center().y;
        assert!((y("Select") - y(label)).abs() <= 1.0, "{label} off the tools' row's line");
    }
}

#[test]
fn merge_down_from_the_properties() {
    let Some((_tmp, mut h)) = opened() else { return };
    let before = ids(&h).len();
    act(&mut h, A::Select(Selection::Element("wall".into())));
    h.get_by_label(labels::MERGE_DOWN).click();
    h.run();
    assert_eq!(ids(&h).len(), before - 1);
}

/// Plan 14d R13: each tool shows its own options.
#[test]
fn the_tool_options_follow_the_tool() {
    let Some((_tmp, mut h)) = opened() else { return };
    let shows = |h: &Harness<'static, App>, label: &str| h.query_by_label(label).is_some();
    act(&mut h, A::Tool(Tool::Brush));
    assert!(shows(&h, labels::PAINT_AS) && !shows(&h, labels::RESTORE));
    act(&mut h, A::Tool(Tool::Eraser));
    assert!(shows(&h, labels::RESTORE) && !shows(&h, labels::PAINT_AS));
    act(&mut h, A::Tool(Tool::Wand));
    assert!(shows(&h, labels::TOLERANCE) && !shows(&h, labels::RESTORE));
    // Plan 14e minors: Fill shows "Paint as", which it lays, and its tolerance.
    act(&mut h, A::Tool(Tool::Fill));
    assert!(shows(&h, labels::TOLERANCE) && shows(&h, labels::PAINT_AS));
    act(&mut h, A::Tool(Tool::Select));
    assert!(!shows(&h, labels::TOLERANCE) && !shows(&h, labels::PAINT_AS));
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
    h.get_by_label(labels::SAVE).click();
    h.run();
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

#[test]
fn place_a_light_and_edit_it() {
    let Some((_tmp, mut h)) = opened() else { return };
    h.get_by_label(labels::TOOL_LIGHT).click();
    h.run();
    let at = canvas_point(&h, (0.0, 0.0));
    hover(&mut h, at);
    press(&mut h, at, true);
    press(&mut h, at, false);
    let doc = h.state().state.document().unwrap();
    let index = doc.level().objects.iter().position(|o| o.as_light().is_some()).expect("a light");
    assert_eq!(h.state().state.selection(), &Selection::Object(index));
    type_number(&mut h, labels::LIGHT_INTENSITY, "250");
    let doc = h.state().state.document().unwrap();
    assert_eq!(doc.level().objects[index].as_light().unwrap().1.intensity, 250);
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
