//! The skin editor's panels driven by label (`egui_kittest`, issue #14):
//! a copy painted, pasted to a row, tinted and made to glow, its light
//! set, undone and redone, made double resolution, and saved; Close
//! asking first. The test host answers a save at once.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_data::image::Rgba;
use olr_skin_editor::state::{DEFAULT_LIGHT, frame_name};
use olr_skin_editor::ui::{self, SkinEditorData, labels};
use olr_skin_editor::{Dialog, SkinAction as A, SkinCommand as C, SkinDoc, SkinEditorState, Target};

use crate::common::classic;

/// The editor and what it asked the host to do.
struct App {
    state: SkinEditorState,
    data: SkinEditorData,
    commands: Vec<C>,
}

fn harness(state: SkinEditorState) -> Harness<'static, App> {
    let app = App { state, data: SkinEditorData::default(), commands: Vec::new() };
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 860.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                for c in app.state.update(a) {
                    // The host saves at once, as a new pack named by its slug.
                    if let C::Save { id, name, .. } = &c {
                        let id = id.clone().unwrap_or_else(|| name.to_lowercase());
                        app.state.update(A::Saved(id));
                    }
                    app.commands.push(c);
                }
            }
        },
        app,
    );
    h.run();
    h
}

/// The screen point of art pixel `(x, y)` of the large frame.
fn at(h: &Harness<'static, App>, x: i32, y: i32) -> egui::Pos2 {
    let s = &h.state().state;
    let rect = h.state().data.canvas_rect().expect("the canvas was drawn");
    let (fw, fh) = s.doc().frame_size();
    let z = s.zoom();
    let o = rect.center() - egui::vec2(fw as f32 * z / 2.0, fh as f32 * z / 2.0);
    let o = egui::pos2(o.x.round(), o.y.round());
    o + egui::vec2((x as f32 + 0.5) * z, (y as f32 + 0.5) * z)
}

fn button(h: &mut Harness<'static, App>, pos: egui::Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

/// A stroke on the large frame through `points` (art pixels).
fn stroke(h: &mut Harness<'static, App>, points: &[(i32, i32)]) {
    let first = at(h, points[0].0, points[0].1);
    h.event(egui::Event::PointerMoved(first));
    h.run();
    button(h, first, true);
    for &(x, y) in &points[1..] {
        let p = at(h, x, y);
        h.event(egui::Event::PointerMoved(p));
        h.run();
    }
    let last = *points.last().unwrap();
    let p = at(h, last.0, last.1);
    button(h, p, false);
}

fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

fn pixel(h: &Harness<'static, App>, f: usize, x: u32, y: u32) -> (Rgba, u8, u8) {
    let fr = h.state().state.doc().frame(f);
    let i = (y * fr.w + x) as usize;
    (fr.px[i], fr.tint[i], fr.glow[i])
}

/// A copy of a classic skin (as the host opens one: no pack of its own).
fn copy() -> Harness<'static, App> {
    let mut src = classic();
    src.name = "Sample copy".into();
    harness(SkinEditorState::new(SkinDoc::new(src).unwrap(), None))
}

#[test]
fn paint_paste_tint_glow_light_undo_double_and_save() {
    let mut h = copy();
    // The 21 frames, each by name.
    for f in 0..21 {
        h.get_by_label(&frame_name(f));
    }
    assert!(h.query_by_label(labels::UNSAVED).is_none());
    // Paint frame 1 (aim 1, walk 1) with the pencil in the paint colour.
    click(&mut h, &frame_name(0));
    assert_eq!(h.state().state.frame(), 0);
    let hex = h.get_by_label(labels::HEX);
    hex.click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(labels::HEX).type_text("#ff2010");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let red = Rgba::rgb(255, 32, 16);
    assert_eq!(h.state().state.colour(), red);
    stroke(&mut h, &[(1, 1), (4, 1), (4, 3)]);
    assert_eq!(pixel(&h, 0, 3, 1).0, red);
    assert_eq!(pixel(&h, 0, 4, 3).0, red);
    h.get_by_label(labels::UNSAVED);
    // The recent colour is offered.
    h.get_by_label("#ff2010");
    // Copy it and paste it to its row.
    click(&mut h, labels::COPY_FRAME);
    click(&mut h, labels::PASTE_ROW);
    for f in 0..7 {
        assert_eq!(pixel(&h, f, 3, 1).0, red, "frame {f}");
    }
    assert_eq!(pixel(&h, 7, 3, 1).0, Rgba::default(), "not the next row");
    // Player colour: the painted pixels take it.
    click(&mut h, labels::TARGET_TINT);
    assert_eq!(h.state().state.target(), Target::Tint);
    stroke(&mut h, &[(1, 1), (2, 1)]);
    assert_eq!(pixel(&h, 0, 2, 1).1, 255);
    assert_eq!(pixel(&h, 0, 3, 1).1, 0);
    // Glow, at half strength.
    click(&mut h, labels::TARGET_GLOW);
    click(&mut h, "50 %");
    stroke(&mut h, &[(4, 1)]);
    assert_eq!(pixel(&h, 0, 4, 1).2, 128);
    // The skin light.
    click(&mut h, labels::SKIN_LIGHT);
    assert_eq!(h.state().state.doc().light(), Some(DEFAULT_LIGHT));
    h.get_by_label(labels::LIGHT_INTENSITY);
    // Undo the light and the glow; redo the glow.
    click(&mut h, labels::UNDO);
    assert_eq!(h.state().state.doc().light(), None);
    click(&mut h, labels::UNDO);
    assert_eq!(pixel(&h, 0, 4, 1).2, 0);
    click(&mut h, labels::REDO);
    assert_eq!(pixel(&h, 0, 4, 1).2, 128);
    // Make double resolution.
    click(&mut h, labels::MAKE_DOUBLE);
    assert_eq!(h.state().state.doc().frame_size(), (40, 36));
    assert!(h.query_by_label(labels::MAKE_DOUBLE).is_none(), "offered for a classic skin only");
    assert_eq!(pixel(&h, 0, 9, 3), (red, 0, 128), "the glowing pixel doubled");
    assert_eq!(pixel(&h, 0, 5, 3), (red, 255, 0), "the tinted one");
    // Save: the name first.
    click(&mut h, labels::SAVE);
    assert_eq!(h.state().state.dialog(), Some(&Dialog::Name));
    h.get_by_label(labels::SKIN_NAME).click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(labels::SKIN_NAME).type_text("Lantern");
    h.run();
    click(&mut h, labels::SAVE_SKIN);
    let saves: Vec<_> = h.state().commands.iter().filter(|c| matches!(c, C::Save { .. })).collect();
    let [C::Save { id: None, name, source }] = saves.as_slice() else { panic!("{saves:?}") };
    assert_eq!(name, "Lantern");
    assert_eq!((source.art_scale, source.name.as_str()), (4, "Lantern"));
    assert!(source.glow.is_some());
    assert_eq!(h.state().state.pack_id(), Some("lantern"));
    assert!(h.query_by_label(labels::UNSAVED).is_none());
    // A second save is in place: no name asked.
    stroke(&mut h, &[(0, 0)]);
    click(&mut h, labels::SAVE);
    assert!(matches!(h.state().commands.last(), Some(C::Save { id: Some(id), .. }) if id == "lantern"));
    assert_eq!(h.state().state.dialog(), None);
}

#[test]
fn shift_flip_and_the_onion_skin_from_the_panel() {
    let mut h = copy();
    click(&mut h, &frame_name(9));
    let before = h.state().state.doc().frame(9);
    click(&mut h, labels::SHIFT_RIGHT);
    assert_eq!(h.state().state.doc().frame(9), before.shifted(1, 0));
    click(&mut h, labels::SHIFT_LEFT);
    click(&mut h, labels::FLIP);
    assert_eq!(h.state().state.doc().frame(9), before.flipped(false));
    click(&mut h, labels::SHIFT_ALL);
    assert!(h.state().state.shift_all());
    let first = h.state().state.doc().frame(0);
    click(&mut h, labels::SHIFT_DOWN);
    assert_eq!(h.state().state.doc().frame(0), first.shifted(0, 1));
    // The onion skin's choice.
    h.get_by_label(labels::ONION).click();
    h.run();
    click(&mut h, "Previous walk");
    assert_eq!(h.state().state.onion(), olr_skin_editor::Onion::PreviousWalk);
}

#[test]
fn close_asks_when_unsaved() {
    let mut h = copy();
    stroke(&mut h, &[(2, 2)]);
    click(&mut h, labels::CLOSE);
    assert_eq!(h.state().state.dialog(), Some(&Dialog::Unsaved));
    click(&mut h, labels::CANCEL);
    assert_eq!(h.state().state.dialog(), None);
    assert!(!h.state().commands.contains(&C::Close));
    click(&mut h, labels::CLOSE);
    click(&mut h, labels::DONT_SAVE);
    assert_eq!(h.state().commands.last(), Some(&C::Close));
}

#[test]
fn the_preview_shows_light_and_dark_cells() {
    let mut h = copy();
    assert!(h.state().data.preview_shown());
    let first = h.state().state.preview_pose();
    for _ in 0..10 {
        let app = h.state_mut();
        app.state.update(A::Tick(0.1));
        h.run();
    }
    assert_ne!(h.state().state.preview_pose(), first, "it walks and aims");
    assert!(h.state().data.preview_shown());
    click(&mut h, labels::ANIMATE);
    assert!(!h.state().state.animate());
}
