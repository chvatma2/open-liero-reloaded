//! The level editor's host without a window (plan 14c Task 5): packs made
//! and copied in the data directory's `packs/`, bundled content never
//! written, the recovery copy, bounded imports and the remembered
//! settings. A temporary data directory over the bundled content
//! (`OLR_CONTENT_DIR`; skipped without it).

use std::collections::BTreeMap;
use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use olr_editor::EditorAction as A;
use olr_editor::browser::Purpose;
use olr_game::editor::{
    EDITOR_FILE, EditorHost, HostOut, MAX_IMPORT_BYTES, RECOVERY_INTERVAL, RecoveryInfo, discard_recovery,
    editable_levels, list_recoveries,
};
use olr_ui::{AppCommand, Store};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// A host on a fresh data directory over the bundled content.
fn host() -> Option<(tempfile::TempDir, PathBuf, EditorHost)> {
    let root = content()?;
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path());
    let packs = store.packs_dir().unwrap();
    let host = EditorHost::new(&mut store, &[packs.as_path(), root.as_path()]).unwrap();
    Some((tmp, root, host))
}

/// Every file under `dir` by relative path, with its length and a hash.
fn digest(dir: &Path) -> BTreeMap<String, (u64, u64)> {
    fn go(base: &Path, dir: &Path, out: &mut BTreeMap<String, (u64, u64)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out);
            } else {
                let bytes = std::fs::read(&p).unwrap();
                let mut h = std::hash::DefaultHasher::new();
                bytes.hash(&mut h);
                out.insert(
                    p.strip_prefix(base).unwrap().display().to_string(),
                    (bytes.len() as u64, h.finish()),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

fn new_level(name: &str) -> AppCommand {
    AppCommand::NewLevel { name: name.into(), size: (120, 80), theme: "olx-theme-dirt".into() }
}

fn messages(out: &[HostOut]) -> Vec<&str> {
    out.iter()
        .filter_map(|o| match o {
            HostOut::Message(m) => Some(m.as_str()),
            _ => None,
        })
        .collect()
}

fn open_dir(h: &EditorHost) -> PathBuf {
    h.state().document().expect("a document is open").dir().to_path_buf()
}

#[test]
fn a_new_level_is_made_in_packs_and_opened() {
    let Some((tmp, _root, mut h)) = host() else { return };
    let out = h.open_from_menu(&new_level("Office"));
    assert!(out.contains(&HostOut::FullUpload), "{out:?}");
    let packs = tmp.path().join("packs");
    assert_eq!(open_dir(&h), packs.join("office"));
    assert!(packs.join("office/pack.ron").is_file());
    assert!(!h.state().dirty());
    assert_eq!(editable_levels(&packs).into_iter().collect::<Vec<_>>(), ["office"]);
    h.open_from_menu(&new_level("Office"));
    assert_eq!(open_dir(&h), packs.join("office-2"));
}

#[test]
fn a_bundled_level_is_edited_as_a_copy() {
    let Some((tmp, root, mut h)) = host() else { return };
    let before = digest(&root);
    let out = h.open_from_menu(&AppCommand::CopyLevel {
        from: "olx-level-dirt-level".into(),
        name: "Dirt copy".into(),
    });
    assert!(messages(&out).is_empty(), "{out:?}");
    let dir = tmp.path().join("packs/dirt-copy");
    assert_eq!(open_dir(&h), dir);
    h.act(vec![A::AddLayer]);
    assert!(h.state().dirty());
    let out = h.act(vec![A::Save]);
    assert!(messages(&out).is_empty(), "{out:?}");
    assert!(!h.state().dirty(), "saved");
    assert!(dir.join("editor/composition.ron").is_file(), "the composition is saved there");
    // Editing a bundled level in place is refused: its Edit is not offered,
    // and opening its folder through the browser is refused too.
    let out = h.open_from_menu(&AppCommand::EditLevel("olx-level-dirt-level".into()));
    assert!(!messages(&out).is_empty(), "{out:?}");
    assert_eq!(open_dir(&h), dir, "the copy stays open");
    h.act(vec![A::Open(root.join("olx-level-dirt-level"))]);
    assert!(matches!(h.state().dialog(), Some(olr_editor::Dialog::Error(_))), "{:?}", h.state().dialog());
    assert_eq!(open_dir(&h), dir);
    assert_eq!(digest(&root), before, "content/packs is byte-identical");
}

#[test]
fn recovery_is_written_while_unsaved_and_dropped_on_save() {
    let Some((tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    let recovery = tmp.path().join("recovery/office");
    let start = Instant::now();
    // A clean document never writes.
    h.tick(start);
    h.tick(start + RECOVERY_INTERVAL * 2);
    assert!(!recovery.exists());
    h.act(vec![A::AddLayer]);
    let t0 = start + RECOVERY_INTERVAL * 3;
    h.tick(t0);
    h.tick(t0 + RECOVERY_INTERVAL - Duration::from_secs(1));
    assert!(!recovery.exists(), "not before the interval");
    h.tick(t0 + RECOVERY_INTERVAL);
    assert!(recovery.join("pack.ron").is_file(), "written");
    let info: RecoveryInfo =
        ron::from_str(&std::fs::read_to_string(recovery.join("recovery.ron")).unwrap()).unwrap();
    assert_eq!(info.origin, tmp.path().join("packs/office"));
    assert_eq!(
        list_recoveries(&tmp.path().join("recovery")),
        [("office".to_string(), info.name.clone(), false)]
    );
    h.act(vec![A::Save]);
    assert!(!recovery.exists(), "a save drops it");
}

#[test]
fn recovery_survives_a_missing_original() {
    let Some((tmp, root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    h.act(vec![A::AddLayer]);
    let t0 = Instant::now();
    h.tick(t0);
    h.tick(t0 + RECOVERY_INTERVAL);
    let packs = tmp.path().join("packs");
    // The game is gone (killed), and so is the pack.
    drop(h);
    std::fs::remove_dir_all(packs.join("office")).unwrap();
    let mut store = Store::open(tmp.path());
    let mut h = EditorHost::new(&mut store, &[packs.as_path(), root.as_path()]).unwrap();
    let out = h.open_from_menu(&AppCommand::Recover("office".into()));
    assert!(out.contains(&HostOut::FullUpload), "{out:?}");
    assert!(h.state().dirty(), "recovered changes are unsaved");
    assert_eq!(open_dir(&h), packs.join("office"));
    let layers = h.state().document().unwrap().items().len();
    assert_eq!(layers, 2, "the base and the layer added");
    h.act(vec![A::Save]);
    assert_eq!(h.state().dialog(), None);
    assert!(packs.join("office/pack.ron").is_file(), "Save makes the pack again");
    assert!(!packs.join("office/recovery.ron").exists(), "the recovery's own file is not saved in the pack");
    assert!(!tmp.path().join("recovery/office").exists());
}

#[test]
fn a_damaged_recovery_is_discarded_not_fatal() {
    let Some((tmp, _root, mut h)) = host() else { return };
    let dir = tmp.path().join("recovery");
    std::fs::create_dir_all(dir.join("x")).unwrap();
    std::fs::write(dir.join("x/recovery.ron"), "garbage(").unwrap();
    std::fs::create_dir_all(dir.join("y")).unwrap();
    assert_eq!(list_recoveries(&dir), [("x".into(), "x".into(), true), ("y".into(), "y".into(), true)]);
    let out = h.open_from_menu(&AppCommand::Recover("x".into()));
    assert!(!messages(&out).is_empty(), "{out:?}");
    assert!(h.state().document().is_none());
    // Not an id: refused, nothing outside `recovery/` touched.
    assert!(discard_recovery(&dir, "../packs").is_err());
    discard_recovery(&dir, "x").unwrap();
    assert!(!dir.join("x").exists());
    assert_eq!(list_recoveries(&dir).len(), 1);
}

#[test]
fn imports_are_bounded_before_reading() {
    let Some((tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    let pics = tmp.path().join("pics");
    std::fs::create_dir_all(&pics).unwrap();
    let big = std::fs::File::create(pics.join("big.png")).unwrap();
    big.set_len(2 << 30).unwrap();
    std::fs::write(pics.join("text.png"), "not an image at all").unwrap();
    let pick = |h: &mut EditorHost, name: &str| {
        h.act(vec![A::Browse(Purpose::ImportImage)]);
        h.act(vec![A::BrowseTo(pics.clone())]);
        h.act(vec![A::Pick(name.into())]);
        let d = h.state().dialog().cloned();
        h.act(vec![A::Choose(olr_editor::Choice::Cancel)]);
        d
    };
    let d = pick(&mut h, "big.png");
    assert!(matches!(&d, Some(olr_editor::Dialog::Error(e)) if e.contains("64 MiB")), "{d:?}");
    let d = pick(&mut h, "text.png");
    assert!(matches!(&d, Some(olr_editor::Dialog::Error(e)) if e.contains("not a PNG or JPEG")), "{d:?}");
    assert_eq!(h.state().document().unwrap().items().len(), 1, "nothing imported");
    assert_eq!(MAX_IMPORT_BYTES, 64 << 20);
}

#[test]
fn listing_answers_the_browser() {
    let Some((tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    h.act(vec![A::Browse(Purpose::OpenPack)]);
    h.act(vec![A::BrowseTo(tmp.path().join("packs"))]);
    let b = h.state().browser().unwrap();
    assert_eq!(b.dir(), tmp.path().join("packs"));
    let shown: Vec<_> = b.shown().iter().map(|e| (e.name.clone(), e.is_pack)).collect();
    assert_eq!(shown, [("office".to_string(), true)]);
    h.act(vec![A::BrowseTo(tmp.path().join("nowhere"))]);
    assert!(h.state().browser().unwrap().message().is_some(), "a listing error is said");
}

#[test]
fn settings_persist() {
    let Some((tmp, root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    h.act(vec![A::PlayBots(3)]);
    assert!(tmp.path().join(EDITOR_FILE).is_file());
    let mut store = Store::open(tmp.path());
    let packs = store.packs_dir().unwrap();
    let h2 = EditorHost::new(&mut store, &[packs.as_path(), root.as_path()]).unwrap();
    assert_eq!(h2.state().settings().play_bots, 3);
}

#[test]
fn play_and_close_reach_the_app() {
    let Some((_tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    let out = h.act(vec![A::Play]);
    let play = out.iter().find_map(|o| match o {
        HostOut::Play(p) => Some(p),
        _ => None,
    });
    let play = play.expect("Play reaches the app");
    assert_eq!(play.level.size, (120, 80));
    assert_eq!(play.bots, h.state().settings().play_bots);
    h.act(vec![A::PlayEnded]);
    let out = h.act(vec![A::Close]);
    assert!(out.contains(&HostOut::Closed), "{out:?}");
}

/// Review Focus 1: closing the window while the editor holds unsaved
/// changes asks first (Save, Don't save, Cancel), as Close does.
#[test]
fn closing_the_window_asks_when_unsaved() {
    use olr_game::editor::{CloseDecision, on_close_request};
    assert_eq!(on_close_request(None), CloseDecision::Quit, "no editor: quit");
    let Some((_tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    assert_eq!(on_close_request(Some(&mut h)), CloseDecision::Quit, "nothing unsaved");
    let Some((_tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    h.act(vec![A::AddLayer]);
    assert_eq!(on_close_request(Some(&mut h)), CloseDecision::Wait);
    assert!(
        matches!(h.state().dialog(), Some(olr_editor::Dialog::Unsaved { then: olr_editor::Pending::Close })),
        "{:?}",
        h.state().dialog()
    );
    // Don't save: the editor closes, and the window with it.
    let out = h.act(vec![A::Choose(olr_editor::Choice::Discard)]);
    assert!(out.contains(&HostOut::Closed), "{out:?}");
    // While Play runs, the play ends first, then the editor asks.
    let Some((_tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    h.act(vec![A::AddLayer, A::Play]);
    assert!(h.state().playing());
    assert_eq!(on_close_request(Some(&mut h)), CloseDecision::Wait);
    assert!(!h.state().playing());
    assert!(h.state().dialog().is_some());
}

/// Batch B review, Important 1: a crash copy survives opening its pack
/// again (Edit instead of Recover, then Close or two minutes of editing):
/// each session writes a recovery of its own.
#[test]
fn a_later_session_keeps_the_crash_copy() {
    let Some((tmp, root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    h.act(vec![A::AddLayer]);
    let t0 = Instant::now();
    h.tick(t0);
    h.tick(t0 + RECOVERY_INTERVAL);
    drop(h);
    let rec = tmp.path().join("recovery");
    assert_eq!(list_recoveries(&rec).len(), 1);
    let mut store = Store::open(tmp.path());
    let packs = store.packs_dir().unwrap();
    let mut h = EditorHost::new(&mut store, &[packs.as_path(), root.as_path()]).unwrap();
    h.open_from_menu(&AppCommand::EditLevel("office".into()));
    h.act(vec![A::AddLayer]);
    h.tick(t0);
    h.tick(t0 + RECOVERY_INTERVAL);
    assert_eq!(list_recoveries(&rec).len(), 2, "this session's copy is another");
    h.act(vec![A::Close, A::Choose(olr_editor::Choice::Discard)]);
    let left: Vec<String> = list_recoveries(&rec).into_iter().map(|r| r.0).collect();
    assert_eq!(left, ["office"], "the crash copy is still offered");
    // A recovery's own folder is not opened as a pack (its save would be
    // removed with the copy).
    let mut h = EditorHost::new(&mut store, &[packs.as_path(), root.as_path()]).unwrap();
    let out = h.open_dir(&rec.join("office"));
    assert!(!messages(&out).is_empty(), "{out:?}");
    assert!(h.state().document().is_none());
}

/// Batch C review, Minor 1 regraded: a quit without the dialog (an
/// autoplay's or `--autoquit-after`'s, a frame's error) writes the recovery
/// copy of unsaved changes first.
#[test]
fn quitting_without_asking_keeps_a_recovery_copy() {
    let Some((tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    h.flush_recovery();
    assert!(list_recoveries(&tmp.path().join("recovery")).is_empty(), "nothing unsaved");
    h.act(vec![A::AddLayer]);
    h.flush_recovery();
    assert_eq!(list_recoveries(&tmp.path().join("recovery")).len(), 1);
}

/// Batch C review, Important 2: a Play that cannot start says why in the
/// editor (a dialog), not on the hidden Editor screen.
#[test]
fn a_failed_play_says_why_in_the_editor() {
    use olr_game::shell::{Shell, ShellConfig};
    let Some(root) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let cfg = ShellConfig {
        content: root,
        interpolation: olr_scene::Interpolation::Linear,
        viewport: olr_scene::Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = olr_ui::UiState::new(olr_ui::Settings::default(), olr_ui::Profiles::default(), 1);
    let mut sh = Shell::new(cfg, ui, Store::open(tmp.path()), olr_audio::Audio::silent());
    sh.act(olr_ui::UiAction::Open(olr_ui::Screen::Editor));
    sh.act(olr_ui::UiAction::NewLevel {
        name: "Office".into(),
        size: (120, 80),
        theme: "olx-theme-dirt".into(),
    });
    assert!(sh.editor_open());
    sh.editor_frame(vec![A::PlayMod("no-such-mod".into()), A::Play], Instant::now());
    sh.poll();
    assert!(!sh.editing_play());
    let h = sh.editor().unwrap();
    assert!(!h.state().playing());
    assert!(
        matches!(h.state().dialog(), Some(olr_editor::Dialog::Error(e)) if e.contains("no-such-mod")),
        "{:?}",
        h.state().dialog()
    );
}

/// Docs review, Important 1: `editor.ron` follows the store's rules: a
/// newer game's file is left untouched by this session's saves.
#[test]
fn a_newer_editor_file_is_left_untouched() {
    let Some(root) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let newer = "(version: 99, value: (play_bots: 5))";
    std::fs::write(tmp.path().join(EDITOR_FILE), newer).unwrap();
    let mut store = Store::open(tmp.path());
    let packs = store.packs_dir().unwrap();
    let mut h = EditorHost::new(&mut store, &[packs.as_path(), root.as_path()]).unwrap();
    assert!(!h.take_warnings().is_empty(), "said");
    h.open_from_menu(&new_level("Office"));
    h.act(vec![A::PlayBots(3)]);
    assert_eq!(std::fs::read_to_string(tmp.path().join(EDITOR_FILE)).unwrap(), newer);
}

/// Docs review, Important 2: `recovery/` is refused for Create and Save as
/// for Open (a save there would go with the copy).
#[test]
fn nothing_is_made_or_saved_in_recovery() {
    let Some((tmp, _root, mut h)) = host() else { return };
    h.open_from_menu(&new_level("Office"));
    let rec = tmp.path().join("recovery");
    let n = olr_editor::NewLevel {
        dir: rec.join("made"),
        id: "made".into(),
        name: "Made".into(),
        size: (40, 30),
        theme: "olx-theme-dirt".into(),
    };
    h.act(vec![A::New(n)]);
    assert!(!rec.join("made").exists(), "nothing made there");
    assert!(matches!(h.state().dialog(), Some(olr_editor::Dialog::Error(_))), "{:?}", h.state().dialog());
}

/// A listing larger than `MAX_ENTRIES` keeps its first entries in the
/// browser's order, not the disk's (deferred minor of batch B).
#[test]
fn a_huge_listing_keeps_its_first_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let n = olr_game::editor::MAX_ENTRIES + 5;
    for i in (0..n).rev() {
        std::fs::write(tmp.path().join(format!("f{i:05}.png")), b"").unwrap();
    }
    std::fs::create_dir(tmp.path().join("zz-folder")).unwrap();
    let mut state = olr_editor::EditorState::new(Default::default(), Default::default());
    state.update(A::Browse(Purpose::ImportImage));
    let cmds = state.update(A::BrowseTo(tmp.path().to_path_buf()));
    assert_eq!(cmds, vec![olr_editor::EditorCommand::List(tmp.path().to_path_buf())]);
    let entries = olr_game::editor::list_dir(tmp.path()).unwrap();
    assert_eq!(entries.len(), olr_game::editor::MAX_ENTRIES);
    assert_eq!(entries[0].name, "zz-folder", "folders first");
    assert_eq!(entries[1].name, "f00000.png");
    assert_eq!(entries.last().unwrap().name, format!("f{:05}.png", olr_game::editor::MAX_ENTRIES - 2));
}
