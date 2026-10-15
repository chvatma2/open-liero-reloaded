//! Choosing the worm to watch: in any game nobody here plays (a replay, a
//! `--spectate` match) the left and right arrows pick the worm the camera
//! and the HUD follow, as an online spectator's do; PageUp and PageDown
//! only change the speed, and a played game's camera stays on its worm.
//! Played headless through `Session`; Classic on Dirt Level from
//! `RL_CONTENT_DIR`, skipped with a notice without it.

use std::path::Path;

use rl_audio::Audio;
use rl_game::app::{WatchKey, game_has_screen, watch_key};
use rl_game::hud::{self, Frame};
use rl_game::session::Session;
use rl_game::setup::{self, Assets, DeathmatchOptions};
use rl_input::KeyCode;
use rl_scene::Interpolation;
use rl_sim::Keys;

use crate::online::{content_dir, viewport};

fn load(dir: &Path, o: &DeathmatchOptions) -> Assets {
    setup::load_deathmatch(&[dir], "Classic", "Dirt Level", "default", o).unwrap()
}

fn session(a: &Assets) -> Session {
    Session::new(a, viewport(), Interpolation::Linear, Audio::silent()).unwrap()
}

/// Bots alone, watched.
fn bots(dir: &Path) -> Assets {
    load(dir, &DeathmatchOptions { bots: 3, spectate: true, seed: 3, ..DeathmatchOptions::default() })
}

/// A replay of `ticks` ticks of a match the human plays (worm 0 against
/// two bots), its viewer set to `viewer`: the menus and the command line
/// seat the human first, an online peer may sit anywhere.
fn replay_viewed_by(dir: &Path, ticks: u64, viewer: Option<u8>) -> Assets {
    let a = load(dir, &DeathmatchOptions { bots: 2, seed: 4, ..DeathmatchOptions::default() });
    let mut s = session(&a);
    assert_eq!(s.local(), Some(0));
    while s.ticks() < ticks {
        s.tick(Keys(0));
    }
    let mut replay = s.take_replay().expect("a Deathmatch records");
    assert_eq!(replay.setup.viewer, Some(0), "recorded from the human's slot");
    replay.setup.viewer = viewer;
    let file = std::env::temp_dir().join(format!("rl-game-watch-{}-{viewer:?}.rlrp", std::process::id()));
    std::fs::write(&file, replay.encode()).unwrap();
    let r = setup::load_replay(&[dir], &file, "default", &[]);
    std::fs::remove_file(&file).unwrap();
    r.unwrap()
}

/// A replay of three bots (Classic, Dirt Level, seed 5) in which worm 1 is
/// withdrawn before tick `k` (as an online match's record makes one, with
/// `Recorder::withdraw`), loaded as the Replays screen loads one.
fn replay_withdrawing(dir: &Path, k: u32, ticks: u32) -> Assets {
    let content = rl_content::Content::open(dir).unwrap();
    let o = DeathmatchOptions { bots: 3, spectate: true, seed: 5, ..DeathmatchOptions::default() };
    let built =
        content.build_match(&setup::match_options(&o, "olx-classic", "olx-level-dirt-level")).unwrap();
    let (mut world, mut rec) = (built.world, built.recorder);
    let (keys, mut out) = (vec![Keys(0); world.worms().len()], rl_sim::TickEvents::new());
    for t in 0..ticks {
        if t == k {
            rec.withdraw(&mut world, 1).unwrap();
        }
        world.tick(&keys, &mut out);
        rec.push(&keys, &world).unwrap();
    }
    let replay = rec.finish(&world).unwrap();
    let file = std::env::temp_dir().join(format!("rl-game-watch-gone-{}.rlrp", std::process::id()));
    std::fs::write(&file, replay.encode()).unwrap();
    let r = setup::load_replay(&[dir], &file, "default", &[]);
    std::fs::remove_file(&file).unwrap();
    r.unwrap()
}

/// N89: the watched worm leaving moves the camera on; following skips it.
#[test]
fn the_watched_worm_moves_on_when_it_leaves() {
    let Some(dir) = content_dir() else { return };
    let mut s = session(&replay_withdrawing(&dir, 100, 200));
    s.follow_next(1);
    assert_eq!(s.followed(), 1);
    while s.ticks() < 101 {
        s.tick(Keys(0));
    }
    assert_eq!(s.followed(), 2, "moved on");
    s.follow_next(-1);
    assert_eq!(s.followed(), 0, "worm 1 is skipped");
}

/// The worm the camera, the HUD and the minimap's big dot are on.
fn on(s: &Session) -> (i32, i32, Option<String>, Vec<usize>) {
    let frame = Frame { minimap: Some(egui::TextureId::default()), ..Frame::default() };
    let view = hud::view(s, &frame);
    let big = view.minimap.unwrap().dots.iter().enumerate().filter(|(_, d)| d.big).map(|(i, _)| i).collect();
    (s.followed(), s.present.scene.camera.target(), view.worm.map(|w| w.name), big)
}

/// A replay is watched: it starts on the worm of the player who made it
/// (worm 0 when there was none), the arrows' steps go round in worm order,
/// and the HUD, the camera and the minimap's big dot follow.
#[test]
fn a_replay_follows_the_worm_chosen() {
    let Some(dir) = content_dir() else { return };
    let mut s = session(&replay_viewed_by(&dir, 200, Some(2)));
    assert_eq!(s.local(), None);
    assert!(s.spectating(), "nobody plays a replay");
    let first = on(&s);
    assert_eq!((first.0, first.1), (2, 2), "a replay starts on its human, here the third worm");
    assert_eq!(first.3, [2]);
    s.follow_next(1);
    let second = on(&s);
    assert_eq!((second.0, second.1), (0, 0), "round");
    assert_ne!(second.2, first.2, "the HUD shows the other worm");
    assert_eq!(second.3, [0], "the big minimap dot follows");
    s.follow_next(1);
    assert_eq!(s.followed(), 1);

    let none = session(&replay_viewed_by(&dir, 50, None));
    assert_eq!(none.followed(), 0, "a replay of nobody's game starts on worm 0");
    // A restart keeps the worm chosen.
    let a = replay_viewed_by(&dir, 50, Some(1));
    let mut s = session(&a);
    assert_eq!(s.followed(), 1);
    s.follow_next(1);
    s.rebuild(&a).unwrap();
    assert_eq!(s.followed(), 2);
}

/// The HUD's health, lives, kills and weapons are the followed worm's,
/// and a notice names it.
#[test]
fn the_hud_shows_the_followed_worm() {
    let Some(dir) = content_dir() else { return };
    let mut s = session(&bots(&dir));
    for _ in 0..300 {
        s.tick(Keys(0));
    }
    s.follow_next(1);
    let w = &s.world.worms()[1];
    let view = hud::view(&s, &Frame::default());
    let hud_worm = view.worm.expect("the followed worm");
    assert_eq!(hud_worm.name, "Bot 2");
    assert_eq!((hud_worm.health, hud_worm.kills, hud_worm.lives), (w.health, w.kills, w.lives));
    assert_eq!(hud_worm.slots.len(), w.slots.len());
    assert!(view.notices.iter().any(|n| n.text == "Watching Bot 2"), "{:?}", view.notices);
}

/// A `--spectate` match is watched too; a played game is not: its camera
/// stays on its worm and the follow code is never reached.
#[test]
fn only_a_game_nobody_plays_is_followed() {
    let Some(dir) = content_dir() else { return };
    let mut s = session(&bots(&dir));
    assert!(s.spectating());
    s.follow_next(1);
    assert_eq!(s.followed(), 1);

    let o = DeathmatchOptions { bots: 2, ..DeathmatchOptions::default() };
    let mut played = session(&load(&dir, &o));
    assert!(!played.spectating());
    played.follow_next(1);
    assert_eq!(played.followed(), 0, "the player's camera stays on its worm");
    let view = hud::view(&played, &Frame::default());
    assert!(view.notices.iter().all(|n| !n.text.starts_with("Watching")));
}

/// The arrows follow in a game nobody plays (a replay, bots alone, an
/// online match watched: `spectating`) and are the player's own keys
/// otherwise; PageUp and PageDown are the speed's, never the follow's, and
/// do nothing where nothing fast-forwards (an online match, played or
/// watched). Nothing while a menu holds the game.
#[test]
fn the_keys_choose_the_worm_or_the_speed() {
    use KeyCode::{ArrowLeft, ArrowRight, PageDown, PageUp};
    // (spectating, human, in game)
    let (replay, played, online_watched) = ((true, false, true), (false, true, true), (true, true, true));
    let key = |code, (spectating, human, in_game)| watch_key(code, spectating, human, in_game);
    assert_eq!(key(ArrowRight, replay), Some(WatchKey::Follow(1)));
    assert_eq!(key(ArrowLeft, replay), Some(WatchKey::Follow(-1)));
    assert_eq!(key(ArrowRight, online_watched), Some(WatchKey::Follow(1)));
    assert_eq!(key(ArrowLeft, played), None, "the arrows move the player's worm");
    assert_eq!(key(ArrowRight, (true, false, false)), None, "a menu's arrows are the menu's");
    assert_eq!(key(PageUp, replay), Some(WatchKey::Speed { faster: true }));
    assert_eq!(key(PageDown, replay), Some(WatchKey::Speed { faster: false }));
    assert_eq!(key(PageUp, online_watched), None, "no fast-forward online");
    assert_eq!(key(PageDown, online_watched), None);
    assert_eq!(key(PageUp, played), None);
    assert_eq!(key(KeyCode::KeyH, replay), None);
}

/// A command-line game (`--spectate`, `--replay`: no menus) always has the
/// screen, though the menus' state stays on the main menu, so the arrows
/// follow there too; with the menus, only while their panels do not hold
/// the game (final review B-I4).
#[test]
fn the_arrows_follow_in_command_line_games() {
    // (menus, names shown by the menus' state)
    assert!(game_has_screen(false, false), "--spectate / --replay");
    assert!(game_has_screen(true, true), "a menu game being played");
    assert!(!game_has_screen(true, false), "a menu panel holds the game");
    let spectate = watch_key(KeyCode::ArrowRight, true, false, game_has_screen(false, false));
    assert_eq!(spectate, Some(WatchKey::Follow(1)));
}
