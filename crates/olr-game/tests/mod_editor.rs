//! The mod editor in the game (issue #17), through the shell without a
//! window: a new mod made in the data directory's `packs/` as a copy of
//! Classic (Classic untouched), edited and saved, listed as the user's and
//! opened again with Classic's values as the originals; a built-in mod
//! refused for Edit; unsaved work kept in a recovery copy and recovered;
//! the window's close asking first; Play (issue #18) running the unsaved
//! mod on the level and bots chosen, refused with an error, and Escape
//! back to the editor as it was. Over the bundled content
//! (`OLR_CONTENT_DIR`; skipped without it).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use olr_game::editor::{CloseDecision, RECOVERY_INTERVAL};
use olr_game::shell::{Shell, ShellConfig};
use olr_mod_editor::{Dialog, ModAction as A};
use olr_ui::{Profiles, Screen, Settings, Store, UiAction, UiState};

const CLASSIC: &str = "olx-classic";

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// A shell on data directory `data`, on the Editor screen.
fn shell_on(root: &Path, data: &Path) -> Shell {
    let cfg = ShellConfig {
        content: root.to_path_buf(),
        interpolation: olr_scene::Interpolation::Linear,
        viewport: olr_scene::Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let mut sh = Shell::new(cfg, ui, Store::open(data), olr_audio::Audio::silent());
    sh.act(UiAction::Open(Screen::Editor));
    sh
}

fn shell() -> Option<(tempfile::TempDir, PathBuf, Shell)> {
    let root = content()?;
    let tmp = tempfile::tempdir().unwrap();
    let sh = shell_on(&root, tmp.path());
    Some((tmp, root, sh))
}

/// Every file under `dir` by relative path, with its bytes.
fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn go(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

fn set(path: &str, value: &str) -> A {
    A::Set { path: path.into(), value: value.into() }
}

fn value(sh: &mut Shell, path: &str) -> Option<String> {
    sh.mod_editor().unwrap().state().value(path)
}

#[test]
fn a_new_mod_is_a_copy_saved_to_your_packs_and_edited_again() {
    let Some((tmp, root, mut sh)) = shell() else { return };
    let classic = root.join(CLASSIC);
    let before = files(&classic);
    sh.act(UiAction::NewMod { from: CLASSIC.into(), name: "Heavy Liero".into() });
    assert!(sh.mod_editor_open(), "{:?}", sh.data.editor_message);
    let dir = tmp.path().join("packs/heavy-liero");
    let h = sh.mod_editor().unwrap();
    assert_eq!(h.dir(), dir.as_path());
    assert_eq!(h.state().base_id(), Some(CLASSIC));
    assert_eq!(h.state().pack().manifest.name, "Heavy Liero");
    assert!(!h.state().dirty());
    // Made at once: listed as yours.
    assert!(sh.data.editable_mods.contains("heavy-liero"));
    assert!(sh.data.catalog.mods.iter().any(|m| m.id == "heavy-liero"));

    sh.mod_frame(
        vec![set("mod.worm.gravity", "120.0"), set("weapons/bazooka.fire.shot.speed", "150"), A::Save],
        Instant::now(),
    );
    let h = sh.mod_editor().unwrap();
    assert!(!h.state().dirty(), "{}", h.state().status());
    let saved = olr_edit::pack::load(&dir).unwrap();
    assert_eq!(
        olr_edit::mods::get(olr_edit::mods::source(&saved).unwrap(), "mod.worm.gravity").unwrap(),
        "120.0"
    );
    assert_eq!(files(&classic), before, "Classic is never changed");
    sh.mod_frame(vec![A::Close], Instant::now());
    assert!(!sh.mod_editor_open());
    assert_eq!(sh.ui.screen(), Screen::Editor);

    // Edit opens it again, Classic's values as the originals.
    sh.act(UiAction::EditMod("heavy-liero".into()));
    assert!(sh.mod_editor_open(), "{:?}", sh.data.editor_message);
    assert_eq!(value(&mut sh, "weapons/bazooka.fire.shot.speed").as_deref(), Some("150"));
    let state = sh.mod_editor().unwrap().state();
    assert_eq!(state.original("mod.worm.gravity").as_deref(), Some("100.0"));
}

#[test]
fn a_built_in_mod_is_only_copied() {
    let Some((_tmp, _root, mut sh)) = shell() else { return };
    sh.act(UiAction::EditMod(CLASSIC.into()));
    assert!(!sh.mod_editor_open());
    assert!(
        sh.data.editor_message.as_deref().is_some_and(|m| m.contains("edit a copy")),
        "{:?}",
        sh.data.editor_message
    );
}

#[test]
fn unsaved_work_is_kept_in_a_recovery_copy() {
    let Some((tmp, root, mut sh)) = shell() else { return };
    sh.act(UiAction::NewMod { from: CLASSIC.into(), name: "Moon".into() });
    let t0 = Instant::now();
    sh.mod_frame(vec![set("mod.worm.gravity", "20.0")], t0);
    sh.mod_frame(Vec::new(), t0 + RECOVERY_INTERVAL + Duration::from_secs(1));
    let copy = tmp.path().join("mod-recovery/moon");
    assert!(copy.join("pack.ron").is_file(), "the copy is written");
    // A crash: the next run offers it on the Mod tab.
    drop(sh);
    let mut sh = shell_on(&root, tmp.path());
    sh.refresh_editor_packs();
    assert_eq!(sh.data.mod_recoveries, vec![("moon".to_string(), "Moon".to_string(), false)]);
    sh.act(UiAction::RecoverMod("moon".into()));
    assert!(sh.mod_editor_open(), "{:?}", sh.data.editor_message);
    assert!(sh.mod_editor().unwrap().state().dirty(), "recovered work is unsaved");
    assert_eq!(value(&mut sh, "mod.worm.gravity").as_deref(), Some("20.0"));
    // Saved, the copy goes.
    sh.mod_frame(vec![A::Save], Instant::now());
    assert!(!copy.exists());
    assert!(sh.data.mod_recoveries.is_empty());
    // Discard removes one.
    sh.mod_frame(vec![set("mod.worm.gravity", "30.0")], t0);
    sh.mod_frame(Vec::new(), t0 + RECOVERY_INTERVAL + Duration::from_secs(1));
    assert!(copy.is_dir());
    sh.mod_frame(vec![A::Close, A::Choice(olr_mod_editor::Choice::DontSave)], Instant::now());
    assert!(!sh.mod_editor_open());
    assert!(!copy.exists(), "Close drops the copy");
}

#[test]
fn closing_the_window_asks_first() {
    let Some((_tmp, _root, mut sh)) = shell() else { return };
    sh.act(UiAction::NewMod { from: CLASSIC.into(), name: "Asked".into() });
    sh.mod_frame(vec![set("mod.worm.gravity", "120.0")], Instant::now());
    assert_eq!(sh.request_close(), CloseDecision::Wait);
    assert_eq!(sh.mod_editor().unwrap().state().dialog(), Some(&Dialog::Unsaved));
    sh.mod_frame(vec![A::Choice(olr_mod_editor::Choice::DontSave)], Instant::now());
    assert!(!sh.mod_editor_open());
    assert_eq!(sh.request_close(), CloseDecision::Quit);
}

fn bazooka_speed(sh: &Shell) -> i32 {
    let p = sh.play.as_ref().expect("a game plays");
    let w = p.assets.m.sim.weapons().iter().find(|w| w.id == "bazooka").unwrap();
    w.spawn.as_ref().unwrap().speed
}

/// Issue #18: Play runs a match of the mod as it is, unsaved changes
/// included, on the level and bots of Play ▾ (remembered in the data
/// directory); Escape (`stop_edited`) returns to the editor where it was,
/// nothing saved; a mod with errors does not start and its problems show.
#[test]
fn play_runs_the_unsaved_mod_and_escape_comes_back() {
    let Some((tmp, root, mut sh)) = shell() else { return };
    sh.act(UiAction::NewMod { from: CLASSIC.into(), name: "Fast".into() });
    assert!(sh.mod_editor_open(), "{:?}", sh.data.editor_message);
    let level = "olx-level-liero-factory";
    let local = sh.ui.settings().gameplay.match_prefs.level_id.clone();
    let s = sh.mod_editor().unwrap().state();
    assert!(s.levels().iter().any(|(id, _)| id == level), "the installed levels");
    assert_eq!(s.play_level().map(|(id, _)| id.clone()), Some(local), "Local Play's level at first");

    let speed = "weapons/bazooka.fire.shot.speed";
    let t = Instant::now();
    sh.mod_frame(vec![A::Select(olr_mod_editor::Item::Weapon("bazooka".into())), set(speed, "150")], t);
    sh.mod_frame(vec![A::PlayLevel(level.into()), A::PlayBots(2), A::Play], t);
    assert!(tmp.path().join(olr_game::editor::mods::MOD_PLAY_FILE).is_file(), "remembered");
    sh.poll();
    assert!(sh.editing_play(), "{:?}", sh.mod_editor().unwrap().state().dialog());
    assert_eq!(bazooka_speed(&sh), 150, "the unsaved speed plays");
    let p = sh.play.as_ref().unwrap();
    assert!(p.assets.edited(), "no replay is saved");
    assert_eq!(p.assets.level_id, level);
    assert_eq!(p.assets.worms(), 3, "the player and 2 bots");
    assert!(sh.mod_editor().unwrap().state().playing());
    // The game plays; the editor waits.
    for _ in 0..50 {
        sh.tick(olr_sim::Keys(0));
    }
    sh.mod_frame(Vec::new(), t);

    // Escape: back where it was, unsaved.
    sh.stop_edited();
    assert!(!sh.editing_play() && sh.play.is_none());
    let s = sh.mod_editor().unwrap().state();
    assert!(!s.playing());
    assert_eq!(s.item(), &olr_mod_editor::Item::Weapon("bazooka".into()));
    assert!(s.dirty() && s.can_undo());
    assert_eq!(s.value(speed).as_deref(), Some("150"));
    let saved = olr_edit::pack::load(&tmp.path().join("packs/fast")).unwrap();
    assert_eq!(olr_edit::mods::get(olr_edit::mods::source(&saved).unwrap(), speed).unwrap(), "140");

    // A mod with errors does not start; the problems show.
    let cycle = r#"{"s": (shared: "bazooka.spawn")}"#;
    sh.mod_frame(vec![set("projectiles/bazooka.spawn.spawns", cycle), A::ShowProblems(false), A::Play], t);
    sh.poll();
    assert!(!sh.editing_play() && sh.play.is_none());
    let s = sh.mod_editor().unwrap().state();
    assert!(!s.playing() && s.show_problems() && s.errors() > 0);
    assert!(s.status().starts_with("Not played"), "{}", s.status());

    // The next session remembers Play's level and bots.
    sh.mod_frame(vec![A::Undo, A::Save, A::Close], t);
    assert!(!sh.mod_editor_open());
    drop(sh);
    let mut sh = shell_on(&root, tmp.path());
    sh.act(UiAction::EditMod("fast".into()));
    let s = sh.mod_editor().unwrap().state();
    assert_eq!((s.play_level().map(|(id, _)| id.as_str()), s.play_bots()), (Some(level), 2));
}

/// Issue #18: closing the window during Play ends the match, then asks
/// about the unsaved changes.
#[test]
fn closing_the_window_during_play_asks_first() {
    let Some((_tmp, _root, mut sh)) = shell() else { return };
    sh.act(UiAction::NewMod { from: CLASSIC.into(), name: "Asked".into() });
    sh.mod_frame(vec![set("mod.worm.gravity", "120.0"), A::PlayBots(0), A::Play], Instant::now());
    sh.poll();
    assert!(sh.editing_play());
    assert_eq!(sh.request_close(), CloseDecision::Wait);
    assert!(!sh.editing_play());
    let s = sh.mod_editor().unwrap().state();
    assert!(!s.playing());
    assert_eq!(s.dialog(), Some(&Dialog::Unsaved));
}
