//! The `cargo xtask full` tier (`OLR_FULL_TESTS`) of headless bot matches
//! (spec §7.1): the grid of both mods × the 4 levels × 3 seeds with chaos
//! and Deathmatch bots, whose coverage must include every weapon of both
//! mods and every action kind they use; and a 30-minute match whose costs
//! stay bounded (plan 6 Review Focus 5).

mod common;

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use olr_cli::coverage::Coverage;
use olr_cli::runner::{Match, MatchRun, Options, play};
use olr_cli::spec::{Difficulty, MatchSpec, Personality};
use olr_cli::verify;

/// Runs `specs` on all cores; gives each run with its verification (a
/// failure, captured in `target/failures/`).
fn run_all(c: &olr_content::Content, specs: &[MatchSpec]) -> Vec<(MatchRun, Option<String>)> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(specs.len());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                while let Some(spec) = specs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let run = play(c, spec, &Options::default()).unwrap();
                    let verified = verify(c, &run).unwrap().map(|f| capture(&run, &f));
                    out.lock().unwrap().push((run, verified));
                }
            });
        }
    });
    // `verify` gives a failed run's own failure.
    let mut runs = out.into_inner().unwrap();
    runs.sort_by_key(|(r, _)| r.spec.stem());
    runs
}

#[test]
fn coverage_report_lists_every_weapon() {
    if !full_tier() {
        return;
    }
    let Some(c) = content() else { return };
    let mut specs = Vec::new();
    for m in MODS {
        for level in LEVELS {
            for seed in [101, 202, 303] {
                for personality in [Personality::Chaos, Personality::Deathmatch] {
                    specs.push(MatchSpec {
                        personality,
                        difficulty: Difficulty::Mixed,
                        bonus_frequency: Some(5.0),
                        kill_limit: if personality == Personality::Chaos { 0 } else { 15 },
                        ..MatchSpec::new(m, level, 8, seed, 6000)
                    });
                }
            }
        }
    }
    let runs = run_all(&c, &specs);
    let mut by_mod: BTreeMap<String, Coverage> = BTreeMap::new();
    let mut problems = Vec::new();
    for (run, verified) in &runs {
        eprintln!("{}", run.summary());
        if let Some(f) = verified {
            problems.push(f.clone());
        }
        if run.p99_us() > 10_000.0 {
            problems.push(format!("{}: p99 tick {:.0} µs over 10 ms", run.spec.stem(), run.p99_us()));
        }
        by_mod.entry(run.spec.mod_id.clone()).or_default().merge(&run.coverage);
    }
    for (m, cov) in &by_mod {
        let sim = c.load_mod(m).unwrap().sim;
        let report = cov.report(&sim);
        eprintln!("== {m}\n{report}");
        for w in sim.weapons() {
            assert!(report.contains(&w.name), "{m}: the report lists {}", w.name);
        }
        let (weapons, actions) = (cov.missing_weapons(&sim), cov.missing_actions(&sim));
        if !weapons.is_empty() || !actions.is_empty() {
            problems.push(format!("{m}: never fired {weapons:?}; never triggered {actions:?}"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Review Focus 5: a 30-minute match of 8 bots on the largest level grows
/// no per-tick cost (bot thinking and the tick allocate nothing; every
/// think stays within its budget; events, projectiles and entities stay
/// within their bounds), the bots' area cache stays within its capacity
/// and the replay grows by a bounded amount per minute.
#[test]
fn long_bot_match_bounded() {
    if !full_tier() {
        return;
    }
    let Some(c) = content() else { return };
    const MINUTE: u32 = 6000;
    let spec = MatchSpec {
        difficulty: Difficulty::Mixed,
        bonus_frequency: Some(10.0),
        kill_limit: 0,
        time_limit: 0.0,
        ..MatchSpec::new(CLASSIC, "olx-level-castlestrike", 8, 404, 30 * MINUTE)
    };
    let mut m = Match::new(&c, &spec, &Options::default()).unwrap();
    let (mut allocs, mut most_events, mut most_projectiles) = (0, 0, 0);
    let mut minutes = Vec::new();
    for t in 0..spec.ticks {
        allocs += allocation_counter::measure(|| m.advance()).count_total;
        if let Err(f) = m.settle() {
            panic!("{}", capture(&m.finish(), &f));
        }
        most_events = most_events.max(m.events().len());
        most_projectiles = most_projectiles.max(m.world().projectiles().count());
        if (t + 1) % MINUTE == 0 {
            minutes.push((m.cache_len(), m.p99_us_since((t + 1 - MINUTE) as usize)));
        }
    }
    let run = m.finish();
    let bytes = run.replay.encode().len();
    eprintln!("{}", run.summary());
    eprintln!("per minute (area cache, p99 µs): {minutes:?}");
    eprintln!("most events {most_events}, projectiles {most_projectiles}; replay {bytes} bytes");
    assert!(run.failure().is_none(), "{}", run.summary());
    assert_eq!(allocs, 0, "bot thinking and the tick allocated");
    assert!(run.stats.units_max <= run.stats.budget_min, "a think went over its budget");
    assert!(minutes.iter().all(|&(len, _)| len <= olr_bots::AreaCache::DEFAULT_CAPACITY));
    assert!(minutes.iter().all(|&(_, p99)| p99 <= 10_000.0), "{minutes:?}");
    assert!(bytes <= 30 * 20_000, "the replay grew by {} bytes a minute", bytes / 30);
}
