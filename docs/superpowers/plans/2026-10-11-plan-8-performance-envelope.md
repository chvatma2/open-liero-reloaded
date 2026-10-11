# Plan 8: The Performance Envelope, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure how far rust-liero can go on the user's machine (players, projectiles, effects, map size, art detail, screen resolution, full-screen shader passes) before the tick, frame or memory budget breaks, compare it with OLX, and commit the numbers as a repeatable baseline (`cargo xtask envelope`) with a report (`docs/perf/envelope.md`) that answers the user's headroom question.

**Architecture:**
- **Bench-only knobs, today's defaults.** `rl-fx` gets a pool size other than OLX's 1024 (`Fx::with_capacity`); `rl-cli stress` gets an effects cap, generated in-memory levels (`gen-<w>x<h>-a<scale>-s<seed>`), per-tick timing of the presentation and the draw-list build, a recorded replay and a raised memory ceiling; `rl-render` gets GPU timestamp queries and a shader-headroom probe pass, both off unless asked for. Nothing in `rl-sim`, `rl-modes`, `rl-bots` or `rl-replay` changes.
- **Measuring tools.** `rl-cli gpu-bench` draws a recorded heavy match offscreen on the chosen (hardware) adapter at each resolution, timing CPU and GPU, plus the probe; `rl-game --frame-log` writes a windowed run's frame times. The same `rl-cli`/`rl-game` are cross-compiled for `x86_64-pc-windows-gnu` and run from WSL through interop (DX12 and Vulkan). The oracle's patched OLX writes each frame's time to a file of its own (`ORACLE_TIMING`), and `rl-cli scenario --time` times `rl-sim` on the same scenarios.
- **Orchestration and results.** `cargo xtask envelope [--quick] [--only <axis>] [--gpu wsl|windows|both|none] [--bless --reason] [--report]` runs everything as separate processes, keeps medians of 3 runs, finds each axis's breaking point, compares with `fixtures/bench/envelope/<host>.ron` (flagged beyond each axis's noise, never failed), and `--report` regenerates `docs/perf/envelope.md` around a hand-written summary.

**Tech Stack:** Rust 1.99 (edition 2024), wgpu 30.0.1 (`Features::TIMESTAMP_QUERY`, `QuerySet`, `RenderPassTimestampWrites`), clap, serde/ron, mingw-w64 (`x86_64-w64-mingw32-gcc`, `x86_64-w64-mingw32-objdump`) with the rustup target `x86_64-pc-windows-gnu`, WSL interop (`/mnt/c/Windows/System32/cmd.exe`, `WSLENV`), Docker (the oracle's OLX image), C++ (the OLX patch).

**Spec:** `docs/superpowers/specs/2026-10-11-performance-envelope-design.md` (read all of it). Also read the root `CLAUDE.md`, and the `CLAUDE.md` of every crate a task touches (`crates/rl-fx`, `crates/rl-present`, `crates/rl-cli`, `crates/rl-render`, `crates/rl-game`, `xtask`, `tools/olx-oracle`) before editing it.

**Content directory.** The converted packs live in the committed `content/packs/` (the default `--content` of `rl-cli` and `rl-game`, and `RL_CONTENT_DIR`). That migration is done outside this plan; wherever this plan says "the content", it means `content/packs` (in `xtask`: `root().join("content/packs")`).

## Global Constraints

- **Budgets (spec §3).** Tick: the simulation tick, bots' thinking included, p99 ≤ 10 ms (OLX's fixed 100 Hz frame, `FRAME_MS`); also report the share of the budget used. Frame: the whole frame (CPU work and GPU time) p99 ≤ 6.9 ms (144 Hz) and ≤ 16.7 ms (60 Hz); both reported. Memory: peak resident below 2 GiB (reported, not a hard stop below the existing `MEMORY_CEILING_MIB`). The breaking point of an axis is its first value that does not hold, with the cause (tick, frame, memory, or a cap reached).
- **Reference configuration (spec §4).** Classic on Castle Strike, 16 chaos bots of mixed difficulty, OLX's pool, 20 s of game time, seed 1. Each sweep varies one axis; each point runs 3 times and keeps the medians.
- **Axes (spec §4, with ruling P2 below).** Worms 2, 4, 8, 16, 24, 32 (32 is `MAX_WORMS`; no sweep past 32, the report projects the cost per worm). Projectile pool 3000, 10 000, 20 000, 50 000, 65 536 on MW 1.0. Effects cap 1024, 4096, 16 384, 65 536. Map size 1024², 2048², 4096 × 2048, 4096² (generated in memory, seeded noise caves, no pack written). Art scale 1, 2, 4, 8 at 2048².
- **GPU (spec §5).** Real GPU time from wgpu timestamp queries when the adapter offers `Features::TIMESTAMP_QUERY`, else CPU-side frame time only, said so in the results. Scenes: the heavy recorded match at 1920 × 1080, 2560 × 1440, 3440 × 1440 and 3840 × 2160, windowed and uncapped (offscreen at sizes the monitor lacks), and the map and art sweeps' levels. Two paths: WSL (wgpu GL over Mesa's d3d12 driver) and a native Windows build (`x86_64-pc-windows-gnu`, run from WSL through interop) on DX12 and Vulkan. The shader-headroom probe: a bench-only full-screen fragment pass (a blur of k taps) run K times after the frame; report the GPU ms per pass at each resolution and how many fit in the 144 Hz and 60 Hz budgets.
- **OLX (spec §6).** The oracle's patched OLX writes each simulated frame's time to a separate file; the traces stay byte-identical (`cargo xtask oracle-record --check` passes). `rl-cli scenario` times the same scenarios in `rl-sim`. Heavy scenarios fill OLX's 3000 projectiles. Docker overhead noted. A limits table (worms, projectiles, map side, effects) beside ours.
- **The command (spec §7).** `cargo xtask envelope [--quick] [--only <axis>] [--gpu wsl|windows|both]`; `--bless --reason "<why>"` writes `fixtures/bench/envelope/<host>.ron` (the host fingerprint as `baselines.ron` has it; another machine's file is only a notice) and logs the reason in `fixtures/blessings.log`. Not part of `cargo xtask full`. A regression beyond each axis's noise (measured at bless time from the 3 runs) is flagged, not failed. `docs/perf/envelope.md` is regenerated from the results file, with the summary written by hand.
- **No behaviour change (spec §8).** Oracle traces, match goldens (`fixtures/matches/`) and render goldens (`fixtures/render/`, `fixtures/ui/`) stay byte-identical. New knobs (effects cap, generated levels, the probe pass, GPU timing) are bench-only options whose defaults are today's behaviour. `rl-sim`'s own 1024-entity mirror (`crates/rl-sim/src/rng.rs`, OLX's `rand()` draws) is never touched.
- **Purity (spec §8).** The simulation stays `#![no_std]` and builds for `wasm32-unknown-unknown`; timing lives only in `rl-cli`, `rl-render` (behind an option), `rl-game` (behind a flag) and `xtask`.
- **Idle machine (spec §8; the user confirmed the PC is idle for the runs).** The command says so and records the load average before each point.
- **Windows (spec §8).** Nothing is installed on Windows. mingw-w64 is already installed in WSL; a task may run `rustup target add x86_64-pc-windows-gnu` (from the repository, so it applies to the pinned 1.99 toolchain).
- **Players (user decision).** 32 is the maximum swept; the report projects the per-worm cost past 32. `MAX_WORMS` is not raised.
- **Out of scope (spec §9).** Optimizations (cheap fixes found are listed for a later plan), raising `MAX_WORMS`, network costs, the browser build.
- **Project rules.** Run `cargo xtask check` before every commit (it prints only failures). Run `cargo xtask full` before committing rendering changes (Task 3) and at the end (Task 9). Every crate's `CLAUDE.md` is updated with what its task changes. Never panic on content. Keep output deterministic: `BTreeMap`, never `HashMap` iteration, in results and reports. Learn dependency APIs from `cargo xtask api` and `target/api/<crate>.txt`.
- **Commits.** On `main` (the user's rule), never pushed. Small imperative subjects such as `rl-cli: add the envelope sweeps`. Every commit message ends with exactly these two lines:

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Rulings (where the spec is silent or the code disagrees)

| # | Ruling | Why |
|---|---|---|
| P1 | The reference configuration is `rl_cli::stress::spec` with 16 worms: chaos bots of mixed difficulty, `LoadingTime` 0, bonuses every 5 s, no kill or time limit, seed 1. | The spec says the sweeps extend `rl-cli stress`; its spec is the stress match. |
| P2 | The pool axis ends at 65 536, not 100 000. | `MatchSetup::MAX_POOL_CAPACITY` is `1 << 16` (`crates/rl-sim/src/setup.rs:80`); `World::new` refuses more. Raising it is a simulation change (out of scope); the report says so. |
| P3 | Generated levels bypass the content loader's caps. A point above `MAX_COLOR_MAP_BYTES` (256 MiB: 2048² at art scale 8) still runs, and the cap is recorded as its breaking cause. | The question is what raising the cap would cost; the cap is reported, not hidden. |
| P4 | Sweeps are headless (tick, presentation, draw-list build, memory). The frame budget is judged in the GPU scenes: the heavy replay at four sizes, and each map and art point's replay at 2560 × 1440 on the WSL path. A sweep's breaking point uses the 144 Hz frame budget; the 60 Hz one is reported beside it. | A headless sweep has no frame; the spec asks for both budgets. |
| P5 | Windowed runs (`rl-game --frame-log`) measure wall frame times only; GPU timestamps are taken offscreen (`rl-cli gpu-bench`). Windowed runs are one run each. | Reading timestamps back every frame would serialise the swap chain and change what is measured. |
| P6 | The colour-terrain edits are timed inside the presentation tick (`present_us`). | `rl_fx::Fx::step_tick` interleaves them with the entity passes (OLX order); splitting them would change the code path. |
| P7 | An effects or pool cap reached at its own value is "saturated" (shown, not a break). Hard caps break a point: a content cap (P3), the bots' area cache full (`AreaCache::DEFAULT_CAPACITY`, 4096 areas), a terrain cropped to the device's texture limit, or the run failing (crash, memory ceiling). | Reaching the pool's size is the axis's own value, not a failure to hold the budget. |
| P8 | OLX's timed region is from the end of `OracleHooks::beforeFrame()` to the start of `afterFrame()`: gus logic, the client's and the server's frame (OLX's whole simulated frame). Timed scenarios: six existing busy ones (`TIMED`) and two new heavy ones in `fixtures/perf/olx/` (not under `fixtures/oracle/`, so the oracle tests never see them; no committed trace). For every scenario OLX writes a header-only trace (`ORACLE_HEADER_ONLY`) and "the same work" is checked per frame by equal projectile counts and `rand()` calls. | Heavy traces would be hundreds of MB; the per-frame counts prove the same work cheaply. |
| P9 | Envelope runs skip the determinism replay (`--no-verify`); `--quick` is one run per point. | Determinism is gated by `check` and `full`; it would double the run time. |
| P10 | The results file is `fixtures/bench/envelope/<slug of the CPU model>-<n>cpus.ron`; its stored `Host` is compared in full (`bench::host_notice`), as `baselines.ron` is. | Same fingerprint as the benchmarks. |
| P11 | GPU scenes draw `Viewport::GAME` (427 × 192 world pixels) over the whole target with the default sharp-bilinear filter (no HUD; the windowed runs have the HUD). | The world pass is the same for every resolution; only the upscale and the probe scale with it. |
| P12 | Windows binaries and a copy of the content (about 21 MB) are staged in `%TEMP%\rust-liero-envelope` and run with that as their working directory and relative paths. `rl-cli` picks the backend with `--backend`; `rl-game` gets `WGPU_BACKEND` through `WSLENV`. | WSL interop forwards only `WSLENV`'s variables; relative paths avoid UNC paths. |
| P13 | Sweep runs pass `--memory-ceiling-mib 16384`, so points past the 2 GiB budget are measured (this PC has 21 GiB in WSL). | The 4 GiB stop would hide the art axis's cost. |
| P14 | OLX's rendering is not timed; the report says so. | The oracle's OLX is a dedicated-only build (`-DDEDICATED_ONLY=Yes`): it has no renderer, so it cannot be done cheaply (spec §6 allows leaving it out). |

## Findings that change the spec's assumptions

1. **WSL's GL path does offer timestamps.** `glxinfo` on this PC (`GALLIUM_DRIVER=d3d12`, NVIDIA GeForce RTX 5070 Ti) lists `GL_ARB_timer_query`, which wgpu-hal's GL backend maps to `TIMESTAMP_QUERY` (`wgpu-hal-30.0.1/src/gles/adapter.rs:530`). The CPU fallback stays for other machines.
2. **WSL has no hardware Vulkan** (only llvmpipe), and `Offscreen::new()` prefers software rasterisers on purpose (goldens). The GPU path needs `Offscreen::open` with a hardware preference and `GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl` (plus `MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA`), as `rl-game` sets for itself.
3. **The pool's hard limit is 65 536** (P2).
4. **Art scale 8 at 2048²** is refused by `load_level` (256 MiB cap) and needs about 3 GiB of colour map (`ColorMap` keeps three images) plus 2 GiB of level art; the GL limit `GL_MAX_TEXTURE_SIZE` is 16 384, exactly 2048 × 8, so the GPU can hold it.
5. **Resolution barely moves the frame cost.** The world pass draws only the view (427 × 192 world pixels) at art scale, then one upscale pass fills the output; resolution changes the upscale (and the HUD), which is why the shader probe is what scales.
6. **WSLg presents at 60 Hz FIFO on Wayland**; uncapped windowed runs on WSL need `--window-system x11`.
7. **Interop details on this PC:** `appendWindowsPath = false` (so `cmd.exe` is only at `/mnt/c/Windows/System32/cmd.exe`), `%TEMP%` is `C:\Users\Martin\AppData\Local\Temp`, interop is enabled.
8. **The bots' area cache holds 4096 areas** (`AreaCache::DEFAULT_CAPACITY`); 4096² maps may fill it (P7).
9. **OLX in the oracle image is a Release build** (`-DDEBUG=No` → `CMAKE_BUILD_TYPE Release`, `-O3`), so the physics comparison is fair; the report records the compiler line.

## Review Focus

1. **A sweep point that crashes, runs out of memory or exceeds the memory ceiling** (art scale 8 needs about 6 GiB): the point must be recorded with that cause as its breaking point and the sweep must go on, never abort a 45-minute run. Pinned by `a_failed_point_is_a_breaking_cause` (Task 7).
2. **A missing prerequisite** (no Windows interop, the rustup target not installed, Docker not running, no display for windowed runs, or only a software adapter where the GPU was asked for): that part is skipped with a notice naming the fix and the rest is still measured and written; llvmpipe must never be reported as "the GPU". Pinned by `a_software_adapter_is_refused_for_hardware_runs` (Task 4) and `prerequisites_are_notices` (Task 7).
3. **Results from another machine, or an older results file without the newer fields**: a notice, never a parse failure or a false regression. Pinned by `another_hosts_results_only_notice` and `old_results_parse_with_defaults` (Task 7).
4. **Run-to-run noise**: a slower point within its axis's noise is not flagged, one beyond it is flagged, an improvement is never flagged, and nothing fails the command. Pinned by `regressions_beyond_noise_are_flagged_not_failed` (Task 7).
5. **Regenerating the report** must keep the hand-written headroom summary word for word. Pinned by `regenerating_keeps_the_hand_written_summary` (Task 8).

## File structure after this plan

```text
crates/rl-fx/src/{lib.rs,pool.rs}           Fx::with_capacity, Fx::capacity (pool size a bench knob)
crates/rl-present/src/lib.rs                Presentation::set_fx_capacity
crates/rl-cli/src/stress.rs                 Config::new, fx cap, build-every, present/build timing, area cache, caps, record, memory ceiling
crates/rl-cli/src/genlevel.rs (new)         GenLevel: gen-<w>x<h>-a<s>-s<seed> noise-cave levels in memory
crates/rl-cli/src/assets.rs (new)           LevelAssets: a pack's or a generated level's match assets; replay worlds
crates/rl-cli/src/runner.rs, verify.rs      Match::with_assets; replay round trip through LevelAssets
crates/rl-cli/src/gpu_bench.rs (new)        the offscreen GPU measurement (library)
crates/rl-cli/src/gpu_bench_cmd.rs (new)    `rl-cli gpu-bench`
crates/rl-cli/src/timing.rs (new)           time_scenario: rl-sim per-frame times of an oracle scenario
crates/rl-cli/src/scenario.rs               `rl-cli scenario --time <file>`
crates/rl-render/src/offscreen.rs           OffscreenOptions (hardware preference, backends, timestamps), list_adapters
crates/rl-render/src/renderer/timing.rs (new)  GPU timestamps: GpuTimes, gpu_times
crates/rl-render/src/renderer/probe.rs (new), probe.wgsl (new)  the shader-headroom probe
crates/rl-game/src/{stats.rs,app.rs,main.rs} --frame-log, --frame-log-skip
tools/olx-oracle/oracle-hooks.patch         ORACLE_TIMING, ORACLE_HEADER_ONLY
fixtures/perf/olx/heavy-*.ron (new)         heavy OLX timing scenarios
xtask/src/winbuild.rs (new)                 the Windows cross build, staging and interop runs; `cargo xtask windows`
xtask/src/olx_timing.rs (new)               OLX timed runs in Docker
xtask/src/oracle.rs                         record_once_with (environment for OLX)
xtask/src/envelope/{mod.rs,results.rs,sweeps.rs,gpu.rs,olx.rs,report.rs} (new)  `cargo xtask envelope`
fixtures/bench/envelope/<host>.ron (new)    the blessed results
docs/perf/envelope.md (new)                 the report
```

## Task overview

1. `rl-fx` pool size and the stress runner's new measurements
2. Generated levels for the map and art sweeps
3. `rl-render`: GPU timestamps, the shader-headroom probe, hardware offscreen
4. `rl-cli gpu-bench` and `rl-game --frame-log`
5. The Windows build run from WSL (`cargo xtask windows`)
6. OLX frame timing (`ORACLE_TIMING`) and `rl-cli scenario --time`
7. `cargo xtask envelope`: sweeps, GPU paths, OLX, results, bless
8. The report: `docs/perf/envelope.md` from the results
9. Run the envelope on this machine, bless, write the headroom summary, docs

---
### Task 1: `rl-fx` pool size and the stress runner's new measurements

**Files:**
- Modify: `crates/rl-fx/src/lib.rs:27-80` (`Fx::new`, new `Fx::with_capacity`, `Fx::capacity`), `crates/rl-fx/src/pool.rs` (capacity from the slots, not the constant)
- Modify: `crates/rl-present/src/lib.rs` (new `Presentation::set_fx_capacity`)
- Modify: `crates/rl-cli/src/stress.rs` (`Config::new`, new fields, new `Report` fields, timing), `crates/rl-cli/src/runner.rs` (`Match::cache_capacity`), `crates/rl-cli/src/stress_cmd.rs` (flags)
- Test: `crates/rl-fx/tests/entities.rs`, `crates/rl-fx/src/pool.rs` (unit), `crates/rl-cli/tests/stress.rs`
- Docs: `crates/rl-fx/CLAUDE.md`, `crates/rl-present/CLAUDE.md`, `crates/rl-cli/CLAUDE.md`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `rl_fx::Fx::with_capacity(seed: u32, capacity: usize) -> Fx` (capacity at least 1), `Fx::capacity(&self) -> usize`; `Fx::new(seed)` is `with_capacity(seed, MAX_ENTITIES)`.
  - `rl_present::Presentation::set_fx_capacity(&mut self, capacity: usize)` (before the first tick).
  - `rl_cli::stress::Config::new(spec: MatchSpec) -> Config` and the fields `fx_capacity: usize`, `build_every: u32`, `memory_ceiling_mib: f64`, `record: Option<PathBuf>` (plus the old `render_every`, `render_size`, `verify`).
  - `rl_cli::stress::Report` new fields (all `#[serde(default)]`): `present_us: Spread`, `build_us: Spread`, `fx_capacity: usize`, `area_cache: (usize, usize)`, `level: (u32, u32, u32)`, `caps: Vec<String>`.
  - `rl_cli::runner::Match::cache_capacity(&self) -> usize`.
  - `rl-cli stress` flags `--fx-cap N` (default 1024), `--build-every N` (default 0), `--memory-ceiling-mib M` (default 4096), `--record FILE`.

- [ ] **Step 1: Write the failing `rl-fx` tests**

In `crates/rl-fx/tests/entities.rs`, change the import on line 8 to `use rl_fx::{Fx, Kind, MAX_ENTITIES};` and add after `entity_cap_drops_new_ones`:

```rust
/// The pool's size is a bench-only knob (plan 8): a larger pool keeps
/// entities OLX's 1024 would drop; the default stays OLX's.
#[test]
fn a_larger_bench_pool_keeps_more_entities() {
    let mut s = Setup::new();
    s.fx = Fx::with_capacity(7, 4 * MAX_ENTITIES);
    let smoke = cosmetic(0, CosmeticKind::Smoke, (20.0, 20.0), (0.0, 0.0));
    s.tick(&vec![smoke; MAX_ENTITIES + 6], 0);
    assert_eq!(s.fx.len(), MAX_ENTITIES + 6);
    assert_eq!(s.fx.capacity(), 4 * MAX_ENTITIES);
    assert_eq!(Fx::new(7).capacity(), MAX_ENTITIES);
    assert_eq!(Fx::with_capacity(7, 0).capacity(), 1, "never an empty pool");
}
```

In `crates/rl-fx/src/pool.rs`'s `mod tests`, add:

```rust
    #[test]
    fn a_pool_of_any_size_fills_to_it() {
        let mut p = Pool::with_capacity(3);
        assert_eq!(p.capacity(), 3);
        for i in 0..3 {
            assert_eq!(p.add(smoke()), Some(i));
        }
        assert!(p.is_full());
        assert_eq!(p.add(smoke()), None);
        assert_eq!(Pool::new().capacity(), MAX_ENTITIES);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p rl-fx`
Expected: FAIL to compile with "no function or associated item named `with_capacity` found for struct `Fx`" (and for `Pool`).

- [ ] **Step 3: Implement the pool size**

In `crates/rl-fx/src/pool.rs`, replace `pub fn new() -> Pool { ... }` with:

```rust
    /// OLX's pool: [`MAX_ENTITIES`] slots.
    pub fn new() -> Pool {
        Pool::with_capacity(MAX_ENTITIES)
    }

    /// A pool of `capacity` slots (a bench-only size, plan 8; at least 1).
    pub fn with_capacity(capacity: usize) -> Pool {
        let blank = Entity {
            kind: Kind::Smoke,
            type2: 0,
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            colour: Rgba::BLACK,
            frame: 0.0,
            extra: TimeDiff(0),
            angl_vel: 0.0,
            rotation: 0,
        };
        let capacity = capacity.max(1);
        Pool { slots: vec![blank; capacity], used: vec![false; capacity], first_unused: 0, last_used: -1 }
    }

    /// How many slots it has.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
```

and replace each `MAX_ENTITIES` inside `is_full` and `add` (three places: `self.first_unused >= MAX_ENTITIES`) with `self.slots.len()`. Keep `use crate::MAX_ENTITIES;` (used by `new` and the tests). Update the struct doc: "A fixed array of slots ([`MAX_ENTITIES`], OLX's, unless a bench sizes it)".

In `crates/rl-fx/src/lib.rs`, replace `Fx::new` with:

```rust
    /// No entities, OLX's pool of [`MAX_ENTITIES`]; `seed` seeds the
    /// entities' own `rand()`.
    pub fn new(seed: u32) -> Fx {
        Fx::with_capacity(seed, MAX_ENTITIES)
    }

    /// [`Fx::new`] with a pool of `capacity` entities (at least 1): a
    /// bench-only knob (`rl-cli stress --fx-cap`, plan 8). Every game keeps
    /// OLX's [`MAX_ENTITIES`].
    pub fn with_capacity(seed: u32, capacity: usize) -> Fx {
        Fx { pool: Pool::with_capacity(capacity), rand: GlibcRand::new(seed) }
    }

    /// The pool's size.
    pub fn capacity(&self) -> usize {
        self.pool.capacity()
    }
```

Update the doc of `MAX_ENTITIES`: "`MAX_ENTITIES` (`include/Entity.h:30`): new entities are dropped beyond it. The pool's size everywhere but the performance envelope's effects sweep (`Fx::with_capacity`)."

- [ ] **Step 4: Run the `rl-fx` tests**

Run: `cargo nextest run -p rl-fx`
Expected: PASS (all tests, including `a_larger_bench_pool_keeps_more_entities` and `a_pool_of_any_size_fills_to_it`).

- [ ] **Step 5: `Presentation::set_fx_capacity`**

In `crates/rl-present/src/lib.rs`, add to `impl Presentation` after `new`:

```rust
    /// A cosmetic-entity pool of `capacity` instead of OLX's 1024 (bench
    /// only: the performance envelope's effects sweep, plan 8). Call it
    /// before the first [`Presentation::on_tick`]: the entities so far go.
    pub fn set_fx_capacity(&mut self, capacity: usize) {
        self.fx = Fx::with_capacity(self.seed.wrapping_add(2), capacity);
    }
```

- [ ] **Step 6: Write the failing stress tests**

In `crates/rl-cli/tests/stress.rs`, change the import to `use rl_cli::stress::{self, Config, Spread};`, change the existing test's config to `let cfg = Config { render_size: (64, 36), ..Config::new(spec) };` (the other fields of the old literal are `Config::new`'s defaults), and add:

```rust
/// The envelope's knobs (plan 8) measure more without touching the match:
/// the same shots, kills and projectiles with a larger effects pool and a
/// draw list built every tick; the recorded replay is the match.
#[test]
fn the_bench_knobs_measure_without_changing_the_match() {
    let Some(content) = content() else { return };
    let spec = stress::spec("olx-mw-1-0", "olx-level-dirt-level", 32, 3000, 8.0, 3);
    let plain = stress::run(&content, &Config { verify: false, ..Config::new(spec.clone()) }, &mut |_| {}).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("stress.rlrp");
    let cfg = Config {
        verify: false,
        fx_capacity: 16_384,
        build_every: 1,
        record: Some(file.clone()),
        ..Config::new(spec)
    };
    let r = stress::run(&content, &cfg, &mut |_| {}).unwrap();
    assert_eq!(
        (r.shots, r.kills, r.projectiles_peak, r.ticks),
        (plain.shots, plain.kills, plain.projectiles_peak, plain.ticks),
        "the knobs never touch the simulation"
    );
    assert_eq!((r.fx_capacity, plain.fx_capacity), (16_384, rl_fx::MAX_ENTITIES));
    assert!(r.fx_peak > rl_fx::MAX_ENTITIES, "32 chaos bots overflow OLX's 1024 entities: {}", r.fx_peak);
    assert!(r.build_us.p50 > 0.0 && r.present_us.p50 > 0.0, "{r:?}");
    assert_eq!(plain.build_us, Spread::default(), "no draw list without --build-every");
    assert!(r.area_cache.1 > 0 && r.area_cache.0 <= r.area_cache.1);
    assert!(r.level.0 > 0 && r.level.1 > 0 && r.level.2 >= 1);
    assert!(r.caps.is_empty(), "{:?}", r.caps);
    let replay = rl_replay::Replay::decode(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(replay.ticks(), r.ticks);
}
```

- [ ] **Step 7: Run them to see them fail**

Run: `cargo nextest run -p rl-cli --test stress`
Expected: FAIL to compile with "no function or associated item named `new` found for struct `Config`" and "struct `Config` has no field named `fx_capacity`".

- [ ] **Step 8: Implement the runner's knobs and measurements**

In `crates/rl-cli/src/runner.rs`, add after `cache_len`:

```rust
    /// The bots' shared area cache's capacity (0 without bots).
    pub fn cache_capacity(&self) -> usize {
        self.bots.first().map_or(0, |b| b.cache().capacity())
    }
```

In `crates/rl-cli/src/stress.rs`:

1. Add `use std::path::PathBuf;` and `use rl_scene::DrawList;` stays (already imported).
2. Replace the `Config` struct with:

```rust
/// How a run is played.
#[derive(Clone, Debug)]
pub struct Config {
    pub spec: MatchSpec,
    /// Render a frame offscreen every this many ticks (0: never).
    pub render_every: u32,
    /// The frame's size in pixels.
    pub render_size: (u32, u32),
    /// Play it again and replay its recording (determinism).
    pub verify: bool,
    /// The cosmetic entities' pool (OLX's [`MAX_ENTITIES`] unless the
    /// envelope's effects sweep raises it).
    pub fx_capacity: usize,
    /// Build the draw list every this many ticks, timed as
    /// [`Report::build_us`] (0: only for the rendered frames).
    pub build_every: u32,
    /// Peak memory above this fails the run (the envelope raises it so
    /// points past the 2 GiB budget are still measured).
    pub memory_ceiling_mib: f64,
    /// Write the match's replay here (the GPU scenes play it).
    pub record: Option<PathBuf>,
}

impl Config {
    /// `spec` with today's defaults: no frames, verified, OLX's entity
    /// pool, no draw list, [`MEMORY_CEILING_MIB`], no recording.
    pub fn new(spec: MatchSpec) -> Config {
        Config {
            spec,
            render_every: 0,
            render_size: (2560, 1440),
            verify: true,
            fx_capacity: MAX_ENTITIES,
            build_every: 0,
            memory_ceiling_mib: MEMORY_CEILING_MIB,
            record: None,
        }
    }
}
```

3. Add to `Report`, after `think_us`:

```rust
    /// The presentation's tick (`Presentation::on_tick`: the entities,
    /// which replay the tick's colour-terrain edits, then the scene), µs.
    #[serde(default)]
    pub present_us: Spread,
    /// The draw list's build (`Presentation::build`) every
    /// `Config::build_every` ticks, µs.
    #[serde(default)]
    pub build_us: Spread,
```

and after `ticks_at_fx_cap`:

```rust
    /// The entity pool's size.
    #[serde(default)]
    pub fx_capacity: usize,
    /// The bots' shared area cache: areas held, and how many it can hold.
    #[serde(default)]
    pub area_cache: (usize, usize),
    /// The level: width and height in world pixels, art scale.
    #[serde(default)]
    pub level: (u32, u32, u32),
    /// Hard limits the run reached (the bots' area cache full, a level the
    /// content loader would refuse): each breaks an envelope point.
    #[serde(default)]
    pub caps: Vec<String>,
```

4. In `Report::text`, replace `cosmetic entities: peak {} of {MAX_ENTITIES}` with `cosmetic entities: peak {} of {}` and pass `self.fx_capacity` after `self.fx_peak`; append after the format (before the render match):

```rust
        let s3 = |x: Spread| format!("p50 {:.0} p99 {:.0} max {:.0}", x.p50, x.p99, x.max);
        t.push_str(&format!("  presentation µs:     {}\n", s3(self.present_us)));
        if self.build_us != Spread::default() {
            t.push_str(&format!("  draw-list build µs:  {}\n", s3(self.build_us)));
        }
        t.push_str(&format!(
            "  level {}x{} at art scale {}; area cache {} of {}\n",
            self.level.0, self.level.1, self.level.2, self.area_cache.0, self.area_cache.1
        ));
        for c in &self.caps {
            t.push_str(&format!("  cap reached: {c}\n"));
        }
```

5. In `run`, right after `let mut present = Presentation::new(&setup, m.world());` add `present.set_fx_capacity(cfg.fx_capacity);` and before the loop:

```rust
    let (mut present_us, mut build_us) = (Vec::new(), Vec::new());
    let mut built = DrawList::new();
```

Inside the loop replace `present.on_tick(w, ev);` with:

```rust
            let p0 = Instant::now();
            present.on_tick(w, ev);
            present_us.push(p0.elapsed().as_secs_f64() * 1e6);
```

replace the bounds check `present.fx.len() > MAX_ENTITIES` with `present.fx.len() > cfg.fx_capacity` (and `{MAX_ENTITIES}` in its message with `{}` plus `cfg.fx_capacity`), replace `present.fx.len() >= MAX_ENTITIES` with `present.fx.len() >= cfg.fx_capacity`, and replace `peak > MEMORY_CEILING_MIB` / its message with `cfg.memory_ceiling_mib`. After the render block (still inside `if m.ticks() > before`) add:

```rust
            if cfg.build_every > 0 && t.is_multiple_of(cfg.build_every) {
                let b0 = Instant::now();
                present.build(w, 1.0, &mut built);
                build_us.push(b0.elapsed().as_secs_f64() * 1e6);
            }
```

After the loop, before `let run = m.finish();`, take what `finish` consumes:

```rust
    let area_cache = (m.cache_len(), m.cache_capacity());
```

After the failure check (`if let Some(f) = run.failure() { ... }`), add:

```rust
    if let Some(path) = &cfg.record {
        std::fs::write(path, run.replay.encode()).with_context(|| format!("writing {}", path.display()))?;
    }
```

and with the other report fields:

```rust
    r.present_us = Spread::of(&present_us);
    r.build_us = Spread::of(&build_us);
    r.fx_capacity = cfg.fx_capacity;
    r.area_cache = area_cache;
    r.level = (level.material.width(), level.material.height(), level.art_scale);
    if area_cache.1 > 0 && area_cache.0 >= area_cache.1 {
        r.caps.push(format!("the bots' area cache is full ({} areas)", area_cache.1));
    }
```

(Task 2 replaces `level` with the assets' level; here it is the `level` this function already loads.)

6. In `crates/rl-cli/src/stress_cmd.rs`, add to `Args`:

```rust
    /// The cosmetic entities' pool (OLX's 1024; the envelope raises it).
    #[arg(long, default_value_t = rl_fx::MAX_ENTITIES)]
    fx_cap: usize,
    /// Build the draw list every this many ticks, timed (0: only for rendered frames).
    #[arg(long, default_value_t = 0)]
    build_every: u32,
    /// Fail when peak memory passes this many MiB.
    #[arg(long, default_value_t = stress::MEMORY_CEILING_MIB)]
    memory_ceiling_mib: f64,
    /// Write the match's replay here.
    #[arg(long)]
    record: Option<PathBuf>,
```

and build the config as:

```rust
    let cfg = Config {
        render_every: args.render_every,
        render_size: (args.render_width.max(1), args.render_height.max(1)),
        verify: !args.no_verify,
        fx_capacity: args.fx_cap.max(1),
        build_every: args.build_every,
        memory_ceiling_mib: args.memory_ceiling_mib,
        record: args.record.clone(),
        ..Config::new(spec)
    };
```

(`rl-fx` is already a dependency of `rl-cli`.)

- [ ] **Step 9: Run the stress tests**

Run: `cargo nextest run -p rl-cli --test stress`
Expected: PASS (`stress_run_is_bounded_and_deterministic`, `the_bench_knobs_measure_without_changing_the_match`). If `fx_peak > MAX_ENTITIES` fails, print `r.fx_peak` and `plain.ticks_at_fx_cap`: if the default run never reached 1024 either, raise the test's seconds from 8.0 to 15.0; do not lower the assertion.

- [ ] **Step 10: Check that today's behaviour did not move**

Run: `cargo run --release -p rl-cli -- stress --mod olx-classic --level olx-level-castlestrike --worms 32 --seconds 5 --render-every 0 --no-verify`
Expected: the report prints `cosmetic entities: peak 1024 of 1024`, a `presentation µs:` line and `area cache N of 4096`; no `draw-list build` line.

- [ ] **Step 11: Docs**

- `crates/rl-fx/CLAUDE.md` **Pool** bullet: add "Its size is `MAX_ENTITIES` (OLX's) everywhere; `Fx::with_capacity` is the performance envelope's knob (plan 8), never used by a game."
- `crates/rl-present/CLAUDE.md` **Use**: add "`set_fx_capacity(n)` before the first tick: a bench-only entity pool (plan 8)."
- `crates/rl-cli/CLAUDE.md` stress entry: add the four flags and the new `Report` fields (`present_us`, `build_us`, `fx_capacity`, `area_cache`, `level`, `caps`) and `Config::new`.

- [ ] **Step 12: Gate and commit**

Run: `cargo xtask check`
Expected: `check passed` (golden matches and oracle tests unchanged).

```bash
git add crates/rl-fx crates/rl-present crates/rl-cli
git commit -F - <<'EOF'
rl-cli: time the presentation and size the effects pool in stress runs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

### Task 2: Generated levels for the map and art sweeps

**Files:**
- Create: `crates/rl-cli/src/genlevel.rs`, `crates/rl-cli/src/assets.rs`
- Modify: `crates/rl-cli/src/lib.rs` (modules), `crates/rl-cli/src/runner.rs:205-230` (`Match::new` → `Match::with_assets`), `crates/rl-cli/src/verify.rs` (replay through `LevelAssets`), `crates/rl-cli/src/stress.rs` (assets loaded once, the content cap)
- Test: unit tests in `genlevel.rs`; `crates/rl-cli/tests/stress.rs`
- Docs: `crates/rl-cli/CLAUDE.md`

**Interfaces:**
- Consumes: Task 1's `stress::Config::new`, `Report::level`, `Report::caps`.
- Produces:
  - `rl_cli::genlevel::GenLevel { width: u32, height: u32, art_scale: u32, seed: u32 }` with `id(&self) -> String` (`gen-<w>x<h>-a<s>-s<seed>`), `parse(id: &str) -> Option<GenLevel>`, `content_cap(&self) -> Option<String>`, `material(&self) -> MaterialGrid`, `level(&self, theme: &str) -> LoadedLevel`; constants `THEME_LEVEL = "olx-level-castlestrike"`, `MAX_SIDE = 4096`, `PREFIX = "gen-"`.
  - `rl_cli::assets::LevelAssets { m: LoadedMod, level: LoadedLevel, theme: LoadedTheme, packs: Packs, generated: Option<GenLevel> }` with `load(content: &Content, mod_id: &str, level_id: &str) -> anyhow::Result<LevelAssets>`, `match_assets(&self) -> MatchAssets<'_>`, `replay_world(&self, content: &Content, replay: &Replay) -> anyhow::Result<World>`.
  - `rl_cli::runner::Match::with_assets(assets: &LevelAssets, spec: &MatchSpec, opts: &Options) -> anyhow::Result<Match>`.
  - `rl-cli stress --level gen-2048x2048-a1-s1` works; so does `rl-cli match --level gen-...`.

- [ ] **Step 1: Write the failing generator tests**

Create `crates/rl-cli/src/genlevel.rs` with only the tests first (the module body follows in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::level::Material;

    #[test]
    fn ids_round_trip_and_bad_ones_are_refused() {
        let g = GenLevel { width: 2048, height: 1024, art_scale: 2, seed: 7 };
        assert_eq!(g.id(), "gen-2048x1024-a2-s7");
        assert_eq!(GenLevel::parse(&g.id()), Some(g));
        for bad in [
            "olx-level-castlestrike",
            "gen-2048x1024-a2",
            "gen-2048x1024-a2-s7-x",
            "gen-8192x1024-a1-s1",
            "gen-32x1024-a1-s1",
            "gen-2048x1024-a9-s1",
            "gen-2048x1024-a0-s1",
            "gen-axb-a1-s1",
        ] {
            assert_eq!(GenLevel::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn caves_are_seeded_and_open() {
        let g = GenLevel { width: 256, height: 128, art_scale: 1, seed: 1 };
        let m = g.material();
        assert_eq!((m.width(), m.height()), (256, 128));
        assert_eq!(m, g.material(), "the same seed, the same level");
        assert_ne!(m, GenLevel { seed: 2, ..g }.material());
        let open = m.count(Material::Background) as f64 / m.cells().len() as f64;
        assert!((0.2..0.8).contains(&open), "background share {open}");
        assert!(m.count(Material::Rock) > 0 && m.count(Material::Dirt) > 0);
    }

    #[test]
    fn art_is_at_its_scale() {
        let g = GenLevel { width: 64, height: 64, art_scale: 3, seed: 1 };
        let l = g.level("olx-theme-x");
        assert_eq!((l.front.width(), l.front.height()), (192, 192));
        assert_eq!((l.back.width(), l.back.height()), (192, 192));
        assert_eq!((l.art_scale, l.theme.as_str(), l.name.as_str()), (3, "olx-theme-x", "gen-64x64-a3-s1"));
        assert!(l.spawns.is_empty(), "worms spawn by FindSpot");
    }

    #[test]
    fn the_content_cap_is_reported_not_enforced() {
        let at = |art_scale| GenLevel { width: 2048, height: 2048, art_scale, seed: 1 };
        assert_eq!(at(4).content_cap(), None, "8192² × 4 bytes is exactly the 256 MiB cap");
        assert!(at(8).content_cap().unwrap().contains("MAX_COLOR_MAP_BYTES"));
    }
}
```

Add `pub mod genlevel;` and `pub mod assets;` to `crates/rl-cli/src/lib.rs` (create `assets.rs` empty for now).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p rl-cli --lib genlevel`
Expected: FAIL to compile with "cannot find type `GenLevel` in this scope".

- [ ] **Step 3: Write the generator**

Put this above the tests in `crates/rl-cli/src/genlevel.rs`:

```rust
//! Generated levels for the performance envelope (plan 8): seeded noise
//! caves of any size and art scale, made in memory (no pack is written),
//! so the map-size and art-scale sweeps need no content but a theme
//! (Castle Strike's). Bench only: a game never loads one.
//!
//! A level's id names it: `gen-<width>x<height>-a<art scale>-s<seed>`,
//! such as `gen-2048x2048-a1-s1`. It is what a `MatchSpec`'s `level_id`
//! and a replay's level pack id hold, so `rl-cli stress`, `match` and
//! `gpu-bench` make the same level from the id alone. The arithmetic is
//! integer and `f32` without library calls, so the Windows build makes
//! the same level (its replays' checksums check it).

use rl_content::{LoadedLevel, MAX_ART_SCALE, MAX_COLOR_MAP_BYTES};
use rl_data::image::RgbaImage;
use rl_data::level::{Material, MaterialGrid};

/// The level whose theme generated levels carve with.
pub const THEME_LEVEL: &str = "olx-level-castlestrike";
/// The longest side: OLX's 12-bit worm positions (`rl-import-lxl`'s `MAX_SIDE`).
pub const MAX_SIDE: u32 = 4096;
/// The shortest side.
pub const MIN_SIDE: u32 = 64;
/// What a generated level's id starts with.
pub const PREFIX: &str = "gen-";

/// A generated level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenLevel {
    pub width: u32,
    pub height: u32,
    pub art_scale: u32,
    pub seed: u32,
}

/// A deterministic hash of a lattice point, 0 to 1 (SplitMix64's finaliser).
fn hash(x: i64, y: i64, seed: u32) -> f32 {
    let mut z = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ u64::from(seed).wrapping_mul(0x1656_67B1_9E37_79F9);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// Value noise at `(x, y)` on a lattice of `cell` pixels, 0 to 1.
fn value(x: u32, y: u32, cell: u32, seed: u32) -> f32 {
    let (gx, gy) = (i64::from(x / cell), i64::from(y / cell));
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth((x % cell) as f32 / cell as f32), smooth((y % cell) as f32 / cell as f32));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let top = lerp(hash(gx, gy, seed), hash(gx + 1, gy, seed), sx);
    let bottom = lerp(hash(gx, gy + 1, seed), hash(gx + 1, gy + 1, seed), sx);
    lerp(top, bottom, sy)
}

/// Three octaves: caves of about 64, 24 and 8 pixels.
fn density(x: u32, y: u32, seed: u32) -> f32 {
    0.55 * value(x, y, 64, seed) + 0.30 * value(x, y, 24, seed ^ 1) + 0.15 * value(x, y, 8, seed ^ 2)
}

impl GenLevel {
    /// `gen-<w>x<h>-a<scale>-s<seed>`.
    pub fn id(&self) -> String {
        format!("{PREFIX}{}x{}-a{}-s{}", self.width, self.height, self.art_scale, self.seed)
    }

    /// The level `id` names, if it is a generated one: sides from
    /// [`MIN_SIDE`] to [`MAX_SIDE`], art scale 1 to `MAX_ART_SCALE`.
    pub fn parse(id: &str) -> Option<GenLevel> {
        let mut parts = id.strip_prefix(PREFIX)?.split('-');
        let (w, h) = parts.next()?.split_once('x')?;
        let a = parts.next()?.strip_prefix('a')?;
        let s = parts.next()?.strip_prefix('s')?;
        if parts.next().is_some() {
            return None;
        }
        let g = GenLevel {
            width: w.parse().ok()?,
            height: h.parse().ok()?,
            art_scale: a.parse().ok()?,
            seed: s.parse().ok()?,
        };
        let side = MIN_SIDE..=MAX_SIDE;
        (side.contains(&g.width) && side.contains(&g.height) && (1..=MAX_ART_SCALE).contains(&g.art_scale))
            .then_some(g)
    }

    /// Why `rl_content::Content::load_level` would refuse a pack of this
    /// level (its colour image above `MAX_COLOR_MAP_BYTES`), if it would.
    /// Generated levels bypass the loader (plan 8 ruling P3); the
    /// envelope reports the cap as the point's breaking cause.
    pub fn content_cap(&self) -> Option<String> {
        let s = u64::from(self.art_scale);
        let bytes = u64::from(self.width) * s * u64::from(self.height) * s * 4;
        (bytes > MAX_COLOR_MAP_BYTES).then(|| {
            format!(
                "the content loader refuses a {}x{} level at art scale {}: a {} MiB colour image, above \
                 MAX_COLOR_MAP_BYTES ({} MiB)",
                self.width,
                self.height,
                self.art_scale,
                bytes >> 20,
                MAX_COLOR_MAP_BYTES >> 20
            )
        })
    }

    /// The material: background where the noise is low (caves), rock where
    /// it is high, dirt between.
    pub fn material(&self) -> MaterialGrid {
        let cells = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .map(|(x, y)| match density(x, y, self.seed) {
                d if d < 0.45 => Material::Background,
                d if d > 0.68 => Material::Rock,
                _ => Material::Dirt,
            })
            .collect();
        MaterialGrid::from_cells(self.width, self.height, cells).expect("width × height cells")
    }

    /// The level with art at its scale (front: the material's colour, back:
    /// a dark sky, each texel varied) and `theme` (a theme pack id).
    pub fn level(&self, theme: &str) -> LoadedLevel {
        let material = self.material();
        let s = self.art_scale;
        let (w, h) = (self.width * s, self.height * s);
        let mut front = Vec::with_capacity(w as usize * h as usize * 4);
        let mut back = Vec::with_capacity(w as usize * h as usize * 4);
        for ay in 0..h {
            for ax in 0..w {
                let n = (hash(i64::from(ax), i64::from(ay), self.seed ^ 7) * 40.0) as u8;
                let sky = [10 + n / 4, 12 + n / 4, 40 + n / 2, 255];
                let px = match material.get(ax / s, ay / s) {
                    Material::Background => sky,
                    Material::Dirt => [120 + n, 80 + n / 2, 40, 255],
                    Material::Rock => [90 + n, 90 + n, 100 + n, 255],
                };
                front.extend_from_slice(&px);
                back.extend_from_slice(&sky);
            }
        }
        LoadedLevel {
            name: self.id(),
            material,
            front: RgbaImage::from_raw(w, h, front).expect("w × h texels"),
            back: RgbaImage::from_raw(w, h, back).expect("w × h texels"),
            art_scale: s,
            theme: theme.to_string(),
            spawns: Vec::new(),
        }
    }
}
```

- [ ] **Step 4: Run the generator tests**

Run: `cargo nextest run -p rl-cli --lib genlevel`
Expected: PASS (4 tests). If `caves_are_seeded_and_open` fails on the background share, adjust the two thresholds (0.45, 0.68) until 0.2–0.8 holds for seeds 1 and 2; the assertion stays.

- [ ] **Step 5: Write the failing generated-level stress test**

Add to `crates/rl-cli/tests/stress.rs`:

```rust
/// A generated level (plan 8) plays like a pack's: bounded, deterministic,
/// and its replay reproduces it (made again from the id, not from a pack).
#[test]
fn a_generated_level_plays_and_replays() {
    let Some(content) = content() else { return };
    let spec = stress::spec("olx-classic", "gen-512x384-a2-s1", 8, 3000, 3.0, 1);
    let r = stress::run(&content, &Config::new(spec), &mut |_| {}).unwrap();
    assert_eq!(r.level, (512, 384, 2));
    assert_eq!(r.deterministic, Some(true));
    assert!(r.caps.is_empty(), "{:?}", r.caps);
    let capped = stress::spec("olx-classic", "gen-2048x2048-a8-s1", 2, 3000, 0.05, 1);
    let r = stress::run(&content, &Config { verify: false, ..Config::new(capped) }, &mut |_| {}).unwrap();
    assert!(r.caps.iter().any(|c| c.contains("MAX_COLOR_MAP_BYTES")), "{:?}", r.caps);
}
```

- [ ] **Step 6: Run it to see it fail**

Run: `cargo nextest run -p rl-cli --test stress a_generated_level_plays_and_replays`
Expected: FAIL with an error like "level pack gen-512x384-a2-s1: no pack" from `Content::load_level`.

- [ ] **Step 7: Write `LevelAssets` and route the runner, verify and stress through it**

`crates/rl-cli/src/assets.rs`:

```rust
//! What a match is made of: a pack's mod, level and theme, or a generated
//! level (`genlevel`, plan 8) with Castle Strike's theme. `rl-cli`'s
//! matches, stress runs, replay checks and GPU scenes all load through
//! here, so a generated level's id is enough to make it again.

use anyhow::{Context as _, ensure};
use rl_content::{Content, LoadedLevel, LoadedMod, LoadedTheme, MatchAssets, World, replay_assets_from};
use rl_data::manifest::PackKind;
use rl_replay::{PackRef, Packs, Replay};

use crate::genlevel::{GenLevel, THEME_LEVEL};

/// The hash a generated level's pack reference carries (there is no pack).
pub const GENERATED_HASH: &str = "generated";

/// A match's assets, loaded once.
pub struct LevelAssets {
    pub m: LoadedMod,
    pub level: LoadedLevel,
    pub theme: LoadedTheme,
    /// The packs a recording names (a generated level by its id).
    pub packs: Packs,
    pub generated: Option<GenLevel>,
}

impl LevelAssets {
    /// Mod pack `mod_id` and level `level_id`: a level pack (exactly what
    /// `Content::build_match` loads) or a generated level's id.
    pub fn load(content: &Content, mod_id: &str, level_id: &str) -> anyhow::Result<LevelAssets> {
        let m = content.load_mod(mod_id)?;
        if let Some(g) = GenLevel::parse(level_id) {
            let theme_id = content.load_level(THEME_LEVEL).context("the generated levels' theme")?.theme;
            let theme = content.load_theme(&theme_id).with_context(|| format!("level {level_id}"))?;
            let packs = Packs {
                mod_pack: PackRef { id: mod_id.into(), hash: content.pack_hash(mod_id, PackKind::Mod)? },
                level: PackRef { id: level_id.into(), hash: GENERATED_HASH.into() },
                theme: PackRef { id: theme_id.clone(), hash: content.pack_hash(&theme_id, PackKind::Theme)? },
            };
            return Ok(LevelAssets { m, level: g.level(&theme_id), theme, packs, generated: Some(g) });
        }
        let level = content.load_level(level_id)?;
        let theme = content.load_theme(&level.theme).with_context(|| format!("level {level_id}"))?;
        let packs = content.replay_packs(mod_id, level_id)?;
        Ok(LevelAssets { m, level, theme, packs, generated: None })
    }

    /// For `rl_content::build_match`.
    pub fn match_assets(&self) -> MatchAssets<'_> {
        MatchAssets { m: &self.m, level: &self.level, theme: &self.theme, packs: &self.packs }
    }

    /// The world `replay` (recorded on these assets) starts from. A pack
    /// level's packs are checked as `Content::world_for_replay` does; a
    /// generated level's must name this level.
    pub fn replay_world(&self, content: &Content, replay: &Replay) -> anyhow::Result<World> {
        match self.generated {
            None => content.check_replay_packs(&replay.setup)?,
            Some(_) => ensure!(
                replay.setup.packs.level == self.packs.level && replay.setup.packs.mod_pack == self.packs.mod_pack,
                "the replay was recorded on {} with {}, not {} with {}",
                replay.setup.packs.level.id,
                replay.setup.packs.mod_pack.id,
                self.packs.level.id,
                self.packs.mod_pack.id
            ),
        }
        replay.world(replay_assets_from(&self.m, &self.level, &self.theme)).map_err(anyhow::Error::new)
    }
}
```

In `crates/rl-cli/src/runner.rs`, replace the body of `Match::new` and add `with_assets`:

```rust
    pub fn new(content: &Content, spec: &MatchSpec, opts: &Options) -> anyhow::Result<Match> {
        let assets = LevelAssets::load(content, &spec.mod_id, &spec.level_id)?;
        Match::with_assets(&assets, spec, opts)
    }

    /// [`Match::new`] on assets already loaded (a stress run keeps them for
    /// its presentation; a generated level is made once).
    pub fn with_assets(assets: &LevelAssets, spec: &MatchSpec, opts: &Options) -> anyhow::Result<Match> {
        ensure!(spec.ticks <= MAX_TICKS, "at most {MAX_TICKS} ticks");
        ensure!((2..=MAX_WORMS).contains(&spec.bots), "a Deathmatch of 2 to {MAX_WORMS} bots");
        let built = rl_content::build_match(assets.match_assets(), &spec.match_options())?;
        let BuiltMatch { setup, mode, world, bots, recorder: rec } = built;
        // ... the rest of the old body unchanged (stats, Ok(Match { .. })) ...
    }
```

(Move the old body's remainder, from `let stats = Stats {` to the end, into `with_assets` unchanged. Add `use crate::assets::LevelAssets;`.)

In `crates/rl-cli/src/verify.rs`, replace `match content.play_replay(&decoded) {` and its arms' input with:

```rust
    let played = crate::assets::LevelAssets::load(content, &run.spec.mod_id, &run.spec.level_id)
        .and_then(|a| a.replay_world(content, &decoded))
        .and_then(|world| decoded.play(world).map_err(anyhow::Error::new));
    match played {
```

(the three arms stay as they are; for pack levels this is `Content::play_replay`'s check and world, so the goldens do not move).

In `crates/rl-cli/src/stress.rs`, replace the four loads at the top of `run` (`Match::new`, `load_mod`, `load_level`, `load_theme`) with:

```rust
    let assets = LevelAssets::load(content, &spec.mod_id, &spec.level_id)?;
    let mut m = Match::with_assets(&assets, spec, &Options::default())?;
    let (mo, level, theme) = (&assets.m, &assets.level, &assets.theme);
```

change `PresentSetup { m: &mo, level: &level, theme: &theme, ..}` to `m: mo, level, theme`, and after the area-cache cap add:

```rust
    if let Some(cap) = assets.generated.and_then(|g| g.content_cap()) {
        r.caps.push(cap);
    }
```

(add `use crate::assets::LevelAssets;`; remove the now unused `Context` import only if clippy says so).

- [ ] **Step 8: Run the tests**

Run: `cargo nextest run -p rl-cli`
Expected: PASS, including `a_generated_level_plays_and_replays`, `tests/golden.rs` (the golden checksum streams are unchanged: pack levels load exactly as before) and `tests/matches.rs`.

- [ ] **Step 9: See a large generated level**

Run: `cargo run --release -p rl-cli -- stress --mod olx-classic --level gen-4096x4096-a1-s1 --worms 16 --seconds 5 --render-every 0 --no-verify`
Expected: a report with `level 4096x4096 at art scale 1` and tick times; note the area-cache line (it may show the cache full: that is a finding, not a failure).

- [ ] **Step 10: Docs, gate, commit**

`crates/rl-cli/CLAUDE.md`: a **Generated levels** paragraph (`genlevel.rs`: the id format, `THEME_LEVEL`, bench only, the content cap reported not enforced) and **Assets** (`assets.rs`: `LevelAssets::load` is what `Match::new`, `verify`, `stress` and `gpu-bench` load through; pack levels exactly as `Content::build_match`).

Run: `cargo xtask check`
Expected: `check passed`.

```bash
git add crates/rl-cli
git commit -F - <<'EOF'
rl-cli: play stress runs on generated levels of any size and art scale

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

### Task 3: `rl-render`: GPU timestamps, the shader-headroom probe, hardware offscreen

**Files:**
- Create: `crates/rl-render/src/renderer/timing.rs`, `crates/rl-render/src/renderer/probe.rs`, `crates/rl-render/src/probe.wgsl`
- Modify: `crates/rl-render/src/renderer.rs` (modules, fields, `enable_gpu_timing`, `gpu_times`, `set_probe`), `crates/rl-render/src/renderer/pass.rs` (timestamps on the passes, probe passes), `crates/rl-render/src/offscreen.rs` (`OffscreenOptions`, `AdapterPreference`, `Offscreen::open`, `list_adapters`), `crates/rl-render/src/lib.rs` (exports)
- Test: unit tests in `timing.rs` and `probe.rs`; `crates/rl-render/tests/gpu.rs`
- Docs: `crates/rl-render/CLAUDE.md`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (all exported from `rl_render`):
  - `AdapterPreference { Software (default), Hardware }`, `OffscreenOptions { prefer: AdapterPreference, backends: Option<wgpu::Backends>, timestamps: bool }` (`Default`: today's behaviour), `Offscreen::open(&OffscreenOptions) -> Result<Offscreen, NoAdapter>`; `Offscreen::new()` is `open(&OffscreenOptions::default())`.
  - `list_adapters(backends: Option<wgpu::Backends>) -> Vec<wgpu::AdapterInfo>`.
  - `GpuTimes { frame_ms: f64, probe_ms: f64 }`, `gpu_times(ticks: [u64; 4], period_ns: f32, probe: bool) -> GpuTimes`.
  - `Probe { passes: u32, taps: u32 }` with `clamped(self) -> Probe`, `MAX_PROBE_PASSES = 64`, `MAX_PROBE_TAPS = 64`.
  - `Renderer::enable_gpu_timing(&mut self) -> bool`, `Renderer::gpu_times(&mut self) -> Option<GpuTimes>`, `Renderer::set_probe(&mut self, probe: Option<Probe>)`.

- [ ] **Step 1: Write the failing CPU tests**

`crates/rl-render/src/renderer/timing.rs` starts with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_become_milliseconds() {
        let t = gpu_times([1_000, 2_001_000, 3_000_000, 7_000_000], 1.0, true);
        assert!((t.frame_ms - 2.0).abs() < 1e-9 && (t.probe_ms - 4.0).abs() < 1e-9, "{t:?}");
        let t = gpu_times([10, 20, 30, 40], 1000.0, false);
        assert!((t.frame_ms - 0.01).abs() < 1e-9 && t.probe_ms == 0.0, "no probe, no probe time");
        assert_eq!(gpu_times([0, 5, 0, 5], 1.0, true), GpuTimes::default(), "never written");
        assert_eq!(gpu_times([9, 5, 9, 5], 1.0, true), GpuTimes::default(), "a clock that ran backwards");
    }
}
```

`crates/rl-render/src/renderer/probe.rs` starts with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_are_bounded() {
        assert_eq!(Probe { passes: 1000, taps: 0 }.clamped(), Probe { passes: MAX_PROBE_PASSES, taps: 1 });
        assert_eq!(Probe { passes: 4, taps: 9 }.clamped(), Probe { passes: 4, taps: 9 });
        assert_eq!(Probe { passes: 1, taps: 500 }.clamped().taps, MAX_PROBE_TAPS);
    }
}
```

Add `mod probe;` and `mod timing;` next to `mod pass;` in `renderer.rs`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p rl-render --lib`
Expected: FAIL to compile with "cannot find function `gpu_times`" and "cannot find struct `Probe`".

- [ ] **Step 3: Write the timing module**

Above the tests in `timing.rs`:

```rust
//! GPU time of each frame (bench only, plan 8): timestamps at the start of
//! the world pass (slot 0), the end of the upscale pass (1) and around the
//! probe passes (2, 3), when the device has `Features::TIMESTAMP_QUERY`
//! (an `Offscreen` opened with `timestamps`). Reading them waits for the
//! frame, so only offscreen measurements use them.

use std::sync::mpsc;

/// Query slots a frame writes.
pub(crate) const QUERIES: u32 = 4;

/// What the GPU spent on the last frame, in ms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuTimes {
    /// The world and upscale passes.
    pub frame_ms: f64,
    /// The probe passes (0 without them).
    pub probe_ms: f64,
}

/// The four slots' `ticks` at `period_ns` a tick as [`GpuTimes`]; a span
/// whose start was never written (0) or that runs backwards is 0.
pub fn gpu_times(ticks: [u64; 4], period_ns: f32, probe: bool) -> GpuTimes {
    let span = |a: u64, b: u64| if a == 0 || b < a { 0.0 } else { (b - a) as f64 * f64::from(period_ns) / 1e6 };
    let t = GpuTimes { frame_ms: span(ticks[0], ticks[1]), probe_ms: if probe { span(ticks[2], ticks[3]) } else { 0.0 } };
    if t.frame_ms == 0.0 { GpuTimes::default() } else { t }
}

/// The query set and its buffers.
pub(crate) struct Timer {
    pub set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period_ns: f32,
    /// A frame wrote the slots since the last read.
    pub written: bool,
    /// That frame ran probe passes.
    pub probe: bool,
}

impl Timer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Timer {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("rl-render timing"),
            ty: wgpu::QueryType::Timestamp,
            count: QUERIES,
        });
        let size = u64::from(QUERIES) * u64::from(wgpu::QUERY_SIZE);
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
        };
        Timer {
            set,
            resolve: buffer("rl-render timing resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            read: buffer("rl-render timing read", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
            period_ns: queue.get_timestamp_period(),
            written: false,
            probe: false,
        }
    }

    /// The frame's timestamps into the read buffer (the end of its encoder).
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.set, 0..QUERIES, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, self.resolve.size());
    }

    /// Waits for the last frame and reads its times.
    pub fn read(&mut self, device: &wgpu::Device) -> Option<GpuTimes> {
        if !std::mem::take(&mut self.written) {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.read.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let mut ticks = [0u64; 4];
        {
            let mapped = self.read.get_mapped_range(..).ok()?;
            for (t, bytes) in ticks.iter_mut().zip(mapped.chunks_exact(8)) {
                *t = u64::from_le_bytes(bytes.try_into().ok()?);
            }
        }
        self.read.unmap();
        Some(gpu_times(ticks, self.period_ns, self.probe))
    }
}
```

(`get_mapped_range(..)` returns a `Result` in wgpu 30.0.1, as `offscreen.rs`'s `read_texture` uses it with `expect`; if the compiler says it is an `Option`, keep `.ok()?` → `?`.)

- [ ] **Step 4: Write the probe module and shader**

`crates/rl-render/src/probe.wgsl`:

```wgsl
// rl-render's shader-headroom probe (bench only, plan 8): a box blur of
// `taps` texels of the last pass's output, over the whole target. Its GPU
// time per pass says how many full-screen effects fit in a frame.

struct Probe {
    size: vec2<f32>,
    taps: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> probe: Probe;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_probe(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    var sum = vec4<f32>(0.0);
    for (var i = 0u; i < probe.taps; i = i + 1u) {
        let o = vec2<f32>(f32(i % 8u) - 3.5, f32(i / 8u) - 3.5);
        sum = sum + textureSampleLevel(src, smp, (frag.xy + o) / probe.size, 0.0);
    }
    return sum / f32(probe.taps);
}
```

Above the tests in `probe.rs`:

```rust
//! The shader-headroom probe (bench only, plan 8): after a frame,
//! `passes` full-screen fragment passes, each a `taps`-tap blur of the
//! previous pass's output, at the frame's size, into two textures of its
//! own (ping-pong). The picture never changes; the probe's GPU time per
//! pass answers "how many full-screen shaders fit in a frame".

use super::{WORLD_FORMAT, entry, pipeline, texture, TEXTURE, UNIFORM};
use super::timing::Timer;

/// The most passes a probe runs.
pub const MAX_PROBE_PASSES: u32 = 64;
/// The most taps a pass reads.
pub const MAX_PROBE_TAPS: u32 = 64;

/// A probe: `passes` passes of a `taps`-tap blur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub passes: u32,
    pub taps: u32,
}

impl Probe {
    /// At most [`MAX_PROBE_PASSES`] passes, 1 to [`MAX_PROBE_TAPS`] taps.
    pub fn clamped(self) -> Probe {
        Probe { passes: self.passes.min(MAX_PROBE_PASSES), taps: self.taps.clamp(1, MAX_PROBE_TAPS) }
    }
}

/// The probe's textures, made for one size.
struct Targets {
    size: (u32, u32),
    views: [wgpu::TextureView; 2],
    /// `binds[j]` reads `views[j]`.
    binds: [wgpu::BindGroup; 2],
}

pub(crate) struct ProbeState {
    pub probe: Probe,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    targets: Option<Targets>,
}

impl ProbeState {
    pub fn new(device: &wgpu::Device, probe: Probe) -> ProbeState {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rl-render probe"),
            entries: &[
                entry(0, frag, UNIFORM),
                entry(1, frag, TEXTURE),
                entry(2, frag, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let pipeline = pipeline(
            device,
            "rl-render probe",
            include_str!("../probe.wgsl"),
            ("vs_full", "fs_probe"),
            &[Some(&layout)],
            &[],
            WORLD_FORMAT,
            None,
            wgpu::PrimitiveTopology::TriangleList,
        );
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rl-render probe"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rl-render probe"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        ProbeState { probe, pipeline, layout, uniform, sampler, targets: None }
    }

    fn ensure(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let views = [0, 1].map(|_| {
            texture(device, "rl-render probe", size, usage).create_view(&wgpu::TextureViewDescriptor::default())
        });
        let binds = [0, 1].map(|j| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rl-render probe"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[j]) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            })
        });
        self.targets = Some(Targets { size, views, binds });
    }

    /// Encodes the passes for an output of `size`; with `timer`, slot 2 at
    /// the first pass's start and 3 at the last pass's end.
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
        timer: Option<&Timer>,
    ) {
        let n = self.probe.passes;
        if n == 0 {
            return;
        }
        let size = (size.0.max(1), size.1.max(1));
        self.ensure(device, size);
        let f = |v: f32| v.to_bits();
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&[f(size.0 as f32), f(size.1 as f32), self.probe.taps, 0]));
        let t = self.targets.as_ref().expect("ensured above");
        for i in 0..n {
            let timestamps = timer.map(|tm| wgpu::RenderPassTimestampWrites {
                query_set: &tm.set,
                beginning_of_pass_write_index: (i == 0).then_some(2),
                end_of_pass_write_index: (i + 1 == n).then_some(3),
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rl-render probe"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.views[(i % 2) as usize],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timestamps,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &t.binds[((i + 1) % 2) as usize], &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
```

In `renderer.rs`, make `entry`, `pipeline`, `texture`, `TEXTURE` and `UNIFORM` `pub(super)`-visible to the child modules (they are private items of the parent module; child modules can already use them via `super::` — no change needed unless the compiler says otherwise).

- [ ] **Step 5: Wire timing and the probe into the renderer**

In `renderer.rs`:

```rust
mod pass;
mod probe;
mod timing;

pub use probe::{MAX_PROBE_PASSES, MAX_PROBE_TAPS, Probe};
pub use timing::{GpuTimes, gpu_times};
```

add the fields `timer: Option<timing::Timer>,` and `probe: Option<probe::ProbeState>,` to `Renderer` (initialised to `None` in `new`), and the methods:

```rust
    /// Bench only (plan 8): time every frame on the GPU from now on, when
    /// the device has `Features::TIMESTAMP_QUERY` (an `Offscreen` opened
    /// with `timestamps`). Returns whether it does.
    pub fn enable_gpu_timing(&mut self) -> bool {
        if !self.device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return false;
        }
        if self.timer.is_none() {
            self.timer = Some(timing::Timer::new(&self.device, &self.queue));
        }
        true
    }

    /// The last frame's GPU times (waits for it); `None` without timing.
    pub fn gpu_times(&mut self) -> Option<GpuTimes> {
        let device = self.device.clone();
        self.timer.as_mut()?.read(&device)
    }

    /// Bench only (the shader-headroom probe, plan 8): `Some` runs the
    /// probe's passes after every frame, into textures of its own (the
    /// picture is unchanged); `None`, the default, runs none.
    pub fn set_probe(&mut self, probe: Option<Probe>) {
        self.probe = probe.map(|p| probe::ProbeState::new(&self.device, p.clamped()));
    }
```

In `renderer/pass.rs`, change `begin` to take the timestamp writes:

```rust
fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        // ... as before ...
        timestamp_writes: timestamps,
        // ...
    })
}
```

and in `render_in`:

```rust
        let stamp = |begin: Option<u32>, end: Option<u32>| {
            self.timer.as_ref().map(|t| wgpu::RenderPassTimestampWrites {
                query_set: &t.set,
                beginning_of_pass_write_index: begin,
                end_of_pass_write_index: end,
            })
        };
        // the world pass:
        let mut pass = begin(&mut encoder, "rl-render world", &world.view, stamp(Some(0), None));
        // the upscale pass:
        let mut pass = begin(&mut encoder, "rl-render upscale", target, stamp(None, Some(1)));
```

then, after the upscale pass's block and before `self.queue.submit(..)`:

```rust
        if let Some(p) = self.probe.as_mut() {
            p.encode(&self.device, &self.queue, &mut encoder, (area[2], area[3]), self.timer.as_ref());
        }
        let probing = self.probe.as_ref().is_some_and(|p| p.probe.passes > 0);
        if let Some(t) = self.timer.as_mut() {
            t.resolve(&mut encoder);
            t.written = true;
            t.probe = probing;
        }
```

(If the borrow checker objects to `stamp` borrowing `self.timer` while `self.target` is borrowed, compute `let timer = self.timer.as_ref();` once before the passes and build the writes from `timer` directly; both are shared borrows.)

- [ ] **Step 6: Offscreen options and adapter listing**

In `crates/rl-render/src/offscreen.rs`:

```rust
/// Which adapter an [`Offscreen`] prefers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdapterPreference {
    /// lavapipe, then llvmpipe, then the rest: goldens draw the same everywhere.
    #[default]
    Software,
    /// A discrete GPU, then an integrated one, then the rest, software last
    /// (the performance envelope, plan 8).
    Hardware,
}

/// How an [`Offscreen`] opens; the default is [`Offscreen::new`]'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OffscreenOptions {
    pub prefer: AdapterPreference,
    /// Only these backends (else what `WGPU_BACKEND` allows).
    pub backends: Option<wgpu::Backends>,
    /// Ask for `Features::TIMESTAMP_QUERY` when the adapter has it (bench only).
    pub timestamps: bool,
}

fn rank(info: &wgpu::AdapterInfo, prefer: AdapterPreference) -> u8 {
    use wgpu::{Backend, DeviceType};
    match prefer {
        AdapterPreference::Software => match (info.device_type, info.backend) {
            (DeviceType::Cpu, Backend::Vulkan) => 0,
            (DeviceType::Cpu, _) => 1,
            _ => 2,
        },
        AdapterPreference::Hardware => match info.device_type {
            DeviceType::DiscreteGpu => 0,
            DeviceType::IntegratedGpu => 1,
            DeviceType::VirtualGpu | DeviceType::Other => 2,
            DeviceType::Cpu => 3,
        },
    }
}

fn instance(backends: Option<wgpu::Backends>) -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    if let Some(b) = backends {
        desc.backends = b;
    }
    wgpu::Instance::new(desc)
}

/// Every adapter wgpu finds (for `rl-cli gpu-bench --list-adapters`).
pub fn list_adapters(backends: Option<wgpu::Backends>) -> Vec<wgpu::AdapterInfo> {
    let i = instance(backends);
    pollster::block_on(i.enumerate_adapters(backends.unwrap_or(wgpu::Backends::all())))
        .iter()
        .map(|a| a.get_info())
        .collect()
}
```

and replace `Offscreen::new` with:

```rust
    /// Opens the preferred software adapter ([`OffscreenOptions::default`]).
    pub fn new() -> Result<Offscreen, NoAdapter> {
        Offscreen::open(&OffscreenOptions::default())
    }

    /// Opens an adapter as `o` says; `WGPU_BACKEND` and the other `WGPU_*`
    /// variables narrow the choice unless `o.backends` is given.
    pub fn open(o: &OffscreenOptions) -> Result<Offscreen, NoAdapter> {
        let instance = instance(o.backends);
        let mut adapters =
            pollster::block_on(instance.enumerate_adapters(o.backends.unwrap_or(wgpu::Backends::all())));
        adapters.sort_by_key(|a| rank(&a.get_info(), o.prefer));
        let adapter = match adapters.into_iter().next() {
            Some(a) => a,
            None => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| NoAdapter(e.to_string()))?,
        };
        let info = adapter.get_info();
        let required_features = if o.timestamps {
            adapter.features() & wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rl-render offscreen"),
            required_features,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| NoAdapter(format!("{} ({:?}): {e}", info.name, info.backend)))?;
        Ok(Offscreen { device, queue, info })
    }
```

Export from `lib.rs`: `pub use offscreen::{AdapterPreference, NoAdapter, OUTPUT_FORMAT, Offscreen, OffscreenOptions, list_adapters};` and `pub use renderer::{GpuTimes, MAX_PROBE_PASSES, MAX_PROBE_TAPS, MIN_SCALE, Probe, RenderSettings, Renderer, WORLD_FORMAT, gpu_times};`.

- [ ] **Step 7: Run the CPU tests**

Run: `cargo nextest run -p rl-render`
Expected: PASS (`timestamps_become_milliseconds`, `probes_are_bounded`, the existing tests).

- [ ] **Step 8: Write the GPU tests**

Add to `crates/rl-render/tests/gpu.rs` (and `Probe, OffscreenOptions` to its `use rl_render::{..}`):

```rust
/// The shader-headroom probe draws into textures of its own: the picture
/// is the same with it as without.
#[test]
fn the_probe_never_changes_the_picture() {
    let Some(gpu) = offscreen() else { return };
    let l = list(View { x: 10.0, y: 10.0, w: 64.0, h: 36.0 });
    let mut plain = gpu.renderer(RenderSettings::default());
    plain.upload_terrain(&map());
    let mut probed = gpu.renderer(RenderSettings::default());
    probed.upload_terrain(&map());
    probed.set_probe(Some(Probe { passes: 4, taps: 9 }));
    let a = gpu.render_to_image(&mut plain, &l, (256, 144));
    let b = gpu.render_to_image(&mut probed, &l, (256, 144));
    assert_eq!(a.as_raw(), b.as_raw());
}

/// GPU timing reports a frame (and the probe) when the device has
/// timestamps, and nothing otherwise; it never fails.
#[test]
fn gpu_timing_reports_when_the_device_has_timestamps() {
    if std::env::var_os("RL_GPU_TESTS").is_none() {
        return;
    }
    let Ok(gpu) = rl_render::Offscreen::open(&OffscreenOptions { timestamps: true, ..OffscreenOptions::default() })
    else {
        return;
    };
    let has = gpu.device().features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let mut r = gpu.renderer(RenderSettings::default());
    r.upload_terrain(&map());
    assert_eq!(r.enable_gpu_timing(), has);
    r.set_probe(Some(Probe { passes: 2, taps: 4 }));
    let _ = gpu.render_to_image(&mut r, &list(View { x: 10.0, y: 10.0, w: 64.0, h: 36.0 }), (256, 144));
    match r.gpu_times() {
        Some(t) => assert!(has && t.frame_ms.is_finite() && t.frame_ms >= 0.0 && t.probe_ms >= 0.0, "{t:?}"),
        None => assert!(!has, "timestamps were asked for and offered"),
    }
    assert_eq!(r.gpu_times(), None, "each frame is read once");
}
```

Run: `RL_GPU_TESTS=1 cargo nextest run -p rl-render --test gpu`
Expected: PASS on lavapipe (the software adapter; `gpu_timing_reports_when_the_device_has_timestamps` passes whichever way the adapter answers).

- [ ] **Step 9: Try it on the real GPU (manual)**

Run: `cargo run -p rl-render --example adapter` (software: unchanged), then
`GALLIUM_DRIVER=d3d12 MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA WGPU_BACKEND=gl RL_GPU_TESTS=1 cargo test -p rl-render --test gpu gpu_timing -- --test-threads 1`
Expected: either PASS, or a segfault when the test thread exits (the d3d12 driver bug in `CLAUDE.md`'s **WSL** note): a segfault after "test ... ok" is that bug, not this change; Task 4's `gpu-bench` runs on the main thread. Record which happened in the ledger.

- [ ] **Step 10: Docs, gates, commit**

`crates/rl-render/CLAUDE.md`: an **Offscreen** addition (`Offscreen::open(&OffscreenOptions)`: `AdapterPreference::Hardware`, `backends`, `timestamps`; `list_adapters`; `new()` unchanged), and a **Bench only (plan 8)** bullet: `enable_gpu_timing`/`gpu_times` (slots 0–3, `timing.rs`; reading waits for the frame, so offscreen only) and `set_probe` (`probe.rs`, `probe.wgsl`: `passes` ping-pong blur passes of `taps` taps at the output's size into the probe's own textures; the picture never changes; `None` by default, so goldens cannot move).

Run: `cargo xtask check`
Expected: `check passed`.
Run: `cargo xtask full`
Expected: `full passed` (render goldens, HUD snapshots, soak, bench unchanged).

```bash
git add crates/rl-render
git commit -F - <<'EOF'
rl-render: add GPU timestamps, the shader probe and hardware offscreen

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

### Task 4: `rl-cli gpu-bench` and `rl-game --frame-log`

**Files:**
- Create: `crates/rl-cli/src/gpu_bench.rs` (library), `crates/rl-cli/src/gpu_bench_cmd.rs` (the command), `crates/rl-cli/tests/gpu_bench.rs`
- Modify: `crates/rl-cli/src/lib.rs`, `crates/rl-cli/src/main.rs` (the `GpuBench` command), `xtask/src/check.rs` (`GPU_TESTS` gains `("rl-cli", "gpu_bench")`)
- Modify: `crates/rl-game/src/stats.rs` (`FrameLog`, `Pct`, `Meter::frame_log`, per-frame busy times), `crates/rl-game/src/app.rs` (`drawn_size`, `App::frame_log`), `crates/rl-game/src/main.rs` (`--frame-log`, `--frame-log-skip`)
- Docs: `crates/rl-cli/CLAUDE.md`, `crates/rl-game/CLAUDE.md`, `xtask/CLAUDE.md` (the GPU tier's new test)

**Interfaces:**
- Consumes: Task 1 (`stress::Config`, `Config::record`, `stress::Spread`), Task 2 (`assets::LevelAssets::load`, `replay_world`), Task 3 (`Offscreen::open`, `OffscreenOptions`, `AdapterPreference`, `list_adapters`, `Renderer::enable_gpu_timing`, `gpu_times`, `set_probe`, `Probe`).
- Produces:
  - `rl_cli::gpu_bench::{BenchConfig, GpuReport, SizeReport, ProbePoint, run, check_adapter}`:
    - `BenchConfig { sizes: Vec<(u32, u32)>, start_tick: u32, frames: u32, probe_passes: Vec<u32>, probe_taps: u32, probe_frames: u32, options: OffscreenOptions }`
    - `GpuReport { adapter: String, backend: String, device_type: String, timestamps: bool, scene: String, start_tick: u32, sizes: Vec<SizeReport> }`
    - `SizeReport { size: (u32, u32), frames: u32, tick_ms: Spread, cpu_ms: Spread, frame_ms: Spread, total_ms: Spread, gpu_ms: Option<Spread>, probe: Vec<ProbePoint> }`
    - `ProbePoint { passes: u32, taps: u32, ms: Spread, timed_on_gpu: bool }`
    - `run(content: &Content, replay: &Replay, cfg: &BenchConfig, progress: &mut dyn FnMut(String)) -> anyhow::Result<GpuReport>`
    - `check_adapter(prefer: AdapterPreference, device_type: wgpu::DeviceType, name: &str) -> anyhow::Result<()>`
  - `rl-cli gpu-bench --replay F [--sizes 1920x1080,...] [--start-tick 1000] [--frames 600] [--probe-passes 0,1,2,4,8,16,32] [--probe-taps 9] [--probe-frames 120] [--adapter hardware|software] [--backend auto|gl|vulkan|dx12] [--content DIR] [--out F.ron] [--list-adapters]`.
  - `rl_game::stats::{FrameLog, Pct}`: `FrameLog { size: (u32, u32), gpu: String, skipped_s: f64, frames: usize, seconds: f64, fps: f64, frame_ms: Pct, busy_ms: Pct, tick_ms: Pct }`, `Pct { p50: f64, p99: f64, max: f64 }`; `Meter::frame_log(&self, skip_s: f64, size: (u32, u32), gpu: &str) -> FrameLog`; `App::frame_log(&self, skip_s: f64) -> FrameLog`.
  - `rl-game --frame-log F.ron [--frame-log-skip 3]`.

- [ ] **Step 1: Write the failing pure tests**

At the bottom of the new `crates/rl-cli/src/gpu_bench.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_sizes("1920x1080,3440x1440").unwrap(), [(1920, 1080), (3440, 1440)]);
        assert!(parse_sizes("1920").is_err() && parse_sizes("0x10").is_err() && parse_sizes("").is_err());
    }

    /// Review Focus 2: llvmpipe is never "the GPU".
    #[test]
    fn a_software_adapter_is_refused_for_hardware_runs() {
        use wgpu::DeviceType as D;
        assert!(check_adapter(AdapterPreference::Hardware, D::Cpu, "llvmpipe").is_err());
        assert!(check_adapter(AdapterPreference::Hardware, D::DiscreteGpu, "RTX").is_ok());
        assert!(check_adapter(AdapterPreference::Hardware, D::Other, "D3D12 (RTX)").is_ok());
        assert!(check_adapter(AdapterPreference::Software, D::Cpu, "lavapipe").is_ok());
        let why = check_adapter(AdapterPreference::Hardware, D::Cpu, "llvmpipe").unwrap_err().to_string();
        assert!(why.contains("GALLIUM_DRIVER=d3d12"), "{why}");
    }
}
```

In `crates/rl-game/src/stats.rs`'s `mod tests`:

```rust
    #[test]
    fn frame_log_skips_the_first_seconds() {
        let (mut m, mut s) = (Meter::new(), Stats::default());
        for _ in 0..100 {
            m.frame(0.020, &mut s);
            m.busy(0.010);
        }
        for _ in 0..100 {
            m.frame(0.005, &mut s);
            m.busy(0.002);
        }
        let log = m.frame_log(1.999, (1920, 1080), "test gpu");
        assert_eq!(log.frames, 100);
        assert!((log.frame_ms.p50 - 5.0).abs() < 1e-3 && (log.busy_ms.p99 - 2.0).abs() < 1e-3, "{log:?}");
        assert!((log.fps - 200.0).abs() < 1.0, "{log:?}");
        assert_eq!((log.size, log.gpu.as_str()), ((1920, 1080), "test gpu"));
        assert_eq!(m.frame_log(100.0, (1, 1), "").frames, 0, "a run shorter than the skip");
    }
```

Add `pub mod gpu_bench;` to `crates/rl-cli/src/lib.rs`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p rl-cli --lib gpu_bench && cargo nextest run -p rl-game --lib stats`
Expected: FAIL to compile ("cannot find function `parse_sizes`", "no method named `frame_log`").

- [ ] **Step 3: Write the GPU bench library**

Above the tests in `crates/rl-cli/src/gpu_bench.rs`:

```rust
//! The GPU half of the performance envelope (plan 8): a recorded match (a
//! replay: the heavy stress match, or a sweep's generated level) drawn
//! offscreen at each size on the chosen adapter, one tick and one frame at
//! a time; the CPU side timed with the clock, the GPU side with timestamps
//! when the device has them (`GpuReport::timestamps`; else only the wall
//! time of the frame waited for, said so). Then the shader-headroom probe
//! (`rl_render::Probe`) at each size. The scene is `Viewport::GAME` over
//! the whole target (no HUD; plan 8 ruling P11).

use std::time::Instant;

use anyhow::{Context as _, anyhow, bail, ensure};
use rl_content::{Content, World};
use rl_present::{PresentSetup, Presentation, default_colour};
use rl_render::{AdapterPreference, OUTPUT_FORMAT, Offscreen, OffscreenOptions, Probe, RenderSettings};
use rl_replay::{Cursor, Replay};
use rl_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use rl_sim::TickEvents;
use serde::{Deserialize, Serialize};

use crate::assets::LevelAssets;
use crate::stress::Spread;

/// What to measure.
#[derive(Clone, Debug)]
pub struct BenchConfig {
    pub sizes: Vec<(u32, u32)>,
    /// Ticks played (unrendered) before the measured frames.
    pub start_tick: u32,
    /// Frames measured at each size (one tick each).
    pub frames: u32,
    /// Probe pass counts, the first 0 (the baseline), e.g. 0, 1, 2, 4, 8.
    pub probe_passes: Vec<u32>,
    pub probe_taps: u32,
    /// Frames drawn for each probe count.
    pub probe_frames: u32,
    pub options: OffscreenOptions,
}

/// The probe at one pass count.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProbePoint {
    pub passes: u32,
    pub taps: u32,
    /// The probe passes' GPU ms (timestamps), else the frame's wall ms over
    /// the 0-pass baseline's median.
    pub ms: Spread,
    pub timed_on_gpu: bool,
}

/// One size.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SizeReport {
    pub size: (u32, u32),
    pub frames: u32,
    /// The replay's tick and the presentation's (`World::tick`, `on_tick`), ms.
    pub tick_ms: Spread,
    /// The draw list, the uploads and the render's encoding and submit, ms.
    pub cpu_ms: Spread,
    /// From the draw list to the device idle (CPU, then the GPU waited for), ms.
    pub frame_ms: Spread,
    /// A tick and a frame: what a 100 Hz game spends on a drawn frame at most, ms.
    pub total_ms: Spread,
    /// The world and upscale passes on the GPU (timestamps), ms.
    pub gpu_ms: Option<Spread>,
    pub probe: Vec<ProbePoint>,
}

/// What `rl-cli gpu-bench --out` writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuReport {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    /// The GPU times are timestamps (else CPU-side times only).
    pub timestamps: bool,
    /// The replay's mod and level.
    pub scene: String,
    pub start_tick: u32,
    pub sizes: Vec<SizeReport>,
}

/// `1920x1080,2560x1440` as sizes.
pub fn parse_sizes(text: &str) -> anyhow::Result<Vec<(u32, u32)>> {
    let sizes = text
        .split(',')
        .map(|s| -> anyhow::Result<(u32, u32)> {
            let (w, h) = s.trim().split_once('x').ok_or_else(|| anyhow!("{s:?} is not WIDTHxHEIGHT"))?;
            let (w, h): (u32, u32) = (w.parse()?, h.parse()?);
            ensure!(w > 0 && h > 0 && w <= 16_384 && h <= 16_384, "{s:?}: sides from 1 to 16384");
            Ok((w, h))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    ensure!(!sizes.is_empty(), "no size");
    Ok(sizes)
}

/// A run that asked for the hardware adapter and got a software
/// rasteriser (llvmpipe, lavapipe) is an error, never a "GPU" result.
pub fn check_adapter(prefer: AdapterPreference, device_type: wgpu::DeviceType, name: &str) -> anyhow::Result<()> {
    if prefer == AdapterPreference::Hardware && device_type == wgpu::DeviceType::Cpu {
        bail!(
            "asked for the hardware adapter, got {name} (a software rasteriser); under WSL run with \
             GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl (and MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA)"
        );
    }
    Ok(())
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Plays on to the next recorded checksum and compares: the scene drawn is
/// the recorded match (a generated level made differently, on another
/// platform, would not be).
fn check_on(replay: &Replay, world: &mut World, out: &mut TickEvents, mut t: u32) -> anyhow::Result<()> {
    while t < replay.ticks() && !replay.checksums.iter().any(|&(ct, _)| ct == t) {
        world.tick(replay.row(t), out);
        t += 1;
    }
    if let Some(&(_, want)) = replay.checksums.iter().find(|&&(ct, _)| ct == t) {
        ensure!(world.checksum() == want, "the replay diverged by tick {t}: this is not the recorded match");
    }
    Ok(())
}

/// Measures `replay` at every size of `cfg`.
pub fn run(
    content: &Content,
    replay: &Replay,
    cfg: &BenchConfig,
    progress: &mut dyn FnMut(String),
) -> anyhow::Result<GpuReport> {
    ensure!(cfg.probe_passes.first().is_none_or(|&k| k == 0), "the probe counts start with 0 (the baseline)");
    ensure!(cfg.start_tick < replay.ticks(), "the replay has {} ticks, starting at {}", replay.ticks(), cfg.start_tick);
    let packs = &replay.setup.packs;
    let assets = LevelAssets::load(content, &packs.mod_pack.id, &packs.level.id)?;
    let gpu = Offscreen::open(&cfg.options)?;
    let info = gpu.adapter_info().clone();
    check_adapter(cfg.options.prefer, info.device_type, &info.name)?;
    let skin = content.skin_or_default("olx-skin-default");
    let mut report = GpuReport {
        adapter: info.name.clone(),
        backend: format!("{:?}", info.backend),
        device_type: format!("{:?}", info.device_type),
        scene: format!("{} on {}", packs.mod_pack.id, packs.level.id),
        start_tick: cfg.start_tick,
        ..GpuReport::default()
    };
    let end = (cfg.start_tick + cfg.frames).min(replay.ticks());
    for &size in &cfg.sizes {
        progress(format!("{}x{}: playing to tick {}", size.0, size.1, cfg.start_tick));
        let mut world = assets.replay_world(content, replay)?;
        let setup = PresentSetup {
            core: content.core(),
            m: &assets.m,
            level: &assets.level,
            theme: &assets.theme,
            viewport: Viewport::GAME,
            options: SceneOptions { interpolation: Interpolation::Latest, show_health_bars: false },
            seed: replay.setup.seed,
        };
        let mut present = Presentation::new(&setup, &world);
        let mut renderer = gpu.renderer(RenderSettings::default());
        report.timestamps = renderer.enable_gpu_timing();
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("gpu-bench frame"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let (mut out, mut list, mut shown) = (TickEvents::new(), DrawList::new(), 0);
        let mut cursor = Cursor::default();
        while cursor.tick() < cfg.start_tick {
            if !cursor.step(replay, &mut world, &mut out).map_err(|m| anyhow!("the replay diverged: {m}"))? {
                break;
            }
            while shown < world.worms().len() {
                present.add_worm(&skin, default_colour(shown), shown == 0, false);
                shown += 1;
            }
            present.on_tick(&world, &out);
        }
        // A first frame makes the world target and uploads everything.
        present.build(&world, 1.0, &mut list);
        present.upload(&mut renderer);
        renderer.render(&list, &view, size);
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
        let _ = renderer.gpu_times();
        let (mut tick, mut cpu, mut frame, mut total, mut gpu_ms) = (vec![], vec![], vec![], vec![], vec![]);
        for t in cfg.start_tick..end {
            let t0 = Instant::now();
            world.tick(replay.row(t), &mut out);
            while shown < world.worms().len() {
                present.add_worm(&skin, default_colour(shown), shown == 0, false);
                shown += 1;
            }
            present.on_tick(&world, &out);
            tick.push(ms(t0));
            let t1 = Instant::now();
            present.build(&world, 1.0, &mut list);
            present.upload(&mut renderer);
            renderer.render(&list, &view, size);
            cpu.push(ms(t1));
            gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
            frame.push(ms(t1));
            total.push(ms(t0));
            if let Some(g) = renderer.gpu_times() {
                gpu_ms.push(g.frame_ms);
            }
        }
        check_on(replay, &mut world, &mut out, end)?;
        progress(format!("{}x{}: the probe", size.0, size.1));
        let mut probe = Vec::new();
        let mut base = 0.0;
        for &passes in &cfg.probe_passes {
            renderer.set_probe((passes > 0).then_some(Probe { passes, taps: cfg.probe_taps }));
            let (mut wall, mut on_gpu) = (vec![], vec![]);
            for _ in 0..cfg.probe_frames {
                let t0 = Instant::now();
                renderer.render(&list, &view, size);
                gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
                wall.push(ms(t0));
                if let Some(g) = renderer.gpu_times() {
                    on_gpu.push(g.probe_ms);
                }
            }
            if passes == 0 {
                base = Spread::of(&wall).p50;
            }
            let timed_on_gpu = report.timestamps && !on_gpu.is_empty();
            let ms = if timed_on_gpu {
                Spread::of(&on_gpu)
            } else {
                Spread::of(&wall.iter().map(|w| (w - base).max(0.0)).collect::<Vec<_>>())
            };
            probe.push(ProbePoint { passes, taps: cfg.probe_taps, ms, timed_on_gpu });
        }
        renderer.set_probe(None);
        for w in renderer.warnings().drain(..) {
            progress(format!("warning: {w}"));
        }
        report.sizes.push(SizeReport {
            size,
            frames: end - cfg.start_tick,
            tick_ms: Spread::of(&tick),
            cpu_ms: Spread::of(&cpu),
            frame_ms: Spread::of(&frame),
            total_ms: Spread::of(&total),
            gpu_ms: (!gpu_ms.is_empty()).then(|| Spread::of(&gpu_ms)),
            probe,
        });
    }
    Ok(report)
}
```

(`rl_content` re-exports `World`; `Replay.checksums` is `Vec<(u32, u64)>` and `World::checksum` gives a `u64`.)

- [ ] **Step 4: Write the command**

`crates/rl-cli/src/gpu_bench_cmd.rs`:

```rust
//! `rl-cli gpu-bench`: a replay drawn offscreen at several sizes on the
//! chosen adapter, timed on the CPU and the GPU, with the shader probe
//! (`rl_cli::gpu_bench`); the report printed and, with `--out`, written as
//! RON (`cargo xtask envelope` reads it).

use std::path::PathBuf;

use anyhow::Context as _;
use rl_cli::gpu_bench::{self, BenchConfig, parse_sizes};
use rl_render::{AdapterPreference, OffscreenOptions};

#[derive(Clone, Copy, clap::ValueEnum)]
enum Adapter {
    Hardware,
    Software,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Backend {
    Auto,
    Gl,
    Vulkan,
    Dx12,
}

/// Draws a recorded match offscreen and times every frame.
#[derive(clap::Args)]
pub struct Args {
    /// The replay to draw (`rl-cli stress --record`).
    #[arg(long, required_unless_present = "list_adapters")]
    replay: Option<PathBuf>,
    /// Output sizes, `WIDTHxHEIGHT` comma-separated.
    #[arg(long, default_value = "1920x1080,2560x1440,3440x1440,3840x2160")]
    sizes: String,
    #[arg(long, default_value_t = 1000)]
    start_tick: u32,
    #[arg(long, default_value_t = 600)]
    frames: u32,
    /// Probe pass counts (0, the baseline, is added first when missing).
    #[arg(long, value_delimiter = ',', default_value = "0,1,2,4,8,16,32")]
    probe_passes: Vec<u32>,
    #[arg(long, default_value_t = 9)]
    probe_taps: u32,
    #[arg(long, default_value_t = 120)]
    probe_frames: u32,
    #[arg(long, value_enum, default_value = "hardware")]
    adapter: Adapter,
    #[arg(long, value_enum, default_value = "auto")]
    backend: Backend,
    /// Imported content.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    #[arg(long)]
    out: Option<PathBuf>,
    /// Print every adapter wgpu finds and stop.
    #[arg(long)]
    list_adapters: bool,
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    let backends = match args.backend {
        Backend::Auto => None,
        Backend::Gl => Some(wgpu::Backends::GL),
        Backend::Vulkan => Some(wgpu::Backends::VULKAN),
        Backend::Dx12 => Some(wgpu::Backends::DX12),
    };
    if args.list_adapters {
        for a in rl_render::list_adapters(backends) {
            println!("{} ({:?}, {:?})", a.name, a.backend, a.device_type);
        }
        return Ok(true);
    }
    let path = args.replay.as_ref().context("--replay")?;
    let bytes = std::fs::read(path).with_context(|| format!("{}", path.display()))?;
    let replay = rl_replay::Replay::decode(&bytes).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let mut probe_passes = args.probe_passes.clone();
    if probe_passes.first() != Some(&0) {
        probe_passes.insert(0, 0);
    }
    let cfg = BenchConfig {
        sizes: parse_sizes(&args.sizes)?,
        start_tick: args.start_tick,
        frames: args.frames,
        probe_passes,
        probe_taps: args.probe_taps,
        probe_frames: args.probe_frames,
        options: OffscreenOptions {
            prefer: match args.adapter {
                Adapter::Hardware => AdapterPreference::Hardware,
                Adapter::Software => AdapterPreference::Software,
            },
            backends,
            timestamps: true,
        },
    };
    let content = rl_content::Content::open(&args.content)?;
    let report = gpu_bench::run(&content, &replay, &cfg, &mut |l| eprintln!("  {l}"))?;
    println!(
        "{} ({}, {}); GPU times: {}",
        report.adapter,
        report.backend,
        report.device_type,
        if report.timestamps { "timestamps" } else { "none (CPU-side frame times only)" }
    );
    for s in &report.sizes {
        let gpu = s.gpu_ms.map_or("-".into(), |g| format!("{:.2}/{:.2}", g.p50, g.p99));
        println!(
            "  {}x{}: tick+frame p50/p99 {:.2}/{:.2} ms, frame {:.2}/{:.2}, gpu {gpu}; probe ms {:?}",
            s.size.0,
            s.size.1,
            s.total_ms.p50,
            s.total_ms.p99,
            s.frame_ms.p50,
            s.frame_ms.p99,
            s.probe.iter().map(|p| (p.passes, (p.ms.p50 * 1000.0).round() / 1000.0)).collect::<Vec<_>>()
        );
    }
    if let Some(out) = &args.out {
        let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())?;
        std::fs::write(out, text + "\n").with_context(|| format!("{}", out.display()))?;
    }
    Ok(true)
}
```

In `crates/rl-cli/src/main.rs`: `mod gpu_bench_cmd;`, a variant

```rust
    /// Draw a recorded match offscreen at several sizes on the GPU, timing the CPU
    /// and (with timestamps) the GPU, plus the shader-headroom probe (plan 8).
    GpuBench(gpu_bench_cmd::Args),
```

and `Command::GpuBench(args) => gpu_bench_cmd::run(&args),`.

- [ ] **Step 5: Write the frame log**

In `crates/rl-game/src/stats.rs`, add the field `busy_ms: Vec<f32>` to `Meter` (`Vec::with_capacity(KEPT_FRAMES)` in `new`), push in `busy`:

```rust
        if self.busy_ms.len() < self.busy_ms.capacity() {
            self.busy_ms.push((seconds * 1000.0) as f32);
        }
```

and add:

```rust
/// Percentiles of a measurement (nearest rank).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pct {
    pub p50: f64,
    pub p99: f64,
    pub max: f64,
}

impl Pct {
    fn of(values: &[f32]) -> Pct {
        let mut v = values.to_vec();
        v.sort_by(f32::total_cmp);
        Pct {
            p50: f64::from(percentile(&v, 0.5)),
            p99: f64::from(percentile(&v, 0.99)),
            max: f64::from(v.last().copied().unwrap_or(0.0)),
        }
    }
}

/// `--frame-log` (the performance envelope, plan 8): a windowed run's
/// frames after the first `skipped_s` seconds (the window settling, a size
/// change), as `cargo xtask envelope` reads it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FrameLog {
    /// The surface's size when the run ended.
    pub size: (u32, u32),
    /// The adapter, backend and present mode.
    pub gpu: String,
    pub skipped_s: f64,
    pub frames: usize,
    pub seconds: f64,
    pub fps: f64,
    /// From frame to frame (what the player sees).
    pub frame_ms: Pct,
    /// The frame without the swap chain's waits.
    pub busy_ms: Pct,
    pub tick_ms: Pct,
}
```

and on `impl Meter`:

```rust
    /// The frames after the first `skip_s` seconds of the run (and the
    /// ticks after as many ticks at 100 a second), for `--frame-log`.
    pub fn frame_log(&self, skip_s: f64, size: (u32, u32), gpu: &str) -> FrameLog {
        let mut elapsed = 0.0;
        let first = self
            .frame_ms
            .iter()
            .position(|ms| {
                elapsed += f64::from(*ms) / 1000.0;
                elapsed >= skip_s
            })
            .map_or(self.frame_ms.len(), |i| i + 1);
        let frames = &self.frame_ms[first..];
        let busy = self.busy_ms.get(first..).unwrap_or(&[]);
        let skip_ticks = ((skip_s * 100.0).max(0.0) as usize).min(self.tick_ms.len());
        let seconds: f64 = frames.iter().map(|&m| f64::from(m) / 1000.0).sum();
        FrameLog {
            size,
            gpu: gpu.to_string(),
            skipped_s: skip_s,
            frames: frames.len(),
            seconds,
            fps: if seconds > 0.0 { frames.len() as f64 / seconds } else { 0.0 },
            frame_ms: Pct::of(frames),
            busy_ms: Pct::of(busy),
            tick_ms: Pct::of(&self.tick_ms[skip_ticks..]),
        }
    }
```

In `crates/rl-game/src/app.rs`: a field `drawn_size: (u32, u32),` on `App` (initialised `(0, 0)` in `App::new` beside `error: None`), set in the frame after `let size = w.gpu.size();` with `self.drawn_size = size;`, and:

```rust
    /// `--frame-log`: the run's frames after `skip_s` seconds (plan 8).
    pub fn frame_log(&self, skip_s: f64) -> crate::stats::FrameLog {
        self.meter.frame_log(skip_s, self.drawn_size, &self.stats.gpu)
    }
```

In `crates/rl-game/src/main.rs`, add to `Args`:

```rust
    /// Write the run's frame times (after --frame-log-skip seconds) here as
    /// RON, for `cargo xtask envelope`.
    #[arg(long)]
    frame_log: Option<PathBuf>,
    /// Seconds at the start the frame log leaves out (the window settling,
    /// a size change).
    #[arg(long, default_value_t = 3.0)]
    frame_log_skip: f64,
```

and in `run`, after `println!("rl-game: {}", app.latency_summary());`:

```rust
    if let Some(path) = &args.frame_log {
        let log = app.frame_log(args.frame_log_skip);
        let text = ron::ser::to_string_pretty(&log, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text + "\n").with_context(|| format!("{}", path.display()))?;
    }
```

- [ ] **Step 6: Run the pure tests**

Run: `cargo nextest run -p rl-cli --lib gpu_bench && cargo nextest run -p rl-game --lib stats`
Expected: PASS (`sizes_parse`, `a_software_adapter_is_refused_for_hardware_runs`, `frame_log_skips_the_first_seconds`, and the old stats tests).

- [ ] **Step 7: Write the GPU bench's GPU-tier test**

`crates/rl-cli/tests/gpu_bench.rs`:

```rust
//! `rl-cli gpu-bench` (plan 8) on the software adapter: a short stress
//! match recorded, then drawn at one size with the probe. Needs
//! `RL_GPU_TESTS` (`cargo xtask full`), content and an adapter.

use std::path::PathBuf;

use rl_cli::gpu_bench::{self, BenchConfig};
use rl_cli::stress::{self, Config};
use rl_render::OffscreenOptions;

#[test]
fn a_recorded_match_is_drawn_timed_and_probed() {
    if std::env::var_os("RL_GPU_TESTS").is_none() {
        eprintln!("RL_GPU_TESTS not set; skipping");
        return;
    }
    let Some(dir) = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set; skipping");
        return;
    };
    if rl_render::Offscreen::new().is_err() {
        eprintln!("no adapter; skipping");
        return;
    }
    let content = rl_content::Content::open(&dir).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("m.rlrp");
    let spec = stress::spec("olx-classic", "olx-level-dirt-level", 8, 3000, 3.0, 1);
    stress::run(&content, &Config { verify: false, record: Some(file.clone()), ..Config::new(spec) }, &mut |_| {})
        .unwrap();
    let replay = rl_replay::Replay::decode(&std::fs::read(&file).unwrap()).unwrap();
    let cfg = BenchConfig {
        sizes: vec![(320, 180)],
        start_tick: 100,
        frames: 50,
        probe_passes: vec![0, 2],
        probe_taps: 4,
        probe_frames: 5,
        options: OffscreenOptions { timestamps: true, ..OffscreenOptions::default() },
    };
    let r = gpu_bench::run(&content, &replay, &cfg, &mut |_| {}).unwrap();
    let s = &r.sizes[0];
    assert_eq!((s.size, s.frames, s.probe.len()), ((320, 180), 50, 2));
    assert!(s.total_ms.p50 >= s.frame_ms.p50 && s.frame_ms.p50 > 0.0, "{s:?}");
    assert_eq!(s.gpu_ms.is_some(), r.timestamps);
    let mut tampered = replay.clone();
    *tampered.row_mut(150).first_mut().unwrap() = rl_sim::Keys(0xFF);
    assert!(gpu_bench::run(&content, &tampered, &cfg, &mut |_| {}).is_err(), "a different match is refused");
}
```

(`rl_sim::Keys` is a tuple struct over the key bits, as `rl-scenario` builds it; if the row at 150 already holds `0xFF`, use `0x00`.)

Add `("rl-cli", "gpu_bench")` to `GPU_TESTS` in `xtask/src/check.rs`.

Run: `RL_GPU_TESTS=1 cargo nextest run -p rl-cli --test gpu_bench`
Expected: PASS on lavapipe.

- [ ] **Step 8: Try both on the real GPU (manual)**

```bash
cargo build --release -p rl-cli -p rl-game
mkdir -p target/envelope/replays
target/release/rl-cli stress --mod olx-mw-1-0 --level olx-level-castlestrike --worms 32 --pool 20000 --seconds 20 --render-every 0 --no-verify --record target/envelope/replays/heavy.rlrp
GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA target/release/rl-cli gpu-bench --replay target/envelope/replays/heavy.rlrp --sizes 2560x1440 --frames 200 --probe-passes 0,4,16
printf '0 size 1920 1080\n' > target/envelope/size.txt
target/release/rl-game --replay target/envelope/replays/heavy.rlrp --uncapped --window-system x11 --no-sound --autoquit-after 10 --script target/envelope/size.txt --frame-log target/envelope/frame-log.ron
cat target/envelope/frame-log.ron
```

Expected: `gpu-bench` prints `NVIDIA GeForce RTX 5070 Ti` (or `D3D12 (NVIDIA GeForce RTX 5070 Ti)`) with backend `Gl` and "GPU times: timestamps" (finding 1; if it says "none", record it: the CPU fallback is what this machine gets); a window plays the replay for 10 s; the frame log shows `size: (1920, 1080)` and an `fps` above 60. If the `size` script command did not take (size stays 1280 × 720), record it for Task 7 (it then uses the frame log's size and marks the point "not reached").

- [ ] **Step 9: Docs, gate, commit**

- `crates/rl-cli/CLAUDE.md`: the `gpu-bench` command (flags, the report's fields, `check_adapter`, the checksum check at the end, `tests/gpu_bench.rs` in the GPU tier).
- `crates/rl-game/CLAUDE.md` **Scripts** bullet: `--frame-log F --frame-log-skip S` (`stats::FrameLog`: frames after `S` seconds, the surface size at the end, the GPU line; for `cargo xtask envelope`).
- `xtask/CLAUDE.md`: the GPU tier runs `rl-cli`'s `gpu_bench` test too.

Run: `cargo xtask check`
Expected: `check passed`.

```bash
git add crates/rl-cli crates/rl-game xtask/src/check.rs xtask/CLAUDE.md
git commit -F - <<'EOF'
rl-cli: add gpu-bench; rl-game: add --frame-log

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

### Task 5: The Windows build run from WSL (`cargo xtask windows`)

**Files:**
- Create: `xtask/src/winbuild.rs`
- Modify: `xtask/src/main.rs` (`mod winbuild;`, the `Windows` command)
- Docs: `xtask/CLAUDE.md`, `README.md` (WSL section: the Windows build)

**Interfaces:**
- Consumes: Task 4's `rl-cli gpu-bench --list-adapters`, `--backend`.
- Produces (`crate::winbuild`):
  - constants `TARGET = "x86_64-pc-windows-gnu"`, `LINKER`, `OBJDUMP`, `CMD = "/mnt/c/Windows/System32/cmd.exe"`, `EXES`, `MINGW_DLLS`, `STAGE = "rust-liero-envelope"`;
  - `missing() -> Option<String>` (why the Windows path cannot run here);
  - `build() -> Result<PathBuf, String>` (the release directory of the Windows exes);
  - `imported_dlls(objdump: &str) -> Vec<String>`, `to_wsl_path(win: &str) -> Option<PathBuf>`, `wslenv(existing: Option<&str>, names: &[&str]) -> String`, `windows_temp() -> Result<PathBuf, String>`;
  - `struct Stage { pub dir: PathBuf }`, `stage(release: &Path, content: &Path, files: &[PathBuf]) -> Result<Stage, String>` (files go to `<stage>/replays/`);
  - `run(stage: &Stage, exe: &str, args: &[String], env: &[(&str, &str)]) -> Result<Output, String>`;
  - `command() -> anyhow::Result<bool>` (`cargo xtask windows`).

- [ ] **Step 1: Install the target (one time)**

Run (from `/home/martin/rust-liero`, so the pinned 1.99 toolchain gets it): `rustup target add x86_64-pc-windows-gnu`
Expected: `info: component 'rust-std' for target 'x86_64-pc-windows-gnu' is up to date` or installed. Then `which x86_64-w64-mingw32-gcc x86_64-w64-mingw32-objdump` prints both paths (if `objdump` is missing: `sudo apt install binutils-mingw-w64-x86-64` and tell the user).

- [ ] **Step 2: Write the failing tests**

`xtask/src/winbuild.rs` begins with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_map_to_mnt() {
        assert_eq!(
            to_wsl_path("C:\\Users\\Martin\\AppData\\Local\\Temp\r\n"),
            Some(PathBuf::from("/mnt/c/Users/Martin/AppData/Local/Temp"))
        );
        assert_eq!(to_wsl_path("D:/x"), Some(PathBuf::from("/mnt/d/x")));
        assert_eq!(to_wsl_path("\\\\wsl.localhost\\x"), None);
        assert_eq!(to_wsl_path(""), None);
    }

    #[test]
    fn imported_dlls_are_read_from_objdump() {
        let dump = "The Import Tables\n\tDLL Name: KERNEL32.dll\n\tvma: x\n\tDLL Name: libwinpthread-1.dll\n";
        assert_eq!(imported_dlls(dump), ["KERNEL32.dll", "libwinpthread-1.dll"]);
    }

    #[test]
    fn wslenv_adds_names_once() {
        assert_eq!(wslenv(None, &["WGPU_BACKEND"]), "WGPU_BACKEND");
        assert_eq!(wslenv(Some("WT_SESSION:WGPU_BACKEND/u"), &["WGPU_BACKEND", "X"]), "WT_SESSION:WGPU_BACKEND/u:X");
    }
}
```

Add `mod winbuild;` to `xtask/src/main.rs`.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo nextest run -p xtask winbuild`
Expected: FAIL to compile ("cannot find function `to_wsl_path`").

- [ ] **Step 4: Write the module**

Above the tests:

```rust
//! The native Windows build (plan 8): `rl-cli` and `rl-game`
//! cross-compiled for `x86_64-pc-windows-gnu` with mingw-w64, staged with
//! the content in Windows' temp directory (`%TEMP%\rust-liero-envelope`)
//! and run from WSL through interop: nothing is installed on Windows.
//! Interop forwards only the variables `WSLENV` names, so `run` adds them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::root;

pub const TARGET: &str = "x86_64-pc-windows-gnu";
pub const LINKER: &str = "x86_64-w64-mingw32-gcc";
pub const OBJDUMP: &str = "x86_64-w64-mingw32-objdump";
/// `appendWindowsPath` may be off: `cmd.exe` by its full path.
pub const CMD: &str = "/mnt/c/Windows/System32/cmd.exe";
pub const EXES: [&str; 2] = ["rl-cli.exe", "rl-game.exe"];
/// mingw's runtime DLLs an exe may import; copied beside it when it does.
pub const MINGW_DLLS: [&str; 3] = ["libwinpthread-1.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll"];
/// The stage's directory name in `%TEMP%`.
pub const STAGE: &str = "rust-liero-envelope";

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Why the Windows path cannot run here, if it cannot (a notice names the fix).
pub fn missing() -> Option<String> {
    if !Path::new(CMD).exists() {
        return Some(format!("no Windows interop ({CMD} is missing)"));
    }
    let interop = std::fs::read_to_string("/proc/sys/fs/binfmt_misc/WSLInterop").unwrap_or_default();
    if !interop.starts_with("enabled") {
        return Some("Windows interop is disabled (/proc/sys/fs/binfmt_misc/WSLInterop)".into());
    }
    let installed = Command::new("rustup")
        .current_dir(root())
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    if !installed.lines().any(|l| l.trim() == TARGET) {
        return Some(format!("the Rust target is missing: run `rustup target add {TARGET}` in the repository"));
    }
    for tool in [LINKER, OBJDUMP] {
        if Command::new(tool).arg("--version").output().is_err() {
            return Some(format!("no {tool}: `sudo apt install mingw-w64`"));
        }
    }
    None
}

/// Builds `rl-cli` and `rl-game` for Windows (release); gives their directory.
pub fn build() -> Result<PathBuf, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root())
        .args(["build", "--quiet", "--release", "--target", TARGET, "-p", "rl-cli", "-p", "rl-game"])
        .env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", LINKER)
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!("the Windows build failed:\n{}", tail(&String::from_utf8_lossy(&out.stderr), 40)));
    }
    Ok(root().join("target").join(TARGET).join("release"))
}

/// The DLLs `objdump -p` says an exe imports.
pub fn imported_dlls(objdump: &str) -> Vec<String> {
    objdump.lines().filter_map(|l| l.trim().strip_prefix("DLL Name:")).map(|n| n.trim().to_string()).collect()
}

/// `C:\Users\x` as `/mnt/c/Users/x` (`None` for anything but a drive path).
pub fn to_wsl_path(win: &str) -> Option<PathBuf> {
    let win = win.trim();
    let (drive, rest) = win.split_once(":\\").or_else(|| win.split_once(":/"))?;
    let mut d = drive.chars();
    let letter = d.next().filter(|c| c.is_ascii_alphabetic() && d.next().is_none())?;
    Some(PathBuf::from(format!("/mnt/{}/{}", letter.to_ascii_lowercase(), rest.replace('\\', "/"))))
}

/// `existing` `WSLENV` with `names` added once each.
pub fn wslenv(existing: Option<&str>, names: &[&str]) -> String {
    let mut parts: Vec<String> =
        existing.unwrap_or("").split(':').filter(|s| !s.is_empty()).map(String::from).collect();
    for n in names {
        if !parts.iter().any(|p| p.split('/').next() == Some(*n)) {
            parts.push((*n).to_string());
        }
    }
    parts.join(":")
}

/// Windows' `%TEMP%`, through `cmd.exe` started in `/mnt/c` (a Linux
/// working directory would make it warn about UNC paths).
pub fn windows_temp() -> Result<PathBuf, String> {
    let out = Command::new(CMD)
        .current_dir("/mnt/c")
        .args(["/c", "echo %TEMP%"])
        .output()
        .map_err(|e| format!("cannot run {CMD}: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    to_wsl_path(text.lines().rfind(|l| !l.trim().is_empty()).unwrap_or(""))
        .ok_or_else(|| format!("cmd.exe gave no temp directory: {text:?}"))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(drop).map_err(|e| format!("copying {} to {}: {e}", from.display(), to.display()))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() { copy_dir(&src, &dst)? } else { copy(&src, &dst)? }
    }
    Ok(())
}

/// The content's file count and bytes: copied again only when it changes.
fn content_stamp(dir: &Path) -> Result<String, String> {
    fn walk(d: &Path, n: &mut u64, bytes: &mut u64) -> Result<(), String> {
        for e in std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                walk(&p, n, bytes)?;
            } else {
                *n += 1;
                *bytes += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            }
        }
        Ok(())
    }
    let (mut n, mut bytes) = (0, 0);
    walk(dir, &mut n, &mut bytes)?;
    Ok(format!("{n} files, {bytes} bytes"))
}

fn mingw_dll(name: &str) -> Option<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/x86_64-w64-mingw32/lib")];
    if let Ok(rd) = std::fs::read_dir("/usr/lib/gcc/x86_64-w64-mingw32") {
        let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        v.sort();
        dirs.extend(v.into_iter().rev());
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// A staged Windows build.
pub struct Stage {
    pub dir: PathBuf,
}

/// Copies the exes (with any mingw DLL they import), the content (when it
/// changed) and `files` (into `replays/`) to `%TEMP%\rust-liero-envelope`.
pub fn stage(release: &Path, content: &Path, files: &[PathBuf]) -> Result<Stage, String> {
    let dir = windows_temp()?.join(STAGE);
    std::fs::create_dir_all(dir.join("replays")).map_err(|e| format!("{}: {e}", dir.display()))?;
    for exe in EXES {
        let src = release.join(exe);
        copy(&src, &dir.join(exe))?;
        let dump = Command::new(OBJDUMP).arg("-p").arg(&src).output().map_err(|e| format!("{OBJDUMP}: {e}"))?;
        for dll in imported_dlls(&String::from_utf8_lossy(&dump.stdout)) {
            if MINGW_DLLS.iter().any(|d| d.eq_ignore_ascii_case(&dll)) {
                let found = mingw_dll(&dll).ok_or_else(|| format!("{exe} needs {dll}, not in mingw-w64's directories"))?;
                copy(&found, &dir.join(&dll))?;
            }
        }
    }
    let stamp = content_stamp(content)?;
    let stamp_file = dir.join("content.stamp");
    if std::fs::read_to_string(&stamp_file).ok().as_deref() != Some(stamp.as_str()) {
        println!("  copying the content to {} (once)", dir.display());
        let _ = std::fs::remove_dir_all(dir.join("content"));
        copy_dir(content, &dir.join("content"))?;
        std::fs::write(&stamp_file, &stamp).map_err(|e| format!("{}: {e}", stamp_file.display()))?;
    }
    for f in files {
        copy(f, &dir.join("replays").join(f.file_name().unwrap_or_default()))?;
    }
    Ok(Stage { dir })
}

/// Runs `exe` of `stage` with the stage as its working directory (paths in
/// `args` are relative to it); `env` reaches Windows through `WSLENV`.
pub fn run(stage: &Stage, exe: &str, args: &[String], env: &[(&str, &str)]) -> Result<Output, String> {
    let mut c = Command::new(stage.dir.join(exe));
    c.current_dir(&stage.dir).args(args);
    if !env.is_empty() {
        for (k, v) in env {
            c.env(k, v);
        }
        let names: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
        c.env("WSLENV", wslenv(std::env::var("WSLENV").ok().as_deref(), &names));
    }
    c.output().map_err(|e| format!("cannot run {exe} through Windows interop: {e}"))
}

/// `cargo xtask windows`: builds, stages, and lists what each backend sees.
pub fn command() -> anyhow::Result<bool> {
    if let Some(why) = missing() {
        println!("FAIL windows: {why}");
        return Ok(false);
    }
    println!("building rl-cli and rl-game for {TARGET} (release; the first build takes minutes)...");
    let release = build().map_err(anyhow::Error::msg)?;
    let stage = stage(&release, &root().join("content/packs"), &[]).map_err(anyhow::Error::msg)?;
    println!("staged in {}", stage.dir.display());
    for backend in ["dx12", "vulkan"] {
        let args = ["gpu-bench", "--list-adapters", "--backend", backend].map(String::from);
        let out = run(&stage, "rl-cli.exe", &args, &[]).map_err(anyhow::Error::msg)?;
        println!("{backend}:\n{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }
    Ok(true)
}
```

In `xtask/src/main.rs`, add the command:

```rust
    /// Build rl-cli and rl-game for Windows (x86_64-pc-windows-gnu), stage them with the
    /// content in Windows' temp directory and list the adapters each backend sees (plan 8).
    Windows,
```

and `Command::Windows => winbuild::command(),`.

- [ ] **Step 5: Run the tests**

Run: `cargo nextest run -p xtask winbuild`
Expected: PASS (3 tests).

- [ ] **Step 6: Build and run on Windows (manual)**

Run: `cargo xtask windows`
Expected: the build finishes, "staged in /mnt/c/Users/Martin/AppData/Local/Temp/rust-liero-envelope", and the `dx12:` list names `NVIDIA GeForce RTX 5070 Ti (Dx12, DiscreteGpu)`, the `vulkan:` list `NVIDIA GeForce RTX 5070 Ti (Vulkan, DiscreteGpu)`. On each outcome:
- **Link errors** naming `-lwinpthread` or an unresolved `__imp_` symbol: retry with `RUSTFLAGS="-C target-feature=+crt-static"` in `build()`'s environment (add `.env("RUSTFLAGS", "-C target-feature=+crt-static")`), rerun, and note it in `xtask/CLAUDE.md`.
- **The exe starts and exits with 0xC0000135** (a missing DLL): run `x86_64-w64-mingw32-objdump -p target/x86_64-pc-windows-gnu/release/rl-cli.exe | grep "DLL Name"`; add any non-system DLL it names to `MINGW_DLLS`.
- **"Exec format error" or permission denied** on `/mnt/c`: interop is off for this distro; `missing()` must then report it — fix `missing()` to catch that case and rerun.
- **An empty Vulkan list**: Windows has no Vulkan driver for this GPU; Task 7 records the Vulkan path as skipped with that notice.

Then a smoke run with the heavy replay from Task 4 Step 8:
```bash
cp target/envelope/replays/heavy.rlrp /mnt/c/Users/Martin/AppData/Local/Temp/rust-liero-envelope/replays/
cd /mnt/c/Users/Martin/AppData/Local/Temp/rust-liero-envelope && ./rl-cli.exe gpu-bench --replay replays/heavy.rlrp --content content --backend dx12 --sizes 2560x1440 --frames 200 --probe-passes 0,4; cd -
```
Expected: the report names the RTX on `Dx12` with "GPU times: timestamps". If the content cannot be read ("content directory content: ..."), check the stage holds `content/olx-core/pack.ron`; the stamp logic copies it.

- [ ] **Step 7: Docs, gate, commit**

- `xtask/CLAUDE.md`: a `windows` entry (`winbuild.rs`: `missing`, `build` with the mingw linker, `stage` in `%TEMP%\rust-liero-envelope` with the content stamp and mingw DLLs, `run` with `WSLENV`, what the manual run showed).
- `README.md` "Windows Subsystem for Linux (development)": "`cargo xtask windows` builds `rl-cli` and `rl-game` for Windows with mingw-w64 (`rustup target add x86_64-pc-windows-gnu` once) and runs them through interop; nothing is installed on Windows."

Run: `cargo xtask check`
Expected: `check passed`.

```bash
git add xtask README.md
git commit -F - <<'EOF'
xtask: build and run rl-cli and rl-game for Windows from WSL

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

### Task 6: OLX frame timing (`ORACLE_TIMING`) and `rl-cli scenario --time`

**Files:**
- Modify: `tools/olx-oracle/oracle-hooks.patch` (regenerated: `src/common/OracleHooks.cpp`, `include/OracleHooks.h`)
- Create: `fixtures/perf/olx/heavy-spikeballs.ron`, `fixtures/perf/olx/heavy-napalm.ron`, `crates/rl-cli/src/timing.rs`, `xtask/src/olx_timing.rs`
- Modify: `crates/rl-cli/src/lib.rs`, `crates/rl-cli/src/scenario.rs` (`--time`), `xtask/src/oracle.rs` (`record_once_with`; `ensure_image` and `id` become `pub(crate)`), `xtask/src/main.rs` (`mod olx_timing;`)
- Test: unit tests in `xtask/src/olx_timing.rs`; `crates/rl-cli/tests/scenario.rs`
- Docs: `tools/olx-oracle/CLAUDE.md`, `crates/rl-cli/CLAUDE.md`, `xtask/CLAUDE.md`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - OLX: with `ORACLE_TIMING=<file>`, each recorded frame writes `frame ns projectiles rc` to that file; with `ORACLE_HEADER_ONLY` set, the trace holds the header and frame −1 only. Unset, nothing changes (traces byte-identical).
  - `rl_cli::timing::{ScenarioTiming, TimingReport, time_scenario}`: `ScenarioTiming { name: String, tick_ns: Vec<u64>, projectiles: Vec<u32>, rand_calls: Vec<u64>, matches_olx: Option<bool> }`, `TimingReport { scenarios: Vec<ScenarioTiming> }`, `time_scenario(content: &Content, name: &str, sc: &Scenario, expected: &Trace) -> anyhow::Result<ScenarioTiming>`.
  - `rl-cli scenario [name...] --time FILE.ron`.
  - `crate::olx_timing::{PERF_DIR, TIMED, HEAVY, OlxFrame, parse_timing, Olx}` with `Olx::open() -> anyhow::Result<Olx>`, `Olx::image(&self) -> &str`, `Olx::time(&self, name: &str, sc: &Scenario, out_gz: Option<&Path>) -> anyhow::Result<Vec<OlxFrame>>`, `Olx::build_info(&self) -> String`; `OlxFrame { frame: i64, ns: u64, projectiles: u32, rand_calls: u64 }`.
  - `crate::oracle::record_once_with(image: &str, user: &str, work: &Path, sc: &Scenario, env: &[(&str, &str)]) -> anyhow::Result<String>`.

- [ ] **Step 1: Write the failing tests**

`xtask/src/olx_timing.rs` starts with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_lines_parse() {
        let f = parse_timing("0 1500 2 17\n1 2500 4 30\n").unwrap();
        assert_eq!(f[1], OlxFrame { frame: 1, ns: 2500, projectiles: 4, rand_calls: 30 });
        assert!(parse_timing("0 1500 2\n").is_err());
        assert!(parse_timing("x 1 2 3\n").is_err());
        assert_eq!(parse_timing("\n").unwrap(), []);
    }

    #[test]
    fn the_timed_scenarios_exist() {
        for name in TIMED {
            assert!(root().join("fixtures/oracle").join(format!("{name}.ron")).is_file(), "{name}");
        }
        for name in HEAVY {
            let p = root().join(PERF_DIR).join(format!("{name}.ron"));
            let sc = Scenario::from_ron(&std::fs::read_to_string(&p).unwrap()).unwrap();
            assert!(sc.worms.len() >= 8 && sc.frames >= 500, "{name}");
            assert_eq!(sc.settings.get("GameOptions.GameInfo.LoadingTime").map(String::as_str), Some("0"));
        }
    }
}
```

Add `mod olx_timing;` to `xtask/src/main.rs`.

Add to `crates/rl-cli/tests/scenario.rs` (it already finds the content the same way its other tests do; reuse its helper):

```rust
/// `rl-cli scenario --time` (plan 8): every frame timed, with the work
/// done (projectiles, rand() calls) and whether it matched OLX.
#[test]
fn scenarios_are_timed_frame_by_frame() {
    let Some(dir) = content_dir() else { return };
    let content = rl_content::Content::open(std::path::Path::new(&dir)).unwrap();
    let (sc, trace) = rl_scenario::load_fixture(&fixtures(), "two-worms-fire").unwrap();
    let t = rl_cli::timing::time_scenario(&content, "two-worms-fire", &sc, &trace).unwrap();
    assert_eq!(t.tick_ns.len(), sc.frames as usize);
    assert_eq!((t.projectiles.len(), t.rand_calls.len()), (t.tick_ns.len(), t.tick_ns.len()));
    assert_eq!(t.matches_olx, Some(true));
    assert!(t.projectiles.iter().any(|&n| n > 10) && t.tick_ns.iter().all(|&ns| ns > 0));
    let mut header_only = trace.clone();
    header_only.frames.truncate(1);
    assert_eq!(rl_cli::timing::time_scenario(&content, "x", &sc, &header_only).unwrap().matches_olx, None);
}
```

(`content_dir()` and `fixtures()` are the file's existing helpers.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p xtask olx_timing; cargo nextest run -p rl-cli --test scenario`
Expected: FAIL to compile ("cannot find function `parse_timing`", "could not find `timing` in `rl_cli`").

- [ ] **Step 3: The heavy scenarios**

`fixtures/perf/olx/heavy-spikeballs.ron`:

```ron
(
    description: "Performance envelope (plan 8), not an oracle fixture: eight worms in Castle Strike's great hall (four at each of castle-shoot-rope's spots, 10000 hp so nobody dies) hold fire with Classic's Spikeballs and LoadingTime 0 for 8 s, to fill OLX's 3000 projectiles. Timed in OLX and in rl-sim; the same work is checked frame by frame by projectile counts and rand() calls.",
    level: "CastleStrike.lxl",
    mod: "Classic",
    seed: 12345,
    frames: 800,
    settings: {"GameOptions.GameInfo.LoadingTime": "0"},
    worms: [
        (x: 400, y: 320), (x: 400, y: 320), (x: 400, y: 320), (x: 400, y: 320),
        (x: 470, y: 320), (x: 470, y: 320), (x: 470, y: 320), (x: 470, y: 320),
    ],
    weapons: [
        (worm: 0, names: ["Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs"], current: 0),
        (worm: 1, names: ["Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs"], current: 0),
        (worm: 2, names: ["Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs"], current: 0),
        (worm: 3, names: ["Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs"], current: 0),
        (worm: 4, names: ["Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs"], current: 0),
        (worm: 5, names: ["Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs"], current: 0),
        (worm: 6, names: ["Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs"], current: 0),
        (worm: 7, names: ["Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs", "Spikeballs"], current: 0),
    ],
    health: [(0, 10000.0), (1, 10000.0), (2, 10000.0), (3, 10000.0), (4, 10000.0), (5, 10000.0), (6, 10000.0), (7, 10000.0)],
    inputs: [
        (frame: 0, worm: 4, keys: "left"), (frame: 0, worm: 5, keys: "left"),
        (frame: 0, worm: 6, keys: "left"), (frame: 0, worm: 7, keys: "left"),
        (frame: 3, worm: 4, keys: ""), (frame: 3, worm: 5, keys: ""),
        (frame: 3, worm: 6, keys: ""), (frame: 3, worm: 7, keys: ""),
        (frame: 5, worm: 0, keys: "fire"), (frame: 5, worm: 1, keys: "up+fire"),
        (frame: 5, worm: 2, keys: "fire"), (frame: 5, worm: 3, keys: "up+fire"),
        (frame: 5, worm: 4, keys: "fire"), (frame: 5, worm: 5, keys: "up+fire"),
        (frame: 5, worm: 6, keys: "fire"), (frame: 5, worm: 7, keys: "up+fire"),
        (frame: 25, worm: 1, keys: "fire"), (frame: 25, worm: 3, keys: "fire"),
        (frame: 25, worm: 5, keys: "fire"), (frame: 25, worm: 7, keys: "fire"),
    ],
)
```

`fixtures/perf/olx/heavy-napalm.ron`: the same structure on `"Liero Factory.lxl"` with the spots `(x: 190, y: 330)` (worms 0–3) and `(x: 240, y: 330)` (worms 4–7), every slot `"Napalm"`, description "… Liero Factory's floor … Classic's Napalm …", the same settings, health and inputs.

- [ ] **Step 4: Patch OLX**

```bash
rm -rf target/olx-patch && cp -r target/olx-src/d1ad953f target/olx-patch
cd target/olx-patch && git apply ../../tools/olx-oracle/oracle-hooks.patch && cd -
```

In `target/olx-patch/src/common/OracleHooks.cpp`:
1. Add `#include <chrono>` to the standard includes (after `#include <cctype>`).
2. After `static FILE* g_rec = NULL;` add:

```cpp
static FILE* g_timing = NULL;          // ORACLE_TIMING: "frame ns projectiles rc" lines (rust-liero plan 8)
static bool g_headerOnly = false;      // ORACLE_HEADER_ONLY: the trace keeps the header and frame -1
static std::chrono::steady_clock::time_point g_frameStart;
```

3. In `record()`, inside the first `if(g_rec) { ... }` after `fclose(g_rec); g_rec = NULL;` add `if(g_timing) { fclose(g_timing); g_timing = NULL; }`; after `if(!g_rec) { err = "cannot write " + file; return false; }` add:

```cpp
	// Bench only (rust-liero plan 8): frame times to a file of their own, so
	// the trace is untouched; a header-only trace for the timed runs.
	if(const char* path = getenv("ORACLE_TIMING")) {
		g_timing = fopen(path, "w");
		if(!g_timing) { err = std::string("cannot write ") + path; return false; }
	}
	g_headerOnly = getenv("ORACLE_HEADER_ONLY") != NULL;
```

4. At the very end of `beforeFrame()` add `if(g_timing) g_frameStart = std::chrono::steady_clock::now();` (while recording, `g_base >= 0`, so the early return never skips it).
5. Replace the first line of `afterFrame()` (`if(g_rec) writeState(frameIndex());`) with:

```cpp
	if(g_timing) {
		const long long ns = (long long)std::chrono::duration_cast<std::chrono::nanoseconds>(
			std::chrono::steady_clock::now() - g_frameStart).count();
		auto& ps = cClient->getProjectiles();
		int projectiles = 0;
		for(int i = 0; i <= ps.lastUsed(); i++)
			if(ps.isUsed(i)) projectiles++;
		fprintf(g_timing, "%ld %lld %d %llu\n", frameIndex(), ns, projectiles, g_randCalls);
	}
	if(g_rec && !g_headerOnly) writeState(frameIndex());
```

In `target/olx-patch/include/OracleHooks.h`, extend the comment above `record`:

```cpp
	// Bench only (rust-liero plan 8): with ORACLE_TIMING=<file> in the environment, each
	// recorded frame's wall time (from the end of beforeFrame() to the start of
	// afterFrame()), projectile count and rand() calls go to that file as
	// "frame ns projectiles rc" lines; with ORACLE_HEADER_ONLY set, the trace keeps only
	// the header and frame -1. Without them nothing changes.
```

Regenerate the patch:

```bash
cd target/olx-patch && git add -N include/OracleHooks.h src/common/OracleHooks.cpp && git diff > ../../tools/olx-oracle/oracle-hooks.patch && cd -
git diff --stat tools/olx-oracle/oracle-hooks.patch
```

Expected: only `oracle-hooks.patch` changes, by about 30 added lines in the `OracleHooks.cpp` and `OracleHooks.h` hunks; `git diff tools/olx-oracle/oracle-hooks.patch` shows no other hunk changed (if `git diff` reordered files or changed context in other hunks, the checkout was not clean: redo from `rm -rf`).

- [ ] **Step 5: Prove the traces did not move**

Run: `cargo xtask oracle-record --check`
Expected: the new image builds (minutes), then `ok   <name>: matches the committed trace (N frames)` for every scenario and no `FAIL`. Docker must be running (`docker info`); if it is not, start it (`sudo service docker start`) and rerun.

- [ ] **Step 6: The timing module in `rl-cli`**

`crates/rl-cli/src/timing.rs`:

```rust
//! Oracle scenarios timed in `rl-sim` (plan 8, the OLX comparison): each
//! frame's `World::tick`, then the work it did (projectiles, `rand()`
//! calls since the seed), to set beside OLX's frame times of the same
//! scenario (`tools/olx-oracle`'s `ORACLE_TIMING`). The trace record of a
//! frame is made outside the timed span.

use std::time::Instant;

use rl_content::Content;
use rl_oracle::{Scenario, Trace};
use serde::{Deserialize, Serialize};

/// One scenario's frames.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioTiming {
    pub name: String,
    /// Frames 0..n: the tick (and a scenario bonus placed before it), ns.
    pub tick_ns: Vec<u64>,
    pub projectiles: Vec<u32>,
    pub rand_calls: Vec<u64>,
    /// Whether the simulation reproduced OLX's trace in every checked
    /// field; `None` when the trace holds only frame -1 (a timing trace).
    pub matches_olx: Option<bool>,
}

/// What `rl-cli scenario --time` writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TimingReport {
    pub scenarios: Vec<ScenarioTiming>,
}

/// Plays `sc` from `expected`'s header and frame -1, timing each frame.
pub fn time_scenario(content: &Content, name: &str, sc: &Scenario, expected: &Trace) -> anyhow::Result<ScenarioTiming> {
    let t0: u64 = expected.header.options.get("T0").and_then(|t| t.parse().ok()).unwrap_or(0);
    let mut out = ScenarioTiming { name: name.into(), ..ScenarioTiming::default() };
    let mut last = Instant::now();
    rl_scenario::play(content, sc, expected, |world, f, _| {
        let ns = last.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        if f >= 0 {
            let frame = rl_scenario::record(world, f, t0);
            out.tick_ns.push(ns);
            out.projectiles.push(frame.p.len() as u32);
            out.rand_calls.push(frame.rc.unwrap_or(0));
        }
        last = Instant::now();
    })?;
    if expected.frames.len() > 1 {
        let actual = rl_scenario::replay(content, sc, expected)?;
        out.matches_olx =
            Some(rl_oracle::compare_fields(expected, &actual, &sc.tolerances, rl_scenario::CHECKED).is_none());
    }
    Ok(out)
}
```

Add `pub mod timing;` to `lib.rs`. In `crates/rl-cli/src/scenario.rs`, add to `Args`:

```rust
    /// Time every frame in rl-sim and write the times as RON here (plan 8;
    /// a fixture with a header-only trace is timed without comparing).
    #[arg(long)]
    time: Option<PathBuf>,
```

and at the start of `run`, after `let content = ...`:

```rust
    if let Some(out) = &args.time {
        let mut report = rl_cli::timing::TimingReport::default();
        let mut ok = true;
        for name in &names {
            let (sc, expected) = rl_scenario::load_fixture(&args.fixtures, name)?;
            let t = rl_cli::timing::time_scenario(&content, name, &sc, &expected)?;
            let mut v = t.tick_ns.clone();
            v.sort_unstable();
            println!(
                "time {name}: {} frames, tick p50 {:.1} µs, peak {} projectiles, matches OLX: {:?}",
                v.len(),
                v.get(v.len() / 2).copied().unwrap_or(0) as f64 / 1000.0,
                t.projectiles.iter().max().copied().unwrap_or(0),
                t.matches_olx
            );
            ok &= t.matches_olx != Some(false);
            report.scenarios.push(t);
        }
        std::fs::write(out, ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())? + "\n")?;
        return Ok(ok);
    }
```

(`names`, `content` are computed above it as today; move the `--time` block after both.)

- [ ] **Step 7: The OLX side in `xtask`**

In `xtask/src/oracle.rs`, make `ensure_image` and `id` `pub(crate)`, and split `record_once`:

```rust
fn record_once(image: &str, user: &str, work: &Path, sc: &Scenario) -> anyhow::Result<String> {
    record_once_with(image, user, work, sc, &[])
}

/// [`record_once`] with `env` set for OLX (`ORACLE_TIMING`,
/// `ORACLE_HEADER_ONLY`: the envelope's timed runs, plan 8).
pub(crate) fn record_once_with(
    image: &str,
    user: &str,
    work: &Path,
    sc: &Scenario,
    env: &[(&str, &str)],
) -> anyhow::Result<String> {
    if work.exists() {
        std::fs::remove_dir_all(work)?;
    }
    std::fs::create_dir_all(work.join("home"))?;
    std::fs::write(work.join("scenario.json"), sc.control_json("/work/trace.jsonl"))?;
    let mut c = Command::new("docker");
    c.args(["run", "--rm", "--init", "-u", user, "-v"])
        .arg(format!("{}:/work", work.display()))
        .args(["-e", "HOME=/work/home", "-e", "ORACLE_SCENARIO=/work/scenario.json"]);
    for (k, v) in env {
        c.arg("-e").arg(format!("{k}={v}"));
    }
    let out = c
        .arg(image)
        .args(["timeout", "-s", "KILL", &RUN_TIMEOUT_SECS.to_string(), "/olx/bin/openlierox"])
        .args(["-dedicated", "-disablestdincli", "-script", "/olx/oracle_control.py"])
        .output()
        .context("running docker")?;
    // ... the rest of the old body unchanged (log, errors, ORACLE_DONE, read trace.jsonl) ...
}
```

Above the tests in `xtask/src/olx_timing.rs`:

```rust
//! OLX's frame times for the performance envelope (plan 8): the oracle's
//! patched OLX (Docker) plays a scenario with `ORACLE_TIMING` (each frame's
//! wall time, projectile count and `rand()` calls, to a file of its own:
//! the trace is untouched) and `ORACLE_HEADER_ONLY` (the trace keeps the
//! header and frame -1, which `rl-cli scenario --time` needs to build the
//! same world). OLX runs in a container on WSL's own kernel: no extra
//! virtualisation, the same clock.

use std::path::Path;

use anyhow::Context as _;
use rl_oracle::Scenario;

use crate::{oracle, root};

/// The heavy timing scenarios (not oracle fixtures: no committed trace).
pub const PERF_DIR: &str = "fixtures/perf/olx";
/// The busiest oracle scenarios (by trace size), timed as they are.
pub const TIMED: [&str; 6] = [
    "two-worms-fire",
    "multi-shot",
    "suicide-score",
    "fire-minigun",
    "weapons/classic/spikeballs",
    "weapons/classic/uzi",
];
/// The heavy ones, in [`PERF_DIR`].
pub const HEAVY: [&str; 2] = ["heavy-spikeballs", "heavy-napalm"];

/// One frame OLX timed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OlxFrame {
    pub frame: i64,
    pub ns: u64,
    pub projectiles: u32,
    pub rand_calls: u64,
}

/// `frame ns projectiles rc` lines.
pub fn parse_timing(text: &str) -> Result<Vec<OlxFrame>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, l)| {
            let bad = || format!("timing line {}: {l:?}", n + 1);
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() != 4 {
                return Err(bad());
            }
            Ok(OlxFrame {
                frame: f[0].parse().map_err(|_| bad())?,
                ns: f[1].parse().map_err(|_| bad())?,
                projectiles: f[2].parse().map_err(|_| bad())?,
                rand_calls: f[3].parse().map_err(|_| bad())?,
            })
        })
        .collect()
}

/// The oracle's OLX image, built if needed, and the user it runs as.
pub struct Olx {
    image: String,
    user: String,
}

impl Olx {
    pub fn open() -> anyhow::Result<Olx> {
        Ok(Olx { image: oracle::ensure_image(&root())?, user: format!("{}:{}", oracle::id("-u")?, oracle::id("-g")?) })
    }

    pub fn image(&self) -> &str {
        &self.image
    }

    /// Plays `sc` in OLX with timing on: its frames; the header-only trace
    /// gzipped to `out_gz` when given (for `rl-cli scenario --time`).
    pub fn time(&self, name: &str, sc: &Scenario, out_gz: Option<&Path>) -> anyhow::Result<Vec<OlxFrame>> {
        let work = root().join("target/envelope/olx-work").join(name);
        let env = [("ORACLE_TIMING", "/work/timing.txt"), ("ORACLE_HEADER_ONLY", "1")];
        let trace = oracle::record_once_with(&self.image, &self.user, &work, sc, &env).with_context(|| name.to_string())?;
        if let Some(gz) = out_gz {
            if let Some(dir) = gz.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(gz, rl_oracle::gzip(&trace))?;
        }
        let text = std::fs::read_to_string(work.join("timing.txt")).context("OLX wrote no timing file")?;
        let frames = parse_timing(&text).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(frames.len() == sc.frames as usize, "{name}: OLX timed {} frames of {}", frames.len(), sc.frames);
        Ok(frames)
    }

    /// How OLX was built: the image's CMake build type and compiler.
    pub fn build_info(&self) -> String {
        let script = "grep -m1 '^CMAKE_BUILD_TYPE' /olx/CMakeCache.txt; c++ --version | head -1";
        std::process::Command::new("docker")
            .args(["run", "--rm", &self.image, "sh", "-c", script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().replace('\n', "; "))
            .unwrap_or_else(|e| format!("unknown ({e})"))
    }
}
```

- [ ] **Step 8: Run the tests**

Run: `cargo nextest run -p xtask olx_timing && cargo nextest run -p rl-cli --test scenario`
Expected: PASS.

- [ ] **Step 9: Time one scenario on both sides (manual)**

Run `rl-sim`'s side from the command line:

```bash
cargo build --release -p rl-cli
target/release/rl-cli scenario two-worms-fire --time target/envelope/rl-two-worms-fire.ron
```

Expected: `time two-worms-fire: 200 frames, tick p50 … µs, peak … projectiles, matches OLX: Some(true)`. (OLX's side is exercised in Task 7's quick run.)

- [ ] **Step 10: Docs, gate, commit**

- `tools/olx-oracle/CLAUDE.md` **What the patch adds**: a bullet for `ORACLE_TIMING` and `ORACLE_HEADER_ONLY` (bench only, plan 8; the timed span; the line format; traces unchanged, proven by `oracle-record --check`); update the patch's size line.
- `crates/rl-cli/CLAUDE.md`: `scenario --time F` (`timing.rs`).
- `xtask/CLAUDE.md`: `olx_timing.rs` (`TIMED`, `HEAVY` in `fixtures/perf/olx/`, `record_once_with`).

Run: `cargo xtask check`
Expected: `check passed`.

```bash
git add tools/olx-oracle fixtures/perf crates/rl-cli xtask
git commit -F - <<'EOF'
olx-oracle: time OLX's frames to a file of their own; rl-cli: scenario --time

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

### Task 7: `cargo xtask envelope`: sweeps, GPU paths, OLX, results, bless

**Files:**
- Create: `xtask/src/envelope/mod.rs` (the command, `Plan`, `Axis`, `GpuPaths`), `xtask/src/envelope/results.rs` (types and pure arithmetic, with the tests), `xtask/src/envelope/sweeps.rs`, `xtask/src/envelope/gpu.rs`, `xtask/src/envelope/olx.rs`
- Modify: `xtask/src/main.rs` (`mod envelope;`, the `Envelope` command)
- Docs: `xtask/CLAUDE.md`

**Interfaces:**
- Consumes: `rl-cli stress` flags and `Report` fields (Tasks 1, 2), `rl-cli gpu-bench` and its `GpuReport` RON (Task 4), `rl-game --frame-log` and its `FrameLog` RON (Task 4), `crate::winbuild::{missing, build, stage, run, Stage}` (Task 5), `crate::olx_timing::{Olx, OlxFrame, TIMED, HEAVY, PERF_DIR}` and `rl-cli scenario --time` with its `TimingReport` RON (Task 6), `crate::bench::{Host, host_notice, median}`, `crate::bless::{log_blessing, date}`.
- Produces (`crate::envelope`):
  - `Axis { Worms, Pool, Fx, Map, Art, Gpu, Olx }` (`clap::ValueEnum`; `name()`, `is_sweep()`, `ALL`), `GpuPaths { Wsl, Windows, Both, None }`, `Plan::new(quick: bool) -> Plan`, `Opts { quick, only: Vec<Axis>, gpu: GpuPaths, bless: Option<String> }`, `run(&Opts) -> anyhow::Result<bool>`.
  - `results::{Spread, Budgets, Envelope, Sweep, Point, Breaking, Fit, FrameCheck, GpuPath, GpuSize, ProbeRow, Windowed, GpuLevel, OlxComparison, OlxRow, Flag}` and the inputs `StressOut`, `GpuReportOut`, `SizeOut`, `ProbeOut`, `FrameLogOut`, `ScenarioTimingOut`, `TimingReportOut`.
  - `results::{point, failed_point, causes, breaking, fit, gpu_size, olx_row, finish, compare, merged, host_key, path_for, load, write, load_average}`; `results::LATEST = "target/envelope/latest.ron"`, `results::DIR = "fixtures/bench/envelope"`, `results::NOISE_FLOOR = 0.05`.
  - `sweeps::REPLAYS = "target/envelope/replays"`, `sweeps::replay_name(axis: &str, label: &str) -> String`.

- [ ] **Step 1: Write the failing results tests**

`xtask/src/envelope/results.rs` begins with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn s(p50: f64, p99: f64) -> Spread {
        Spread { p50, p99, max: p99 * 1.5 }
    }

    fn run(tick_p99_us: f64, mib: f64) -> StressOut {
        StressOut { tick_us: s(tick_p99_us / 2.0, tick_p99_us), peak_mib: Some(mib), area_cache: (10, 4096), ..StressOut::default() }
    }

    fn pt(value: &str, n: f64, tick_p99_us: f64) -> Point {
        point(value, n, &[run(tick_p99_us, 100.0)], 0.1)
    }

    #[test]
    fn points_take_the_median_of_their_runs() {
        let p = point("16", 16.0, &[run(900.0, 100.0), run(1000.0, 300.0), run(5000.0, 200.0)], 0.2);
        assert_eq!((p.tick_us.p99, p.peak_mib, p.runs), (1000.0, 200.0, 3));
        assert!((p.spread - 4.1).abs() < 1e-9, "(5000 - 900) / 1000: {}", p.spread);
        assert!((p.budget_share - 0.1).abs() < 1e-9, "1 ms of 10");
        let pool = StressOut { ticks_at_pool_cap: 5, pool_capacity: 3000, ..run(100.0, 1.0) };
        assert_eq!(point("3000", 3000.0, &[pool], 0.0).saturated.len(), 1, "a full pool is saturated, not a cap");
    }

    #[test]
    fn breaking_point_is_the_first_value_that_does_not_hold() {
        let b = Budgets::default();
        let mut points = vec![pt("2", 2.0, 500.0), pt("4", 4.0, 12_000.0), pt("8", 8.0, 30_000.0)];
        let br = breaking(&points, &b, b.frame_144_ms).unwrap();
        assert_eq!(br.value, "4");
        assert!(br.causes[0].starts_with("tick p99 12.00 ms"), "{:?}", br.causes);
        points[1].tick_us.p99 = 900.0;
        points[0].peak_mib = 4096.0;
        assert!(causes(&points[0], &b, b.frame_144_ms)[0].starts_with("memory 4096 MiB"));
        points[0].peak_mib = 1.0;
        points[1].frame = Some(FrameCheck { total_ms: s(5.0, 9.0), ..FrameCheck::default() });
        assert_eq!(breaking(&points, &b, b.frame_144_ms).unwrap().value, "4", "9 ms misses 144 Hz");
        assert_eq!(breaking(&points, &b, b.frame_60_ms).unwrap().value, "8", "but holds 60 Hz");
        points[0].caps.push("the bots' area cache is full (4096 areas)".into());
        assert_eq!(breaking(&points, &b, b.frame_60_ms).unwrap().value, "2");
    }

    /// Review Focus 1.
    #[test]
    fn a_failed_point_is_a_breaking_cause() {
        let b = Budgets::default();
        let points = vec![pt("1", 1.0, 100.0), failed_point("8", 8.0, "rl-cli stress: 16400 MiB resident, over 16384", 0.0)];
        let br = breaking(&points, &b, b.frame_144_ms).unwrap();
        assert_eq!(br.value, "8");
        assert!(br.causes[0].contains("failed: rl-cli stress: 16400 MiB"), "{:?}", br.causes);
        assert!(fit(&points, &b).is_none(), "one measured point fits nothing");
    }

    #[test]
    fn worm_cost_is_a_least_squares_slope() {
        let b = Budgets::default();
        let points: Vec<Point> = [2.0, 4.0, 8.0].iter().map(|&n| pt(&n.to_string(), n, 100.0 + 50.0 * n)).collect();
        let f = fit(&points, &b).unwrap();
        assert!((f.per_unit_us_p99 - 50.0).abs() < 1e-6 && (f.intercept_us_p99 - 100.0).abs() < 1e-6, "{f:?}");
        assert!((f.limit - 198.0).abs() < 1e-6, "(10000 - 100) / 50: {}", f.limit);
    }

    #[test]
    fn probe_passes_fit_the_budget() {
        let b = Budgets::default();
        let probe = |k: u32, ms: f64| ProbeOut { passes: k, ms: s(ms, ms), ..ProbeOut::default() };
        let size = SizeOut {
            size: (3840, 2160),
            frame_ms: s(2.0, 2.5),
            total_ms: s(3.0, 3.5),
            gpu_ms: Some(s(0.8, 0.94)),
            probe: vec![probe(0, 0.0), probe(2, 0.5), probe(4, 1.0)],
            ..SizeOut::default()
        };
        let g = gpu_size(&[size], &b);
        assert!((g.ms_per_pass - 0.25).abs() < 1e-9, "{}", g.ms_per_pass);
        assert_eq!((g.passes_144, g.passes_60), (24, 62), "(6.944 - 0.94) / 0.25, (16.667 - 0.94) / 0.25");
    }

    #[test]
    fn olx_rows_compare_the_same_work() {
        let olx = vec![(0..4).map(|f| OlxFrame { frame: f, ns: 2000, projectiles: f as u32, rand_calls: 10 * f as u64 }).collect()];
        let rl = ScenarioTimingOut {
            name: "x".into(),
            tick_ns: vec![1000; 4],
            projectiles: vec![0, 1, 2, 3],
            rand_calls: vec![0, 10, 20, 30],
            matches_olx: Some(true),
        };
        let row = olx_row("x", &olx, std::slice::from_ref(&rl));
        assert!(row.same_work && (row.ratio_p50 - 0.5).abs() < 1e-9 && row.projectiles_peak == 3, "{row:?}");
        let other = ScenarioTimingOut { projectiles: vec![0, 1, 2, 4], ..rl };
        assert!(!olx_row("x", &olx, &[other]).same_work);
    }

    /// Review Focus 4.
    #[test]
    fn regressions_beyond_noise_are_flagged_not_failed() {
        let sweep = |p99: f64| Sweep { axis: "worms".into(), noise: 0.10, points: vec![pt("16", 16.0, p99)], ..Sweep::default() };
        let mut stored = Envelope::default();
        stored.sweeps.insert("worms".into(), sweep(1000.0));
        let flagged = |p99: f64| {
            let mut now = Envelope::default();
            now.sweeps.insert("worms".into(), sweep(p99));
            compare(&stored, &now).into_iter().filter(Flag::flagged).count()
        };
        assert_eq!(flagged(1090.0), 0, "within the axis's 10 % noise");
        assert_eq!(flagged(1200.0), 1, "beyond it");
        assert_eq!(flagged(500.0), 0, "an improvement");
    }

    /// Review Focus 3.
    #[test]
    fn another_hosts_results_only_notice() {
        let here = Host { cpus: 12, cpu: "X".into(), adapter: "llvmpipe".into() };
        let stored = Envelope { host: Host { cpus: 8, ..here.clone() }, ..Envelope::default() };
        assert!(crate::bench::host_notice(&stored.host, &here).unwrap().contains("another machine"));
        assert_eq!(path_for(&here), "fixtures/bench/envelope/x-12cpus.ron");
    }

    /// Review Focus 3.
    #[test]
    fn old_results_parse_with_defaults() {
        let old = "(host: (cpus: 12, cpu: \"X\"), sweeps: {\"worms\": (axis: \"worms\", points: [(value: \"2\")])})";
        let e: Envelope = ron::from_str(old).unwrap();
        assert_eq!(e.sweeps["worms"].points[0].value, "2");
        assert!(e.gpu.is_empty() && e.olx.is_none());
        assert_eq!(e.budgets, Budgets::default());
    }

    #[test]
    fn a_bless_keeps_what_it_did_not_measure() {
        let mut old = Envelope::default();
        old.sweeps.insert("art".into(), Sweep::default());
        old.gpu.insert("windows-dx12".into(), GpuPath::default());
        let mut new = Envelope::default();
        new.sweeps.insert("worms".into(), Sweep::default());
        let m = merged(Some(old), new);
        assert_eq!(m.sweeps.keys().collect::<Vec<_>>(), ["art", "worms"]);
        assert_eq!(m.kept, ["sweep art", "gpu windows-dx12"]);
    }
}
```

Create `xtask/src/envelope/mod.rs` with `pub mod results;` only for now, and add `mod envelope;` to `xtask/src/main.rs`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p xtask envelope`
Expected: FAIL to compile ("cannot find struct `StressOut`", "cannot find function `point`").

- [ ] **Step 3: Write `results.rs`**

Above the tests:

```rust
//! The performance envelope's results (plan 8, spec §3 and §7): the RON
//! `rl-cli stress`, `gpu-bench`, `scenario --time` and `rl-game
//! --frame-log` write (read here as `*Out`), what `cargo xtask envelope`
//! keeps of them (`Envelope`, the file `fixtures/bench/envelope/<host>.ron`)
//! and the pure arithmetic: medians of runs, budgets, breaking points,
//! noise, the comparison with the committed file, merging a bless.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bench::{Host, median};
use crate::olx_timing::OlxFrame;

/// Where the blessed results go.
pub const DIR: &str = "fixtures/bench/envelope";
/// Every run's results (not committed).
pub const LATEST: &str = "target/envelope/latest.ron";
/// The least noise an axis is given (run-to-run spread below it is luck).
pub const NOISE_FLOOR: f64 = 0.05;

/// Percentiles of a measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spread {
    pub p50: f64,
    pub p99: f64,
    pub max: f64,
}

impl Spread {
    /// Of `values` (any order), nearest rank, as `rl-cli`'s.
    pub fn of(values: &[f64]) -> Spread {
        if values.is_empty() {
            return Spread::default();
        }
        let mut v = values.to_vec();
        v.sort_by(f64::total_cmp);
        let at = |q: f64| v[((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1];
        Spread { p50: at(0.5), p99: at(0.99), max: at(1.0) }
    }

    /// Each percentile's median over `runs`.
    pub fn median_of(runs: &[Spread]) -> Spread {
        let m = |f: fn(&Spread) -> f64| median(&runs.iter().map(f).collect::<Vec<_>>());
        Spread { p50: m(|s| s.p50), p99: m(|s| s.p99), max: m(|s| s.max) }
    }
}

/// Spec §3's budgets.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Budgets {
    /// OLX's fixed 100 Hz frame (`FRAME_MS`).
    pub tick_ms: f64,
    pub frame_144_ms: f64,
    pub frame_60_ms: f64,
    pub memory_mib: f64,
}

impl Default for Budgets {
    fn default() -> Budgets {
        Budgets { tick_ms: 10.0, frame_144_ms: 1000.0 / 144.0, frame_60_ms: 1000.0 / 60.0, memory_mib: 2048.0 }
    }
}

// ---- What the tools write (the parts read here; every field defaults).

/// `rl-cli stress --out` (`rl_cli::stress::Report`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct StressOut {
    pub name: String,
    pub ticks: u32,
    pub tick_us: Spread,
    pub sim_us: Spread,
    pub think_us: Spread,
    pub present_us: Spread,
    pub build_us: Spread,
    pub pool_capacity: usize,
    pub projectiles_peak: usize,
    pub ticks_at_pool_cap: u32,
    pub fx_peak: usize,
    pub ticks_at_fx_cap: u32,
    pub fx_capacity: usize,
    pub events_max: usize,
    pub peak_mib: Option<f64>,
    pub area_cache: (usize, usize),
    pub level: (u32, u32, u32),
    pub caps: Vec<String>,
}

/// `rl-cli gpu-bench --out` (`rl_cli::gpu_bench::GpuReport`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct GpuReportOut {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    pub timestamps: bool,
    pub sizes: Vec<SizeOut>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct SizeOut {
    pub size: (u32, u32),
    pub frames: u32,
    pub tick_ms: Spread,
    pub cpu_ms: Spread,
    pub frame_ms: Spread,
    pub total_ms: Spread,
    pub gpu_ms: Option<Spread>,
    pub probe: Vec<ProbeOut>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ProbeOut {
    pub passes: u32,
    pub taps: u32,
    pub ms: Spread,
    pub timed_on_gpu: bool,
}

/// `rl-game --frame-log` (`rl_game::stats::FrameLog`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct FrameLogOut {
    pub size: (u32, u32),
    pub gpu: String,
    pub frames: usize,
    pub fps: f64,
    pub frame_ms: Spread,
    pub busy_ms: Spread,
}

/// `rl-cli scenario --time` (`rl_cli::timing`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ScenarioTimingOut {
    pub name: String,
    pub tick_ns: Vec<u64>,
    pub projectiles: Vec<u32>,
    pub rand_calls: Vec<u64>,
    pub matches_olx: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TimingReportOut {
    pub scenarios: Vec<ScenarioTimingOut>,
}

// ---- What is kept.

/// A frame measured for a sweep point (its level drawn, plan 8 ruling P4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameCheck {
    pub path: String,
    pub size: (u32, u32),
    pub total_ms: Spread,
    pub gpu_ms: Option<Spread>,
}

/// One value of an axis: the medians of its runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Point {
    pub value: String,
    /// The value as a number (for the fit).
    pub n: f64,
    pub runs: usize,
    pub tick_us: Spread,
    pub sim_us: Spread,
    pub think_us: Spread,
    pub present_us: Spread,
    pub build_us: Spread,
    pub projectiles_peak: usize,
    pub fx_peak: usize,
    pub events_max: usize,
    pub peak_mib: f64,
    pub area_cache: (usize, usize),
    /// Hard limits reached (each breaks the point, ruling P7).
    pub caps: Vec<String>,
    /// The pool or effects cap full at its own size (shown, not a break).
    pub saturated: Vec<String>,
    pub load_avg: f64,
    /// The runs' tick p99 spread: (max − min) / median.
    pub spread: f64,
    /// Tick p99 over the tick budget.
    pub budget_share: f64,
    pub frame: Option<FrameCheck>,
}

/// The first value that does not hold, and why.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Breaking {
    pub value: String,
    pub causes: Vec<String>,
}

/// Tick time against the axis's value, least squares over measured points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fit {
    pub per_unit_us_p50: f64,
    pub per_unit_us_p99: f64,
    pub intercept_us_p99: f64,
    /// The value where the tick p99 would reach the budget.
    pub limit: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sweep {
    pub axis: String,
    pub mod_id: String,
    pub level: String,
    /// What varies and what stays the reference.
    pub what: String,
    /// The largest point spread (at least [`NOISE_FLOOR`]): the comparison's tolerance.
    pub noise: f64,
    pub points: Vec<Point>,
    /// With the 144 Hz frame budget (ruling P4).
    pub breaking_144: Option<Breaking>,
    pub breaking_60: Option<Breaking>,
    pub fit: Option<Fit>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeRow {
    pub passes: u32,
    pub ms_p50: f64,
}

/// One resolution of a GPU path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuSize {
    pub size: (u32, u32),
    pub tick_ms: Spread,
    pub cpu_ms: Spread,
    pub frame_ms: Spread,
    pub total_ms: Spread,
    pub gpu_ms: Option<Spread>,
    pub spread: f64,
    pub probe: Vec<ProbeRow>,
    /// GPU ms one full-screen probe pass costs (slope through the origin).
    pub ms_per_pass: f64,
    /// Passes that fit beside the frame (its GPU p99, else its frame p99).
    pub passes_144: u32,
    pub passes_60: u32,
}

/// A windowed, uncapped `rl-game` run (one run, ruling P5).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Windowed {
    pub requested: (u32, u32),
    pub size: (u32, u32),
    /// The window got the requested size (else the monitor is smaller).
    pub reached: bool,
    pub fps: f64,
    pub frame_ms: Spread,
    pub busy_ms: Spread,
}

/// A sweep point's level drawn.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuLevel {
    pub axis: String,
    pub value: String,
    pub size: (u32, u32),
    pub total_ms: Spread,
    pub gpu_ms: Option<Spread>,
}

/// One GPU path: `wsl-gl`, `windows-dx12`, `windows-vulkan`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuPath {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    /// GPU times are timestamps; else CPU-side frame times only.
    pub timestamps: bool,
    pub noise: f64,
    pub sizes: Vec<GpuSize>,
    pub windowed: Vec<Windowed>,
    pub levels: Vec<GpuLevel>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OlxRow {
    pub name: String,
    pub frames: usize,
    pub projectiles_peak: u32,
    pub olx_us: Spread,
    pub rl_us: Spread,
    /// rl-sim's p50 over OLX's.
    pub ratio_p50: f64,
    /// The same projectiles and `rand()` calls every frame.
    pub same_work: bool,
    pub matches_olx: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OlxComparison {
    pub image: String,
    pub build: String,
    pub rows: Vec<OlxRow>,
}

/// The results file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Envelope {
    pub note: String,
    pub host: Host,
    pub date: String,
    pub quick: bool,
    pub runs: usize,
    pub seconds: f32,
    pub budgets: Budgets,
    pub sweeps: BTreeMap<String, Sweep>,
    pub gpu: BTreeMap<String, GpuPath>,
    pub olx: Option<OlxComparison>,
    /// Parts a bless did not measure, kept from the file before.
    pub kept: Vec<String>,
}

fn relative_spread(v: &[f64]) -> f64 {
    let m = median(v);
    let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    if m > 0.0 && v.len() > 1 { (hi - lo) / m } else { 0.0 }
}

/// A point from its runs: each figure's median.
pub fn point(value: &str, n: f64, runs: &[StressOut], load_avg: f64) -> Point {
    let sp = |f: fn(&StressOut) -> Spread| Spread::median_of(&runs.iter().map(f).collect::<Vec<_>>());
    let m = |f: fn(&StressOut) -> f64| median(&runs.iter().map(f).collect::<Vec<_>>());
    let first = runs.first().cloned().unwrap_or_default();
    let tick_us = sp(|r| r.tick_us);
    let mut saturated = Vec::new();
    if first.ticks_at_pool_cap > 0 {
        saturated.push(format!("pool full ({}) for {} ticks", first.pool_capacity, first.ticks_at_pool_cap));
    }
    if first.ticks_at_fx_cap > 0 {
        saturated.push(format!("effects full ({}) for {} ticks", first.fx_capacity, first.ticks_at_fx_cap));
    }
    Point {
        value: value.into(),
        n,
        runs: runs.len(),
        tick_us,
        sim_us: sp(|r| r.sim_us),
        think_us: sp(|r| r.think_us),
        present_us: sp(|r| r.present_us),
        build_us: sp(|r| r.build_us),
        projectiles_peak: first.projectiles_peak,
        fx_peak: first.fx_peak,
        events_max: first.events_max,
        peak_mib: m(|r| r.peak_mib.unwrap_or(0.0)),
        area_cache: first.area_cache,
        caps: first.caps.clone(),
        saturated,
        load_avg,
        spread: relative_spread(&runs.iter().map(|r| r.tick_us.p99).collect::<Vec<_>>()),
        budget_share: tick_us.p99 / 1000.0 / Budgets::default().tick_ms,
        frame: None,
    }
}

/// A point whose run failed (a crash, the memory ceiling): the failure is its cap.
pub fn failed_point(value: &str, n: f64, why: &str, load_avg: f64) -> Point {
    let first_line = why.lines().rfind(|l| !l.trim().is_empty()).unwrap_or(why);
    Point { value: value.into(), n, caps: vec![format!("failed: {}", first_line.trim())], load_avg, ..Point::default() }
}

/// Why `p` does not hold (empty: it holds), with the frame budget `frame_budget_ms`.
pub fn causes(p: &Point, b: &Budgets, frame_budget_ms: f64) -> Vec<String> {
    let mut c = Vec::new();
    if p.runs > 0 && p.tick_us.p99 / 1000.0 > b.tick_ms {
        c.push(format!("tick p99 {:.2} ms > {} ms", p.tick_us.p99 / 1000.0, b.tick_ms));
    }
    if p.peak_mib > b.memory_mib {
        c.push(format!("memory {:.0} MiB > {:.0} MiB", p.peak_mib, b.memory_mib));
    }
    if let Some(f) = &p.frame
        && f.total_ms.p99 > frame_budget_ms
    {
        c.push(format!("frame p99 {:.2} ms > {:.1} ms", f.total_ms.p99, frame_budget_ms));
    }
    c.extend(p.caps.iter().map(|x| format!("cap: {x}")));
    c
}

/// The first point that does not hold.
pub fn breaking(points: &[Point], b: &Budgets, frame_budget_ms: f64) -> Option<Breaking> {
    points.iter().find_map(|p| {
        let c = causes(p, b, frame_budget_ms);
        (!c.is_empty()).then(|| Breaking { value: p.value.clone(), causes: c })
    })
}

/// Least squares of tick time against the value, over measured points (two at least).
pub fn fit(points: &[Point], b: &Budgets) -> Option<Fit> {
    let pts: Vec<&Point> = points.iter().filter(|p| p.runs > 0).collect();
    if pts.len() < 2 {
        return None;
    }
    let line = |y: fn(&Point) -> f64| {
        let n = pts.len() as f64;
        let (sx, sy) = (pts.iter().map(|p| p.n).sum::<f64>(), pts.iter().map(|&p| y(p)).sum::<f64>());
        let sxx = pts.iter().map(|p| p.n * p.n).sum::<f64>();
        let sxy = pts.iter().map(|&p| p.n * y(p)).sum::<f64>();
        let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
        (slope, (sy - slope * sx) / n)
    };
    let (p50, _) = line(|p| p.tick_us.p50);
    let (p99, c99) = line(|p| p.tick_us.p99);
    let limit = if p99 > 0.0 { (b.tick_ms * 1000.0 - c99) / p99 } else { f64::INFINITY };
    Some(Fit { per_unit_us_p50: p50, per_unit_us_p99: p99, intercept_us_p99: c99, limit })
}

/// One resolution from its runs, with the probe's cost and what fits.
pub fn gpu_size(runs: &[SizeOut], b: &Budgets) -> GpuSize {
    let sp = |f: fn(&SizeOut) -> Spread| Spread::median_of(&runs.iter().map(f).collect::<Vec<_>>());
    let gpu: Vec<Spread> = runs.iter().filter_map(|r| r.gpu_ms).collect();
    let gpu_ms = (!gpu.is_empty()).then(|| Spread::median_of(&gpu));
    let first = runs.first().cloned().unwrap_or_default();
    let probe: Vec<ProbeRow> = first
        .probe
        .iter()
        .enumerate()
        .map(|(i, p)| ProbeRow {
            passes: p.passes,
            ms_p50: median(&runs.iter().filter_map(|r| r.probe.get(i)).map(|q| q.ms.p50).collect::<Vec<_>>()),
        })
        .collect();
    let (kk, km) = probe.iter().fold((0.0, 0.0), |(kk, km), r| {
        let k = f64::from(r.passes);
        (kk + k * k, km + k * r.ms_p50)
    });
    let ms_per_pass = if kk > 0.0 { km / kk } else { 0.0 };
    let frame_ms = sp(|r| r.frame_ms);
    let base = gpu_ms.map_or(frame_ms.p99, |g| g.p99);
    let fits = |budget: f64| {
        if ms_per_pass <= 1e-6 { 9999 } else { ((budget - base).max(0.0) / ms_per_pass).floor().min(9999.0) as u32 }
    };
    GpuSize {
        size: first.size,
        tick_ms: sp(|r| r.tick_ms),
        cpu_ms: sp(|r| r.cpu_ms),
        frame_ms,
        total_ms: sp(|r| r.total_ms),
        gpu_ms,
        spread: relative_spread(&runs.iter().map(|r| r.total_ms.p99).collect::<Vec<_>>()),
        probe,
        ms_per_pass,
        passes_144: fits(b.frame_144_ms),
        passes_60: fits(b.frame_60_ms),
    }
}

/// OLX's runs and rl-sim's of one scenario, in µs.
pub fn olx_row(name: &str, olx: &[Vec<OlxFrame>], rl: &[ScenarioTimingOut]) -> OlxRow {
    let us = |ns: &[u64]| Spread::of(&ns.iter().map(|&n| n as f64 / 1000.0).collect::<Vec<_>>());
    let olx_us = Spread::median_of(&olx.iter().map(|r| us(&r.iter().map(|f| f.ns).collect::<Vec<_>>())).collect::<Vec<_>>());
    let rl_us = Spread::median_of(&rl.iter().map(|r| us(&r.tick_ns)).collect::<Vec<_>>());
    let (o, r) = (olx.first().cloned().unwrap_or_default(), rl.first().cloned().unwrap_or_default());
    let same_work = o.len() == r.projectiles.len()
        && o.iter().zip(&r.projectiles).all(|(f, &p)| f.projectiles == p)
        && o.iter().zip(&r.rand_calls).all(|(f, &c)| f.rand_calls == c);
    OlxRow {
        name: name.into(),
        frames: o.len(),
        projectiles_peak: o.iter().map(|f| f.projectiles).max().unwrap_or(0),
        olx_us,
        rl_us,
        ratio_p50: if olx_us.p50 > 0.0 { rl_us.p50 / olx_us.p50 } else { 0.0 },
        same_work,
        matches_olx: r.matches_olx,
    }
}

/// Noise, frames from the GPU paths' levels (the WSL path first), fits and
/// breaking points.
pub fn finish(env: &mut Envelope) {
    let b = env.budgets;
    let levels: Vec<(String, GpuLevel)> = ["wsl-gl", "windows-dx12", "windows-vulkan"]
        .iter()
        .filter_map(|k| env.gpu.get(*k).map(|p| (k.to_string(), p.levels.clone())))
        .flat_map(|(k, ls)| ls.into_iter().map(move |l| (k.clone(), l)))
        .collect();
    for sweep in env.sweeps.values_mut() {
        for p in &mut sweep.points {
            if let Some((path, l)) = levels.iter().find(|(_, l)| l.axis == sweep.axis && l.value == p.value) {
                p.frame = Some(FrameCheck { path: path.clone(), size: l.size, total_ms: l.total_ms, gpu_ms: l.gpu_ms });
            }
        }
        sweep.noise = sweep.points.iter().map(|p| p.spread).fold(NOISE_FLOOR, f64::max);
        sweep.breaking_144 = breaking(&sweep.points, &b, b.frame_144_ms);
        sweep.breaking_60 = breaking(&sweep.points, &b, b.frame_60_ms);
        sweep.fit = if sweep.axis == "map" { None } else { fit(&sweep.points, &b) };
    }
    for path in env.gpu.values_mut() {
        path.noise = path.sizes.iter().map(|s| s.spread).fold(NOISE_FLOOR, f64::max);
    }
}

/// A measurement against the committed one.
#[derive(Clone, Debug, PartialEq)]
pub struct Flag {
    pub name: String,
    pub baseline: f64,
    pub measured: f64,
    pub noise: f64,
}

impl Flag {
    /// Slower by more than the noise (an improvement never is).
    pub fn flagged(&self) -> bool {
        self.baseline > 0.0 && self.measured > self.baseline * (1.0 + self.noise)
    }

    pub fn line(&self) -> String {
        let change = if self.baseline > 0.0 { (self.measured / self.baseline - 1.0) * 100.0 } else { 0.0 };
        format!(
            "{} {:<44} {:>10.3} (committed {:>10.3}) {:+6.1} % (noise {:.0} %)",
            if self.flagged() { "!" } else { " " },
            self.name,
            self.measured,
            self.baseline,
            change,
            self.noise * 100.0
        )
    }
}

/// Tick p99 (ms) of every sweep point and frame p99 and probe ms per pass
/// of every GPU size measured in both, with the committed noise.
pub fn compare(stored: &Envelope, now: &Envelope) -> Vec<Flag> {
    let mut out = Vec::new();
    for (axis, s) in &now.sweeps {
        let Some(old) = stored.sweeps.get(axis) else { continue };
        for p in &s.points {
            if let Some(o) = old.points.iter().find(|o| o.value == p.value && o.runs > 0 && p.runs > 0) {
                let name = format!("{axis} {} tick p99 ms", p.value);
                out.push(Flag { name, baseline: o.tick_us.p99 / 1000.0, measured: p.tick_us.p99 / 1000.0, noise: old.noise });
            }
        }
    }
    for (key, g) in &now.gpu {
        let Some(old) = stored.gpu.get(key) else { continue };
        for s in &g.sizes {
            if let Some(o) = old.sizes.iter().find(|o| o.size == s.size) {
                let at = format!("{key} {}x{}", s.size.0, s.size.1);
                out.push(Flag { name: format!("{at} frame p99 ms"), baseline: o.total_ms.p99, measured: s.total_ms.p99, noise: old.noise });
                out.push(Flag { name: format!("{at} ms per pass"), baseline: o.ms_per_pass, measured: s.ms_per_pass, noise: old.noise });
            }
        }
    }
    out
}

/// `new`, plus the sweeps, GPU paths and OLX table of `old` it did not measure (listed in `kept`).
pub fn merged(old: Option<Envelope>, new: Envelope) -> Envelope {
    let mut m = new;
    m.kept.clear();
    let Some(old) = old else { return m };
    for (k, v) in old.sweeps {
        if !m.sweeps.contains_key(&k) {
            m.kept.push(format!("sweep {k}"));
            m.sweeps.insert(k, v);
        }
    }
    for (k, v) in old.gpu {
        if !m.gpu.contains_key(&k) {
            m.kept.push(format!("gpu {k}"));
            m.gpu.insert(k, v);
        }
    }
    if m.olx.is_none() && old.olx.is_some() {
        m.kept.push("olx".into());
        m.olx = old.olx;
    }
    m
}

/// The file name's host part: the CPU model's slug and the CPU count.
pub fn host_key(host: &Host) -> String {
    format!("{}-{}cpus", rl_data::slug::slug(&host.cpu), host.cpus)
}

/// `fixtures/bench/envelope/<host>.ron`.
pub fn path_for(host: &Host) -> String {
    format!("{DIR}/{}.ron", host_key(host))
}

/// The results at `path`, if there are any.
pub fn load(path: &Path) -> Result<Option<Envelope>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => ron::from_str(&text).map(Some).map_err(|e| format!("{}: {e}", path.display())),
        Err(_) => Ok(None),
    }
}

pub fn write(path: &Path, env: &Envelope) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = ron::ser::to_string_pretty(env, ron::ser::PrettyConfig::default())?;
    std::fs::write(path, text + "\n")?;
    Ok(())
}

/// The one-minute load average (`/proc/loadavg`).
pub fn load_average() -> Option<f64> {
    std::fs::read_to_string("/proc/loadavg").ok()?.split_whitespace().next()?.parse().ok()
}

impl Point {
    /// One line for the console.
    pub fn line(&self) -> String {
        if self.runs == 0 {
            return self.caps.join("; ");
        }
        format!(
            "tick p50/p99 {:.2}/{:.2} ms ({:.0} % of the budget), present p99 {:.2} ms, build p99 {:.2} ms, \
             {} projectiles, {} effects, {:.0} MiB{}",
            self.tick_us.p50 / 1000.0,
            self.tick_us.p99 / 1000.0,
            self.budget_share * 100.0,
            self.present_us.p99 / 1000.0,
            self.build_us.p99 / 1000.0,
            self.projectiles_peak,
            self.fx_peak,
            self.peak_mib,
            if self.caps.is_empty() { String::new() } else { format!("; caps: {}", self.caps.join("; ")) }
        )
    }
}
```

(`rl-data` is already an `xtask` dependency; its `slug` is `no_std` + `alloc` and usable from std code.)

- [ ] **Step 4: Run the results tests**

Run: `cargo nextest run -p xtask envelope`
Expected: PASS (10 tests). The numbers in the assertions are worked out in their messages; if one fails, recheck the arithmetic in the function before touching the test.

- [ ] **Step 5: Write the sweeps**

`xtask/src/envelope/sweeps.rs`:

```rust
//! The headless sweeps (spec §4): each point is `rl-cli stress` in its own
//! process (its peak memory is its own), `runs` times, from the reference
//! configuration (ruling P1) with one axis changed. The map and art points'
//! first runs record their replays for the GPU scenes (ruling P4).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::results::{self, Point, StressOut, Sweep};
use super::{Axis, Plan};
use crate::root;

pub const REPLAYS: &str = "target/envelope/replays";
const REF_MOD: &str = "olx-classic";
const REF_LEVEL: &str = "olx-level-castlestrike";
const POOL_MOD: &str = "olx-mw-1-0";
const REF_WORMS: u32 = 16;
const REF_POOL: u32 = 3000;
const REF_FX: u32 = 1024;
/// Ruling P13: past the 2 GiB budget, below this machine's memory.
const MEMORY_CEILING_MIB: u32 = 16_384;

/// `<axis>-<label>.rlrp`.
pub fn replay_name(axis: &str, label: &str) -> String {
    format!("{axis}-{label}.rlrp")
}

/// Builds `packages` in the release profile.
pub fn build_release(packages: &[&str]) -> Result<(), String> {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root()).args(["build", "--quiet", "--release"]);
    for p in packages {
        c.args(["-p", p]);
    }
    let out = c.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) }
}

/// One point's configuration.
#[derive(Clone, Debug)]
pub struct Value {
    pub label: String,
    pub n: f64,
    pub mod_id: &'static str,
    pub level: String,
    pub worms: u32,
    pub pool: u32,
    pub fx: u32,
}

fn reference(label: String, n: f64) -> Value {
    Value { label, n, mod_id: REF_MOD, level: REF_LEVEL.into(), worms: REF_WORMS, pool: REF_POOL, fx: REF_FX }
}

/// The points of `axis`.
pub fn values(axis: Axis, plan: &Plan) -> Vec<Value> {
    let f = |x: u32| f64::from(x);
    match axis {
        Axis::Worms => plan.worms.iter().map(|&w| Value { worms: w, ..reference(w.to_string(), f(w)) }).collect(),
        Axis::Pool => {
            plan.pools.iter().map(|&p| Value { pool: p, mod_id: POOL_MOD, ..reference(p.to_string(), f(p)) }).collect()
        }
        Axis::Fx => plan.fx.iter().map(|&x| Value { fx: x, ..reference(x.to_string(), f(x)) }).collect(),
        Axis::Map => plan
            .maps
            .iter()
            .map(|&(w, h)| Value { level: format!("gen-{w}x{h}-a1-s1"), ..reference(format!("{w}x{h}"), f(w) * f(h)) })
            .collect(),
        Axis::Art => plan
            .arts
            .iter()
            .map(|&a| Value { level: format!("gen-2048x2048-a{a}-s1"), ..reference(format!("a{a}"), f(a)) })
            .collect(),
        Axis::Gpu | Axis::Olx => Vec::new(),
    }
}

fn stress(cli: &Path, content: &Path, v: &Value, seconds: f32, record: Option<&Path>) -> Result<StressOut, String> {
    let out = root().join("target/envelope/stress.ron");
    let _ = std::fs::remove_file(&out);
    let mut c = Command::new(cli);
    c.current_dir(root())
        .args(["stress", "--mod", v.mod_id, "--level", &v.level])
        .args(["--worms", &v.worms.to_string(), "--pool", &v.pool.to_string(), "--fx-cap", &v.fx.to_string()])
        .args(["--seconds", &seconds.to_string(), "--seed", "1", "--render-every", "0", "--build-every", "1"])
        .args(["--no-verify", "--memory-ceiling-mib", &MEMORY_CEILING_MIB.to_string()])
        .arg("--content")
        .arg(content)
        .arg("--out")
        .arg(&out);
    if let Some(r) = record {
        c.arg("--record").arg(r);
    }
    let o = c.output().map_err(|e| format!("cannot run rl-cli: {e}"))?;
    if !o.status.success() {
        return Err(format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)));
    }
    let text = std::fs::read_to_string(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", out.display()))
}

/// Every point of `axis`; a failed run makes a failed point (Review Focus
/// 1) and the sweep goes on.
pub fn run_axis(cli: &Path, content: &Path, axis: Axis, plan: &Plan) -> Sweep {
    let vals = values(axis, plan);
    let mut points: Vec<Point> = Vec::new();
    for v in &vals {
        let load = results::load_average().unwrap_or(0.0);
        if load > 1.5 {
            println!("  notice: load average {load:.2} before {} {}: is the PC idle?", axis.name(), v.label);
        }
        let mut runs = Vec::new();
        let mut failure = None;
        for run in 0..plan.runs {
            let record: Option<PathBuf> = (run == 0 && matches!(axis, Axis::Map | Axis::Art))
                .then(|| root().join(REPLAYS).join(replay_name(axis.name(), &v.label)));
            if let Some(dir) = record.as_ref().and_then(|r| r.parent()) {
                let _ = std::fs::create_dir_all(dir);
            }
            match stress(cli, content, v, plan.seconds, record.as_deref()) {
                Ok(r) => runs.push(r),
                Err(why) => {
                    failure = Some(why);
                    break;
                }
            }
        }
        let p = match failure {
            Some(why) => results::failed_point(&v.label, v.n, &why, load),
            None => results::point(&v.label, v.n, &runs, load),
        };
        println!("  {} {}: {}", axis.name(), v.label, p.line());
        points.push(p);
    }
    let first = vals.first();
    Sweep {
        axis: axis.name().into(),
        mod_id: first.map_or(REF_MOD, |v| v.mod_id).into(),
        level: first.map_or(REF_LEVEL.into(), |v| v.level.clone()),
        what: format!(
            "{} varies; otherwise {REF_WORMS} chaos bots (mixed difficulties), pool {REF_POOL}, effects {REF_FX}, \
             {} s, seed 1, LoadingTime 0, bonuses every 5 s",
            axis.name(),
            plan.seconds
        ),
        points,
        ..Sweep::default()
    }
}
```

- [ ] **Step 6: Write the GPU paths**

`xtask/src/envelope/gpu.rs`:

```rust
//! The GPU paths (spec §5): the heavy replay and the map and art points'
//! replays drawn by `rl-cli gpu-bench` (offscreen, hardware adapter,
//! timestamps when there are any) and played windowed and uncapped by
//! `rl-game --frame-log`, on WSL (GL over Mesa's d3d12 driver) and on
//! Windows (DX12 and Vulkan, through interop). A part that cannot run here
//! is a notice (Review Focus 2); nothing here fails the command.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::results::{self, Budgets, Envelope, FrameLogOut, GpuLevel, GpuPath, GpuReportOut, SizeOut, Windowed};
use super::sweeps::{self, REPLAYS};
use super::{GpuPaths, Plan};
use crate::{root, winbuild};

const HEAVY: &str = "heavy.rlrp";

/// The environment that puts WSL's GL on the real GPU (as `rl-game` sets
/// for itself), naming NVIDIA's adapter when its WSL driver is there.
pub fn wsl_env(nvidia: bool) -> Vec<(String, String)> {
    let mut v = vec![("GALLIUM_DRIVER".to_string(), "d3d12".to_string()), ("WGPU_BACKEND".into(), "gl".into())];
    if nvidia {
        v.push(("MESA_D3D12_DEFAULT_ADAPTER_NAME".into(), "NVIDIA".into()));
    }
    v
}

/// Whether a window can open here (WSLg sets both).
pub fn has_display(display: Option<&str>, wayland: Option<&str>) -> bool {
    display.is_some_and(|d| !d.is_empty()) || wayland.is_some_and(|d| !d.is_empty())
}

fn nvidia() -> bool {
    ["libcuda.so.1", "nvidia-smi"].iter().any(|f| Path::new("/usr/lib/wsl/lib").join(f).exists())
}

/// Where a path's tools run.
enum Runner<'a> {
    Wsl { cli: &'a Path, game: PathBuf, content: &'a Path },
    Windows { stage: &'a winbuild::Stage, backend: &'static str },
}

impl Runner<'_> {
    fn name(&self) -> String {
        match self {
            Runner::Wsl { .. } => "wsl-gl".into(),
            Runner::Windows { backend, .. } => format!("windows-{backend}"),
        }
    }

    fn replay(&self, name: &str) -> String {
        match self {
            Runner::Wsl { .. } => root().join(REPLAYS).join(name).display().to_string(),
            Runner::Windows { .. } => format!("replays/{name}"),
        }
    }

    fn gpu_bench(&self, mut args: Vec<String>) -> Result<GpuReportOut, String> {
        args.insert(0, "gpu-bench".into());
        let (out, file) = match self {
            Runner::Wsl { cli, content, .. } => {
                let file = root().join("target/envelope/gpu.ron");
                args.extend(["--content".into(), content.display().to_string(), "--out".into(), file.display().to_string()]);
                let mut c = Command::new(cli);
                c.current_dir(root()).args(&args);
                for (k, v) in wsl_env(nvidia()) {
                    c.env(k, v);
                }
                (c.output().map_err(|e| format!("cannot run rl-cli: {e}"))?, file)
            }
            Runner::Windows { stage, backend } => {
                args.extend(sv(&["--content", "content", "--backend", backend, "--out", "gpu.ron"]));
                (winbuild::run(stage, "rl-cli.exe", &args, &[])?, stage.dir.join("gpu.ron"))
            }
        };
        if !out.status.success() {
            return Err(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
        }
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        ron::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))
    }

    fn windowed(&self, size: (u32, u32), seconds: f64) -> Result<FrameLogOut, String> {
        let script = format!("0 size {} {}\n", size.0, size.1);
        let common = |replay: &str, log: &str, script_file: &str, content: &str, data: &str| {
            sv(&[
                "--replay", replay, "--uncapped", "--no-sound", "--autoquit-after", &seconds.to_string(),
                "--frame-log", log, "--frame-log-skip", "3", "--script", script_file, "--content", content,
                "--data-dir", data,
            ])
        };
        let (out, file) = match self {
            Runner::Wsl { game, content, .. } => {
                let dir = root().join("target/envelope");
                let (log, sf) = (dir.join("frame-log.ron"), dir.join("size.txt"));
                std::fs::write(&sf, script).map_err(|e| e.to_string())?;
                let mut args = common(
                    &self.replay(HEAVY),
                    &log.display().to_string(),
                    &sf.display().to_string(),
                    &content.display().to_string(),
                    &dir.join("game-data").display().to_string(),
                );
                args.extend(["--window-system".into(), "x11".into()]);
                let mut c = Command::new(game);
                // The game puts itself on the GPU (wsl.rs) only when these are unset.
                c.current_dir(root()).args(&args).env_remove("WGPU_BACKEND").env_remove("GALLIUM_DRIVER");
                (c.output().map_err(|e| format!("cannot run rl-game: {e}"))?, log)
            }
            Runner::Windows { stage, backend } => {
                std::fs::write(stage.dir.join("size.txt"), script).map_err(|e| e.to_string())?;
                let args = common(&self.replay(HEAVY), "frame-log.ron", "size.txt", "content", "game-data");
                (winbuild::run(stage, "rl-game.exe", &args, &[("WGPU_BACKEND", *backend)])?, stage.dir.join("frame-log.ron"))
            }
        };
        if !out.status.success() {
            return Err(format!("rl-game: {}", String::from_utf8_lossy(&out.stderr)));
        }
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        ron::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))
    }
}

/// The heavy match (MW 1.0, 32 chaos bots, a 20 000 pool), recorded once.
fn heavy_replay(cli: &Path, content: &Path, plan: &Plan) -> Result<PathBuf, String> {
    let file = root().join(REPLAYS).join(HEAVY);
    std::fs::create_dir_all(root().join(REPLAYS)).map_err(|e| e.to_string())?;
    let out = Command::new(cli)
        .current_dir(root())
        .args(["stress", "--mod", "olx-mw-1-0", "--level", "olx-level-castlestrike", "--worms", "32"])
        .args(["--pool", "20000", "--seconds", &plan.heavy_seconds.to_string(), "--render-every", "0", "--no-verify"])
        .arg("--content")
        .arg(content)
        .arg("--record")
        .arg(&file)
        .output()
        .map_err(|e| format!("cannot run rl-cli: {e}"))?;
    if out.status.success() { Ok(file) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) }
}

/// The map and art points' replays the sweeps recorded (this run's or an earlier one's).
fn level_replays(plan: &Plan) -> Vec<(String, String, PathBuf)> {
    let mut v = Vec::new();
    for axis in [super::Axis::Map, super::Axis::Art] {
        for val in sweeps::values(axis, plan) {
            let p = root().join(REPLAYS).join(sweeps::replay_name(axis.name(), &val.label));
            if p.is_file() {
                v.push((axis.name().to_string(), val.label, p));
            }
        }
    }
    v
}

/// Owned arguments.
fn sv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn join<T: ToString>(v: &[T]) -> String {
    v.iter().map(T::to_string).collect::<Vec<_>>().join(",")
}

fn measure(r: &Runner, levels: &[(String, String, PathBuf)], plan: &Plan, b: &Budgets) -> Result<GpuPath, String> {
    let sizes = join(&plan.sizes.iter().map(|(w, h)| format!("{w}x{h}")).collect::<Vec<_>>());
    let mut runs = Vec::new();
    for i in 0..plan.runs {
        println!("  {} offscreen, run {}", r.name(), i + 1);
        runs.push(r.gpu_bench(sv(&[
            "--replay", &r.replay(HEAVY), "--adapter", "hardware", "--sizes", &sizes,
            "--start-tick", &plan.start_tick.to_string(), "--frames", &plan.gpu_frames.to_string(),
            "--probe-passes", &join(&plan.probe_passes), "--probe-frames", &plan.probe_frames.to_string(),
        ]))?);
    }
    let first = &runs[0];
    let mut path = GpuPath {
        adapter: first.adapter.clone(),
        backend: first.backend.clone(),
        device_type: first.device_type.clone(),
        timestamps: first.timestamps,
        ..GpuPath::default()
    };
    if !first.timestamps {
        path.notes.push("no timestamp queries on this adapter: CPU-side frame times only".into());
    }
    for (i, _) in plan.sizes.iter().enumerate() {
        let per: Vec<SizeOut> = runs.iter().filter_map(|x| x.sizes.get(i).cloned()).collect();
        path.sizes.push(results::gpu_size(&per, b));
    }
    for (axis, value, file) in levels {
        let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let args = sv(&[
            "--replay", &r.replay(&name), "--adapter", "hardware", "--sizes", "2560x1440",
            "--start-tick", &plan.level_start.to_string(), "--frames", &plan.level_frames.to_string(),
            "--probe-passes", "0", "--probe-frames", "1",
        ]);
        match r.gpu_bench(args) {
            Ok(rep) => {
                if let Some(s) = rep.sizes.first() {
                    path.levels.push(GpuLevel { axis: axis.clone(), value: value.clone(), size: s.size, total_ms: s.total_ms, gpu_ms: s.gpu_ms });
                }
            }
            Err(e) => path.notes.push(format!("{axis} {value}: {}", e.lines().last().unwrap_or(""))),
        }
    }
    let windows = matches!(r, Runner::Windows { .. });
    if windows || has_display(std::env::var("DISPLAY").ok().as_deref(), std::env::var("WAYLAND_DISPLAY").ok().as_deref()) {
        for &size in &plan.sizes {
            println!("  {} windowed {}x{}", r.name(), size.0, size.1);
            match r.windowed(size, plan.windowed_seconds) {
                Ok(log) => path.windowed.push(Windowed {
                    requested: size,
                    size: log.size,
                    reached: log.size == size,
                    fps: log.fps,
                    frame_ms: log.frame_ms,
                    busy_ms: log.busy_ms,
                }),
                Err(e) => path.notes.push(format!("windowed {}x{}: {}", size.0, size.1, e.lines().last().unwrap_or(""))),
            }
        }
    } else {
        path.notes.push("no display: windowed runs skipped".into());
    }
    Ok(path)
}

/// Every GPU path `which` names; skips are notices.
pub fn run(cli: &Path, content: &Path, which: GpuPaths, plan: &Plan, env: &mut Envelope) {
    if which == GpuPaths::None {
        return;
    }
    let b = env.budgets;
    if let Err(e) = heavy_replay(cli, content, plan) {
        println!("skip gpu (the heavy replay failed: {})", e.lines().last().unwrap_or(""));
        return;
    }
    let levels = level_replays(plan);
    if matches!(which, GpuPaths::Wsl | GpuPaths::Both) {
        match sweeps::build_release(&["rl-game"]) {
            Err(e) => println!("skip gpu wsl (rl-game does not build: {e})"),
            Ok(()) => {
                let r = Runner::Wsl { cli, game: root().join("target/release/rl-game"), content };
                match measure(&r, &levels, plan, &b) {
                    Ok(p) => {
                        env.gpu.insert(r.name(), p);
                    }
                    Err(e) => println!("skip gpu wsl ({})", e.lines().last().unwrap_or("")),
                }
            }
        }
    }
    if matches!(which, GpuPaths::Windows | GpuPaths::Both) {
        if let Some(why) = winbuild::missing() {
            println!("skip gpu windows ({why})");
            return;
        }
        println!("  building for Windows (the first build takes minutes)");
        let files: Vec<PathBuf> =
            std::iter::once(root().join(REPLAYS).join(HEAVY)).chain(levels.iter().map(|l| l.2.clone())).collect();
        let stage = match winbuild::build().and_then(|rel| winbuild::stage(&rel, content, &files)) {
            Ok(s) => s,
            Err(e) => {
                println!("skip gpu windows ({})", e.lines().last().unwrap_or(""));
                return;
            }
        };
        for backend in ["dx12", "vulkan"] {
            let r = Runner::Windows { stage: &stage, backend };
            match measure(&r, &levels, plan, &b) {
                Ok(p) => {
                    env.gpu.insert(r.name(), p);
                }
                Err(e) => println!("skip gpu windows {backend} ({})", e.lines().last().unwrap_or("")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review Focus 2: the WSL GPU's environment, and windowed runs only
    /// with a display.
    #[test]
    fn prerequisites_are_notices() {
        let e = wsl_env(true);
        assert!(e.contains(&("GALLIUM_DRIVER".into(), "d3d12".into())) && e.len() == 3);
        assert_eq!(wsl_env(false).len(), 2);
        assert!(has_display(Some(":0"), None) && has_display(None, Some("wayland-0")));
        assert!(!has_display(None, None) && !has_display(Some(""), None));
    }
}
```

- [ ] **Step 7: Write the OLX part**

`xtask/src/envelope/olx.rs`:

```rust
//! The OLX comparison (spec §6, ruling P8): each scenario timed in OLX
//! (Docker, `crate::olx_timing`) and in rl-sim (`rl-cli scenario --time`),
//! `runs` times each, medians, and the same work checked frame by frame.

use std::path::Path;
use std::process::Command;

use anyhow::Context as _;
use rl_oracle::Scenario;

use super::Plan;
use super::results::{self, OlxComparison, ScenarioTimingOut, TimingReportOut};
use crate::olx_timing::{Olx, PERF_DIR};
use crate::root;

const TIMED_DIR: &str = "target/envelope/olx";

fn rl_time(cli: &Path, content: &Path, name: &str, fixtures: &Path) -> anyhow::Result<ScenarioTimingOut> {
    let out = root().join("target/envelope/olx-time.ron");
    let _ = std::fs::remove_file(&out);
    let o = Command::new(cli)
        .current_dir(root())
        .args(["scenario", name, "--fixtures"])
        .arg(fixtures)
        .arg("--content")
        .arg(content)
        .arg("--time")
        .arg(&out)
        .output()
        .context("running rl-cli scenario")?;
    let text = std::fs::read_to_string(&out)
        .with_context(|| format!("rl-cli scenario wrote no times: {}", String::from_utf8_lossy(&o.stderr)))?;
    let r: TimingReportOut = ron::from_str(&text)?;
    r.scenarios.into_iter().next().context("no scenario timed")
}

pub fn run(cli: &Path, content: &Path, plan: &Plan) -> anyhow::Result<OlxComparison> {
    let olx = Olx::open()?;
    let timed = root().join(TIMED_DIR);
    std::fs::create_dir_all(&timed)?;
    let mut rows = Vec::new();
    let all = plan.olx_timed.iter().map(|n| (*n, false)).chain(plan.olx_heavy.iter().map(|n| (*n, true)));
    for (name, heavy) in all {
        let src = root().join(if heavy { PERF_DIR } else { "fixtures/oracle" }).join(format!("{name}.ron"));
        let sc = Scenario::from_ron(&std::fs::read_to_string(&src)?).with_context(|| src.display().to_string())?;
        let (mut o_runs, mut r_runs) = (Vec::new(), Vec::new());
        for run in 0..plan.runs {
            println!("  olx {name}, run {}", run + 1);
            let gz = (heavy && run == 0).then(|| timed.join(format!("{name}.jsonl.gz")));
            o_runs.push(olx.time(name, &sc, gz.as_deref())?);
            if heavy && run == 0 {
                std::fs::copy(&src, timed.join(format!("{name}.ron")))?;
            }
            let fixtures = if heavy { timed.clone() } else { root().join("fixtures/oracle") };
            r_runs.push(rl_time(cli, content, name, &fixtures)?);
        }
        let row = results::olx_row(name, &o_runs, &r_runs);
        println!(
            "  {name}: OLX p50 {:.1} µs, rl-sim p50 {:.1} µs (×{:.2}), peak {} projectiles, same work: {}",
            row.olx_us.p50, row.rl_us.p50, row.ratio_p50, row.projectiles_peak, row.same_work
        );
        rows.push(row);
    }
    Ok(OlxComparison { image: olx.image().into(), build: olx.build_info(), rows })
}
```

- [ ] **Step 8: Write the command**

Replace `xtask/src/envelope/mod.rs` with:

```rust
//! `cargo xtask envelope` (plan 8; spec
//! `docs/superpowers/specs/2026-10-11-performance-envelope-design.md`):
//! how far each axis goes on this machine before the tick, frame or
//! memory budget breaks, the GPU on WSL and native Windows with the shader
//! probe, and OLX's frame times. Every run writes `target/envelope/latest.ron`
//! and compares with `fixtures/bench/envelope/<host>.ron` (beyond each
//! axis's noise is flagged, never failed); `--bless --reason` writes that
//! file and logs it. Not part of `full`.

pub mod gpu;
pub mod olx;
pub mod results;
pub mod sweeps;

use crate::bench::Host;
use crate::root;
use results::Envelope;

/// A part of the envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum Axis {
    Worms,
    Pool,
    Fx,
    Map,
    Art,
    Gpu,
    Olx,
}

impl Axis {
    pub const ALL: [Axis; 7] = [Axis::Worms, Axis::Pool, Axis::Fx, Axis::Map, Axis::Art, Axis::Gpu, Axis::Olx];

    pub fn name(self) -> &'static str {
        match self {
            Axis::Worms => "worms",
            Axis::Pool => "pool",
            Axis::Fx => "fx",
            Axis::Map => "map",
            Axis::Art => "art",
            Axis::Gpu => "gpu",
            Axis::Olx => "olx",
        }
    }

    pub fn is_sweep(self) -> bool {
        !matches!(self, Axis::Gpu | Axis::Olx)
    }
}

/// Which GPU paths run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum GpuPaths {
    Wsl,
    Windows,
    Both,
    None,
}

/// What a run measures (spec §4, §5; `--quick` one run per point, fewer points).
#[derive(Clone, Debug)]
pub struct Plan {
    pub quick: bool,
    pub runs: usize,
    pub seconds: f32,
    pub worms: Vec<u32>,
    pub pools: Vec<u32>,
    pub fx: Vec<u32>,
    pub maps: Vec<(u32, u32)>,
    pub arts: Vec<u32>,
    pub heavy_seconds: f32,
    pub sizes: Vec<(u32, u32)>,
    pub start_tick: u32,
    pub gpu_frames: u32,
    pub probe_passes: Vec<u32>,
    pub probe_frames: u32,
    pub level_start: u32,
    pub level_frames: u32,
    pub windowed_seconds: f64,
    pub olx_timed: Vec<&'static str>,
    pub olx_heavy: Vec<&'static str>,
}

impl Plan {
    pub fn new(quick: bool) -> Plan {
        use crate::olx_timing::{HEAVY, TIMED};
        if quick {
            return Plan {
                quick,
                runs: 1,
                seconds: 5.0,
                worms: vec![2, 16, 32],
                pools: vec![3000, 65_536],
                fx: vec![1024, 65_536],
                maps: vec![(1024, 1024), (4096, 4096)],
                arts: vec![1, 8],
                heavy_seconds: 15.0,
                sizes: vec![(1920, 1080), (3840, 2160)],
                start_tick: 500,
                gpu_frames: 200,
                probe_passes: vec![0, 4, 16],
                probe_frames: 40,
                level_start: 200,
                level_frames: 100,
                windowed_seconds: 8.0,
                olx_timed: TIMED[..2].to_vec(),
                olx_heavy: HEAVY[..1].to_vec(),
            };
        }
        Plan {
            quick,
            runs: 3,
            seconds: 20.0,
            worms: vec![2, 4, 8, 16, 24, 32],
            pools: vec![3000, 10_000, 20_000, 50_000, 65_536],
            fx: vec![1024, 4096, 16_384, 65_536],
            maps: vec![(1024, 1024), (2048, 2048), (4096, 2048), (4096, 4096)],
            arts: vec![1, 2, 4, 8],
            heavy_seconds: 40.0,
            sizes: vec![(1920, 1080), (2560, 1440), (3440, 1440), (3840, 2160)],
            start_tick: 1000,
            gpu_frames: 600,
            probe_passes: vec![0, 1, 2, 4, 8, 16, 32],
            probe_frames: 120,
            level_start: 500,
            level_frames: 300,
            windowed_seconds: 15.0,
            olx_timed: TIMED.to_vec(),
            olx_heavy: HEAVY.to_vec(),
        }
    }
}

pub struct Opts {
    pub quick: bool,
    pub only: Vec<Axis>,
    pub gpu: GpuPaths,
    pub bless: Option<String>,
}

pub fn run(o: &Opts) -> anyhow::Result<bool> {
    if let Some(reason) = &o.bless {
        anyhow::ensure!(!reason.trim().is_empty(), "--reason must say why the results change");
    }
    let content = root().join("content/packs");
    anyhow::ensure!(content.join("olx-core").is_dir(), "no content in {} (`cargo xtask fetch-olx-content`)", content.display());
    let host = Host::here();
    let plan = Plan::new(o.quick);
    let axes: Vec<Axis> = if o.only.is_empty() { Axis::ALL.to_vec() } else { o.only.clone() };
    println!(
        "envelope ({}): keep this PC otherwise idle; load average now {:.2}. About {} minutes.",
        if o.quick { "quick" } else { "full" },
        results::load_average().unwrap_or(0.0),
        if o.quick { 10 } else { 45 }
    );
    sweeps::build_release(&["rl-cli"]).map_err(anyhow::Error::msg)?;
    let cli = root().join("target/release/rl-cli");
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut env = Envelope {
        note: "`cargo xtask envelope` (plan 8): medians of `runs` runs; valid on `host` only.".into(),
        host: host.clone(),
        date: crate::bless::date(secs),
        quick: o.quick,
        runs: plan.runs,
        seconds: plan.seconds,
        ..Envelope::default()
    };
    for axis in axes.iter().copied().filter(|a| a.is_sweep()) {
        println!("== {} sweep", axis.name());
        let sweep = sweeps::run_axis(&cli, &content, axis, &plan);
        env.sweeps.insert(axis.name().into(), sweep);
    }
    if axes.contains(&Axis::Gpu) {
        println!("== gpu");
        gpu::run(&cli, &content, o.gpu, &plan, &mut env);
    }
    if axes.contains(&Axis::Olx) {
        println!("== olx");
        match olx::run(&cli, &content, &plan) {
            Ok(c) => env.olx = Some(c),
            Err(e) => println!("skip olx ({e:#}); is Docker running (`docker info`)?"),
        }
    }
    results::finish(&mut env);
    results::write(&root().join(results::LATEST), &env)?;
    for (axis, s) in &env.sweeps {
        let b = |x: &Option<results::Breaking>| {
            x.as_ref().map_or("holds everywhere".into(), |b| format!("{} ({})", b.value, b.causes.join("; ")))
        };
        println!("{axis}: breaks at {} (144 Hz); {} (60 Hz)", b(&s.breaking_144), b(&s.breaking_60));
    }
    let path = results::path_for(&host);
    let stored = results::load(&root().join(&path)).map_err(anyhow::Error::msg)?;
    match &stored {
        Some(s) => match crate::bench::host_notice(&s.host, &host) {
            Some(n) => println!("notice: {path}: {n}"),
            None => {
                let flags = results::compare(s, &env);
                for f in &flags {
                    println!("{}", f.line());
                }
                let n = flags.iter().filter(|f| f.flagged()).count();
                println!("{n} of {} measurements beyond their noise (flagged with !, not failed)", flags.len());
            }
        },
        None => println!("notice: no results for this host yet ({path}); `--bless --reason \"<why>\"` writes them"),
    }
    if let Some(reason) = &o.bless {
        let same_host = stored.filter(|s| crate::bench::host_notice(&s.host, &host).is_none());
        let merged = results::merged(same_host, env);
        if !merged.kept.is_empty() {
            println!("kept from before (not measured now): {:?}", merged.kept);
        }
        let file = root().join(&path);
        let before = std::fs::read(&file).ok();
        results::write(&file, &merged)?;
        let changed = if std::fs::read(&file).ok() != before { vec![path] } else { vec![] };
        crate::bless::log_blessing("envelope", reason, &changed)?;
    }
    println!("results: {}", results::LATEST);
    Ok(true)
}
```

In `xtask/src/main.rs`, add the command:

```rust
    /// The performance envelope (plan 8): sweeps of worms, projectiles, effects, map
    /// size and art scale, the GPU on WSL and native Windows with the shader probe,
    /// and OLX's frame times; compared with fixtures/bench/envelope/<host>.ron
    /// (flagged, never failed). About 45 minutes (--quick about 10). Not in `full`.
    Envelope {
        /// One run per point and fewer points.
        #[arg(long)]
        quick: bool,
        /// Only these parts (repeatable): worms, pool, fx, map, art, gpu, olx.
        #[arg(long, value_enum)]
        only: Vec<envelope::Axis>,
        /// Which GPU paths run.
        #[arg(long, value_enum, default_value = "both")]
        gpu: envelope::GpuPaths,
        /// Write the results for this host, logged in fixtures/blessings.log.
        #[arg(long, requires = "reason")]
        bless: bool,
        /// Why the results change (with --bless).
        #[arg(long)]
        reason: Option<String>,
    },
```

and `Command::Envelope { quick, only, gpu, bless, reason } => envelope::run(&envelope::Opts { quick, only, gpu, bless: reason.filter(|_| bless) }),`.

- [ ] **Step 9: Run the tests and clippy**

Run: `cargo nextest run -p xtask && cargo clippy -p xtask --all-targets -- -D warnings`
Expected: PASS and no warnings (the long argument arrays are fine for rustfmt; run `cargo fmt` if `check`'s fmt step complains).

- [ ] **Step 10: A quick envelope run (shakes out the wiring)**

Run: `cargo xtask envelope --quick`
Expected (about 10 minutes): the five sweeps print a line per point (the 4096² map and `a8` points may print caps: the content cap for `a8`, perhaps the area cache for 4096²); `== gpu` prints `wsl-gl offscreen, run 1` and windowed runs (windows open on the desktop), then the Windows build and `windows-dx12` / `windows-vulkan`; `== olx` prints a line per scenario with `same work: true`; a breaking-point summary per axis; "notice: no results for this host yet". On each outcome:
- A sweep point line shows `failed: ...` for a reason other than memory (a panic, an invariant): stop, debug it with the systematic-debugging skill (it is a real bug the envelope found), fix it in its own commit (`rl-...: <fix>`), and note it in the ledger.
- `same work: false` for an existing oracle scenario: compare the first differing frame with `rl-cli scenario <name> --dump <frame>`; the OLX run used a header-only trace, so a difference means the timing patch changed behaviour: redo Task 6 Step 4.
- `heavy-spikeballs` peaks below 2500 projectiles: raise its worms to 16 (repeat the two spots and the inputs for worms 8–15) and its frames to 1000 in `fixtures/perf/olx/heavy-spikeballs.ron`, rerun `cargo xtask envelope --quick --only olx`.
- `skip gpu wsl (asked for the hardware adapter, got llvmpipe ...)`: the d3d12 GL driver did not load; run `GALLIUM_DRIVER=d3d12 glxinfo -B | head` and fix the environment in `wsl_env` (do not drop the check).
- A windowed point with `reached: false`: the monitor is smaller; keep it (the report says "offscreen only" for that size).

- [ ] **Step 11: Docs, gate, commit**

`xtask/CLAUDE.md`: an `envelope` entry: the parts (`--only`), `--gpu`, `--quick`, `--bless --reason`; the modules (`results.rs` pure and tested, `sweeps.rs`, `gpu.rs`, `olx.rs`); `target/envelope/latest.ron` every run; the file `fixtures/bench/envelope/<host>.ron` (`host_key`); flagged beyond `noise` (the largest run spread of an axis or GPU path, at least `NOISE_FLOOR`), never failed; not in `full`.

Run: `cargo xtask check`
Expected: `check passed`.

```bash
git add xtask
git commit -F - <<'EOF'
xtask: add the performance envelope command

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

### Task 8: The report: `docs/perf/envelope.md` from the results

**Files:**
- Create: `xtask/src/envelope/report.rs`
- Modify: `xtask/src/envelope/mod.rs` (`pub mod report;`, `Opts::report`, `Opts::from`), `xtask/src/main.rs` (`--report`, `--from`)
- Docs: `xtask/CLAUDE.md`

**Interfaces:**
- Consumes: Task 7's `results::{Envelope, Sweep, Point, GpuPath, Budgets, causes, load, path_for}`.
- Produces: `report::{REPORT, START, END, summary_of, render, write}`: `REPORT = "docs/perf/envelope.md"`, `START = "<!-- summary:start -->"`, `END = "<!-- summary:end -->"`, `summary_of(existing: &str) -> Option<String>`, `render(env: &Envelope, summary: &str) -> String`, `write(from: Option<&Path>) -> anyhow::Result<PathBuf>`; `cargo xtask envelope --report [--from FILE]`.

- [ ] **Step 1: Write the failing tests**

`xtask/src/envelope/report.rs` begins with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::results::{Breaking, Fit, GpuSize, Point, Spread, Sweep};

    fn env() -> Envelope {
        let mut e = Envelope { date: "2026-10-11".into(), runs: 3, ..Envelope::default() };
        let p = |v: &str, n: f64, p99: f64| Point {
            value: v.into(),
            n,
            runs: 3,
            tick_us: Spread { p50: p99 / 2.0, p99, max: p99 },
            ..Point::default()
        };
        e.sweeps.insert(
            "worms".into(),
            Sweep {
                axis: "worms".into(),
                points: vec![p("16", 16.0, 2000.0), p("32", 32.0, 12_000.0)],
                breaking_144: Some(Breaking { value: "32".into(), causes: vec!["tick p99 12.00 ms > 10 ms".into()] }),
                fit: Some(Fit { per_unit_us_p50: 150.0, per_unit_us_p99: 300.0, intercept_us_p99: 100.0, limit: 33.0 }),
                ..Sweep::default()
            },
        );
        e.gpu.insert(
            "wsl-gl".into(),
            GpuPath {
                adapter: "RTX".into(),
                timestamps: true,
                sizes: vec![GpuSize { size: (3840, 2160), ms_per_pass: 0.25, passes_144: 24, passes_60: 62, ..GpuSize::default() }],
                ..GpuPath::default()
            },
        );
        e
    }

    /// Review Focus 5.
    #[test]
    fn regenerating_keeps_the_hand_written_summary() {
        let summary = "Shaders: about 24 passes at 4K.\n\nMaps: 4096² holds.";
        let doc = render(&env(), summary);
        assert_eq!(summary_of(&doc).as_deref(), Some(summary));
        assert_eq!(render(&env(), &summary_of(&doc).unwrap()), doc, "regenerating is stable");
        assert_eq!(summary_of("no markers"), None);
    }

    #[test]
    fn the_report_names_breaking_points_projections_and_probe_fits() {
        let doc = render(&env(), "s");
        for want in [
            "Breaking point (144 Hz): **32**",
            "tick p99 12.00 ms > 10 ms",
            "reaches 10 ms at about 33 worms",
            "| 3840×2160 | 0.250 | 24 | 62 |",
            "RTX",
            "| Projectiles | 3000",
        ] {
            assert!(doc.contains(want), "missing {want:?} in:\n{doc}");
        }
    }
}
```

Add `pub mod report;` to `xtask/src/envelope/mod.rs`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p xtask report`
Expected: FAIL to compile ("cannot find function `render`").

- [ ] **Step 3: Write the report**

Above the tests:

```rust
//! `docs/perf/envelope.md` from a results file (plan 8, spec §7): the
//! method, the machine, every sweep's table and breaking point, each GPU
//! path's frames, shader-probe fits and windowed runs, the OLX comparison
//! and the limits. The headroom summary between [`START`] and [`END`] is
//! written by hand and kept word for word when the report is made again.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::results::{self, Budgets, Envelope, GpuPath, Point, Sweep};
use crate::root;

pub const REPORT: &str = "docs/perf/envelope.md";
pub const START: &str = "<!-- summary:start -->";
pub const END: &str = "<!-- summary:end -->";
const NO_SUMMARY: &str = "_Not written yet. Write it here; `cargo xtask envelope --report` keeps it._";

/// The hand-written summary in an existing report.
pub fn summary_of(existing: &str) -> Option<String> {
    let a = existing.find(START)? + START.len();
    let b = existing[a..].find(END)? + a;
    Some(existing[a..b].trim_matches('\n').to_string())
}

fn size(s: (u32, u32)) -> String {
    format!("{}×{}", s.0, s.1)
}

fn ms(us: f64) -> f64 {
    us / 1000.0
}

fn point_row(p: &Point, b: &Budgets) -> String {
    if p.runs == 0 {
        return format!("| {} | — | — | — | — | — | — | — | — | — | — | {} |", p.value, p.caps.join("; "));
    }
    let frame = p.frame.as_ref().map_or("—".to_string(), |f| format!("{:.2}", f.total_ms.p99));
    let c = results::causes(p, b, b.frame_144_ms);
    let holds = if c.is_empty() { "yes".to_string() } else { c.join("; ") };
    let sat = if p.saturated.is_empty() { String::new() } else { format!(" ({})", p.saturated.join("; ")) };
    format!(
        "| {} | {:.2} / {:.2} | {:.0} % | {:.2} | {:.2} | {:.2} | {:.2} | {} | {} | {:.0} | {frame} | {holds}{sat} |",
        p.value,
        ms(p.tick_us.p50),
        ms(p.tick_us.p99),
        p.budget_share * 100.0,
        ms(p.sim_us.p99),
        ms(p.think_us.p99),
        ms(p.present_us.p99),
        ms(p.build_us.p99),
        p.projectiles_peak,
        p.fx_peak,
        p.peak_mib
    )
}

fn sweep(o: &mut String, s: &Sweep, b: &Budgets) {
    let _ = writeln!(o, "### {}\n\n{} ({} on {}).\n", s.axis, s.what, s.mod_id, s.level);
    let _ = writeln!(
        o,
        "| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | \
         projectiles | effects | MiB | frame p99 (ms) | holds at 144 Hz |\n|---|---|---|---|---|---|---|---|---|---|---|---|"
    );
    for p in &s.points {
        let _ = writeln!(o, "{}", point_row(p, b));
    }
    let br = |x: &Option<results::Breaking>| {
        x.as_ref().map_or("none: every value holds".to_string(), |b| format!("**{}** ({})", b.value, b.causes.join("; ")))
    };
    let _ = writeln!(o, "\nBreaking point (144 Hz): {}. Breaking point (60 Hz): {}.", br(&s.breaking_144), br(&s.breaking_60));
    if let Some(f) = s.fit {
        if s.axis == "worms" {
            let _ = writeln!(
                o,
                "\nCost per worm: {:.0} µs of tick at p50, {:.0} µs at p99. At this rate the tick p99 reaches 10 ms at \
                 about {:.0} worms (a projection: `MAX_WORMS` stays 32).",
                f.per_unit_us_p50, f.per_unit_us_p99, f.limit
            );
        } else {
            let _ = writeln!(o, "\nTick p99 grows {:.4} µs per unit of {} (least squares).", f.per_unit_us_p99, s.axis);
        }
    }
    let _ = writeln!(o);
}

fn gpu(o: &mut String, key: &str, g: &GpuPath) {
    let _ = writeln!(o, "### {key}: {} ({}, {})\n", g.adapter, g.backend, g.device_type);
    let _ = writeln!(
        o,
        "GPU times: {}.\n",
        if g.timestamps { "timestamp queries" } else { "none (no timestamp queries): CPU-side frame times only" }
    );
    for n in &g.notes {
        let _ = writeln!(o, "- {n}");
    }
    let _ = writeln!(
        o,
        "\nThe heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool) offscreen, a tick and a frame each:\n\n\
         | size | tick + frame p50 / p99 (ms) | frame p99, CPU then GPU (ms) | GPU p99 (ms) |\n|---|---|---|---|"
    );
    for s in &g.sizes {
        let gp = s.gpu_ms.map_or("—".to_string(), |x| format!("{:.2}", x.p99));
        let _ = writeln!(o, "| {} | {:.2} / {:.2} | {:.2} | {gp} |", size(s.size), s.total_ms.p50, s.total_ms.p99, s.frame_ms.p99);
    }
    let _ = writeln!(
        o,
        "\nShader headroom (a full-screen blur pass, `rl_render::Probe`):\n\n\
         | size | GPU ms per pass | passes that fit at 144 Hz | at 60 Hz |\n|---|---|---|---|"
    );
    for s in &g.sizes {
        let _ = writeln!(o, "| {} | {:.3} | {} | {} |", size(s.size), s.ms_per_pass, s.passes_144, s.passes_60);
    }
    if !g.windowed.is_empty() {
        let _ = writeln!(o, "\nWindowed, uncapped (`rl-game --frame-log`, HUD included):\n\n| asked | got | FPS | frame p50 / p99 (ms) | busy p99 (ms) |\n|---|---|---|---|---|");
        for w in &g.windowed {
            let got = if w.reached { size(w.size) } else { format!("{} (monitor smaller: see offscreen)", size(w.size)) };
            let _ = writeln!(o, "| {} | {got} | {:.0} | {:.2} / {:.2} | {:.2} |", size(w.requested), w.fps, w.frame_ms.p50, w.frame_ms.p99, w.busy_ms.p99);
        }
    }
    if !g.levels.is_empty() {
        let _ = writeln!(o, "\nThe sweeps' levels at 2560×1440:\n\n| axis | value | tick + frame p99 (ms) | GPU p99 (ms) |\n|---|---|---|---|");
        for l in &g.levels {
            let gp = l.gpu_ms.map_or("—".to_string(), |x| format!("{:.2}", x.p99));
            let _ = writeln!(o, "| {} | {} | {:.2} | {gp} |", l.axis, l.value, l.total_ms.p99);
        }
    }
    let _ = writeln!(o);
}

/// The report of `env` with `summary` (kept as given).
pub fn render(env: &Envelope, summary: &str) -> String {
    let b = env.budgets;
    let mut o = String::new();
    let _ = writeln!(o, "# Performance envelope\n");
    let _ = writeln!(
        o,
        "Made by `cargo xtask envelope --report` from the results measured on {} ({} run{} per point{}). \
         Everything but the summary is regenerated; edit only the summary.\n",
        env.date,
        env.runs,
        if env.runs == 1 { "" } else { "s" },
        if env.quick { ", `--quick`" } else { "" }
    );
    let _ = writeln!(o, "## Headroom summary\n\n{START}\n{}\n{END}\n", summary.trim_matches('\n'));
    let _ = writeln!(
        o,
        "## Method\n\n\
         - Budgets: tick p99 ≤ {} ms (OLX's 100 Hz frame), frame p99 ≤ {:.1} ms (144 Hz) and ≤ {:.1} ms (60 Hz), \
         peak memory below {:.0} MiB. A point holds when all of them do; the breaking point is the first value that \
         does not, with the cause (tick, frame, memory, or a cap).\n\
         - Sweeps: `rl-cli stress`, one process a point, medians of the runs; the reference is Classic on Castle \
         Strike, 16 chaos bots of mixed difficulty, OLX's pool, {} s, seed 1, `LoadingTime` 0, bonuses every 5 s. \
         The pool sweep plays MW 1.0. Map and art levels are generated in memory (`gen-<w>x<h>-a<scale>-s1`).\n\
         - Frames: `rl-cli gpu-bench` offscreen on the hardware adapter (timestamps when the adapter has them), and \
         `rl-game --frame-log` windowed and uncapped; on WSL (GL over Mesa's d3d12) and native Windows (DX12, \
         Vulkan) built with mingw-w64 and run through interop.\n\
         - OLX: the oracle's patched OLX (Docker, a Release build) timing each simulated frame to a file of its own; \
         rl-sim timed on the same scenarios, the same work checked frame by frame.\n",
        b.tick_ms, b.frame_144_ms, b.frame_60_ms, b.memory_mib, env.seconds
    );
    let _ = writeln!(o, "## The machine\n\n- CPU: {} ({} logical CPUs)", env.host.cpu, env.host.cpus);
    for (k, g) in &env.gpu {
        let _ = writeln!(o, "- {k}: {} ({}, {})", g.adapter, g.backend, g.device_type);
    }
    let _ = writeln!(o, "\n## Sweeps\n");
    for s in env.sweeps.values() {
        sweep(&mut o, s, &b);
    }
    let _ = writeln!(o, "## GPU\n");
    for (k, g) in &env.gpu {
        gpu(&mut o, k, g);
    }
    let _ = writeln!(o, "## OLX\n");
    match &env.olx {
        Some(c) => {
            let _ = writeln!(
                o,
                "Image `{}` ({}). OLX runs in a Docker container on WSL's own kernel (no extra virtualisation, the same \
                 clock); its frame is the gus logic, the client's and the server's frame, without the trace writing. \
                 OLX draws on the CPU at 640 × 480 and is not timed here (not comparable like for like).\n\n\
                 | scenario | frames | peak projectiles | OLX p50 / p99 (µs) | rl-sim p50 / p99 (µs) | rl-sim / OLX (p50) | same work | matches OLX |\n\
                 |---|---|---|---|---|---|---|---|",
                c.image, c.build
            );
            for r in &c.rows {
                let _ = writeln!(
                    o,
                    "| {} | {} | {} | {:.1} / {:.1} | {:.1} / {:.1} | {:.2} | {} | {} |",
                    r.name, r.frames, r.projectiles_peak, r.olx_us.p50, r.olx_us.p99, r.rl_us.p50, r.rl_us.p99,
                    r.ratio_p50, if r.same_work { "yes" } else { "no" },
                    r.matches_olx.map_or("— (timing trace)", |m| if m { "yes" } else { "no" })
                );
            }
        }
        None => {
            let _ = writeln!(o, "Not measured in these results.");
        }
    }
    let _ = writeln!(
        o,
        "\n### Limits\n\n| | OLX (`d1ad953f`) | rust-liero |\n|---|---|---|\n\
         | Worms | 32 (`MAX_WORMS`, `include/Consts.h:34`) | 32 (`rl_sim::MAX_WORMS`) |\n\
         | Projectiles | 3000 (`MAX_PROJECTILES`, `include/CProjectile.h:36`) | 3000 by default, up to 65 536 (`MatchSetup::MAX_POOL_CAPACITY`) |\n\
         | Map side | 4096 (12-bit worm positions) | 4096 for imported levels (`rl-import-lxl`) |\n\
         | Effects | 1024 (`MAX_ENTITIES`, `include/Entity.h:30`) | 1024 (`rl_fx::MAX_ENTITIES`; `Fx::with_capacity` in benches) |\n\
         | Colour image | — | 256 MiB (`MAX_COLOR_MAP_BYTES`), art scale at most 8 |"
    );
    o
}

/// Writes the report of this host's results file (or `from`), keeping the summary.
pub fn write(from: Option<&Path>) -> anyhow::Result<PathBuf> {
    let file = match from {
        Some(p) => p.to_path_buf(),
        None => root().join(results::path_for(&crate::bench::Host::here())),
    };
    let env = results::load(&file).map_err(anyhow::Error::msg)?.ok_or_else(|| {
        anyhow::anyhow!("no results at {} (`cargo xtask envelope --bless --reason \"<why>\"` first)", file.display())
    })?;
    let out = root().join(REPORT);
    let summary =
        std::fs::read_to_string(&out).ok().and_then(|t| summary_of(&t)).unwrap_or_else(|| NO_SUMMARY.to_string());
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, render(&env, &summary))?;
    Ok(out)
}
```

- [ ] **Step 4: Wire `--report`**

In `xtask/src/envelope/mod.rs`, add `pub report: bool,` and `pub from: Option<std::path::PathBuf>,` to `Opts`, and at the start of `run`:

```rust
    if o.report {
        let out = report::write(o.from.as_deref())?;
        println!("wrote {}", out.display());
        return Ok(true);
    }
```

In `xtask/src/main.rs`'s `Envelope` command add:

```rust
        /// Write docs/perf/envelope.md from this host's results file (or --from) and stop.
        #[arg(long)]
        report: bool,
        /// The results file the report is made from.
        #[arg(long, requires = "report")]
        from: Option<PathBuf>,
```

and pass `report, from` into `envelope::Opts`.

- [ ] **Step 5: Run the tests**

Run: `cargo nextest run -p xtask report`
Expected: PASS (2 tests). If `the_report_names_breaking_points_projections_and_probe_fits` misses a string, fix the renderer's wording to match the test, not the test.

- [ ] **Step 6: A report from the quick run**

Run: `cargo xtask envelope --report --from target/envelope/latest.ron && sed -n 1,60p docs/perf/envelope.md`
Expected: "wrote …/docs/perf/envelope.md"; the file opens with the summary markers holding the "Not written yet" line, then every section. Then `git checkout -- docs/perf 2>/dev/null; rm -f docs/perf/envelope.md` (Task 9 writes the real one from the blessed results).

- [ ] **Step 7: Docs, gate, commit**

`xtask/CLAUDE.md` envelope entry: `--report [--from F]` (`report.rs`: regenerated from the results; the summary between `<!-- summary:start -->` and `<!-- summary:end -->` is kept).

Run: `cargo xtask check`
Expected: `check passed`.

```bash
git add xtask
git commit -F - <<'EOF'
xtask: generate the performance envelope report

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

### Task 9: Run the envelope on this machine, bless, write the headroom summary, docs

**Files:**
- Create: `fixtures/bench/envelope/<host>.ron` (by the bless), `docs/perf/envelope.md` (by `--report`, then the summary by hand)
- Modify: `fixtures/blessings.log` (by the bless), `docs/porting/STATUS.md`, `README.md` (Checks), `CHANGELOG.md` (Unreleased), `crates/rl-cli/CLAUDE.md` and `xtask/CLAUDE.md` (run times seen)

**Interfaces:**
- Consumes: everything above: `cargo xtask envelope [--bless --reason] [--report]`.
- Produces: the user's answer (the report and its summary) and the committed baseline.

- [ ] **Step 1: Preconditions**

Run, from the repository:

```bash
cat /proc/loadavg
docker info --format '{{.ServerVersion}}'
rustup target list --installed | grep x86_64-pc-windows-gnu
echo "$DISPLAY $WAYLAND_DISPLAY"
cargo xtask check
```

Expected: a load average below 1.0 (the PC is idle: the user confirmed it; close other programs if not); a Docker version (else `sudo service docker start`); the Windows target listed (else Task 5 Step 1); a display set (WSLg); `check passed`. Tell the user that windows will open on the desktop during the run.

- [ ] **Step 2: The full run, blessed**

Run (about 45 minutes; run it in the background and wait for it):

```bash
cargo xtask envelope --bless --reason "first performance envelope on this machine (plan 8): WSL GL and native Windows DX12/Vulkan, OLX comparison" 2>&1 | tee target/envelope/run.log
```

Expected: every sweep prints its points; `== gpu` measures `wsl-gl`, `windows-dx12` and `windows-vulkan` (or prints a `skip ...` notice naming why); `== olx` prints a row per scenario with `same work: true`; the breaking-point summary; "notice: no results for this host yet"; then "blessed, recorded in fixtures/blessings.log: … envelope: first performance envelope … fixtures/bench/envelope/<host>.ron".

On each outcome:
- A part skipped with a notice (e.g. Vulkan absent on Windows): keep it; the report says so. Do not bless again for it.
- A point `failed:` for anything but memory or a cap: a real bug; debug it (systematic-debugging skill), fix it in its own commit with a test, then rerun only that axis: `cargo xtask envelope --only <axis> --gpu none --bless --reason "<axis> again after <fix>"` (the bless merges: the other parts are kept).
- The load-average notice printed for many points: the PC was not idle; rerun those axes with `--only` once it is.

- [ ] **Step 3: Generate the report**

Run: `cargo xtask envelope --report`
Expected: "wrote …/docs/perf/envelope.md".

- [ ] **Step 4: Write the headroom summary**

Replace the "Not written yet" line between `<!-- summary:start -->` and `<!-- summary:end -->` in `docs/perf/envelope.md` with the answer, every number taken from the report's tables below it (no number that is not there). Exactly these paragraphs, each starting with its bold label:

1. **Budget left today.** At the reference configuration and with today's settings (16 bots, Classic, OLX's pool): the tick p99 and its share of the 10 ms budget; the heavy replay's tick + frame p99 at 2560 × 1440 on WSL and on Windows DX12 against 6.9 ms and 16.7 ms; how much is left for networking (which will need several ticks per frame for re-simulation).
2. **Shaders.** The GPU ms one full-screen pass costs at 1920 × 1080, 2560 × 1440 and 3840 × 2160 on each path, and how many fit at 144 Hz and 60 Hz; whether the probe was timed on the GPU or only on the CPU (and why, from the path's notes); one sentence on what that means (e.g. "a bloom of N passes fits at 4K 60").
3. **Larger maps.** The map sweep's breaking point and cause (tick, memory, a cap such as the area cache), the frame p99 of the largest map drawn, and what grows with map size (bots' route cache, memory).
4. **More detailed art.** The art sweep: memory at each art scale against 2 GiB, the content cap at art scale 8 (`MAX_COLOR_MAP_BYTES`), the frame cost of the art-scale levels, and what raising the cap would need.
5. **More players.** The cost per worm (p50 and p99), the tick p99 at 32, and the projection where 10 ms would be reached past 32 (a projection only: `MAX_WORMS` stays 32); add the projectile and effects sweeps' breaking points here (more players fire more).
6. **Against OLX.** rl-sim's tick against OLX's frame on the same scenarios (the ratio range), that the work was the same (projectiles and `rand()` calls), and the Docker note.
7. **Cheap fixes noticed (for a later plan).** A bullet list of what the runs showed would be cheap to improve (e.g. a cap reached early, a phase that dominates the tick), each with the table it comes from. If there are none, say so.

Then run `cargo xtask envelope --report` again and confirm with `git diff docs/perf/envelope.md` that the summary is unchanged (Review Focus 5 on the real file).

- [ ] **Step 5: Docs**

- `docs/porting/STATUS.md`: below the benchmark and stress rows (lines 71–72), the row `| Performance envelope (plan 8): sweeps of worms, pool, effects, map and art; the GPU on WSL GL and Windows DX12/Vulkan with timestamps and the shader probe; OLX frame timing | `tools/olx-oracle` (`ORACLE_TIMING`) | `xtask/src/envelope/`, `rl-cli gpu-bench`, `rl-game --frame-log` | measured | `fixtures/bench/envelope/<host>.ron`, `docs/perf/envelope.md` |`. No deliberate deviation (nothing in the simulation changed). After the "Spec §21 decisions" section, a "Performance envelope decisions (plan 8)" section listing rulings P1–P13, one line each.
- `README.md` "Checks": "`cargo xtask envelope [--quick] [--only <part>] [--gpu wsl|windows|both|none]` measures how far the game goes on this machine (about 45 minutes; not part of `full`) and compares with `fixtures/bench/envelope/<host>.ron`; `--bless --reason "<why>"` writes it, `--report` regenerates `docs/perf/envelope.md`. `cargo xtask windows` builds the Windows binaries."
- `CHANGELOG.md` under `## Unreleased`, a `### Added` section (create it above `### Changed` if missing):
  - "`cargo xtask envelope`: the performance envelope (players, projectiles, effects, map size, art scale, resolution, full-screen shader passes) on this machine, on WSL and native Windows, against OLX; results in `fixtures/bench/envelope/`, report in `docs/perf/envelope.md`."
  - "`rl-cli gpu-bench` (a recorded match drawn on the GPU at several sizes, with GPU timestamps and a shader-headroom probe) and `rl-cli stress` options `--fx-cap`, `--build-every`, `--memory-ceiling-mib`, `--record` and generated levels (`--level gen-<w>x<h>-a<scale>-s<seed>`)."
  - "`rl-game --frame-log`: a windowed run's frame times as RON."
  - "`cargo xtask windows`: `rl-cli` and `rl-game` cross-compiled for Windows and run from WSL."
- `xtask/CLAUDE.md`: the envelope's real duration on this machine (from `run.log`'s timestamps or a stopwatch).

- [ ] **Step 6: Gates**

Run: `cargo xtask check`
Expected: `check passed`.
Run: `cargo xtask full`
Expected: `full passed` (render goldens, HUD snapshots, match goldens and the bench baselines unchanged by this plan).
Run: `git status --short`
Expected: only the files of this task (and no `target/` or staged Windows files).

- [ ] **Step 7: Commit**

```bash
git add fixtures/bench/envelope fixtures/blessings.log docs/perf/envelope.md docs/porting/STATUS.md README.md CHANGELOG.md crates/rl-cli/CLAUDE.md xtask/CLAUDE.md
git commit -F - <<'EOF'
perf: bless the first performance envelope and write its report

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
EOF
```

- [ ] **Step 8: Hand the answer to the user**

Report: the path `docs/perf/envelope.md`, the summary's seven paragraphs (quoted), each axis's breaking point and cause, which parts were skipped and why, and the cheap fixes listed. Never push.
