//! A Deathmatch against bots and its replay, played headless through
//! `Session` (the window only draws it). Needs the imported content
//! (`RL_CONTENT_DIR`); skips without it.

use std::path::{Path, PathBuf};

use rl_audio::Audio;
use rl_bots::Difficulty;
use rl_game::drive::percentile_ms;
use rl_game::hud::clock;
use rl_game::session::{Driver, Session};
use rl_game::setup::{self, DeathmatchOptions};
use rl_scene::{DrawList, Interpolation, Viewport};
use rl_sim::{EndReason, Keys};

fn content() -> Option<PathBuf> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping");
    }
    dir
}

fn session(dir: &Path, m: &str, level: &str, o: &DeathmatchOptions) -> (setup::Assets, Session) {
    let a = setup::load_deathmatch(dir, m, level, "default", o).unwrap();
    let vp = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
    let s = Session::new(&a, vp, Interpolation::Linear, Audio::silent()).unwrap();
    (a, s)
}

/// The player's keys at tick `t`: walk about, jump and shoot now and then.
fn player_keys(t: u64) -> Keys {
    let walk = if (t / 200).is_multiple_of(2) { Keys::RIGHT } else { Keys::LEFT };
    let jump = if t.is_multiple_of(97) { Keys::JUMP } else { 0 };
    let fire = if (t / 50).is_multiple_of(3) { Keys::FIRE } else { 0 };
    Keys(walk | jump | fire)
}

/// A short Deathmatch ends by its time limit: the results name the winner
/// (the mode's), list every worm in the mode's order with its kills,
/// deaths and suicides, and the clock stays at the end's time while the
/// world plays on.
#[test]
fn deathmatch_session_ends_with_results() {
    let Some(dir) = content() else { return };
    let o = DeathmatchOptions {
        seed: 3,
        bots: 3,
        difficulty: Difficulty::Hard,
        time_limit: 0.25,
        ..DeathmatchOptions::default()
    };
    let (a, mut s) = session(&dir, "Classic", "Dirt Level", &o);
    assert_eq!(s.time_left_ms, Some(15_000), "the clock starts at the limit");
    assert_eq!((s.names.of(0), s.names.of(1)), ("You".into(), "Bot 1".into()));
    let mut list = DrawList::new();
    let mut t = 0;
    while s.results.is_none() && t < 2000 {
        s.tick(player_keys(t));
        s.build(0.5, &mut list);
        t += 1;
    }
    let r = s.results.clone().expect("the time limit ends the match");
    let result = s.world.result().unwrap();
    assert_eq!(result.reason, EndReason::TimeLimit);
    assert_eq!(r.reason, "time limit");
    assert_eq!(r.winner, (result.winner >= 0).then(|| s.names.of(result.winner)));
    assert_eq!(r.rows.len(), 4, "every worm");
    for (row, score) in r.rows.iter().zip(s.world.scores()) {
        assert_eq!(row.worm, score.worm, "the mode's order");
        let w = &s.world.worms()[row.worm as usize];
        assert_eq!((row.kills, row.deaths, row.suicides), (w.kills, w.deaths, w.suicides));
        assert_eq!(row.name, s.names.of(row.worm));
    }
    assert_eq!(r.time_left_ms, Some(0));
    assert_eq!(clock(s.time_left_ms.unwrap()), "0:00");
    assert!(
        s.feed.all().any(|l| l == "Timelimit has been reached"),
        "{:?}",
        s.feed.all().collect::<Vec<_>>()
    );
    assert_eq!(s.tally.matches_ended, 1);
    // The world plays on (the physics stops 3 s later); the end stays.
    for _ in 0..400 {
        s.tick(Keys(0));
    }
    assert_eq!(s.results.as_ref(), Some(&r));
    assert_eq!(s.time_left_ms, Some(0));
    assert_eq!(s.world.result(), Some(result));
    // F5: a rematch on the next seed; the match played is handed back,
    // its recording finished 3 s after the end.
    let first = s.world.clone();
    let played = s.restart(&a).unwrap().expect("the first match's replay");
    assert_eq!(played.ticks() as u64, t + rl_game::drive::RECORD_AFTER_END);
    assert_eq!((s.rematches, s.ticks(), s.results.as_ref()), (1, 0, None));
    assert_eq!(s.time_left_ms, Some(15_000));
    assert_eq!(s.feed.all().count(), 0);
    assert_ne!(s.world.worms()[0].pos, first.worms()[0].pos, "another seed, other spawns");
    s.tick(Keys(0));
    assert!(s.world.result().is_none());
}

/// Eight bots on a big level think on the main thread within their work
/// budget every tick, and well within a 60 Hz frame (16.7 ms) together.
#[test]
fn bots_think_within_frame_budget() {
    let Some(dir) = content() else { return };
    let o = DeathmatchOptions {
        seed: 5,
        bots: 7,
        difficulty: Difficulty::Medium,
        ..DeathmatchOptions::default()
    };
    let (_a, mut s) = session(&dir, "MW 1.0", "CastleStrike", &o);
    for t in 0..1500 {
        s.tick(player_keys(t));
    }
    let Driver::Match(d) = &s.driver else { panic!("a Deathmatch") };
    assert_eq!(d.bots.len(), 7);
    assert_eq!(d.over_budget, 0, "no think spends more than its budget");
    assert!(d.units_max > 0, "the bots worked");
    assert_eq!(d.think_ns.len(), 1500, "the bots think once a tick");
    let (p50, p99) = (percentile_ms(&d.think_ns, 0.5), percentile_ms(&d.think_ns, 0.99));
    eprintln!(
        "7 bots on CastleStrike: think p50 {p50:.3} ms, p99 {p99:.3} ms, max {:.3} ms",
        d.think_ns_max as f64 / 1e6
    );
    assert!(p99 < 8.0, "the bots' p99 think time {p99:.3} ms leaves no room in a frame");
}

/// A Deathmatch's recording, saved and loaded as a file, plays back in a
/// replay session to the same world, every recorded checksum matching.
#[test]
fn replay_session_matches_recorded_checksums() {
    let Some(dir) = content() else { return };
    let o = DeathmatchOptions {
        seed: 9,
        bots: 3,
        difficulty: Difficulty::Xtreme,
        ..DeathmatchOptions::default()
    };
    let (_a, mut s) = session(&dir, "MW 1.0", "Liero Factory", &o);
    for t in 0..750 {
        s.tick(player_keys(t));
    }
    let end = s.world.checksum();
    let replay = s.take_replay().expect("a Deathmatch records");
    assert_eq!(replay.ticks(), 750);
    assert!(s.take_replay().is_none(), "taken once");
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("match.rlrp");
    std::fs::write(&file, replay.encode()).unwrap();

    let a = setup::load_replay(&dir, &file, "default").unwrap();
    let vp = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
    let mut r = Session::new(&a, vp, Interpolation::Linear, Audio::silent()).unwrap();
    assert_eq!((r.names.of(0), r.names.of(1)), ("Worm 1".into(), "Worm 2".into()));
    for _ in 0..800 {
        r.tick(Keys(Keys::FIRE)); // ignored: the replay plays its own keys
    }
    let Driver::Replay(d) = &r.driver else { panic!("a replay") };
    assert!(d.is_done());
    assert_eq!(d.mismatch, None);
    assert_eq!(d.cursor.checked(), replay.checksums.len(), "every checksum checked");
    assert_eq!(r.ticks(), 750, "the replay stops at its end");
    assert_eq!(r.world.checksum(), end, "the same world");
    // F5 plays it again from the start.
    r.restart(&a).unwrap();
    assert_eq!(r.ticks(), 0);
    r.tick(Keys(0));
    assert_eq!(r.ticks(), 1);

    // Other keys in tick 50: the divergence shows at the checksum of tick
    // 100, and the replay plays on to its end.
    let mut bad = replay.clone();
    bad.row_mut(50)[0] = Keys(Keys::JUMP | Keys::FIRE | Keys::LEFT);
    std::fs::write(&file, bad.encode()).unwrap();
    let a = setup::load_replay(&dir, &file, "default").unwrap();
    let mut r = Session::new(&a, vp, Interpolation::Linear, Audio::silent()).unwrap();
    for _ in 0..800 {
        r.tick(Keys(0));
    }
    let Driver::Replay(d) = &r.driver else { panic!("a replay") };
    assert_eq!(d.mismatch.map(|m| m.tick), Some(100));
    assert!(d.is_done());
    assert_eq!(r.ticks(), 750);
}
