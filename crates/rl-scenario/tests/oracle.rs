//! M3 acceptance (spec §6, §10): every scenario in `fixtures/oracle`, replayed
//! in the Rust simulation, matches OLX's recorded trace bit for bit.
//! Needs the imported OLX content; `cargo xtask check` sets `RL_CONTENT_DIR`.

use std::path::{Path, PathBuf};

#[test]
fn every_scenario_matches_olx_bit_for_bit() {
    let Ok(content) = std::env::var("RL_CONTENT_DIR") else {
        eprintln!("RL_CONTENT_DIR not set; skipping the oracle comparison");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let names = rl_scenario::fixture_names(&dir).unwrap();
    assert_eq!(names, ["aim", "dig", "fall", "jump", "rope", "tap", "walk"]);
    let mut failures = Vec::new();
    for name in names {
        let (sc, expected) = rl_scenario::load_fixture(&dir, &name).unwrap();
        let actual = rl_scenario::replay(Path::new(&content), &sc, &expected).unwrap();
        if let Err(d) = rl_oracle::compare(&expected, &actual, &sc.tolerances) {
            failures.push(format!("{name}: {d}"));
        }
    }
    assert!(failures.is_empty(), "diverged from OLX:\n{}", failures.join("\n"));
}

#[test]
fn a_changed_trace_is_detected() {
    let Ok(content) = std::env::var("RL_CONTENT_DIR") else { return };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let (sc, mut expected) = rl_scenario::load_fixture(&dir, "walk").unwrap();
    let actual = rl_scenario::replay(Path::new(&content), &sc, &expected).unwrap();
    expected.frames[200].w[0].x = rl_oracle::Num(expected.frames[200].w[0].x.0 + 1.0);
    let d = rl_oracle::compare(&expected, &actual, &[]).unwrap_err();
    assert_eq!((d.frame, d.path.as_str()), (199, "w[0].x"));
}
