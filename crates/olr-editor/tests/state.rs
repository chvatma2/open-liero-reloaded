//! The editor state (plan 14b Task 6): files, undo, play.

use olr_edit::element::Property;
use olr_editor::{Choice, Dialog, EditorAction as A, EditorCommand as C};

use crate::common::{content, editor, run};

fn upload(cmds: &[C]) -> bool {
    cmds.iter().any(|c| matches!(c, C::Upload(_)))
}

#[test]
fn open_edit_save_round_trip() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let dir = s.document().unwrap().dir().to_path_buf();
    assert!(upload(&run(&mut s, A::AddLayer)));
    assert!(s.dirty());
    let cmds = run(&mut s, A::Save);
    assert!(cmds.iter().any(|c| matches!(c, C::Save { .. })));
    assert!(!s.dirty());
    let ron = std::fs::read_to_string(dir.join("editor/asset.ron")).unwrap();
    assert!(ron.contains("id: \"layer\""), "{ron}");
    let errors: Vec<_> = olr_pack::validate(&olr_vfs::DirFs::new(&dir))
        .into_iter()
        .filter(|d| d.severity == olr_pack::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn unsaved_changes_ask_before_close() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    assert!(run(&mut s, A::Close).is_empty());
    assert!(matches!(s.dialog(), Some(Dialog::Unsaved { .. })));
    assert!(run(&mut s, A::Choose(Choice::Cancel)).is_empty());
    assert!(s.dialog().is_none());
    run(&mut s, A::Close);
    assert_eq!(run(&mut s, A::Choose(Choice::Discard)), vec![C::RecoveryDone, C::Close]);

    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    run(&mut s, A::Close);
    let cmds = run(&mut s, A::Choose(Choice::Save));
    assert!(matches!(cmds.as_slice(), [C::Save { .. }, C::RecoveryDone, C::Close]), "{cmds:?}");
    assert!(!s.dirty());
}

#[test]
fn a_failed_save_keeps_the_changes() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    s.update(A::Save);
    s.update(A::Saved(Err("disk full".into())));
    assert!(s.dirty());
    assert!(matches!(s.dialog(), Some(Dialog::Error(e)) if e.contains("disk full")));
}

#[test]
fn play_hands_over_the_bake_and_changes_nothing() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let before = s.document().unwrap().level().clone();
    let cmds = run(&mut s, A::Play);
    let [C::Play { level, mod_id, bots }] = cmds.as_slice() else { panic!("{cmds:?}") };
    assert_eq!((mod_id.as_str(), *bots), ("olx-classic", 1));
    assert!(**level == before);
    assert!(s.playing());
    assert!(run(&mut s, A::AddLayer).is_empty(), "ignored while playing");
    run(&mut s, A::PlayEnded);
    assert!(!s.playing());
    assert!(*s.document().unwrap().level() == before);
    assert_eq!(s.document().unwrap().items().len(), 6);
    let cmds = run(&mut s, A::PlayBots(12));
    assert_eq!(s.settings().play_bots, 8);
    assert!(matches!(cmds.as_slice(), [C::SaveSettings(set)] if set.play_bots == 8));
}

#[test]
fn undo_redo_through_actions() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let name = |s: &olr_editor::EditorState| {
        let d = s.document().unwrap();
        d.items()[d.index_of("tiny").unwrap()].element.name.clone()
    };
    assert!(upload(&run(&mut s, A::Set("tiny".into(), Property::Name("Small".into())))));
    assert_eq!(name(&s), "Small");
    assert!(s.can_undo());
    assert!(upload(&run(&mut s, A::Undo)));
    assert_eq!(name(&s), "Tiny");
    assert!(s.can_redo());
    assert!(upload(&run(&mut s, A::Redo)));
    assert_eq!(name(&s), "Small");
}

#[test]
fn a_refused_edit_says_why() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let cmds = run(&mut s, A::Set("base".into(), Property::PixelSize(2)));
    assert!(cmds.is_empty());
    let message = s.status().message.clone().unwrap_or_default();
    assert!(message.contains("is not an image element"), "{message}");
    assert!(!s.can_undo());
}

#[test]
fn new_creates_and_opens_a_level() {
    let Some(root) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut s =
        olr_editor::EditorState::new(olr_editor::EditorSettings::default(), crate::common::catalog(&root));
    let n = olr_editor::NewLevel {
        dir: tmp.path().join("mine"),
        id: "mine".into(),
        name: "Mine".into(),
        size: (64, 48),
        theme: "olx-theme-dirt".into(),
    };
    run(&mut s, A::New(n));
    assert_eq!(s.document().unwrap().level().size, (64, 48));
    assert!(!s.dirty());
}

/// Final review: an edit made after a Save was sent stays unsaved when the
/// save's answer arrives.
#[test]
fn an_edit_during_a_save_stays_unsaved() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    let cmds = s.update(A::Save);
    assert!(matches!(cmds.as_slice(), [C::Save { .. }]));
    run(&mut s, A::AddLayer);
    s.update(A::Saved(Ok(Vec::new())));
    assert!(s.dirty(), "the second layer was not in the saved pack");
}

/// A second save sent before the first one's answer keeps the first's
/// follow-up (the Close it was asked for).
#[test]
fn a_second_save_keeps_the_first_ones_follow_up() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    run(&mut s, A::Close);
    assert!(matches!(s.update(A::Choose(Choice::Save)).as_slice(), [C::Save { .. }]));
    assert!(matches!(s.update(A::Save).as_slice(), [C::Save { .. }]));
    assert_eq!(s.update(A::Saved(Ok(Vec::new()))), vec![C::RecoveryDone, C::Close]);
    assert_eq!(s.update(A::Saved(Ok(Vec::new()))), vec![C::RecoveryDone]);
}

/// While a dialog is open only its answer, the host's answers and the view
/// act: edits wait.
#[test]
fn edits_wait_behind_a_dialog() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    run(&mut s, A::Close);
    assert!(s.dialog().is_some());
    let items = s.document().unwrap().items().len();
    assert!(run(&mut s, A::Set("tiny".into(), Property::Name("x".into()))).is_empty());
    assert!(run(&mut s, A::AddLayer).is_empty());
    assert!(run(&mut s, A::Undo).is_empty());
    assert_eq!(s.document().unwrap().items().len(), items);
    run(&mut s, A::Choose(Choice::Cancel));
    assert!(!run(&mut s, A::Undo).is_empty(), "after the dialog, edits act again");
}

/// An edit that changes nothing records no step and leaves the level clean.
#[test]
fn an_unchanged_value_is_no_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    assert!(run(&mut s, A::Set("tiny".into(), Property::Name("Tiny".into()))).is_empty());
    assert!(run(&mut s, A::MoveElement { id: "tiny".into(), to: 1 }).is_empty());
    assert!(!s.can_undo() && !s.dirty());
}

/// Plan 14d: merge down and split, from actions, are one step each.
#[test]
fn merge_down_and_split_are_one_step_each() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let ids = |s: &olr_editor::EditorState| -> Vec<String> {
        s.document().unwrap().items().iter().map(|i| i.element.id.clone()).collect()
    };
    let start = ids(&s);
    run(&mut s, A::MergeDown("wall".into()));
    assert_eq!(ids(&s).len(), start.len() - 1, "wall merged into tunnels");
    run(&mut s, A::Undo);
    assert_eq!(ids(&s), start);
    run(&mut s, A::Split("tiny".into()));
    assert!(!ids(&s).contains(&"tiny".to_string()));
    run(&mut s, A::Undo);
    assert_eq!(ids(&s), start);
}

fn texture(name: &str) -> olr_editor::catalog::TextureEntry {
    olr_editor::catalog::TextureEntry { name: name.into(), image: olr_data::image::RgbaImage::new(4, 4) }
}

#[test]
fn textures_changed_replaces_the_user_library() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::TexturesChanged(vec![texture("a"), texture("b")]));
    let names: Vec<_> = s.catalog().textures.iter().map(|t| t.name.clone()).collect();
    assert_eq!(names, ["a", "b"]);
    run(&mut s, A::TexturesChanged(vec![texture("c")]));
    assert_eq!(s.catalog().textures, vec![texture("c")]);
}

#[test]
fn a_removed_texture_falls_back_to_the_colour() {
    use olr_editor::tools::{PaintChoice, TextureRef};
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::TexturesChanged(vec![texture("a")]));
    run(&mut s, A::Paint(PaintChoice::Texture(TextureRef::User("a".into()))));
    assert_eq!(s.paint(), &PaintChoice::Texture(TextureRef::User("a".into())));
    run(&mut s, A::TexturesChanged(vec![texture("b")]));
    assert_eq!(s.paint(), &PaintChoice::default());
    // Asking for a texture that is not there is the colour too.
    run(&mut s, A::Paint(PaintChoice::Texture(TextureRef::Builtin(99))));
    assert_eq!(s.paint(), &PaintChoice::default());
}

#[test]
fn removing_a_texture_asks_the_host() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    assert_eq!(s.update(A::RemoveTexture("a".into())), vec![C::RemoveTexture("a".into())]);
}

fn at(s: &olr_editor::EditorState, id: &str) -> (i32, i32) {
    let d = s.document().unwrap();
    match &d.items()[d.index_of(id).unwrap()].element.kind {
        olr_format::levels::ElementKind::Image { at, .. } => *at,
        olr_format::levels::ElementKind::Drawn { at, .. } => *at,
        k => panic!("{k:?}"),
    }
}

fn group(ids: &[&str]) -> A {
    A::Select(olr_editor::Selection::Many(ids.iter().map(|s| s.to_string()).collect()))
}

/// Plan 14e R8: a group moves as one step.
#[test]
fn a_group_moves_and_undoes_as_one_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let (t, g) = (at(&s, "tiny"), at(&s, "gradient"));
    run(&mut s, group(&["tiny", "gradient"]));
    run(&mut s, A::Nudge { dx: 2, dy: 0 });
    assert_eq!((at(&s, "tiny"), at(&s, "gradient")), ((t.0 + 2, t.1), (g.0 + 2, g.1)));
    run(&mut s, A::Undo);
    assert_eq!((at(&s, "tiny"), at(&s, "gradient")), (t, g));
    assert!(!s.can_undo(), "one step");
}

/// Review Focus 4: a locked member refuses moves and deletes, naming it;
/// unlocking applies to all, as one step.
#[test]
fn a_group_with_a_locked_member_refuses_moves() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Set("tiny".into(), Property::Locked(true)));
    let (t, g) = (at(&s, "tiny"), at(&s, "gradient"));
    let items = s.document().unwrap().items().len();
    run(&mut s, group(&["tiny", "gradient"]));
    assert!(run(&mut s, A::Nudge { dx: 2, dy: 0 }).is_empty());
    assert_eq!((at(&s, "tiny"), at(&s, "gradient")), (t, g));
    assert!(s.status().message.as_deref().is_some_and(|m| m.contains("tiny")), "{:?}", s.status().message);
    run(&mut s, A::Remove);
    assert_eq!(s.document().unwrap().items().len(), items);
    assert!(s.status().message.as_deref().is_some_and(|m| m.contains("tiny")));
    run(&mut s, A::Set("gradient".into(), Property::Locked(true)));
    run(&mut s, A::SetMany(vec!["tiny".into(), "gradient".into()], Property::Locked(false)));
    let locked = |s: &olr_editor::EditorState, id: &str| {
        let d = s.document().unwrap();
        d.items()[d.index_of(id).unwrap()].element.locked
    };
    assert!(!locked(&s, "tiny") && !locked(&s, "gradient"));
    run(&mut s, A::Undo);
    assert!(locked(&s, "tiny") && locked(&s, "gradient"), "one step relocks both");
}

#[test]
fn remove_many_is_one_step() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let ids = |s: &olr_editor::EditorState| -> Vec<String> {
        s.document().unwrap().items().iter().map(|i| i.element.id.clone()).collect()
    };
    let before = ids(&s);
    run(&mut s, group(&["badge", "tiny"]));
    assert_eq!(
        s.selection(),
        &olr_editor::Selection::Many(vec!["tiny".into(), "badge".into()]),
        "in stack order"
    );
    run(&mut s, A::Remove);
    assert_eq!(ids(&s), ["base", "gradient", "tunnels", "wall"]);
    assert_eq!(s.selection(), &olr_editor::Selection::None);
    run(&mut s, A::Undo);
    assert_eq!(ids(&s), before);
}

/// The list's Ctrl and Shift clicks (plan 14e R8).
#[test]
fn toggles_and_ranges_select_groups() {
    use olr_editor::Selection;
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::SelectToggle("gradient".into()));
    assert_eq!(s.selection(), &Selection::Element("gradient".into()));
    run(&mut s, A::SelectToggle("tiny".into()));
    assert_eq!(s.selection(), &Selection::Many(vec!["tiny".into(), "gradient".into()]));
    run(&mut s, A::SelectToggle("gradient".into()));
    assert_eq!(s.selection(), &Selection::Element("tiny".into()));
    run(&mut s, A::SelectRange("tunnels".into()));
    assert_eq!(
        s.selection(),
        &Selection::Many(vec!["tiny".into(), "gradient".into(), "badge".into(), "tunnels".into()])
    );
}

fn ids_of(s: &olr_editor::EditorState) -> Vec<String> {
    s.document().unwrap().items().iter().map(|i| i.element.id.clone()).collect()
}

#[test]
fn cut_and_paste_are_one_step_each() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let before = ids_of(&s);
    run(&mut s, A::Select(olr_editor::Selection::Element("tiny".into())));
    run(&mut s, A::Cut);
    assert!(!ids_of(&s).contains(&"tiny".to_string()));
    run(&mut s, A::Undo);
    assert_eq!(ids_of(&s), before, "one undo brings it back");
    run(&mut s, A::Redo);
    run(&mut s, A::Paste);
    let pasted = ids_of(&s).last().cloned().unwrap();
    assert_eq!(s.selection(), &olr_editor::Selection::Element(pasted.clone()), "the copy is selected");
    run(&mut s, A::Undo);
    assert!(!ids_of(&s).contains(&pasted));
}

#[test]
fn the_clipboard_survives_opening_another_level() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(olr_editor::Selection::Many(vec!["tiny".into(), "tunnels".into()])));
    run(&mut s, A::Copy);
    let (tmp2, dir) = crate::common::copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    run(&mut s, A::Open(dir));
    assert!(s.document().unwrap().index_of("tiny").is_none());
    run(&mut s, A::Paste);
    let ids = ids_of(&s);
    assert_eq!(&ids[ids.len() - 2..], ["tiny-2", "tunnels-2"]);
    drop(tmp2);
}

#[test]
fn pastes_cascade() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    let t = at(&s, "tiny");
    run(&mut s, A::Select(olr_editor::Selection::Element("tiny".into())));
    run(&mut s, A::Copy);
    run(&mut s, A::Paste);
    run(&mut s, A::Paste);
    let ids = ids_of(&s);
    let (a, b) = (&ids[ids.len() - 2], &ids[ids.len() - 1]);
    assert_eq!(at(&s, a), (t.0 + 4, t.1 + 4));
    assert_eq!(at(&s, b), (t.0 + 8, t.1 + 8));
}

/// Minors: a fill that converts the layer is undone as one step.
#[test]
fn a_fill_and_its_conversion_are_one_step() {
    use olr_format::levels::ElementKind;
    let Some(root) = content() else { return };
    let (_tmp, mut s) = crate::common::editor_at(&root, std::path::Path::new("olx-level-dirt-level"));
    let plain = |s: &olr_editor::EditorState| {
        let d = s.document().unwrap();
        matches!(d.items()[d.index_of("dirt").unwrap()].element.kind, ElementKind::Drawn { mixed: false, .. })
    };
    let art = s.document().unwrap().layer_art("dirt").unwrap().clone();
    let l = s.document().unwrap().level();
    let i = l.material.iter().position(|m| *m == olr_data::level::Material::Dirt).unwrap();
    let p = ((i % l.size.0 as usize) as i32, (i / l.size.0 as usize) as i32);
    run(&mut s, A::Select(olr_editor::Selection::Element("dirt".into())));
    run(&mut s, A::Tool(olr_editor::tools::Tool::Fill));
    run(&mut s, A::PaintMaterial(Some(olr_format::levels::ElementMaterial::Rock)));
    run(
        &mut s,
        A::Pointer { kind: olr_editor::tools::PointerKind::Down, world: p, shift: false, toggle: false },
    );
    run(
        &mut s,
        A::Pointer { kind: olr_editor::tools::PointerKind::Up, world: p, shift: false, toggle: false },
    );
    assert!(!plain(&s), "the fill made it mixed");
    run(&mut s, A::Undo);
    assert!(plain(&s) && s.document().unwrap().layer_art("dirt").unwrap() == &art);
    assert!(!s.can_undo(), "one step");
}

/// Minors: Copy with nothing to copy says so.
#[test]
fn copying_nothing_says_so() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Copy);
    assert!(
        s.status().message.as_deref().is_some_and(|m| m.contains("nothing to copy")),
        "{:?}",
        s.status().message
    );
    run(&mut s, A::Cut);
    assert!(s.status().message.as_deref().is_some_and(|m| m.contains("nothing to copy")));
}

/// Minors: redoing a paste that brought an original brings it again.
#[test]
fn redoing_a_paste_brings_its_original() {
    let Some(root) = content() else { return };
    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::Select(olr_editor::Selection::Element("tiny".into())));
    run(&mut s, A::Copy);
    let (_t2, dir) = crate::common::copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    run(&mut s, A::Open(dir));
    run(&mut s, A::Paste);
    let has = |s: &olr_editor::EditorState| {
        crate::common::files(s.document().unwrap()).contains_key("editor/images/tiny.jpg")
    };
    assert!(has(&s));
    run(&mut s, A::Undo);
    assert!(!has(&s), "no element names it");
    run(&mut s, A::Redo);
    assert!(has(&s), "redone, it is back");
}
