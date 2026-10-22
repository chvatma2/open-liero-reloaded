//! The live heat preview's controls (issue #45): the state's commands to
//! the host's preview, the material compared following the library, and
//! the panel's controls driven by label (`egui_kittest`).

use egui_kittest::kittest::Queryable;
use olr_material_editor::preview::{PATCH, SCENE};
use olr_material_editor::ui::labels;
use olr_material_editor::{
    BlastSize, Key, MaterialAction as A, MaterialCommand as C, MaterialEditorState, PreviewAction as P,
    PreviewCommand as PC, TimeScale,
};

use crate::common::{library, material};
use crate::flows::{click, harness};

fn preview(s: &mut MaterialEditorState, a: P) -> Vec<C> {
    s.update(A::Preview(a))
}

fn command(c: PC) -> Vec<C> {
    vec![C::Preview(c)]
}

#[test]
fn the_controls_ask_the_hosts_preview() {
    let mut s = MaterialEditorState::new(library());
    assert_eq!(s.preview().size, BlastSize::Bazooka);
    assert_eq!(s.preview().scale, TimeScale::One);
    assert_eq!(s.preview().compare, None);
    assert_eq!(
        preview(&mut s, P::Blast([10, 20])),
        command(PC::Blast { at: [10, 20], size: BlastSize::Bazooka })
    );
    assert!(preview(&mut s, P::Size(BlastSize::Nuke)).is_empty());
    assert_eq!(preview(&mut s, P::Blast([1, 2])), command(PC::Blast { at: [1, 2], size: BlastSize::Nuke }));
    assert_eq!(preview(&mut s, P::Scale(TimeScale::Four)), command(PC::TimeScale(4)));
    assert!(preview(&mut s, P::Scale(TimeScale::Four)).is_empty(), "no change, nothing asked");
    assert_eq!(preview(&mut s, P::Scale(TimeScale::One)), command(PC::TimeScale(1)));
    assert_eq!(preview(&mut s, P::Restart), command(PC::Restart));
    // A dialog open: the preview still acts, and never touches the material.
    s.update(A::New);
    assert!(s.dialog().is_some());
    assert_eq!(preview(&mut s, P::Restart), command(PC::Restart));
    assert!(!s.dirty());
    // Even a built-in material's preview blasts.
    s.update(A::Choice(olr_material_editor::Choice::Cancel));
    s.update(A::Select(Key::builtin("stone")));
    assert_eq!(preview(&mut s, P::Blast([0, 0])).len(), 1);
}

#[test]
fn the_material_compared_follows_the_library() {
    let mut s = MaterialEditorState::new(library());
    let stone = material("stone", "Stone", 100);
    assert_eq!(
        preview(&mut s, P::Compare(Some(Key::builtin("stone")))),
        command(PC::SetCompare(Some(stone)))
    );
    assert!(preview(&mut s, P::Compare(Some(Key::builtin("stone")))).is_empty(), "the same: nothing");
    assert_eq!(preview(&mut s, P::Compare(Some(Key::builtin("nothing")))), command(PC::SetCompare(None)));
    assert_eq!(s.preview().compare, None, "a key not listed compares with none");
    // The user's Lava rock, open and compared: saving it sends it again.
    let lava = s.doc().unwrap().clone();
    assert_eq!(
        preview(&mut s, P::Compare(Some(Key::user("lava-rock")))),
        command(PC::SetCompare(Some(lava)))
    );
    s.update(A::Set(olr_material_editor::Edit::Capacity(300)));
    let edited = s.doc().unwrap().clone();
    let out = s.update(A::Save);
    assert_eq!(out, vec![C::Save(edited.clone())]);
    let out = s.update(A::Saved(edited.clone()));
    assert_eq!(out, command(PC::SetCompare(Some(edited.clone()))), "the library's copy changed");
    // Renamed: still compared, under its new id.
    let to = olr_format::materials::MaterialSource { id: "magma".into(), name: "Magma".into(), ..edited };
    let out = s.update(A::Renamed { from: "lava-rock".into(), to: to.clone() });
    assert_eq!(out, command(PC::SetCompare(Some(to))));
    assert_eq!(s.preview().compare, Some(Key::user("magma")));
    // Deleted: none.
    assert_eq!(s.update(A::Deleted("magma".into())), command(PC::SetCompare(None)));
    assert_eq!(s.preview().compare, None);
}

#[test]
fn the_panel_blasts_and_sets_its_controls_by_label() {
    let mut h = harness(MaterialEditorState::new(library()));
    let previews = |h: &egui_kittest::Harness<'static, crate::flows::App>| -> Vec<PC> {
        h.state()
            .commands
            .iter()
            .filter_map(|c| match c {
                C::Preview(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    };
    // The picture's middle is the wall: a click there blasts nothing.
    click(&mut h, labels::PREVIEW);
    assert_eq!(previews(&h), vec![]);
    // A click on the left patch's middle.
    let r = h.get_by_label(labels::PREVIEW).rect();
    let at = egui::pos2(
        r.min.x + (PATCH.0 as f32 / 2.0 + 0.5) / SCENE.0 as f32 * r.width(),
        r.min.y + (PATCH.1 as f32 / 2.0 + 0.5) / SCENE.1 as f32 * r.height(),
    );
    h.hover_at(at);
    h.run();
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    let half = [PATCH.0 / 2, PATCH.1 / 2];
    assert_eq!(previews(&h), vec![PC::Blast { at: half, size: BlastSize::Bazooka }]);
    click(&mut h, BlastSize::Nuke.label());
    click(&mut h, TimeScale::Four.label());
    click(&mut h, labels::RESTART);
    assert_eq!(h.state().state.preview().size, BlastSize::Nuke);
    // The material compared, picked from the library.
    click(&mut h, labels::COMPARE);
    click(&mut h, &labels::compare_entry("Wood", true));
    assert_eq!(h.state().state.preview().compare, Some(Key::builtin("wood")));
    click(&mut h, labels::COMPARE);
    click(&mut h, labels::COMPARE_NONE);
    let wood = material("wood", "Wood", 50);
    assert_eq!(
        previews(&h)[1..],
        [PC::TimeScale(4), PC::Restart, PC::SetCompare(Some(wood)), PC::SetCompare(None)]
    );
}
