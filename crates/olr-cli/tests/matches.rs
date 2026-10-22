//! Headless bot matches (spec §7.1, §7.3) in the `check` tier: two short
//! matches of 8 bots on OLX's content, deterministic and replaying to the
//! same checksums, a match of local players pressing scripted keys (issue
//! #29), and a failing match that writes its replay and seed.

use crate::common;

use std::process::Command;

use common::*;
use olr_cli::runner::{End, Match, Options, play};
use olr_cli::spec::{Difficulty, MAX_LOCALS, MatchSpec, Personality, local_keys};
use olr_cli::verify;
use olr_replay::Replay;

/// Plays `spec`, checks it ends cleanly, then plays it again and replays
/// its recording (`verify`); a failure is captured in `target/failures/`.
fn clean_and_verified(spec: &MatchSpec) {
    let Some(c) = content() else { return };
    let run = play(&c, spec, &Options::default()).unwrap();
    eprintln!("{}", run.summary());
    if let Some(f) = verify(&c, &run).unwrap() {
        panic!("{}", capture(&run, &f));
    }
    assert_eq!(run.replay.ticks(), run.ticks);
    assert!(run.ticks == spec.ticks || matches!(run.end, End::GameOver { .. }), "{:?}", run.end);
    assert!(run.coverage.shots() > 0, "nobody fired");
}

#[test]
fn classic_deathmatch_bots_are_deterministic_and_replay() {
    let spec = MatchSpec {
        difficulty: Difficulty::Mixed,
        bonus_frequency: Some(5.0),
        ..MatchSpec::new(CLASSIC, "olx-level-dirt-level", 8, 31, 3000)
    };
    clean_and_verified(&spec);
}

#[test]
fn mw_chaos_bots_are_deterministic_and_replay() {
    let spec = MatchSpec {
        personality: Personality::Chaos,
        kill_limit: 0,
        ..MatchSpec::new(MW, "olx-level-liero-factory", 8, 32, 3000)
    };
    clean_and_verified(&spec);
}

/// Issue #29: `--locals 4` plays four local players, worms 0 to 3 before
/// the bots, each moved by its own scripted keys and no other's (every
/// tick's recorded keys of worm `p` are `local_keys(p, t)`, and the four
/// differ); the match is deterministic and replays from its recording to
/// the same checksums, its local slots recorded; the bots keep their own
/// configurations.
#[test]
fn local_players_press_their_own_keys_and_replay() {
    let Some(c) = content() else { return };
    let spec =
        MatchSpec { locals: MAX_LOCALS, ..MatchSpec::new(CLASSIC, "olx-level-dirt-level", 2, 33, 1500) };
    assert_eq!(spec.match_options().players.len(), 6);
    assert_eq!(spec.stem(), "classic-dirt-level-4l-2dm-mixed-seed33");
    clean_and_verified(&spec);
    let mut m = Match::new(&c, &spec, &Options::default()).unwrap();
    m.step();
    let start: Vec<_> = m.world().worms().iter().map(|w| w.pos).collect();
    let mut farthest = [0f32; MAX_LOCALS];
    while m.step() {
        for (p, far) in farthest.iter_mut().enumerate() {
            let (a, b) = (start[p], m.world().worms()[p].pos);
            *far = far.max((a.x - b.x).abs() + (a.y - b.y).abs());
        }
    }
    eprintln!("farthest from the spawn: {farthest:?}");
    assert!(farthest.iter().all(|&d| d > 10.0), "every local player's worm moved: {farthest:?}");
    let run = m.finish();
    let r = &run.replay;
    assert_eq!(r.setup.viewers, [0, 1, 2, 3], "the local slots, recorded");
    for t in 0..r.ticks() {
        for p in 0..MAX_LOCALS {
            assert_eq!(r.row(t)[p], local_keys(p, t), "tick {t}: worm {p} pressed player {p}'s keys");
        }
    }
    let differ = (0..r.ticks()).filter(|&t| (1..MAX_LOCALS).any(|p| r.row(t)[p] != r.row(t)[0])).count();
    assert!(differ > r.ticks() as usize / 2, "each its own keys: {differ} of {} ticks differ", r.ticks());
    // Too many local players, or too few worms, are refused.
    let five = MatchSpec { locals: MAX_LOCALS + 1, ..spec.clone() };
    assert!(play(&c, &five, &Options::default()).is_err());
    let one = MatchSpec { locals: 1, bots: 0, ..spec };
    assert!(play(&c, &one, &Options::default()).is_err(), "a Deathmatch needs two worms");
}

fn olr_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_olr-cli"))
}

/// Spec §7.3: a failing match writes its replay and seed to the failures
/// directory, and `olr-cli replay <file> --until <tick> --dump` reproduces
/// it. The failure is injected (`--fail-at`, a panic in the tick).
#[test]
fn failure_writes_replay() {
    let Some(dir) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let out = olr_cli()
        .args(["match", "--content", &dir, "--mod", CLASSIC, "--level", "olx-level-dirt-level"])
        .args(["--mode", "deathmatch", "--bots", "4", "--seed", "77", "--ticks", "400", "--fail-at", "150"])
        .arg("--failures")
        .arg(tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "a failing match exits with status 1\n{stdout}");
    assert!(stdout.contains("failed at tick 150"), "{stdout}");
    let files: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().path()).collect();
    let replay_file = files.iter().find(|p| p.extension().is_some_and(|e| e == "olrp")).expect("a replay");
    let info = files.iter().find(|p| p.extension().is_some_and(|e| e == "ron")).expect("the failure");
    let info = std::fs::read_to_string(info).unwrap();
    assert!(info.contains("seed: 77") && info.contains("tick: 150"), "{info}");
    assert!(info.contains(&replay_file.file_name().unwrap().to_string_lossy().into_owned()), "{info}");

    // The replay holds the failing tick, whose keys come last.
    let replay = Replay::decode(&std::fs::read(replay_file).unwrap()).unwrap();
    assert_eq!(replay.ticks(), 151);
    let out = olr_cli()
        .args(["replay", "--content", &dir, "--until", "150", "--dump"])
        .arg(replay_file)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("tick 150") && stdout.contains("\"worms\""), "{stdout}");
    let out = olr_cli().args(["replay", "--content", &dir]).arg(replay_file).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
}

/// Plan 15c: `--personality idle` plays a match of bots that do nothing,
/// and it verifies.
#[test]
fn match_takes_idle_bots() {
    let Some(dir) = content_dir() else { return };
    let out = olr_cli()
        .args(["match", "--content", &dir, "--mod", CLASSIC, "--level", "olx-level-dirt-level"])
        .args(["--personality", "idle", "--bots", "2", "--seed", "1", "--ticks", "500"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
}

/// Issue #29: `--locals N` on the command line plays N local players (here
/// two, without bots), and the match verifies.
#[test]
fn match_takes_local_players() {
    let Some(dir) = content_dir() else { return };
    let out = olr_cli()
        .args(["match", "--content", &dir, "--mod", CLASSIC, "--level", "olx-level-dirt-level"])
        .args(["--locals", "2", "--bots", "0", "--seed", "1", "--ticks", "500"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("-2l-0dm-") && stdout.contains("ok   deterministic"), "{stdout}");
}
