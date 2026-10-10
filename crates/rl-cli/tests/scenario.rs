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

#[test]
fn reports_matches_and_dumps_frames() {
    let Ok(content) = std::env::var("RL_CONTENT_DIR") else { return };
    let out = scenario(&content, &fixtures(), &["fall", "--dump", "8"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("ok   fall: 201 frames match OLX"), "{text}");
    assert!(text.contains("olx {\"f\":8,") && text.contains("sim {\"f\":8,"), "{text}");
}

#[test]
fn reports_a_divergence_with_status_1() {
    let Ok(content) = std::env::var("RL_CONTENT_DIR") else { return };
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
