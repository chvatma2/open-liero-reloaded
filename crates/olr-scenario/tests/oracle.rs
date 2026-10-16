//! Acceptance (spec §6, §10): every scenario in `fixtures/oracle`, replayed in
//! the Rust simulation, matches OLX's recorded trace bit for bit in every field
//! the trace records (`Fields::ALL`), except the three pinned D9 scenarios below.
//! Needs the imported OLX content: `OLR_CONTENT_DIR`, which `.cargo/config.toml`
//! sets to `content/packs` unless the environment already does.

use std::path::{Path, PathBuf};

use olr_oracle::Fields;

/// The imported content (`OLR_CONTENT_DIR`, which `.cargo/config.toml` defaults
/// to `content/packs`), or `None` with a notice if it has not been imported.
fn content_dir() -> Option<String> {
    let dir = std::env::var("OLR_CONTENT_DIR").ok()?;
    if std::path::Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        None
    }
}

/// Scenarios whose bit-exact match is impossible without porting something
/// the simulation deliberately leaves out. Each must still diverge, and first
/// exactly at the `(frame, field)` listed (as `compare_fields` with `CHECKED`
/// reports it); the last column is the reason. Every other scenario must match
/// in every field.
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

/// Why the rows above cannot match (spec §18 D9, `docs/porting/STATUS.md`).
/// The dummy's health reaches exactly 0, which takes OLX's Gusanos `think()`
/// death path. Its Gusanos particles draw from the `FindSpot` generator
/// (597, 544 and 669 `rnd()` draws in these scenarios), and that particle
/// system is out of scope, so the simulation's generator stays behind. The
/// worm's death, its credit and everything else match; only the next random
/// respawn position differs. Reproducing it bit for bit would need the
/// Gusanos particle system ported.
const D9: &str = "D9: exact-zero health; OLX's Gusanos death particles consume the FindSpot rnd() \
                  stream and are not ported, so the next random respawn spot differs";

/// `CHECKED` without `rc`, `mt` and `ents`: the second pass. Draw counters
/// come first in a frame, so a regression in health, the map, motion, shots
/// or projectiles in the frame of a pinned draw divergence above would hide
/// behind it. This pass pins the first divergence of the other fields too.
const WITHOUT_DRAWS: Fields = olr_scenario::CHECKED.difference(Fields::RAND);

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
    divergence: Option<olr_oracle::Divergence>,
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
    assert_eq!(olr_scenario::CHECKED, Fields::ALL, "every trace field must be compared");
    let Some(content) = content_dir() else { return };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let names = olr_scenario::fixture_names(&dir).unwrap();
    for (n, ..) in EXPECTED_DIVERGENCES.iter().chain(EXPECTED_DIVERGENCES_WITHOUT_DRAWS) {
        assert!(names.iter().any(|m| m == n), "missing scenario {n}");
    }
    assert_eq!(names.iter().filter(|n| n.starts_with("weapons/")).count(), 38 + 50, "weapon scenarios");
    let content = olr_content::Content::open(Path::new(&content)).unwrap();
    let mut failures = Vec::new();
    for name in names {
        let (sc, expected) = olr_scenario::load_fixture(&dir, &name).unwrap();
        let actual = olr_scenario::replay(&content, &sc, &expected).unwrap();
        let tol = &sc.tolerances;
        let first = olr_oracle::compare_fields(&expected, &actual, tol, olr_scenario::CHECKED);
        check(&name, first, ("EXPECTED_DIVERGENCES", EXPECTED_DIVERGENCES), &mut failures);
        let second = olr_oracle::compare_fields(&expected, &actual, tol, WITHOUT_DRAWS);
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
    let (sc, mut expected) = olr_scenario::load_fixture(&dir, "walk").unwrap();
    let content = olr_content::Content::open(Path::new(&content)).unwrap();
    let actual = olr_scenario::replay(&content, &sc, &expected).unwrap();
    expected.frames[200].w[0].x = olr_oracle::Num(expected.frames[200].w[0].x.0 + 1.0);
    let d = olr_oracle::compare_fields(&expected, &actual, &[], olr_scenario::CHECKED).unwrap();
    assert_eq!((d.frame, d.path.as_str()), (199, "w[0].x"));
}

/// Natural weapon bonuses hold only weapons OLX's default restriction file
/// (`Standard 100lt`) does not ban (spec §20 F4): `bonus-natural-default`
/// records OLX with the default, and the simulation must reproduce it.
#[test]
fn natural_bonuses_use_default_restrictions() {
    let Some(content) = content_dir() else { return };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let (sc, expected) = olr_scenario::load_fixture(&dir, "bonus-natural-default").unwrap();
    assert_eq!(
        expected.header.features.get("WeaponRestrictionsFile").map(String::as_str),
        Some("Standard 100lt")
    );
    let content = olr_content::Content::open(Path::new(&content)).unwrap();
    let rules = content.weapon_rules(&olr_content::olx_mod_id(&sc.mod_dir), "Standard 100lt").unwrap();
    let allowed = rules.bonus_weapons();
    assert_eq!(allowed.len(), 12, "Classic's Standard 100lt allows 12 of 38 weapons");
    // Every spawned bonus draws its weapon from the allowed ones, and the
    // scenario spawns enough weapon bonuses to tell the lists apart.
    let mut spawned = std::collections::BTreeSet::new();
    for f in &expected.frames {
        for b in &f.sb {
            assert!(allowed.contains(&b.wpn), "frame {}: bonus weapon {} is banned", f.f, b.wpn);
            spawned.insert((b.spawn, b.t));
        }
    }
    assert!(spawned.iter().filter(|(_, t)| *t == 0).count() >= 5, "{spawned:?}");
    let actual = olr_scenario::replay(&content, &sc, &expected).unwrap();
    let d = olr_oracle::compare_fields(&expected, &actual, &sc.tolerances, olr_scenario::CHECKED);
    assert!(d.is_none(), "diverged from OLX: {}", d.unwrap());
}
