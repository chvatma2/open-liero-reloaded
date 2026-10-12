# xtask

Workspace automation (`cargo xtask <cmd>`): `check`, `full`, `layers`, `api`,
`oracle-record`, `tidy`, `bless-render`, `bless-regression`, `fetch-olx-content`,
`e2e`, `bench`, `stress`, `windows`, `envelope`.

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
- `envelope` (`envelope/`, plan 8, spec
  `docs/superpowers/specs/2026-10-11-performance-envelope-design.md`;
  `cargo xtask envelope [--quick] [--only <part>]... [--gpu
  wsl|windows|both|none] [--bless --reason "<why>"]`, or `--report
  [--from F]`): how far each axis
  goes on this machine before the tick (10 ms p99), frame (6.9 / 16.7 ms
  p99) or memory (2 GiB) budget breaks. The parts (`Axis`, `--only`,
  repeatable; all by default): the sweeps `worms`, `pool`, `fx`, `map`,
  `art`, then `gpu` and `olx`. `--quick` (`Plan::new`) is one run per
  point and fewer points (about 2 min; the full run, 3 runs a point,
  took 7 min on this machine warm, 2026-10-12: sweeps 1 min, GPU 5 min
  with the Windows build cached, OLX 1 min; a cold Windows build adds a
  few). Modules: `results.rs` (the RON the tools write, read as
  `*Out` with only the fields used and no defaults, so a field renamed
  in a tool is a failed point saying why, never a measured 0; the
  samples in `fixtures/bench/tool-output/` are kept equal to the tools'
  output by `rl-cli`'s `tests/tool_output.rs` and `rl-game`'s
  `frame_log_sample`, and `tool_outputs_parse` reads them; the kept
  `Envelope`, lenient so older files read; the pure
  arithmetic, unit-tested: medians of runs, `Spread::of` nearest rank as
  `rl-cli`'s and `rl-game`'s (ruling PF8), breaking points per budget
  (a map or art point's frame is `Point::frame_p99_ms`: its level's
  `gpu-bench` tick and frame, which replay recorded inputs and so hold
  no route finding, plus the point's bots' think p99), the
  least-squares cost per unit, probe passes that fit, `compare`,
  `merged`); `sweeps.rs` (each point is `rl-cli stress` in release in its
  own process, from the reference configuration: Classic on Castle
  Strike, 16 bots of mixed difficulties, pool 3000, effects 1024, seed 1;
  every run `--no-verify` (ruling P9: verifying art 8 would hold about
  7 GiB) and `--memory-ceiling-mib 16384`; chaos bots, except Deathmatch
  bots (`--personality dm`) on the map-size and art-scale axes, whose
  cost includes the area cache (ruling T1-A); a failed run is a failed
  point, its cause a breaking cap naming the exit status and the last
  output line (`failure`: a run the OOM killer stops says `signal: 9`); the map and art points record replays
  in `target/envelope/replays`); `gpu.rs` (the heavy replay, MW 1.0 with
  32 chaos bots and a 20 000 pool, and the levels' replays through
  `rl-cli gpu-bench --adapter hardware` offscreen, then `rl-game
  --frame-log` windowed and uncapped at each size; on WSL with GL over
  Mesa's d3d12 driver (`wsl_env`), on Windows through `winbuild` for
  `dx12` and `vulkan`, each run bounded by `winbuild::RUN_TIMEOUT`);
  `olx.rs` (`olx_timing` for OLX and `rl-cli scenario --time` for rl-sim,
  the same work checked frame by frame, `projectiles` and `rand_calls`
  the same length as OLX's frames; a scenario that fails is one of the
  table's `notes` and the others go on; OLX's build named as Release,
  `-O3`, ruling T6-A). `mod.rs` runs `steps`: a part whose prerequisite
  is missing (`Prereqs`: `winbuild::missing`, `docker_missing`, a
  display) or that fails, and a GPU path measured on a software-only
  adapter (`adapter_problem`: a `Cpu` device, or a software rasteriser
  by name, `software_name`, as WARP behind Mesa's d3d12 is `Other`; a
  windowed run on one is a note, `gpu::windowed_row`), is a `skip`
  notice and the run goes on
  (ruling PF5; `execute` takes a `Machine`, which the tests fake). Every
  run writes `target/envelope/latest.ron` and compares with
  `fixtures/bench/envelope/<host>.ron` (`host_key`: the CPU model's slug
  and CPU count; another machine's file is only a notice): a measurement
  slower than its axis's or GPU path's `noise` (the largest run spread,
  at least `NOISE_FLOOR`, 5 %) is flagged with `!`, never failed; stored
  results of another kind (`incomparable`: quick against full, or other
  match lengths) are only a notice. `--bless --reason` (refused with
  `--quick`) writes that file, keeping the parts it did not measure
  from results of the same kind (`kept`), and logs it in
  `fixtures/blessings.log`. `--report [--from F]` (`report.rs`) only
  writes `docs/perf/envelope.md` from this host's blessed results (or
  `F`): method, machine, each sweep's table and breaking points, each GPU
  path's offscreen frames, shader headroom (labelled GPU-side, not the
  whole frame), windowed runs (frame and busy p50/p99 and whether each
  holds 144 and 60 Hz; the text follows the measurements, never the
  path's name: rows presented with Fifo, busy time growing with the
  window's pixels (`busy_growth`: ms per million pixels, as WSL's
  presentation costs, against the offscreen frame), the sizes missing
  each budget, and the other paths that hold both everywhere), dropped GPU spans and notes,
  the OLX table with its build, load, timed-span asymmetry and notes, and
  the limits; the hand-written summary between `<!-- summary:start -->`
  and `<!-- summary:end -->` is kept word for word. Results also keep
  `gpu-bench`'s `gpu_dropped` counts (frames whose timestamps gave no real
  span). Not part of `full`.
- `olx_timing.rs` (plan 8; its caller is `envelope`, task 7): OLX's
  per-frame physics times. `Olx::open` builds or finds the oracle image
  (`oracle::ensure_image`); `Olx::time` plays a scenario through
  `oracle::record_once_with` (`record_once` with extra `docker -e`
  variables) with `ORACLE_TIMING` and `ORACLE_HEADER_ONLY`
  (`tools/olx-oracle/CLAUDE.md`) in `target/envelope/olx-work/<name>`,
  parses the `frame ns projectiles rc` lines (`parse_timing`) and can
  write the header-only trace (gzipped) for `rl-cli scenario --time`;
  `build_info` names the image's release flags and compiler (the build is
  Release, `-O3`, from `CMakeOlxCommon.cmake:43-47`; the cache's
  `CMAKE_BUILD_TYPE` is empty and never reported, ruling T6-A). The
  scenarios: `TIMED`, the six busiest oracle fixtures, and `HEAVY`, eight
  worms holding fire for 8 s: Spikeballs on Castle Strike, which reaches
  OLX's 3000-projectile cap, and Napalm on Liero Factory, a mid-load point
  peaking at about 830 projectiles; in `fixtures/perf/olx/` (`PERF_DIR`;
  not oracle fixtures, no committed trace). `docker_missing()` says why
  Docker cannot be used (not installed, or the daemon not running:
  `docker info` fails), for `envelope` to turn into a notice (ruling PF5);
  `Olx::open` fails with that reason before building anything.
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
