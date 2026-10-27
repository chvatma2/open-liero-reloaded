# olr-runner

The issue runner (spec: `docs/superpowers/specs/2026-10-20-issue-runner-design.md`,
plan: `docs/superpowers/plans/2026-10-20-issue-runner.md`). It polls GitHub
for issues labelled `approved` in the one open milestone, works them one at
a time in headless Claude Code sessions in its own clone
(`~/.local/share/olr-runner/work`), and pushes the result to `main` or
hands the issue back (`needs-maintainer`).

Commands (`olr-runner <command>`):

- `install`: copies itself to `~/.local/bin/olr-runner`, writes the systemd
  user timer (every 15 minutes) and the sessions' Claude Code config
  (`~/.config/olr-runner/claude/`), creates the labels, checks prerequisites.
- `tick`: one poll-and-work pass (what the timer runs).
- `run <issue>`: work one approved issue now.
- `status`, `pause`, `resume`, `dry-run`.

Modules:
- `github`: the data model, the `GitHub` trait, its `gh` implementation and parsers.
- `trust`: what counts as the maintainer's task.
- `queue`: which issue is next; an open `needs-maintainer` issue older
  than it holds the queue (`Pick::Held`, `status`'s `held: #<n>`).
- `prompt`, `result`, `stream`, `limit`: the session's input and output.
- `session`: the headless session and its watchdog.
- `repo`: the workspace clone.
- `gate`, `report`: the checks before a push and the issue comments.
- `state`, `run`, `install`.

Tests never touch the network, GitHub or the user's repositories: they use
temp git repositories, a `FakeGitHub` (`tests/fake_github.rs`) and a fake
`claude` script (`tests/data/fake-claude.sh`).

Recovery: `current.ron` holds the run's `Phase` (`Claimed`, `Session`,
`Gate`, `Pushed`), written before each step with side effects. A tick
that finds one carries it on: a claim is handed back, a session resumed
once (after killing any session left in `session.pid`), a gate re-run, a
push only reported. A session that cannot run at all (crash, login,
network) pauses the runner rather than drain the queue. The gate's checks
run in bubblewrap (`gate::sandboxed`: read-only but for the workspace,
`~/.cargo` and `/tmp`; credentials and the runner's config hidden; niced).
