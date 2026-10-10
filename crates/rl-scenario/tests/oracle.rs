//! Acceptance (spec §6, §10): every scenario in `fixtures/oracle`, replayed in
//! the Rust simulation, matches OLX's recorded trace bit for bit in every field
//! the simulation reproduces so far (`rl_scenario::CHECKED`).
//! Needs the imported OLX content (`RL_CONTENT_DIR`, default `target/content`).

use std::path::{Path, PathBuf};

/// The imported content (`RL_CONTENT_DIR`, which `.cargo/config.toml` defaults
/// to `target/content`), or `None` with a notice if it has not been imported.
fn content_dir() -> Option<String> {
    let dir = std::env::var("RL_CONTENT_DIR").ok()?;
    if std::path::Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        None
    }
}

/// Scenarios the simulation cannot play yet; each must still diverge, so the
/// list shrinks as M4 lands. Empty at the end of M4.
const EXPECTED_TO_DIVERGE: [&str; 10] = [
    "beam-burst",
    "bleed",
    "bonus-health",
    "bonus-weapon",
    "die-before-delivery",
    "explode-kill-respawn",
    "fire-minigun",
    "multi-shot",
    "reload-switch",
    "two-worms-fire",
];

/// The scenarios that must still diverge: [`EXPECTED_TO_DIVERGE`] and every
/// weapon scenario (`weapons/<mod>/<weapon>`, `cargo xtask oracle-gen-weapons`).
fn expected_to_diverge(name: &str) -> bool {
    name.starts_with("weapons/") || EXPECTED_TO_DIVERGE.contains(&name)
}

#[test]
fn every_scenario_matches_olx_bit_for_bit() {
    let Some(content) = content_dir() else { return };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let names = rl_scenario::fixture_names(&dir).unwrap();
    let matching = ["aim", "dig", "fall", "jump", "rope", "rope-dirt", "tap", "walk"];
    for n in matching.iter().chain(&EXPECTED_TO_DIVERGE) {
        assert!(names.iter().any(|m| m == n), "missing scenario {n}");
    }
    assert_eq!(names.iter().filter(|n| n.starts_with("weapons/")).count(), 38 + 50, "weapon scenarios");
    assert_eq!(names.len(), matching.len() + EXPECTED_TO_DIVERGE.len() + 38 + 50, "{names:?}");
    let mut failures = Vec::new();
    for name in names {
        let (sc, expected) = rl_scenario::load_fixture(&dir, &name).unwrap();
        let actual = rl_scenario::replay(Path::new(&content), &sc, &expected).unwrap();
        let divergence = rl_oracle::compare_fields(&expected, &actual, &sc.tolerances, rl_scenario::CHECKED);
        match (divergence, expected_to_diverge(&name)) {
            (Some(d), false) => failures.push(format!("{name}: {d}")),
            (None, true) => failures.push(format!("{name}: matches now; remove it from EXPECTED_TO_DIVERGE")),
            _ => {}
        }
    }
    assert!(failures.is_empty(), "diverged from OLX:\n{}", failures.join("\n"));
}

#[test]
fn a_changed_trace_is_detected() {
    let Some(content) = content_dir() else { return };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let (sc, mut expected) = rl_scenario::load_fixture(&dir, "walk").unwrap();
    let actual = rl_scenario::replay(Path::new(&content), &sc, &expected).unwrap();
    expected.frames[200].w[0].x = rl_oracle::Num(expected.frames[200].w[0].x.0 + 1.0);
    let d = rl_oracle::compare_fields(&expected, &actual, &[], rl_scenario::CHECKED).unwrap();
    assert_eq!((d.frame, d.path.as_str()), (199, "w[0].x"));
}
