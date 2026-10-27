//! Issue #81: the level tab shows the ground as one locked copy; Edit in
//! level paints it in the asset tab, whose edits reach the level as one
//! unsaved step, "Edit Ground", on a switch to the level tab, Play or the
//! asset tab's Save.

use std::path::Path;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_edit::element::Property;
use olr_editor::ground::{EDIT_GROUND, GROUND_COPY, GROUND_LOCKED, GROUND_WENT_BACK, LAYERS_IN_ASSET_TAB};
use olr_editor::tools::{Overlay, PointerKind, Tool};
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{DocTab, EditorAction as A, EditorCommand as C, EditorState, Selection};

use crate::common::{content, copy_pack, edit_ground, editor, editor_at, run, tree};

struct App {
    state: EditorState,
    data: EditorData,
    commands: Vec<C>,
}

fn harness(state: EditorState) -> Harness<'static, App> {
    let app = App { state, data: EditorData::default(), commands: Vec::new() };
    Harness::builder().with_size(egui::vec2(1280.0, 720.0)).with_max_steps(16).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                let c = run(&mut app.state, a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

fn click(s: &mut EditorState, world: (i32, i32)) {
    for kind in [PointerKind::Down, PointerKind::Up] {
        run(s, A::Pointer { kind, world, shift: false, toggle: false });
    }
}

fn drag(s: &mut EditorState, from: (i32, i32), to: (i32, i32)) {
    run(s, A::Pointer { kind: PointerKind::Down, world: from, shift: false, toggle: false });
    run(s, A::Pointer { kind: PointerKind::Move, world: to, shift: false, toggle: false });
    run(s, A::Pointer { kind: PointerKind::Up, world: to, shift: false, toggle: false });
}

fn ids(s: &EditorState) -> Vec<String> {
    s.document().unwrap().items().iter().map(|i| i.element.id.clone()).collect()
}

/// A brush stroke across `id` in the asset tab's ground, along row `y`.
fn paint(s: &mut EditorState, id: &str, y: i32) {
    run(s, A::Select(Selection::Element(id.into())));
    run(s, A::Tool(Tool::Brush));
    drag(s, (20, y), (60, y));
}

fn ground_selected(s: &EditorState) -> bool {
    s.selection() == &Selection::Copy(GROUND_COPY.into())
}

#[test]
fn a_plain_level_opens_as_one_locked_ground() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, Path::new("olx-level-dirt-level"));
    // A glow and clear flags made outside the editor (`Kept`).
    let fs = olr_vfs::DirFs::new(&dir);
    let mut src = olr_format::levels::read_level_files(&fs).unwrap();
    let (w, h) = src.size;
    let scale = src.art_scale;
    let mut glow = olr_format::levels::GlowLayers::empty((w * scale, h * scale), (w, h));
    glow.front.iter_mut().step_by(7).for_each(|v| *v = 200);
    src.glow = Some(glow);
    src.clear = Some((0..w * h).map(|i| u8::from(i % 3 == 0) * 255).collect());
    olr_format::levels::write_level_files(&mut olr_vfs::DirFs::new(&dir), &src).unwrap();
    let (_t, mut s) = editor_at(&root, &dir);
    assert_eq!(s.doc_tab(), DocTab::Level);
    assert_eq!(ids(&s), ["background", "dirt", "rock"], "split into its three layers");
    // One copy, selected anywhere on the level, drawn locked, without handles.
    run(&mut s, A::Tool(Tool::Select));
    click(&mut s, (w as i32 / 2, h as i32 / 2));
    assert!(ground_selected(&s));
    let overlays = s.overlays();
    let all = s.document().unwrap().all();
    assert!(overlays.contains(&Overlay::Box { rect: all, selected: true, locked: true }), "{overlays:?}");
    assert!(!overlays.iter().any(|o| matches!(o, Overlay::Handle { .. })), "no handles");
    run(&mut s, A::Remove);
    assert_eq!(s.status().message.as_deref(), Some(GROUND_LOCKED));
    assert_eq!(ids(&s), ["background", "dirt", "rock"]);
    assert!(!s.dirty() && !s.can_undo());
    // Painted in the asset tab, it keeps its glow and clear flags.
    edit_ground(&mut s);
    paint(&mut s, "dirt", 20);
    run(&mut s, A::ShowTab(DocTab::Level));
    assert!(s.dirty());
    run(&mut s, A::Save);
    let saved =
        olr_format::levels::read_level_files(&olr_vfs::DirFs::new(s.document().unwrap().dir())).unwrap();
    assert_eq!((saved.glow, saved.clear), (src.glow, src.clear));
}

#[test]
fn selecting_the_ground_offers_edit_in_level() {
    let Some(root) = content() else { return };
    assert_ne!(labels::EDIT_IN_LEVEL, labels::EDIT_LIBRARY_ASSET);
    let (_tmp, s) = editor(&root);
    let mut h = harness(s);
    h.run();
    assert!(h.query_by_label(labels::EDIT_IN_LEVEL).is_none());
    h.get_by_label(labels::COPIES);
    h.get_by_label("Ground").click();
    h.run();
    assert!(ground_selected(&h.state().state));
    // Its name and size.
    let (w, ht) = h.state().state.document().unwrap().level().size;
    assert!(h.query_all_by_label_contains(&format!("{w} × {ht}")).next().is_some(), "its size");
    h.get_by_label(labels::EDIT_IN_LEVEL).click();
    h.run();
    let s = &h.state().state;
    assert!(s.ground_open());
    assert_eq!(s.doc_tab(), DocTab::Asset);
    assert_eq!(s.asset_title(), "Ground in Composed Level");
    // Select on any pixel selects it as well.
    let mut s = editor(&root).1;
    run(&mut s, A::Tool(Tool::Select));
    click(&mut s, (3, 300));
    assert!(ground_selected(&s));
    run(&mut s, A::EditGround);
    assert!(s.ground_open());
}

#[test]
fn double_click_opens_the_ground() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Tool(Tool::Select));
    run(&mut s, A::DoubleClick { world: (100, 100) });
    assert!(s.ground_open());
    assert_eq!(s.doc_tab(), DocTab::Asset);
    // Outside the level, nothing.
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::DoubleClick { world: (-50, -50) });
    assert!(!s.ground_open());
}

#[test]
fn switching_tabs_applies_the_ground_as_one_unsaved_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let before = s.document().unwrap().items().to_vec();
    edit_ground(&mut s);
    paint(&mut s, "tunnels", 20);
    paint(&mut s, "tunnels", 40);
    let painted = s.active_document().unwrap().layer_art("tunnels").unwrap().clone();
    assert!(s.active_dirty() && !s.dirty(), "not yet in the level");
    assert_eq!(s.document().unwrap().items(), &before[..]);
    run(&mut s, A::ShowTab(DocTab::Level));
    assert!(s.dirty(), "the level unsaved");
    assert_eq!(s.document().unwrap().layer_art("tunnels").unwrap(), &painted, "the paint shows");
    assert_eq!(s.undo_name(), Some(EDIT_GROUND));
    // The asset tab keeps its own strokes: two steps there.
    run(&mut s, A::ShowTab(DocTab::Asset));
    run(&mut s, A::Undo);
    assert!(s.can_undo(), "two strokes, two steps: {:?}", s.status().message);
    run(&mut s, A::Redo);
    // Switching again with nothing new adds no step.
    run(&mut s, A::ShowTab(DocTab::Level));
    assert_eq!(s.document().unwrap().layer_art("tunnels").unwrap(), &painted);
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().items(), &before[..], "one step, undone whole");
    assert!(!s.can_undo(), "one step");
}

#[test]
fn play_applies_the_ground_first() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    edit_ground(&mut s);
    paint(&mut s, "tunnels", 20);
    let painted = s.active_document().unwrap().layer_art("tunnels").unwrap().clone();
    let out = run(&mut s, A::Play);
    assert!(out.iter().any(|c| matches!(c, C::Play { .. })), "{out:?}");
    assert_eq!(s.document().unwrap().layer_art("tunnels").unwrap(), &painted);
    assert!(s.dirty());
    run(&mut s, A::PlayEnded);
    run(&mut s, A::ShowTab(DocTab::Level));
    assert_eq!(s.undo_name(), Some(EDIT_GROUND));
}

#[test]
fn save_in_the_asset_tab_saves_the_level_too() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let dir = s.document().unwrap().dir().to_path_buf();
    let on_disk = tree(&dir);
    edit_ground(&mut s);
    paint(&mut s, "tunnels", 20);
    let out = run(&mut s, A::Save);
    assert!(out.iter().any(|c| matches!(c, C::Save { .. })), "the level saved");
    assert!(!out.iter().any(|c| matches!(c, C::SaveAsset { .. })), "not as an asset");
    assert!(!s.active_dirty() && !s.dirty(), "both unsaved marks clear");
    assert_ne!(tree(&dir), on_disk);
    // The level tab's Save saves the ground as last applied.
    paint(&mut s, "tunnels", 40);
    run(&mut s, A::ShowTab(DocTab::Level));
    let painted = s.document().unwrap().layer_art("tunnels").unwrap().clone();
    run(&mut s, A::Save);
    assert!(!s.dirty());
    let pack = olr_edit::pack::load(&dir).unwrap();
    let reopened = olr_editor::Document::open(pack, dir, s.catalog()).unwrap();
    assert_eq!(reopened.layer_art("tunnels").unwrap(), &painted);
}

#[test]
fn the_ground_draws_under_the_levels_look() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::SetLook(Some(olr_format::look::LookSource::default())));
    let level = s.document().unwrap();
    let look = level.level().look.clone();
    assert!(look.is_some());
    let tiles = (level.front_tile().clone(), level.back_tile().clone(), level.level().theme.clone());
    edit_ground(&mut s);
    let g = s.active_document().unwrap();
    assert!(g.is_ground() && !g.is_asset());
    assert_eq!(g.canvas_look(), look.as_ref(), "the level's look, not a preview look");
    assert_eq!((g.front_tile().clone(), g.back_tile().clone(), g.level().theme.clone()), tiles);
    // A look edit in the level tab shows in the asset tab.
    run(&mut s, A::ShowTab(DocTab::Level));
    let mut changed = look.clone().unwrap();
    changed.ambient.intensity = 12;
    run(&mut s, A::SetLook(Some(changed.clone())));
    run(&mut s, A::ShowTab(DocTab::Asset));
    assert_eq!(s.active_document().unwrap().canvas_look(), Some(&changed));
}

#[test]
fn the_ground_keeps_its_distant_layers_in_the_asset_tab() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    edit_ground(&mut s);
    let bytes = std::fs::read(crate::common::fixture().join("editor/images/gradient.jpg")).unwrap();
    run(&mut s, A::ImportImage { file_name: "gradient.jpg".into(), bytes });
    let Selection::Element(id) = s.selection().clone() else { panic!("the import is selected") };
    let background = olr_format::levels::ElementMaterial::Background;
    run(&mut s, A::Set(id.clone(), Property::Material(background)));
    run(&mut s, A::Set(id.clone(), Property::Distant(Some(50))));
    let distant = |d: &olr_editor::Document| d.items()[d.index_of(&id).unwrap()].element.distant;
    assert_eq!(distant(s.active_document().unwrap()), Some(50), "{:?}", s.status().message);
    run(&mut s, A::PreviewParallax(true));
    assert!(s.preview_parallax());
    run(&mut s, A::ShowTab(DocTab::Level));
    assert_eq!(distant(s.document().unwrap()), Some(50), "applied to the level");
    run(&mut s, A::ShowTab(DocTab::Asset));
    assert_eq!(distant(s.active_document().unwrap()), Some(50), "kept in the asset tab");
    assert!(s.can_undo(), "its history kept");
    // The panels offer the Distant switch and View ▾ Preview parallax.
    let mut h = harness(s);
    h.run();
    h.get_by_label(labels::DISTANT);
    h.get_by_label(labels::VIEW).click();
    h.run();
    h.get_by_label(labels::PREVIEW_PARALLAX);
}

#[test]
fn undoing_edit_ground_reopens_the_asset_tab_on_it() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let before = s.document().unwrap().layer_art("tunnels").unwrap().clone();
    edit_ground(&mut s);
    paint(&mut s, "tunnels", 20);
    run(&mut s, A::ShowTab(DocTab::Level));
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().layer_art("tunnels").unwrap(), &before);
    run(&mut s, A::ShowTab(DocTab::Asset));
    assert_eq!(s.active_document().unwrap().layer_art("tunnels").unwrap(), &before, "the undone version");
    assert_eq!(s.status().message.as_deref(), Some(GROUND_WENT_BACK));
    assert!(!s.can_undo() && !s.active_dirty(), "its own history cleared");
    // Redo brings the edits back, and the asset tab with them.
    run(&mut s, A::ShowTab(DocTab::Level));
    run(&mut s, A::Redo);
    run(&mut s, A::ShowTab(DocTab::Asset));
    assert_ne!(s.active_document().unwrap().layer_art("tunnels").unwrap(), &before);
}

#[test]
fn paint_tools_are_absent_in_the_level_tab() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let offered: Vec<Tool> = Tool::ALL.into_iter().filter(|t| s.offers(*t)).collect();
    assert!(
        offered.len() == 3 && [Tool::Select, Tool::Haze, Tool::Spawn].iter().all(|t| offered.contains(t))
    );
    for tool in [Tool::Brush, Tool::Eraser, Tool::Glow, Tool::Surface, Tool::SelectPixels, Tool::Fill] {
        run(&mut s, A::Tool(tool));
        assert_ne!(s.tool(), tool, "{} is the asset tab's", tool.name());
    }
    let items = ids(&s);
    for refused in [
        A::AddLayer,
        A::Select(Selection::Element("tunnels".into())),
        A::ImportImage { file_name: "g.png".into(), bytes: Vec::new() },
    ] {
        run(&mut s, refused);
        assert_eq!(s.status().message.as_deref(), Some(LAYERS_IN_ASSET_TAB));
    }
    assert_eq!(ids(&s), items);
    assert!(!s.dirty());
    // The rail shows them not.
    let mut h = harness(s);
    h.run();
    for absent in
        [labels::DRAW, labels::PIXELS, labels::TOOL_GLOW, labels::TOOL_SURFACE, labels::IMPORT_IMAGE]
    {
        assert!(h.query_by_label(absent).is_none(), "{absent}");
    }
    for present in ["Select", labels::TOOL_HAZE, "Spawn"] {
        h.get_by_label(present);
    }
}

#[test]
fn resizing_the_level_grows_the_ground() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    edit_ground(&mut s);
    run(&mut s, A::ShowTab(DocTab::Level));
    let black = olr_data::image::Rgba::BLACK;
    let material = olr_data::level::Material::Rock;
    run(&mut s, A::Resize { size: (600, 400), offset: (0, 0), material, colour: black });
    assert_eq!(s.document().unwrap().level().size, (600, 400));
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().level().size, (504, 350), "one step");
    assert!(!s.can_undo());
    run(&mut s, A::Redo);
    run(&mut s, A::ShowTab(DocTab::Asset));
    let g = s.active_document().unwrap();
    assert_eq!(g.level().size, (600, 400), "the ground grew");
    assert_eq!(g.all(), s.document().unwrap().all());
}

/// Every committed Reloaded level opened and saved elsewhere, its ground
/// visited in the asset tab: its files as committed.
#[test]
fn every_shipped_level_saves_byte_identical() {
    let Some(root) = content() else { return };
    let reloaded = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/reloaded");
    let mut levels: Vec<_> = std::fs::read_dir(&reloaded)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("olr-level-"))
        .collect();
    levels.sort();
    assert!(!levels.is_empty());
    for level in levels {
        let (_tmp, mut s) = editor_at(&root, &level);
        let dir = s.document().unwrap().dir().to_path_buf();
        edit_ground(&mut s);
        run(&mut s, A::ShowTab(DocTab::Level));
        assert!(!s.dirty(), "{}: unchanged", level.display());
        let out_dir = tempfile::tempdir().unwrap();
        let to = out_dir.path().join(dir.file_name().unwrap());
        std::fs::create_dir_all(&to).unwrap();
        let pack = s.document().unwrap().to_pack();
        olr_edit::pack::save(&to, &pack).unwrap();
        let (want, got) = (tree(&level), tree(&to));
        assert_eq!(want.keys().collect::<Vec<_>>(), got.keys().collect::<Vec<_>>(), "{}", level.display());
        for (name, bytes) in &want {
            assert!(&got[name] == bytes, "{}: {name} differs", level.display());
        }
    }
}
