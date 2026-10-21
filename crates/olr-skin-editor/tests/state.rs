//! The state without panels: painting per target, strokes as steps, the
//! frame operations, the settings, Make double resolution, saving and
//! closing, a bad layout refused.

use olr_data::image::Rgba;
use olr_data::skin::SkinLight;
use olr_skin_editor::state::{DEFAULT_LIGHT, MAX_RECENT};
use olr_skin_editor::{
    Choice, Dialog, FRAMES, Onion, PasteTo, SkinAction as A, SkinCommand as C, SkinDoc, SkinEditorState,
    Target, Tool,
};

use crate::common::classic;

fn editor() -> SkinEditorState {
    SkinEditorState::new(SkinDoc::new(classic()).unwrap(), None)
}

fn run(s: &mut SkinEditorState, actions: impl IntoIterator<Item = A>) -> Vec<C> {
    actions.into_iter().flat_map(|a| s.update(a)).collect()
}

fn pixel(s: &SkinEditorState, f: usize, x: u32, y: u32) -> (Rgba, u8, u8) {
    let fr = s.doc().frame(f);
    let i = (y * fr.w + x) as usize;
    (fr.px[i], fr.tint[i], fr.glow[i])
}

const RED: Rgba = Rgba::rgb(255, 0, 0);

#[test]
fn a_stroke_paints_and_is_one_step() {
    let mut s = editor();
    run(&mut s, [A::Frame(0), A::Colour(RED), A::Press { x: 0, y: 0 }, A::Drag { x: 3, y: 0 }, A::Release]);
    for x in 0..4 {
        assert_eq!(pixel(&s, 0, x, 0).0, RED);
    }
    assert!(s.dirty());
    assert_eq!(s.recent(), &[RED]);
    run(&mut s, [A::Undo]);
    assert_eq!(pixel(&s, 0, 2, 0).0, Rgba::default(), "one undo takes the whole stroke");
    assert!(!s.dirty());
    assert!(!s.can_undo());
    run(&mut s, [A::Redo]);
    assert_eq!(pixel(&s, 0, 2, 0).0, RED);
}

#[test]
fn a_stroke_off_the_frame_changes_nothing() {
    let mut s = editor();
    run(&mut s, [A::Press { x: -5, y: -5 }, A::Drag { x: -9, y: -2 }, A::Release]);
    assert!(!s.can_undo());
    // A far pointer is clamped: no huge line.
    run(&mut s, [A::Press { x: i32::MIN, y: 0 }, A::Drag { x: i32::MAX, y: 0 }, A::Release]);
    assert!(s.can_undo());
}

#[test]
fn the_two_pixel_brush_and_the_eraser() {
    let mut s = editor();
    run(&mut s, [A::Frame(2), A::Colour(RED), A::Size(2), A::Press { x: 0, y: 0 }, A::Release]);
    for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        assert_eq!(pixel(&s, 2, x, y).0, RED);
    }
    assert_eq!(pixel(&s, 2, 2, 0).0, Rgba::default());
    // The eraser clears colour, tint and glow.
    run(&mut s, [A::Size(1), A::Tool(Tool::Eraser), A::Press { x: 7, y: 6 }, A::Release]);
    assert_eq!(pixel(&s, 2, 7, 6), (Rgba::default(), 0, 0));
}

#[test]
fn line_rectangle_fill_and_eyedropper() {
    let mut s = editor();
    run(&mut s, [A::Frame(0), A::Colour(RED), A::Tool(Tool::Line), A::Press { x: 0, y: 0 }]);
    run(&mut s, [A::Drag { x: 4, y: 4 }, A::Drag { x: 4, y: 0 }, A::Release]);
    assert_eq!(pixel(&s, 0, 4, 0).0, RED);
    assert_eq!(pixel(&s, 0, 2, 2).0, Rgba::default(), "the line follows the last point only");
    let blue = Rgba::rgb(0, 0, 255);
    run(&mut s, [A::Colour(blue), A::Tool(Tool::Rect), A::Press { x: 0, y: 10 }, A::Drag { x: 3, y: 13 }]);
    run(&mut s, [A::Release]);
    assert_eq!(pixel(&s, 0, 0, 13).0, blue);
    assert_eq!(pixel(&s, 0, 1, 11).0, Rgba::default(), "an outline");
    // Fill the block of frame 0 (6 × 4 of one colour).
    let green = Rgba::rgb(0, 255, 0);
    run(&mut s, [A::Colour(green), A::Tool(Tool::Fill), A::Press { x: 8, y: 7 }, A::Release]);
    assert_eq!(pixel(&s, 0, 12, 9).0, green);
    assert_eq!(pixel(&s, 0, 13, 9).0, Rgba::default());
    // The eyedropper picks a colour and goes back to the pencil.
    run(&mut s, [A::Tool(Tool::Eyedropper), A::Press { x: 4, y: 0 }, A::Release]);
    assert_eq!((s.colour(), s.tool()), (RED, Tool::Pencil));
    assert_eq!(s.recent()[0], green);
}

#[test]
fn tint_and_glow_mark_opaque_pixels_only() {
    let mut s = editor();
    run(&mut s, [A::Frame(5), A::Target(Target::Tint), A::Press { x: 8, y: 8 }, A::Release]);
    assert_eq!(pixel(&s, 5, 8, 8).1, 255);
    run(&mut s, [A::Press { x: 0, y: 0 }, A::Release]);
    assert_eq!(pixel(&s, 5, 0, 0).1, 0, "a transparent pixel takes no tint");
    run(&mut s, [A::Tool(Tool::Eraser), A::Press { x: 8, y: 6 }, A::Release]);
    assert_eq!(pixel(&s, 5, 8, 6), (Rgba::rgb(50, 100, 200), 0, 0), "the eraser untints, colour kept");
    run(
        &mut s,
        [A::Tool(Tool::Fill), A::Target(Target::Glow), A::GlowStrength(128), A::Press { x: 9, y: 8 }],
    );
    assert_eq!(pixel(&s, 5, 12, 9).2, 128, "a glow fill marks the block");
    assert_eq!(pixel(&s, 5, 0, 0).2, 0);
    // The glow is saved only when used.
    assert!(s.doc().to_source().glow.is_some());
    run(&mut s, [A::Undo]);
    assert!(s.doc().to_source().glow.is_none());
}

#[test]
fn copy_and_paste_to_a_frame_a_row_and_a_column() {
    let mut s = editor();
    run(&mut s, [A::Paste(PasteTo::Frame)]);
    assert!(!s.can_undo(), "nothing copied yet");
    run(&mut s, [A::Frame(8), A::CopyFrame]);
    let copied = s.doc().frame(8);
    run(&mut s, [A::Frame(10), A::Paste(PasteTo::Row)]);
    for f in 7..14 {
        assert_eq!(s.doc().frame(f), copied, "frame {f}");
    }
    assert_ne!(s.doc().frame(3), copied);
    run(&mut s, [A::Undo]);
    assert_ne!(s.doc().frame(10), copied, "one step");
    run(&mut s, [A::Frame(3), A::Paste(PasteTo::Column)]);
    for f in [3, 10, 17] {
        assert_eq!(s.doc().frame(f), copied, "frame {f}");
    }
    run(&mut s, [A::Frame(0), A::Paste(PasteTo::Frame)]);
    assert_eq!(s.doc().frame(0), copied);
}

#[test]
fn flip_and_shift() {
    let mut s = editor();
    run(&mut s, [A::Frame(1), A::Colour(RED), A::Press { x: 0, y: 0 }, A::Release]);
    run(&mut s, [A::Flip { vertical: false }]);
    assert_eq!(pixel(&s, 1, 19, 0).0, RED);
    run(&mut s, [A::Flip { vertical: true }]);
    assert_eq!(pixel(&s, 1, 19, 17).0, RED);
    // Off one edge, back at the other.
    run(&mut s, [A::Shift { dx: 1, dy: 0 }]);
    assert_eq!(pixel(&s, 1, 0, 17).0, RED);
    run(&mut s, [A::Shift { dx: 0, dy: -1 }]);
    assert_eq!(pixel(&s, 1, 0, 16).0, RED);
    // Every frame at once, one step.
    let before: Vec<_> = (0..FRAMES).map(|f| s.doc().frame(f)).collect();
    run(&mut s, [A::ShiftAll(true), A::Shift { dx: 0, dy: 1 }]);
    for (f, b) in before.iter().enumerate() {
        assert_eq!(s.doc().frame(f), b.shifted(0, 1), "frame {f}");
    }
    run(&mut s, [A::Undo]);
    assert_eq!(s.doc().frame(20), before[20]);
}

#[test]
fn the_onion_skin_names_the_neighbour() {
    assert_eq!(Onion::Off.of(8), None);
    assert_eq!(Onion::PreviousAim.of(8), Some(7));
    assert_eq!(Onion::PreviousAim.of(7), None);
    assert_eq!(Onion::PreviousWalk.of(8), Some(1));
    assert_eq!(Onion::PreviousWalk.of(1), Some(15), "the walk cycles");
}

#[test]
fn name_and_light_are_steps_within_the_caps() {
    let mut s = editor();
    run(&mut s, [A::SetName("  Lantern ".into())]);
    assert_eq!(s.doc().name(), "Lantern");
    run(&mut s, [A::SetName("   ".into())]);
    assert_eq!(s.doc().name(), "Lantern", "no empty name");
    run(&mut s, [A::SetLight(Some(DEFAULT_LIGHT))]);
    assert_eq!(s.doc().light(), Some(DEFAULT_LIGHT));
    run(&mut s, [A::SetLight(Some(SkinLight { intensity: 9999, reach: 0 }))]);
    assert_eq!(s.doc().light(), Some(SkinLight { intensity: 200, reach: 1 }));
    run(&mut s, [A::Undo, A::Undo]);
    assert_eq!(s.doc().light(), None);
    run(&mut s, [A::Undo]);
    assert_eq!(s.doc().name(), "Sample");
}

#[test]
fn make_double_resolution_is_one_step() {
    let mut s = editor();
    assert!(s.doc().is_classic());
    run(&mut s, [A::MakeDouble]);
    assert_eq!((s.doc().art_scale(), s.doc().frame_size()), (4, (40, 36)));
    // Scale2x keeps a solid block solid: its tinted top row too.
    let fr = s.doc().frame(3);
    assert_eq!(fr.px[12 * 40 + 16], Rgba::rgb(30, 100, 200));
    assert_eq!(fr.tint[12 * 40 + 16], 255);
    assert_eq!(fr.tint[14 * 40 + 16], 0);
    // A corner rounds off.
    assert_eq!(fr.px[12 * 40 + 14], Rgba::default());
    run(&mut s, [A::MakeDouble]);
    assert_eq!(s.doc().art_scale(), 4, "only once");
    // A frame copied before does not paste into the doubled skin.
    run(&mut s, [A::Undo]);
    run(&mut s, [A::CopyFrame, A::Redo, A::Paste(PasteTo::Frame)]);
    assert_eq!(s.status(), "The copied frame is of another size");
    run(&mut s, [A::Undo]);
    assert!(s.doc().is_classic());
    assert_eq!(s.doc().to_source(), classic());
}

#[test]
fn the_first_save_asks_a_name_then_saves_in_place() {
    let mut s = editor();
    run(&mut s, [A::Colour(RED), A::Press { x: 1, y: 1 }, A::Release]);
    assert!(run(&mut s, [A::Save]).is_empty());
    assert_eq!(s.dialog(), Some(&Dialog::Name));
    // While a dialog is open the skin is not painted.
    run(&mut s, [A::Press { x: 2, y: 2 }, A::Release]);
    assert_eq!(pixel(&s, 4, 2, 2).0, Rgba::default());
    let c = run(&mut s, [A::SaveAs(" Lantern ".into())]);
    let [C::Save { id: None, name, source }] = c.as_slice() else { panic!("{c:?}") };
    assert_eq!((name.as_str(), source.name.as_str()), ("Lantern", "Lantern"));
    // An edit while the host saves stays unsaved.
    run(&mut s, [A::Press { x: 3, y: 3 }, A::Release, A::Saved("lantern".into())]);
    assert!(s.dirty());
    assert_eq!(s.pack_id(), Some("lantern"));
    let c = run(&mut s, [A::Save]);
    assert!(matches!(c.as_slice(), [C::Save { id: Some(id), .. }] if id == "lantern"), "{c:?}");
    run(&mut s, [A::Saved("lantern".into())]);
    assert!(!s.dirty());
}

#[test]
fn closing_asks_when_unsaved() {
    let mut s = editor();
    assert_eq!(run(&mut s, [A::Close]), vec![C::Close], "nothing changed: no question");
    run(&mut s, [A::Press { x: 1, y: 1 }, A::Release, A::Close]);
    assert_eq!(s.dialog(), Some(&Dialog::Unsaved));
    run(&mut s, [A::Choice(Choice::Cancel)]);
    assert_eq!(s.dialog(), None);
    run(&mut s, [A::Close]);
    assert_eq!(run(&mut s, [A::Choice(Choice::DontSave)]), vec![C::Close]);
    // Save and close: the name first, then the close once saved.
    let mut s = editor();
    run(&mut s, [A::Press { x: 1, y: 1 }, A::Release, A::Close, A::Choice(Choice::Save)]);
    assert_eq!(s.dialog(), Some(&Dialog::Name));
    let c = run(&mut s, [A::SaveAs("Mine".into())]);
    assert!(matches!(c.as_slice(), [C::Save { id: None, .. }]));
    assert_eq!(run(&mut s, [A::Saved("mine".into())]), vec![C::Close]);
    // A failed save says why and closes nothing.
    let mut s = SkinEditorState::new(SkinDoc::new(classic()).unwrap(), Some("mine".into()));
    run(&mut s, [A::Press { x: 1, y: 1 }, A::Release, A::Close, A::Choice(Choice::Save)]);
    run(&mut s, [A::Failed("disk full".into())]);
    assert_eq!(s.dialog(), Some(&Dialog::Error("disk full".into())));
    assert!(run(&mut s, [A::Choice(Choice::Cancel)]).is_empty());
    assert!(s.dirty());
}

#[test]
fn recent_colours_are_kept_newest_first() {
    let mut s = editor();
    for i in 0..(MAX_RECENT as u8 + 3) {
        run(&mut s, [A::Colour(Rgba::rgb(i, 0, 0)), A::Press { x: 0, y: 0 }, A::Release]);
    }
    assert_eq!(s.recent().len(), MAX_RECENT);
    assert_eq!(s.recent()[0], Rgba::rgb(MAX_RECENT as u8 + 2, 0, 0));
    run(&mut s, [A::Colour(Rgba::rgb(5, 0, 0)), A::Press { x: 0, y: 0 }, A::Release]);
    assert_eq!(s.recent()[0], Rgba::rgb(5, 0, 0));
    assert_eq!(s.recent().iter().filter(|&&c| c == Rgba::rgb(5, 0, 0)).count(), 1);
}

#[test]
fn the_preview_walks_aims_and_turns() {
    let mut s = editor();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..2000 {
        run(&mut s, [A::Tick(0.02)]);
        seen.insert(s.preview_pose());
    }
    for f in 0..FRAMES {
        assert!(seen.contains(&(f, false)) && seen.contains(&(f, true)), "frame {f} both ways: {seen:?}");
    }
    run(&mut s, [A::Animate(false), A::Frame(9)]);
    assert_eq!(s.preview_pose(), (9, false));
}

#[test]
fn other_layouts_are_refused() {
    let mut src = classic();
    src.frame_size = (21, 18);
    assert!(SkinDoc::new(src).is_err());
    let mut src = classic();
    src.tint.pop();
    assert!(SkinDoc::new(src).is_err());
    let mut src = classic();
    src.glow = Some(vec![0; 3]);
    assert!(SkinDoc::new(src).is_err());
    assert_eq!(SkinDoc::blank("New", true).frame_size(), (40, 36));
}
