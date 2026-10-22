//! The material editor in the game (issue #44): the host lists the
//! built-in materials and the user's, writes only the user's (New, Save,
//! Rename, Delete; the built-in folder byte for byte as it was), keeps
//! unsaved changes in a recovery copy offered by the next run, recovered,
//! and removed once saved or thrown away; through the shell, the Editor
//! screen's Material tab opens it on the bundled materials and the
//! window's close asks first. The host's tests need no content; the
//! shell's run over the bundled content (`OLR_CONTENT_DIR`; skipped
//! without it).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use olr_format::materials::{MaterialSource, material_text, read_material};
use olr_game::editor::material::{
    MaterialHost, MaterialOut, discard_material_recovery, list_material_recoveries,
};
use olr_game::editor::{CloseDecision, RECOVERY_INTERVAL};
use olr_game::shell::{Shell, ShellConfig};
use olr_material_editor::{Choice, Dialog, Edit, Key, MaterialAction as A, new_material};
use olr_ui::{AppCommand, Profiles, Screen, Settings, Store, UiAction, UiState};

/// A data directory and a built-in library holding Stone.
struct Dirs {
    _tmp: tempfile::TempDir,
    builtin: PathBuf,
    user: PathBuf,
    recovery: PathBuf,
}

fn dirs() -> Dirs {
    let tmp = tempfile::tempdir().unwrap();
    let builtin = tmp.path().join("content/materials");
    std::fs::create_dir_all(&builtin).unwrap();
    std::fs::write(builtin.join("stone.ron"), material_text(&new_material("stone", "Stone")).unwrap())
        .unwrap();
    let user = tmp.path().join("data/materials");
    let recovery = tmp.path().join("data/material-recovery");
    Dirs { builtin, user, recovery, _tmp: tmp }
}

fn open(d: &Dirs, c: &AppCommand) -> MaterialHost {
    MaterialHost::open(c, Some(&d.builtin), &d.user, Some(d.recovery.clone())).unwrap()
}

/// Every file under `dir` by name, with its bytes.
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let Ok(rd) = std::fs::read_dir(dir) else { return BTreeMap::new() };
    rd.map(|e| e.unwrap().path())
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect()
}

/// The user's material `id` as the game reads it.
fn user_file(d: &Dirs, id: &str) -> MaterialSource {
    let text = std::fs::read_to_string(d.user.join(format!("{id}.ron"))).unwrap();
    read_material(&format!("{id}.ron"), &text).unwrap()
}

fn capacity(h: &MaterialHost) -> u16 {
    h.state().doc().unwrap().heat.as_ref().unwrap().capacity
}

#[test]
fn materials_are_listed_made_saved_and_never_built_in() {
    let d = dirs();
    let builtin_before = files(&d.builtin);
    let mut h = open(&d, &AppCommand::OpenMaterialEditor);
    // Only the built-in one, open read-only.
    let lib: Vec<(bool, &str)> =
        h.state().library().iter().map(|e| (e.builtin, e.material.id.as_str())).collect();
    assert_eq!(lib, [(true, "stone")]);
    assert!(h.state().read_only());
    // Saving a built-in one does nothing.
    h.act(vec![A::Set(Edit::Capacity(400)), A::Save]);
    assert!(files(&d.user).is_empty());

    // Edit a copy: written at once to the user's folder.
    h.act(vec![A::Copy, A::Named("Stone copy".into())]);
    assert_eq!(h.state().selected(), Some(&Key::user("stone-copy")));
    assert_eq!(user_file(&d, "stone-copy").name, "Stone copy");
    h.act(vec![A::Set(Edit::Capacity(400)), A::Set(Edit::OwnRamp(true)), A::Save]);
    let saved = user_file(&d, "stone-copy");
    assert_eq!(saved.heat.as_ref().unwrap().capacity, 400);
    assert_eq!(saved, h.state().doc().unwrap().clone());
    assert!(!h.state().dirty());

    // New: an id free of the built-in ones.
    h.act(vec![A::New, A::Named("Stone".into())]);
    assert_eq!(user_file(&d, "stone-2").name, "Stone");
    // A file there already is never written over.
    std::fs::write(d.user.join("basalt.ron"), "not even RON").unwrap();
    h.act(vec![A::New, A::Named("Basalt".into())]);
    assert!(matches!(h.state().dialog(), Some(Dialog::Error(_))), "{:?}", h.state().dialog());
    assert_eq!(std::fs::read(d.user.join("basalt.ron")).unwrap(), b"not even RON");
    h.act(vec![A::Choice(Choice::Cancel)]);

    // Opened again: the built-in one first, then the user's (the bad file
    // a warning, left out).
    let mut h = open(&d, &AppCommand::OpenMaterialEditor);
    let lib: Vec<(bool, &str)> =
        h.state().library().iter().map(|e| (e.builtin, e.material.id.as_str())).collect();
    assert_eq!(lib, [(true, "stone"), (false, "stone-2"), (false, "stone-copy")]);
    assert!(h.take_warnings().iter().any(|w| w.contains("basalt")));
    assert_eq!(files(&d.builtin), builtin_before, "the built-in folder is never written");
    // No temporary file is left beside them.
    assert!(files(&d.user).keys().all(|n| !n.starts_with('.')), "{:?}", files(&d.user).keys());
}

#[test]
fn rename_and_delete_change_the_users_files() {
    let d = dirs();
    let mut h = open(&d, &AppCommand::OpenMaterialEditor);
    h.act(vec![A::New, A::Named("Granite".into()), A::Set(Edit::Capacity(321))]);
    // A rename writes the saved values under the new name; the change stays unsaved.
    h.act(vec![A::Rename, A::Named("Pink granite".into())]);
    assert!(!d.user.join("granite.ron").exists());
    assert_eq!(user_file(&d, "pink-granite").heat.unwrap().capacity, 100);
    assert!(h.state().dirty());
    assert_eq!(capacity(&h), 321);
    h.act(vec![A::Save]);
    assert_eq!(user_file(&d, "pink-granite").heat.unwrap().capacity, 321);
    h.act(vec![A::Delete, A::ConfirmDelete]);
    assert!(files(&d.user).is_empty());
    assert!(h.state().doc().is_none());
    // Close: the editor is done.
    assert_eq!(h.act(vec![A::Close]), vec![MaterialOut::Closed]);
}

#[test]
fn unsaved_changes_are_kept_in_a_recovery_copy() {
    let d = dirs();
    let mut h = open(&d, &AppCommand::OpenMaterialEditor);
    h.act(vec![A::New, A::Named("Lava".into())]);
    let t0 = Instant::now();
    h.act(vec![A::Set(Edit::Capacity(640))]);
    h.tick(t0);
    let copy = d.recovery.join("lava.ron");
    assert!(!copy.exists(), "not before the interval");
    h.tick(t0 + RECOVERY_INTERVAL + Duration::from_secs(1));
    assert!(copy.is_file(), "the copy is written");
    // A crash: the next run offers it.
    drop(h);
    assert_eq!(list_material_recoveries(&d.recovery), [("lava".to_string(), "Lava".to_string(), false)]);
    let mut h = open(&d, &AppCommand::RecoverMaterial("lava".into()));
    assert!(h.state().dirty(), "recovered work is unsaved");
    assert_eq!(capacity(&h), 640);
    // Saved, the copy goes.
    h.act(vec![A::Save]);
    assert_eq!(user_file(&d, "lava").heat.unwrap().capacity, 640);
    assert!(!copy.exists());
    // Thrown away, it goes too.
    h.act(vec![A::Set(Edit::Capacity(700))]);
    h.flush_recovery();
    assert!(copy.is_file(), "a quit that does not ask keeps it");
    h.act(vec![A::Select(Key::builtin("stone")), A::Choice(Choice::DontSave)]);
    assert!(!copy.exists());
    assert_eq!(user_file(&d, "lava").heat.unwrap().capacity, 640);
    // A copy of another material than the one open goes when that one's is written.
    h.act(vec![A::Select(Key::user("lava")), A::Set(Edit::Capacity(710))]);
    h.flush_recovery();
    assert!(copy.is_file());
    assert_eq!(h.act(vec![A::Close, A::Choice(Choice::DontSave)]), vec![MaterialOut::Closed]);
    assert!(!copy.exists(), "Close drops the copy");

    // A damaged copy is listed, never opened, and discarded.
    std::fs::create_dir_all(&d.recovery).unwrap();
    std::fs::write(d.recovery.join("broken.ron"), "(((").unwrap();
    std::fs::write(d.recovery.join("other.ron"), material_text(&new_material("lava", "Lava")).unwrap())
        .unwrap();
    std::fs::write(d.recovery.join(".lava.ron.tmp"), "half").unwrap();
    assert_eq!(
        list_material_recoveries(&d.recovery),
        [
            ("broken".to_string(), "broken".to_string(), true),
            ("other".to_string(), "other".to_string(), true)
        ]
    );
    for id in ["broken", "other", "../escape"] {
        assert!(
            MaterialHost::open(
                &AppCommand::RecoverMaterial(id.into()),
                Some(&d.builtin),
                &d.user,
                Some(d.recovery.clone())
            )
            .is_err()
        );
    }
    discard_material_recovery(&d.recovery, "broken").unwrap();
    assert!(discard_material_recovery(&d.recovery, "../x").is_err());
    assert_eq!(list_material_recoveries(&d.recovery).len(), 1);
}

#[test]
fn the_users_folder_is_never_the_built_in_one() {
    let d = dirs();
    let inside = d.builtin.join("mine");
    assert!(MaterialHost::open(&AppCommand::OpenMaterialEditor, Some(&d.builtin), &inside, None).is_err());
    assert!(MaterialHost::open(&AppCommand::OpenMaterialEditor, Some(&d.builtin), &d.builtin, None).is_err());
}

/// The preview's commands (issue #45) wait for the app, in order and
/// bounded; nothing else answers them.
#[test]
fn the_previews_commands_wait_for_the_app() {
    use olr_game::editor::material::MAX_PREVIEW_COMMANDS;
    use olr_material_editor::{BlastSize, PreviewAction as P, PreviewCommand as PC, TimeScale};
    let d = dirs();
    let mut h = open(&d, &AppCommand::OpenMaterialEditor);
    assert!(h.take_preview().is_empty());
    let out = h.act(vec![
        A::Preview(P::Blast([3, 4])),
        A::Preview(P::Scale(TimeScale::Four)),
        A::Preview(P::Compare(Some(Key::builtin("stone")))),
        A::Preview(P::Restart),
    ]);
    assert!(out.is_empty());
    assert_eq!(
        h.take_preview(),
        [
            PC::Blast { at: [3, 4], size: BlastSize::Bazooka },
            PC::TimeScale(4),
            PC::SetCompare(Some(new_material("stone", "Stone"))),
            PC::Restart,
        ]
    );
    assert!(h.take_preview().is_empty(), "taken once");
    // An app that never takes them keeps a bounded few.
    h.act((0..MAX_PREVIEW_COMMANDS + 10).map(|_| A::Preview(P::Restart)).collect());
    assert_eq!(h.take_preview().len(), MAX_PREVIEW_COMMANDS);
    assert!(files(&d.user).is_empty(), "the preview writes nothing");
}

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// A shell on a fresh data directory, on the Editor screen.
fn shell() -> Option<(tempfile::TempDir, Shell)> {
    let root = content()?;
    let tmp = tempfile::tempdir().unwrap();
    let cfg = ShellConfig {
        content: root,
        interpolation: olr_scene::Interpolation::Linear,
        viewport: olr_scene::Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let mut sh = Shell::new(cfg, ui, Store::open(tmp.path()), olr_audio::Audio::silent());
    sh.act(UiAction::Open(Screen::Editor));
    Some((tmp, sh))
}

#[test]
fn the_material_tab_opens_the_editor_on_the_bundled_materials() {
    let Some((tmp, mut sh)) = shell() else { return };
    sh.act(UiAction::OpenMaterialEditor);
    assert!(sh.material_editor_open(), "{:?}", sh.data.editor_message);
    let h = sh.material_editor().unwrap();
    // The preview carves with the dirt theme's holes (issue #45).
    assert_eq!(h.holes().map(Vec::len), Some(5));
    let builtin: Vec<&str> =
        h.state().library().iter().filter(|e| e.builtin).map(|e| e.material.id.as_str()).collect();
    for id in ["stone", "wood", "metal", "glass"] {
        assert!(builtin.contains(&id), "{id}: {builtin:?}");
    }
    // Edit a copy of wood, changed: the window's close asks first.
    let t = Instant::now();
    sh.material_frame(
        vec![A::Select(Key::builtin("wood")), A::Copy, A::Named("Oak".into()), A::Set(Edit::Cooling(5))],
        t,
    );
    let oak = tmp.path().join("materials/oak.ron");
    assert!(oak.is_file());
    assert_eq!(sh.request_close(), CloseDecision::Wait);
    assert!(matches!(sh.material_editor().unwrap().state().dialog(), Some(Dialog::Unsaved(_))));
    // Saved and closed: the Editor screen again, nothing to recover.
    sh.material_frame(vec![A::Choice(Choice::Save)], t);
    assert!(!sh.material_editor_open());
    assert_eq!(sh.ui.screen(), Screen::Editor);
    let text = std::fs::read_to_string(&oak).unwrap();
    assert_eq!(read_material("oak.ron", &text).unwrap().heat.unwrap().cooling, 5);
    assert!(sh.data.material_recoveries.is_empty());
    assert_eq!(sh.request_close(), CloseDecision::Quit);

    // A crash's copy is offered on the tab, and Discard removes it.
    sh.act(UiAction::OpenMaterialEditor);
    sh.material_frame(vec![A::Select(Key::user("oak")), A::Set(Edit::Cooling(9))], t);
    // A quit that does not ask writes it (`Shell::finish`).
    sh.material_editor().unwrap().flush_recovery();
    sh.close_material_editor();
    assert_eq!(sh.data.material_recoveries, [("oak".to_string(), "Oak".to_string(), false)]);
    sh.act(UiAction::RecoverMaterial("oak".into()));
    assert!(sh.material_editor().unwrap().state().dirty());
    sh.close_material_editor();
    sh.act(UiAction::DiscardMaterialRecovery("oak".into()));
    assert!(sh.data.material_recoveries.is_empty());
    assert!(!tmp.path().join("material-recovery/oak.ron").exists());
}
