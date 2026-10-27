//! The asset tab (issue #77): a library asset opened beside the level,
//! painted and saved into the user's library; the tabs' own histories and
//! unsaved marks; Close asking for each; crash recovery the level's; the
//! level-only tools absent; the open level's theme painting its tiles; an
//! asset's states and exposure kept through a save, and the edits that
//! would break them refused.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_data::image::Rgba;
use olr_data::level::Material;
use olr_edit::asset::{Asset, OwnPicture};
use olr_edit::element::Property;
use olr_editor::asset_tab::{BUILT_IN_NOTE, FIRST_LAYER, NewAsset};
use olr_editor::tools::{PaintChoice, PointerKind, Tool};
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{
    AssetKey, Choice, Dialog, DocTab, EditorAction as A, EditorCommand as C, EditorState, Pending, Selection,
};
use olr_format::assets::{ExposedSource, PropertySource as P, StateValue, ValueSource as V};
use olr_format::levels::{ElementGlass, ElementMaterial, GlowBeam, MAX_SIDE};

use crate::common::{built_in_assets, content, editor, run, set_library, tree};

const RED: Rgba = Rgba::rgb(255, 0, 0);

/// The fixture open, with an empty library of yours beside the built-in
/// assets.
fn with_library() -> Option<(tempfile::TempDir, EditorState)> {
    let root = content()?;
    let (tmp, mut s) = editor(&root);
    let assets = set_library(&built_in_assets(), &tmp.path().join("yours"));
    run(&mut s, A::AssetsChanged(assets));
    Some((tmp, s))
}

fn built_in(id: &str) -> AssetKey {
    AssetKey { builtin: true, id: id.into() }
}

/// The built-in ceiling lamp (`rock` at the top, glowing `lamps` under it).
fn lamp() -> Asset {
    olr_edit::asset::load_dir(&built_in_assets().join("ceiling-lamp")).unwrap()
}

/// `asset` written into the user's library, the state told: its key.
fn save_yours(tmp: &tempfile::TempDir, s: &mut EditorState, asset: &Asset) -> AssetKey {
    let yours = tmp.path().join("yours");
    std::fs::create_dir_all(&yours).unwrap();
    let id = olr_edit::asset::add_to_library(&yours, asset, None).unwrap();
    run(s, A::AssetsChanged(set_library(&built_in_assets(), &yours)));
    AssetKey { builtin: false, id }
}

/// A brush stroke of `paint` on layer `id` at world pixel `at`.
fn stroke(s: &mut EditorState, id: &str, paint: PaintChoice, at: (i32, i32)) {
    run(s, A::Select(Selection::Element(id.into())));
    run(s, A::Tool(Tool::Brush));
    run(s, A::Paint(paint));
    for kind in [PointerKind::Down, PointerKind::Move, PointerKind::Up] {
        run(s, A::Pointer { kind, world: at, shift: false, toggle: false });
    }
}

/// Layer `id`'s art pixel at world pixel `at` of the shown document.
fn pixel(s: &EditorState, id: &str, at: (u32, u32)) -> Rgba {
    let d = s.active_document().unwrap();
    let sc = d.level().art_scale;
    d.layer_art(id).unwrap().get(at.0 * sc, at.1 * sc)
}

fn message(s: &EditorState) -> String {
    s.status().message.clone().unwrap_or_default()
}

#[test]
fn an_asset_opens_edits_and_saves_to_the_library() {
    let Some((tmp, mut s)) = with_library() else { return };
    let key = save_yours(&tmp, &mut s, &lamp());
    let level_dir = s.document().unwrap().dir().to_path_buf();
    run(&mut s, A::EditAsset(key.clone()));
    assert_eq!(s.doc_tab(), DocTab::Asset);
    assert_eq!(s.asset_key(), Some(&key));
    let doc = s.active_document().unwrap();
    assert!(doc.is_asset() && !s.active_dirty());
    assert_eq!(doc.level().size, (13, 6));
    assert!(doc.level().objects.is_empty());
    assert_eq!(s.document().unwrap().dir(), level_dir, "the level stays the level");
    stroke(&mut s, "rock", PaintChoice::Colour(RED), (5, 1));
    assert_eq!(pixel(&s, "rock", (5, 1)), RED);
    assert!(s.active_dirty() && !s.dirty(), "the asset is unsaved, the level is not");
    let cmds = run(&mut s, A::Save);
    assert!(
        cmds.iter().any(|c| matches!(c, C::SaveAsset { replace: Some(id), .. } if *id == key.id)),
        "{cmds:?}"
    );
    assert!(!s.active_dirty());
    assert_eq!(s.asset_key(), Some(&key));
    let saved = olr_edit::asset::load_dir(&tmp.path().join("yours").join(&key.id)).unwrap();
    let rock = saved.pieces.iter().find(|p| p.element.id == "rock").unwrap();
    let olr_edit::compose::Decoded::Drawn { art, .. } = &rock.decoded else { panic!("a drawn rock") };
    assert_eq!(art.get(5 * saved.art_scale, saved.art_scale), RED);
}

#[test]
fn a_built_in_asset_saves_as_yours() {
    let Some((tmp, mut s)) = with_library() else { return };
    let before = tree(&built_in_assets().join("ceiling-lamp"));
    run(&mut s, A::EditAsset(built_in("ceiling-lamp")));
    assert_eq!(s.asset_key(), Some(&built_in("ceiling-lamp")), "read-only in place till saved");
    assert_eq!(message(&s), BUILT_IN_NOTE);
    stroke(&mut s, "rock", PaintChoice::Colour(RED), (5, 1));
    let cmds = run(&mut s, A::Save);
    assert!(cmds.iter().any(|c| matches!(c, C::SaveAsset { replace: None, .. })), "{cmds:?}");
    let yours = AssetKey { builtin: false, id: "ceiling-lamp".into() };
    assert_eq!(s.asset_key(), Some(&yours), "the user's copy from now on");
    assert!(!s.active_dirty());
    assert_eq!(tree(&built_in_assets().join("ceiling-lamp")), before, "a built-in folder is never written");
    // Saved again: over the user's copy.
    stroke(&mut s, "rock", PaintChoice::Colour(RED), (8, 1));
    let cmds = run(&mut s, A::Save);
    assert!(cmds.iter().any(|c| matches!(c, C::SaveAsset { replace: Some(id), .. } if id == "ceiling-lamp")));

    // The built-in one again: its name is the user's now, so Replace or Keep
    // both.
    run(&mut s, A::EditAsset(built_in("ceiling-lamp")));
    assert_eq!(s.asset_key(), Some(&built_in("ceiling-lamp")));
    run(&mut s, A::Save);
    assert_eq!(
        s.dialog(),
        Some(&Dialog::AssetExists { name: "Ceiling lamp".into(), id: "ceiling-lamp".into() })
    );
    run(&mut s, A::Choose(Choice::KeepBoth));
    assert_eq!(s.asset_key(), Some(&AssetKey { builtin: false, id: "ceiling-lamp-2".into() }));
    assert_eq!(s.active_document().unwrap().level().name, "Ceiling lamp 2");
    assert!(tmp.path().join("yours/ceiling-lamp-2/asset.ron").is_file());
    assert_eq!(tree(&built_in_assets().join("ceiling-lamp")), before);
}

#[test]
fn new_asset_asks_name_size_and_art_scale() {
    let Some((_tmp, mut s)) = with_library() else { return };
    let art_scale = s.document().unwrap().level().art_scale;
    run(&mut s, A::NewAssetDialog);
    let Some(Dialog::NewAsset(n)) = s.dialog().cloned() else { panic!("{:?}", s.dialog()) };
    assert_eq!(n.art_scale, art_scale, "the open level's art scale at first");
    // An empty name, and a side above MAX_SIDE, are refused and asked again.
    run(&mut s, A::MakeAsset(NewAsset { name: " ".into(), ..n.clone() }));
    assert!(matches!(s.dialog(), Some(Dialog::NewAsset(_))));
    assert!(message(&s).contains("name"), "{}", message(&s));
    run(&mut s, A::MakeAsset(NewAsset { size: (MAX_SIDE + 1, 8), ..n.clone() }));
    assert!(matches!(s.dialog(), Some(Dialog::NewAsset(_))));
    assert!(message(&s).contains("width and height"), "{}", message(&s));
    assert_eq!(s.doc_tab(), DocTab::Level);
    run(&mut s, A::MakeAsset(NewAsset { name: "Crate".into(), size: (20, 10), art_scale: 2 }));
    assert!(s.dialog().is_none());
    assert_eq!(s.doc_tab(), DocTab::Asset);
    let d = s.active_document().unwrap();
    assert_eq!((d.level().size, d.level().art_scale, d.level().name.as_str()), ((20, 10), 2, "Crate"));
    let items = d.items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].element.name, FIRST_LAYER);
    assert!(matches!(
        items[0].element.kind,
        olr_format::levels::ElementKind::Drawn { material: ElementMaterial::Background, .. }
    ));
    assert!(s.active_dirty() && s.asset_key().is_none(), "unsaved till saved");
    let cmds = run(&mut s, A::Save);
    assert!(cmds.iter().any(|c| matches!(c, C::SaveAsset { replace: None, .. })), "{cmds:?}");
    assert_eq!(s.asset_key(), Some(&AssetKey { builtin: false, id: "crate".into() }));
}

#[test]
fn the_tabs_keep_their_own_history_and_unsaved_mark() {
    let Some((tmp, mut s)) = with_library() else { return };
    let key = save_yours(&tmp, &mut s, &lamp());
    run(&mut s, A::EditAsset(key));
    let was = pixel(&s, "rock", (5, 1));
    stroke(&mut s, "rock", PaintChoice::Colour(RED), (5, 1));
    run(&mut s, A::ShowTab(DocTab::Level));
    assert_eq!(s.doc_tab(), DocTab::Level);
    assert!(!s.active_dirty() && !s.can_undo(), "the level's own mark and history");
    let rename = crate::common::rename_level(&s, "Caves");
    run(&mut s, rename);
    assert!(s.dirty() && s.can_undo());
    run(&mut s, A::ShowTab(DocTab::Asset));
    assert_eq!(s.selection(), &Selection::Element("rock".into()), "the asset tab's selection");
    assert_eq!(s.tool(), Tool::Brush);
    run(&mut s, A::Undo);
    assert_eq!(pixel(&s, "rock", (5, 1)), was, "the asset's stroke undone");
    assert!(!s.can_undo());
    assert_eq!(s.document().unwrap().level().name, "Caves", "the level's edit stays");
    assert!(s.dirty());
    run(&mut s, A::ShowTab(DocTab::Level));
    assert!(s.can_undo());
    run(&mut s, A::Undo);
    assert_ne!(s.document().unwrap().level().name, "Caves");
}

#[test]
fn close_asks_for_each_unsaved_tab() {
    let Some((tmp, mut s)) = with_library() else { return };
    let key = save_yours(&tmp, &mut s, &lamp());
    run(&mut s, A::EditAsset(key));
    stroke(&mut s, "rock", PaintChoice::Colour(RED), (5, 1));
    run(&mut s, A::ShowTab(DocTab::Level));
    let rename = crate::common::rename_level(&s, "Caves");
    run(&mut s, rename);
    let cmds = run(&mut s, A::Close);
    assert!(cmds.is_empty());
    assert_eq!(s.dialog(), Some(&Dialog::Unsaved { then: Pending::CloseAsset }), "the asset first");
    let cmds = run(&mut s, A::Choose(Choice::Save));
    assert!(cmds.iter().any(|c| matches!(c, C::SaveAsset { .. })), "{cmds:?}");
    assert_eq!(s.dialog(), Some(&Dialog::Unsaved { then: Pending::Close }), "then the level");
    assert!(s.asset_document().is_none(), "the asset tab is closed");
    let cmds = run(&mut s, A::Choose(Choice::Discard));
    assert!(cmds.contains(&C::Close), "{cmds:?}");

    // Only the asset unsaved: one question; Cancel keeps everything.
    let Some((tmp, mut s)) = with_library() else { return };
    let key = save_yours(&tmp, &mut s, &lamp());
    run(&mut s, A::EditAsset(key));
    stroke(&mut s, "rock", PaintChoice::Colour(RED), (5, 1));
    run(&mut s, A::Close);
    run(&mut s, A::Choose(Choice::Cancel));
    assert!(s.dialog().is_none() && s.asset_dirty());
    run(&mut s, A::Close);
    let cmds = run(&mut s, A::Choose(Choice::Discard));
    assert!(cmds.contains(&C::Close), "{cmds:?}");
    // Nothing unsaved: no question.
    let Some((_tmp, mut s)) = with_library() else { return };
    run(&mut s, A::EditAsset(built_in("torch")));
    assert!(run(&mut s, A::Close).contains(&C::Close));
}

#[test]
fn recovery_is_the_levels() {
    let Some((tmp, mut s)) = with_library() else { return };
    let level_dir = s.document().unwrap().dir().to_path_buf();
    let key = save_yours(&tmp, &mut s, &lamp());
    run(&mut s, A::EditAsset(key));
    stroke(&mut s, "rock", PaintChoice::Colour(RED), (5, 1));
    // Only the asset unsaved: nothing to recover.
    assert!(!s.dirty());
    assert!(!s.document().unwrap().is_asset());
    run(&mut s, A::ShowTab(DocTab::Level));
    let rename = crate::common::rename_level(&s, "Caves");
    run(&mut s, rename);
    run(&mut s, A::ShowTab(DocTab::Asset));
    // Both unsaved, the asset shown: the level's document and mark.
    assert!(s.active_dirty() && s.dirty());
    let doc = s.document().unwrap();
    assert!(!doc.is_asset());
    assert_eq!(doc.dir(), level_dir);
    assert!(s.active_document().unwrap().is_asset());
}

#[test]
fn level_only_tools_are_absent_in_the_asset_tab() {
    let Some((_tmp, mut s)) = with_library() else { return };
    run(&mut s, A::EditAsset(built_in("ceiling-lamp")));
    // Refused by the state too.
    run(&mut s, A::Tool(Tool::Spawn));
    assert_ne!(s.tool(), Tool::Spawn);
    run(&mut s, A::Tool(Tool::Haze));
    assert_ne!(s.tool(), Tool::Haze);
    assert!(run(&mut s, A::Play).is_empty() && !s.playing());
    struct App {
        state: EditorState,
        data: EditorData,
    }
    let mut h = Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                run(&mut app.state, a);
            }
        },
        App { state: s, data: EditorData::default() },
    );
    h.run();
    for absent in [Tool::Spawn.name(), Tool::Haze.name(), labels::PLAY, labels::SPAWNS, "New width"] {
        assert!(h.query_by_label(absent).is_none(), "{absent} shows in the asset tab");
    }
    h.get_by_label(labels::ASSET_SIZE);
    h.get_by_label(labels::WIDTH);
    h.get_by_label(labels::LEVEL_TAB).click();
    h.run();
    assert_eq!(h.state().state.doc_tab(), DocTab::Level);
    for present in [Tool::Spawn.name(), Tool::Haze.name(), labels::PLAY, labels::SPAWNS, "New width"] {
        assert!(h.query_by_label(present).is_some(), "{present} is missing in the level tab");
    }
}

#[test]
fn tile_paints_use_the_open_levels_theme() {
    let Some((_tmp, mut s)) = with_library() else { return };
    let theme = s.document().unwrap().level().theme.clone();
    let tile = s.catalog().themes.iter().find(|t| t.id == theme).unwrap().front_tile.clone();
    run(&mut s, A::EditAsset(built_in("ceiling-lamp")));
    let d = s.active_document().unwrap();
    assert_eq!(d.level().theme, theme);
    assert_eq!(d.front_tile(), &tile);
    // A level's holes are the crater brushes here too.
    assert!(!s.holes().is_empty());
    stroke(&mut s, "rock", PaintChoice::FrontTile, (5, 1));
    let sc = s.active_document().unwrap().level().art_scale;
    let (ax, ay) = (5 * sc, sc);
    assert_eq!(pixel(&s, "rock", (5, 1)), tile.get(ax % tile.width(), ay % tile.height()));
}

/// The ceiling lamp with states Normal and Broken (its own dark picture
/// of the lamps, their glow off, a looped hum), an exposed glow strength
/// and a script.
fn broken_lamp() -> Asset {
    let mut a = lamp();
    olr_edit::asset::add_sound(&mut a, "hum.wav", wav()).unwrap();
    olr_edit::asset::add_state(&mut a, "Broken", None).unwrap();
    let lamps = a.pieces.iter().find(|p| p.element.id == "lamps").unwrap();
    let mut decoded = lamps.decoded.clone();
    if let olr_edit::compose::Decoded::Drawn { art, .. } = &mut decoded {
        for y in 0..art.height() {
            for x in 0..art.width() {
                if art.get(x, y).a != 0 {
                    art.set(x, y, Rgba::rgb(20, 20, 20));
                }
            }
        }
    }
    olr_edit::asset::own_picture(&mut a, "Broken", "lamps", OwnPicture { decoded, glow_mask: None }).unwrap();
    olr_edit::asset::set_state_value(&mut a, "Broken", P::GlowStrength("lamps".into()), Some(V::Int(0)))
        .unwrap();
    olr_edit::asset::set_state_sounds(&mut a, "Broken", Some(Some("hum.wav".into())), None).unwrap();
    let x = ExposedSource {
        name: "lamp_glow".into(),
        property: P::GlowStrength("lamps".into()),
        default: Some(V::Int(80)),
        min: None,
        max: None,
    };
    olr_edit::asset::expose(&mut a, x).unwrap();
    olr_edit::asset::set_script(&mut a, Some("// the lamp\n".into())).unwrap();
    a
}

/// A short WAV.
fn wav() -> Vec<u8> {
    let data = 800u32;
    let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 1, 0]);
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 8, 0]);
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 0x80);
    b
}

#[test]
fn an_asset_with_states_keeps_them_through_open_and_save() {
    let Some((tmp, mut s)) = with_library() else { return };
    let key = save_yours(&tmp, &mut s, &broken_lamp());
    let dir = tmp.path().join("yours").join(&key.id);
    let before = olr_edit::asset::read_dir(&dir).unwrap();
    run(&mut s, A::EditAsset(key.clone()));
    assert_eq!(s.active_document().unwrap().asset_parts().unwrap().states.len(), 2);
    stroke(&mut s, "rock", PaintChoice::Colour(RED), (5, 1));
    assert!(s.can_undo(), "a stroke on the start is not refused: {}", message(&s));
    run(&mut s, A::Save);
    assert!(s.dialog().is_none(), "{:?}", s.dialog());
    let after = olr_edit::asset::read_dir(&dir).unwrap();
    let (b, a) = (&before.file, &after.file);
    assert_eq!(
        (&a.states, &a.start, &a.exposed, &a.sounds, &a.health),
        (&b.states, &b.start, &b.exposed, &b.sounds, &b.health)
    );
    assert_eq!(after.sounds, before.sounds, "the sounds byte for byte");
    assert_eq!(after.script, before.script);
    // The lamps' stacked pictures (the Broken one among them) as they were;
    // the rock painted.
    let lamps = olr_format::assets::asset_files(b.elements.iter().find(|e| e.id == "lamps").unwrap());
    for f in &lamps {
        assert_eq!(after.files.get(f), before.files.get(f), "{f}");
    }
    let rock = olr_format::assets::asset_files(b.elements.iter().find(|e| e.id == "rock").unwrap());
    assert!(rock.iter().any(|f| after.files.get(f) != before.files.get(f)));
}

/// The ceiling lamp whose lamps are directional glass, their colour, beam
/// and opacity exposed, a state Dim setting their glass's opacity.
fn exposed_lamp() -> Asset {
    let mut a = lamp();
    let lamps = a.pieces.iter_mut().find(|p| p.element.id == "lamps").unwrap();
    lamps.element.glass = Some(ElementGlass { opacity: 50, tint: 0, frost: 0 });
    if let Some(g) = lamps.element.glow.as_mut() {
        g.beam = Some(GlowBeam { direction: 90, spread: 35 });
    }
    for (name, property) in [
        ("lamp_colour", P::GlowColour("lamps".into())),
        ("beam", P::BeamDirection("lamps".into())),
        ("lamp_opacity", P::Opacity("lamps".into())),
    ] {
        a.exposed.push(ExposedSource { name: name.into(), property, default: None, min: None, max: None });
    }
    olr_edit::asset::add_state(&mut a, "Dim", None).unwrap();
    a.states[1].values.push(StateValue { property: P::GlassOpacity("lamps".into()), value: V::Int(20) });
    a
}

/// `action` refused: the asset tab's document as it was, no step, the
/// status line saying `says`.
fn refused(s: &mut EditorState, action: A, says: &str) {
    let before = s.active_document().unwrap().to_asset();
    let dirty = s.active_dirty();
    run(s, action);
    assert!(message(s).contains(says), "{:?} does not say {says:?}", message(s));
    assert_eq!(s.active_document().unwrap().to_asset(), before, "the document is unchanged");
    assert!(!s.can_undo() && s.active_dirty() == dirty);
}

#[test]
fn edits_that_break_exposure_or_states_are_refused_with_the_reason() {
    let Some((tmp, mut s)) = with_library() else { return };
    let key = save_yours(&tmp, &mut s, &exposed_lamp());
    run(&mut s, A::EditAsset(key));
    let glow = s.active_document().unwrap().to_asset().pieces[1].element.glow.clone().unwrap();
    run(&mut s, A::Select(Selection::Element("lamps".into())));
    refused(&mut s, A::Remove, "lamps is used by exposed lamp_colour, beam, lamp_opacity: unexpose it first");
    refused(&mut s, A::Split("lamps".into()), "lamps is used by exposed lamp_colour");
    refused(&mut s, A::MergeDown("lamps".into()), "lamps");
    refused(&mut s, A::SetGlow { id: "lamps".into(), glow: None }, "lamps is used by exposed lamp_colour");
    refused(&mut s, A::Set("lamps".into(), Property::Glass(None)), "lamps is set by state Dim");
    let flat = olr_format::levels::ElementGlow { beam: None, ..glow };
    refused(&mut s, A::SetGlow { id: "lamps".into(), glow: Some(flat) }, "lamps is used by exposed beam:");
    assert_eq!(s.selection(), &Selection::Element("lamps".into()));
    // What breaks nothing still lands.
    run(&mut s, A::Set("lamps".into(), Property::Name("Bulbs".into())));
    assert!(s.can_undo(), "{}", message(&s));

    // A Background layer whose opacity is exposed stays background.
    let mut a = lamp();
    let property = P::Opacity("lamps".into());
    a.exposed.push(ExposedSource {
        name: "lamp_opacity".into(),
        property,
        default: None,
        min: None,
        max: None,
    });
    let key = save_yours(&tmp, &mut s, &a);
    run(&mut s, A::EditAsset(key));
    assert!(matches!(s.dialog(), Some(Dialog::Unsaved { then: Pending::EditAsset { .. } })), "the rename");
    run(&mut s, A::Choose(Choice::Discard));
    refused(
        &mut s,
        A::Set("lamps".into(), Property::Material(ElementMaterial::Dirt)),
        "lamps is used by exposed lamp_opacity: unexpose it first",
    );

    // Issue #79 carries a state's own picture along: a move and a resize
    // land; merge and split are refused; a removal still meets the
    // exposure's refusal.
    let key = save_yours(&tmp, &mut s, &broken_lamp());
    run(&mut s, A::EditAsset(key));
    assert!(s.dialog().is_none(), "nothing unsaved: no question");
    assert_eq!(s.active_document().unwrap().asset_parts().unwrap().states.len(), 2);
    run(&mut s, A::Select(Selection::Element("lamps".into())));
    refused(&mut s, A::Remove, "lamps is used by exposed lamp_glow: unexpose it first");
    refused(&mut s, A::Split("lamps".into()), "Broken paints");
    run(&mut s, A::Set("lamps".into(), Property::At((3, 2))));
    assert!(s.can_undo(), "{}", message(&s));
    run(&mut s, A::Undo);
    run(
        &mut s,
        A::Resize { size: (20, 8), offset: (0, 0), material: Material::Background, colour: Rgba::BLACK },
    );
    assert_eq!(s.active_document().unwrap().level().size, (20, 8), "{}", message(&s));
    run(&mut s, A::Undo);
    // Painting it is no geometry.
    stroke(&mut s, "lamps", PaintChoice::Colour(RED), (5, 4));
    assert!(s.can_undo(), "{}", message(&s));

    // A built-in asset without states is never refused.
    run(&mut s, A::EditAsset(built_in("ceiling-lamp")));
    run(&mut s, A::Choose(Choice::Discard));
    run(&mut s, A::Select(Selection::Element("lamps".into())));
    run(&mut s, A::Remove);
    assert_eq!(s.active_document().unwrap().items().len(), 1);
}

#[test]
fn resizing_an_asset_is_one_step() {
    let Some((tmp, mut s)) = with_library() else { return };
    let key = save_yours(&tmp, &mut s, &lamp());
    run(&mut s, A::EditAsset(key));
    let was = pixel(&s, "rock", (5, 1));
    run(
        &mut s,
        A::Resize { size: (20, 9), offset: (0, 0), material: Material::Background, colour: Rgba::BLACK },
    );
    let d = s.active_document().unwrap();
    assert_eq!(d.level().size, (20, 9));
    assert_eq!(pixel(&s, "rock", (5, 1)), was, "from the top-left");
    run(&mut s, A::Undo);
    assert_eq!(s.active_document().unwrap().level().size, (13, 6));
    assert!(!s.can_undo(), "one step");
}
