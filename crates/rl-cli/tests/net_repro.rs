use crate::common;

use proptest::prelude::*;
use rl_cli::bundle::{BUNDLE_VERSION, DesyncBundle, FIELDS, first_difference, reproduce};
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
    let worms: Vec<&str> = diff.split(", ").filter(|p| p.starts_with("worms")).collect();
    assert_eq!(worms.len(), 1, "{diff}");
    assert!(worms[0].starts_with("worms[3]"), "{diff}");
    assert!(!diff.contains("worms[0]") && !diff.contains("worms[2]") && !diff.contains("worms[4]"), "{diff}");
    assert!(repro.uncovered.is_empty());
    assert_eq!(repro.host_diff, None, "the host's state is the replay's");
}

#[test]
fn fields_are_the_world_states_fields() {
    let Some(c) = common::content() else { return };
    let spec: NetSpec = ron::from_str(
        r#"(mod_id: "olx-classic", level_id: "olx-level-castlestrike", seed: 1, slots: 2, clients: 1, spectators: 0, profile: "lan", ticks: 10)"#,
    )
    .unwrap();
    use rl_netcode::harness::Factory as _;
    let sim = rl_cli::netsim::WorldFactory::new(&c, &spec).unwrap().sim(0);
    let value = serde_json::to_value(sim.world().state()).unwrap();
    let mut real: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
    let mut ours: Vec<&str> = FIELDS.to_vec();
    real.sort_unstable();
    ours.sort_unstable();
    assert_eq!(ours, real);
}

fn valid_bundle() -> Vec<u8> {
    let spec: NetSpec = ron::from_str(
        r#"(mod_id: "m", level_id: "l", seed: 1, slots: 2, clients: 1, spectators: 0, profile: "lan", ticks: 10, faults: [(1, 4, 0, 3.0)])"#,
    )
    .unwrap();
    DesyncBundle {
        spec,
        peer: 1,
        fingerprint: 7,
        platform: "x86_64-linux".into(),
        inputs: vec![[3; 32]; 12],
        local_checksums: vec![(9, 1)],
        host: (9, 2),
        local_state: Some((3, vec![1, 2, 3])),
        host_state: Some((4, vec![9; 40])),
    }
    .encode()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = DesyncBundle::decode(&bytes);
        let mut with_header = b"RLDS\x01\x00".to_vec();
        with_header.extend(&bytes);
        let _ = DesyncBundle::decode(&with_header);
    }

    #[test]
    fn decoding_mutated_bundles_never_panics(
        edits in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..8),
        cut in proptest::option::of(any::<usize>()),
    ) {
        let mut b = valid_bundle();
        for (i, v) in edits {
            let n = b.len();
            b[i % n] = v;
        }
        if let Some(c) = cut {
            b.truncate(c % (b.len() + 1));
        }
        let _ = DesyncBundle::decode(&b);
    }
}
