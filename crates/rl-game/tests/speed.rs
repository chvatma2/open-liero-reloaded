//! Fast-forward (`speed.rs`) never changes what is simulated: a match of
//! bots alone played at 1×, 4×, 16× and max gives the same world and the
//! same replay, byte for byte, and that replay plays back at max with every
//! checksum reproduced. Only the sounds are thinned out. A game a person
//! plays refuses `--speed`. Needs the imported content (`RL_CONTENT_DIR`);
//! skips without it.

use std::path::{Path, PathBuf};

use rl_audio::Audio;
use rl_game::session::{Driver, Session};
use rl_game::setup::{self, DeathmatchOptions};
use rl_game::speed::{self, Speed};
use rl_scene::{DrawList, Interpolation, Viewport};
use rl_sim::Keys;

fn content() -> Option<PathBuf> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping");
    }
    dir
}

fn viewport() -> Viewport {
    Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0)
}

/// Six bots alone on Liero Factory, watched.
fn watched() -> DeathmatchOptions {
    DeathmatchOptions { seed: 11, bots: 6, spectate: true, time_limit: 0.25, ..DeathmatchOptions::default() }
}

/// What a run at `speed` gives: the world's checksum after `ticks`
/// ticks, the replay's bytes, and how many ticks played their sounds.
struct Run {
    checksum: u64,
    replay: Vec<u8>,
    sounded: u32,
    frames: u32,
}

/// Plays `ticks` ticks of `a`'s game in frames of `due` real-time ticks
/// (alternating 1 and 2, as 60 Hz frames owe), through `speed::run_frame`
/// as the app does, drawing each frame.
fn play(a: &setup::Assets, speed: Speed, ticks: u64) -> Run {
    let mut s = Session::new(a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    assert!(!s.human(), "nobody plays");
    let speed = speed.for_game(s.human());
    let mut list = DrawList::new();
    let (mut sounded, mut frames) = (0, 0);
    while s.ticks() < ticks {
        let due = 1 + frames % 2;
        speed::run_frame::<()>(
            speed,
            due,
            |n| n >= 37,
            |sound| {
                s.sound = sound;
                let ticked = s.ticks() < ticks && s.tick(Keys(Keys::FIRE));
                sounded += u32::from(sound && ticked);
                Ok(ticked)
            },
        )
        .unwrap();
        s.build(0.5, &mut list);
        frames += 1;
    }
    let checksum = s.world.checksum();
    let replay = s.take_replay().expect("a Deathmatch records").encode();
    Run { checksum, replay, sounded, frames }
}

#[test]
fn fast_forward_never_changes_the_simulation() {
    let Some(dir) = content() else { return };
    let a =
        setup::load_deathmatch(&[dir.as_path()], "Classic", "Liero Factory", "default", &watched()).unwrap();
    assert!(!a.human_plays());
    // Past the time limit's end, so the game over and the results are in.
    let ticks = 1_700;
    let base = play(&a, Speed::X1, ticks);
    assert_eq!(base.sounded as u64, ticks, "1× plays every tick's sounds");
    for speed in [Speed::X4, Speed::X16, Speed::Max] {
        let r = play(&a, speed, ticks);
        assert_eq!(r.checksum, base.checksum, "{speed:?}: the same world");
        assert!(r.replay == base.replay, "{speed:?}: the same replay");
        assert!(r.frames < base.frames, "{speed:?}: fewer frames ({} vs {})", r.frames, base.frames);
        match speed {
            Speed::Max => assert_eq!(r.sounded, 0, "max is silent"),
            _ => assert!(r.sounded <= r.frames, "{speed:?}: at most a tick's sounds a frame"),
        }
    }

    // The replay plays back at max with every checksum reproduced.
    let file = std::env::temp_dir().join(format!("rl-game-speed-{}.rlrp", std::process::id()));
    std::fs::write(&file, &base.replay).unwrap();
    let r = setup::load_replay(&[dir.as_path()], &file, "default", &[]);
    std::fs::remove_file(&file).unwrap();
    let r = r.unwrap();
    let mut s = Session::new(&r, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    assert!(!s.human());
    let mut frames = 0;
    while speed::run_frame::<()>(
        Speed::Max,
        1,
        |n| n >= 64,
        |sound| {
            s.sound = sound;
            Ok(s.tick(Keys(0)))
        },
    )
    .unwrap()
        > 1
    {
        frames += 1;
    }
    let Driver::Replay(d) = &s.driver else { panic!("a replay") };
    assert!(d.is_done() && d.mismatch.is_none(), "every checksum reproduced: {:?}", d.mismatch);
    assert!(frames < 50, "{frames} frames for {ticks} ticks at max");
    assert_eq!(s.world.checksum(), base.checksum);
}

/// A game a person plays runs at 1×: its session says so (the app's
/// `Speed::for_game`), and `--speed` on its command line is refused
/// before any window opens.
#[test]
fn a_played_game_never_runs_fast() {
    let Some(dir) = content() else { return };
    let o = DeathmatchOptions { seed: 2, bots: 2, ..DeathmatchOptions::default() };
    let a = setup::load_deathmatch(&[dir.as_path()], "Classic", "Dirt Level", "default", &o).unwrap();
    let s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    assert!(a.human_plays() && s.human());
    assert_eq!(Speed::Max.for_game(s.human()), Speed::X1);
    let sandbox = setup::SandboxOptions {
        mod_id: String::new(),
        level_id: String::new(),
        seed: 1,
        targets: 1,
        bonuses: false,
    };
    let a = setup::load(&[dir.as_path()], "Classic", "Dirt Level", "default", sandbox).unwrap();
    assert!(a.human_plays(), "the sandbox is played");

    for args in [&["--mode", "deathmatch", "--speed", "4"][..], &["--level", "Dirt Level", "--speed", "max"]]
    {
        let out = game(&dir, args);
        assert!(!out.status.success(), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("--speed is for watching"), "{args:?}: {err}");
    }
}

/// Who plays is the game's as it is now: a restart after the options
/// changed (the menus' bots alone becoming a match with a player) says so.
#[test]
fn a_restart_knows_who_plays() {
    let Some(dir) = content() else { return };
    let mut a =
        setup::load_deathmatch(&[dir.as_path()], "Classic", "Dirt Level", "default", &watched()).unwrap();
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    assert!(!s.human());
    let setup::Game::Deathmatch { options, .. } = &mut a.game else { panic!("a Deathmatch") };
    options.players[0] = rl_content::Player::Human;
    s.restart(&a).unwrap();
    assert!(s.human(), "worm 0 is played now");
    assert_eq!(Speed::Max.for_game(s.human()), Speed::X1);
}

/// `--fake-clock` runs a fixed number of ticks a frame: only for an
/// autoplay, never for a person (it would speed a played game up).
#[test]
fn the_fake_clock_needs_an_autoplay() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rl-game"))
        .args(["--fake-clock", "--mode", "deathmatch"])
        .env("WGPU_BACKEND", "vulkan")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--autoplay"), "{err}");
}

/// The game binary with `args` on content `dir`, no relaunch (the
/// backend chosen), no window system needed for what fails first.
fn game(dir: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_rl-game"))
        .args(args)
        .arg("--content")
        .arg(dir)
        .env("WGPU_BACKEND", "vulkan")
        .output()
        .unwrap()
}
