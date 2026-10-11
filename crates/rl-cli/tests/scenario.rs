//! `rl-cli scenario` on the real fixtures (needs `RL_CONTENT_DIR`).

use std::path::PathBuf;
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle")
}

fn scenario(content: &str, fixtures: &std::path::Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rl-cli"))
        .args(["scenario", "--content", content, "--fixtures"])
        .arg(fixtures)
        .args(extra)
        .output()
        .unwrap()
}

/// The imported content (`RL_CONTENT_DIR`, which `.cargo/config.toml` defaults
/// to `content/packs`), or `None` with a notice if it has not been imported.
fn content_dir() -> Option<String> {
    let dir = std::env::var("RL_CONTENT_DIR").ok()?;
    if std::path::Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        None
    }
}

#[test]
fn reports_matches_and_dumps_frames() {
    let Some(content) = content_dir() else { return };
    let out = scenario(&content, &fixtures(), &["fall", "--dump", "8"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("ok   fall: 201 frames match OLX"), "{text}");
    assert!(text.contains("olx {\"f\":8,") && text.contains("sim {\"f\":8,"), "{text}");
}

#[test]
fn reports_a_divergence_with_status_1() {
    let Some(content) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    std::fs::copy(fixtures().join("fall.ron"), tmp.path().join("fall.ron")).unwrap();
    let mut trace =
        rl_oracle::Trace::from_gz(&std::fs::read(fixtures().join("fall.jsonl.gz")).unwrap()).unwrap();
    trace.frames[50].w[0].vy = rl_oracle::Num(0.5);
    std::fs::write(tmp.path().join("fall.jsonl.gz"), rl_oracle::gzip(&trace.to_jsonl())).unwrap();
    let out = scenario(&content, tmp.path(), &[]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success());
    assert!(text.contains("DIFF fall: frame 49: w[0].vy expected 0.5"), "{text}");
}

/// `rl-cli scenario --time` (plan 8): every frame timed, with the work
/// done (projectiles, rand() calls) and whether it matched OLX.
#[test]
fn scenarios_are_timed_frame_by_frame() {
    let Some(dir) = content_dir() else { return };
    let content = rl_content::Content::open(std::path::Path::new(&dir)).unwrap();
    let (sc, trace) = rl_scenario::load_fixture(&fixtures(), "two-worms-fire").unwrap();
    let t = rl_cli::timing::time_scenario(&content, "two-worms-fire", &sc, &trace).unwrap();
    assert_eq!(t.tick_ns.len(), sc.frames as usize);
    assert_eq!((t.projectiles.len(), t.rand_calls.len()), (t.tick_ns.len(), t.tick_ns.len()));
    assert_eq!(t.matches_olx, Some(true));
    assert!(t.projectiles.iter().any(|&n| n > 10) && t.tick_ns.iter().all(|&ns| ns > 0));
    let mut header_only = trace.clone();
    header_only.frames.truncate(1);
    assert_eq!(rl_cli::timing::time_scenario(&content, "x", &sc, &header_only).unwrap().matches_olx, None);
}
