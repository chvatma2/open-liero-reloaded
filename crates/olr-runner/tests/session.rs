use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use olr_runner::limit::Reset;
use olr_runner::session::{Outcome, SessionSpec, args, envs, run};

pub fn fake_claude() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/fake-claude.sh").display().to_string()
}

/// A temp working directory whose `fake.env` sets `mode` and `extra` lines.
fn workdir(mode: &str, extra: &str) -> (tempfile::TempDir, SessionSpec) {
    let tmp = tempfile::tempdir().unwrap();
    let result = tmp.path().join("result.md");
    std::fs::write(
        tmp.path().join("fake.env"),
        format!("FAKE_MODE={mode}\nFAKE_RESULT={}\n{extra}\n", result.display()),
    )
    .unwrap();
    let spec = SessionSpec {
        claude_bin: fake_claude(),
        config_dir: PathBuf::from("/cfg"),
        workdir: tmp.path().to_path_buf(),
        log: tmp.path().join("log.jsonl"),
        session_id: "1234".into(),
        prompt: Some("do it".into()),
        idle: Duration::from_secs(10),
        budget: Duration::from_secs(60),
        pid_file: None,
    };
    (tmp, spec)
}

#[test]
fn args_new_and_resume() {
    let (_tmp, mut spec) = workdir("done", "");
    let flags = [
        "--model",
        "opus",
        "--effort",
        "high",
        "--permission-mode",
        "bypassPermissions",
        "--output-format",
        "stream-json",
        "--verbose",
    ];
    let mut new = vec!["-p", "do it", "--session-id", "1234"];
    new.extend(flags);
    assert_eq!(args(&spec), new);
    spec.prompt = None;
    let mut resume = vec!["-p", "Continue the task where you left off.", "--resume", "1234"];
    resume.extend(flags);
    assert_eq!(args(&spec), resume);
}

#[test]
fn envs_exact() {
    let (_tmp, spec) = workdir("done", "");
    let pairs: Vec<(String, String)> = [
        ("CLAUDE_CONFIG_DIR", "/cfg"),
        ("CLAUDE_CODE_PROMPT_CACHE_TTL", "1h"),
        ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "400k"),
        ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    assert_eq!(envs(&spec), pairs);
}

#[test]
fn finished_done() {
    let (tmp, spec) = workdir("done", "");
    let (outcome, _) = run(&spec).unwrap();
    assert_eq!(outcome, Outcome::Finished { is_error: false, text: "Finished.".into() });
    assert!(std::fs::read_to_string(tmp.path().join("result.md")).unwrap().starts_with("done"));
}

#[test]
fn idle_watchdog_fires() {
    let (_tmp, mut spec) = workdir("silent", "");
    spec.idle = Duration::from_secs(1);
    let t = Instant::now();
    let (outcome, _) = run(&spec).unwrap();
    assert_eq!(outcome, Outcome::Idle);
    assert!(t.elapsed() < Duration::from_secs(5), "took {:?}", t.elapsed());
}

#[test]
fn a_finished_turn_outlives_the_watchdog() {
    let (tmp, mut spec) = workdir("linger", "");
    spec.idle = Duration::from_secs(1);
    let t = Instant::now();
    let (outcome, _) = run(&spec).unwrap();
    assert_eq!(outcome, Outcome::Finished { is_error: false, text: "Finished.".into() });
    assert!(t.elapsed() < Duration::from_secs(5), "took {:?}", t.elapsed());
    assert!(std::fs::read_to_string(tmp.path().join("result.md")).unwrap().starts_with("done"));
}

#[test]
fn ceiling_fires() {
    let (_tmp, mut spec) = workdir("silent", "");
    spec.budget = Duration::from_secs(1);
    let t = Instant::now();
    let (outcome, active) = run(&spec).unwrap();
    assert_eq!(outcome, Outcome::Ceiling);
    assert!(t.elapsed() < Duration::from_secs(5));
    assert!(active >= Duration::from_secs(1));
}

#[test]
fn limit_detected() {
    let (_tmp, spec) = workdir("limit-then-done", "");
    let (outcome, _) = run(&spec).unwrap();
    assert_eq!(outcome, Outcome::Limit(Reset::At { hour: 15, minute: 45 }));
}

#[test]
fn crash_reported() {
    let (_tmp, spec) = workdir("crash", "");
    match run(&spec).unwrap().0 {
        Outcome::Crashed(why) => assert!(why.contains("boom"), "{why}"),
        other => panic!("expected a crash, got {other:?}"),
    }
}

#[test]
fn log_has_every_line() {
    let (_tmp, spec) = workdir("done", "");
    run(&spec).unwrap();
    let log = std::fs::read_to_string(&spec.log).unwrap();
    assert_eq!(log.lines().count(), 3);
    assert!(log.lines().last().unwrap().contains(r#""type":"result""#));
}
