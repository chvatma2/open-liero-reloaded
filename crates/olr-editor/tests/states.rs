//! The asset tab's states (issue #79): the states panel's operations as
//! steps, painting a layer in a state making it the state's own, the
//! canvas showing a state where things are, the own pictures carried
//! along by layer edits, the refusals, and the states saved.

use olr_data::image::Rgba;
use olr_data::level::Material;
use olr_edit::asset::{Asset, OwnPicture};
use olr_edit::compose::Decoded;
use olr_edit::element::Property;
use olr_editor::state_ops::{
    HEALTH_ENDS_HERE, IMAGE_GLOW_IN_STATE, IMAGE_IN_STATE, START_STAYS, SURFACE_IN_STATE, StatePictures,
};
use olr_editor::tools::{PaintChoice, PointerKind, Tool};
use olr_editor::{AssetKey, Choice, Dialog, EditorAction as A, EditorState, Selection};
use olr_format::assets::{HealthSource, PropertySource as P, ValueSource as V};

use crate::common::{built_in_assets, content, editor, run, set_library};

pub(crate) const RED: Rgba = Rgba::rgb(255, 0, 0);
const DARK: Rgba = Rgba::rgb(20, 20, 20);

/// The built-in ceiling lamp (`rock` at the top, glowing `lamps` under it,
/// boxed at (2, 3), 9 × 3).
pub(crate) fn lamp() -> Asset {
    olr_edit::asset::load_dir(&built_in_assets().join("ceiling-lamp")).unwrap()
}

/// `piece`'s picture with every covered pixel `c`.
fn painted(a: &Asset, id: &str, c: Rgba) -> OwnPicture {
    let p = a.pieces.iter().find(|p| p.element.id == id).unwrap();
    let mut decoded = p.decoded.clone();
    if let Decoded::Drawn { art, .. } = &mut decoded {
        for y in 0..art.height() {
            for x in 0..art.width() {
                if art.get(x, y).a != 0 {
                    art.set(x, y, c);
                }
            }
        }
    }
    OwnPicture { decoded, glow_mask: None }
}

/// The lamp with an Alarm state painting its lamps red and an Off state
/// painting them dark, unlit (its glow value 0).
pub(crate) fn alarm_lamp() -> Asset {
    let mut a = lamp();
    olr_edit::asset::add_state(&mut a, "Alarm", None).unwrap();
    olr_edit::asset::add_state(&mut a, "Off", None).unwrap();
    let red = painted(&a, "lamps", RED);
    olr_edit::asset::own_picture(&mut a, "Alarm", "lamps", red).unwrap();
    let dark = painted(&a, "lamps", DARK);
    olr_edit::asset::own_picture(&mut a, "Off", "lamps", dark).unwrap();
    olr_edit::asset::set_state_value(&mut a, "Off", P::GlowStrength("lamps".into()), Some(V::Int(0)))
        .unwrap();
    a
}

/// The fixture open, `asset` saved in the user's library and opened in the
/// asset tab.
pub(crate) fn editing(asset: &Asset) -> Option<(tempfile::TempDir, EditorState, AssetKey)> {
    let root = content()?;
    let (tmp, mut s) = editor(&root);
    let yours = tmp.path().join("yours");
    std::fs::create_dir_all(&yours).unwrap();
    let id = olr_edit::asset::add_to_library(&yours, asset, None).unwrap();
    run(&mut s, A::AssetsChanged(set_library(&built_in_assets(), &yours)));
    let key = AssetKey { builtin: false, id };
    run(&mut s, A::EditAsset(key.clone()));
    assert!(s.active_document().is_some_and(|d| d.is_asset()));
    Some((tmp, s, key))
}

/// A brush stroke of `paint` on layer `id` at world pixel `at`.
pub(crate) fn stroke(s: &mut EditorState, id: &str, paint: Rgba, at: (i32, i32)) {
    run(s, A::Select(Selection::Element(id.into())));
    run(s, A::Tool(Tool::Brush));
    run(s, A::Paint(PaintChoice::Colour(paint)));
    for kind in [PointerKind::Down, PointerKind::Move, PointerKind::Up] {
        run(s, A::Pointer { kind, world: at, shift: false, toggle: false });
    }
}

/// Layer `id`'s art pixel at world pixel `at` of the shown document (its
/// box's top-left at `box_at`).
fn pixel(s: &EditorState, id: &str, at: (u32, u32), box_at: (u32, u32)) -> Rgba {
    let d = s.active_document().unwrap();
    let sc = d.level().art_scale;
    d.layer_art(id).unwrap().get((at.0 - box_at.0) * sc, (at.1 - box_at.1) * sc)
}

fn message(s: &EditorState) -> String {
    s.status().message.clone().unwrap_or_default()
}

/// What the canvas shows: the bake's material, front, back and glow.
fn canvas(s: &EditorState) -> (Vec<Material>, Vec<u8>, Vec<u8>, Option<Vec<u8>>) {
    let l = s.active_document().unwrap().level();
    (
        l.material.clone(),
        l.front.as_raw().to_vec(),
        l.back.as_raw().to_vec(),
        l.glow.as_ref().map(|g| g.front.clone()),
    )
}

fn shown(s: &EditorState) -> usize {
    s.active_document().unwrap().shown_state().unwrap()
}

fn owns(s: &EditorState, id: &str) -> bool {
    s.active_document().unwrap().shown_owns(id)
}

fn names(s: &EditorState) -> Vec<String> {
    s.active_document().unwrap().states().iter().map(|st| st.name.clone()).collect()
}

#[test]
fn a_new_state_looks_like_the_start() {
    let Some((_tmp, mut s, _)) = editing(&lamp()) else { return };
    let start = canvas(&s);
    run(&mut s, A::AddState);
    assert_eq!(names(&s), ["Normal", "State 1"]);
    assert_eq!(shown(&s), 1, "the new state is shown");
    assert_eq!(canvas(&s), start, "it looks like the start");
    let a = s.active_document().unwrap().to_asset();
    let pose = |i| olr_edit::asset::preview_state(&a, i, false);
    assert_eq!(pose(1), pose(0));
    assert!(a.states[1].own.is_empty() && a.states[1].values == a.states[0].values);
    run(&mut s, A::Undo);
    assert_eq!((names(&s), shown(&s)), (vec!["Normal".to_string()], 0), "one step");
}

#[test]
fn painting_in_a_state_owns_the_layer_in_one_step() {
    let Some((_tmp, mut s, _)) = editing(&lamp()) else { return };
    let was = pixel(&s, "lamps", (5, 4), (2, 3));
    run(&mut s, A::AddState);
    stroke(&mut s, "lamps", RED, (5, 4));
    assert!(owns(&s, "lamps"), "{}", message(&s));
    assert_eq!(pixel(&s, "lamps", (5, 4), (2, 3)), RED);
    // The start keeps its picture.
    run(&mut s, A::SelectState(0));
    assert_eq!(pixel(&s, "lamps", (5, 4), (2, 3)), was);
    run(&mut s, A::Undo);
    assert_eq!(shown(&s), 1, "undo shows the state the stroke was made in");
    assert!(!owns(&s, "lamps"), "it follows the start again");
    assert_eq!(pixel(&s, "lamps", (5, 4), (2, 3)), was);
    run(&mut s, A::Redo);
    assert!(owns(&s, "lamps") && pixel(&s, "lamps", (5, 4), (2, 3)) == RED);
}

#[test]
fn a_stroke_changing_nothing_does_not_own() {
    let Some((_tmp, mut s, _)) = editing(&lamp()) else { return };
    run(&mut s, A::AddState);
    // Far off the lamps' box: nothing painted.
    stroke(&mut s, "lamps", RED, (-40, -40));
    assert!(!owns(&s, "lamps"));
    run(&mut s, A::Undo);
    assert_eq!(names(&s).len(), 1, "the only step was Add state");
}

#[test]
fn the_canvas_ignores_position_values_and_the_preview_does_not() {
    let mut a = lamp();
    olr_edit::asset::add_state(&mut a, "Moved", None).unwrap();
    for (p, v) in [(P::OffsetX("lamps".into()), V::Int(2)), (P::GlowStrength("lamps".into()), V::Int(0))] {
        olr_edit::asset::set_state_value(&mut a, "Moved", p, Some(v)).unwrap();
    }
    let Some((_tmp, mut s, _)) = editing(&a) else { return };
    let start = canvas(&s);
    assert!(start.3.is_some(), "the lamps glow at the start");
    run(&mut s, A::SelectState(1));
    let moved = canvas(&s);
    assert_eq!((&moved.0, &moved.1, &moved.2), (&start.0, &start.1, &start.2), "nothing moves on the canvas");
    assert_ne!(moved.3, start.3, "its glow value applies: the lamps are dark");
    let asset = s.active_document().unwrap().to_asset();
    let (p0, p1) =
        (olr_edit::asset::preview_state(&asset, 0, false), olr_edit::asset::preview_state(&asset, 1, false));
    assert_ne!(p0, p1, "the preview moves them");
}

#[test]
fn images_the_glow_brush_on_images_and_the_surface_brush_are_refused_in_a_state() {
    let Some((_tmp, mut s, _)) = editing(&lamp()) else { return };
    let bytes = std::fs::read(crate::common::fixture().join("editor/images/gradient.jpg")).unwrap();
    run(&mut s, A::ImportImage { file_name: "gradient.jpg".into(), bytes });
    let Selection::Element(image) = s.selection().clone() else { panic!("the import is selected") };
    run(&mut s, A::AddState);
    let refused = |s: &mut EditorState, tool: Tool, id: &str, says: &str| {
        let before = s.active_document().unwrap().to_asset();
        run(s, A::Select(Selection::Element(id.into())));
        run(s, A::Tool(tool));
        for kind in [PointerKind::Down, PointerKind::Move, PointerKind::Up] {
            run(s, A::Pointer { kind, world: (6, 3), shift: false, toggle: false });
        }
        assert_eq!(message(s), says);
        assert_eq!(s.active_document().unwrap().to_asset(), before, "{says}: nothing changed");
    };
    refused(&mut s, Tool::Brush, &image, IMAGE_IN_STATE);
    refused(&mut s, Tool::Eraser, &image, IMAGE_IN_STATE);
    refused(&mut s, Tool::Glow, &image, IMAGE_GLOW_IN_STATE);
    refused(&mut s, Tool::Surface, "lamps", SURFACE_IN_STATE);
    // The Glow brush on a drawn layer owns it, with its own mask.
    run(&mut s, A::Select(Selection::Element("lamps".into())));
    run(&mut s, A::Tool(Tool::Glow));
    run(&mut s, A::GlowStrength(30));
    for kind in [PointerKind::Down, PointerKind::Move, PointerKind::Up] {
        run(&mut s, A::Pointer { kind, world: (5, 4), shift: false, toggle: false });
    }
    assert!(owns(&s, "lamps"), "{}", message(&s));
    let a = s.active_document().unwrap().to_asset();
    let own = &a.states[1].own["lamps"];
    let start = a.pieces.iter().find(|p| p.element.id == "lamps").unwrap();
    assert!(own.glow_mask.is_some() && own.glow_mask != start.glow_mask, "its own mask");
    assert!(start.glow_mask.as_ref().is_some_and(|m| m.iter().all(|v| *v == 255)), "the start's whole");
    // In the start they all paint.
    run(&mut s, A::SelectState(0));
    run(&mut s, A::Select(Selection::Element(image.clone())));
    run(&mut s, A::Tool(Tool::Eraser));
    for kind in [PointerKind::Down, PointerKind::Move, PointerKind::Up] {
        run(&mut s, A::Pointer { kind, world: (6, 3), shift: false, toggle: false });
    }
    assert_ne!(message(&s), IMAGE_IN_STATE);
}

#[test]
fn only_changed_state_pictures_are_recomputed() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    let mut pictures = StatePictures::default();
    let key = s.shown_generation();
    assert_eq!(pictures.refresh(s.active_document().unwrap(), key), [0, 1, 2]);
    assert_eq!(pictures.made(), 3);
    run(&mut s, A::SelectState(2));
    assert!(pictures.refresh(s.active_document().unwrap(), key).is_empty(), "selecting recomputes nothing");
    stroke(&mut s, "lamps", RED, (5, 4));
    assert_eq!(pictures.refresh(s.active_document().unwrap(), key), [2], "a stroke in Off: Off's");
    // In the start, the rock every state follows: all of them.
    run(&mut s, A::SelectState(0));
    assert!(pictures.refresh(s.active_document().unwrap(), key).is_empty());
    stroke(&mut s, "rock", RED, (5, 1));
    assert_eq!(pictures.refresh(s.active_document().unwrap(), key), [0, 1, 2]);
    // The start's lamps, which Alarm and Off paint themselves: the start's.
    stroke(&mut s, "lamps", RED, (5, 4));
    assert_eq!(pictures.refresh(s.active_document().unwrap(), key), [0]);
    assert_eq!(pictures.made(), 8, "never all of them on every release");
}

#[test]
fn a_state_shows_the_same_whatever_came_before() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    run(&mut s, A::SelectState(1));
    run(&mut s, A::SelectState(2));
    let from_alarm = canvas(&s);
    run(&mut s, A::SelectState(0));
    run(&mut s, A::SelectState(2));
    assert_eq!(canvas(&s), from_alarm);
    assert_eq!(pixel(&s, "lamps", (5, 4), (2, 3)), DARK);
    assert!(!s.can_undo(), "selecting is no step");
    assert!(!s.active_dirty());
}

#[test]
fn follow_start_drops_the_picture() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    let start = pixel(&s, "lamps", (5, 4), (2, 3));
    run(&mut s, A::SelectState(1));
    assert!(owns(&s, "lamps"));
    run(&mut s, A::FollowStart("lamps".into()));
    assert!(!owns(&s, "lamps"), "{}", message(&s));
    assert_eq!(pixel(&s, "lamps", (5, 4), (2, 3)), start);
    assert!(s.active_document().unwrap().to_asset().states[1].own.is_empty());
    run(&mut s, A::Undo);
    assert!(owns(&s, "lamps") && pixel(&s, "lamps", (5, 4), (2, 3)) == RED);
}

#[test]
fn moving_and_resizing_a_layer_relays_every_states_picture() {
    // A new asset's one layer is the asset's size: a resize re-lays it.
    let Some((_tmp, mut s, _)) = editing(&lamp()) else { return };
    run(&mut s, A::NewAssetDialog);
    let n = olr_editor::asset_tab::NewAsset { name: "Sign".into(), size: (12, 8), art_scale: 1 };
    run(&mut s, A::MakeAsset(n));
    if matches!(s.dialog(), Some(Dialog::Unsaved { .. })) {
        run(&mut s, A::Choose(Choice::Discard));
    }
    let layer = s.active_document().unwrap().items()[0].element.id.clone();
    stroke(&mut s, &layer, RED, (3, 3));
    run(&mut s, A::AddState);
    stroke(&mut s, &layer, Rgba::rgb(0, 0, 255), (5, 5));
    assert!(owns(&s, &layer));
    let resize =
        A::Resize { size: (16, 10), offset: (2, 1), material: Material::Background, colour: Rgba::BLACK };
    run(&mut s, resize);
    assert_eq!(s.active_document().unwrap().level().size, (16, 10), "{}", message(&s));
    let a = s.active_document().unwrap().to_asset();
    let Decoded::Drawn { art, .. } = &a.states[1].own[&layer].decoded else { panic!() };
    assert_eq!((art.width(), art.height()), (16, 10), "the state's picture follows the layer");
    assert_eq!(art.get(7, 6), Rgba::rgb(0, 0, 255), "moved by the offset");
    let Decoded::Drawn { art, .. } = &a.pieces[0].decoded else { panic!() };
    assert_eq!(art.get(5, 4), RED);
    // Moving the layer moves every state's picture with it.
    run(&mut s, A::Set(layer.clone(), Property::At((1, 0))));
    assert_eq!(pixel(&s, &layer, (8, 6), (1, 0)), Rgba::rgb(0, 0, 255), "{}", message(&s));
    run(&mut s, A::SelectState(0));
    assert_eq!(pixel(&s, &layer, (6, 4), (1, 0)), RED);
    run(&mut s, A::Undo);
    run(&mut s, A::Undo);
    assert_eq!(s.active_document().unwrap().level().size, (12, 8));
    let a = s.active_document().unwrap().to_asset();
    let Decoded::Drawn { art, .. } = &a.states[1].own[&layer].decoded else { panic!() };
    assert_eq!((art.width(), art.get(5, 5)), (12, Rgba::rgb(0, 0, 255)));
}

#[test]
fn removing_a_layer_forgets_it_in_every_state_and_undo_restores() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    let before = s.active_document().unwrap().to_asset();
    run(&mut s, A::SelectState(1));
    run(&mut s, A::Select(Selection::Element("lamps".into())));
    run(&mut s, A::Remove);
    assert_eq!(
        message(&s),
        "removed with it: Alarm's picture of Lamps, Off's picture of Lamps, Off's lamps.glow"
    );
    let a = s.active_document().unwrap().to_asset();
    assert!(a.states.iter().all(|st| st.own.is_empty()));
    run(&mut s, A::Undo);
    assert_eq!(s.active_document().unwrap().to_asset(), before);
    assert!(owns(&s, "lamps") && pixel(&s, "lamps", (5, 4), (2, 3)) == RED, "Alarm shown, its picture back");
}

#[test]
fn merge_and_split_refuse_owned_layers() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    let before = s.active_document().unwrap().to_asset();
    for action in [A::MergeDown("lamps".into()), A::Split("lamps".into())] {
        run(&mut s, action);
        assert_eq!(message(&s), "Alarm paints Lamps itself: make it follow the start first");
        assert_eq!(s.active_document().unwrap().to_asset(), before);
    }
    // Onto a layer a state owns.
    let mut a = lamp();
    olr_edit::asset::add_state(&mut a, "Cracked", None).unwrap();
    let pic = painted(&a, "rock", DARK);
    olr_edit::asset::own_picture(&mut a, "Cracked", "rock", pic).unwrap();
    let Some((_tmp, mut s, _)) = editing(&a) else { return };
    run(&mut s, A::MergeDown("lamps".into()));
    assert_eq!(message(&s), "Cracked paints Rock itself: make it follow the start first");
    assert!(!s.can_undo());
}

#[test]
fn make_start_keeps_every_state_and_undoes() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    let looks = |s: &mut EditorState| {
        let mut out = Vec::new();
        for i in 0..3 {
            run(s, A::SelectState(i));
            out.push(canvas(s));
        }
        let a = s.active_document().unwrap().to_asset();
        let previews: Vec<_> = (0..3).map(|i| olr_edit::asset::preview_state(&a, i, true)).collect();
        (out, previews)
    };
    let before = looks(&mut s);
    run(&mut s, A::SelectState(2));
    run(&mut s, A::MakeStart);
    let d = s.active_document().unwrap();
    assert_eq!((d.start_state(), d.shown_state()), (Some(2), Some(2)), "{}", message(&s));
    assert_eq!(looks(&mut s), before, "every state looks as it did");
    run(&mut s, A::Undo);
    assert_eq!(s.active_document().unwrap().start_state(), Some(0));
    assert_eq!(looks(&mut s), before);
    assert_eq!(s.active_document().unwrap().to_asset(), alarm_lamp_as_opened());
}

/// The asset as the library holds it (`alarm_lamp`, saved and read).
fn alarm_lamp_as_opened() -> Asset {
    let tmp = tempfile::tempdir().unwrap();
    let id = olr_edit::asset::add_to_library(tmp.path(), &alarm_lamp(), None).unwrap();
    olr_edit::asset::load_dir(&tmp.path().join(id)).unwrap()
}

#[test]
fn remove_state_asks_and_is_refused_for_the_start_and_health() {
    let mut a = alarm_lamp();
    olr_edit::asset::set_health(&mut a, Some(HealthSource { points: 50, at_zero: "Off".into() })).unwrap();
    let Some((_tmp, mut s, _)) = editing(&a) else { return };
    run(&mut s, A::RemoveState);
    assert_eq!((message(&s), s.dialog()), (START_STAYS.to_string(), None));
    run(&mut s, A::SelectState(2));
    run(&mut s, A::RemoveState);
    assert_eq!((message(&s), s.dialog()), (HEALTH_ENDS_HERE.to_string(), None));
    run(&mut s, A::SelectState(1));
    run(&mut s, A::RemoveState);
    assert_eq!(s.dialog(), Some(&Dialog::RemoveState { index: 1, name: "Alarm".into() }));
    run(&mut s, A::Choose(Choice::Cancel));
    assert_eq!(names(&s).len(), 3);
    run(&mut s, A::RemoveState);
    run(&mut s, A::Choose(Choice::Confirm));
    assert_eq!(names(&s), ["Normal", "Off"]);
    assert_eq!(shown(&s), 0, "the start shown");
    run(&mut s, A::Undo);
    assert_eq!(names(&s), ["Normal", "Alarm", "Off"]);
    assert_eq!(shown(&s), 1, "Alarm back and shown");
    assert!(owns(&s, "lamps") && pixel(&s, "lamps", (5, 4), (2, 3)) == RED);
}

#[test]
fn renaming_refuses_duplicates() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    run(&mut s, A::SelectState(1));
    run(&mut s, A::RenameState("normal".into()));
    assert!(message(&s).contains("has a state \"Normal\" already"), "{}", message(&s));
    run(&mut s, A::RenameState("bad/name".into()));
    assert!(message(&s).contains("a state's name is"), "{}", message(&s));
    assert!(!s.can_undo());
    run(&mut s, A::RenameState("Siren".into()));
    assert_eq!(names(&s), ["Normal", "Siren", "Off"]);
    assert_eq!(shown(&s), 1);
    run(&mut s, A::Undo);
    assert_eq!(names(&s)[1], "Alarm");
}

#[test]
fn states_survive_save_and_reopen() {
    let Some((tmp, mut s, key)) = editing(&lamp()) else { return };
    run(&mut s, A::AddState);
    run(&mut s, A::RenameState("Off".into()));
    stroke(&mut s, "lamps", DARK, (5, 4));
    run(&mut s, A::Save);
    let folder = tmp.path().join("yours").join(&key.id);
    // The lamps' file stacks the start's picture over Off's.
    let png = std::fs::read(folder.join("layers/lamps.png")).unwrap();
    let img = olr_format::image_io::decode_rgba_within(&png, olr_format::image_io::LEVEL_ART).unwrap();
    assert_eq!(img.height(), 2 * 3 * lamp().art_scale, "two pictures stacked");
    let saved = olr_edit::asset::load_dir(&folder).unwrap();
    assert_eq!(saved.states.len(), 2);
    assert_eq!(saved, s.active_document().unwrap().to_asset());
    let Decoded::Drawn { art, .. } = &saved.states[1].own["lamps"].decoded else { panic!() };
    let sc = saved.art_scale;
    assert_eq!(art.get(3 * sc, sc), DARK);
}

/// A pixel rectangle selected from world pixel `from` to `to` on `id`.
fn select_pixels(s: &mut EditorState, id: &str, from: (i32, i32), to: (i32, i32)) {
    run(s, A::Select(Selection::Element(id.into())));
    run(s, A::Tool(Tool::SelectPixels));
    for (kind, world) in [(PointerKind::Down, from), (PointerKind::Move, to), (PointerKind::Up, to)] {
        run(s, A::Pointer { kind, world, shift: false, toggle: false });
    }
}

#[test]
fn copy_cut_and_paste_take_the_states_pictures() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    run(&mut s, A::SelectState(1));
    select_pixels(&mut s, "lamps", (2, 3), (4, 5));
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    let Selection::Element(id) = s.selection().clone() else { panic!("{:?}", s.selection()) };
    let d = s.active_document().unwrap();
    assert_eq!(d.layer_art(&id).unwrap().get(0, 0), RED, "Alarm's lamps copied: {}", message(&s));
    // A cut in Alarm erases Alarm's rock, making it Alarm's own; the start's
    // stays.
    let rock = d.layer_art("rock").unwrap().clone();
    let sc = d.level().art_scale;
    let (x, y) = (0..rock.height() / sc)
        .flat_map(|y| (0..rock.width() / sc).map(move |x| (x, y)))
        .find(|&(x, y)| rock.get(x * sc, y * sc).a != 0)
        .expect("rock has pixels");
    select_pixels(&mut s, "rock", (x as i32, y as i32), (x as i32, y as i32));
    run(&mut s, A::Cut);
    assert!(owns(&s, "rock"), "{}", message(&s));
    assert_eq!(s.active_document().unwrap().layer_art("rock").unwrap().get(x * sc, y * sc).a, 0);
    run(&mut s, A::SelectState(0));
    assert!(s.active_document().unwrap().layer_art("rock").unwrap() == &rock, "the start's rock kept");
    run(&mut s, A::Undo);
    assert_eq!(shown(&s), 1, "undo shows Alarm");
    assert!(!owns(&s, "rock"));
}
