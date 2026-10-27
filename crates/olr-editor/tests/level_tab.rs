//! Issue #81: the level tab shows the ground as one locked copy; Edit in
//! level paints it in the asset tab, whose edits reach the level as one
//! unsaved step, "Edit Ground", on a switch to the level tab, Play or the
//! asset tab's Save.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_edit::element::Property;
use olr_editor::ground::{EDIT_GROUND, GROUND_COPY, GROUND_LOCKED, GROUND_WENT_BACK, LAYERS_IN_ASSET_TAB};
use olr_editor::tools::{Overlay, PointerKind, Tool};
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{DocTab, EditorAction as A, EditorCommand as C, EditorState, Selection};

use crate::common::{
    built_in_assets, catalog, content, copy_pack, edit_ground, editor, editor_at, fixture, run, tree,
};
use olr_editor::level_tab::CopyChange;

pub struct App {
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

/// The Copies list's row named `name` (issue #83: the library on the left
/// shows a thumbnail of the same name): the rightmost.
pub fn row<'a>(h: &'a Harness<'static, App>, name: &'a str) -> egui_kittest::Node<'a> {
    h.query_all_by_label(name).max_by(|a, b| a.rect().left().total_cmp(&b.rect().left())).expect(name)
}

/// The library's thumbnail named `name` (issue #83): the leftmost button.
pub fn thumbnail<'a>(h: &'a Harness<'static, App>, name: &'a str) -> egui_kittest::Node<'a> {
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, name)
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect(name)
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
    assert_eq!(s.undo_name().as_deref(), Some(EDIT_GROUND));
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
    assert_eq!(s.undo_name().as_deref(), Some(EDIT_GROUND));
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

// Issue #82: the level tab places copies.

/// The fixture with copies placed by `olr_edit::placed`, baked and saved:
/// a built-in ceiling lamp at (40, 30), a torch stretched to twice its
/// width at (200, 100), and the strip light as the level's own at (300,
/// 50).
pub fn placed_level(root: &Path) -> (tempfile::TempDir, PathBuf) {
    use olr_edit::placed::{Size, add_copy, bake_pack, resize_copy};
    use olr_format::levels::AssetOrigin;
    let (tmp, dir) = copy_pack(root, &fixture());
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    let lib = built_in_assets();
    let read = |id: &str| olr_edit::asset::read_dir(&lib.join(id)).unwrap();
    let builtin = |id: &str| AssetOrigin::Builtin(id.into());
    add_copy(&mut pack, &read("ceiling-lamp"), builtin("ceiling-lamp"), (40, 30), None, None).unwrap();
    let torch = add_copy(&mut pack, &read("torch"), builtin("torch"), (200, 100), None, None).unwrap();
    resize_copy(&mut pack, &torch.id, Size::Width(28)).unwrap();
    add_copy(&mut pack, &read("strip-light"), AssetOrigin::Own, (300, 50), None, None).unwrap();
    bake_pack(&mut pack, &catalog(root).themes[0].back_tile).unwrap();
    olr_edit::pack::save(&dir, &pack).unwrap();
    (tmp, dir)
}

fn copy_ids(s: &EditorState) -> Vec<String> {
    s.document().unwrap().copies().into_iter().map(|c| c.id).collect()
}

#[test]
fn a_placed_level_opens_whole() {
    let Some(root) = content() else { return };
    let (_t, dir) = placed_level(&root);
    let (_t2, s) = editor_at(&root, &dir);
    assert!(!s.dirty(), "{:?}", s.status().message);
    assert_eq!(copy_ids(&s), ["ground", "ceiling-lamp", "torch", "strip-light"]);
    let doc = s.document().unwrap();
    use olr_format::levels::AssetOrigin;
    assert_eq!(doc.copy_origin("torch"), Some(AssetOrigin::Builtin("torch".into())));
    assert_eq!(doc.copy_origin("strip-light"), Some(AssetOrigin::Own));
    assert_eq!(doc.copy_box("torch").map(|b| b.1), Some((28, 32)));
    assert_eq!(ids(&s), ["base", "tiny", "gradient", "badge", "tunnels", "wall"], "the ground's layers");
    // The bake is the placement's, byte for byte.
    let pack = olr_edit::pack::load(&dir).unwrap();
    let olr_edit::pack::Source::Level(stored) = &pack.source else { panic!() };
    let l = doc.level();
    assert!(l.material == stored.material, "material");
    assert!(l.front == stored.front, "front");
    assert!(l.back == stored.back, "back");
    assert_eq!(doc.placed_look(), Some(&stored.look), "the gathered look");
    assert_eq!(doc.playable().glow, stored.glow);
    // Saved unchanged, its files as they were.
    let out = tempfile::tempdir().unwrap();
    let to = out.path().join("level");
    std::fs::create_dir_all(&to).unwrap();
    olr_edit::pack::save(&to, &doc.to_pack()).unwrap();
    let (want, got) = (tree(&dir), tree(&to));
    assert_eq!(want.keys().collect::<Vec<_>>(), got.keys().collect::<Vec<_>>());
    for (name, bytes) in &want {
        assert!(&got[name] == bytes, "{name} differs");
    }
}

// Issue #82: a level of placed copies.

/// `placed_level` open, its library the built-in assets (and an empty
/// one of the user's), Select on.
pub fn placed(root: &Path) -> (tempfile::TempDir, tempfile::TempDir, EditorState) {
    let (t, dir) = placed_level(root);
    let (t2, mut s) = editor_at(root, &dir);
    let assets = crate::common::set_library(&built_in_assets(), &t2.path().join("yours"));
    run(&mut s, A::AssetsChanged(assets));
    run(&mut s, A::Tool(Tool::Select));
    (t, t2, s)
}

/// A world pixel copy `id` covers (the topmost there), its box scanned.
fn on(s: &EditorState, id: &str) -> (i32, i32) {
    let d = s.document().unwrap();
    let ((x, y), (w, h)) = d.copy_box(id).unwrap();
    (0..h as i32)
        .flat_map(|dy| (0..w as i32).map(move |dx| (x + dx, y + dy)))
        .find(|p| d.copy_at(*p).as_deref() == Some(id))
        .unwrap_or_else(|| panic!("{id} covers no pixel"))
}

fn at(s: &EditorState, id: &str) -> (i32, i32) {
    s.document().unwrap().placed_copy(id).unwrap().at
}

fn size(s: &EditorState, id: &str) -> (u32, u32) {
    s.document().unwrap().copy_box(id).unwrap().1
}

fn pointer(s: &mut EditorState, kind: PointerKind, world: (i32, i32), shift: bool, toggle: bool) -> Vec<C> {
    run(s, A::Pointer { kind, world, shift, toggle })
}

fn uploads(cmds: &[C]) -> Vec<olr_edit::compose::Rect> {
    cmds.iter().filter_map(|c| if let C::Upload(r) = c { Some(*r) } else { None }).collect()
}

/// Whether rectangle `r` holds box `b`.
fn holds(r: olr_edit::compose::Rect, ((x, y), (w, h)): ((i32, i32), (u32, u32))) -> bool {
    let (rx, ry) = (i64::from(r.x), i64::from(r.y));
    rx <= i64::from(x)
        && ry <= i64::from(y)
        && rx + i64::from(r.w) >= i64::from(x) + i64::from(w)
        && ry + i64::from(r.h) >= i64::from(y) + i64::from(h)
}

fn copy(id: &str) -> Selection {
    Selection::Copy(id.into())
}

fn message(s: &EditorState) -> String {
    s.status().message.clone().unwrap_or_default()
}

fn set(s: &mut EditorState, id: &str, change: CopyChange) {
    run(s, A::SetCopy { id: id.into(), change });
}

/// The front art's pixel at world pixel `p`'s centre.
fn front(s: &EditorState, p: (i32, i32)) -> olr_data::image::Rgba {
    let l = s.document().unwrap().level();
    let k = l.art_scale;
    l.front.get(p.0 as u32 * k + k / 2, p.1 as u32 * k + k / 2)
}

#[test]
fn place_makes_a_copy() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    let (items, before) = (ids(&s), copy_ids(&s));
    let key = olr_editor::AssetKey { builtin: true, id: "torch".into() };
    run(&mut s, A::PickAsset(Some(key.clone())));
    run(&mut s, A::PlaceAssetCentred(key.clone()));
    let after = copy_ids(&s);
    assert_eq!(after.len(), before.len() + 1, "{}", message(&s));
    let new = after.last().unwrap().clone();
    assert_eq!(s.selection(), &copy(&new), "the new copy selected, on top");
    let d = s.document().unwrap();
    assert_eq!(d.copy_origin(&new), Some(olr_format::levels::AssetOrigin::Builtin("torch".into())));
    // Its natural size, centred on the view (no canvas yet: the level's
    // centre).
    let ((x, y), (w, h)) = d.copy_box(&new).unwrap();
    assert_eq!((w, h), (14, 16));
    let (lw, lh) = d.level().size;
    assert_eq!((x + 7, y + 8), (lw as i32 / 2, lh as i32 / 2));
    assert_eq!(ids(&s), items, "the ground keeps its layers");
    // The level holds the asset once: both torches share it.
    let key_of = |s: &EditorState, id: &str| s.document().unwrap().placed_copy(id).unwrap().asset;
    assert_eq!(key_of(&s, &new), key_of(&s, "torch"));
    assert!(s.dirty());
    // One step.
    run(&mut s, A::Undo);
    assert_eq!(copy_ids(&s), before);
    assert_eq!(s.selection(), &Selection::None);
    // The asset tab places none.
    edit_ground(&mut s);
    run(&mut s, A::PlaceAsset { key, at: (0, 0) });
    assert_eq!(message(&s), olr_editor::level_tab::PLACED_IN_THE_LEVEL);
}

#[test]
fn click_selects_the_topmost_covering_copy_and_falls_through_transparent_pixels() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    // The level without the lamp, to tell its clear pixels by.
    run(&mut s, A::Select(copy("ceiling-lamp")));
    run(&mut s, A::Remove);
    let ((x, y), (w, h)) = ((40, 30), (13u32, 6u32));
    let without: Vec<_> =
        (0..h as i32).flat_map(|dy| (0..w as i32).map(move |dx| (x + dx, y + dy))).collect();
    let bare: Vec<_> = without.iter().map(|p| front(&s, *p)).collect();
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().copy_box("ceiling-lamp"), Some(((x, y), (w, h))));
    let (mut solid, mut clear) = (None, None);
    for (p, bare) in without.iter().zip(&bare) {
        match s.document().unwrap().copy_at(*p).as_deref() {
            Some("ceiling-lamp") => solid = solid.or(Some(*p)),
            Some(GROUND_COPY) => {
                // Where the click falls through, the lamp shows nothing.
                assert_eq!(front(&s, *p), *bare, "{p:?}");
                clear = clear.or(Some(*p));
            }
            other => panic!("{p:?}: {other:?}"),
        }
    }
    let (solid, clear) = (solid.expect("an opaque pixel"), clear.expect("a clear pixel"));
    run(&mut s, A::SelectParent);
    click(&mut s, clear);
    assert_eq!(s.selection(), &copy(GROUND_COPY), "a clear pixel falls through");
    click(&mut s, solid);
    assert_eq!(s.selection(), &copy("ceiling-lamp"));
    // Of two copies on a pixel, the top one.
    run(&mut s, A::Select(copy("torch")));
    run(&mut s, A::Duplicate);
    let Selection::Copy(dup) = s.selection().clone() else { panic!("the duplicate selected") };
    let p = on(&s, "torch");
    let to = at(&s, "torch");
    set(&mut s, &dup, CopyChange::At(to));
    click(&mut s, p);
    assert_eq!(s.selection(), &copy(&dup));
    // Off the level and every copy: none.
    click(&mut s, (-50, -50));
    assert_eq!(s.selection(), &Selection::None);
}

#[test]
fn ctrl_click_and_escape_select() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    let (lamp, torch) = (on(&s, "ceiling-lamp"), on(&s, "torch"));
    click(&mut s, lamp);
    for kind in [PointerKind::Down, PointerKind::Up] {
        pointer(&mut s, kind, torch, false, true);
    }
    assert_eq!(s.selection(), &Selection::Copies(vec!["ceiling-lamp".into(), "torch".into()]));
    // Both boxes drawn, no handles for two.
    let overlays = s.overlays();
    assert_eq!(overlays.iter().filter(|o| matches!(o, Overlay::Box { selected: true, .. })).count(), 2);
    assert!(!overlays.iter().any(|o| matches!(o, Overlay::Handle { .. })));
    for kind in [PointerKind::Down, PointerKind::Up] {
        pointer(&mut s, kind, lamp, false, true);
    }
    assert_eq!(s.selection(), &copy("torch"));
    assert_eq!(s.overlays().iter().filter(|o| matches!(o, Overlay::Handle { .. })).count(), 8);
    run(&mut s, A::SelectParent);
    assert_eq!(s.selection(), &Selection::None);
}

#[test]
fn dragging_moves_in_one_step_and_rebakes_both_boxes() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    let p = on(&s, "torch");
    let old = s.document().unwrap().copy_box("torch").unwrap();
    let all = s.document().unwrap().all();
    let mut cmds = pointer(&mut s, PointerKind::Down, p, false, false);
    cmds.extend(pointer(&mut s, PointerKind::Move, (p.0 + 20, p.1 + 5), false, false));
    // Live: the torch follows before the release, no step yet.
    assert_eq!(at(&s, "torch"), (220, 105));
    assert!(!s.can_undo());
    cmds.extend(pointer(&mut s, PointerKind::Move, (p.0 + 50, p.1 + 20), false, false));
    cmds.extend(pointer(&mut s, PointerKind::Up, (p.0 + 50, p.1 + 20), false, false));
    assert_eq!(at(&s, "torch"), (250, 120));
    let new = s.document().unwrap().copy_box("torch").unwrap();
    // Each step of the drag rebakes the boxes it left and reached, the
    // level no more.
    let rects: Vec<_> = uploads(&cmds).into_iter().filter(|r| r.w > 0 && r.h > 0).collect();
    let mid = ((220, 105), old.1);
    assert!(holds(rects[0], old) && holds(rects[0], mid), "{rects:?}");
    assert!(holds(rects[1], mid) && holds(rects[1], new), "{rects:?}");
    assert!(rects.iter().all(|r| r.w < all.w && r.h <= 64), "no whole-level bake for a move: {rects:?}");
    // The bake is the placement's: saved and opened again, the same.
    let front = s.document().unwrap().level().front.clone();
    run(&mut s, A::Save);
    let (_t3, again) = editor_at(&root, s.document().unwrap().dir());
    assert!(again.document().unwrap().level().front == front, "the live bake is the saved one");
    // One step.
    run(&mut s, A::Undo);
    assert_eq!(at(&s, "torch"), (200, 100));
    assert!(!s.can_undo());
}

#[test]
fn arrows_nudge_by_one() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    run(&mut s, A::Select(Selection::Copies(vec!["ceiling-lamp".into(), "torch".into()])));
    run(&mut s, A::Nudge { dx: 1, dy: 0 });
    run(&mut s, A::Nudge { dx: 0, dy: -1 });
    assert_eq!((at(&s, "ceiling-lamp"), at(&s, "torch")), ((41, 29), (201, 99)));
    run(&mut s, A::Undo);
    assert_eq!((at(&s, "ceiling-lamp"), at(&s, "torch")), ((41, 30), (201, 100)), "one step each");
    // The ground does not move.
    run(&mut s, A::Select(copy(GROUND_COPY)));
    run(&mut s, A::Nudge { dx: 1, dy: 0 });
    assert_eq!(message(&s), GROUND_LOCKED);
    assert_eq!(at(&s, GROUND_COPY), (0, 0));
}

/// Drags copy `id`'s handle `(col, row)` by `by`, Shift held for `shift`;
/// what the status line said before the release.
fn handle_drag(s: &mut EditorState, id: &str, handle: (u8, u8), by: (i32, i32), shift: bool) -> String {
    run(s, A::Select(copy(id)));
    let ((x, y), (w, h)) = s.document().unwrap().copy_box(id).unwrap();
    let p = (x + (w as i32 * i32::from(handle.0)) / 2, y + (h as i32 * i32::from(handle.1)) / 2);
    pointer(s, PointerKind::Down, p, shift, false);
    pointer(s, PointerKind::Move, (p.0 + by.0, p.1 + by.1), shift, false);
    let said = message(s);
    pointer(s, PointerKind::Up, (p.0 + by.0, p.1 + by.1), shift, false);
    said
}

#[test]
fn handles_keep_the_aspect_unless_free() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    // The torch is 14 × 16, laid 28 × 32 at (200, 100). The bottom-right
    // corner from the top-left one, the aspect kept.
    handle_drag(&mut s, "torch", (2, 2), (14, 2), false);
    assert_eq!((at(&s, "torch"), size(&s, "torch")), ((200, 100), (42, 48)));
    assert!(s.can_undo());
    run(&mut s, A::Undo);
    assert_eq!(size(&s, "torch"), (28, 32), "one step");
    // The top-left corner from the bottom-right one.
    handle_drag(&mut s, "torch", (0, 0), (-14, 0), false);
    assert_eq!((at(&s, "torch"), size(&s, "torch")), ((186, 84), (42, 48)));
    run(&mut s, A::Undo);
    // An edge from the opposite edge, the other side centred.
    handle_drag(&mut s, "torch", (2, 1), (14, 0), false);
    assert_eq!((at(&s, "torch"), size(&s, "torch")), ((200, 92), (42, 48)));
    run(&mut s, A::Undo);
    // Free: each side its own, the edge alone.
    set(&mut s, "torch", CopyChange::KeepAspect(false));
    assert!(s.document().unwrap().placed_copy("torch").unwrap().free_aspect);
    handle_drag(&mut s, "torch", (2, 1), (10, 0), false);
    assert_eq!((at(&s, "torch"), size(&s, "torch")), ((200, 100), (38, 32)));
    handle_drag(&mut s, "torch", (1, 2), (0, -12), false);
    assert_eq!(size(&s, "torch"), (38, 20));
    // Keep aspect on again: the height snaps to the width's.
    set(&mut s, "torch", CopyChange::KeepAspect(true));
    let b = size(&s, "torch");
    assert_eq!(b, (38, 43), "{b:?}");
    // Never 0 nor beyond the bound.
    handle_drag(&mut s, "torch", (2, 2), (-1000, -1000), false);
    let (w, h) = size(&s, "torch");
    assert!(w >= 1 && h >= 1, "{w} × {h}");
    handle_drag(&mut s, "torch", (2, 2), (9000, 9000), false);
    let (w, h) = size(&s, "torch");
    assert!(w <= olr_format::levels::MAX_SIDE && h <= olr_format::levels::MAX_SIDE, "{w} × {h}");
    // The copy panel's Width keeps the aspect too.
    set(&mut s, "torch", CopyChange::Width(14));
    assert_eq!(size(&s, "torch"), (14, 16));
    assert_eq!(
        s.document().unwrap().placed_copy("torch").unwrap().size,
        None,
        "the natural size stores none"
    );
}

#[test]
fn shift_snaps_to_whole_multiples() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    // 28 + 9 = 37: 2.6 × 14, the nearest whole multiple 3×.
    let said = handle_drag(&mut s, "torch", (2, 2), (9, 0), true);
    assert_eq!(said, "42 × 48, 3×");
    assert_eq!(size(&s, "torch"), (42, 48));
    // 42 - 9 = 33: 2.4 ×, 2×.
    handle_drag(&mut s, "torch", (2, 2), (-9, 0), true);
    assert_eq!(size(&s, "torch"), (28, 32));
    // Below 1× nothing snaps.
    handle_drag(&mut s, "torch", (2, 2), (-18, 0), true);
    assert_eq!(size(&s, "torch"), (10, 11));
    // Free: each moved side to its own multiple.
    set(&mut s, "torch", CopyChange::KeepAspect(false));
    set(&mut s, "torch", CopyChange::Width(28));
    set(&mut s, "torch", CopyChange::Height(32));
    let said = handle_drag(&mut s, "torch", (2, 2), (15, -9), true);
    assert_eq!(size(&s, "torch"), (42, 16), "{said}");
    assert_eq!(said, "42 × 16, 3× × 1×");
    // Unit: the arithmetic.
    use olr_editor::level_tab::copy_resized;
    let start = ((0, 0), (28, 32));
    assert_eq!(copy_resized(start, (2, 2), (6, 0), (14, 16), false, true).0, ((0, 0), (28, 32)));
    assert_eq!(copy_resized(start, (2, 2), (8, 0), (14, 16), false, true).0, ((0, 0), (42, 48)));
    assert_eq!(copy_resized(start, (0, 0), (8, 0), (14, 16), false, true).0, ((14, 16), (14, 16)));
    assert_eq!(copy_resized(start, (0, 0), (8, 0), (14, 16), false, true).1, olr_edit::placed::Size::Natural);
}

#[test]
fn a_locked_copy_does_not_move() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    set(&mut s, "torch", CopyChange::Locked(true));
    assert!(s.document().unwrap().placed_copy("torch").unwrap().locked);
    let p = on(&s, "torch");
    pointer(&mut s, PointerKind::Down, p, false, false);
    pointer(&mut s, PointerKind::Move, (p.0 + 30, p.1), false, false);
    assert_eq!(message(&s), "torch is locked");
    pointer(&mut s, PointerKind::Up, (p.0 + 30, p.1), false, false);
    assert_eq!(s.selection(), &copy("torch"), "selected");
    assert_eq!(at(&s, "torch"), (200, 100));
    // Drawn locked, without handles.
    assert!(!s.overlays().iter().any(|o| matches!(o, Overlay::Handle { .. })));
    assert!(s.overlays().iter().any(|o| matches!(o, Overlay::Box { locked: true, .. })));
    run(&mut s, A::Nudge { dx: 1, dy: 0 });
    assert_eq!(at(&s, "torch"), (200, 100));
    // The ground too.
    click(&mut s, (2, 300));
    assert_eq!(s.selection(), &copy(GROUND_COPY));
    pointer(&mut s, PointerKind::Down, (2, 300), false, false);
    pointer(&mut s, PointerKind::Move, (40, 300), false, false);
    pointer(&mut s, PointerKind::Up, (40, 300), false, false);
    assert_eq!(at(&s, GROUND_COPY), (0, 0));
}

#[test]
fn delete_removes_and_undo_restores_the_asset_too() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    let asset = s.document().unwrap().placed_copy("strip-light").unwrap().asset;
    let has = |s: &EditorState| s.document().unwrap().placement().unwrap().assets.contains_key(&asset);
    assert!(has(&s));
    run(&mut s, A::Select(copy("strip-light")));
    run(&mut s, A::Remove);
    assert_eq!(copy_ids(&s), ["ground", "ceiling-lamp", "torch"]);
    assert!(!has(&s), "its asset went with it");
    assert!(message(&s).contains("asset"), "{}", message(&s));
    assert_eq!(s.selection(), &Selection::None);
    run(&mut s, A::Undo);
    assert_eq!(copy_ids(&s), ["ground", "ceiling-lamp", "torch", "strip-light"]);
    assert!(has(&s), "and came back");
    // Two at once, one step.
    run(&mut s, A::Select(Selection::Copies(vec!["ceiling-lamp".into(), "torch".into()])));
    run(&mut s, A::Remove);
    assert_eq!(copy_ids(&s), ["ground", "strip-light"]);
    run(&mut s, A::Undo);
    assert_eq!(copy_ids(&s).len(), 4);
    // Saved after a removal and an undo, the level is as it was.
    run(&mut s, A::Save);
    let (_t3, again) = editor_at(&root, s.document().unwrap().dir());
    assert_eq!(copy_ids(&again), ["ground", "ceiling-lamp", "torch", "strip-light"]);
}

#[test]
fn copy_paste_and_duplicate_copies() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    run(&mut s, A::Select(copy("torch")));
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    let Selection::Copy(a) = s.selection().clone() else { panic!("the paste selected") };
    assert_eq!(at(&s, &a), (208, 108));
    assert_eq!(size(&s, &a), (28, 32), "its size kept");
    run(&mut s, A::Paste);
    let Selection::Copy(b) = s.selection().clone() else { panic!() };
    assert_eq!(at(&s, &b), (216, 116), "each paste further");
    // Ctrl+D: the selection duplicated (8, 8) further.
    run(&mut s, A::Duplicate);
    let Selection::Copy(c) = s.selection().clone() else { panic!() };
    assert_eq!(at(&s, &c), (224, 124));
    assert_eq!(copy_ids(&s).len(), 7);
    // Each one step.
    run(&mut s, A::Undo);
    assert_eq!(copy_ids(&s).len(), 6);
    // Several at once.
    run(&mut s, A::Select(Selection::Copies(vec!["ceiling-lamp".into(), "torch".into()])));
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    let Selection::Copies(two) = s.selection().clone() else { panic!("both pasted, selected") };
    assert_eq!(two.len(), 2);
    assert_eq!(copy_ids(&s).len(), 8);
    // The asset tab pastes no copies; the level tab no elements.
    edit_ground(&mut s);
    run(&mut s, A::Paste);
    assert_eq!(message(&s), olr_editor::level_tab::COPIES_PASTE_IN_LEVEL_TAB);
    run(&mut s, A::Select(Selection::Element("tiny".into())));
    run(&mut s, A::Copy);
    run(&mut s, A::ShowTab(DocTab::Level));
    let n = copy_ids(&s).len();
    run(&mut s, A::Paste);
    assert_eq!(message(&s), olr_editor::level_tab::ELEMENTS_PASTE_IN_ASSET_TAB);
    assert_eq!(copy_ids(&s).len(), n);
    // The ground is not copied.
    run(&mut s, A::Select(copy(GROUND_COPY)));
    run(&mut s, A::Duplicate);
    assert_eq!(message(&s), GROUND_LOCKED);
    run(&mut s, A::Copy);
    assert_eq!(message(&s), GROUND_LOCKED);
}

#[test]
fn copies_reorder_from_the_list() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    run(&mut s, A::RestackCopy { id: "ceiling-lamp".into(), to: 3 });
    assert_eq!(copy_ids(&s), ["ground", "torch", "strip-light", "ceiling-lamp"]);
    run(&mut s, A::RestackCopy { id: "strip-light".into(), to: 1 });
    assert_eq!(copy_ids(&s), ["ground", "strip-light", "torch", "ceiling-lamp"]);
    run(&mut s, A::Undo);
    assert_eq!(copy_ids(&s), ["ground", "torch", "strip-light", "ceiling-lamp"], "one step a drop");
    // Nothing goes under the ground.
    run(&mut s, A::RestackCopy { id: "torch".into(), to: 0 });
    assert_eq!(message(&s), olr_editor::copies::GROUND_AT_THE_BOTTOM);
    assert_eq!(copy_ids(&s)[0], "ground");
    // The list shows them top first.
    let mut h = harness(s);
    h.run();
    let y = |h: &Harness<'static, App>, name: &str| row(h, name).rect().top();
    assert!(y(&h, "Ceiling lamp") < y(&h, "Strip light") && y(&h, "Strip light") < y(&h, "Ground"));
}

#[test]
fn own_assets_are_marked_own_not_starred() {
    let Some(root) = content() else { return };
    let (_t, _t2, s) = placed(&root);
    let mut h = harness(s);
    h.run();
    h.get_by_label(labels::COPIES).click();
    h.run();
    assert_eq!(h.query_all_by_label(labels::OWN_MARK).count(), 2, "the ground and the strip light");
    assert_eq!(h.query_all_by_label(labels::LIBRARY_MARK).count(), 2, "the lamp and the torch");
    assert!(h.query_all_by_label_contains("★").next().is_none());
    // Its panel's button is Edit in level; a library copy's Edit library
    // asset.
    row(&h, "Strip light").click();
    h.run();
    assert_eq!(h.state().state.selection(), &copy("strip-light"));
    h.get_by_label(labels::EDIT_IN_LEVEL);
    assert!(h.query_by_label(labels::EDIT_LIBRARY_COPY).is_none());
    row(&h, "Torch").click();
    h.run();
    h.get_by_label(labels::EDIT_LIBRARY_COPY);
    h.get_by_label(labels::KEEP_ASPECT);
}

#[test]
fn resizing_the_level_moves_copies_and_grows_the_ground() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    set(&mut s, "torch", CopyChange::Locked(true));
    let (w, h) = s.document().unwrap().level().size;
    run(
        &mut s,
        A::Resize {
            size: (w + 20, h + 10),
            offset: (20, 10),
            material: olr_data::level::Material::Background,
            colour: olr_data::image::Rgba::BLACK,
        },
    );
    assert_eq!(s.document().unwrap().level().size, (w + 20, h + 10), "{}", message(&s));
    assert_eq!((at(&s, "ceiling-lamp"), at(&s, "torch")), ((60, 40), (220, 110)), "locked ones too");
    assert_eq!(s.document().unwrap().copy_box(GROUND_COPY), Some(((0, 0), (w + 20, h + 10))));
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().level().size, (w, h));
    assert_eq!((at(&s, "ceiling-lamp"), at(&s, "torch")), ((40, 30), (200, 100)), "one step");
    // Saved, it opens as it shows.
    run(&mut s, A::Redo);
    run(&mut s, A::Save);
    let (_t3, again) = editor_at(&root, s.document().unwrap().dir());
    assert_eq!(at(&again, "torch"), (220, 110));
    assert!(again.document().unwrap().level().front == s.document().unwrap().level().front);
}

#[test]
fn double_click_on_a_library_copy_opens_the_library_asset() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    let p = on(&s, "torch");
    let cmds = run(&mut s, A::DoubleClick { world: p });
    let key = olr_editor::AssetKey { builtin: true, id: "torch".into() };
    assert!(
        cmds.iter().any(|c| matches!(c, C::ReadAssetToEdit(k) if *k == key)) || s.asset_key() == Some(&key)
    );
    assert_eq!(s.doc_tab(), DocTab::Asset);
    assert_eq!(s.asset_key(), Some(&key), "as the Assets tab's Edit asset opens it");
    assert!(!s.ground_open() && !s.own_open());
    // Its panel's button does the same.
    run(&mut s, A::ShowTab(DocTab::Level));
    run(&mut s, A::EditCopy("ceiling-lamp".into()));
    assert_eq!(s.asset_key(), Some(&olr_editor::AssetKey { builtin: true, id: "ceiling-lamp".into() }));
}

#[test]
fn double_click_on_an_own_copy_edits_it_in_the_level() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s) = placed(&root);
    let p = on(&s, "strip-light");
    run(&mut s, A::DoubleClick { world: p });
    assert_eq!(s.doc_tab(), DocTab::Asset);
    assert!(s.own_open() && !s.ground_open());
    assert_eq!(s.asset_key(), None, "not the library's");
    assert!(!s.dirty());
    // An edit there reaches the level as one level step.
    let mut meta = s.active_document().unwrap().meta();
    meta.name = "Long light".into();
    run(&mut s, A::Meta(meta));
    run(&mut s, A::ShowTab(DocTab::Level));
    assert!(s.dirty());
    assert_eq!(s.document().unwrap().copy_name("strip-light").as_deref(), Some("Long light"));
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().copy_name("strip-light").as_deref(), Some("Strip light"));
    assert!(!s.can_undo(), "one step");
    // The ground's double click is Edit in level as before.
    run(&mut s, A::DoubleClick { world: (2, 300) });
    assert!(s.ground_open());
}

/// The canvas point showing world pixel `w`.
fn screen_of(h: &Harness<'static, App>, w: (i32, i32)) -> egui::Pos2 {
    let s = &h.state().state;
    let p = s.view().to_screen(w, s.document().unwrap().level().art_scale);
    h.state().data.canvas_rect().unwrap().min + egui::vec2(p[0] + 1.0, p[1] + 1.0)
}

/// A drag on the canvas from world pixel `from` to `to`.
fn canvas_drag(h: &mut Harness<'static, App>, from: (i32, i32), to: (i32, i32), modifiers: egui::Modifiers) {
    let (a, b) = (screen_of(h, from), screen_of(h, to));
    h.event(egui::Event::PointerMoved(a));
    h.run();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    };
    h.event_modifiers(button(a, true), modifiers);
    h.run();
    for k in 1..=4 {
        let t = k as f32 / 4.0;
        h.event_modifiers(egui::Event::PointerMoved(a + (b - a) * t), modifiers);
        h.run();
    }
    h.event_modifiers(button(b, false), modifiers);
    h.run();
}

#[test]
fn move_and_resize_a_copy_through_the_canvas() {
    let Some(root) = content() else { return };
    let (_t, _t2, s) = placed(&root);
    let mut h = harness(s);
    h.run();
    // Place a built-in asset from the library.
    thumbnail(&h, "Torch").click();
    h.run();
    h.get_by_role_and_label(egui::accesskit::Role::Button, labels::PLACE).click();
    h.run();
    let Selection::Copy(id) = h.state().state.selection().clone() else { panic!("the placed copy selected") };
    let start = at(&h.state().state, &id);
    assert_eq!(size(&h.state().state, &id), (14, 16));
    // Drag it by its middle.
    let p = (start.0 + 7, start.1 + 8);
    assert_eq!(h.state().state.document().unwrap().copy_at(p).as_deref(), Some(id.as_str()));
    canvas_drag(&mut h, p, (p.0 + 30, p.1 + 12), egui::Modifiers::NONE);
    let moved = at(&h.state().state, &id);
    assert_eq!(moved, (start.0 + 30, start.1 + 12));
    // Its bottom-right corner with Shift: whole multiples.
    let corner = (moved.0 + 14, moved.1 + 16);
    canvas_drag(&mut h, corner, (corner.0 + 16, corner.1 + 2), egui::Modifiers::SHIFT);
    assert_eq!(size(&h.state().state, &id), (28, 32), "{}", message(&h.state().state));
    // Keep aspect off from the copy panel, then its right edge alone.
    h.get_by_label(labels::KEEP_ASPECT).click();
    h.run();
    assert!(h.state().state.document().unwrap().placed_copy(&id).unwrap().free_aspect);
    let edge = (moved.0 + 28, moved.1 + 16);
    canvas_drag(&mut h, edge, (edge.0 + 10, edge.1), egui::Modifiers::NONE);
    assert_eq!(size(&h.state().state, &id), (38, 32));
    assert_eq!(at(&h.state().state, &id), moved);
}
