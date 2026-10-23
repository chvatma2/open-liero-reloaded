//! The asset library in the editor (issue #9): Save as asset (the
//! selection, or the pixel rectangle cut from every visible layer, the
//! rectangle not clipped to its layer), the name dialog and its clash,
//! placing as one step (its glow groups joined or added, a level without a
//! look given one and told), Remove asking first, Import through the
//! browser; and an `egui_kittest` flow that saves and places.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_edit::compose::Rect;
use olr_editor::browser::Purpose;
use olr_editor::tools::{PointerKind, Tool};
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{
    AssetKey, Choice, Dialog, EditorAction as A, EditorCommand as C, EditorState, PanelTab, Selection,
};
use olr_format::look::{GlowGroupSource, LookSource, ReachSource};

use crate::common::{built_in_assets, content, editor, open_fixture, run, set_library};

/// The fixture open, with an empty library of yours (in the temporary
/// directory) beside the built-in assets.
fn with_library() -> Option<(tempfile::TempDir, EditorState)> {
    let root = content()?;
    let (tmp, mut s) = editor(&root);
    let assets = set_library(&built_in_assets(), &tmp.path().join("yours"));
    run(&mut s, A::AssetsChanged(assets));
    Some((tmp, s))
}

fn yours(id: &str) -> AssetKey {
    AssetKey { builtin: false, id: id.into() }
}

fn built_in(id: &str) -> AssetKey {
    AssetKey { builtin: true, id: id.into() }
}

fn drag(s: &mut EditorState, from: (i32, i32), to: (i32, i32)) {
    for (kind, world) in [(PointerKind::Down, from), (PointerKind::Move, to), (PointerKind::Up, to)] {
        run(s, A::Pointer { kind, world, shift: false, toggle: false });
    }
}

fn top_ids(s: &EditorState, n: usize) -> Vec<String> {
    let items = s.document().unwrap().items();
    items[items.len() - n..].iter().map(|i| i.element.id.clone()).collect()
}

#[test]
fn saving_the_selection_asks_its_name_and_writes_it_into_yours() {
    let Some((tmp, mut s)) = with_library() else { return };
    // Nothing selected: nothing to save, said.
    run(&mut s, A::SaveAsAsset);
    assert!(s.dialog().is_none());
    assert!(s.status().message.as_deref().is_some_and(|m| m.contains("nothing to save")));
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::SaveAsAsset);
    assert_eq!(s.dialog(), Some(&Dialog::AssetName { suggested: "Tunnels".into() }));
    // An empty name asks again.
    run(&mut s, A::NameAsset("  ".into()));
    assert!(matches!(s.dialog(), Some(Dialog::AssetName { .. })));
    let cmds = run(&mut s, A::NameAsset(" Tunnels ".into()));
    assert!(cmds.iter().any(|c| matches!(c, C::SaveAsset { replace: None, .. })), "{cmds:?}");
    assert!(tmp.path().join("yours/tunnels/asset.ron").is_file());
    assert_eq!(s.picked_asset(), Some(&yours("tunnels")));
    assert_eq!(s.tab(), PanelTab::Assets);
    assert!(!s.placing(), "a saved asset is selected, not armed");
    assert_eq!(s.asset(&yours("tunnels")).map(|a| a.name.as_str()), Some("Tunnels"));
    assert!(!s.dirty(), "saving an asset leaves the level as it was");

    // The same name again: replace, or keep both.
    run(&mut s, A::SaveAsAsset);
    run(&mut s, A::NameAsset("Tunnels".into()));
    assert_eq!(s.dialog(), Some(&Dialog::AssetExists { name: "Tunnels".into(), id: "tunnels".into() }));
    run(&mut s, A::Choose(Choice::KeepBoth));
    assert_eq!(s.asset(&yours("tunnels-2")).map(|a| a.name.as_str()), Some("Tunnels 2"));
    run(&mut s, A::Select(Selection::Element("wall".into())));
    run(&mut s, A::SaveAsAsset);
    run(&mut s, A::NameAsset("Tunnels".into()));
    let cmds = run(&mut s, A::Choose(Choice::Confirm));
    assert!(cmds.iter().any(|c| matches!(c, C::SaveAsset { replace: Some(id), .. } if id == "tunnels")));
    let replaced = olr_edit::asset::load_dir(&tmp.path().join("yours/tunnels")).unwrap();
    assert_eq!(replaced.pieces[0].element.id, "wall");
    // Cancel drops it.
    run(&mut s, A::SaveAsAsset);
    run(&mut s, A::Choose(Choice::Cancel));
    assert_eq!(s.catalog().assets.iter().filter(|a| !a.key.builtin).count(), 2);
}

#[test]
fn a_pixel_rectangle_saves_its_world_rectangle_of_every_visible_layer() {
    let Some((_tmp, mut s)) = with_library() else { return };
    // The wall's box starts at x 4: the rectangle shown is clipped to it,
    // the one saved is not.
    run(&mut s, A::Select(Selection::Element("wall".into())));
    run(&mut s, A::Tool(Tool::SelectPixels));
    drag(&mut s, (0, 0), (9, 9));
    assert_eq!(s.pixel_rect().map(|r| r.1), Some(Rect { x: 4, y: 0, w: 6, h: 10 }));
    let cmds = run(&mut s, A::SaveAsAsset);
    assert!(cmds.is_empty());
    run(&mut s, A::NameAsset("Corner".into()));
    let a = olr_edit::asset::load_dir(&_tmp.path().join("yours/corner")).unwrap();
    assert_eq!(a.size, (10, 10));
    assert_eq!(a.pieces[0].element.id, "base");
    // Every visible layer with pixels there, a hidden one left out.
    let art = s.document().unwrap().layer_art("tunnels").unwrap().clone();
    let sc = s.document().unwrap().level().art_scale;
    let p = (0..art.height() / sc - 10)
        .flat_map(|y| (0..art.width() / sc - 10).map(move |x| (x, y)))
        .find(|&(x, y)| art.get(x * sc, y * sc).a != 0)
        .map(|(x, y)| (x as i32, y as i32))
        .expect("tunnels has pixels");
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    drag(&mut s, p, (p.0 + 9, p.1 + 9));
    run(&mut s, A::SaveAsAsset);
    run(&mut s, A::NameAsset("Tunnel bit".into()));
    let a = olr_edit::asset::load_dir(&_tmp.path().join("yours/tunnel-bit")).unwrap();
    let ids: Vec<&str> = a.pieces.iter().map(|p| p.element.id.as_str()).collect();
    assert!(ids.contains(&"base") && ids.contains(&"tunnels"), "{ids:?}");
    run(&mut s, A::Set("tunnels".into(), olr_edit::element::Property::Visible(false)));
    run(&mut s, A::Select(Selection::Element("base".into())));
    drag(&mut s, p, (p.0 + 9, p.1 + 9));
    run(&mut s, A::SaveAsAsset);
    run(&mut s, A::NameAsset("Hidden".into()));
    let a = olr_edit::asset::load_dir(&_tmp.path().join("yours/hidden")).unwrap();
    assert!(a.pieces.iter().all(|p| p.element.id != "tunnels"));
}

#[test]
fn placing_is_one_step_that_selects_the_new_elements_and_adds_lighting() {
    let Some((_tmp, mut s)) = with_library() else { return };
    let before = s.document().unwrap().items().len();
    assert_eq!(s.document().unwrap().level().look, None);
    run(&mut s, A::PickAsset(Some(built_in("control-desk"))));
    assert!(s.placing());
    let cmds = run(&mut s, A::PlaceAsset { key: built_in("control-desk"), at: (100, 120) });
    assert!(cmds.iter().any(|c| matches!(c, C::ReadAsset { .. })), "{cmds:?}");
    assert!(!s.placing(), "placed once");
    let doc = s.document().unwrap();
    assert_eq!(doc.items().len(), before + 3);
    let ids = top_ids(&s, 3);
    assert_eq!(ids, ["control-desk-background", "control-desk-screens", "control-desk-displays"]);
    assert_eq!(s.selection(), &Selection::Many(ids.clone()));
    assert_eq!(s.tab(), PanelTab::Elements);
    // The level had no look: the default one with the asset's groups.
    let look = doc.level().look.clone().expect("a look");
    let groups: Vec<&str> = look.glow_groups.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(groups, ["screens", "displays"]);
    // And its surface materials (issue #46: the factory's).
    let materials: Vec<&str> = look.materials.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(materials, ["concrete", "metal", "glass"]);
    assert_eq!(
        LookSource { glow_groups: Vec::new(), materials: Vec::new(), ..look.clone() },
        LookSource::default()
    );
    assert_eq!(s.status().message.as_deref(), Some(olr_editor::asset_ops::ADDED_LIGHTING));
    // Placed where asked: the desk's box starts at the point.
    let at = |id: &str| {
        let d = s.document().unwrap();
        olr_format::levels::element_box(&d.items()[d.index_of(id).unwrap()].element.kind, d.level().size)
            .unwrap()
            .0
    };
    assert_eq!(at("control-desk-background"), (100, 120));
    // One undo takes the elements and the look away together.
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().items().len(), before);
    assert_eq!(s.document().unwrap().level().look, None);
    run(&mut s, A::Redo);
    assert_eq!(s.document().unwrap().items().len(), before + 3);
}

#[test]
fn placing_joins_a_group_the_level_has_and_adds_one_it_lacks() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let mine = GlowGroupSource {
        id: "screens".into(),
        colour: Default::default(),
        intensity: 42,
        reach: ReachSource::Pixels(7),
        animation: Default::default(),
        phase_ms: None,
    };
    let look = LookSource { glow_groups: vec![mine.clone()], ..LookSource::default() };
    doc.set_look(Some(look)).unwrap();
    let desk = olr_edit::asset::load_dir(&built_in_assets().join("control-desk")).unwrap();
    let placed = doc.place_asset(&desk, (0, 0)).unwrap();
    assert_eq!(placed.note, None, "the level has a look");
    let groups = &doc.level().look.as_ref().unwrap().glow_groups;
    // Its own `screens` kept as it was; `displays` with the asset's settings,
    // not a new group's defaults.
    assert_eq!(groups[0], mine);
    assert_eq!(groups[1], desk.groups.iter().find(|g| g.id == "displays").unwrap().clone());
    assert_eq!(groups.len(), 2);
}

fn material(id: &str, name: &str) -> olr_format::materials::MaterialSource {
    olr_format::materials::MaterialSource {
        light_soak: None,
        id: id.into(),
        name: name.into(),
        notes: String::new(),
        heat: None,
    }
}

/// Issue #38: placing joins the asset's materials by id (the level's
/// settings kept) or adds them; its elements keep their surfaces and their
/// painted maps follow the level's numbering; one undo takes it all back.
#[test]
fn placing_joins_or_adds_materials_and_lands_painted_maps() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let look = LookSource { materials: vec![material("wood", "Oak")], ..LookSource::default() };
    doc.set_look(Some(look)).unwrap();
    let before = doc.to_pack();
    let mut desk = olr_edit::asset::load_dir(&built_in_assets().join("control-desk")).unwrap();
    desk.materials = vec![material("metal", "Metal"), material("wood", "Wood")];
    let piece = &mut desk.pieces[0];
    piece.element.surface = Some("metal".into());
    piece.element.surface_map = true;
    let (w, h) = olr_format::levels::surface_map_size(&piece.element.kind, desk.size);
    let mut map = vec![0u8; (w * h) as usize];
    map[0] = 2;
    piece.surface_map = Some(std::sync::Arc::new(map));
    let placed = doc.place_asset(&desk, (0, 0)).unwrap();
    let materials = &doc.level().look.as_ref().unwrap().materials;
    assert_eq!(materials, &vec![material("wood", "Oak"), material("metal", "Metal")]);
    let id = &placed.ids[0];
    let e = &doc.items().iter().find(|i| i.element.id == *id).unwrap().element;
    assert_eq!((e.surface.as_deref(), e.surface_map), (Some("metal"), true));
    let pack = doc.to_pack();
    let file = olr_format::levels::surface_file(id);
    let values = olr_format::image_io::decode_grey_within(
        &pack.composition.as_ref().unwrap().files[&file],
        olr_format::image_io::IMAGES,
    )
    .unwrap()
    .2;
    assert_eq!((values[0], values[1]), (1, 0), "wood is the level's first");
    for edit in placed.edits.iter().rev() {
        doc.apply(edit, false);
    }
    assert_eq!(doc.to_pack(), before);
}

#[test]
fn removing_asks_first_and_only_yours() {
    let Some((tmp, mut s)) = with_library() else { return };
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::SaveAsAsset);
    run(&mut s, A::NameAsset("Tunnels".into()));
    run(&mut s, A::RemoveAsset("tunnels".into()));
    assert_eq!(s.dialog(), Some(&Dialog::RemoveAsset { id: "tunnels".into(), name: "Tunnels".into() }));
    run(&mut s, A::Choose(Choice::Cancel));
    assert!(tmp.path().join("yours/tunnels").is_dir());
    run(&mut s, A::RemoveAsset("tunnels".into()));
    run(&mut s, A::Choose(Choice::Confirm));
    assert!(!tmp.path().join("yours/tunnels").exists());
    assert!(s.asset(&yours("tunnels")).is_none() && s.picked_asset().is_none());
    // A built-in asset is none of yours: nothing asked, nothing removed.
    run(&mut s, A::RemoveAsset("torch".into()));
    assert!(s.dialog().is_none());
    assert!(built_in_assets().join("torch").is_dir());
}

#[test]
fn an_asset_folder_is_imported_through_the_browser() {
    let Some((tmp, mut s)) = with_library() else { return };
    run(&mut s, A::Browse(Purpose::ImportAsset));
    let dir = built_in_assets();
    run(&mut s, A::Listed { dir: dir.clone(), entries: Ok(crate::common::list_dir(&dir)) });
    let cmds = run(&mut s, A::Pick("torch".into()));
    assert!(cmds.iter().any(|c| matches!(c, C::ImportAsset(p) if p.ends_with("torch"))), "{cmds:?}");
    assert!(tmp.path().join("yours/torch/asset.ron").is_file());
    assert_eq!(s.picked_asset(), Some(&yours("torch")));
    assert!(s.browser().is_none());
}

/// The editor and the host's answers, as `flows.rs` drives it.
struct App {
    state: EditorState,
    data: EditorData,
}

#[test]
fn save_and_place_through_the_panels() {
    let Some((tmp, state)) = with_library() else { return };
    let app = App { state, data: EditorData::default() };
    let mut h = Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                run(&mut app.state, a);
            }
        },
        app,
    );
    h.run();
    h.get_by_label("Tunnels").click();
    h.run();
    h.get_by_label(labels::SAVE_AS_ASSET).click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, labels::ASSET_NAME).type_text(" pit");
    h.run();
    h.get_by_label(labels::SAVE).click();
    h.run();
    assert!(tmp.path().join("yours/tunnels-pit/asset.ron").is_file());
    // The Assets tab shows it under Yours, with the built-in ones.
    assert_eq!(h.state().state.tab(), PanelTab::Assets);
    h.get_by_label(labels::YOURS);
    h.get_by_label(labels::BUILT_IN);
    h.get_by_role_and_label(Role::Button, "Torch");
    // Picked, then a press on the canvas places it there.
    let before = h.state().state.document().unwrap().items().len();
    h.get_by_role_and_label(Role::Button, "Tunnels pit").click();
    h.run();
    assert!(h.state().state.placing());
    let pos = h.state().data.canvas_rect().unwrap().center();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
    let s = &h.state().state;
    assert_eq!(s.document().unwrap().items().len(), before + 1);
    assert_eq!(s.selection(), &Selection::Element(top_ids(s, 1)[0].clone()));
    assert_eq!(top_ids(s, 1), ["tunnels-pit"]);
    assert!(!s.placing());
}
