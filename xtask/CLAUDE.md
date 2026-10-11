# xtask

Workspace automation (`cargo xtask <cmd>`): `check`, `full`, `layers`, `api`,
`oracle-record`, `tidy`, `bless-render`, `bless-regression`, `fetch-olx-content`,
`e2e`, `bench`, `stress`.

- `full` runs `check`, then `bot-grid` (`check::FULL_TESTS`: `rl-cli`'s
  `tests/grid.rs` with `RL_FULL_TESTS=1`), then the GPU and audio steps,
  then `e2e`, `bench` and `stress`.
- `bench` (`bench.rs`, also `cargo xtask bench [--offline]`, spec §21 G8):
  the divan benchmarks (`BENCHES`: `rl-sim/benches/tick.rs`,
  `rl-render/benches/frame.rs`, `rl-cli/benches/game.rs`; bench profile)
  `RUNS` (3) times, each one's median of the runs' medians against
  `fixtures/bench/baselines.ron` (ns); then `stress`: the short stress run
  (`STRESS`: MW 1.0 and Classic × pool 3000 and 20 000 on Castle Strike,
  32 chaos bots, `STRESS_SECONDS` (20) each, `rl-cli stress` in release,
  determinism checked on the first run) `RUNS` times, the median tick p50,
  frame p50 and peak MiB against `fixtures/bench/stress.ron`. Noise guard:
  a measurement fails only over `TOLERANCE` (+10 %; peak memory
  `MEMORY_TOLERANCE`, +25 %), and then only if the best of `RETRIES` (2)
  more runs is still over (a notice says it was retried). A baseline that
  nothing measured fails, unless its target said it skipped (`skip ...`:
  no adapter, no content; a stress run without frames or `/proc`): then a
  notice; a measurement without a baseline is a notice. Both files store
  the `Host` they were measured on (CPU count and model, the offscreen
  adapter); on another host `full` prints a notice and does not gate (the
  stress runs still check determinism). `full` prints one summary line
  each; the command prints every measurement. `bench --bless --reason
  "<why>"` measures and writes both files (logged like `bless-render`);
  baselines it could not measure are kept from the old file and listed in
  `kept`, and offline the stress file stays as it was. In repeated runs
  here (WSL2, 12 cores, lavapipe) the medians stayed within ±6 %. About
  35 s once built.
- `stress [--seconds 180]` (`bench::stress_command`): the long stress
  run, every `STRESS` configuration through `rl-cli stress` (progress and
  each report as it plays, determinism checked), then a summary table
  (tick µs, projectiles, entities, frame ms, MiB) and the adapter.
- `e2e` (`e2e.rs`, also `cargo xtask e2e [--offline]`): builds `rl-game`
  (dev profile), starts Xvfb on the first display from `:90` without a
  lock file (`-displayfd` says when it is ready; stopped with SIGTERM so
  its lock goes; WSLg mounts `/tmp/.X11-unix` read-only, Xvfb then listens
  on the abstract socket), and plays `fixtures/autoplay/menus.ron` with
  `--fake-clock --no-sound --window-system x11`, a fresh `target/e2e/data`
  holding `fixtures/autoplay/settings.ron`, and Mesa's software GL
  (`SOFTWARE_GL`: `WGPU_BACKEND=gl`, `LIBGL_ALWAYS_SOFTWARE=1`,
  `GALLIUM_DRIVER=llvmpipe`; setting `WGPU_BACKEND` also stops the WSL
  relaunch onto the real GPU). It passes on exit 0, an `autoplay.jsonl`
  ending in `pass`, every screenshot the script names, and no `warning:`,
  `error:` or panic line on stderr (`bad_lines`). Skipped without Xvfb or
  content; killed after 240 s. About 15 s.
- `bless-render --reason <why>` re-draws `fixtures/render/` (the render
  goldens) and `fixtures/ui/` (the HUD snapshots, `rl-ui`'s
  `hud_snapshots`) with `RL_BLESS_RENDER=1`; the GPU ones need an adapter.
- `bless-regression --reason <why> [--from target/failures/<name>.ron]`
  re-records `fixtures/matches/*.ron` (runs `rl-cli --test golden` with
  `RL_BLESS_REGRESSION=1`); `--from` first copies a failure record in as a
  new golden. Like `bless-render`, it appends the date, the reason and the
  changed files to `fixtures/blessings.log` (`bless::log_blessing`).

## Disk use

- The dev profile (workspace `Cargo.toml`) sets `debug = "line-tables-only"`
  for workspace crates AND dependencies (`[profile.dev.package."*"]`). Every
  integration test is its own binary linking wgpu/winit/egui, so full debug
  info once grew `target/` to 45 GB. Line tables keep panic backtraces with
  file:line everywhere, including inside wgpu/winit/egui; only variable and
  type info (debugger-only) is dropped. Symbols are not stripped. Our workflow
  reads backtraces and nobody steps a debugger; if a debugger session is ever
  needed, set `debug = true` for that crate locally (do not commit it).
- `oracle-record` prunes after building a new `rl-olx-oracle:<hash>` image:
  it removes every other `rl-olx-oracle:*` image except the current tag and
  `:dev`. Pruning failures only warn. `images_to_prune` and `prune_commands`
  (unit-tested) pick the tags and the commands.
- `tidy` deletes `target/*/incremental` (and `target/<triple>/*/incremental`)
  and prunes oracle images as above; `tidy --docker-cache` also runs
  `docker builder prune -f --keep-storage 2GB`. That cache is the whole
  machine's (Docker cannot select one project's layers), so pruning it is
  opt-in and nothing else does it. It never touches `target/content` or
  `target/olx`. `check` prints a `warn` line (not a failure) when `target/`
  exceeds 25 GB (`tidy::WARN_BYTES`); the size walk gives up after 1 s.
- WSL footnote: the Windows drive hosts the virtual disk; `wsl --shutdown`
  then `wsl --manage <distro> --set-sparse true` lets it shrink.
