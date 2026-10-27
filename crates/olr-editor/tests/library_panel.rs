//! Issue #83: the level tab's asset library and its links with the open
//! level: placing by a drag or a pick, placing again reusing the level's
//! copy (or taking the library's newer version and offering it to the
//! older copies), library saves reaching the level in one step through
//! `CompareLibrary` (never when they break a copy, never from the user's
//! library to a built-in copy), Unlink, Save to library (and its
//! refusals), Use your copy in this level, and Search.

use std::path::{Path, PathBuf};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_edit::element::Property;
use olr_edit::library::{Libraries, Status, read_origins};
use olr_editor::ground::GROUND_COPY;
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{AssetKey, Dialog, DocTab, EditorAction as A, EditorCommand as C, EditorState, Selection};
use olr_format::assets::AssetSource;
use olr_format::levels::AssetOrigin;

use crate::common::{
    built_in_assets, catalog, content, copy_pack, editor, editor_at, fixture, run, set_library,
};
use crate::level_tab::{placed, thumbnail};

/// The editor and the host's answers, as `level_tab.rs` drives it.
pub struct App {
    pub state: EditorState,
    pub data: EditorData,
}

fn harness(state: EditorState) -> Harness<'static, App> {
    let app = App { state, data: EditorData::default() };
    Harness::builder().with_size(egui::vec2(1280.0, 720.0)).with_max_steps(16).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                run(&mut app.state, a);
            }
        },
        app,
    )
}

/// The canvas point showing world pixel `w`'s centre (at any zoom).
fn screen_of(h: &Harness<'static, App>, w: (i32, i32)) -> egui::Pos2 {
    let s = &h.state().state;
    let k = s.document().unwrap().level().art_scale;
    let (a, b) = (s.view().to_screen(w, k), s.view().to_screen((w.0 + 1, w.1 + 1), k));
    h.state().data.canvas_rect().unwrap().min + egui::vec2((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0)
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// A drag of the pointer from `from` to `to` (screen points).
pub fn pointer_drag(h: &mut Harness<'static, App>, from: egui::Pos2, to: egui::Pos2) {
    h.event(egui::Event::PointerMoved(from));
    h.run();
    h.event(button(from, true));
    h.run();
    for k in 1..=6 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
        h.run();
    }
    h.event(button(to, false));
    h.run();
}

fn builtin(id: &str) -> AssetKey {
    AssetKey { builtin: true, id: id.into() }
}

fn yours(id: &str) -> AssetKey {
    AssetKey { builtin: false, id: id.into() }
}

/// Built-in asset `id`'s files.
fn read_builtin(id: &str) -> AssetSource {
    olr_edit::asset::read_dir(&built_in_assets().join(id)).unwrap()
}

/// `src` written into the user's library `dir` as folder `folder`.
fn put(dir: &Path, folder: &str, src: &AssetSource) {
    olr_edit::asset::add_source_to_library(dir, src, Some(folder)).unwrap();
}

/// The library changed on disk told to the editor (the host's answer to a
/// change of it).
fn changed(s: &mut EditorState, dir: &Path) {
    let list = set_library(&built_in_assets(), dir);
    run(s, A::AssetsChanged(list));
}

/// The libraries' version of every origin the level links, read as the
/// host reads them.
fn libraries(s: &EditorState, dir: &Path) -> Libraries {
    let origins = s.document().unwrap().linked_origins();
    read_origins(&origins, Some(&built_in_assets()), Some(dir))
}

/// The level's link of `origin`'s status against the libraries on disk.
fn status_of(s: &EditorState, dir: &Path, origin: &AssetOrigin) -> Status {
    let links = s.document().unwrap().compare_library(&libraries(s, dir));
    links.into_iter().find(|l| l.origin == *origin).expect("linked").status
}

fn message(s: &EditorState) -> String {
    s.status().message.clone().unwrap_or_default()
}

/// The copy selected (issue #82: a placed copy is).
fn selected(s: &EditorState) -> String {
    match s.selection() {
        Selection::Copy(id) => id.clone(),
        other => panic!("a copy selected, not {other:?}"),
    }
}

/// The level's asset key of copy `id`.
fn key_of(s: &EditorState, id: &str) -> String {
    s.document().unwrap().placed_copy(id).unwrap().asset
}

/// The placed level (`level_tab::placed`) and the user's library's folder.
fn level(root: &Path) -> (tempfile::TempDir, tempfile::TempDir, EditorState, PathBuf) {
    let (t, t2, s) = placed(root);
    let dir = t2.path().join("yours");
    (t, t2, s, dir)
}

/// The built-in strip light in the user's library as `strip`, told.
fn strip_in_yours(s: &mut EditorState, dir: &Path) -> AssetSource {
    let src = read_builtin("strip-light");
    put(dir, "strip", &src);
    changed(s, dir);
    src
}

#[test]
fn dragging_a_thumbnail_places_a_copy_where_let_go() {
    let Some(root) = content() else { return };
    let (_t, _t2, s, _dir) = level(&root);
    let before = s.document().unwrap().copies().len();
    let mut h = harness(s);
    h.run();
    let from = thumbnail(&h, "Torch").rect().center();
    let to = screen_of(&h, (100, 60));
    pointer_drag(&mut h, from, to);
    let s = &h.state().state;
    assert_eq!(s.document().unwrap().copies().len(), before + 1, "{}", message(s));
    let id = selected(s);
    assert_eq!(s.document().unwrap().placed_copy(&id).unwrap().at, (100, 60), "its top-left where let go");
    assert_eq!(s.document().unwrap().copy_origin(&id), Some(AssetOrigin::Builtin("torch".into())));
    assert!(!s.placing(), "a drag arms nothing");
}

#[test]
fn pick_then_click_places_and_escape_stops() {
    let Some(root) = content() else { return };
    let (_t, _t2, s, _dir) = level(&root);
    let before = s.document().unwrap().copies().len();
    let mut h = harness(s);
    h.run();
    thumbnail(&h, "Ceiling lamp").click();
    h.run();
    assert!(h.state().state.placing());
    let at = screen_of(&h, (150, 80));
    h.event(egui::Event::PointerMoved(at));
    h.run();
    for pressed in [true, false] {
        h.event(button(at, pressed));
        h.run();
    }
    let s = &h.state().state;
    assert_eq!(s.document().unwrap().copies().len(), before + 1);
    assert_eq!(s.document().unwrap().placed_copy(&selected(s)).unwrap().at, (150, 80));
    assert!(!s.placing(), "one press places one copy");
    // Picked again, Escape stops.
    thumbnail(&h, "Ceiling lamp").click();
    h.run();
    assert!(h.state().state.placing());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().state.placing(), "Escape stops");
    // A second click unpicks.
    thumbnail(&h, "Ceiling lamp").click();
    h.run();
    thumbnail(&h, "Ceiling lamp").click();
    h.run();
    assert_eq!(h.state().state.picked_asset(), None);
    // A tool change stops.
    thumbnail(&h, "Ceiling lamp").click();
    h.run();
    h.get_by_label("Spawn").click();
    h.run();
    assert!(!h.state().state.placing(), "a tool change stops");
    assert_eq!(h.state().state.document().unwrap().copies().len(), before + 1);
}

#[test]
fn placing_twice_reuses_the_levels_copy() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, _dir) = level(&root);
    let torch = AssetOrigin::Builtin("torch".into());
    let keys = s.document().unwrap().keys_of(&torch);
    assert_eq!(keys.len(), 1);
    let before = s.document().unwrap().copies().len();
    for at in [(10, 10), (40, 10)] {
        run(&mut s, A::PlaceAsset { key: builtin("torch"), at });
    }
    let doc = s.document().unwrap();
    assert_eq!(doc.copies().len(), before + 2);
    assert_eq!(doc.keys_of(&torch), keys, "one copy of the asset in the level");
    assert_eq!(key_of(&s, &selected(&s)), keys[0]);
}

#[test]
fn placing_a_changed_library_asset_uses_the_new_version_and_offers_the_update() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, dir) = level(&root);
    let v1 = strip_in_yours(&mut s, &dir);
    let strip = AssetOrigin::Yours("strip".into());
    run(&mut s, A::PlaceAsset { key: yours("strip"), at: (10, 10) });
    let first = selected(&s);
    // The library's version changes behind the level's back.
    let mut v2 = v1.clone();
    v2.file.name = "Strip light, brighter".into();
    put(&dir, "strip", &v2);
    run(&mut s, A::PlaceAsset { key: yours("strip"), at: (40, 10) });
    let second = selected(&s);
    let doc = s.document().unwrap();
    // The older copy took the library's version: one version again.
    assert_eq!(doc.keys_of(&strip).len(), 1, "{}", message(&s));
    assert_eq!(key_of(&s, &first), key_of(&s, &second));
    assert_eq!(doc.asset_name(&key_of(&s, &first)).as_deref(), Some("Strip light, brighter"));
    assert!(message(&s).contains("1 copy of Strip light, brighter updated"), "{}", message(&s));
    assert_eq!(status_of(&s, &dir, &strip), Status::Same);
    assert_eq!(s.undo_name().as_deref(), Some("Update Strip light, brighter"));
    // Undone, the new copy holds the library's version under a new key
    // and the older one its own.
    run(&mut s, A::Undo);
    let doc = s.document().unwrap();
    assert_eq!(doc.keys_of(&strip).len(), 2);
    assert_ne!(key_of(&s, &first), key_of(&s, &second));
    assert_eq!(doc.asset_name(&key_of(&s, &second)).as_deref(), Some("Strip light, brighter"));
    assert_eq!(doc.asset_name(&key_of(&s, &first)).as_deref(), Some("Strip light"));
}

/// The asset tab's asset renamed `name` (an edit of it).
fn rename_asset(s: &mut EditorState, name: &str) {
    let meta = s.active_document().unwrap().meta();
    run(s, A::Meta(olr_editor::LevelMeta { name: name.into(), ..meta }));
    assert!(s.active_dirty());
}

#[test]
fn saving_a_library_asset_updates_the_open_levels_copies_in_one_step() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, dir) = level(&root);
    strip_in_yours(&mut s, &dir);
    let strip = AssetOrigin::Yours("strip".into());
    for at in [(10, 10), (40, 10)] {
        run(&mut s, A::PlaceAsset { key: yours("strip"), at });
    }
    let key = key_of(&s, &selected(&s));
    run(&mut s, A::Save);
    assert!(!s.dirty());
    // Edited in the asset tab and saved over the library's.
    run(&mut s, A::EditAsset(yours("strip")));
    assert_eq!(s.doc_tab(), DocTab::Asset);
    rename_asset(&mut s, "Long light");
    run(&mut s, A::Save);
    assert!(!s.active_dirty());
    let doc = s.document().unwrap();
    assert_eq!(doc.asset_name(&key).as_deref(), Some("Long light"), "the level's copy follows");
    assert_eq!(status_of(&s, &dir, &strip), Status::Same);
    assert!(message(&s).contains("2 copies of Long light updated"), "{}", message(&s));
    assert!(s.dirty(), "the level unsaved");
    run(&mut s, A::ShowTab(DocTab::Level));
    assert_eq!(s.undo_name().as_deref(), Some("Update Long light"));
    run(&mut s, A::Undo);
    assert_eq!(
        s.document().unwrap().asset_name(&key).as_deref(),
        Some("Strip light"),
        "the old version back"
    );
    assert!(matches!(status_of(&s, &dir, &strip), Status::Changed { .. }));
}

/// A level placing a copy of the user's `strip` (the strip light with a
/// second state, "Off") that starts in "Off"; the library holding it.
fn starting_off(root: &Path) -> (tempfile::TempDir, tempfile::TempDir, EditorState, PathBuf, AssetSource) {
    use olr_edit::placed::{CopyProperty, add_copy, bake_pack, set_copy};
    let mut asset = olr_edit::asset::decode(&read_builtin("strip-light")).unwrap();
    olr_edit::asset::add_state(&mut asset, "Off", None).unwrap();
    let v1 = olr_edit::asset::encode(&asset).unwrap();
    let (t, dir) = copy_pack(root, &fixture());
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    let added = add_copy(&mut pack, &v1, AssetOrigin::Yours("strip".into()), (10, 10), None, None).unwrap();
    set_copy(&mut pack, &added.id, CopyProperty::State(Some("Off".into()))).unwrap();
    bake_pack(&mut pack, &catalog(root).themes[0].back_tile).unwrap();
    olr_edit::pack::save(&dir, &pack).unwrap();
    let (t2, mut s) = editor_at(root, &dir);
    let lib = t2.path().join("yours");
    put(&lib, "strip", &v1);
    changed(&mut s, &lib);
    (t, t2, s, lib, v1)
}

#[test]
fn a_breaking_library_change_is_not_applied() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, dir, v1) = starting_off(&root);
    let strip = AssetOrigin::Yours("strip".into());
    assert_eq!(status_of(&s, &dir, &strip), Status::Same);
    assert!(!s.can_undo());
    // The library's version loses the state the copy starts in.
    let mut asset = olr_edit::asset::decode(&v1).unwrap();
    olr_edit::asset::remove_state(&mut asset, "Off").unwrap();
    put(&dir, "strip", &olr_edit::asset::encode(&asset).unwrap());
    changed(&mut s, &dir);
    let doc = s.document().unwrap();
    let key = &doc.keys_of(&strip)[0];
    assert_eq!(doc.placement().unwrap().assets[key].file.states.len(), 2, "not applied");
    assert!(!s.can_undo() && !s.dirty());
    assert!(message(&s).contains("Strip light changed: 1 copy would break"), "{}", message(&s));
    let link = s.library_links().iter().find(|l| l.origin == strip).expect("compared");
    assert!(matches!(&link.status, Status::Changed { breaks } if breaks.len() == 1), "{link:?}");
}

#[test]
fn builtin_copies_ignore_the_users_library() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, dir) = level(&root);
    // The user's library has a folder named as the built-in lamp's.
    let mut mine = read_builtin("ceiling-lamp");
    mine.file.name = "My lamp".into();
    put(&dir, "ceiling-lamp", &mine);
    let cmds = run(&mut s, A::AssetsChanged(set_library(&built_in_assets(), &dir)));
    let origins = cmds
        .iter()
        .find_map(|c| if let C::CompareLibrary { origins } = c { Some(origins.clone()) } else { None })
        .expect("compared");
    assert!(origins.contains(&AssetOrigin::Builtin("ceiling-lamp".into())));
    let doc = s.document().unwrap();
    assert_eq!(doc.asset_name(&key_of(&s, "ceiling-lamp")).as_deref(), Some("Ceiling lamp"));
    assert_eq!(status_of(&s, &dir, &AssetOrigin::Builtin("ceiling-lamp".into())), Status::Same);
    assert!(!s.can_undo() && !s.dirty());
}

#[test]
fn unlink_makes_the_copy_own() {
    let Some(root) = content() else { return };
    let (_t, _t2, s, _dir) = level(&root);
    let mut h = harness(s);
    h.run();
    crate::level_tab::row(&h, "Torch").click();
    h.run();
    h.get_by_label(&labels::origin(true, "Torch"));
    assert!(h.query_by_label(labels::SAVE_TO_LIBRARY).is_none());
    h.get_by_label(labels::UNLINK).click();
    h.run();
    let s = &h.state().state;
    assert_eq!(s.document().unwrap().copy_origin("torch"), Some(AssetOrigin::Own));
    assert_eq!(s.undo_name().as_deref(), Some("Unlink Torch"));
    // The level's own now: it may be saved to the library.
    h.get_by_label(labels::SAVE_TO_LIBRARY);
    assert!(h.query_by_label(labels::UNLINK).is_none());
    let mut s = h.into_state().state;
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().copy_origin("torch"), Some(AssetOrigin::Builtin("torch".into())));
    assert!(!s.can_undo(), "one step");
}

#[test]
fn save_to_library_links_an_own_asset_and_compares_same() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, dir) = level(&root);
    assert_eq!(s.document().unwrap().copy_origin("strip-light"), Some(AssetOrigin::Own));
    run(&mut s, A::SaveToLibrary("strip-light".into()));
    assert_eq!(s.dialog(), Some(&Dialog::AssetName { suggested: "Strip light".into() }));
    run(&mut s, A::NameAsset("My strip".into()));
    assert!(dir.join("my-strip/asset.ron").is_file());
    let mine = AssetOrigin::Yours("my-strip".into());
    assert_eq!(s.document().unwrap().copy_origin("strip-light"), Some(mine.clone()), "{}", message(&s));
    assert_eq!(status_of(&s, &dir, &mine), Status::Same, "the level's copy is the library's");
    assert_eq!(s.picked_asset(), Some(&yours("my-strip")));
    assert!(s.dirty());
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().copy_origin("strip-light"), Some(AssetOrigin::Own));
    assert!(!s.can_undo(), "one step");
}

#[test]
fn save_to_library_refuses_unused_groups_and_distant_layers_with_a_reason() {
    let Some(root) = content() else { return };
    // An own asset listing a glow group no layer glows in.
    let (_t, dir) = copy_pack(&root, &fixture());
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    let mut spare = read_builtin("strip-light");
    let mut group = spare.file.glow_groups[0].clone();
    group.id = "spare".into();
    spare.file.glow_groups.push(group);
    let added =
        olr_edit::placed::add_copy(&mut pack, &spare, AssetOrigin::Own, (10, 10), None, None).unwrap();
    olr_edit::placed::bake_pack(&mut pack, &catalog(&root).themes[0].back_tile).unwrap();
    olr_edit::pack::save(&dir, &pack).unwrap();
    let (_t2, mut s) = editor_at(&root, &dir);
    run(&mut s, A::SaveToLibrary(added.id.clone()));
    assert!(s.dialog().is_none());
    assert_eq!(message(&s), "remove the unused glow group spare first");
    // A plain level's ground, made distant in part: stays the level's.
    let (_t3, mut s) = editor_at(&root, Path::new("olx-level-dirt-level"));
    let lib = _t3.path().join("yours");
    changed(&mut s, &lib);
    crate::common::edit_ground(&mut s);
    run(&mut s, A::Set("background".into(), Property::Distant(Some(50))));
    run(&mut s, A::ShowTab(DocTab::Level));
    run(&mut s, A::SaveToLibrary(GROUND_COPY.into()));
    assert!(s.dialog().is_none());
    assert_eq!(message(&s), "a level's ground with distant layers stays the level's");
    // A plain level's ground, three drawn layers, saves; the ground stays
    // the level's own.
    let (_t4, mut s) = editor_at(&root, Path::new("olx-level-dirt-level"));
    let lib = _t4.path().join("yours");
    changed(&mut s, &lib);
    run(&mut s, A::SaveToLibrary(GROUND_COPY.into()));
    assert!(matches!(s.dialog(), Some(Dialog::AssetName { .. })), "{}", message(&s));
    run(&mut s, A::NameAsset("Dirt ground".into()));
    assert!(lib.join("dirt-ground/asset.ron").is_file());
    assert_eq!(s.document().unwrap().copy_origin(GROUND_COPY), Some(AssetOrigin::Own));
    let saved = olr_edit::asset::load_dir(&lib.join("dirt-ground")).unwrap();
    assert_eq!(saved.pieces.len(), 3, "its three layers");
}

#[test]
fn saving_an_edited_builtin_offers_use_your_copy() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, dir) = level(&root);
    run(&mut s, A::EditAsset(builtin("ceiling-lamp")));
    rename_asset(&mut s, "My lamp");
    run(&mut s, A::Save);
    assert!(dir.join("my-lamp/asset.ron").is_file());
    let builtin_lamp = AssetOrigin::Builtin("ceiling-lamp".into());
    let doc = s.document().unwrap();
    assert_eq!(doc.copy_origin("ceiling-lamp"), Some(builtin_lamp.clone()), "built-in copies stay");
    assert!(s.use_yours().is_some(), "offered");
    // The status line offers it, in either tab.
    let mut h = harness(s);
    h.run();
    h.get_by_label(labels::USE_YOUR_COPY).click();
    h.run();
    let mut s = h.into_state().state;
    let mine = AssetOrigin::Yours("my-lamp".into());
    assert_eq!(s.document().unwrap().copy_origin("ceiling-lamp"), Some(mine.clone()), "{}", message(&s));
    assert!(message(&s).contains("1 copy of My lamp now uses yours"), "{}", message(&s));
    assert_eq!(status_of(&s, &dir, &mine), Status::Same);
    assert!(s.use_yours().is_none(), "taken");
    run(&mut s, A::ShowTab(DocTab::Level));
    assert_eq!(s.undo_name().as_deref(), Some("Use your My lamp"));
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().copy_origin("ceiling-lamp"), Some(builtin_lamp));
    assert!(!s.can_undo(), "one step");
}

#[test]
fn comparisons_go_through_compare_library() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, dir) = level(&root);
    strip_in_yours(&mut s, &dir);
    run(&mut s, A::PlaceAsset { key: yours("strip"), at: (10, 10) });
    let key = key_of(&s, &selected(&s));
    run(&mut s, A::EditAsset(yours("strip")));
    rename_asset(&mut s, "Long light");
    // The host's part done by hand: nothing reads the library but it.
    let cmds = s.update(A::Save);
    let Some(C::SaveAsset { asset, replace }) = cmds.into_iter().find(|c| matches!(c, C::SaveAsset { .. }))
    else {
        panic!("saved")
    };
    let id = olr_edit::asset::add_to_library(&dir, &asset, replace.as_deref()).unwrap();
    let wants = |cmds: &[C]| {
        cmds.iter().any(|c| matches!(c, C::CompareLibrary { origins } if origins.contains(&AssetOrigin::Yours("strip".into()))))
    };
    let cmds = s.update(A::AssetsChanged(set_library(&built_in_assets(), &dir)));
    assert!(wants(&cmds), "{cmds:?}");
    let source = olr_edit::asset::read_dir(&dir.join(&id)).ok().map(Box::new);
    let cmds = s.update(A::AssetSaved { key: yours(&id), source });
    assert!(wants(&cmds), "{cmds:?}");
    let name = |s: &EditorState| s.document().unwrap().asset_name(&key);
    assert_eq!(name(&s).as_deref(), Some("Strip light"), "nothing changes before the answer");
    let origins = s.document().unwrap().linked_origins();
    s.update(A::LibraryCompared(read_origins(&origins, Some(&built_in_assets()), Some(&dir))));
    assert_eq!(name(&s).as_deref(), Some("Long light"));
}

#[test]
fn search_filters_by_name() {
    let Some(root) = content() else { return };
    let (t, mut s) = editor(&root);
    changed(&mut s, &t.path().join("yours"));
    let mut h = harness(s);
    h.run();
    let shown =
        |h: &Harness<'static, App>, name: &str| h.query_by_role_and_label(Role::Button, name).is_some();
    assert!(shown(&h, "Torch") && shown(&h, "Ceiling lamp"));
    h.get_by_role_and_label(Role::TextInput, labels::SEARCH_ASSETS).click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, labels::SEARCH_ASSETS).type_text("LAMP");
    h.run();
    assert!(shown(&h, "Ceiling lamp"), "case aside");
    assert!(!shown(&h, "Torch") && !shown(&h, "Strip light"));
    let entry =
        |name: &str| h.state().state.catalog().assets.iter().find(|a| a.name == name).unwrap().clone();
    assert!(ui::asset_matches(&entry("Ceiling lamp"), "lamp"));
    assert!(!ui::asset_matches(&entry("Torch"), "lamp"));
}
