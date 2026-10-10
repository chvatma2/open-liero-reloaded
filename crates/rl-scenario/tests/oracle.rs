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
/// divergence. The list shrinks as M4 lands and is empty at its end.
///
/// The exact first divergence is the guard against hidden regressions: a
/// change that breaks an already ported field earlier in a listed scenario
/// moves its first divergence, and the test fails with the new pair. Every
/// pair here sits at a frame where OLX injures a worm (its blood draws) or
/// draws a beam or bonus (Tasks 7-9).
const EXPECTED_DIVERGENCES: &[(&str, i64, &str, &str)] = &[
    ("beam-burst", 2, "ents", "Task 8: DrawBeam's entities"),
    ("bleed", 2, "rc", "Task 7: bleeding"),
    ("bonus-health", 5, "ents", "Task 9: a placed bonus's entity"),
    ("bonus-weapon", 5, "ents", "Task 9: a placed bonus's entity"),
    ("die-before-delivery", 2, "rc", "Task 7: bleeding"),
    ("explode-kill-respawn", 61, "rc", "Task 7: an injury's blood"),
    ("two-worms-fire", 51, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/bazooka", 41, "rc", "Task 7: an explosion's blood"),
    ("weapons/classic/blaster", 52, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/bouncy-larpa", 39, "rc", "Task 7: an explosion's blood"),
    ("weapons/classic/bouncy-mine", 85, "rc", "Task 7: an explosion's blood"),
    ("weapons/classic/cannon", 44, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/chaingun", 35, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/chiquita-bomb", 237, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/cluster-bomb", 211, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/crackler", 50, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/dart", 39, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/doomsday", 46, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/flamer", 63, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/gauss-gun", 11, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/grenade", 185, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/handgun", 24, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/hellraider", 334, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/larpa", 29, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/laser", 2, "ents", "Task 8: DrawBeam's entities"),
    ("weapons/classic/mini-nuke", 117, "rc", "Task 7: an explosion's blood"),
    ("weapons/classic/mini-rockets", 46, "rc", "Task 7: an explosion's blood"),
    ("weapons/classic/minigun", 25, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/napalm", 57, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/rb-rampage", 44, "rc", "Task 7: an explosion's blood"),
    ("weapons/classic/rifle", 13, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/shotgun", 30, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/spikeballs", 72, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/super-shotgun", 30, "rc", "Task 7: an explosion's blood"),
    ("weapons/classic/uzi", 29, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/winchester", 13, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/zimm", 29, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/acid-gun", 39, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/anti-air-gun", 318, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/apache", 72, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/assassin-s-shot", 25, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/bazooka", 52, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/cluster-grenade", 115, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/demolition-grenade", 139, "rc", "Task 7: an explosion's blood"),
    ("weapons/mw-1-0/disease-bomb", 50, "rc", "Task 7: an explosion's blood"),
    ("weapons/mw-1-0/disease-grenade", 229, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/doomrockets", 29, "rc", "Task 7: an explosion's blood"),
    ("weapons/mw-1-0/energy-ball", 40, "rc", "Task 7: an explosion's blood"),
    ("weapons/mw-1-0/fireball", 39, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/flak", 35, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/flak-grenade", 113, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/flamer", 65, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/floating-bomber", 125, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/force-field", 31, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/hellfire", 35, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/inferno", 212, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/infest", 35, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/laser", 2, "ents", "Task 8: DrawBeam's entities"),
    ("weapons/mw-1-0/medical-camp", 361, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/mortar-launcher", 39, "rc", "Task 7: an explosion's blood"),
    ("weapons/mw-1-0/napalm-grenade", 216, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/nuclear-bomb", 22, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/nuclear-missile", 306, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/nuclear-waste", 47, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/nw-bomb", 163, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/petrol-bomb", 47, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/plasma-launcher", 17, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/poison-rifle", 17, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/ripper", 28, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/rocket-launcher", 43, "rc", "Task 7: an explosion's blood"),
    ("weapons/mw-1-0/shockrifle", 25, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/shockstrike", 42, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/smoking-bomb", 125, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/spikeballs", 31, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/sunray-beamer", 18, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/sunray-grenade", 118, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/surface-sweeper", 79, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/toxic-bomb", 310, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/toxic-strike", 39, "rc", "Task 7: an explosion's blood"),
    ("weapons/mw-1-0/toxic-waste", 90, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/twin-rockets", 40, "rc", "Task 7: an explosion's blood"),
    ("weapons/mw-1-0/twin-shotgun", 28, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/volcanic-gun", 25, "rc", "Task 7: an injury's blood"),
];

/// `CHECKED` without `rc`, `mt` and `ents`: the second pass. Most pinned
/// divergences above are an injury's blood draws (`rc`), which come first in
/// their frame, so a regression in health, the map, motion, shots or
/// projectiles in that very frame would hide behind them. This pass pins
/// the first divergence of the other fields too; at an injury frame they
/// all match (only a kill or a beam diverges there).
const WITHOUT_DRAWS: Fields = Fields::WORM_MOTION
    .union(Fields::SLOTS)
    .union(Fields::ROPE)
    .union(Fields::MATERIAL)
    .union(Fields::HEALTH)
    .union(Fields::SCORE)
    .union(Fields::SHOTS)
    .union(Fields::PROJECTILES)
    .union(Fields::PROJ_HIDDEN);

/// Like [`EXPECTED_DIVERGENCES`], for the second pass ([`WITHOUT_DRAWS`]).
const EXPECTED_DIVERGENCES_WITHOUT_DRAWS: &[(&str, i64, &str, &str)] = &[
    ("beam-burst", 40, "w[0].dirt", "Task 8: a beam's hole"),
    ("bonus-health", 140, "w[0].hp", "Task 9: a health bonus"),
    ("bonus-weapon", 129, "w[0].slots[0].id", "Task 9: a weapon bonus"),
    ("die-before-delivery", 64, "s[0].rnd", "Task 7: bleeding's draws"),
    ("explode-kill-respawn", 61, "w[0].kills", "Task 7: a kill"),
    ("two-worms-fire", 54, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/bazooka", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/blaster", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/bouncy-larpa", 46, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/bouncy-mine", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/cannon", 61, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/chaingun", 39, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/chiquita-bomb", 290, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/cluster-bomb", 290, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/crackler", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/dart", 46, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/doomsday", 53, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/flamer", 67, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/gauss-gun", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/grenade", 290, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/handgun", 32, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/larpa", 34, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/laser", 4, "w[1].hp", "Task 8: a beam's damage"),
    ("weapons/classic/mini-nuke", 118, "p[0].col", "Task 7: draws after an injury"),
    ("weapons/classic/mini-rockets", 52, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/minigun", 28, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/napalm", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/rb-rampage", 48, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/rifle", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/shotgun", 89, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/spikeballs", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/super-shotgun", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/uzi", 34, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/winchester", 56, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/classic/zimm", 101, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/acid-gun", 45, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/anti-air-gun", 325, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/apache", 102, "w[0].kills", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/assassin-s-shot", 26, "w[1].alive", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/bazooka", 53, "w[0].kills", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/cluster-grenade", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/demolition-grenade", 143, "p.len", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/disease-bomb", 101, "p.len", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/disease-grenade", 290, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/doomrockets", 43, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/energy-ball", 41, "p.len", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/fireball", 50, "p.len", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/flak", 39, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/flak-grenade", 113, "w[0].kills", "Task 7: a kill"),
    ("weapons/mw-1-0/flamer", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/floating-bomber", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/force-field", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/hellfire", 45, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/inferno", 290, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/infest", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/laser", 4, "w[1].hp", "Task 8: a beam's damage"),
    ("weapons/mw-1-0/mortar-launcher", 41, "p.len", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/napalm-grenade", 290, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/nuclear-bomb", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/nuclear-waste", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/nw-bomb", 290, "p[20].vx", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/petrol-bomb", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/plasma-launcher", 33, "p.len", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/poison-rifle", 39, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/ripper", 29, "p[2].col", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/rocket-launcher", 54, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/shockrifle", 30, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/shockstrike", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/smoking-bomb", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/spikeballs", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/sunray-beamer", 34, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/sunray-grenade", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/surface-sweeper", 80, "p.len", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/toxic-strike", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/toxic-waste", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/twin-rockets", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/twin-shotgun", 163, "s[0].rnd", "Task 7: draws after an injury"),
    ("weapons/mw-1-0/volcanic-gun", 28, "s[0].rnd", "Task 7: draws after an injury"),
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
