//! Look editing in the editor's document and state (plan 15a Task 17):
//! glow, its groups and brush, lights, the look, the view and the clock.

use olr_edit::element::Property;
use olr_editor::state::ClockAction;
use olr_editor::tools::{PointerKind, Tool};
use olr_editor::{Choice, Dialog, EditorAction as A, EditorCommand as C, Selection};
use olr_format::levels::{ElementGlow, LevelObject};
use olr_format::look::{LightSource, LookSource, ReachSource};
use olr_look::ViewMode;

use crate::common::{catalog, content, editor, editor_at, run};

fn upload(cmds: &[C]) -> bool {
    cmds.iter().any(|c| matches!(c, C::Upload(_)))
}

/// A glow on the fixture's `tunnels` layer, its group left to the editor.
fn glow_tunnels(s: &mut olr_editor::EditorState) -> Vec<C> {
    let glow = ElementGlow { strength: 100, group: String::new(), mask: false };
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

fn lights(s: &olr_editor::EditorState) -> Vec<(i32, i32)> {
    let doc = s.document().unwrap();
    doc.level().objects.iter().filter_map(|o| o.as_light().map(|_| o.at())).collect()
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
fn lights_place_move_and_undo() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Tool(Tool::Light));
    let at = |world, kind| A::Pointer { kind, world, shift: false, toggle: false };
    use PointerKind::{Down, Move, Up};
    run(&mut s, at((50, 60), Down));
    run(&mut s, at((50, 60), Up));
    assert_eq!(lights(&s), [(50, 60)]);
    let index = s.document().unwrap().level().objects.len() - 1;
    assert_eq!(*s.selection(), Selection::Object(index));
    // A press on it drags it.
    run(&mut s, at((51, 60), Down));
    run(&mut s, at((71, 75), Move));
    run(&mut s, at((71, 75), Up));
    assert_eq!(lights(&s), [(70, 75)]);
    let light = LightSource {
        colour: olr_format::Colour(olr_data::image::Rgba::rgb(0, 128, 255)),
        intensity: 150,
        reach: ReachSource::Pixels(90),
        animation: Default::default(),
        phase_ms: None,
    };
    run(&mut s, A::SetLight { index, at: (70, 75), light: light.clone(), radius: 4 });
    let o = &s.document().unwrap().level().objects[index];
    assert_eq!(o.as_light(), Some((4, light)));
    for _ in 0..3 {
        run(&mut s, A::Undo);
    }
    assert!(lights(&s).is_empty());
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
    run(&mut s, A::Tool(Tool::Light));
    run(&mut s, at((30, 40), Down));
    run(&mut s, at((30, 40), Up));
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
    let (tmp, dir) = crate::common::copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    let fs = olr_vfs::DirFs::new(&dir);
    let mut src = olr_format::levels::read_level_files(&fs).unwrap();
    let (w, h) = src.size;
    let s = src.art_scale;
    let mut glow = olr_format::levels::GlowLayers::empty((w * s, h * s), (w, h));
    glow.front.iter_mut().step_by(7).for_each(|v| *v = 200);
    glow.back.iter_mut().step_by(5).for_each(|v| *v = 100);
    src.glow = Some(glow);
    src.clear = Some((0..w * h).map(|i| u8::from(i % 3 == 0) * 255).collect());
    src.objects.push(LevelObject::light(
        (10, 10),
        2,
        LightSource {
            colour: olr_format::Colour(olr_data::image::Rgba::rgb(255, 255, 255)),
            intensity: 100,
            reach: ReachSource::Pixels(50),
            animation: Default::default(),
            phase_ms: None,
        },
    ));
    olr_format::levels::write_level_files(&mut olr_vfs::DirFs::new(&dir), &src).unwrap();
    let (_t, mut st) = editor_at(&root, &dir);
    let doc = st.document().unwrap();
    assert!(doc.diagnostics().iter().any(|p| p.text.contains("outside the editor")), "it says so");
    assert_eq!((&doc.level().glow, &doc.level().clear), (&src.glow, &src.clear));
    let meta = olr_editor::LevelMeta { name: "Renamed".into(), ..doc.meta() };
    run(&mut st, A::Meta(meta));
    run(&mut st, A::Save);
    let saved = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(&dir)).unwrap();
    assert_eq!((saved.glow, saved.clear), (src.glow, src.clear));
}
