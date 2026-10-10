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

/// Scenarios the simulation cannot play yet: each must still diverge, and
/// first exactly at the `(frame, field)` listed (as `compare_fields` with
/// `CHECKED` reports it). The last column names the task that owns that
/// divergence. The list shrinks as M4 lands and is empty at its end.
///
/// The exact first divergence is the guard against hidden regressions: a
/// change that breaks an already ported field earlier in a listed scenario
/// moves its first divergence, and the test fails with the new pair. Every
/// pair here sits at a frame where OLX injures a worm, carves, places dirt
/// or draws a beam or bonus (all ported in Tasks 6-9).
const EXPECTED_DIVERGENCES: &[(&str, i64, &str, &str)] = &[
    ("beam-burst", 2, "ents", "Task 8: DrawBeam's entities"),
    ("bleed", 2, "rc", "Task 7: bleeding"),
    ("bonus-health", 5, "ents", "Task 9: a placed bonus's entity"),
    ("bonus-weapon", 5, "ents", "Task 9: a placed bonus's entity"),
    ("die-before-delivery", 2, "rc", "Task 7: bleeding"),
    ("explode-kill-respawn", 61, "rc", "Task 7: an injury's blood"),
    ("fire-minigun", 99, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("multi-shot", 47, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("reload-switch", 99, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("two-worms-fire", 40, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/bazooka", 41, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/classic/blaster", 35, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/bouncy-larpa", 39, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/classic/bouncy-mine", 85, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/classic/cannon", 37, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/chaingun", 35, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/chiquita-bomb", 218, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/cluster-bomb", 197, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/crackler", 17, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/dart", 39, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/dirt-ball", 117, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/doomsday", 46, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/explosives", 76, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/flamer", 46, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/gauss-gun", 11, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/grasshopper", 36, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/greenball", 41, "rc", "Task 6: green dirt's draws"),
    ("weapons/classic/grenade", 168, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/handgun", 24, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/hellraider", 36, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/larpa", 29, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/laser", 2, "ents", "Task 8: DrawBeam's entities"),
    ("weapons/classic/mini-nuke", 117, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/classic/mini-rockets", 46, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/classic/minigun", 25, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/napalm", 40, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/classic/rb-rampage", 44, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/classic/rifle", 13, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/shotgun", 30, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/spikeballs", 72, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/super-shotgun", 30, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/classic/uzi", 29, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/winchester", 13, "rc", "Task 7: an injury's blood"),
    ("weapons/classic/zimm", 29, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/acid-gun", 39, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/anti-air-gun", 318, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/apache", 72, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/assassin-s-shot", 25, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/bazooka", 52, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/cluster-grenade", 35, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/demolition-grenade", 139, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/mw-1-0/disease-bomb", 50, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/mw-1-0/disease-grenade", 39, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/doomrockets", 29, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/mw-1-0/energy-ball", 40, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/mw-1-0/fireball", 29, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/flak", 25, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/flak-grenade", 103, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/flamer", 38, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/floating-bomber", 95, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/force-field", 24, "rc", "Task 6: green dirt's draws"),
    ("weapons/mw-1-0/hellfire", 25, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/inferno", 156, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/infest", 35, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/laser", 2, "ents", "Task 8: DrawBeam's entities"),
    ("weapons/mw-1-0/medical-camp", 154, "rc", "Task 6: green dirt's draws"),
    ("weapons/mw-1-0/mortar-launcher", 39, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/mw-1-0/napalm-grenade", 106, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/nuclear-bomb", 22, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/nuclear-missile", 98, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/nuclear-waste", 47, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/nw-bomb", 163, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/petrol-bomb", 32, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/plasma-launcher", 17, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/poison-rifle", 17, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/ripper", 28, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/rocket-launcher", 43, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/mw-1-0/shockrifle", 25, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/shockstrike", 42, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/smoking-bomb", 57, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/spikeballs", 31, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/sunray-beamer", 18, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/sunray-grenade", 106, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/surface-sweeper", 47, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/toxic-bomb", 103, "w[0].dirt", "Task 6: a hole's dirt count"),
    ("weapons/mw-1-0/toxic-strike", 39, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/mw-1-0/toxic-waste", 90, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/twin-rockets", 40, "rc", "Tasks 6-7: an explosion that injures"),
    ("weapons/mw-1-0/twin-shotgun", 28, "rc", "Task 7: an injury's blood"),
    ("weapons/mw-1-0/volcanic-gun", 25, "rc", "Task 7: an injury's blood"),
];

#[test]
fn every_scenario_matches_olx_bit_for_bit() {
    let Some(content) = content_dir() else { return };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let names = rl_scenario::fixture_names(&dir).unwrap();
    for (n, ..) in EXPECTED_DIVERGENCES {
        assert!(names.iter().any(|m| m == n), "missing scenario {n}");
    }
    assert_eq!(names.iter().filter(|n| n.starts_with("weapons/")).count(), 38 + 50, "weapon scenarios");
    let mut failures = Vec::new();
    for name in names {
        let (sc, expected) = rl_scenario::load_fixture(&dir, &name).unwrap();
        let actual = rl_scenario::replay(Path::new(&content), &sc, &expected).unwrap();
        let divergence = rl_oracle::compare_fields(&expected, &actual, &sc.tolerances, rl_scenario::CHECKED);
        let listed = EXPECTED_DIVERGENCES.iter().find(|(n, ..)| *n == name);
        match (divergence, listed) {
            (Some(d), None) => failures.push(format!("{name}: {d}")),
            (None, Some(_)) => {
                failures.push(format!("{name}: matches now; remove it from EXPECTED_DIVERGENCES"))
            }
            (Some(d), Some(&(_, frame, path, owner))) if (d.frame, d.path.as_str()) != (frame, path) => {
                failures.push(format!(
                    "{name}: expected the first divergence at frame {frame}, {path} ({owner}), got {d}\n    \
                     if intended: (\"{name}\", {}, \"{}\", \"<owner>\"),",
                    d.frame, d.path
                ))
            }
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
