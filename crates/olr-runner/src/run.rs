//! Working issues: claim, session, gate, push and report, or hand back
//! (spec: "A run"). The run's phase is kept in `current.ron` before each
//! step with side effects, so an interrupted runner carries on from where
//! it was: a claim is handed back, a session resumed once, a gate run
//! again, a push only reported.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::gate::{self, DEFERRED_FILE};
use crate::github::GitHub;
use crate::limit::{self, Reset};
use crate::prompt;
use crate::queue::{self, Pick};
use crate::repo::Repo;
use crate::report::{self, Done, HandBack};
use crate::result::{self, Status};
use crate::session::{self, Outcome, SessionSpec};
use crate::settings::{self, Settings};
use crate::state::{Current, Phase, State};
use crate::trust;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunResult {
    Pushed { first: String, last: String },
    HandedBack(HandBack),
}

fn result_path(s: &Settings) -> PathBuf {
    s.workspace.join("target/olr-runner/result.md")
}

fn notes_path(s: &Settings) -> PathBuf {
    s.workspace.join("target/olr-runner/notes.md")
}

/// Works approved issue `number` from the start.
pub fn run_issue(gh: &dyn GitHub, s: &Settings, st: &State, number: u64) -> Result<RunResult> {
    let detail = gh.issue(number)?;
    let mut current = Current {
        issue: number,
        session_id: session::new_session_id()?,
        base: String::new(),
        active_secs: 0,
        log: st.new_log(number),
        phase: Phase::Claimed,
        resumes: 0,
    };
    st.set_current(Some(&current))?;
    gh.edit_labels(number, &["in-progress"], &["approved"])?;
    let task = match trust::classify(&detail, &s.maintainer) {
        Ok(task) => task,
        Err(refusal) => return hand_back(gh, s, st, number, HandBack::Untrusted(refusal), &current.log),
    };
    let repo = Repo::prepare(&s.workspace, &s.remote_url)?;
    for stale in [result_path(s), notes_path(s)] {
        if stale.exists() {
            std::fs::remove_file(stale)?;
        }
    }
    std::fs::create_dir_all(s.workspace.join("target/olr-runner"))?;
    current.base = repo.head()?;
    current.phase = Phase::Session;
    st.set_current(Some(&current))?;
    let prompt = prompt::render(&task, &result_path(s), &notes_path(s));
    continue_run(gh, s, st, current, Some(prompt))
}

/// Carries `current` on from its phase: the session (with no prompt, a
/// resume), the gate and push, the report.
fn continue_run(
    gh: &dyn GitHub,
    s: &Settings,
    st: &State,
    mut current: Current,
    prompt: Option<String>,
) -> Result<RunResult> {
    let (n, log) = (current.issue, current.log.clone());
    let back = |h: HandBack| hand_back(gh, s, st, n, h, &log);
    if current.phase == Phase::Session {
        match session_until_done(s, st, &mut current, prompt)? {
            Outcome::Finished { is_error: true, text } if !result_path(s).exists() => {
                st.set_paused(true)?;
                return back(HandBack::Infrastructure(text));
            }
            Outcome::Finished { .. } => {}
            Outcome::Idle => return back(HandBack::Watchdog("no activity for the idle limit")),
            Outcome::Ceiling => return back(HandBack::Watchdog("the session ran past its time ceiling")),
            Outcome::Crashed(why) => {
                st.set_paused(true)?;
                return back(HandBack::Infrastructure(why));
            }
            Outcome::Limit(_) => unreachable!("limits are waited out"),
        }
        let result = match std::fs::read_to_string(result_path(s)) {
            Ok(text) => match result::parse(&text) {
                Ok(r) => r,
                Err(e) => return back(HandBack::NoResult(e.to_string())),
            },
            Err(_) => return back(HandBack::NoResult("no result file".into())),
        };
        if result.status == Status::Blocked {
            return back(HandBack::Blocked(result.text));
        }
        current.phase = Phase::Gate { summary: result.text };
        st.set_current(Some(&current))?;
    }
    let done = match current.phase.clone() {
        Phase::Pushed(done) => done,
        Phase::Gate { summary } => match gate_and_push(s, st, &mut current, summary)? {
            Ok(done) => done,
            Err(h) => return back(h),
        },
        Phase::Claimed | Phase::Session => unreachable!("handled above"),
    };
    gh.comment(n, &report::done_comment(&done))?;
    gh.close(n)?;
    let add: &[&str] = if done.deferred || done.render_unverified { &["needs-bench"] } else { &[] };
    gh.edit_labels(n, add, &["in-progress"])?;
    st.record(&format!("#{n} pushed {}..{}", done.first, done.last))?;
    st.set_current(None)?;
    Ok(RunResult::Pushed { first: done.first, last: done.last })
}

/// Checks the session's commits, rebases them onto a moved `main`, pushes
/// them, and records the push before anything is reported.
fn gate_and_push(
    s: &Settings,
    st: &State,
    current: &mut Current,
    summary: String,
) -> Result<Result<Done, HandBack>> {
    let repo = Repo { dir: s.workspace.clone() };
    let base = current.base.clone();
    let files = match gate_commits(&repo, s, &base, current.issue)? {
        Ok(files) => files,
        Err(h) => return Ok(Err(h)),
    };
    let mut checked = run_checks(&repo, s, &files)?;
    if checked.failed.is_none() {
        repo.fetch()?;
        if repo.origin_main()? != base {
            if !repo.rebase_onto_origin_main()? {
                return Ok(Err(HandBack::RebaseConflict));
            }
            checked = run_checks(&repo, s, &files)?;
        }
    }
    if let Some(tail) = checked.failed {
        let main = Repo::prepare(&s.workspace, &s.remote_url)?;
        if !main.run(&wrapped(s, &s.check_cmd))?.0 {
            st.set_paused(true)?;
            return Ok(Err(HandBack::MainBroken));
        }
        return Ok(Err(HandBack::CheckFailed(tail)));
    }
    let origin = repo.origin_main()?;
    let ids = repo.commit_ids(&origin, &repo.head()?)?;
    let (Some(first), Some(last)) = (ids.first().cloned(), ids.last().cloned()) else {
        return Ok(Err(HandBack::NoCommits));
    };
    let lock_before = repo.show(&origin, "Cargo.lock")?;
    let lock_after = repo.show("HEAD", "Cargo.lock")?;
    if let Err(e) = repo.push_main() {
        return Ok(Err(HandBack::PushFailed(format!("{e:#}"))));
    }
    let done = Done {
        summary,
        first,
        last,
        commits: ids.len(),
        claude_md: gate::claude_md_edits(&files),
        new_packages: gate::new_packages(&lock_before, &lock_after),
        deferred: files.iter().any(|f| f == DEFERRED_FILE),
        render_unverified: checked.render_skipped,
    };
    current.phase = Phase::Pushed(done.clone());
    st.set_current(Some(current))?;
    Ok(Ok(done))
}

/// Runs the session, waiting out usage limits and resuming after them.
fn session_until_done(
    s: &Settings,
    st: &State,
    current: &mut Current,
    mut prompt: Option<String>,
) -> Result<Outcome> {
    loop {
        let spec = SessionSpec {
            claude_bin: s.claude_bin.clone(),
            config_dir: s.claude_config_dir.clone(),
            workdir: s.workspace.clone(),
            log: current.log.clone(),
            session_id: current.session_id.clone(),
            prompt: prompt.take(),
            idle: Duration::from_secs(s.idle_secs),
            budget: Duration::from_secs(s.ceiling_secs.saturating_sub(current.active_secs)),
            pid_file: Some(st.session_pid()),
        };
        let (outcome, active) = session::run(&spec)?;
        current.active_secs += active.as_secs();
        st.set_current(Some(current))?;
        let Outcome::Limit(reset) = outcome else { return Ok(outcome) };
        let wait = match reset {
            Reset::At { hour, minute } => limit::seconds_until(local_time()?, (hour, minute)),
            Reset::Unknown => s.limit_fallback_secs,
        };
        st.record(&format!("#{} usage limit: waiting {wait} s", current.issue))?;
        std::thread::sleep(Duration::from_secs(wait));
    }
}

/// The local wall-clock time as (hour, minute, second).
fn local_time() -> Result<(u32, u32, u32)> {
    let out = Command::new("date").arg("+%H %M %S").output().context("cannot run date")?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace().map(|p| p.parse::<u32>().unwrap_or_default());
    Ok((parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), parts.next().unwrap_or_default()))
}

/// The session's commits: clean, on the `main` it started from, present,
/// numbered and off protected paths. Returns the changed files.
fn gate_commits(repo: &Repo, s: &Settings, base: &str, n: u64) -> Result<Result<Vec<String>, HandBack>> {
    let dirty = repo.uncommitted()?;
    if !dirty.is_empty() {
        return Ok(Err(HandBack::DirtyTree(dirty)));
    }
    if !repo.is_ancestor(base)? {
        return Ok(Err(HandBack::NotOnMain));
    }
    let head = repo.head()?;
    let subjects = repo.subjects(base, &head)?;
    if subjects.is_empty() {
        return Ok(Err(HandBack::NoCommits));
    }
    let bad = gate::bad_subjects(&subjects, n);
    if !bad.is_empty() {
        return Ok(Err(HandBack::BadSubjects(bad)));
    }
    let files = repo.changed_files(base, &head)?;
    let hits = gate::protected_hits(&files, &s.protected);
    if !hits.is_empty() {
        return Ok(Err(HandBack::Protected(hits)));
    }
    Ok(Ok(files))
}

/// A check's verdict: the failing output's tail, and whether a rendering
/// change's goldens went unchecked (no graphics adapter).
struct Checked {
    failed: Option<String>,
    render_skipped: bool,
}

/// `cmd` as the gate runs it: sandboxed unless the settings say not.
fn wrapped(s: &Settings, cmd: &[String]) -> Vec<String> {
    if s.gate_sandbox { gate::sandboxed(cmd, &s.workspace, &settings::home()) } else { cmd.to_vec() }
}

/// The check, or for a rendering change the render check (which includes it).
fn run_checks(repo: &Repo, s: &Settings, files: &[String]) -> Result<Checked> {
    let render = gate::touches_render(files);
    let cmd = if render { &s.render_check_cmd } else { &s.check_cmd };
    let (ok, tail) = repo.run(&wrapped(s, cmd))?;
    Ok(Checked { render_skipped: render && tail.contains("skip render"), failed: (!ok).then_some(tail) })
}

fn hand_back(
    gh: &dyn GitHub,
    s: &Settings,
    st: &State,
    n: u64,
    h: HandBack,
    log: &Path,
) -> Result<RunResult> {
    gh.comment(n, &report::handback_comment(&h, log))?;
    // Remove only the labels it has (an interrupted claim may not have run).
    let present = gh.issue(n)?.labels;
    let remove: Vec<&str> =
        ["in-progress", "approved"].into_iter().filter(|l| present.iter().any(|p| p == l)).collect();
    gh.edit_labels(n, &["needs-maintainer"], &remove)?;
    // Reset the workspace; a failure here is repaired by the next run's prepare.
    let _ = Repo::prepare(&s.workspace, &s.remote_url);
    st.record(&format!("#{n} handed back: {}", h.describe().lines().next().unwrap_or_default()))?;
    st.set_current(None)?;
    Ok(RunResult::HandedBack(h))
}

/// Carries an interrupted run on: a claim goes back, a session is resumed
/// once (after stopping any session the runner left behind), a gate runs
/// again, a push is reported.
fn recover(gh: &dyn GitHub, s: &Settings, st: &State, mut current: Current) -> Result<RunResult> {
    let (n, log) = (current.issue, current.log.clone());
    session::stop_left_over(&st.session_pid());
    match current.phase {
        Phase::Claimed => hand_back(gh, s, st, n, HandBack::Interrupted("before its session started"), &log),
        Phase::Session if current.resumes >= 1 => {
            hand_back(gh, s, st, n, HandBack::Interrupted("twice during its session"), &log)
        }
        Phase::Session => {
            current.resumes += 1;
            st.set_current(Some(&current))?;
            continue_run(gh, s, st, current, None)
        }
        Phase::Gate { .. } | Phase::Pushed(_) => continue_run(gh, s, st, current, None),
    }
}

/// What the next poll would do, with each candidate's approval time.
pub fn plan_next(gh: &dyn GitHub) -> Result<Pick> {
    let milestones = gh.open_milestones()?;
    let [milestone] = milestones.as_slice() else { return Ok(queue::pick(&milestones, &[], &[])) };
    let mut candidates = Vec::new();
    for issue in gh.approved_issues(milestone.number)? {
        let at = if queue::eligible(&issue.labels) {
            trust::approved_at(&gh.issue(issue.number)?).map(|e| e.at.clone())
        } else {
            None
        };
        candidates.push((issue, at));
    }
    let handed_back: Vec<u64> = gh.handed_back_issues(milestone.number)?.iter().map(|i| i.number).collect();
    Ok(queue::pick(&milestones, &candidates, &handed_back))
}

fn describe(pick: &Pick) -> String {
    match pick {
        Pick::NoMilestone => "idle: no open milestone".into(),
        Pick::ManyMilestones(titles) => format!("idle: more than one open milestone ({})", titles.join(", ")),
        Pick::Empty { milestone } => format!("idle: no approved issue in {milestone}"),
        Pick::Next { milestone, number } => format!("next: #{number} in {milestone}"),
        Pick::Held { milestone, number } => format!("held: #{number} in {milestone} needs the maintainer"),
    }
}

/// One poll-and-work pass: finish a crashed run, then work the queue until
/// it is empty or the runner is paused.
pub fn tick(gh: &dyn GitHub, s: &Settings, st: &State) -> Result<()> {
    let Some(_lock) = st.lock()? else { return Ok(()) };
    if let Some(current) = st.current() {
        st.record(&format!("#{} carrying on after an interrupted run", current.issue))?;
        recover(gh, s, st, current)?;
    }
    while !st.paused() {
        let pick = plan_next(gh)?;
        let Pick::Next { number, .. } = pick else {
            st.record(&describe(&pick))?;
            return Ok(());
        };
        run_issue(gh, s, st, number)?;
    }
    Ok(())
}

/// Works one approved issue now (`olr-runner run <n>`), under the lock.
/// An interrupted run comes first, and an issue not approved is refused
/// untouched (the runner never triages).
pub fn run_now(gh: &dyn GitHub, s: &Settings, st: &State, number: u64) -> Result<RunResult> {
    let Some(_lock) = st.lock()? else { anyhow::bail!("another runner holds the lock") };
    if let Some(current) = st.current() {
        anyhow::bail!(
            "the interrupted run of #{} comes first: `olr-runner tick` carries it on",
            current.issue
        );
    }
    if !queue::eligible(&gh.issue(number)?.labels) {
        anyhow::bail!("#{number} is not approved (or is in progress or handed back)");
    }
    run_issue(gh, s, st, number)
}

pub fn status(gh: &dyn GitHub, st: &State) -> Result<String> {
    let mut out = String::new();
    out += &format!("paused: {}\n", if st.paused() { "yes" } else { "no" });
    let busy = st.lock()?.is_none();
    out += &match st.current() {
        Some(c) if busy => format!("working: #{} ({} s of session time)\n", c.issue, c.active_secs),
        Some(c) => format!("interrupted run: #{} (the next tick resumes it)\n", c.issue),
        None if busy => "working: (starting)\n".into(),
        None => "working: nothing\n".into(),
    };
    out += &format!("queue: {}\n", describe(&plan_next(gh)?));
    out += "history:\n";
    for line in st.history(5) {
        out += &format!("  {line}\n");
    }
    Ok(out)
}

/// What the next run would send, with no side effects.
pub fn dry_run(gh: &dyn GitHub, s: &Settings) -> Result<String> {
    let pick = plan_next(gh)?;
    let Pick::Next { number, .. } = pick else { return Ok(describe(&pick)) };
    let detail = gh.issue(number)?;
    Ok(match trust::classify(&detail, &s.maintainer) {
        Ok(task) => {
            format!("{}\n\n{}", describe(&pick), prompt::render(&task, &result_path(s), &notes_path(s)))
        }
        Err(r) => format!("{}: would be handed back: {}", describe(&pick), r.describe()),
    })
}
