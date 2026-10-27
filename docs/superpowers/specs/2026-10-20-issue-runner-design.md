# Issue runner — design

**Status:** design, 2026-10-20. Not yet planned.

## Why

OLR's repository is public: anyone can file a bug report or a feature
request on GitHub. The maintainer reads each one, adds what is missing,
puts it in a release and approves it. Today every approved issue then waits
for the maintainer to open a Claude Code session and drive it by hand.

The user wants that last step automated: a process on this PC polls
GitHub, and when an issue for the active release is approved, it spawns a
Claude Code session that implements it and commits the result to main. The
user cares about four things, in their words: low token usage, ease of
use, limiting risks, and limiting excessive testing and long retesting
loops.

## Goals

1. **Approve on GitHub, get a commit on main.** The maintainer's only
   actions are on GitHub: review, edit, set the milestone, add the
   `approved` label. The runner does the rest and reports back on the
   issue.
2. **Only the maintainer can start work.** Anyone may open issues; only
   the maintainer's approval queues one. Text written by others is never
   treated as instructions.
3. **One issue at a time, on this machine.** A queue, worked in order; a
   lock guarantees no two sessions overlap.
4. **Cheap verification.** A session verifies with `cargo xtask check` (and,
   when it touches performance, a new quick benchmark). Nothing heavier
   runs per issue: no `full`, `bench`, `stress`, e2e, device tier, playtest
   build or dist. What heavier checking a change needs is written down
   for the maintainer to batch before a release.
5. **Fail fast and hand back.** A session that cannot finish stops,
   explains why on the issue and gives the issue back to the maintainer.
   It never loops on a failure and never leaves main broken.
6. **Easy to undo.** Every issue's commits are identifiable, and the
   report on the issue gives the exact command that reverts them. The
   user's stance: if a change feels wrong in a later playthrough, revert it.
7. **Low token use** through lean prompts, a minimal session
   configuration and resuming instead of restarting (see "Session
   configuration").

## Non-goals

- Pull requests or code review before merge. The output is a commit on
  main (the user's choice).
- Working several issues in parallel.
- Cloud execution (GitHub Actions, Claude cloud sessions). It runs on this
  PC.
- Release management: tagging, `cargo xtask full`, dist, playtest builds.
  Those stay manual, done once per release.
- A per-issue token budget. The subscription's 5-hour window is the only
  limit, and the user does not see it as a risk.
- Triage: the runner never comments on, labels or closes issues the
  maintainer has not approved.

## Anti-goals (a failure even if it works)

- **A stranger steers a session.** An issue body, a comment or an edit by
  anyone but the maintainer changes what gets built or what runs.
- **Main is broken or half-done.** A push that fails `check`, or a
  partial feature from a session that ran out of time.
- **The PC is taken over.** Benchmarks or heavy suites running uninvited
  on a machine the user shares (standing rule: ask before bench, stress or
  full runs).
- **Silent loops.** A session retrying the same failure for hours, or the
  runner re-queuing an issue that keeps failing.
- **Babysitting.** A process that needs regular hand-holding is worse than
  doing the work by hand.

## The flow

### On GitHub (the maintainer)

1. Someone opens an issue. Optional issue forms (bug report, feature
   request) under `.github/ISSUE_TEMPLATE/` give reports a consistent
   shape (my call: cheap, helps the maintainer review).
2. The maintainer reviews it, edits the title and body or adds comments
   with the details the session needs, and sets the **milestone** of the
   release it belongs to (e.g. `0.2.0`).
3. The maintainer adds the **`approved`** label. That is the signal. On
   GitHub only people with write access can label, so the gate costs
   nothing; the runner still checks that the maintainer added the label
   (the issue's timeline names who did).

**Identity.** The runner acts on GitHub as the maintainer's own account
(`chvatma2`), through the existing `gh` login: it comments, labels,
closes and pushes as the maintainer (the user's choice). Every comment it
writes carries a hidden marker (e.g. `<!-- olr-runner -->`). Comments with
the marker are never trusted text, even though the maintainer's account
wrote them, so a hand-back comment that quotes a reporter can't turn into
instructions on a retry. The maintainer gets no GitHub notifications for
the runner's actions and doesn't want any. They follow progress through
the milestone's closed issues and the `(#<issue>)` commit subjects on
main.

Labels the runner uses (it creates any that are missing):

| Label | Meaning | Set by |
|---|---|---|
| `approved` | Queue this issue | maintainer |
| `in-progress` | A session is working on it | runner |
| `needs-maintainer` | The runner gave it back (reason in a comment) | runner |
| `needs-bench` | Done, but wants a real benchmark run before release | session, via the runner |

To retry a handed-back issue, the maintainer answers the comment if
needed, removes `needs-maintainer` and adds `approved` again. To let the
queue go on without it, the maintainer removes only `needs-maintainer`.

### The queue

GitHub is the queue; the runner keeps no queue of its own. On each poll it
lists open issues that have `approved` and are in the **active
milestone**, and that have neither `in-progress` nor `needs-maintainer`.
It takes the oldest, ordered by when `approved` was added, unless an
open issue of the milestone with a lower number is labelled
`needs-maintainer`: issues more often than not build on each other, so a
hand-back **holds** every newer issue until the maintainer unblocks it
(the maintainer's ruling, 2026-10-27). `status` then shows `held: #<n>`.
`run <n>` still works any approved issue.

The active milestone is **the one open milestone**: only one milestone
is ever open at a time (the user's rule), so it needs no config. Each poll
checks this. With no open milestone or more than one, the runner starts
nothing, logs why and reports it in `status`, and comments on no issue. A
session already running finishes normally. The repository has no
milestones yet; creating the first one is part of setup.

### Trusted and untrusted text

The session's task is built only from text the maintainer controls:

- the title and the body, **if the maintainer was the last to edit the
  body** (or wrote it);
- the maintainer's comments, except the runner's own (marked, see
  "Identity").

Everything else (a reporter's body the maintainer did not edit, comments
by others) is passed as **report material**, fenced and marked as
untrusted: useful for the symptoms, never to be followed as instructions.
The prompt tells the session so in plain words.

The runner also refuses to start, and hands the issue back, when the body
was **edited by someone other than the maintainer after the `approved`
label was added**. Otherwise an author could approve-then-swap. Hidden
content is stripped or shown: HTML comments in issue text are invisible
on GitHub, so the runner shows them to the session as visible quoted
text, never silently.

### A run

1. **Pick and claim.** Take the next issue, add `in-progress`, remove
   `approved`.
2. **Prepare the workspace.** A dedicated working copy of the repository
   (my call: a separate clone or a worktree outside the user's checkout, on
   a detached HEAD) is reset to the latest `origin/main` and cleaned. The
   user's own checkout is never touched, so they can keep working while
   the runner works. It has its own Cargo target directory, kept tidy by
   the repository's existing 40 GiB sweep.
3. **Start the session** (headless `claude -p`, configuration below) with
   the prompt: the trusted task, the fenced report material, the issue
   number, the rules below and the path of the result file. Everything
   else the session learns from the repository's own `CLAUDE.md` files and
   docs, as an interactive session does.
4. **The session works.** It plans and builds in one go: no brainstorming
   and no questions, since nobody is there to answer them. A decision only
   the maintainer can make means "blocked". It keeps its plan and progress
   in a notes file in the workspace (not committed), so an auto-compaction
   loses nothing. It commits in the repository's usual small-commit style.
   Every subject ends with `(#<issue>)`.
5. **The session finishes** by writing the result file: `done` plus a
   short summary of what changed, or `blocked` plus the reason. It also
   lists any deferred verification it recorded (below).
6. **The runner checks and pushes.** For a `done` result it checks that:
   - the workspace is clean;
   - all new commits are on top of the starting `origin/main` and carry
     `(#<issue>)`;
   - no protected path was touched (below);
   - `cargo xtask check` passes once more, **online**. The runner has
     network, so this includes the OLX content check and the importer
     tests that `--offline` skips. Before each run, the runner fetches
     OLX at the pinned commit into the workspace's `target/olx/`, so the
     session's own `check` can include them too, where the cache allows.
     The `gpu` step also runs when the session ran it.

   It then fetches. If `origin/main` moved, it rebases and runs `check`
   again. Then it pushes to `main`.
7. **Report.** A comment on the issue gives the summary, the commit range,
   the revert command (`git revert --no-edit <first>^..<last>`) and any
   deferred verification. The runner closes the issue and removes
   `in-progress`.
8. **Hand back** on anything else: a `blocked` result, no result, a failed
   final `check`, a rebase conflict, a protected path, or the watchdog.
   The comment says why and links the session log on this PC. The runner
   labels the issue `needs-maintainer`, removes `in-progress`, pushes
   nothing and resets the workspace. Commits from a handed-back run are
   not kept (my call: retrying from a clean main is simpler than resuming
   half a feature).

### Session rules (in the prompt)

- Verify with `cargo xtask check --offline`. Never run `full`, `bench`,
  `stress`, `envelope`, `e2e`, `net-nat`, `net-soak`, `dist`,
  `oracle-record` or the device tier.
- When the change may affect performance, run `cargo xtask bench-quick`
  (below). If it reports a regression it cannot fix, or the change needs
  more than the quick benchmark can tell, commit anyway and record a
  deferred-verification entry.
- **Rendering changes run the render goldens.** `check` does not cover
  them; they live in the `gpu` step of `full` (renderer GPU tests and
  `render_golden`). A session whose change touches rendering, the look
  system or the HUD runs that one step, with a new
  `cargo xtask check --also gpu` (or equivalent) that adds a named step
  from `full`. It does not run the rest of `full`. The sandbox therefore
  passes the GPU through (`/dev/dxg` on WSL). If that fails, the gpu step
  is a deferred-verification entry (my call: a targeted step, not "full
  testing").
- **Dependencies.** The session's sandbox may reach crates.io (and only
  that), so a feature can add a crate. The report lists every new dependency (my
  call).
- Re-blessing the match and render goldens is allowed when the change
  intends them, with `--reason "#<issue>: <why>"`. Oracle traces are
  recorded from real OLX and are never re-recorded. A change that breaks
  the oracle puts its new behaviour behind the quirks setting (the
  project's existing rule) or stops as blocked.
- **When a session counts as stuck:** the same failure survives three
  fix attempts in a row. Ordinary edit-and-check cycles on a big feature
  are not a failure streak. The issue turning out unclear, much bigger
  than described, or contradicted by the code also means stop as blocked.
- Do not push, do not use `gh`, do not edit the protected paths. The
  runner does all of that after the session.
- Keep docs current the way the repository already expects
  (`docs/porting/STATUS.md` and so on).
- Before writing `done`, review your own diff once against the issue.

### Deferred verification

A file in the repository, `docs/deferred-verification.md` (my call on the
name), lists what a change still needs before release: the issue, what to
run or measure (e.g. `cargo xtask bench`, a Windows 1080p measurement, a
playtest of a feel change), and why. Sessions append; the maintainer
clears entries when they batch-verify before a release. When an issue has
an entry, the runner adds `needs-bench` (or a fitting label) so it shows
on GitHub too.

### Protected paths

The runner refuses to push a diff that touches:
- the runner's own code and prompt;
- `.github/`;
- `.claude/`;
- the committed oracle traces (`fixtures/oracle/*.jsonl.gz`).

Those issues go back to the maintainer. The list lives in the runner's
config. A `CLAUDE.md` edit is allowed (the repository asks for them), but
the report lists it explicitly, since it steers every later session.

## Quick benchmark

New work in this project: `cargo xtask bench-quick`. The existing
`cargo xtask bench` takes too long for one issue: divan medians, three
runs, retries and a stress run against a 10 % wall-time tolerance.

What it should be:
- **Fast:** about a minute, warm.
- **Hot paths only:** a sim tick (`olr-sim`), one offscreen render frame,
  and a short headless bot match.
- **Stable on a shared PC.** Prefer **instruction counts** over wall time,
  so the answer does not depend on what else the machine is doing. Counting
  through `perf_event_open` should work at this machine's
  `perf_event_paranoid = 2` (own-process, user-space counting). Whether the
  WSL2 kernel exposes the hardware counter is unverified; checking it is
  the first step. If it doesn't, the fallback is the minimum of several
  wall-time runs, with a generous tolerance.
- **Generous tolerance:** it only catches large regressions (my starting
  value: 15 % on instructions, more on wall time). Its baselines are
  blessed like the others, with a reason logged in `fixtures/blessings.log`.
- **Never decisive:** a pass does not replace the real benchmarks; a fail
  becomes a deferred-verification entry, never a blocker in itself.

## Running on this PC

- **Trigger:** a systemd user timer (systemd user services already run in
  this WSL) starts `tick` every 15 minutes (my call). A tick that finds the
  lock held exits at once; otherwise it works the queue until it is empty,
  then exits. While WSL is down, nothing runs (see "Open for the builder").
- **Lock:** one file lock; at most one session on the machine.
- **Crash recovery:** an issue still labelled `in-progress` with no live
  session gets a resume of its session if possible. Otherwise it is
  handed back.
- **Usage limit:** when a session hits the 5-hour window, the runner reads
  the reset time from the message (`You've hit your session limit · resets
  3:45pm`; falls back to 30 minutes), sleeps until then, and **resumes the
  same session** (`--resume <session-id>`), so no context is rebuilt. The
  hit is not counted as a failure. The exact headless exit code and output
  for this case are not documented, so recognising it is a setup test.
- **Watchdog:** generous by design, since past sessions in this repo have
  run for hours and features can be big. A session is stopped when it
  shows no activity (no stream event) for 45 minutes or runs past 8 hours.
  Both values are in the config. The watchdog stops a session with SIGINT
  first, so the turn is recorded, and the issue is handed back.
- **Being a good neighbour:** sessions and their builds run under `nice`
  and idle I/O priority (my call), so a match or the user's own build stays
  responsive.
- **Logs:** each run keeps its stream-JSON transcript and the runner's own
  log under the runner's state directory, named by issue and date. The
  hand-back comment points to it.

### Commands (ease of use)

| Command | Does |
|---|---|
| `install` | Write the systemd units and the session config, check the prerequisites (gh login, claude login in the runner's config dir, bwrap, socat) |
| `status` | Lock holder, current issue, queue for the active milestone, last results |
| `pause` / `resume` | Stop taking new issues (a running session finishes) / start again |
| `tick` | One poll-and-work pass (what the timer runs; also run by hand) |
| `run <issue>` | Work one approved issue now, regardless of order |
| `dry-run` | Show what the next run would send: the queue and the prompt, with the trusted and untrusted parts marked |

## Session configuration

Every runner session uses its own config, apart from the user's:

- **Own config directory** (`CLAUDE_CONFIG_DIR`). It has no plugins and
  keeps only the `rtk` output-compression hook. The superpowers plugin's
  start-up prompt costs tokens and pushes towards asking questions, which
  stalls headless. The LSP and GitHub plugins aren't needed. It needs a
  one-time `claude login` there. `--bare` is not an option: it ignores the
  subscription login and the repository's `CLAUDE.md`.
- **Model and effort:** Opus, effort `high`, set explicitly, since Opus
  5.5 defaults to `medium`. A failed run costs more than the thinking.
- **Prompt cache:** `CLAUDE_CODE_PROMPT_CACHE_TTL=1h`, set explicitly. A
  check or build often takes longer than 5 minutes, and a 5-minute cache
  would expire between turns.
- **Auto-compaction:** `CLAUDE_CODE_AUTO_COMPACT_WINDOW=400k`, a starting
  value to tune on real issues. The default is about 967K on the 1M model,
  and every turn re-sends the whole context. The notes file makes
  compaction safe.
- **Session identity:** `--session-id <uuid>` chosen by the runner, and
  session persistence kept, so a limit wait or a crash can resume.
- **Auto memory off.** Nobody would curate a runner's memories, and the
  user's memory folder must not fill with them. The few standing rulings
  that matter to implementation live in the prompt or the repository docs.
- **Permissions:**
  - `bypassPermissions`, inside the Bash sandbox (bubblewrap and socat
    are installed). The sandbox is set to fail if unavailable, with no
    unsandboxed escape, and allows writes to the workspace and Cargo's
    directories.
  - Its network is limited to crates.io.
  - GPU passthrough is allowed for the `gpu` step.
  - It denies reading the gh and git credentials.
  - `git push` and `gh` are denied on top.

  The session cannot reach GitHub; only the runner can.
- **Output:** `--output-format stream-json --verbose`, which feeds both
  the log and the watchdog's activity signal.
- **No turn or budget cap**, by the user's choice; the watchdog covers
  hangs.

The runner installs its **own copy** of its code and prompt (`install`).
A session that edits the runner's source in the repository changes nothing
until the maintainer reinstalls, and the protected-path guard stops such a
push anyway.

## Decided by the user

- Trigger: an approval on GitHub that only the maintainer can give.
- Output: a commit on main, not a pull request.
- No playtest build and no full testing per issue. Benchmark-worthy work
  is noted in the repository for later.
- A lightweight benchmark sessions can run quickly.
- Runs on this PC, one issue at a time, through a queue.
- Generous time limits; big features happen.
- Sessions may change the goldens; unwanted feel gets reverted later.
- A session plans and builds in one go.
- Only one milestone is open at a time; it is the active release. The
  runner works nothing while zero or several are open. No config.
- The runner acts as the maintainer's own GitHub account. No
  notification channel: the maintainer checks milestones and commit
  messages.
- No token budget per issue.
- The label and milestone mechanics are open to whatever is least
  friction.

## Calls made in this design (check these)

- An `approved` label marks approval; the
  label set above.
- An issue body counts as trusted only if the maintainer edited it last.
  A non-maintainer edit after approval sends the issue back.
- A separate working copy with its own target directory; the user's
  checkout is untouched.
- Handed-back runs keep no commits; a retry starts from main.
- The runner re-runs `check` before pushing, and again after a rebase.
- The protected-path list.
- Polling every 15 minutes; idle watchdog 45 minutes, ceiling 8 hours.
- `nice` / idle I/O for sessions.
- `docs/deferred-verification.md` as the deferred list, plus a
  `needs-bench` label.
- The quick benchmark's shape and its 15 % starting tolerance.
- Issue forms for bug reports and feature requests.

## Left to the builder

- Smaller calls already made, for the plan to follow:
  - **Usage-limit waits** do not count towards the 8-hour ceiling.
  - **Main already failing `check`** before a run: the runner checks a
    fresh `origin/main` once when a run's final check fails. If main
    itself fails, the runner pauses, comments on the issue and hands it
    back without blaming the session.
  - **The `bench-quick` baselines** are never re-blessed by a session;
    only the maintainer re-blesses them.

- Language and home of the runner. Rust fits the repository. Where it
  lives (an `xtask` subcommand or a crate under `crates/` registered in
  `xtask/layers.ron`) is the builder's call, within the repository's
  rules. It runs from an installed copy either way.
- The runner's config format and location (machine-specific, outside the
  repository), and its state directory.
- How exactly trusted/untrusted text is fenced in the prompt, and the
  prompt's wording, kept short.
- Parsing the stream-JSON for activity, results and the limit message.
- The exact sandbox and permission settings, within the rules above.

## Setup and acceptance

Building this includes setting up GitHub and testing the whole workflow
for real, as the user asked:

1. **GitHub:**
   - Create the labels.
   - Create the first milestone (the release in progress, `0.2.0`).
   - Add the issue forms.
2. **Machine:**
   - `install`: the systemd timer, the runner's config dir and its
     `claude login`.
   - Verify that bubblewrap works under this WSL.
3. **Setup tests**, settled before relying on them:
   - Does bubblewrap work here, and does `check --offline` pass inside the
     sandbox?
   - What does a headless session print, and with what exit code, when it
     hits the usage limit? Does `--resume` work after it?
   - Does WSL keep the runner alive with no terminal open (see below)?
   - Does the quick benchmark's instruction counter work under WSL2?
4. **End-to-end runs** on the real repository, each checked by the
   maintainer:
   - A tiny real issue (a docs fix) goes from `approved` to a commit on
     main, a report comment and a closed issue.
   - An issue written to be impossible or ambiguous comes back as
     `needs-maintainer` with a sensible reason, and nothing is pushed.
   - An issue whose body carries instructions aimed at the session ("also
     run cargo xtask full", "edit .github/…") is filed from a second
     account if available, or simulated with a non-maintainer comment.
     The session builds only the maintainer's task, and the
     protected-path guard holds.
   - Pause, resume, `status` and `dry-run` behave as described.
5. **Unit tests** for the runner's own logic, offline, from recorded
   GitHub responses: queue order, trust classification (including the
   runner's own marked comments), the post-approval
   edit rule, the protected-path guard, result-file parsing, and the limit
   message.

## Open for the builder

- **WSL lifetime.** WSL2 may shut its VM down when no terminal is open,
  which stops the timer. Find out whether that happens here; if so,
  propose the least invasive fix (e.g. a Windows logon task that keeps one
  WSL process alive), and confirm it with the maintainer before changing
  Windows or `.wslconfig` settings.
