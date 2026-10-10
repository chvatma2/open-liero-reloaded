//! Acceptance (spec §6, §10): every scenario in `fixtures/oracle`, replayed in
//! the Rust simulation, matches OLX's recorded trace bit for bit in every field
//! the simulation reproduces so far (`rl_scenario::CHECKED`).
//! Needs the imported OLX content (`RL_CONTENT_DIR`, default `target/content`).

use std::path::{Path, PathBuf};

use rl_oracle::Fields;

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

/// Scenarios the simulation cannot play yet: each must still diverge, and
/// first exactly at the `(frame, field)` listed (as `compare_fields` with
/// `CHECKED` reports it). The last column names the task that owns that
/// divergence, or the deviation behind it. The list shrinks as M4 lands;
/// at its end only deviations stay.
///
/// The exact first divergence is the guard against hidden regressions: a
/// change that breaks an already ported field earlier in a listed scenario
/// moves its first divergence, and the test fails with the new pair. Every
/// pair here sits at a frame where OLX respawns a worm after a D9 death.
const EXPECTED_DIVERGENCES: &[(&str, i64, &str, &str)] = &[
    ("weapons/mw-1-0/assassin-s-shot", 276, "mt", D9),
    ("weapons/mw-1-0/flak", 325, "mt", D9),
    ("weapons/mw-1-0/plasma-launcher", 283, "w[1].x", D9),
];

/// The owner of a divergence that stays (spec §18 D9): the dummy dies at
/// exactly 0 health, and the Gusanos death particles OLX then spawns draw
/// from the `FindSpot` stream (597, 544 and 669 draws here), so its respawn
/// position differs.
const D9: &str = "D9: Gusanos death particles' rnd() draws (not ported)";

/// `CHECKED` without `rc`, `mt` and `ents`: the second pass. Draw counters
/// come first in a frame, so a regression in health, the map, motion, shots
/// or projectiles in the frame of a pinned draw divergence above would hide
/// behind it. This pass pins the first divergence of the other fields too.
const WITHOUT_DRAWS: Fields = rl_scenario::CHECKED.difference(Fields::RAND);

/// Like [`EXPECTED_DIVERGENCES`], for the second pass ([`WITHOUT_DRAWS`]).
const EXPECTED_DIVERGENCES_WITHOUT_DRAWS: &[(&str, i64, &str, &str)] = &[
    ("weapons/mw-1-0/assassin-s-shot", 276, "w[1].x", D9),
    ("weapons/mw-1-0/flak", 325, "w[1].x", D9),
    ("weapons/mw-1-0/plasma-launcher", 283, "w[1].x", D9),
];

/// Checks one pass: a listed scenario must first diverge exactly as listed,
/// an unlisted one must match.
fn check(
    name: &str,
    divergence: Option<rl_oracle::Divergence>,
    table: (&str, &[(&str, i64, &str, &str)]),
    failures: &mut Vec<String>,
) {
    let listed = table.1.iter().find(|(n, ..)| *n == name);
    match (divergence, listed) {
        (Some(d), None) => failures.push(format!("{name}: {d} (not in {})", table.0)),
        (None, Some(_)) => failures.push(format!("{name}: matches now; remove it from {}", table.0)),
        (Some(d), Some(&(_, frame, path, owner))) if (d.frame, d.path.as_str()) != (frame, path) => failures
            .push(format!(
                "{name}: expected the first divergence at frame {frame}, {path} ({owner}), got {d}\n    \
                 if intended, in {}: (\"{name}\", {}, \"{}\", \"<owner>\"),",
                table.0, d.frame, d.path
            )),
        _ => {}
    }
}

#[test]
fn every_scenario_matches_olx_bit_for_bit() {
    let Some(content) = content_dir() else { return };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let names = rl_scenario::fixture_names(&dir).unwrap();
    for (n, ..) in EXPECTED_DIVERGENCES.iter().chain(EXPECTED_DIVERGENCES_WITHOUT_DRAWS) {
        assert!(names.iter().any(|m| m == n), "missing scenario {n}");
    }
    assert_eq!(names.iter().filter(|n| n.starts_with("weapons/")).count(), 38 + 50, "weapon scenarios");
    let mut failures = Vec::new();
    for name in names {
        let (sc, expected) = rl_scenario::load_fixture(&dir, &name).unwrap();
        let actual = rl_scenario::replay(Path::new(&content), &sc, &expected).unwrap();
        let tol = &sc.tolerances;
        let first = rl_oracle::compare_fields(&expected, &actual, tol, rl_scenario::CHECKED);
        check(&name, first, ("EXPECTED_DIVERGENCES", EXPECTED_DIVERGENCES), &mut failures);
        let second = rl_oracle::compare_fields(&expected, &actual, tol, WITHOUT_DRAWS);
        check(
            &name,
            second,
            ("EXPECTED_DIVERGENCES_WITHOUT_DRAWS", EXPECTED_DIVERGENCES_WITHOUT_DRAWS),
            &mut failures,
        );
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
