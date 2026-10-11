//! The engine stress test's runner (`rl_cli::stress`), short: 32 chaos
//! bots with near-instant reloads on a pool of 200 projectiles fill it at
//! once; every tick's invariants hold, the buffers stay bounded, the match
//! is the same when played again and its replay reproduces it. Needs
//! `RL_CONTENT_DIR`; skips without it. (`cargo xtask stress` plays it for
//! minutes on Castle Strike; `full` 20 s against a baseline.)

use std::path::PathBuf;

use rl_cli::spec::Personality;
use rl_cli::stress::{self, Config, Spread};

fn content() -> Option<rl_content::Content> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping");
    }
    dir.map(|d| rl_content::Content::open(&d).unwrap())
}

#[test]
fn stress_run_is_bounded_and_deterministic() {
    let Some(content) = content() else { return };
    let spec = stress::spec("olx-mw-1-0", "olx-level-dirt-level", 32, 200, 4.0, 3);
    let cfg = Config { render_size: (64, 36), ..Config::new(spec) };
    let mut lines = Vec::new();
    let r = stress::run(&content, &cfg, &mut |l| lines.push(l)).unwrap();
    assert_eq!(r.ticks, 400);
    assert_eq!(r.pool_capacity, 200);
    assert!(r.projectiles_peak == 200 && r.ticks_at_pool_cap > 0, "the small pool fills: {r:?}");
    assert!(r.shots > 100, "chaos bots fire all the time: {}", r.shots);
    assert_eq!(r.deterministic, Some(true));
    assert!(r.tick_us.p50 > 0.0 && r.tick_us.max >= r.tick_us.p99 && r.tick_us.p99 >= r.tick_us.p50);
    assert!(r.render.is_none());
    if cfg!(target_os = "linux") {
        assert!(r.peak_mib.is_some_and(|m| m > 1.0));
    }
    assert!(r.text().contains("projectiles: peak 200 of 200"), "{}", r.text());
    assert!(lines.iter().any(|l| l.contains("playing it again")));
}

/// The envelope's knobs (plan 8) measure more without touching the match:
/// the same shots, kills and projectiles with a larger effects pool and a
/// draw list built every tick; the recorded replay is the match.
#[test]
fn the_bench_knobs_measure_without_changing_the_match() {
    let Some(content) = content() else { return };
    let spec = stress::spec("olx-mw-1-0", "olx-level-dirt-level", 32, 3000, 8.0, 3);
    let plain =
        stress::run(&content, &Config { verify: false, ..Config::new(spec.clone()) }, &mut |_| {}).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("stress.rlrp");
    let cfg = Config {
        verify: false,
        fx_capacity: 16_384,
        build_every: 1,
        record: Some(file.clone()),
        ..Config::new(spec)
    };
    let r = stress::run(&content, &cfg, &mut |_| {}).unwrap();
    assert_eq!(
        (r.shots, r.kills, r.projectiles_peak, r.ticks),
        (plain.shots, plain.kills, plain.projectiles_peak, plain.ticks),
        "the knobs never touch the simulation"
    );
    assert_eq!((r.fx_capacity, plain.fx_capacity), (16_384, rl_fx::MAX_ENTITIES));
    assert_eq!(plain.personality, Personality::Chaos, "the stress bots stay chaos by default");
    assert!(r.fx_peak > rl_fx::MAX_ENTITIES, "32 chaos bots overflow OLX's 1024 entities: {}", r.fx_peak);
    assert!(r.build_us.p50 > 0.0 && r.present_us.p50 > 0.0, "{r:?}");
    assert_eq!(plain.build_us, Spread::default(), "no draw list without --build-every");
    assert!(r.area_cache.1 > 0 && r.area_cache.0 <= r.area_cache.1);
    assert!(r.level.0 > 0 && r.level.1 > 0 && r.level.2 >= 1);
    assert!(r.caps.is_empty(), "{:?}", r.caps);
    let replay = rl_replay::Replay::decode(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(replay.ticks(), r.ticks);
}

/// Deathmatch bots (plan 8 ruling T1-A) route through the shared area
/// cache, which chaos bots never touch: the envelope's map-size and
/// art-scale sweeps measure it. Their stress match keeps its bounds, plays
/// the same again and its replay reproduces it.
#[test]
fn deathmatch_bots_fill_the_area_cache() {
    let Some(content) = content() else { return };
    let spec = stress::spec("olx-classic", "olx-level-dirt-level", 6, 3000, 4.0, 3);
    let cfg = Config { personality: Personality::Deathmatch, ..Config::new(spec) };
    let r = stress::run(&content, &cfg, &mut |_| {}).unwrap();
    assert_eq!((r.ticks, r.personality), (400, Personality::Deathmatch));
    assert!(r.name.contains("-6dm-"), "{}", r.name);
    assert!(r.area_cache.0 > 0 && r.area_cache.0 <= r.area_cache.1, "{:?}", r.area_cache);
    assert_eq!(r.deterministic, Some(true));
    assert!(r.text().contains("deathmatch bots;"), "{}", r.text());
}

/// A generated level (plan 8) plays like a pack's: bounded, deterministic,
/// and its replay reproduces it (made again from the id, not from a pack).
/// Small (ruling PF3): the envelope plays the large ones. An id past the
/// limits is an error before anything is made (ruling PF7).
#[test]
fn a_generated_level_plays_and_replays() {
    let Some(content) = content() else { return };
    let spec = stress::spec("olx-classic", "gen-256x256-a2-s1", 8, 3000, 3.0, 1);
    let r = stress::run(&content, &Config::new(spec), &mut |_| {}).unwrap();
    assert_eq!(r.level, (256, 256, 2));
    assert_eq!(r.deterministic, Some(true));
    assert!(r.caps.is_empty(), "{:?}", r.caps);
    assert!(r.text().contains("level 256x256 at art scale 2"), "{}", r.text());
    let spec = stress::spec("olx-classic", "gen-256x256-a2-s1", 4, 3000, 2.0, 1);
    let cfg = Config { personality: Personality::Deathmatch, ..Config::new(spec) };
    let r = stress::run(&content, &cfg, &mut |_| {}).unwrap();
    assert_eq!((r.personality, r.deterministic), (Personality::Deathmatch, Some(true)));
    let huge = stress::spec("olx-classic", "gen-4096x4096-a8-s1", 2, 3000, 0.05, 1);
    let e = stress::run(&content, &Config::new(huge), &mut |_| {}).unwrap_err();
    assert!(format!("{e:#}").contains("generated level"), "{e:#}");
}
