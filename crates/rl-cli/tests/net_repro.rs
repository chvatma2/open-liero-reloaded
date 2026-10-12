mod common;

use rl_cli::bundle::{BUNDLE_VERSION, DesyncBundle, first_difference, reproduce};
use rl_cli::netsim::{NetSpec, run_with};
use serde_json::json;

#[test]
fn json_paths_name_the_first_difference() {
    let a = json!({"worms": [{"health": 100.0}, {"health": 100.0}], "time": 5});
    let b = json!({"worms": [{"health": 100.0}, {"health": 3.0}], "time": 5});
    assert_eq!(first_difference(&a, &b).as_deref(), Some("worms[1].health"));
    assert_eq!(first_difference(&a, &a), None);
    let c = json!({"worms": [{"health": 100.0}], "time": 5});
    assert_eq!(first_difference(&a, &c).as_deref(), Some("worms.len"));
}

#[test]
fn bundles_round_trip_and_bad_ones_are_named() {
    let spec: NetSpec = ron::from_str(
        r#"(mod_id: "m", level_id: "l", seed: 1, slots: 2, clients: 1, spectators: 0, profile: "lan", ticks: 10)"#,
    )
    .unwrap();
    let b = DesyncBundle {
        spec,
        peer: 1,
        fingerprint: 7,
        platform: "x86_64-linux".into(),
        inputs: vec![[3; 32]; 4],
        local_checksums: vec![(9, 1)],
        host: (9, 2),
        local_state: Some((3, vec![1, 2])),
        host_state: None,
    };
    let bytes = b.encode();
    assert_eq!(&bytes[..4], b"RLDS");
    assert_eq!(DesyncBundle::decode(&bytes).unwrap(), b);
    let mut bad = bytes.clone();
    bad[0] = b'X';
    assert!(format!("{:#}", DesyncBundle::decode(&bad).unwrap_err()).contains("magic"));
    let mut bad = bytes.clone();
    bad[4..6].copy_from_slice(&(BUNDLE_VERSION + 1).to_le_bytes());
    assert!(format!("{:#}", DesyncBundle::decode(&bad).unwrap_err()).contains("version"));
    assert!(DesyncBundle::decode(&bytes[..8]).is_err());
}

#[test]
fn a_planted_desync_is_repaired_and_pinpointed() {
    let Some(c) = common::content() else { return };
    let spec = NetSpec {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-castlestrike".into(),
        seed: 4,
        slots: 6,
        clients: 2,
        spectators: 0,
        join_tick: None,
        profile: "good".into(),
        drift_ppm: 0,
        ticks: 800,
        outages: vec![],
        net_seed: 4,
        difficulty: None,
        faults: vec![(1, 400, 3, 7.0)],
    };
    let dir = tempfile::tempdir().unwrap();
    let r = run_with(&c, &spec, Some(dir.path())).unwrap();
    assert!(r.verdict.is_ok(), "repaired: {:?}", r.verdict);
    assert_eq!(r.bundles.len(), 1);
    let path = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "rlds"))
        .expect("a bundle was written");
    assert_eq!(r.bundles[0], path);
    let b = DesyncBundle::decode(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(b.peer, 1);
    let repro = reproduce(&c, &b).unwrap();
    let t = repro.first_local_divergence.expect("the faulty peer's stream diverges from the replay");
    assert!((400..420).contains(&t), "diverged at {t}");
    let diff = repro.local_diff.expect("its state differs from the replay");
    assert!(diff.starts_with("worms[3]"), "{diff}");
    assert_eq!(repro.host_diff, None, "the host's state is the replay's");
}
