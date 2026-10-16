//! Headless bot matches (spec §7.1, §7.3) in the `check` tier: two short
//! matches of 8 bots on OLX's content, deterministic and replaying to the
//! same checksums, and a failing match that writes its replay and seed.

use crate::common;

use std::process::Command;

use common::*;
use olr_cli::runner::{End, Options, play};
use olr_cli::spec::{Difficulty, MatchSpec, Personality};
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
    let replay_file = files.iter().find(|p| p.extension().is_some_and(|e| e == "rlrp")).expect("a replay");
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
