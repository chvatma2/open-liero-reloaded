mod common;

use common::{content, full_tier};
use rl_cli::netsim::{NetRun, NetSpec, run};
use rl_netcode::Event;

fn spec(profile: &str, seed: u32) -> NetSpec {
    NetSpec {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-castlestrike".into(),
        seed,
        slots: 8,
        clients: 3,
        spectators: 1,
        join_tick: None,
        profile: profile.into(),
        drift_ppm: 2_000,
        ticks: 600,
        outages: vec![],
        net_seed: seed as u64,
        difficulty: None,
        faults: vec![],
        exits: vec![],
        host_freeze: None,
        profile_changes: vec![],
        migration: false,
    }
}

fn pass(c: &rl_content::Content, s: &NetSpec) -> NetRun {
    let r = run(c, s).unwrap();
    if let Err(f) = &r.verdict {
        let path = rl_cli::netsim::write_failure(&common::failures_dir(), &r).unwrap();
        panic!(
            "{} failed at tick {}: {} (rl-cli net-sim --from {})",
            s.stem(),
            f.tick,
            f.what,
            path.display()
        );
    }
    r
}

#[test]
fn bots_play_online_over_a_typical_and_a_bad_network() {
    let Some(c) = content() else { return };
    pass(&c, &spec("typical", 1));
    pass(&c, &spec("bad", 2));
}

#[test]
fn a_spectator_joins_a_real_match() {
    let Some(c) = content() else { return };
    let mut s = spec("typical", 3);
    s.join_tick = Some(300);
    pass(&c, &s);
}

/// Spec §3.7 on real worlds and bots: the host crashes at tick 400 of
/// 1000; client 1 (the first successor) takes the match over, its bots
/// restarted on its world, and every survivor agrees with an offline replay
/// of its inputs. The stalls the host's departure causes are excused only
/// within the migration (bounded by N66's 3 s).
#[test]
fn a_host_crash_migrates_a_real_match() {
    let Some(c) = content() else { return };
    let mut s = spec("typical", 1);
    s.ticks = 1_000;
    s.exits = vec![(0, 400, "crash".into())];
    let r = pass(&c, &s);
    assert_eq!(r.report.hosts, [0, 1]);
    assert_eq!(r.report.migrations.len(), 1);
    assert!(r.report.host_lost.is_empty(), "{:?}", r.report.host_lost);
    assert!(r.report.exited.contains(&0));
    let after = &r.report.host_log[600..900];
    assert!(after.iter().any(|row| row[5] != 0), "a restarted bot plays at the new host");
    let summary = r.summary();
    assert!(summary.contains("migrated: 0 -> 1 at tick "), "{summary}");
}

/// N70 on real worlds: a host frozen 1.7 s (past `STEP_DOWN_US`, short of
/// `HOST_SILENT_US`) steps down when it wakes, and its `Bye` alone moves the
/// match: no peer ever found it silent.
#[test]
fn a_frozen_host_steps_down_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut s = spec("good", 2);
    s.ticks = 1_000;
    s.host_freeze = Some((400, 570));
    let r = pass(&c, &s);
    assert_eq!(r.report.hosts, [0, 1]);
    assert!(r.report.exited.contains(&0), "the host stepped down");
    for (peer, events) in &r.report.events {
        assert!(!events.contains(&Event::HostSilent), "peer {peer} found the host silent: no step-down");
    }
}

#[test]
fn net_grid() {
    if !full_tier() {
        return;
    }
    let Some(c) = content() else { return };
    for profile in ["lan", "good", "typical", "bad", "awful"] {
        for seed in 1..=4 {
            let mut s = spec(profile, seed);
            s.ticks = 3_000;
            if seed % 2 == 0 {
                s.mod_id = "olx-mw-1-0".into();
            }
            pass(&c, &s);
        }
    }
}

/// Every profile × a crash, a leave and a 2 s freeze of the host at tick 800
/// of 2000 (MW 1.0 for the freezes).
#[test]
fn net_migration_grid() {
    if !full_tier() {
        return;
    }
    let Some(c) = content() else { return };
    for profile in ["lan", "good", "typical", "bad", "awful"] {
        for how in ["crash", "leave", "freeze"] {
            let mut s = spec(profile, 1);
            s.ticks = 2_000;
            if how == "freeze" {
                s.mod_id = "olx-mw-1-0".into();
                s.host_freeze = Some((800, 1_000));
            } else {
                s.exits = vec![(0, 800, how.into())];
            }
            let r = pass(&c, &s);
            assert_eq!(r.report.hosts.len(), 2, "{profile} {how}: {:?}", r.report.hosts);
            assert!(r.report.host_lost.is_empty(), "{profile} {how}: {:?}", r.report.host_lost);
        }
    }
}
