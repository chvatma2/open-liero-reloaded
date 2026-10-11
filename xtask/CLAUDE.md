# xtask

Workspace automation (`cargo xtask <cmd>`): `check`, `full`, `layers`, `api`,
`oracle-record`, `tidy`, `bless-render`, `bless-regression`, `fetch-olx-content`,
`e2e`, `bench`, `stress`, `windows`.

- `fetch-olx-content` (`fetch.rs`) imports the manifest into the
  committed `content/packs`; `check`'s `olx-content` step (`fetch::verify`,
  also `bench`, `stress`, `e2e`) imports into `target/content-fresh` and
  fails listing the files (`differences`) if `content/packs` is stale.
  `--offline` skips it: the tests run on the committed packs.
- `full` runs `check`, then `bot-grid` (`check::FULL_TESTS`: `rl-cli`'s
  `tests/grid.rs` with `RL_FULL_TESTS=1`), then the GPU step
  (`check::GPU_TESTS` with `RL_GPU_TESTS=1`: `rl-render`'s `gpu`,
  `rl-cli`'s `render_golden` and `gpu_bench` (plan 8's `gpu-bench` on the
  software adapter), `rl-game`'s `soak`, `rl-ui`'s `hud_snapshots`) and the
  audio step, then `e2e`, `bench` and `stress`.
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
- `windows` (`winbuild.rs`, plan 8): the native Windows build run from
  WSL, for `envelope --gpu windows`. `missing()` says why it cannot run
  here (no `cmd.exe` or `WSLInterop`/`WSLInterop-late` not enabled, no
  Windows PowerShell, the rustup target `x86_64-pc-windows-gnu` absent,
  no mingw-w64 linker or objdump), a notice, not a failure, for
  `envelope`. `build()` runs `cargo
  build --release --target x86_64-pc-windows-gnu -p rl-cli -p rl-game`
  with `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc`
  (about 1 min from cold here; no `crt-static` needed). `stage()` copies
  the exes, any `MINGW_DLLS` an exe imports (`objdump -p`: today none,
  the toolchain links mingw's runtime statically; only system DLLs) and
  the content (re-copied only when `content.stamp`, its file count,
  bytes and newest modification time, changes) to
  `%TEMP%\rust-liero-envelope` (`windows_temp()`: `cmd.exe /d /c echo
  %TEMP%`, started in `/mnt/c`), the given files into its `replays/`;
  first it stops any process left running from the stage (an interrupted
  run would hold the exes). `run()` starts an exe there (the stage as its
  working directory, so arguments use relative paths), forwards its `env`
  through `WSLENV` (`wslenv`; interop passes nothing else) and fails on
  an unsuccessful exit (interop keeps a Windows code's low 8 bits:
  0xC0000135 is 53) or past its `timeout` (`RUN_TIMEOUT`, 10 min, for
  the command and envelope points; the error says how many processes it
  stopped): Windows PowerShell's `STOP_SCRIPT` force-stops every process
  whose image path lies under the stage (by path prefix, never by image
  name), then the interop child is killed. The stage's path reaches the
  script in `RL_STAGE` through `WSLENV`, never in its source (PowerShell
  also takes U+2018-U+201B as quotes), and `stop_target` refuses a path
  that is not a `rust-liero-envelope` directory, so the user's own
  processes are never touched. The ignored test
  `runs_fail_on_a_bad_exit_and_are_stopped_past_their_timeout` checks
  both on the real machine (`--run-ignored only`, after a stage). The
  command builds, stages and lists `gpu-bench --list-adapters` for `dx12`
  and `vulkan`, failing when either run fails. On the user's machine: DX12
  and Vulkan both see the RTX 5070 Ti (and the AMD iGPU; DX12 also
  Microsoft's Basic Render Driver, a `Cpu` adapter), with timestamps on
  both; `rl-game.exe --demo` writes its shots under the stage's `target/`. Nothing is left running; the
  stage stays for the next run.
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
  opt-in and nothing else does it. It never touches `content/packs` or
  `target/olx`. `check` prints a `warn` line (not a failure) when `target/`
  exceeds 25 GB (`tidy::WARN_BYTES`); the size walk gives up after 1 s.
- WSL footnote: the Windows drive hosts the virtual disk; `wsl --shutdown`
  then `wsl --manage <distro> --set-sparse true` lets it shrink.
