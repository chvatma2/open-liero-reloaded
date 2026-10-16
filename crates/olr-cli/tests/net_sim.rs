mod common;

use common::{content, full_tier};
use olr_cli::netsim::{NetRun, NetSpec, run};
use olr_netcode::Event;

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

fn pass(c: &olr_content::Content, s: &NetSpec) -> NetRun {
    let r = run(c, s).unwrap();
    if let Err(f) = &r.verdict {
        let path = olr_cli::netsim::write_failure(&common::failures_dir(), &r).unwrap();
        panic!(
            "{} failed at tick {}: {} (olr-cli net-sim --from {})",
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

/// Plan 12d on the real simulation: a client leaving mid-match is withdrawn
/// at one tick on every peer, the verdict's offline replay (the real world)
/// agrees, and the world after it holds the worm out. Coverage: green since
/// Task 4.
#[test]
fn a_client_leave_withdraws_its_worm_in_a_real_match() {
    let Some(c) = content() else { return };
    let s = NetSpec { ticks: 800, exits: vec![(2, 300, "leave".into())], ..spec("typical", 4) };
    let r = pass(&c, &s);
    let t = r.report.host_gone.iter().find(|g| g.1 == 1).expect("client 2 is slot 1").0;
    let mut f = olr_cli::netsim::WorldFactory::new(&c, &s).unwrap();
    let mut sim = olr_netcode::harness::Factory::sim(&mut f, 0);
    for (tick, row) in r.report.host_log.iter().enumerate().take(t as usize + 1) {
        if tick as u32 == t {
            olr_netcode::Simulation::withdraw(&mut sim, 1);
        }
        let keys: Vec<olr_sim::Keys> =
            row[..usize::from(s.slots)].iter().map(|&k| olr_sim::Keys(k)).collect();
        olr_netcode::Simulation::tick(&mut sim, &keys);
    }
    let w = sim.world();
    assert!(!w.worms()[1].alive && w.gone() == 0b10);
}

/// N87, ruling PF12d-5: a client that crashes on `bad` stalls the other
/// players for its timeout's fills; the verdict excuses exactly that.
/// RED before the window: "player peer 3 stalled".
#[test]
fn a_client_crash_on_bad_passes() {
    let Some(c) = content() else { return };
    pass(&c, &NetSpec { ticks: 1_600, exits: vec![(1, 300, "crash".into())], ..spec("bad", 5) });
}

/// A net-sim desync bundle carries the withdrawals, and `net-repro`
/// withdraws: its replay meets the host's state, and the planted fault's
/// divergence comes at the fault, not at the withdrawal near 200. RED: the
/// bundle has no `gone`, and the replay diverges at the withdrawal.
#[test]
fn a_bundle_with_a_withdrawal_reproduces_to_the_fault() {
    let Some(c) = content() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = NetSpec {
        ticks: 800,
        exits: vec![(2, 200, "leave".into())],
        faults: vec![(1, 500, 0, 1.0)],
        ..spec("typical", 6)
    };
    let r = olr_cli::netsim::run_with(&c, &s, Some(dir.path())).unwrap();
    let path = r.bundles.first().expect("the planted desync wrote a bundle");
    let b = olr_cli::bundle::DesyncBundle::decode(&std::fs::read(path).unwrap()).unwrap();
    assert!(b.gone.iter().any(|g| g.1 == 1));
    let repro = olr_cli::bundle::reproduce(&c, &b).unwrap();
    assert!(repro.first_local_divergence.is_some_and(|t| t >= 500), "{:?}", repro.first_local_divergence);
    assert!(repro.host_diff.is_none(), "{:?}", repro.host_diff);
}
