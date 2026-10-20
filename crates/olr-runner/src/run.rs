//! Working issues: claim, session, gate, push and report, or hand back
//! (spec: "A run").

use std::path::PathBuf;
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
use crate::settings::Settings;
use crate::state::{Current, State};
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
    gh.edit_labels(number, &["in-progress"], &["approved"])?;
    let log = st.new_log(number);
    let task = match trust::classify(&detail, &s.maintainer) {
        Ok(task) => task,
        Err(refusal) => return hand_back(gh, s, st, number, HandBack::Untrusted(refusal), &log),
    };
    let repo = Repo::prepare(&s.workspace, &s.remote_url)?;
    for stale in [result_path(s), notes_path(s)] {
        if stale.exists() {
            std::fs::remove_file(stale)?;
        }
    }
    std::fs::create_dir_all(s.workspace.join("target/olr-runner"))?;
    let current = Current {
        issue: number,
        session_id: session::new_session_id()?,
        base: repo.head()?,
        active_secs: 0,
        log,
    };
    st.set_current(Some(&current))?;
    let prompt = prompt::render(&task, &result_path(s), &notes_path(s));
    continue_run(gh, s, st, current, Some(prompt))
}

/// Runs (or, with no prompt, resumes) the session of `current`, then gates,
/// pushes and reports.
fn continue_run(
    gh: &dyn GitHub,
    s: &Settings,
    st: &State,
    mut current: Current,
    prompt: Option<String>,
) -> Result<RunResult> {
    let (n, log) = (current.issue, current.log.clone());
    let back = |h: HandBack| hand_back(gh, s, st, n, h, &log);
    match session_until_done(s, st, &mut current, prompt)? {
        Outcome::Finished { .. } => {}
        Outcome::Idle => return back(HandBack::Watchdog("no activity for the idle limit")),
        Outcome::Ceiling => return back(HandBack::Watchdog("the session ran past its time ceiling")),
        Outcome::Crashed(why) => return back(HandBack::NoResult(why)),
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

    let repo = Repo { dir: s.workspace.clone() };
    let base = current.base.clone();
    let files = match gate_commits(&repo, s, &base, n)? {
        Ok(files) => files,
        Err(h) => return back(h),
    };
    let mut failed = run_checks(&repo, s, &files)?;
    if failed.is_none() {
        repo.fetch()?;
        if repo.origin_main()? != base {
            if !repo.rebase_onto_origin_main()? {
                return back(HandBack::RebaseConflict);
            }
            failed = run_checks(&repo, s, &files)?;
        }
    }
    if let Some(tail) = failed {
        let main = Repo::prepare(&s.workspace, &s.remote_url)?;
        if !main.run(&s.check_cmd)?.0 {
            st.set_paused(true)?;
            return back(HandBack::MainBroken);
        }
        return back(HandBack::CheckFailed(tail));
    }

    let origin = repo.origin_main()?;
    let ids = repo.commit_ids(&origin, &repo.head()?)?;
    let (first, last) = (ids[0].clone(), ids[ids.len() - 1].clone());
    let lock_before = repo.show(&origin, "Cargo.lock")?;
    let lock_after = repo.show("HEAD", "Cargo.lock")?;
    repo.push_main()?;
    let deferred = files.iter().any(|f| f == DEFERRED_FILE);
    let done = Done {
        summary: result.text,
        first: first.clone(),
        last: last.clone(),
        commits: ids.len(),
        claude_md: gate::claude_md_edits(&files),
        new_packages: gate::new_packages(&lock_before, &lock_after),
        deferred,
    };
    gh.comment(n, &report::done_comment(&done))?;
    gh.close(n)?;
    let add: &[&str] = if deferred { &["needs-bench"] } else { &[] };
    gh.edit_labels(n, add, &["in-progress"])?;
    st.record(&format!("#{n} pushed {first}..{last}"))?;
    st.set_current(None)?;
    Ok(RunResult::Pushed { first, last })
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

/// The session's commits: clean, present, numbered and off protected
/// paths. Returns the changed files.
fn gate_commits(repo: &Repo, s: &Settings, base: &str, n: u64) -> Result<Result<Vec<String>, HandBack>> {
    if !repo.is_clean()? {
        return Ok(Err(HandBack::DirtyTree));
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

/// The check, plus the render check when the change touches rendering;
/// the failing output's tail, if any.
fn run_checks(repo: &Repo, s: &Settings, files: &[String]) -> Result<Option<String>> {
    let (ok, tail) = repo.run(&s.check_cmd)?;
    if !ok {
        return Ok(Some(tail));
    }
    if gate::touches_render(files) {
        let (ok, tail) = repo.run(&s.render_check_cmd)?;
        if !ok {
            return Ok(Some(tail));
        }
    }
    Ok(None)
}

fn hand_back(
    gh: &dyn GitHub,
    s: &Settings,
    st: &State,
    n: u64,
    h: HandBack,
    log: &std::path::Path,
) -> Result<RunResult> {
    gh.comment(n, &report::handback_comment(&h, log))?;
    gh.edit_labels(n, &["needs-maintainer"], &["in-progress"])?;
    // Reset the workspace; a failure here is repaired by the next run's prepare.
    let _ = Repo::prepare(&s.workspace, &s.remote_url);
    st.record(&format!("#{n} handed back: {}", h.describe().lines().next().unwrap_or_default()))?;
    st.set_current(None)?;
    Ok(RunResult::HandedBack(h))
}

/// What the next poll would do, with each candidate's approval time.
pub fn plan_next(gh: &dyn GitHub) -> Result<Pick> {
    let milestones = gh.open_milestones()?;
    let [milestone] = milestones.as_slice() else { return Ok(queue::pick(&milestones, &[])) };
    let mut candidates = Vec::new();
    for issue in gh.approved_issues(milestone.number)? {
        let at = if queue::eligible(&issue.labels) {
            trust::approved_at(&gh.issue(issue.number)?).map(|e| e.at.clone())
        } else {
            None
        };
        candidates.push((issue, at));
    }
    Ok(queue::pick(&milestones, &candidates))
}

fn describe(pick: &Pick) -> String {
    match pick {
        Pick::NoMilestone => "idle: no open milestone".into(),
        Pick::ManyMilestones(titles) => format!("idle: more than one open milestone ({})", titles.join(", ")),
        Pick::Empty { milestone } => format!("idle: no approved issue in {milestone}"),
        Pick::Next { milestone, number } => format!("next: #{number} in {milestone}"),
    }
}

/// One poll-and-work pass: finish a crashed run, then work the queue until
/// it is empty or the runner is paused.
pub fn tick(gh: &dyn GitHub, s: &Settings, st: &State) -> Result<()> {
    let Some(_lock) = st.lock()? else { return Ok(()) };
    if let Some(current) = st.current() {
        st.record(&format!("#{} resuming after an interrupted run", current.issue))?;
        continue_run(gh, s, st, current, None)?;
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

/// Works one issue now (`olr-runner run <n>`), under the lock.
pub fn run_now(gh: &dyn GitHub, s: &Settings, st: &State, number: u64) -> Result<RunResult> {
    let Some(_lock) = st.lock()? else { anyhow::bail!("another runner holds the lock") };
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
