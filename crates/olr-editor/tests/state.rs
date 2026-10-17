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
    let ron = std::fs::read_to_string(dir.join("editor/composition.ron")).unwrap();
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
    assert_eq!(run(&mut s, A::Choose(Choice::Discard)), vec![C::Close]);

    let (_tmp, mut s) = editor(&root);
    run(&mut s, A::AddLayer);
    run(&mut s, A::Close);
    let cmds = run(&mut s, A::Choose(Choice::Save));
    assert!(matches!(cmds.as_slice(), [C::Save { .. }, C::Close]), "{cmds:?}");
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
    assert_eq!(s.document().unwrap().items().len(), 5);
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
