# xtask

Workspace automation (`cargo xtask <cmd>`): `check`, `full`, `layers`, `api`,
`oracle-record`, `tidy`, `bless-render`, `bless-regression`, `fetch-olx-content`,
`e2e`, `net-nat`, `bench`, `bench-quick`, `stress`, `net-feel`, `net-soak`, `windows`, `envelope`,
`determinism`, `dist`, `flatpak`, `publish`, `format-docs`.

- `format-docs [--check]` (`format_docs.rs`, plan 13d R4; the level page
  also documents `editor/composition.ron`, plan 14a) writes
  `docs/formats/{mod,level,skin,theme,core,pack}.md` from the authoring
  types (`PAGES`: each page's sources, file types and map files), read
  with `syn`: each serialized field's name, type ("list of T", "map of name
  → T", "T (optional)", a link to a named type's section; a hand-serialized
  type such as `Colour` is a section of its doc comment), default
  (`required`; a `default = "d::…"` value from `defaults.rs`' macros or a
  local function; a type's zero, an `impl Default` literal or an enum's
  `#[default]`; "none" for an `Option`) and doc comment, which states the
  unit, limits and OLX source. Enums list their variants, struct variants
  in a sub-table; `#[serde(from = "W")]` documents `W`'s fields with the
  struct's docs and defaults (`Spawn`: "by context"); type aliases link
  to their target or get a section. Sections follow first reference from
  the file types (deterministic). A serialized field or variant without a
  doc comment is an error naming `file:line`. Each file type shows its
  example: the guide's block (`docs/formats/README.md`, `<!-- file:
  <kind>/<path> -->` then a `ron` block), so editing the guide means
  regenerating; a malformed marker, or a file type without its example, is
  an error. `--check` compares instead and fails naming each stale page; `check`'s `format-docs` step (before `test`, offline too) runs it.
  Unit tests on inline sources.

- `fetch-olx-content` (`fetch.rs`) imports the manifest into the
  committed `content/packs`; `check`'s `olx-content` step (`fetch::verify`,
  also `bench`, `stress`, `e2e`) imports into `target/content-fresh` and
  fails listing the files (`differences`) if `content/packs` is stale.
  `--offline` skips it: the tests run on the committed packs.
- `layers` also enforces `forbidden` in `layers.ron`: no crate listed on the
  left may depend, directly or transitively, on the one on the right (the
  game, the server and the UI never link the legacy importers).
- `full` runs `check`, then `bot-grid` (`check::TIMED_TESTS`: `olr-cli`'s
  `tests/grid.rs`, alone and one test at a time, its p99 tick budget being
  wall time; then `check::FULL_TESTS`: `tests/net_sim.rs`' online grid and `tests/transport.rs`'
  real-transport matches (spec §9.4), `olr-net`'s `link`, and `olr-game`'s
  `integration` again for its full-tier migration case, with
  `OLR_FULL_TESTS=1`), then the GPU step
  (`check::GPU_TESTS` with `OLR_GPU_TESTS=1`: `olr-render`'s `gpu`,
  `olr-cli`'s `render_golden` and `gpu_bench` (plan 8's `gpu-bench` on the
  software adapter), `olr-game`'s `soak` and `canvas_gpu` (the editor's
  canvas), `olr-ui`'s `hud_snapshots`; `the_gpu_tier_names_test_binaries`) and the
  audio step, then `e2e`, `net-nat`, `bench`, `stress` and `net-feel`.
  Each step prints `ok   <name> (<s>s)` (`net-nat`: `ok   net-nat (12
  scenarios, <s>s)`, `run_with`'s `finish` with a detail) or `FAIL` and
  its output.
- `bench` (`bench.rs`, also `cargo xtask bench [--offline]`, spec §21 G8):
  the divan benchmarks (`BENCHES`: `olr-sim/benches/tick.rs`,
  `olr-render/benches/frame.rs`, `olr-cli/benches/game.rs` and `rollback.rs`; bench profile)
  `RUNS` (3) times, each one's median of the runs' medians against
  `fixtures/bench/baselines.ron` (ns); then `stress`: the short stress run
  (`STRESS`: MW 1.0 and Classic × pool 3000 and 20 000 on Castle Strike,
  32 chaos bots, `STRESS_SECONDS` (20) each, `olr-cli stress` in release,
  determinism checked on the first run) `RUNS` times, the median tick p50,
  frame p50 and peak MiB against `fixtures/bench/stress.ron`. Noise guard:
  a measurement fails only over `TOLERANCE` (+10 %; peak memory
  `MEMORY_TOLERANCE`, +25 %). The divan targets are built first
  (`build_benches`: `cargo bench --no-run --message-format=json` per
  target, `bench_executable`), then the machine is let `settle` (fewer
  than `SETTLE_BUSY_CPUS`, 1, CPUs busy in `/proc/stat` over
  `SETTLE_SAMPLE`, 1 s; at most `SETTLE_MAX`, 60 s, then a notice), and
  every run executes a fresh copy of the target's executable (`Copies`,
  in `target/bench-copies`, all kept until the step ends, then removed):
  `tick_32_worms` ran 13-20 % slower from some byte-identical copies than
  from others, the same copy every time, so runs of one built file are
  one draw. A divan benchmark still over is measured in up to `RETRIES`
  (2) later windows, each `RETRY_PAUSE` (5 s) after the last and settled,
  each the median of `RUNS` runs of its whole target (as the baseline;
  a function alone measured 2-3 % faster), from fresh copies; the best
  window counts (`best_window`: noise only slows) and a notice lists each
  (`window_notice`, busy ones marked). A rerun that does not measure the
  benchmark is an error, never a pass (the retries once passed divan
  `--exact <function>`, which matches nothing: it needs
  `<target>::<function>`). A stress measurement over is run `RETRIES`
  more times, the best counting. On this machine `tick_32_worms` was
  +25 % to +60 % a run with 2 to 12 other threads busy; a +15 % synthetic
  regression measured +18 % to +21 % in every window and failed 4 of 4
  times. A baseline that
  nothing measured fails, unless its target said it skipped (`skip ...`:
  no adapter, no content; a stress run without frames or `/proc`): then a
  notice; a measurement without a baseline is a notice. Both files store
  the `Host` they were measured on (CPU count and model, the offscreen
  adapter); on another host `full` prints a notice and does not gate (the
  stress runs still check determinism). `full` prints one summary line
  each; the command prints every measurement. `bench --bless --reason
  "<why>"` measures (built, settled, fresh copies) and writes both files
  (logged like `bless-render`); baselines it could not measure are kept
  from the old file and listed in `kept`, and offline the stress file
  stays as it was. About 35 s once built.
- `bench-quick [--bless --reason "<why>"]` (`bench_quick.rs`; the issue
  runner's one-minute benchmark of one change, not a `full` step): builds
  `olr-cli` in release, runs `quick-bench --out target/bench-quick.ron`
  and reads it strictly (`QuickReport`, unknown fields refused), then
  compares with `fixtures/bench/quick.ron` (`Baseline`: the `Host` and the
  report): instructions of the match's ticks and draw lists fail over
  `INSTR_TOLERANCE` (+15 %), the fastest 1080p frame over
  `RENDER_TOLERANCE` (+50 %); a missing baseline or number, or another
  host's baseline, is a notice. `--bless` writes the baseline, logged in
  `fixtures/blessings.log`. Unit tests on `compare`.
- `stress [--seconds 180]` (`bench::stress_command`): the long stress
  run, every `STRESS` configuration through `olr-cli stress` (progress and
  each report as it plays, determinism checked), then a summary table
  (tick µs, projectiles, entities, frame ms, MiB) and the adapter.
- `net-feel [--bless --reason "<why>"]` (`net.rs`, plan 9, spec §9.2;
  also `full`'s last step): builds `olr-cli` in release, runs `net-sim
  --feel target/net/feel.ron` (every network profile × seeds 1 and 2 and
  the 32-worm bandwidth run; virtual time, so host-independent) and reads
  it strictly (`FeelReport`, unknown fields refused; the sample
  `fixtures/bench/tool-output/feel.ron` is tested), then compares with
  `fixtures/net/feel.ron`: `ok   <profile>` / `FAIL <profile>: <why>` and
  notices. Absolute rules: no stalls for `lan`, `good`, `typical`, `bad`;
  `bandwidth32.down_bytes_per_s <= DOWN_BUDGET` (24 576, spec §3.1).
  Regression rules: a metric worse than its baseline by more than 10 % and
  more than its floor (rollback depth 1 tick, correction 1 px, own
  corrections 0.5/min, bytes 256 B/s) fails; better by as much, a notice
  to re-bless. No baseline: a notice, and the absolute rules still apply.
  `--bless` writes the baseline (note `NOTE`) only when the absolute rules
  pass (else the netcode has a problem to fix, not the gate) and logs the
  reason in `fixtures/blessings.log`. About 40 s once built.
- `net-soak [--minutes 10] [--seed S] [--offline]` (`net::soak`, plan 12c,
  N77, spec §9.10; **not** a `full` step): builds `olr-cli` in release
  (`bench::olr_cli_with`; `--offline` builds with cargo's `--offline`) and
  runs `olr-cli net-soak --minutes M --seed S --content content/packs
  --failures target/failures` (`soak_args`), its output streaming (one line
  per case, a summary), and exits 1 if a case failed. Seeded host crashes,
  departures and freezes, outages, profile changes and spectators over
  `net-sim`'s virtual time, every 5th case a real-transport match on
  127.0.0.1 (nothing leaves the machine). Each failure is kept in
  `target/failures` as a case (`net-<stem>.ron`: replay it with `olr-cli
  net-sim --from`, keep it with `bless-regression --from`) or, for a
  transport case, `soak-transport-<seed>/` (spec, verdict, server log);
  at most 32, then it stops. A failure is a finding to fix in its task,
  not a gate to loosen. Unit test `net_soak_args_are_parsed`.
- `e2e` (`e2e.rs`, also `cargo xtask e2e [--offline]`): builds
  (`build_binaries`, shared with `net-nat`) `olr-game`
  and `olr-server` (dev profile), and `olr-cli --features nat-tools` in a
  cargo run of its own (one run would give the others olr-discovery's
  test relay too; `target/debug/olr-cli` is then the feature build, and
  `dist`'s plain `cargo build -p olr-cli` and this step rebuild it in turn
  in `full`: harmless, each builds right before its use), starts Xvfb on the first display from `:90` without a
  lock file (`-displayfd` says when it is ready; stopped with SIGTERM so
  its lock goes; WSLg mounts `/tmp/.X11-unix` read-only, Xvfb then listens
  on the abstract socket), and plays `fixtures/autoplay/menus.ron` with
  `--fake-clock --no-sound --window-system x11`, a fresh `target/e2e/data`
  holding `fixtures/autoplay/settings.ron`, and Mesa's software GL
  (`SOFTWARE_GL`: `WGPU_BACKEND=gl`, `LIBGL_ALWAYS_SOFTWARE=1`,
  `GALLIUM_DRIVER=llvmpipe`; setting `WGPU_BACKEND` also stops the WSL
  relaunch onto the real GPU). It passes on exit 0, an `autoplay.jsonl`
  ending in `pass`, every screenshot the script names, and no `warning:`,
  `error:` or panic line on stderr (`bad_lines`). Then the editor run
  (plan 14c, `editor`, in `target/e2e/editor`): `fixtures/autoplay/editor.ron`
  the same way in a data directory of its own (a new level "E2E Level", a
  drawn layer painted, a spawn, Play (ticking) and Stop, Play and Esc,
  Save, Close), then its `editor-painted.png` (`canvas_problem`: not the
  canvas's black at the picture's centre) and the pack it saved, `data/packs/e2e-level` (`editor_pack_problems`: a `Drawn` layer in
  `editor/composition.ron`, `level.ron` named "E2E Level" with one spawn)
  and `olr-cli pack check` on it exiting 0. Then the online run
  (plan 10 task 15, plan 12b task 10, `online`, in `target/e2e/online`):
  the run's own Nostr relay, `olr-cli nostr-relay --listen 127.0.0.1:0`
  (its URL read from its first line, `nostr relay: ws://…`; nothing
  leaves the machine), `olr-server --config` with
  `fixtures/server/e2e.ron`'s `{relay}` replaced by that URL (written to
  `online/server.ron`: Public on that relay at `public_address`
  127.0.0.1 with `allow_local`, no STUN, no port mapping, port 0, no fill
  bots, one 0.2-minute Classic match on Dirt Level that starts when both
  players are ready; its port read from the log's "listening on port N",
  `unstamped`), `olr-cli net-client --bot --name "e2e bot" --join
  127.0.0.1:<port>` waited for in the server's log (so the game's lobby
  has both members at once), then the game plays
  `fixtures/autoplay/online.ron` (`{port}` replaced) on the real clock (no
  `--fake-clock`: the netcode keeps real time; the script's waits are
  frames, about 180 a second here, so its long `within`s are generous)
  from `settings.ron` naming the relay (`online_settings` replaces its
  `relays: Some([])`): Multiplayer, the Direct tab, the address typed,
  Join, the lobby's 2 members (the dedicated server is no member), Back,
  then the Internet tab (2 frames for the table's sizing pass), the
  listed "e2e server" clicked until its Join shows (`ClickUntil`), Join (its announced address, directly),
  the lobby's 2 members again, a chat line, Ready, the match with keys
  held, its results, Back to lobby, Back, Quit. Besides the game's checks
  it needs the bot client to exit 0 by itself after its match, the server
  to stop cleanly on SIGTERM ("server stopped"), the relay to run until
  then and stop on SIGTERM (not killed), the server's log (stamps removed) to show three joins (the
  bot's, the game's typed and listed), the chat line, a match start and
  end, and no `bad_lines` in any helper's output (`Helper`: output in
  `<name>.out`/`.err` files, killed when dropped, so a failing run leaves
  nothing running; a failure's report includes all three). Skipped
  without Xvfb or content; each game killed after 240 s. The runs about
  29 s (local 10 s, editor 1 s, online 16 s) once built.
- `net-nat [--keep] [--offline] [--only <scenario>]...` (`nat.rs`, plan
  12b Task 11, spec §9.7, ruling N58; also `full`'s step after `e2e`,
  `nat::step(offline)`): the Docker NAT tests; topology, scenarios,
  lifetimes and timings in `tools/net-nat/CLAUDE.md`. Prerequisites
  (`prerequisites`; each missing one is `skip net-nat (<why and the
  fix>)`, a pass): `olx_timing::docker_missing`, `/etc/os-release` with
  `ID` and `VERSION_ID` of a Debian-like distribution (`base_image`),
  the content (`content/packs`). The binaries are `e2e`'s dev builds
  (`e2e::build_binaries`: `olr-game` and `olr-server`, then `olr-cli
  --features nat-tools` in a cargo run of its own, as feature
  unification would give the others olr-discovery's test relay; both
  steps run exactly these, so `full` builds them once), mounted from
  `target/debug`. `ensure_image`: `olr-nat:<image_tag>` (SHA-256 of
  `TOOL_FILES` and the base) built when missing (`docker build
  --progress=plain --build-arg BASE=<ID>:<VERSION_ID>`), then
  `oracle::prune_old` removes the other `olr-nat` tags; skips (N58):
  `--offline` without the image, a base that cannot be pulled, and
  `build_skip` (apt offline: `Failed to fetch`, `Temporary failure
  resolving`; a missing package: `Unable to locate package`, `has no
  installation candidate`); any other build failure fails. Then the
  machine-wide `lock` (`<temp>/olr-nat.lock`, at most 10 min), the
  removal of anything labelled `olr-nat` (an interrupted run's), the
  configs (`Config::text`) in `target/net-nat/configs/`, and a `Run`
  (id: the process id, in every name and the label `olr-nat=<id>`;
  `life`: `LIFE` 120 s, `KEEP_LIFE` 3600 with `--keep`) whose networks
  (`Run::networks`, `network_args`: macvlan `--internal`, no parent;
  `network_skip`: a Docker without macvlan skips, judged from the
  daemon's message alone, as `docker()`'s error starts with the command
  line, which names macvlan; an overlapping subnet or a taken name fails) live for the run and
  go when it ends (`Networks`' `Drop`) unless `--keep`. Each of
  `SCENARIOS` (`select`: `--only`, table order, an unknown name an
  error) is `play`ed: `units` (the pure plan: per container its stage,
  `docker run` arguments (`--name olr-nat-<id>-<role>`, the labels
  `olr-nat=<id>` and `olr-nat.scenario=<name>`, `-e OLR_NAT_LIFE`, our
  networks only, the command bounded by `node.sh`, `router.sh` or
  `timeout --preserve-status -k 5 <life>`) and `Ready`: lines, an
  invite code (`invite_code`, with an address when `Code::Address`), or
  its exit), started stage by stage (`run_units`; the joiner's
  `{invite}` replaced by the host's code) and waited for (`wait_ready`,
  until `LIMIT`, 60 s, from the scenario's start; a container exiting
  before it is ready fails), then the host stopped with SIGTERM when
  `host_stops`, and `verdict` (pure: `Expect` against `Seen`: exit
  status, stdout and stderr lines, the host's log (one of each group),
  the relay's `allocations` (at least one, when `relay_allocates`), the
  invite's kind, the host's exit). `Containers` saves every log to
  `target/net-nat/<scenario>/` and removes the run's containers when
  dropped (unless `--keep`), so a failing or panicking scenario leaves
  nothing running; a failure prints the last 40 lines of each
  container. **Ctrl+C** (`watch_interrupts`: `ctrlc`, installed once,
  only acting while a run is `ACTIVE`; otherwise, or a second time, it
  exits 130 at once): the waits return `interrupted`, the guards remove
  the containers and networks, and the process exits 130 (in `full` too:
  no later step runs). The command prints `     net-nat: <scenario>
  passed in <s> s` per scenario. Unit tests: `base_image_from_os_release`,
  `invite_code_is_read_from_the_log`,
  `scenario_names_are_unique_and_selectable`,
  `image_tag_changes_with_the_scripts`,
  `every_container_stays_on_our_networks_and_ends_by_itself` (names,
  labels, `OLR_NAT_LIFE`, networks and their subnets, read-only mounts,
  no host network, every command bounded, for `LIFE` and `KEEP_LIFE`),
  `the_networks_are_internal_macvlan_without_a_parent`,
  `configs_are_ron_on_our_servers_only`,
  `the_verdict_names_what_is_missing`,
  `allocations_are_read_from_the_relay_log`,
  `image_build_failures_that_skip`, `network_failures_that_skip`,
  `the_migration_scenario_is_listed_and_selectable`,
  `checksum_lines_are_compared`, `the_migration_verdict_names_what_is_missing`.
  The `migration` scenario (`Scenario::migration`: a second client, `Joiner::name`;
  `limit()` 90 s; `crash_host` kills the host with `docker kill` 8 s after
  both clients' `match starts:`; `Expect::other_stdout`,
  `checksums_equal`) is in `tools/net-nat/CLAUDE.md`. About 146 s here once built.
- `windows` (`winbuild.rs`, plan 8): the native Windows build run from
  WSL, for `envelope --gpu windows`. `missing()` says why it cannot run
  here (no `cmd.exe` or `WSLInterop`/`WSLInterop-late` not enabled, no
  Windows PowerShell, the rustup target `x86_64-pc-windows-gnu` absent,
  no mingw-w64 linker or objdump), a notice, not a failure, for
  `envelope`. `build()` runs `cargo
  build --release --target x86_64-pc-windows-gnu -p olr-cli -p olr-game`
  with `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc`
  (about 1 min from cold here; no `crt-static` needed). `stage()` copies
  the exes, any `MINGW_DLLS` an exe imports (`objdump -p`: today none,
  the toolchain links mingw's runtime statically; only system DLLs) and
  the content (re-copied only when `content.stamp`, its file count,
  bytes and newest modification time, changes) to
  `%TEMP%\olr-envelope` (`windows_temp()`: `cmd.exe /d /c echo
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
  script in `OLR_STAGE` through `WSLENV`, never in its source (PowerShell
  also takes U+2018-U+201B as quotes), and `stop_target` refuses a path
  that is not a `olr-envelope` directory, so the user's own
  processes are never touched. The ignored test
  `runs_fail_on_a_bad_exit_and_are_stopped_past_their_timeout` checks
  both on the real machine (`--run-ignored only`, after a stage). The
  command builds, stages and lists `gpu-bench --list-adapters` for `dx12`
  and `vulkan`, failing when either run fails. On the user's machine: DX12
  and Vulkan both see the RTX 5070 Ti (and the AMD iGPU; DX12 also
  Microsoft's Basic Render Driver, a `Cpu` adapter), with timestamps on
  both; `olr.exe --demo` writes its shots under the stage's `target/`. Nothing is left running; the
  stage stays for the next run.
- `dist [--no-verify] [--final]` (`dist.rs`): the releases (`release_name`): the
  Linux Flatpak, `target/release-zips/open-liero-reloaded-<version>-linux-x64.flatpak`
  (`flatpak::bundle`; there is no Linux zip, and an old one there is
  removed), and the Windows zip, `…-windows-x64.zip` (`ZIP_DIR`; `zip_name`,
  `folder`). `<version>` (`version.rs`, `release_version`) is a work in
  progress's, `wip_version`: `<VERSION>-dev.<commits since the newest
  release tag the commit has, v<x.y.z> only (is_release_tag); all without
  one>.<7-hex commit>`, `.dirty` with uncommitted changes (untracked files
  aside); with `--final`, `final_version`: the plain `VERSION` (xtask's,
  the workspace's), refused unless `git tag --points-at HEAD` has
  `v<VERSION>` and the tree is clean (it never tags). The builds get it as
  `OLR_BUILD_VERSION` (`version::BUILD_VERSION_VAR`) and the release
  repository the game checks for updates, `OLR_RELEASE_REPO`
  (`version::RELEASE_REPO_VAR`, `RELEASE_REPO`; issue #4: a build without
  it never checks), the folder in the zip,
  the Flatpak's name and its metainfo `<release version>` too; the Linux
  check runs `olr --version`, which must say `olr <version>`. `flatpak`
  alone names a work in progress. Without the Flatpak
  tools (`flatpak::tools_missing`) the command fails before building. `olr-game` and
  `olr-server` are built with `cargo build --profile dist` (workspace
  `Cargo.toml`: release, `strip`, no debug info, panics unwinding),
  natively and for `winbuild::TARGET` (`winbuild::build_missing()` fails
  the command: both releases or none), staged in `target/dist-stage/` (removed
  after) under one folder `open-liero-reloaded-<version>/`: the exes, the mingw
  DLLs an exe imports (`winbuild::mingw_imports`), `DOCS`, the example
  server config as `server.example.ron` (`files`), `content/packs`
  without `import-report.ron` (`copy_content`), OLR's own packs
  `content/reloaded` (`RELOADED`, issue #7) without their README
  (`copy_reloaded`), and last
  `install-manifest.txt` (`write_install_manifest`: every file, the
  updater's record of what the release installed, `olr_update::install`).
  `write_zip` (the `zip`
  crate, deflate only): files in byte order of their path, dated
  1980-01-01, mode 0755 for `executables` and 0644 for the rest, so the
  same inputs give the same bytes (tested). The zips are kept apart
  from cargo's `dist` profile output (`target/dist/`, swept by `tidy` as
  any profile). Verification (unless
  `--no-verify`): the Flatpak's staged folder (`flatpak::staged_app`,
  what the bundle's `/app/lib/olr/` holds; its programs mode 0755), with
  `olr --help` run from a temporary directory, must show the
  content beside it as `--content`'s default, the folder must hold the
  Reloaded levels (`RELOADED_LEVELS`, `check_reloaded`), and `olr-server` (config
  `CHECK_CONFIG`: invite only, no relays, no STUN, port 0, two fill seats (the client and a bot),
  `min_players: 1`; its rotation names the Reloaded levels after Dirt
  Level, so it checks they load at its start) must log its map (`next map:`), then start a match
  with the dev `olr-cli net-client --bot` joined (`match start:`), and stop
  cleanly on SIGTERM with no `bad_lines`. The Windows zip is unpacked into
  `%TEMP%\olr-envelope\dist` (`winbuild`'s stage, so
  `stop_stage_processes` reaches it) and run through interop
  (`winbuild::run_in` for `olr.exe --help`, `check_reloaded`; `olr-server.exe` until it
  logs its map, then stopped by path; no client: WSL cannot reach a
  Windows listener reliably); without interop (`winbuild::interop_missing`)
  a `skip` notice. Prints each release's path and size.
- `flatpak` (`flatpak.rs`): the Linux release alone, unchecked (`dist`
  makes it with the Windows zip and checks both): the build (`dist::build`,
  `dist::stage`) repackaged by `bundle` as
  `target/release-zips/open-liero-reloaded-<version>-linux-x64.flatpak`
  (`bundle_name`) with `tools/flatpak/`'s manifest, desktop entry,
  metainfo and icon (`APP_ID`); details in `tools/flatpak/CLAUDE.md`.
  Without `flatpak` and `flatpak-builder` it fails naming them. Nothing
  is run. Unit tests: the bundle's name; the files name `APP_ID`, as
  `olr-game`'s `window::APP_ID`; the manifest installs every file and
  links both programs.
- `publish --notes <file>` (`publish.rs`, issue #4; how to release, the
  keys and backups: `docs/releasing.md`): publishes the build `dist` just
  made (`--version`, else the newest Windows zip in `ZIP_DIR`). Every
  input is a `Config` field (`command` fills the real ones), and `publish`
  checks all before writing: `check_version` (no `.dirty`; a dev build's
  base above `latest_final`, the newest `v<x.y.z>` tag; a final not
  below it), the ed25519 key (PKCS#8 PEM) against `SIGNING_PUBLIC`, the
  GPG home's secret keys against `GPG_PUBLIC`'s fingerprint
  (`gpg_fingerprint`: OpenPGP v4, SHA-1 of the key packet; a test pins
  the maintainer's), `target/flatpak/repo`'s `master` metainfo naming the
  version, and for `gh` `gh_preflight` (logged in, no release of the tag
  yet, a final's tag or a dev build's commit on GitHub: the repository
  never goes live without its release being makeable). Then `<zip>.sig`; the Flatpak committed (`flatpak
  build-commit-from`, signed) into the persistent repository on
  `branches_for(prerelease)`, a warning when `permissions` (the
  metadata's sections but `[Application]`/`[Build]`) differ from the
  branch's last commit; `build-update-repo --prune --prune-depth=<keep>`;
  `build-bundle --repo-url --gpg-keys` (beta for a pre-release, else
  stable) over `dist`'s bundle; the Pages tree (`copy_repo` without
  `.lock`, `tmp`, `state`; `.nojekyll`, `index_html`, the
  `.flatpakrepo`, both `.flatpakref`s, the key) as a fresh one-commit
  `git init`, force-pushed to `PAGES_BRANCH`; `live` (`curl`) polled
  every `WAIT_EVERY` until the served `summary` equals the local one
  (`WAIT`, 30 min, else it fails before releasing); then `gh release
  create` (`ReleaseTarget::Gh`; a dev build `--prerelease --target
  <its commit>`, a final `--verify-tag --latest`) or a directory
  (`--release-dir`, `ReleaseTarget::Dir`: the assets, the notes,
  `release.txt`). Tests (skipped without flatpak, ostree or git; unsigned,
  as gpg's agent cannot run in the runner's sandbox): a pre-release then a
  final release with a new permission published into temporary
  directories (a bare git repository as Pages), checking the signature
  with the game's own checks, the branches, the Pages files and its one
  commit, the release; every refusal leaving nothing written; Pages
  never going live leaving no release. Never run it unasked.
- `envelope` (`envelope/`, plan 8, spec
  `docs/superpowers/specs/2026-10-11-performance-envelope-design.md`;
  `cargo xtask envelope [--quick] [--only <part>]... [--gpu
  wsl|windows|both|none] [--bless --reason "<why>"]`, or `--report
  [--from F]`): how far each axis
  goes on this machine before the tick (10 ms p99), frame (6.9 / 16.7 ms
  p99) or memory (2 GiB) budget breaks. The parts (`Axis`, `--only`,
  repeatable; all by default): the sweeps `worms`, `pool`, `fx`, `map`,
  `art`, then `rollback` (plan 9: `olr-cli rollback-cost` at Castle Strike
  depth 12 and 28 and `gen-4096x4096-a1-s1` depth 12, 16 worms; `RollbackOut`
  strict, `Envelope::rollback` lenient and merged like `olx`; total p99
  against 2000 µs / 4000 µs at depth 12, flagged in the report's Rollback
  table), `gpu` and `olx`. `--quick` (`Plan::new`) is one run per
  point and fewer points (about 2 min; the full run, 3 runs a point,
  took 7 min on this machine warm, 2026-10-12: sweeps 1 min, GPU 5 min
  with the Windows build cached, OLX 1 min; a cold Windows build adds a
  few). Modules: `results.rs` (the RON the tools write, read as
  `*Out` with only the fields used and no defaults, so a field renamed
  in a tool is a failed point saying why, never a measured 0; the
  samples in `fixtures/bench/tool-output/` are kept equal to the tools'
  output by `olr-cli`'s `tests/tool_output.rs` and `olr-game`'s
  `frame_log_sample`, and `tool_outputs_parse` reads them; the kept
  `Envelope`, lenient so older files read; the pure
  arithmetic, unit-tested: medians of runs, `Spread::of` nearest rank as
  `olr-cli`'s and `olr-game`'s (ruling PF8), breaking points per budget
  (a map or art point's frame is `Point::frame_p99_ms`: its level's
  `gpu-bench` tick and frame, which replay recorded inputs and so hold
  no route finding, plus the point's bots' think p99), the
  least-squares cost per unit, probe passes that fit, `compare`,
  `merged`); `sweeps.rs` (each point is `olr-cli stress` in release in its
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
  `olr-cli gpu-bench --adapter hardware` offscreen, then `olr-game
  --frame-log` windowed and uncapped at each size; on WSL with GL over
  Mesa's d3d12 driver (`wsl_env`), on Windows through `winbuild` for
  `dx12` and `vulkan`, each run bounded by `winbuild::RUN_TIMEOUT`);
  `olx.rs` (`olx_timing` for OLX and `olr-cli scenario --time` for olr-sim,
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
  and CPU count; another machine's file, or one that does not parse,
  `load_or_notice`, is only a notice): a measurement
  slower than its axis's or GPU path's `noise` (the largest run spread,
  at least `NOISE_FLOOR`, 5 %, also for a stored file without `noise`)
  is flagged with `!`, never failed; stored
  results of another kind (`incomparable`: quick against full, or other
  match lengths) are only a notice. `--bless --reason` (refused with
  `--quick`) writes that file, keeping the parts it did not measure
  from results of the same kind (`kept`; `merged` runs `finish` again,
  so kept sweeps' frames and breaking points follow the GPU levels
  measured now), and logs it in
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
  and `<!-- summary:end -->` is kept word for word (a report that lost
  its markers is refused, `summary_to_keep`, not overwritten). Results also keep
  `gpu-bench`'s `gpu_dropped` counts (frames whose timestamps gave no real
  span). Not part of `full`.
- `olx_timing.rs` (plan 8; its caller is `envelope`, task 7): OLX's
  per-frame physics times. `Olx::open` builds or finds the oracle image
  (`oracle::ensure_image`); `Olx::time` plays a scenario through
  `oracle::record_once_with` (`record_once` with extra `docker -e`
  variables) with `ORACLE_TIMING` and `ORACLE_HEADER_ONLY`
  (`tools/olx-oracle/CLAUDE.md`) in `target/envelope/olx-work/<name>`,
  parses the `frame ns projectiles rc` lines (`parse_timing`) and can
  write the header-only trace (gzipped) for `olr-cli scenario --time`;
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
  goldens) and `fixtures/ui/` (the HUD snapshots, `olr-ui`'s
  `hud_snapshots`) with `OLR_BLESS_RENDER=1`; the GPU ones need an adapter.
- `bless-regression --reason <why> [--from target/failures/<name>.ron]`
  re-records `fixtures/matches/*.ron` (runs `olr-cli --test golden` with
  `OLR_BLESS_REGRESSION=1`); `--from` first copies a failure record in as a
  new golden; a `net-*.ron` record (`olr-cli net-sim`'s) is kept in
  `fixtures/net/cases/` instead (logged with the reason) and `olr-cli --test
  net_cases` runs. Like `bless-render`, it appends the date, the reason and the
  changed files to `fixtures/blessings.log` (`bless::log_blessing`).

## Disk use

- The dev profile (workspace `Cargo.toml`) sets `debug = "line-tables-only"`
  for workspace crates and `debug = false` for dependencies
  (`[profile.dev.package."*"]`). Every test binary links wgpu/winit/egui:
  full debug info once grew `target/` to 45 GB, and the dependencies' line
  tables still let it reach 76 GB. Panic backtraces keep file:line in our
  crates; frames inside dependencies show function names only (symbols are
  not stripped). Nobody steps a debugger; if a debugger session or a
  dependency's lines are ever needed, set `debug = true` for that crate
  locally (do not commit it).
- `oracle-record` prunes after building a new `olr-olx-oracle:<hash>` image:
  it removes every other `olr-olx-oracle:*` image except the current tag and
  `:dev`. Pruning failures only warn. `images_to_prune` (the current tag's
  repository) and `prune_commands` (unit-tested) pick the tags and the
  commands; `net-nat` prunes its `olr-nat:*` images the same way
  (`oracle::prune_old`). The base image it builds on (`ubuntu:26.04`
  here, 162 MB) stays for the next build.
- `tidy` (`tidy.rs`) sweeps `target/` and prunes oracle and `olr-nat`
  images as above (`nat::prune`: none when the current tag cannot be
  computed);
  `tidy --docker-cache` also runs `docker builder prune -f --keep-storage
  2GB`. That cache is the whole machine's (Docker cannot select one
  project's layers), so pruning it is opt-in and nothing else does it.
  The sweep (`sweep`): cargo never deletes a build whose configuration
  changed (profile, features, `Cargo.lock`, toolchain), the new one just
  gets a new hash. In every cargo profile directory under `target/` (one
  holding `.fingerprint`, at most three levels down: `debug`,
  `<triple>/release`, ...), `stale_artifacts` groups `deps/` files into
  builds by `<stem>-<16 hex>` (`split_artifact`; `lib` prefix and
  extension dropped) and keys each build by stem, kind (rlib, proc-macro,
  executable, metadata only), crate root (the `.d` file's first source, a
  registry `name-x.y.z` reduced to its semver series by `version_series`,
  so `syn` 2 and 3 both stay but 2.0.118 is superseded by 2.0.119) and,
  from `.fingerprint/<package>-<hash>/<unit>.json`, unit (`bin-olr-cli`
  vs `test-bin-olr-cli`) and features (so a crate built with two feature
  sets keeps both). It removes every build older than its key's newest by
  more than `SAME_RUN` (10 min: one run's builds alike but for their
  dependencies, e.g. for build scripts and for the game, stay); a build
  without its `.d` goes beside a newer one of its stem and kind.
  `stale_sessions` does the same for `incremental/` sessions per crate
  name (a whole `<crate>-<hash>` directory when all its sessions go; a
  lost session only costs a non-incremental compile). The plain stem is
  not a key: `integration`, `matches` or `no_alloc` test binaries of
  several crates, `syn` 2 and 3, a crate's builds for two feature sets are
  all live at once. Removing a live build only costs a rebuild. Only
  regular files and directories inside `target/` go, never through a
  symbolic link; `content/packs`, `target/olx` and `target/release-zips` (the
  release zips, `tidy::KEPT`: never entered) are never touched.
  Builds of targets that no longer exist (a removed test file) are the
  newest of their key and stay until `cargo clean`. `check` runs the
  sweep first when `target/` exceeds 40 GB (`tidy::SWEEP_BYTES`, GiB; a
  size walk over 10 s counts as over) and prints one line: `tidy target/
  was <n> GB (over 40 GB): freed <n> GB (<n> stale artifacts, <n>
  incremental sessions)`.
- WSL footnote: the Windows drive hosts the virtual disk; `wsl --shutdown`
  then `wsl --manage <distro> --set-sparse true` lets it shrink.

## determinism (`determinism.rs`, plan 9 task 13, spec §9.5)

`cargo xtask determinism [--wasm] [--windows] [--arm64] [--all]`: the golden
bot matches of `fixtures/matches/` (`check`: only `mw-castlestrike-8chaos`;
`--all` and the Windows and arm64 legs: all three) are recorded natively
(`olr-cli match ... --record target/determinism/<stem>.olrp --no-verify`,
flags read from the golden's `spec`), `olr-detcheck` prints their checksum
streams (tick 0, every 10 ticks, the last) and each leg's stream must equal
the native one (`compare`: the first differing `<file> <tick>`, or a length
difference). No leg flag means the WASM leg; a flag runs that leg only.
- WASM (`check`'s `determinism` step, one replay, about 15 s warm): `olr-detcheck`
  for `wasm32-wasip1` run by `node tools/wasi-run.mjs` (Node's built-in WASI,
  the repository root preopened as `/`). Needs `sudo apt install nodejs`.
- Windows (`full`'s `determinism-windows`): plan 8's `winbuild` (`EXES` and
  `build()` include `olr-detcheck`), `stage`, `run` with `RUN_TIMEOUT`.
- arm64 (`full`'s `determinism-arm64`): `olr-detcheck` for
  `aarch64-unknown-linux-gnu` (linker `aarch64-linux-gnu-gcc`, `.cargo/config.toml`)
  under `qemu-aarch64 -L /usr/aarch64-linux-gnu`. Needs `sudo apt install
  qemu-user gcc-aarch64-linux-gnu` and the rustup target.
Every leg also passes `--bake fixtures/edit/composed-level` (`BAKE`;
WASI `/fixtures/…`, Windows a copy in the stage's `bake/`), so the last
line, the level composition's bake digest (plan 14a), must be equal too.
A leg whose tool or target is missing prints `skip <step> (<why and the fix>)`
before anything is recorded; a differing stream fails the step (a real
finding: rollback would desync across those platforms).
`rust-toolchain.toml`'s `targets` name `wasm32-wasip1` and
`aarch64-unknown-linux-gnu` on purpose: a fresh clone downloads them (keep them).
`Spec`/`Golden` deny unknown fields, so a field added to `olr-cli`'s `MatchSpec`
fails the step until `flags` passes it on.
