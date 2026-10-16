# Performance Envelope: Design

- **Date:** 2026-10-11
- **Status:** Draft, awaiting the user's review
- **Builds on:** `2026-10-10-foundation-and-local-play-design.md` (§21 G8
  benchmarks, the engine stress test of plan 7 task 8).

## 1. Purpose

The user's words: *"I want to know what headroom we may have if in the future
we decide to add shaders, larger or more detailed map and other improvements
to truly make the game next gen."* And: *"It should serve as both 'how far we
can go' answer, and baseline benchmark for advanced optimizations in the
future."*

So the milestone produces two things:

1. **The answer:** a report saying, for each axis (players, projectiles,
   effects, map size, art detail, screen resolution), where the game stops
   holding its frame and tick budgets on the user's machine, what breaks
   first, how much budget is left at today's settings, and how it compares
   with OLX.
2. **The baseline:** the same measurements as a repeatable command with
   committed, machine-local results, so a later optimization (or a new
   shader) is judged against numbers, not impressions.

## 2. What exists and what is missing

Today (`fixtures/bench/`): divan medians gated at +10 % in `cargo xtask
full` (a 3000-projectile tick 227 µs, 32 worms 7 µs, 8 bots 22 µs, an
offscreen frame 4.9 ms at 1080p), and a 20 s stress run of 32 chaos bots on
Castle Strike (MW 1.0 with a 20 000 pool: 11 000 live projectiles, tick
p50 0.8 ms, p99 2.1 ms; memory about 200 MiB).

Missing:

- every render number comes from llvmpipe (the CPU); nothing from the
  user's RTX 5070 Ti, under WSL or natively;
- one map (1350 × 550); no large maps, no art scale above the packs';
- the effects cap (`olr_fx::MAX_ENTITIES`, 1024) is hit in every stress run,
  so its true cost is unknown;
- no breaking points: every run is a single load;
- no OLX numbers at all.

## 3. Budgets

A point "holds" when, over its run:

- **Tick:** the simulation tick (bots' thinking included) p99 ≤ 10 ms,
  OLX's fixed 100 Hz frame (`FRAME_MS`). The report also gives the share of
  the budget used, since networking will need part of it (rollback or
  re-simulation costs several ticks per frame).
- **Frame:** the whole frame (CPU work and GPU time) p99 ≤ 6.9 ms (144 Hz)
  and ≤ 16.7 ms (60 Hz); both reported.
- **Memory:** peak resident below 2 GiB (reported, not a hard stop below
  the existing `MEMORY_CEILING_MIB`).

The **breaking point** of an axis is its first value that does not hold,
together with the cause (tick, frame, memory, or a cap reached).

## 4. The sweeps (headless, CPU)

One reference configuration: Classic on Castle Strike, 16 chaos bots of
mixed difficulty, OLX's pool, 20 s of game time, seed 1. Each sweep varies
one axis from it; each point runs 3 times and keeps the medians.

| Axis | Values | Notes |
|---|---|---|
| Worms | 2, 4, 8, 16, 24, 32 | 32 is OLX's `MAX_WORMS` and ours. Raising it is a separate decision; the report gives the cost per worm and a projection past 32. |
| Projectile pool | 3000, 10 000, 20 000, 50 000, 100 000 | MW 1.0 (the heavy mod); `MatchOptions::pool_capacity` exists. |
| Effects cap | 1024, 4096, 16 384, 65 536 | Made a bench-only option (as the pool), default unchanged. Steps `olr_fx` and builds the draw list. |
| Map size | 1024², 2048², 4096 × 2048, 4096² | Generated in memory (noise caves, seeded), no pack written. 4096 is OLX's largest side. Bots' route cache (`AreaCache`) build and update cost, carving, the colour terrain. |
| Art scale | 1, 2, 4, 8 at 2048² | The colour map and its dirty-rectangle uploads; memory. 8 is `MAX_ART_SCALE`. |

Measured per point: tick p50/p99/max split into simulation and bots'
thinking, entities and events, the draw-list build, the colour terrain
edits, memory peak, and any cap reached. This extends `olr-cli stress`
(new options) rather than adding a second runner.

## 5. The GPU (the user's machine)

- **Real GPU time:** `olr-render` measures each pass with wgpu timestamp
  queries when the adapter offers them (`Features::TIMESTAMP_QUERY`), else
  reports CPU-side frame time only, saying so. WSL's GL path is expected to
  lack them.
- **Scenes:** a heavy recorded match (the stress run's replay, MW 1.0, many
  projectiles and effects), drawn windowed and uncapped at 1920 × 1080,
  2560 × 1440, 3440 × 1440 and 3840 × 2160 (offscreen at sizes the monitor
  lacks), and the map and art-scale sweep's largest levels.
- **Two paths:**
  1. WSL, as the game runs today (wgpu GL over Mesa's d3d12 driver).
  2. A native Windows build (cross-compiled from WSL with
     `x86_64-pc-windows-gnu`, run from WSL through Windows interop), on
     DX12 and Vulkan. It needs `sudo apt install mingw-w64` and `rustup
     target add x86_64-pc-windows-gnu`. This shows what WSL costs and what
     a Windows player gets.
- **Shader headroom probe:** a bench-only post pass that runs a tunable
  full-screen fragment shader (a blur of k taps) K times after the frame.
  The report gives the GPU milliseconds a full-screen pass costs at each
  resolution and how many such passes fit in the 144 Hz and 60 Hz budgets:
  a concrete answer to "how many shaders can we add".

## 6. OLX comparison

- **Physics:** the patched OLX of the oracle (`tools/olx-oracle`, Docker)
  gets a timing hook that writes each simulated frame's physics time to a
  separate file (the traces stay byte-identical). Scenarios: the existing
  oracle scenarios plus new heavy ones (worms firing to fill OLX's 3000
  projectiles). `olr-cli scenario` times the same scenarios in `olr-sim`,
  which reproduces them bit-exactly, so the work is identical. Same machine,
  the Docker overhead noted.
- **Rendering:** OLX draws on the CPU at 640 × 480 (SDL, 2×); its frame time
  is recorded if the patch can do so cheaply, labelled as not comparable
  like for like.
- **Limits:** a table of OLX's hard limits (worms, projectiles, map side,
  effects) next to ours.

## 7. The command, the results and the report

- `cargo xtask envelope [--quick] [--only <axis>] [--gpu wsl|windows|both]`
  runs the sweeps (about 15-25 min in full; `--quick` one run per point and
  fewer points, a few minutes) and prints a table with the change against
  the committed results for this host.
- `cargo xtask envelope --bless --reason "<why>"` writes
  `fixtures/bench/envelope/<host>.ron` (the host fingerprint as
  `baselines.ron` has it; another machine's file is only a notice) and logs
  the reason in `fixtures/blessings.log`.
- Not part of `cargo xtask full` (too long); the existing gated benches stay
  the per-change gate. A regression beyond each axis's noise (measured at
  bless time from the 3 runs) is flagged, not failed.
- **The report:** `docs/perf/envelope.md`: method, the machine, every
  sweep's table and breaking point, the GPU and shader-headroom tables for
  both paths, the OLX comparison, and a short "headroom" summary per goal
  the user named (shaders, larger maps, more detailed art, more players).
  It is regenerated from the results file, with the summary written by
  hand.

## 8. Constraints

- No change to the simulation's behaviour: oracle traces, match goldens and
  render goldens stay byte-identical. New knobs (effects cap, generated
  levels, the probe pass) are bench-only options with today's defaults.
- The simulation stays `no_std` and WASM-clean; timing lives in `olr-cli`,
  `olr-render` (behind an option) and `xtask`.
- Measurements on the user's machine need it otherwise idle; the command
  says so and records the load average before each point.
- WSL and Windows interop: the Windows build and its runs need nothing
  installed on Windows.

## 9. Out of scope

- Optimizations themselves: the milestone only finds the limits and
  records the baseline. Cheap fixes it uncovers are listed for a later plan,
  unless one blocks a sweep (then fixed and noted).
- Raising `MAX_WORMS`, network costs (no network code yet), the browser
  build.
