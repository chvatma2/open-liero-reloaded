use std::path::{Path, PathBuf};

use olr_runner::github::{IssueDetail, LabelEvent, Milestone};
use olr_runner::repo::Repo;
use olr_runner::run::{RunResult, run_issue, tick};
use olr_runner::settings::Settings;
use olr_runner::state::{Current, State};

use crate::fake_github::FakeGitHub;
use crate::repo::{git, origin};
use crate::session::fake_claude;

const M: &str = "chvatma2";

struct Env {
    _tmp: tempfile::TempDir,
    s: Settings,
    st: State,
    gh: FakeGitHub,
    scratch: PathBuf,
}

/// An approved issue `n` by the maintainer.
fn approved(n: u64) -> IssueDetail {
    IssueDetail {
        number: n,
        title: format!("Issue {n}"),
        body: "Add the thing.".into(),
        author: M.into(),
        editor: None,
        last_edited_at: None,
        labels: vec!["approved".into()],
        comments: vec![],
        label_events: vec![LabelEvent {
            actor: M.into(),
            label: "approved".into(),
            at: format!("2026-10-20T10:{n:02}:00Z"),
            added: true,
        }],
        renames: vec![],
    }
}

/// A prepared workspace on a temp origin, one open milestone, issue 7
/// approved, and the fake session configured by `fake_env` lines.
fn env(fake_env: &str) -> Env {
    let (tmp, url, scratch) = origin();
    let s = Settings {
        remote_url: url.clone(),
        idle_secs: 5,
        ceiling_secs: 60,
        limit_fallback_secs: 1,
        claude_bin: fake_claude(),
        check_cmd: vec!["true".into()],
        render_check_cmd: vec!["true".into()],
        workspace: tmp.path().join("work"),
        state_dir: tmp.path().join("state"),
        claude_config_dir: tmp.path().join("cfg"),
        ..Settings::default()
    };
    Repo::prepare(&s.workspace, &url).unwrap();
    set_fake(&s, fake_env);
    let st = State::new(&s.state_dir).unwrap();
    let gh = FakeGitHub::default();
    gh.milestones.borrow_mut().push(Milestone { number: 1, title: "0.2.0".into() });
    gh.details.borrow_mut().insert(7, approved(7));
    Env { _tmp: tmp, s, st, gh, scratch }
}

fn set_fake(s: &Settings, lines: &str) {
    let result = s.workspace.join("target/olr-runner/result.md");
    std::fs::create_dir_all(s.workspace.join("target")).unwrap();
    std::fs::write(
        s.workspace.join("target/fake.env"),
        format!("FAKE_RESULT={}\n{lines}\n", result.display()),
    )
    .unwrap();
}

fn origin_has(e: &Env, file: &str) -> bool {
    git(&e.scratch, &["pull", "-q", "origin", "main"]);
    e.scratch.join(file).exists()
}

fn handed_back(e: &Env, n: u64) -> bool {
    e.gh.labels(n).contains(&"needs-maintainer".to_string())
        && e.gh.logged().iter().any(|l| l.starts_with(&format!("comment {n} ")) && l.contains("Handed back"))
}

const COMMIT: &str = "FAKE_MODE=commit\nFAKE_FILE=src/new.rs\nFAKE_SUBJECT='olr: the thing (#7)'";

#[test]
fn approved_issue_is_pushed_and_closed() {
    let e = env(COMMIT);
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(origin_has(&e, "src/new.rs"));
    let log = e.gh.logged();
    assert_eq!(log[0], "labels 7 +in-progress -approved");
    assert!(log.iter().any(|l| l.starts_with("comment 7 ") && l.contains("git revert --no-edit")), "{log:?}");
    assert!(log.contains(&"close 7".to_string()));
    assert!(!e.gh.labels(7).contains(&"in-progress".to_string()));
    assert!(e.st.current().is_none());
    assert!(e.st.history(5).iter().any(|h| h.contains("#7 pushed")), "{:?}", e.st.history(5));
}

#[test]
fn blocked_hands_back() {
    let e = env("FAKE_MODE=done\nFAKE_STATUS=blocked");
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(handed_back(&e, 7));
    assert!(!origin_has(&e, "src/new.rs"));
    assert!(e.st.current().is_none());
}

#[test]
fn protected_path_hands_back() {
    let e = env("FAKE_MODE=commit\nFAKE_FILE=.github/x.yml\nFAKE_SUBJECT='ci (#7)'");
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(handed_back(&e, 7));
    assert!(!origin_has(&e, ".github/x.yml"));
}

#[test]
fn bad_subject_hands_back() {
    let e = env("FAKE_MODE=commit\nFAKE_FILE=src/new.rs\nFAKE_SUBJECT='no number'");
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(handed_back(&e, 7));
    assert!(!origin_has(&e, "src/new.rs"));
}

#[test]
fn failed_check_hands_back() {
    let mut e = env(COMMIT);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/check-fails-off-main.sh");
    e.s.check_cmd = vec!["bash".into(), script.display().to_string()];
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(handed_back(&e, 7));
    assert!(e.gh.logged().iter().any(|l| l.contains("`cargo xtask check` failed on the change")));
    assert!(!e.st.paused());
    assert!(!origin_has(&e, "src/new.rs"));
}

#[test]
fn broken_main_pauses() {
    let mut e = env(COMMIT);
    e.s.check_cmd = vec!["false".into()];
    e.gh.details.borrow_mut().insert(8, approved(8));
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(handed_back(&e, 7));
    assert!(e.gh.logged().iter().any(|l| l.contains("fails on `main` itself")));
    assert!(e.st.paused());
    assert!(e.gh.labels(8).contains(&"approved".to_string()), "paused: issue 8 is not taken");
}

#[test]
fn rebase_conflict_hands_back() {
    let e = env("");
    let before = format!(
        "git -C {0} pull -q origin main; echo theirs > {0}/README; git -C {0} -c user.name=t -c user.email=t@t commit -qam theirs; git -C {0} push -q origin main",
        e.scratch.display()
    );
    set_fake(
        &e.s,
        &format!("FAKE_MODE=commit\nFAKE_FILE=README\nFAKE_SUBJECT='mine (#7)'\nFAKE_BEFORE='{before}'"),
    );
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(handed_back(&e, 7));
    assert!(e.gh.logged().iter().any(|l| l.contains("no longer rebases cleanly")));
    git(&e.scratch, &["pull", "-q", "origin", "main"]);
    assert_eq!(std::fs::read_to_string(e.scratch.join("README")).unwrap().trim(), "theirs");
    assert!(Repo { dir: e.s.workspace.clone() }.is_clean().unwrap());
}

#[test]
fn limit_waits_and_resumes() {
    let e = env(
        "FAKE_MODE=limit-then-done\nFAKE_LIMIT_TEXT=\"You've hit your usage limit\"\nFAKE_STATUS=blocked",
    );
    let r = run_issue(&e.gh, &e.s, &e.st, 7).unwrap();
    assert!(matches!(r, RunResult::HandedBack(_)), "the resumed session reported blocked");
    let calls = std::fs::read_to_string(e.s.workspace.join("target/fake.calls")).unwrap();
    assert_eq!(calls.lines().count(), 2);
    assert!(calls.lines().nth(1).unwrap().contains("--resume"));
}

#[test]
fn crash_resumes_before_new_work() {
    let e = env(COMMIT);
    e.gh.details.borrow_mut().get_mut(&7).unwrap().labels = vec!["in-progress".into()];
    e.gh.details.borrow_mut().insert(8, approved(8));
    let repo = Repo { dir: e.s.workspace.clone() };
    e.st.set_current(Some(&Current {
        issue: 7,
        session_id: "abc".into(),
        base: repo.head().unwrap(),
        active_secs: 0,
        log: e.st.new_log(7),
    }))
    .unwrap();
    tick(&e.gh, &e.s, &e.st).unwrap();
    let log = e.gh.logged();
    let closed7 = log.iter().position(|l| l == "close 7").expect("7 finished");
    let claimed8 = log.iter().position(|l| l == "labels 8 +in-progress -approved").expect("8 taken");
    assert!(closed7 < claimed8, "{log:?}");
    let calls = std::fs::read_to_string(e.s.workspace.join("target/fake.calls")).unwrap();
    assert!(calls.lines().next().unwrap().contains("--resume abc"));
}

#[test]
fn paused_does_nothing() {
    let e = env(COMMIT);
    e.st.set_paused(true).unwrap();
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(e.gh.logged().is_empty());
}

#[test]
fn two_milestones_do_nothing() {
    let e = env(COMMIT);
    e.gh.milestones.borrow_mut().push(Milestone { number: 2, title: "0.3.0".into() });
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(e.gh.logged().is_empty());
    assert!(e.st.history(1)[0].contains("0.2.0, 0.3.0"), "{:?}", e.st.history(1));
}

#[test]
fn github_down_changes_nothing() {
    let e = env(COMMIT);
    e.gh.fail.set(true);
    assert!(tick(&e.gh, &e.s, &e.st).is_err());
    assert!(e.gh.logged().is_empty());
    assert_eq!(e.gh.labels(7), ["approved"]);
}

#[test]
fn untrusted_refusal_hands_back_without_session() {
    let e = env(COMMIT);
    e.gh.details.borrow_mut().get_mut(&7).unwrap().author = "reporter".into();
    tick(&e.gh, &e.s, &e.st).unwrap();
    assert!(handed_back(&e, 7));
    assert!(e.gh.logged().iter().any(|l| l.contains("nothing the maintainer wrote")));
    assert!(!e.s.workspace.join("target/fake.calls").exists(), "no session ran");
}
