//! `rl-cli net-soak` (plan 12c Task 11): the cases' generation, the
//! soak's bounds and the failure files.

use std::path::Path;

use rl_cli::netsim::{NetCase, NetSpec};
use rl_cli::soak::{MAX_FAILURES, SoakCase, SoakConfig, case, case_short, run, run_cases, transport_case};
use rl_cli::transport::{ServerExit, ServerExitHow, TransportSpec};
use rl_netcode::netsim::Profile;

use crate::common::content_dir;

fn config(failures: &Path, runs: u32) -> SoakConfig {
    SoakConfig {
        minutes: 60.0,
        seed: 1,
        transport_every: 0,
        runs: Some(runs),
        max_failures: MAX_FAILURES,
        failures: failures.to_path_buf(),
        content_dir: content_dir().map(Into::into).unwrap_or_default(),
    }
}

#[test]
fn cases_are_deterministic_and_within_bounds() {
    let (mut crash, mut leave, mut freeze, mut none) = (0, 0, 0, 0);
    let (mut departures, mut client_leaves, mut client_crashes) = (0, 0, 0);
    for seed in 0..200 {
        let spec = case(seed);
        assert_eq!(spec, case(seed), "seed {seed}");
        spec.scenario().unwrap_or_else(|e| panic!("seed {seed}: {e:#}"));
        assert!(Profile::by_name(&spec.profile).is_some(), "seed {seed}: {}", spec.profile);
        assert!((1500..=6000).contains(&spec.ticks), "{}", spec.ticks);
        assert!((2..=16).contains(&spec.slots), "{}", spec.slots);
        assert!((1..=spec.slots.min(7)).contains(&spec.clients), "{} clients", spec.clients);
        assert!(spec.spectators <= 2);
        assert!(spec.profile_changes.len() <= 3 && spec.outages.len() <= 2);
        for c in &spec.profile_changes {
            assert!(Profile::by_name(&c.2).is_some(), "profile change {c:?}");
        }
        for &(_, from, until) in &spec.outages {
            assert!((50..=600).contains(&(until - from)), "outage {from}..{until}");
            assert!(until <= spec.ticks, "outage {from}..{until} of {}", spec.ticks);
        }
        let middle = spec.ticks / 5..=spec.ticks * 4 / 5;
        // Plan 12d: a client departure (a leave or a crash) in about one
        // case in four, naming a client, in the middle 60 %; at most one.
        let host_exits: Vec<_> = spec.exits.iter().filter(|e| e.0 == 0).cloned().collect();
        assert!(spec.exits.len() - host_exits.len() <= 1, "seed {seed}: {:?}", spec.exits);
        for e in spec.exits.iter().filter(|e| e.0 != 0) {
            assert!((1..=u32::from(spec.clients)).contains(&e.0) && middle.contains(&e.1), "{e:?}");
            assert!(spec.clients >= 2, "seed {seed}: the only client departs: {e:?}");
            if e.2 == "crash" {
                client_crashes += 1
            } else {
                client_leaves += 1
            }
            departures += 1;
        }
        match (host_exits.as_slice(), spec.host_freeze) {
            ([], None) => none += 1,
            ([(0, tick, how)], None) => {
                assert!(middle.contains(tick), "exit at {tick} of {}", spec.ticks);
                if how == "crash" { crash += 1 } else { leave += 1 }
            }
            ([], Some((from, until))) => {
                assert!(middle.contains(&from), "freeze at {from} of {}", spec.ticks);
                assert!((160..=250).contains(&(until - from)), "freeze {from}..{until}");
                freeze += 1;
            }
            other => panic!("seed {seed}: more than one host exit: {other:?}"),
        }
    }
    assert!(crash > 0 && leave > 0 && freeze > 0, "{crash} crashes, {leave} leaves, {freeze} freezes");
    assert!((25..=75).contains(&departures), "about a quarter of the cases: {departures}");
    assert!(client_leaves > 0 && client_crashes > 0, "{client_leaves} leaves, {client_crashes} crashes");
    assert!((60..=140).contains(&(200 - none)), "about half have a host exit: {}", 200 - none);
    let mods: std::collections::BTreeSet<_> = (0..200).map(|s| case(s).mod_id).collect();
    assert_eq!(mods.len(), 2, "{mods:?}");
    assert_ne!(case(1), case(2));
    assert_eq!(case_short(7).ticks, 600);
}

#[test]
fn transport_cases_are_deterministic_and_within_bounds() {
    let dir = Path::new("data");
    let (mut with_exit, mut without) = (0, 0);
    for seed in 0..100 {
        let spec = transport_case(seed, dir);
        assert_eq!(spec.players, transport_case(seed, dir).players);
        assert_eq!(spec.server_exit, transport_case(seed, dir).server_exit);
        assert!((2..=4).contains(&spec.players));
        assert!((0.3..=0.5).contains(&spec.minutes), "{}", spec.minutes);
        assert!(matches!(spec.profile.as_deref(), Some("good" | "typical" | "bad")));
        assert_eq!(spec.migration, spec.server_exit.is_some(), "migration only with a server exit");
        match spec.server_exit {
            Some(e) => {
                with_exit += 1;
                assert!(e.after_s > 1.0 && e.after_s < spec.minutes * 60.0 * 0.8, "{e:?}");
                assert!(spec.fill_bots > spec.players, "the server's own bots play on");
            }
            None => without += 1,
        }
    }
    assert!((25..=75).contains(&with_exit), "about half: {with_exit} of {}", with_exit + without);
}

#[test]
fn a_short_soak_runs_and_reports() {
    if content_dir().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut out = Vec::new();
    let summary =
        run_cases(&config(dir.path(), 3), &mut |_, seed| SoakCase::Net(case_short(seed)), &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(summary.runs, 3, "{text}");
    assert!(summary.failures.is_empty(), "{text}");
    assert_eq!(text.lines().filter(|l| l.starts_with("run ") && l.contains(": ok (")).count(), 3, "{text}");
    assert!(text.contains("3 runs"), "{text}");
    assert!(text.contains("seed 1 ") && text.contains("seed 3 "), "case n is seeded seed + n: {text}");
    assert!(dir.path().read_dir().unwrap().next().is_none(), "nothing kept for passing cases");
}

#[test]
fn a_failing_case_is_kept_and_counted() {
    let Some(_) = content_dir() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut cases = |n: u32, seed: u64| {
        let mut spec = case_short(seed);
        if n == 2 {
            spec.mod_id = "no-such-mod".into();
        }
        SoakCase::Net(spec)
    };
    let mut out = Vec::new();
    let summary = run_cases(&config(dir.path(), 4), &mut cases, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(summary.runs, 4, "the soak goes on: {text}");
    assert_eq!(summary.failures.len(), 1, "{text}");
    let kept = NetCase::load(&summary.failures[0]).unwrap();
    assert_eq!(kept.spec.mod_id, "no-such-mod");
    let why = kept.failure.expect("names the error");
    assert!(why.contains("no-such-mod"), "{why}");
    assert!(text.lines().any(|l| l.starts_with("FAIL error: ")), "{text}");
    assert!(text.contains("1 failures"), "{text}");

    // The cap: the soak stops right after the failure it counts.
    let other = tempfile::tempdir().unwrap();
    let cfg = SoakConfig { max_failures: 1, ..config(other.path(), 4) };
    let summary = run_cases(&cfg, &mut cases, &mut Vec::new()).unwrap();
    assert_eq!((summary.runs, summary.failures.len()), (3, 1));
}

/// The cap with every case failing (a mod that does not exist), and a
/// content directory that is not there at all.
#[test]
fn the_failure_cap_stops_the_soak() {
    let dir = tempfile::tempdir().unwrap();
    let missing = SoakConfig { content_dir: dir.path().join("no-content"), ..config(dir.path(), 1) };
    assert!(run(&missing, &mut Vec::new()).is_err(), "no content: an error, not a failure");
    if content_dir().is_none() {
        return;
    }
    let cfg = SoakConfig { max_failures: 2, ..config(&dir.path().join("failures"), 10) };
    let mut bad =
        |_: u32, seed: u64| SoakCase::Net(NetSpec { mod_id: "no-such-mod".into(), ..case_short(seed) });
    let mut out = Vec::new();
    let summary = run_cases(&cfg, &mut bad, &mut out).unwrap();
    assert_eq!((summary.runs, summary.failures.len()), (2, 2), "{}", String::from_utf8_lossy(&out));
}

#[test]
fn an_error_case_round_trips_through_its_writer() {
    let dir = tempfile::tempdir().unwrap();
    let spec = case(5);
    let path = rl_cli::soak::write_error_case(dir.path(), &spec, &anyhow::anyhow!("no such pack")).unwrap();
    assert!(path.file_name().unwrap().to_str().unwrap().starts_with("net-"));
    let back = NetCase::load(&path).unwrap();
    assert_eq!(back.spec, spec);
    assert!(back.failure.unwrap().contains("no such pack"));
}

#[test]
fn a_transport_spec_round_trips_through_ron() {
    let mut spec = TransportSpec::new(3, 0.4, Some("bad"), Path::new("data"));
    spec.server_exit = Some(ServerExit { after_s: 7.5, how: ServerExitHow::Leave });
    spec.migration = true;
    spec.fill_bots = 5;
    spec.difficulty = Some(rl_bots::Difficulty::Xtreme);
    spec.spectators = vec![std::time::Duration::from_millis(2500)];
    spec.may_drop = vec!["Client 2".into()];
    let text = ron::ser::to_string_pretty(&spec, ron::ser::PrettyConfig::default()).unwrap();
    assert!(text.contains("\"xtreme\""), "difficulty by name: {text}");
    let back: TransportSpec = ron::from_str(&text).unwrap();
    assert_eq!(back, spec);
    spec.difficulty = None;
    let text = ron::to_string(&spec).unwrap();
    assert_eq!(ron::from_str::<TransportSpec>(&text).unwrap(), spec);
    let bad = text.replace("None", "Some(\"godlike\")");
    assert!(ron::from_str::<TransportSpec>(&bad).is_err(), "an unknown difficulty is an error");
}

/// Found by the soak's first minute: an outage cuts a peer off for longer
/// than `MAX_PREDICTION` ticks, which stalls it on any profile; the verdict
/// must not call that a failure of a `lan` link.
#[test]
fn an_outage_stall_is_not_a_failure() {
    let Some(c) = crate::common::content() else { return };
    let spec = NetSpec { outages: vec![(1, 150, 270)], profile: "lan".into(), ..case_short(4) };
    let spec = NetSpec { exits: vec![], host_freeze: None, profile_changes: vec![], ..spec };
    let r = rl_cli::netsim::run(&c, &spec).unwrap();
    assert!(r.report.stats[&1].stalls > 0, "the outage stalled peer 1");
    assert!(r.verdict.is_ok(), "{:?}", r.verdict);
}
