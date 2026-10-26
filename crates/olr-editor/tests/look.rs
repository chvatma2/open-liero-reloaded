//! Look editing in the editor's document and state (plan 15a Task 17):
//! glow, its groups and brush, lights, the look, the view and the clock.

use olr_edit::element::Property;
use olr_editor::state::ClockAction;
use olr_editor::tools::{PointerKind, Tool};
use olr_editor::{Choice, Dialog, EditorAction as A, EditorCommand as C, Selection};
use olr_format::levels::{ElementGlow, LevelObject};
use olr_format::look::{LookSource, ReachSource};
use olr_look::ViewMode;

use crate::common::{catalog, content, editor, editor_at, run};

fn upload(cmds: &[C]) -> bool {
    cmds.iter().any(|c| matches!(c, C::Upload(_)))
}

/// A glow on the fixture's `tunnels` layer, its group left to the editor.
fn glow_tunnels(s: &mut olr_editor::EditorState) -> Vec<C> {
    let glow = ElementGlow { strength: 100, group: String::new(), mask: false, beam: None };
    run(s, A::SetGlow { id: "tunnels".into(), glow: Some(glow) })
}

fn groups(s: &olr_editor::EditorState) -> Vec<String> {
    let doc = s.document().unwrap();
    doc.level().look.iter().flat_map(|l| l.glow_groups.iter().map(|g| g.id.clone())).collect()
}

fn element_glow(s: &olr_editor::EditorState, id: &str) -> Option<ElementGlow> {
    let doc = s.document().unwrap();
    doc.items()[doc.index_of(id).unwrap()].element.glow.clone()
}

#[test]
fn a_new_glow_makes_a_group() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    assert!(upload(&glow_tunnels(&mut s)));
    assert_eq!(groups(&s), ["tunnels"]);
    assert_eq!(element_glow(&s, "tunnels").unwrap().group, "tunnels");
    assert!(s.document().unwrap().level().glow.is_some(), "the bake glows");
    // One step: undo takes the glow and its group.
    run(&mut s, A::Undo);
    assert!(groups(&s).is_empty());
    assert_eq!(element_glow(&s, "tunnels"), None);
    assert!(s.document().unwrap().level().glow.is_none());
}

#[test]
fn glow_brush_paints_the_mask_in_one_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    glow_tunnels(&mut s);
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Glow));
    run(&mut s, A::GlowStrength(0));
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    use PointerKind::{Down, Move, Up};
    assert!(upload(&run(&mut s, at((100, 100), Down))));
    run(&mut s, at((140, 100), Move));
    run(&mut s, at((140, 100), Up));
    let doc = s.document().unwrap();
    let a = doc.level().art_scale;
    let width = doc.level().size.0 * a;
    let mask = doc.glow_mask("tunnels").expect("a painted mask");
    assert_eq!(mask[(100 * a * width + 120 * a) as usize], 0, "erased along the stroke");
    assert_eq!(mask[(10 * a * width + 10 * a) as usize], 255, "kept elsewhere");
    assert!(element_glow(&s, "tunnels").unwrap().mask);
    // One step: the mask and its flag go together.
    run(&mut s, A::Undo);
    assert!(!element_glow(&s, "tunnels").unwrap().mask);
    run(&mut s, A::Redo);
    let doc = s.document().unwrap();
    assert_eq!(doc.glow_mask("tunnels").unwrap()[(100 * a * width + 120 * a) as usize], 0);
}

#[test]
fn removing_a_group_unglows_in_one_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    glow_tunnels(&mut s);
    assert!(run(&mut s, A::RemoveGroup("tunnels".into())).is_empty(), "asks first");
    assert!(matches!(s.dialog(), Some(Dialog::RemoveGroup { .. })));
    run(&mut s, A::Choose(Choice::Confirm));
    assert!(groups(&s).is_empty());
    assert_eq!(element_glow(&s, "tunnels"), None);
    run(&mut s, A::Undo);
    assert_eq!(groups(&s), ["tunnels"]);
    assert!(element_glow(&s, "tunnels").is_some());
}

#[test]
fn look_edits_undo() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let mut look = LookSource::default();
    look.ambient.intensity = 30;
    look.light_soak = 4;
    assert!(s.document().unwrap().level().look.is_none());
    run(&mut s, A::SetLook(Some(look.clone())));
    assert_eq!(s.document().unwrap().level().look, Some(look));
    assert!(s.dirty());
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().level().look, None);
}

#[test]
fn the_clock_plays_pauses_and_scrubs() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    assert_eq!(s.clock_ms(), 0.0);
    s.advance_clock(16.0);
    assert_eq!(s.clock_ms(), 0.0, "paused at first");
    run(&mut s, A::Clock(ClockAction::Play));
    s.advance_clock(16.0);
    s.advance_clock(16.0);
    assert_eq!(s.clock_ms(), 32.0);
    run(&mut s, A::Clock(ClockAction::Pause));
    s.advance_clock(16.0);
    assert_eq!(s.clock_ms(), 32.0);
    run(&mut s, A::Clock(ClockAction::Scrub(500.0)));
    assert_eq!(s.clock_ms(), 500.0);
    run(&mut s, A::Clock(ClockAction::Scrub(f64::NAN)));
    assert_eq!(s.clock_ms(), 500.0, "a bad scrub changes nothing");
    assert_eq!(s.view_mode(), ViewMode::Lit);
    run(&mut s, A::SetViewMode(ViewMode::LightOnly));
    assert_eq!(s.view_mode(), ViewMode::LightOnly);
    assert!(!s.dirty(), "the view and the clock are no edits");
}

#[test]
fn look_survives_save_and_reopen() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    glow_tunnels(&mut s);
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Glow));
    run(&mut s, A::GlowStrength(40));
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    use PointerKind::{Down, Up};
    run(&mut s, at((100, 100), Down));
    run(&mut s, at((100, 100), Up));
    run(&mut s, A::Set("wall".into(), Property::Clear(true)));
    let before = s.document().unwrap();
    let (level, mask) = (before.level().clone(), before.glow_mask("tunnels").unwrap().to_vec());
    let dir = before.dir().to_path_buf();
    run(&mut s, A::Save);
    assert!(dir.join("editor/glow/tunnels.png").is_file());
    let errors: Vec<_> = olr_pack::validate(&olr_vfs::DirFs::new(&dir))
        .into_iter()
        .filter(|d| d.severity == olr_pack::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let reopened =
        olr_editor::Document::open(olr_edit::pack::load(&dir).unwrap(), dir.clone(), &catalog(&root))
            .unwrap();
    assert_eq!(reopened.glow_mask("tunnels").unwrap(), &mask[..]);
    let after = reopened.level();
    assert_eq!((&after.look, &after.objects), (&level.look, &level.objects));
    assert_eq!((&after.glow, &after.clear), (&level.glow, &level.clear), "the bake is the same");
}

#[test]
fn locked_elements_refuse_glow() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Set("tunnels".into(), Property::Locked(true)));
    assert!(!upload(&glow_tunnels(&mut s)));
    assert_eq!(element_glow(&s, "tunnels"), None);
    assert!(groups(&s).is_empty(), "no group made for a refused glow");
    run(&mut s, A::Set("tunnels".into(), Property::Locked(false)));
    glow_tunnels(&mut s);
    run(&mut s, A::Set("tunnels".into(), Property::Locked(true)));
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Glow));
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    use PointerKind::{Down, Up};
    run(&mut s, at((100, 100), Down));
    run(&mut s, at((100, 100), Up));
    assert!(s.document().unwrap().glow_mask("tunnels").is_none(), "the brush refused");
}

/// Batch A review Important 2: a plain level whose glow and clear solids
/// were made outside the editor opens, saves and keeps them.
#[test]
fn a_plain_level_keeps_its_glow_and_clear() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = crate::common::copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    let fs = olr_vfs::DirFs::new(&dir);
    let mut src = olr_format::levels::read_level_files(&fs).unwrap();
    let (w, h) = src.size;
    let s = src.art_scale;
    let mut glow = olr_format::levels::GlowLayers::empty((w * s, h * s), (w, h));
    glow.front.iter_mut().step_by(7).for_each(|v| *v = 200);
    glow.back.iter_mut().step_by(5).for_each(|v| *v = 100);
    src.glow = Some(glow);
    src.clear = Some((0..w * h).map(|i| u8::from(i % 3 == 0) * 255).collect());
    olr_format::levels::write_level_files(&mut olr_vfs::DirFs::new(&dir), &src).unwrap();
    let (_t, mut st) = editor_at(&root, &dir);
    let doc = st.document().unwrap();
    assert!(doc.diagnostics().iter().any(|p| p.text.contains("outside the editor")), "it says so");
    assert_eq!((&doc.level().glow, &doc.level().clear), (&src.glow, &src.clear));
    // The editor opened its own copy: what it saves is there.
    let saved_dir = doc.dir().to_path_buf();
    let meta = olr_editor::LevelMeta { name: "Renamed".into(), ..doc.meta() };
    run(&mut st, A::Meta(meta));
    run(&mut st, A::Save);
    let saved = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(&saved_dir)).unwrap();
    assert_eq!(saved.name, "Renamed", "saved where it was read back");
    assert_eq!((saved.glow, saved.clear), (src.glow, src.clear));
}

/// Final review M1: a plain level's glow beams made outside the editor are
/// kept with its glow (`Kept`): the bake and a save keep them.
#[test]
fn a_plain_level_keeps_its_glow_beams() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = crate::common::copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    let fs = olr_vfs::DirFs::new(&dir);
    let mut src = olr_format::levels::read_level_files(&fs).unwrap();
    let (w, h) = src.size;
    let s = src.art_scale;
    let mut glow = olr_format::levels::GlowLayers::empty((w * s, h * s), (w, h));
    glow.front.iter_mut().step_by(7).for_each(|v| *v = 200);
    glow.back.iter_mut().step_by(5).for_each(|v| *v = 100);
    src.glow = Some(glow);
    let mut beams = olr_format::levels::GlowBeamMap::none((w, h));
    for i in (0..(w * h) as usize).step_by(11) {
        (beams.front[2 * i], beams.front[2 * i + 1]) = (46, 30);
        (beams.back[2 * i], beams.back[2 * i + 1]) = (1, 20);
    }
    src.glow_beams = Some(beams);
    olr_format::levels::write_level_files(&mut olr_vfs::DirFs::new(&dir), &src).unwrap();
    let (_t, mut st) = editor_at(&root, &dir);
    let doc = st.document().unwrap();
    assert!(src.glow_beams.is_some() && doc.level().glow_beams == src.glow_beams, "the bake keeps them");
    let saved_dir = doc.dir().to_path_buf();
    let meta = olr_editor::LevelMeta { name: "Renamed".into(), ..doc.meta() };
    run(&mut st, A::Meta(meta));
    run(&mut st, A::Save);
    let saved = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(&saved_dir)).unwrap();
    assert_eq!(saved.glow_beams, src.glow_beams, "and so does a save");
}

/// Element `id` given a glow and a dab of the brush at `at`, then selected.
fn paint_glow_on(s: &mut olr_editor::EditorState, id: &str, at: (i32, i32)) {
    let glow = ElementGlow { strength: 100, group: String::new(), mask: false, beam: None };
    run(s, A::SetGlow { id: id.into(), glow: Some(glow) });
    run(s, A::Select(Selection::Element(id.into())));
    run(s, A::Tool(Tool::Glow));
    run(s, A::GlowStrength(0));
    let p = |kind| A::Pointer { kind, world: at, shift: false, toggle: false };
    run(s, p(PointerKind::Down));
    run(s, p(PointerKind::Up));
    assert!(s.document().unwrap().glow_mask(id).is_some());
}

/// The mask fits its element's box (× art scale), and the pack saves clean.
fn saves_clean(s: &mut olr_editor::EditorState, id: &str) {
    let doc = s.document().unwrap();
    let e = &doc.items()[doc.index_of(id).unwrap()].element;
    let (_, b) = olr_format::levels::element_box(&e.kind, doc.level().size).unwrap();
    let a = doc.level().art_scale;
    let mask = doc.glow_mask(id).expect("the mask is kept");
    assert_eq!(mask.len() as u32, b.0 * a * b.1 * a, "the mask fits the box");
    let dir = doc.dir().to_path_buf();
    let cmds = run(s, A::Save);
    assert!(!s.dirty(), "saved: {cmds:?} {:?}", s.dialog());
    let errors: Vec<_> = olr_pack::validate(&olr_vfs::DirFs::new(&dir))
        .into_iter()
        .filter(|d| d.severity == olr_pack::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// Batch D review Critical 1: an image's new size resamples its mask.
#[test]
fn a_resized_masked_image_keeps_its_mask() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    paint_glow_on(&mut s, "badge", (10, 120));
    run(&mut s, A::Set("badge".into(), Property::Size((96, 72))));
    saves_clean(&mut s, "badge");
    run(&mut s, A::Undo);
    let len = s.document().unwrap().glow_mask("badge").unwrap().len() as u32;
    let a = s.document().unwrap().level().art_scale;
    assert_eq!(len, 48 * a * 48 * a, "undo takes the old mask back");
}

/// A level resize moves a level-sized layer's mask with it.
#[test]
fn a_level_resize_keeps_a_layers_mask() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    paint_glow_on(&mut s, "tunnels", (100, 100));
    let (w, h) = s.document().unwrap().level().size;
    let resize = A::Resize {
        size: (w + 20, h),
        offset: (10, 0),
        material: olr_data::level::Material::Background,
        colour: olr_data::image::Rgba::BLACK,
    };
    run(&mut s, resize);
    let doc = s.document().unwrap();
    let a = doc.level().art_scale;
    let mask = doc.glow_mask("tunnels").unwrap();
    assert_eq!(mask[(100 * a * (w + 20) * a + 110 * a) as usize], 0, "the dab moved with the layer");
    saves_clean(&mut s, "tunnels");
}

/// An art-scale change rescales the masks.
#[test]
fn art_scale_rescales_glow_masks() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    paint_glow_on(&mut s, "tunnels", (100, 100));
    let a = s.document().unwrap().level().art_scale;
    run(&mut s, A::ArtScale(a + 1));
    run(&mut s, A::Choose(Choice::Confirm));
    assert_eq!(s.document().unwrap().level().art_scale, a + 1);
    saves_clean(&mut s, "tunnels");
}

/// A pasted copy of a masked element brings its mask (and glows alike).
#[test]
fn paste_keeps_the_glow_mask() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    paint_glow_on(&mut s, "badge", (10, 120));
    let mask = s.document().unwrap().glow_mask("badge").unwrap().to_vec();
    run(&mut s, A::Select(Selection::Element("badge".into())));
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    let Selection::Element(id) = s.selection().clone() else { panic!("the paste is selected") };
    assert_ne!(id, "badge");
    assert_eq!(s.document().unwrap().glow_mask(&id), Some(&mask[..]));
    saves_clean(&mut s, &id);
    // Removing it, then undoing, keeps its mask too.
    run(&mut s, A::Remove);
    run(&mut s, A::Undo);
    assert_eq!(s.document().unwrap().glow_mask(&id), Some(&mask[..]));
}

/// Batch D review Important 2: a plain level's kept glow follows its look:
/// a removed group's pixels light nothing, the save stays clean.
#[test]
fn a_plain_levels_kept_glow_follows_look_edits() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = crate::common::copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    let fs = olr_vfs::DirFs::new(&dir);
    let mut src = olr_format::levels::read_level_files(&fs).unwrap();
    let (w, h) = src.size;
    let s = src.art_scale;
    let group = |id: &str| olr_format::look::GlowGroupSource {
        id: id.into(),
        colour: Default::default(),
        intensity: 100,
        reach: ReachSource::Pixels(32),
        animation: Default::default(),
        phase_ms: None,
    };
    src.look = Some(LookSource { glow_groups: vec![group("a"), group("b")], ..Default::default() });
    let mut glow = olr_format::levels::GlowLayers::empty((w * s, h * s), (w, h));
    glow.front.iter_mut().for_each(|v| *v = 100);
    glow.groups.iter_mut().enumerate().for_each(|(i, g)| *g = 1 + (i % 2) as u8);
    src.glow = Some(glow);
    olr_format::levels::write_level_files(&mut olr_vfs::DirFs::new(&dir), &src).unwrap();
    let (_t, mut st) = editor_at(&root, &dir);
    run(&mut st, A::RemoveGroup("a".into()));
    run(&mut st, A::Choose(Choice::Confirm));
    let g = st.document().unwrap().level().glow.clone().unwrap();
    assert!(g.groups.iter().all(|v| *v <= 1), "group b is now 1, a's pixels 0");
    assert!(g.groups.contains(&1) && g.groups.contains(&0));
    let saved_dir = st.document().unwrap().dir().to_path_buf();
    run(&mut st, A::Save);
    assert!(!st.dirty(), "{:?}", st.dialog());
    let saved = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(&saved_dir)).unwrap();
    assert_eq!(saved.glow, Some(g));
    // A resize re-lays the kept glow and clear flags, as `level resize` does.
    let resize = A::Resize {
        size: (w + 8, h),
        offset: (0, 0),
        material: olr_data::level::Material::Rock,
        colour: olr_data::image::Rgba::BLACK,
    };
    run(&mut st, resize);
    let l = st.document().unwrap().level();
    assert!(l.glow.as_ref().is_some_and(|g| g.world == (w + 8, h)), "kept through the resize");
}

/// The user's request (2026-10-19): spawns copy, cut and paste like
/// elements: each paste 4 px further, selected, one step.
#[test]
fn objects_copy_cut_and_paste() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    let spawns = |s: &olr_editor::EditorState| {
        s.document()
            .unwrap()
            .level()
            .objects
            .iter()
            .filter(|o| matches!(o, LevelObject::Spawn { .. }))
            .map(|o| o.at())
            .collect::<Vec<_>>()
    };
    run(&mut s, A::Tool(Tool::Spawn));
    run(&mut s, at((50, 60), PointerKind::Down));
    run(&mut s, at((50, 60), PointerKind::Up));
    let before = spawns(&s);
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    run(&mut s, A::Paste);
    assert_eq!(spawns(&s)[before.len()..], [(54, 64), (58, 68)]);
    let n = s.document().unwrap().level().objects.len();
    assert_eq!(*s.selection(), Selection::Object(n - 1), "the paste is selected");
    run(&mut s, A::Undo);
    assert_eq!(spawns(&s).len(), before.len() + 1, "one paste, one step");
    // A spawn cuts and pastes back.
    run(&mut s, A::Select(Selection::Object(n - 3)));
    run(&mut s, A::Cut);
    assert_eq!(spawns(&s).len(), before.len(), "cut");
    run(&mut s, A::Paste);
    assert!(spawns(&s).contains(&(54, 64)), "{:?}", spawns(&s));
}

/// The user's request (2026-10-19): the Glow brush on an element without
/// a glow gives it one, dark but for what is painted; one step.
#[test]
fn the_glow_brush_starts_a_dark_glow() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Glow));
    run(&mut s, A::GlowStrength(80));
    let p = |kind| A::Pointer { kind, world: (100, 100), shift: false, toggle: false };
    run(&mut s, p(PointerKind::Down));
    run(&mut s, p(PointerKind::Up));
    let doc = s.document().unwrap();
    let a = doc.level().art_scale;
    let w = doc.level().size.0 * a;
    let g = element_glow(&s, "tunnels").expect("a glow");
    assert!(g.mask && groups(&s).contains(&g.group), "its own group");
    let mask = doc.glow_mask("tunnels").unwrap();
    assert_eq!(mask[(100 * a * w + 100 * a) as usize], 204, "80 % where painted");
    assert_eq!(mask[(10 * a * w + 10 * a) as usize], 0, "dark elsewhere");
    run(&mut s, A::Undo);
    assert_eq!(element_glow(&s, "tunnels"), None, "one step");
    assert!(groups(&s).is_empty());
}

/// Glow by colour: one click lights the connected pixels alike in colour.
#[test]
fn glow_by_colour_lights_alike_pixels() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    // A red square on the Tunnels layer: a "painted lamp".
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::Rect));
    run(&mut s, A::Paint(olr_editor::tools::PaintChoice::Colour(olr_data::image::Rgba::rgb(250, 20, 20))));
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    run(&mut s, at((200, 150), PointerKind::Down));
    run(&mut s, at((210, 158), PointerKind::Move));
    run(&mut s, at((210, 158), PointerKind::Up));
    run(&mut s, A::Tool(Tool::Glow));
    run(&mut s, A::GlowByColour(true));
    run(&mut s, A::Tolerance(10));
    run(&mut s, A::GlowStrength(100));
    run(&mut s, at((205, 154), PointerKind::Down));
    run(&mut s, at((205, 154), PointerKind::Up));
    let doc = s.document().unwrap();
    let a = doc.level().art_scale;
    let w = doc.level().size.0 * a;
    let mask = doc.glow_mask("tunnels").expect("a mask");
    let v = |x: u32, y: u32| mask[(y * a * w + x * a) as usize];
    assert_eq!(v(205, 154), 255, "the red glows");
    assert_eq!(v(201, 151), 255);
    assert_eq!(v(150, 100), 0, "the rest does not");
    assert_eq!(v(215, 154), 0, "past the square");
    run(&mut s, A::Undo);
    assert_eq!(element_glow(&s, "tunnels"), None, "one step");
}

/// A red square painted on Tunnels at (200, 150)-(210, 158).
fn red_square(s: &mut olr_editor::EditorState) {
    run(s, A::Select(Selection::Element("tunnels".into())));
    run(s, A::Tool(Tool::Rect));
    run(s, A::Paint(olr_editor::tools::PaintChoice::Colour(olr_data::image::Rgba::rgb(250, 20, 20))));
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    run(s, at((200, 150), PointerKind::Down));
    run(s, at((210, 158), PointerKind::Move));
    run(s, at((210, 158), PointerKind::Up));
}

/// The user's request (2026-10-19): pixels selected by colour take a glow
/// of their own, the rest of the layer none; one step.
#[test]
fn a_colour_selection_takes_a_glow() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    red_square(&mut s);
    run(&mut s, A::Tool(Tool::SelectPixels));
    run(&mut s, A::SelectByColour(true));
    run(&mut s, A::Tolerance(10));
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    run(&mut s, at((205, 154), PointerKind::Down));
    run(&mut s, at((205, 154), PointerKind::Up));
    assert!(s.pixel_rect().is_some(), "selected");
    run(&mut s, A::GlowSelection(70));
    let doc = s.document().unwrap();
    let a = doc.level().art_scale;
    let w = doc.level().size.0 * a;
    let mask = doc.glow_mask("tunnels").expect("a mask");
    let v = |x: u32, y: u32| mask[(y * a * w + x * a) as usize];
    assert_eq!((v(205, 154), v(201, 151)), (179, 179), "70 % on the red");
    assert_eq!((v(150, 100), v(215, 154)), (0, 0), "none elsewhere");
    run(&mut s, A::Undo);
    assert_eq!(element_glow(&s, "tunnels"), None, "one step");
}

/// Ctrl adds another patch alike in colour to the selection; copy refuses
/// a selection by colour (it takes a rectangle).
#[test]
fn ctrl_adds_to_a_colour_selection() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    red_square(&mut s);
    let at = |world, kind, toggle| A::Pointer { kind, world, shift: false, toggle };
    run(&mut s, at((300, 150), PointerKind::Down, false));
    run(&mut s, at((306, 156), PointerKind::Move, false));
    run(&mut s, at((306, 156), PointerKind::Up, false));
    run(&mut s, A::Tool(Tool::SelectPixels));
    run(&mut s, A::SelectByColour(true));
    run(&mut s, A::Tolerance(10));
    run(&mut s, at((205, 154), PointerKind::Down, false));
    run(&mut s, at((205, 154), PointerKind::Up, false));
    run(&mut s, at((303, 153), PointerKind::Down, true));
    run(&mut s, at((303, 153), PointerKind::Up, true));
    run(&mut s, A::Copy);
    assert!(s.status().message.as_deref().is_some_and(|m| m.contains("rectangle")), "copy refused");
    run(&mut s, A::GlowSelection(100));
    let doc = s.document().unwrap();
    let a = doc.level().art_scale;
    let w = doc.level().size.0 * a;
    let mask = doc.glow_mask("tunnels").expect("a mask");
    let v = |x: u32, y: u32| mask[(y * a * w + x * a) as usize];
    assert_eq!((v(205, 154), v(303, 153)), (255, 255), "both patches");
    assert_eq!((v(250, 154), v(150, 100)), (0, 0), "none between or elsewhere");
}

/// Plan 15a's final review: a colour selection made at one art scale, then
/// added to by Ctrl after the art scale changed, never panics (its flags
/// were laid out for the old scale); the stale selection is dropped.
#[test]
fn a_colour_selection_survives_an_art_scale_change() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    red_square(&mut s);
    run(&mut s, A::Tool(Tool::SelectPixels));
    run(&mut s, A::SelectByColour(true));
    run(&mut s, A::Tolerance(10));
    let at = |world, kind, toggle| A::Pointer { kind, world, shift: false, toggle };
    run(&mut s, at((205, 154), PointerKind::Down, false));
    run(&mut s, at((205, 154), PointerKind::Up, false));
    assert!(s.pixel_region().is_some());
    let a = s.document().unwrap().level().art_scale;
    run(&mut s, A::ArtScale(if a > 1 { a - 1 } else { a + 1 }));
    run(&mut s, A::Choose(Choice::Confirm));
    assert!(s.pixel_rect().is_none(), "a whole change drops the pixel selection");
    run(&mut s, at((205, 154), PointerKind::Down, true));
    run(&mut s, at((205, 154), PointerKind::Up, true));
    run(&mut s, A::GlowSelection(100));
    assert!(s.document().unwrap().glow_mask("tunnels").is_some());
}

/// A rectangle of pixels takes a glow too.
#[test]
fn a_rectangle_selection_takes_a_glow() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(Selection::Element("tunnels".into())));
    run(&mut s, A::Tool(Tool::SelectPixels));
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    run(&mut s, at((40, 40), PointerKind::Down));
    run(&mut s, at((60, 50), PointerKind::Move));
    run(&mut s, at((60, 50), PointerKind::Up));
    run(&mut s, A::GlowSelection(100));
    let doc = s.document().unwrap();
    let a = doc.level().art_scale;
    let w = doc.level().size.0 * a;
    let mask = doc.glow_mask("tunnels").expect("a mask");
    let v = |x: u32, y: u32| mask[(y * a * w + x * a) as usize];
    assert_eq!(v(45, 45), 255);
    assert_eq!((v(30, 45), v(45, 60)), (0, 0));
}

/// Plan 15c: glowing pixels are the only light; the Light tool (K) went.
#[test]
fn the_editor_has_no_light_tool() {
    assert!(Tool::ALL.iter().all(|t| t.name() != "Light"), "{:?}", Tool::ALL);
    assert!(Tool::ALL.iter().all(|t| t.key() != 'K'), "K selects nothing");
}
