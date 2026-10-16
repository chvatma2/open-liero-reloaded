//! The menus driving the game (`shell.rs`) without a window: starting
//! matches from the menus with missing or broken content (plan 7 Review
//! Focus 3) and rapid menu actions (Review Focus 4). The parts that play
//! OLX's levels need `OLR_CONTENT_DIR`.

use std::path::{Path, PathBuf};

use olr_audio::Audio;
use olr_game::shell::{Shell, ShellConfig};
use olr_scene::{Interpolation, Viewport};
use olr_ui::{Profiles, Screen, Settings, Store, UiAction, UiState};

fn shell(content: &Path, store: Store) -> Shell {
    let cfg = ShellConfig {
        content: content.to_path_buf(),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    Shell::new(cfg, ui, store, Audio::silent())
}

fn content_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("OLR_CONTENT_DIR not set; skipping the imported content");
    }
    dir
}

/// A content directory of links to every pack of `from`.
#[cfg(unix)]
fn linked_content(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), to.join(e.file_name())).unwrap();
    }
}

/// `ticks` ticks of the game playing, no key held.
fn play(sh: &mut Shell, ticks: usize) {
    let s = sh.session_mut().unwrap();
    for _ in 0..ticks {
        s.tick(olr_sim::Keys(0));
    }
}

fn message(sh: &Shell) -> String {
    sh.ui.message().unwrap_or_default().to_string()
}

#[test]
fn starting_with_missing_content_returns_to_menu() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sh = shell(tmp.path(), Store::in_memory());
    // No mod: back to local play with the reason, ready to start again.
    sh.act(UiAction::Open(Screen::LocalPlay));
    sh.act(UiAction::StartMatch);
    assert!(sh.ui.is_starting());
    sh.poll();
    assert_eq!(sh.ui.screen(), Screen::LocalPlay);
    let m = message(&sh);
    assert!(m.contains("olx-classic") || m.contains("olx-level-liero-factory"), "{m}");
    assert!(sh.play.is_none() && !sh.ui.is_starting());
    // The sandbox, from the main menu.
    sh.act(UiAction::Back);
    sh.act(UiAction::StartSandbox);
    sh.poll();
    assert_eq!(sh.ui.screen(), Screen::Main);
    assert!(!message(&sh).is_empty());
    // A replay file that is gone, or is not a replay.
    sh.act(UiAction::Open(Screen::Replays));
    let gone = tmp.path().join("gone.rlrp");
    sh.act(UiAction::PlayReplay(gone));
    sh.poll();
    assert_eq!(sh.ui.screen(), Screen::Replays);
    assert!(message(&sh).contains("gone.rlrp"), "{}", message(&sh));
    let junk = tmp.path().join("junk.rlrp");
    std::fs::write(&junk, b"junk").unwrap();
    sh.act(UiAction::PlayReplay(junk));
    sh.poll();
    assert_eq!(sh.ui.screen(), Screen::Replays);
    assert!(message(&sh).contains("junk.rlrp"), "{}", message(&sh));
    assert!(sh.play.is_none());

    #[cfg(unix)]
    {
        // The menu listed the level; its pack is deleted before the start,
        // then broken; restored, the match starts.
        let Some(content) = content_dir() else { return };
        let dir = tempfile::tempdir().unwrap();
        linked_content(&content, dir.path());
        let mut sh = shell(dir.path(), Store::in_memory());
        let level = "olx-level-liero-factory";
        assert!(sh.data.catalog.level(level).is_some());
        std::fs::remove_file(dir.path().join(level)).unwrap();
        sh.act(UiAction::Open(Screen::LocalPlay));
        sh.act(UiAction::StartMatch);
        sh.poll();
        assert_eq!(sh.ui.screen(), Screen::LocalPlay);
        assert!(message(&sh).contains(level), "{}", message(&sh));
        let broken = dir.path().join(level);
        std::fs::create_dir(&broken).unwrap();
        std::fs::copy(content.join(level).join("pack.ron"), broken.join("pack.ron")).unwrap();
        std::fs::write(broken.join("level.ron"), "(name: 5").unwrap();
        sh.act(UiAction::StartMatch);
        sh.poll();
        assert_eq!(sh.ui.screen(), Screen::LocalPlay);
        assert!(message(&sh).contains(level), "{}", message(&sh));
        std::fs::remove_dir_all(&broken).unwrap();
        std::os::unix::fs::symlink(content.join(level), &broken).unwrap();
        sh.act(UiAction::StartMatch);
        sh.poll();
        assert_eq!(sh.ui.screen(), Screen::WeaponSelect);
        assert!(sh.play.is_some());
    }
}

#[test]
fn menu_actions_are_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    // Start pressed twice, Esc and a menu button while it loads: one load.
    let mut sh = shell(tmp.path(), Store::in_memory());
    sh.act(UiAction::Open(Screen::LocalPlay));
    for a in [UiAction::StartMatch, UiAction::StartMatch, UiAction::Back, UiAction::Open(Screen::Options)] {
        sh.act(a);
    }
    sh.poll();
    sh.poll();
    assert_eq!(sh.launches(), 1);
    assert_eq!(sh.ui.screen(), Screen::LocalPlay, "it failed: no content");
    // The same for a replay.
    sh.act(UiAction::Back);
    sh.act(UiAction::Open(Screen::Replays));
    sh.act(UiAction::PlayReplay(tmp.path().join("a.rlrp")));
    sh.act(UiAction::PlayReplay(tmp.path().join("b.rlrp")));
    sh.poll();
    assert_eq!(sh.launches(), 2);
    assert!(message(&sh).contains("a.rlrp"), "{}", message(&sh));

    let Some(content) = content_dir() else { return };
    let data = tempfile::tempdir().unwrap();
    let mut sh = shell(&content, Store::open(data.path()));
    sh.act(UiAction::Open(Screen::LocalPlay));
    for a in [UiAction::StartMatch, UiAction::StartMatch, UiAction::Back] {
        sh.act(a);
    }
    sh.poll();
    assert_eq!((sh.launches(), sh.ui.screen(), sh.ui.is_paused()), (1, Screen::WeaponSelect, false));
    let seed = |sh: &Shell| sh.play.as_ref().unwrap().session.world.clone();
    let first = seed(&sh);
    // Done pressed twice arms the player once.
    let weapons = sh
        .data
        .weapon_choice()
        .unwrap_or(olr_ui::weapon_select::WeaponSelect::new(sh.data.weapons.as_ref().unwrap(), &[]).slots());
    for _ in 0..2 {
        sh.act(UiAction::ChooseWeapons { weapons, names: vec![] });
    }
    assert_eq!(sh.ui.screen(), Screen::InGame);
    // Leave only from the pause menu; Esc twice is pause and resume.
    sh.act(UiAction::LeaveMatch);
    assert!(sh.play.is_some());
    sh.act(UiAction::Back);
    sh.act(UiAction::Back);
    assert!(!sh.ui.is_paused() && !sh.paused());
    sh.act(UiAction::Back);
    assert!(sh.paused());
    // Restart (F5) only plays on, it never starts a second match.
    sh.act(UiAction::Restart);
    assert!(sh.paused(), "not while paused");
    sh.act(UiAction::Back);
    for _ in 0..3 {
        play(&mut sh, 10);
        sh.act(UiAction::Restart);
    }
    sh.act(UiAction::Restart);
    assert_eq!(sh.launches(), 1);
    play(&mut sh, 10);
    sh.act(UiAction::Back);
    sh.act(UiAction::LeaveMatch);
    sh.act(UiAction::LeaveMatch);
    assert!(sh.play.is_none());
    assert_eq!(sh.ui.screen(), Screen::Main);
    // Each played match's replay was saved once (three restarts, the
    // leave; the restart at tick 0 played nothing).
    let saved = olr_ui::replays::list(&data.path().join(olr_ui::REPLAYS_DIR));
    assert_eq!(saved.len(), 4, "{saved:?}");
    // The next match has the next seed: another world.
    sh.act(UiAction::Open(Screen::LocalPlay));
    sh.act(UiAction::StartMatch);
    sh.poll();
    assert_eq!(sh.launches(), 2);
    assert_ne!(seed(&sh), first);
}

/// A settings or profiles file from a newer version of the game survives
/// a session that changes the settings and saves them (`Shell::flush`),
/// and the menus say so.
#[test]
fn a_newer_settings_file_survives_a_flush() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let file = data.join("settings.ron");
    let future = format!("(version: {}, settings: ())", olr_ui::SETTINGS_VERSION + 1);
    std::fs::write(&file, &future).unwrap();
    let mut store = Store::open(&data);
    let settings = store.load_settings().value;
    let cfg = ShellConfig {
        content: tmp.path().join("content"),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(settings.clone(), Profiles::default(), 1);
    let mut sh = Shell::new(cfg, ui, store, Audio::silent());
    assert_eq!(sh.data.notices.len(), 1, "{:?}", sh.data.notices);
    assert!(sh.data.notices[0].contains("settings.ron"));
    let mut audio = settings.audio;
    audio.master = 0.25;
    sh.act(UiAction::SetAudio(audio));
    sh.flush();
    sh.finish();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), future);
}
