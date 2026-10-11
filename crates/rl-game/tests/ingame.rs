//! A menu match in game (plan 7 task 5), through the `Shell` without a
//! window: the weapon selection arms the player, the pause stops the ticks
//! and the replay holds only the ticks played, and the results show the
//! world's scores. Needs the imported content (`RL_CONTENT_DIR`).

use std::path::{Path, PathBuf};

use rl_audio::Audio;
use rl_game::session::Driver;
use rl_game::shell::{Shell, ShellConfig};
use rl_scene::{Interpolation, Viewport};
use rl_sim::{Keys, MAX_SLOTS};
use rl_ui::{AppCommand, Profiles, Screen, Settings, Store, UiAction, UiState};

fn content() -> Option<PathBuf> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping");
    }
    dir
}

fn shell(content: &Path, store: Store, settings: Settings) -> Shell {
    let cfg = ShellConfig {
        content: content.to_path_buf(),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    Shell::new(cfg, UiState::new(settings, Profiles::default(), 5), store, Audio::silent())
}

/// The player's keys at tick `t`: walk about, jump and shoot now and then.
fn player_keys(t: u64) -> Keys {
    let walk = if (t / 200).is_multiple_of(2) { Keys::RIGHT } else { Keys::LEFT };
    let jump = if t.is_multiple_of(97) { Keys::JUMP } else { 0 };
    let fire = if (t / 50).is_multiple_of(3) { Keys::FIRE } else { 0 };
    Keys(walk | jump | fire)
}

/// Starts local play's match and returns at the weapon selection.
fn start(sh: &mut Shell) {
    sh.act(UiAction::Open(Screen::LocalPlay));
    sh.act(UiAction::StartMatch);
    sh.poll();
    assert_eq!(sh.ui.screen(), Screen::WeaponSelect, "{:?}", sh.ui.message());
}

/// Chooses enabled weapons other than the random draw's, and Done.
fn choose(sh: &mut Shell) -> [i32; MAX_SLOTS] {
    let menu = sh.data.weapons.clone().expect("the match's weapons");
    let enabled: Vec<i32> = (0..menu.names.len() as i32).filter(|&w| menu.enabled[w as usize]).collect();
    let weapons = [enabled[4], enabled[1], enabled[1], enabled[0], enabled[enabled.len() - 1]];
    let names = weapons.iter().map(|&w| menu.names[w as usize].clone()).collect();
    sh.act(UiAction::ChooseWeapons { weapons, names });
    assert_eq!(sh.ui.screen(), Screen::InGame);
    weapons
}

#[test]
fn pause_stops_ticks_and_replays_stay_valid() {
    let Some(dir) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    settings.gameplay.match_prefs.bots = 2;
    let mut sh = shell(&dir, Store::open(tmp.path()), settings);
    start(&mut sh);

    // The weapon selection holds the world before its first tick.
    let waiting = sh.session().unwrap().world.clone();
    for _ in 0..50 {
        assert!(!sh.tick(Keys(Keys::FIRE | Keys::RIGHT)), "no tick during the selection");
    }
    assert_eq!(sh.session().unwrap().world, waiting);
    let weapons = choose(&mut sh);
    let s = sh.session().unwrap();
    let held: Vec<i32> = s.world.worms()[0].slots.iter().map(|sl| sl.weapon).collect();
    assert_eq!(held, weapons, "the player holds the chosen weapons");
    assert_eq!(s.ticks(), 0);
    assert_eq!(sh.ui.profiles().selected().unwrap().weapons.len(), MAX_SLOTS, "remembered");

    for t in 0..300 {
        assert!(sh.tick(player_keys(t)));
    }
    // Paused (and in the options from the pause menu): no tick, the world
    // and its recording stay as they are.
    sh.act(UiAction::Back);
    assert!(sh.paused());
    let (sum, ticks) = (sh.session().unwrap().world.checksum(), sh.session().unwrap().ticks());
    for t in 0..400 {
        assert!(!sh.tick(player_keys(t)));
    }
    sh.act(UiAction::Open(Screen::Options));
    assert_eq!(sh.ui.screen(), Screen::Options);
    for t in 0..100 {
        assert!(!sh.tick(player_keys(t)));
    }
    sh.act(UiAction::Back);
    assert_eq!((sh.ui.screen(), sh.paused()), (Screen::InGame, true));
    assert_eq!((sh.session().unwrap().world.checksum(), sh.session().unwrap().ticks()), (sum, ticks));
    sh.act(UiAction::Resume);
    for t in 300..500 {
        assert!(sh.tick(player_keys(t)));
    }
    let end = sh.session().unwrap().world.checksum();
    sh.act(UiAction::Back);
    sh.act(UiAction::LeaveMatch);
    assert!(sh.play.is_none());

    // The saved replay: the 500 ticks played, the chosen weapons, every
    // checksum reproduced.
    let files = rl_ui::replays::list(&tmp.path().join(rl_ui::REPLAYS_DIR));
    assert_eq!(files.len(), 1, "{files:?}");
    let replay = rl_replay::Replay::decode(&std::fs::read(&files[0].path).unwrap()).unwrap();
    assert_eq!(replay.ticks(), 500);
    let recorded: Vec<i32> = replay.setup.worms[0].loadout.slots.iter().map(|sl| sl.weapon).collect();
    assert_eq!(recorded, weapons, "the replay holds the choice");
    let content = rl_content::Content::open(&dir).unwrap();
    let run = content.play_replay(&replay).expect("the replay plays back");
    assert_eq!(run.world.checksum(), end, "to the same world");
}

#[test]
fn results_match_world_scores() {
    let Some(dir) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    settings.gameplay.match_prefs.bots = 3;
    settings.gameplay.match_prefs.rules.time_limit = 0.25;
    let mut sh = shell(&dir, Store::open(tmp.path()), settings);
    start(&mut sh);
    choose(&mut sh);
    let mut t = 0;
    while sh.ui.screen() != Screen::Results && t < 3000 {
        sh.tick(player_keys(t));
        sh.after_ticks();
        t += 1;
    }
    assert_eq!(sh.ui.screen(), Screen::Results, "the time limit ends the match");
    let r = sh.data.results.clone().expect("the results the screen shows");
    let s = sh.session().unwrap();
    let scores = s.world.scores();
    assert_eq!(r.rows.len(), 4, "every worm");
    for (row, score) in r.rows.iter().zip(scores) {
        assert_eq!(row.worm, score.worm, "the mode's order");
        assert_eq!(
            (row.score, row.kills, row.deaths, row.suicides),
            (score.score, score.kills, score.deaths, score.suicides)
        );
        assert_eq!(row.damage, rl_modes::olx_round(score.damage));
        let w = &s.world.worms()[row.worm as usize];
        assert_eq!((row.kills, row.deaths, row.suicides), (w.kills, w.deaths, w.suicides));
        assert_eq!(row.name, s.names.of(row.worm));
    }
    let result = s.world.result().unwrap();
    assert_eq!(r.winner, (result.winner >= 0).then(|| s.names.of(result.winner)));
    assert!(matches!(s.driver, Driver::Match(_)));
    let ended = s.ticks() - s.since_end().unwrap();

    // Save replay keeps it under its own name; Back to menu leaves.
    sh.act(UiAction::SaveReplay);
    let m = sh.ui.message().unwrap_or_default().to_string();
    assert!(m.contains("replay-001.rlrp"), "{m}");
    let kept = tmp.path().join(rl_ui::REPLAYS_DIR).join("replay-001.rlrp");
    let replay = rl_replay::Replay::decode(&std::fs::read(&kept).unwrap()).unwrap();
    assert_eq!(replay.ticks() as u64, ended + rl_game::drive::RECORD_AFTER_END, "the match and its end");
    // Rematch: the weapons are chosen again.
    let c = sh.ui.clone().update(UiAction::StartMatch);
    assert!(matches!(c[..], [AppCommand::EndMatch, AppCommand::StartMatch(_)]));
    sh.act(UiAction::StartMatch);
    sh.poll();
    assert_eq!(sh.ui.screen(), Screen::WeaponSelect);
    assert!(sh.data.results.is_none());
    sh.act(UiAction::LeaveMatch);
    assert_eq!(sh.ui.screen(), Screen::Main);
}
