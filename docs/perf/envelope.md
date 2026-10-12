# Performance envelope

Made by `cargo xtask envelope --report` from the results measured on 2026-10-12 (3 runs per point). Everything but the summary is regenerated; edit only the summary.

## Headroom summary

<!-- summary:start -->
**Budget left today.** At the reference configuration with today's settings (16 bots, Classic on Castle Strike, OLX's 3000-projectile pool), the simulation tick's p99 is 0.15 ms: 1 % of the 10 ms budget (worms sweep, 16). The heaviest scene drawn, the heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool), costs 2.95 ms at p99 for a tick and a frame at 2560 × 1440 on WSL and 2.29 ms on Windows DX12 (2.23 ms on Vulkan). That is well inside both frame budgets, 6.9 ms (144 Hz) and 16.7 ms (60 Hz), and leaves about 4–4.7 ms of the 144 Hz frame depending on the path (3.99 ms on WSL, 4.65 ms on DX12, 4.71 ms on Vulkan). In a real window on Windows, with the HUD, the game runs uncapped at 1234–1462 FPS on DX12 (frame p99 2.22–2.46 ms) and 1972–2159 FPS on Vulkan (1.73–1.89 ms) at every size up to 3840 × 2160. Under WSL the window is presented through WSLg at the compositor's rate (Fifo), so its 38–124 FPS are WSLg's limit, not the game's cost. For networking, which will re-simulate several ticks within one frame, the frame is the limit that counts. Within what a 144 Hz frame leaves after today's heavy scene, about 26 (WSL) to 31 (DX12, Vulkan) re-simulated reference ticks (0.15 ms p99 each) fit, but only about 3 at the heaviest sweep point's tick (1.20 ms p99, a 4096 × 4096 map); at 60 Hz (13.72 ms left on WSL, 14.38 ms on DX12, 14.44 ms on Vulkan) about 91–96 reference ticks fit, or 11–12 at the 4096 × 4096 point. Per tick alone, the 10 ms tick budget would hold about 66 reference ticks or 8 at the 4096 × 4096 point, but re-simulated ticks share the frame with drawing, so the per-frame counts are the ones to plan with. What this means: today the game uses a few per cent of its tick budget and under half of a 144 Hz frame, so nothing has to be optimised before adding features; rollback networking on very large maps at 144 Hz is the one combination that would be tight.

**Shaders.** One full-screen blur pass costs, measured on the GPU with timestamp queries on all three paths (real GPU times, not CPU estimates): on WSL GL 0.027 ms at 1920 × 1080, 0.046 ms at 2560 × 1440 and 0.102 ms at 3840 × 2160; on Windows DX12 0.034, 0.060 and 0.133 ms; on Windows Vulkan 0.028, 0.048 and 0.107 ms. Beside today's frame that leaves room, depending on the path, for 203–260 such passes at 1080p and 144 Hz (488–625 at 60 Hz), 115–150 at 1440p and 144 Hz (278–361 at 60 Hz), and 51–67 at 4K and 144 Hz (124–162 at 60 Hz). The game's own GPU work is tiny: the heavy replay's GPU p99 is 0.02–0.06 ms at every size. These counts are GPU-side headroom: the tick and the draw list run on the CPU beside them and share the frame (see the budget above). What this means: a bloom, colour grading and a CRT look together, say 10 full-screen passes (about 1.3 ms at 4K on DX12), fit at 4K and 144 Hz many times over; shaders are not where the game will run out.

**Larger maps.** The map sweep holds at every size up to OLX's largest, 4096 × 4096: no breaking point at 144 Hz or at 60 Hz, and the bots' area cache never filled. At 4096 × 4096 the tick p99 is 1.20 ms (12 % of the budget), memory peaks at 411 MiB, and the level drawn at 2560 × 1440 costs 0.87 ms tick + frame p99 on WSL (0.32 ms on DX12, 0.27 ms on Vulkan). What grows with the map is the bots' route finding: their p99 goes from 0.19 ms (1024 × 1024) to 0.31, 0.66 and 1.19 ms (4096 × 4096), nearly the whole tick; memory grows with the area, 35, 111, 211 and 411 MiB. Drawing does not grow, since the screen shows the same area of the world on any map. On the largest map the Deathmatch bots meet less (52 projectiles at the peak against 71–75 on the smaller ones), so a busy large map adds the projectile cost of the next part on top. The 4096 side is a limit of the level format (OLX's 12-bit worm positions), not of speed. What this means: maps as large as OLX allows are affordable today at about an eighth of the tick budget; past that, the bots' route finding is what to watch, and a side beyond 4096 needs a format change rather than a faster machine.

**More detailed art.** At 2048 × 2048, memory peaks at 111 MiB (art scale 1), 351 MiB (2), 1311 MiB (4) and 5151 MiB (8), against the 2048 MiB budget. Art scale 8 is the breaking point for two reasons: memory (5151 MiB) and the content cap, since its colour image is 1024 MiB, four times `MAX_COLOR_MAP_BYTES` (256 MiB), so the game would refuse such a level today (the sweep generates it in memory to measure it anyway). Drawing is not the problem: tick + frame p99 at 2560 × 1440 is 1.01, 1.04, 1.28 and 2.00 ms on WSL and 0.38–0.45 ms on DX12, the GPU p99 at most 0.06 ms; the tick does not change (0.30–0.32 ms p99). Raising the cap needs memory work first: the peak is about five times the colour image, likely because the full-size art is held several times over (an inference from the code, not measured: the colour map's picture, its front and back art, and the loaded level's own copy); keeping fewer copies, or tiling the art, would bring art scale 8 towards the budget. What this means: art four times as detailed as the packs' fits today in about 1.3 GiB; eight times needs a memory diet before the cap can be lifted, not a faster GPU.

**More players.** Each worm adds about 6 µs of tick at p50 and 15 µs at p99 (the worms sweep, chaos bots on Castle Strike). At 32 worms, OLX's and our `MAX_WORMS`, the tick p99 is 0.42 ms, 4 % of the budget. At that rate the tick p99 would reach 10 ms at about 686 worms (the least-squares line over the sweep's p99s: 14.62 µs per worm, intercept −38.7 µs, so (10 000 + 38.7) / 14.62 ≈ 686): a projection only, `MAX_WORMS` stays 32. More players fire more. The projectile-pool sweep (MW 1.0, 16 bots) holds at every pool size up to 65 536, its tick p99 0.57–0.69 ms, but its bots never had more than 4770 projectiles alive at once, so the larger pools were not filled; OLX's full 3000-projectile pool (heavy-spikeballs) costs rl-sim 153.8 µs at p50 and 497.8 µs at p99 per tick. The effects sweep holds at every cap up to 65 536: with the cap lifted the game reaches 9892 live effects, the presentation p99 rises from 0.02 to 0.08 ms and the draw list's p99 from 0.13 to 0.20 ms. No sweep has a breaking point. What this means: 32 players use 4 % of the tick, and neither more projectiles nor more effects came near the budget; the simulation is not what would stop a bigger game.

**Against OLX.** On the same scenarios rl-sim's tick takes 0.17 to 0.52 of OLX's frame time at p50, two to six times faster. On heavy-spikeballs, which fills OLX's 3000-projectile pool, OLX takes 442.3 / 1439.3 µs (p50 / p99) and rl-sim 153.8 / 497.8 µs (ratio 0.35); heavy-napalm is a mid-load point at 829 projectiles (ratio 0.30). The work was the same: in all eight scenarios both had the same projectiles and `rand()` calls every frame, and the six with a full trace match OLX. OLX is a Release build at -O3 running in Docker on WSL's own kernel (no extra virtualisation, the same clock); its timed span leaves out the shot-list delivery and message parsing that rl-sim does inside its tick, so the comparison slightly flatters OLX. OLX's rendering is not timed. What this means: the port does exactly OLX's work faster than OLX does, so being faithful costs no speed.

**Cheap fixes noticed (for a later plan).**

- The effects cap (1024, OLX's `MAX_ENTITIES`) is full for 1250 ticks of the reference run and 1866 at 32 worms; lifting it to 16 384 costs 0.06 ms of presentation p99 and 0.06 ms of draw-list p99 (fx and worms tables). Cheap, but it shows more debris than OLX does: a look decision, not only a speed one.
- The bots' route finding is nearly the whole tick on large maps (bots p99 1.19 of the tick's 1.20 ms at 4096 × 4096; map table): the first place to optimise if maps or bot counts grow.
- Art scale 8 peaks at 5151 MiB for a 1024 MiB colour image (art table): holding fewer full-size copies of the art is the precondition for raising `MAX_COLOR_MAP_BYTES`.
- The pool sweep's bots never pass 4770 live projectiles (pool table), so pools above it are not exercised; a pool point with 32 bots, as `cargo xtask stress` plays, would measure them.
- The report's least-squares line on the flat sweeps ("Tick p99 grows -0.4356 µs per unit of art"; art, fx and pool tables) is noise; the report could leave it out when the tick does not grow.
<!-- summary:end -->

## Method

- Budgets: tick p99 ≤ 10 ms (OLX's 100 Hz frame), frame p99 ≤ 6.9 ms (144 Hz) and ≤ 16.7 ms (60 Hz), peak memory below 2048 MiB. A point holds when all of them do; the breaking point is the first value that does not, with the cause (tick, frame, memory, or a cap).
- Sweeps: `rl-cli stress`, one process a point, medians of the runs; the reference is Classic on Castle Strike, 16 bots of mixed difficulty (chaos bots; Deathmatch bots on the map and art sweeps, so route finding counts), OLX's pool, 20 s, seed 1, `LoadingTime` 0, bonuses every 5 s. The pool sweep plays MW 1.0. Map and art levels are generated in memory (`gen-<w>x<h>-a<scale>-s1`).
- Frames: `rl-cli gpu-bench` offscreen on the hardware adapter (timestamps when the adapter has them), and `rl-game --frame-log --uncapped` windowed; on WSL (GL over Mesa's d3d12, presented through WSLg) and native Windows (DX12, Vulkan) built with mingw-w64 and run through interop.
- OLX: the oracle's patched OLX (Docker; a Release build, -O3) timing each simulated frame to a file of its own; rl-sim timed on the same scenarios, the same work checked frame by frame.

## The machine

- CPU: AMD Ryzen 5 7600X 6-Core Processor (12 logical CPUs)
- windows-dx12: NVIDIA GeForce RTX 5070 Ti (Dx12, DiscreteGpu)
- windows-vulkan: NVIDIA GeForce RTX 5070 Ti (Vulkan, DiscreteGpu)
- wsl-gl: D3D12 (NVIDIA GeForce RTX 5070 Ti) (Gl, Other)

## Sweeps

### art

art varies; otherwise 16 Deathmatch bots (OLX's bot: routes through the area cache) of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-classic on gen-2048x2048-a1-s1).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| a1 | 0.06 / 0.32 | 3 % | 0.02 | 0.32 | 0.02 | 0.02 | 75 | 1024 | 111 | 1.01 | yes (effects full (1024) for 33 ticks) |
| a2 | 0.06 / 0.31 | 3 % | 0.02 | 0.30 | 0.02 | 0.02 | 75 | 1024 | 351 | 1.04 | yes (effects full (1024) for 33 ticks) |
| a4 | 0.06 / 0.30 | 3 % | 0.02 | 0.30 | 0.02 | 0.02 | 75 | 1024 | 1311 | 1.28 | yes (effects full (1024) for 33 ticks) |
| a8 | 0.06 / 0.31 | 3 % | 0.02 | 0.31 | 0.04 | 0.02 | 75 | 1024 | 5151 | 2.00 | memory 5151 MiB > 2048 MiB; cap: the content loader refuses a 2048x2048 level at art scale 8: a 1024 MiB colour image, above MAX_COLOR_MAP_BYTES (256 MiB) (effects full (1024) for 33 ticks) |

Frame: the point's level drawn offscreen at 2560×1440 on wsl-gl (a tick and a frame, `rl-cli gpu-bench`).

Breaking point (144 Hz): **a8** (memory 5151 MiB > 2048 MiB; cap: the content loader refuses a 2048x2048 level at art scale 8: a 1024 MiB colour image, above MAX_COLOR_MAP_BYTES (256 MiB)). Breaking point (60 Hz): **a8** (memory 5151 MiB > 2048 MiB; cap: the content loader refuses a 2048x2048 level at art scale 8: a 1024 MiB colour image, above MAX_COLOR_MAP_BYTES (256 MiB)).

Tick p99 grows -0.4356 µs per unit of art (least squares).

### fx

fx varies; otherwise 16 chaos bots of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-classic on olx-level-castlestrike).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1024 | 0.06 / 0.14 | 1 % | 0.14 | 0.00 | 0.02 | 0.13 | 993 | 1024 | 28 | — | yes (effects full (1024) for 1250 ticks) |
| 4096 | 0.06 / 0.14 | 1 % | 0.14 | 0.01 | 0.05 | 0.15 | 993 | 4096 | 28 | — | yes (effects full (4096) for 439 ticks) |
| 16384 | 0.06 / 0.14 | 1 % | 0.14 | 0.01 | 0.08 | 0.19 | 993 | 9892 | 29 | — | yes |
| 65536 | 0.06 / 0.14 | 1 % | 0.14 | 0.01 | 0.07 | 0.20 | 993 | 9892 | 32 | — | yes |

Breaking point (144 Hz): none: every value holds. Breaking point (60 Hz): none: every value holds.

Tick p99 grows -0.0000 µs per unit of fx (least squares).

### map

map varies; otherwise 16 Deathmatch bots (OLX's bot: routes through the area cache) of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-classic on gen-1024x1024-a1-s1).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1024x1024 | 0.03 / 0.20 | 2 % | 0.01 | 0.19 | 0.01 | 0.02 | 71 | 1024 | 35 | 0.91 | yes (effects full (1024) for 17 ticks) |
| 2048x2048 | 0.06 / 0.31 | 3 % | 0.02 | 0.31 | 0.02 | 0.02 | 75 | 1024 | 111 | 0.97 | yes (effects full (1024) for 33 ticks) |
| 4096x2048 | 0.07 / 0.66 | 7 % | 0.02 | 0.66 | 0.01 | 0.01 | 75 | 1024 | 211 | 0.82 | yes (effects full (1024) for 15 ticks) |
| 4096x4096 | 0.40 / 1.20 | 12 % | 0.01 | 1.19 | 0.02 | 0.01 | 52 | 856 | 411 | 0.87 | yes |

Frame: the point's level drawn offscreen at 2560×1440 on wsl-gl (a tick and a frame, `rl-cli gpu-bench`).

Breaking point (144 Hz): none: every value holds. Breaking point (60 Hz): none: every value holds.

### pool

pool varies; otherwise 16 chaos bots of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-mw-1-0 on olx-level-castlestrike).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3000 | 0.24 / 0.57 | 6 % | 0.56 | 0.01 | 0.03 | 0.07 | 3000 | 1024 | 42 | — | yes (pool full (3000) for 38 ticks; effects full (1024) for 1757 ticks) |
| 10000 | 0.28 / 0.68 | 7 % | 0.68 | 0.01 | 0.03 | 0.08 | 4770 | 1024 | 43 | — | yes (effects full (1024) for 1855 ticks) |
| 20000 | 0.28 / 0.68 | 7 % | 0.68 | 0.01 | 0.04 | 0.08 | 4770 | 1024 | 45 | — | yes (effects full (1024) for 1855 ticks) |
| 50000 | 0.28 / 0.69 | 7 % | 0.68 | 0.01 | 0.03 | 0.08 | 4770 | 1024 | 49 | — | yes (effects full (1024) for 1855 ticks) |
| 65536 | 0.28 / 0.67 | 7 % | 0.67 | 0.01 | 0.04 | 0.08 | 4770 | 1024 | 52 | — | yes (effects full (1024) for 1855 ticks) |

Breaking point (144 Hz): none: every value holds. Breaking point (60 Hz): none: every value holds.

Tick p99 grows 0.0009 µs per unit of pool (least squares).

### worms

worms varies; otherwise 16 chaos bots of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-classic on olx-level-castlestrike).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 2 | 0.01 / 0.02 | 0 % | 0.02 | 0.00 | 0.01 | 0.01 | 217 | 1024 | 27 | — | yes (effects full (1024) for 60 ticks) |
| 4 | 0.01 / 0.03 | 0 % | 0.03 | 0.00 | 0.01 | 0.01 | 315 | 1024 | 27 | — | yes (effects full (1024) for 48 ticks) |
| 8 | 0.02 / 0.05 | 1 % | 0.05 | 0.00 | 0.02 | 0.04 | 550 | 1024 | 27 | — | yes (effects full (1024) for 424 ticks) |
| 16 | 0.06 / 0.15 | 1 % | 0.14 | 0.01 | 0.02 | 0.13 | 993 | 1024 | 28 | — | yes (effects full (1024) for 1250 ticks) |
| 24 | 0.13 / 0.36 | 4 % | 0.36 | 0.01 | 0.03 | 0.05 | 2272 | 1024 | 29 | — | yes (effects full (1024) for 1792 ticks) |
| 32 | 0.17 / 0.42 | 4 % | 0.42 | 0.01 | 0.03 | 0.05 | 2338 | 1024 | 30 | — | yes (effects full (1024) for 1866 ticks) |

Breaking point (144 Hz): none: every value holds. Breaking point (60 Hz): none: every value holds.

Cost per worm: 6 µs of tick at p50, 15 µs at p99. At this rate the tick p99 reaches 10 ms at about 686 worms (a projection: `MAX_WORMS` stays 32).

## GPU

### windows-dx12: NVIDIA GeForce RTX 5070 Ti (Dx12, DiscreteGpu)

GPU times: timestamp queries.

The heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool) offscreen, a tick and a frame each:

| size | tick + frame p50 / p99 (ms) | frame p99, CPU then GPU (ms) | GPU p99 (ms) |
|---|---|---|---|
| 1920×1080 | 1.18 / 2.39 | 0.75 | 0.02 |
| 2560×1440 | 1.18 / 2.29 | 0.71 | 0.02 |
| 3440×1440 | 1.18 / 2.32 | 0.81 | 0.03 |
| 3840×2160 | 1.20 / 2.36 | 0.76 | 0.04 |

Shader headroom, GPU side (a full-screen blur pass, `rl_render::Probe`): the GPU time one pass costs, and how many passes fit beside the frame's GPU p99 (its frame p99 without timestamps) within the 6.9 ms (144 Hz) and 16.7 ms (60 Hz) budgets. This is GPU-side headroom, not the whole frame: the tick and the draw list run on the CPU beside it.

| size | GPU ms per pass | passes that fit at 144 Hz | at 60 Hz |
|---|---|---|---|
| 1920×1080 | 0.034 | 203 | 488 |
| 2560×1440 | 0.060 | 115 | 278 |
| 3440×1440 | 0.080 | 86 | 207 |
| 3840×2160 | 0.133 | 51 | 124 |

Windowed, uncapped (`rl-game --frame-log --uncapped`, HUD included). Busy is a frame's CPU time without waiting to present.

| asked | got | present mode | frames | FPS | frame p50 / p99 (ms) | busy p99 (ms) |
|---|---|---|---|---|---|---|
| 1920×1080 | 1920×1080 | Immediate | 15362 | 1280 | 0.63 / 2.46 | 2.23 |
| 2560×1440 | 2560×1440 | Immediate | 15441 | 1287 | 0.62 / 2.35 | 2.17 |
| 3440×1440 | 3440×1440 | Immediate | 14811 | 1234 | 0.64 / 2.37 | 2.18 |
| 3840×2160 | 3840×2160 | Immediate | 17549 | 1462 | 0.58 / 2.22 | 2.03 |

The sweeps' levels drawn offscreen:

| axis | value | size | tick + frame p99 (ms) | GPU p99 (ms) |
|---|---|---|---|---|
| map | 1024x1024 | 2560×1440 | 0.37 | 0.02 |
| map | 2048x2048 | 2560×1440 | 0.45 | 0.02 |
| map | 4096x2048 | 2560×1440 | 0.34 | 0.02 |
| map | 4096x4096 | 2560×1440 | 0.32 | 0.02 |
| art | a1 | 2560×1440 | 0.40 | 0.02 |
| art | a2 | 2560×1440 | 0.38 | 0.02 |
| art | a4 | 2560×1440 | 0.41 | 0.02 |
| art | a8 | 2560×1440 | 0.45 | 0.04 |

### windows-vulkan: NVIDIA GeForce RTX 5070 Ti (Vulkan, DiscreteGpu)

GPU times: timestamp queries.

The heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool) offscreen, a tick and a frame each:

| size | tick + frame p50 / p99 (ms) | frame p99, CPU then GPU (ms) | GPU p99 (ms) |
|---|---|---|---|
| 1920×1080 | 1.09 / 2.19 | 0.52 | 0.02 |
| 2560×1440 | 1.09 / 2.23 | 0.48 | 0.02 |
| 3440×1440 | 1.10 / 2.23 | 0.53 | 0.03 |
| 3840×2160 | 1.11 / 2.20 | 0.53 | 0.04 |

Shader headroom, GPU side (a full-screen blur pass, `rl_render::Probe`): the GPU time one pass costs, and how many passes fit beside the frame's GPU p99 (its frame p99 without timestamps) within the 6.9 ms (144 Hz) and 16.7 ms (60 Hz) budgets. This is GPU-side headroom, not the whole frame: the tick and the draw list run on the CPU beside it.

| size | GPU ms per pass | passes that fit at 144 Hz | at 60 Hz |
|---|---|---|---|
| 1920×1080 | 0.028 | 248 | 597 |
| 2560×1440 | 0.048 | 143 | 344 |
| 3440×1440 | 0.064 | 107 | 258 |
| 3840×2160 | 0.107 | 64 | 155 |

Windowed, uncapped (`rl-game --frame-log --uncapped`, HUD included). Busy is a frame's CPU time without waiting to present.

| asked | got | present mode | frames | FPS | frame p50 / p99 (ms) | busy p99 (ms) |
|---|---|---|---|---|---|---|
| 1920×1080 | 1920×1080 | Immediate | 25909 | 2159 | 0.39 / 1.73 | 1.59 |
| 2560×1440 | 2560×1440 | Immediate | 25587 | 2132 | 0.39 / 1.73 | 1.58 |
| 3440×1440 | 3440×1440 | Immediate | 23663 | 1972 | 0.42 / 1.89 | 1.73 |
| 3840×2160 | 3840×2160 | Immediate | 24677 | 2056 | 0.40 / 1.80 | 1.63 |

The sweeps' levels drawn offscreen:

| axis | value | size | tick + frame p99 (ms) | GPU p99 (ms) |
|---|---|---|---|---|
| map | 1024x1024 | 2560×1440 | 0.25 | 0.02 |
| map | 2048x2048 | 2560×1440 | 0.28 | 0.02 |
| map | 4096x2048 | 2560×1440 | 0.21 | 0.02 |
| map | 4096x4096 | 2560×1440 | 0.27 | 0.02 |
| art | a1 | 2560×1440 | 0.26 | 0.02 |
| art | a2 | 2560×1440 | 0.30 | 0.02 |
| art | a4 | 2560×1440 | 0.26 | 0.02 |
| art | a8 | 2560×1440 | 0.31 | 0.04 |

### wsl-gl: D3D12 (NVIDIA GeForce RTX 5070 Ti) (Gl, Other)

GPU times: timestamp queries.

The heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool) offscreen, a tick and a frame each:

| size | tick + frame p50 / p99 (ms) | frame p99, CPU then GPU (ms) | GPU p99 (ms) |
|---|---|---|---|
| 1920×1080 | 1.68 / 3.13 | 1.92 | 0.03 |
| 2560×1440 | 1.67 / 2.95 | 1.55 | 0.03 |
| 3440×1440 | 1.67 / 3.16 | 1.58 | 0.04 |
| 3840×2160 | 1.63 / 2.92 | 1.70 | 0.06 |

Shader headroom, GPU side (a full-screen blur pass, `rl_render::Probe`): the GPU time one pass costs, and how many passes fit beside the frame's GPU p99 (its frame p99 without timestamps) within the 6.9 ms (144 Hz) and 16.7 ms (60 Hz) budgets. This is GPU-side headroom, not the whole frame: the tick and the draw list run on the CPU beside it.

| size | GPU ms per pass | passes that fit at 144 Hz | at 60 Hz |
|---|---|---|---|
| 1920×1080 | 0.027 | 260 | 625 |
| 2560×1440 | 0.046 | 150 | 361 |
| 3440×1440 | 0.062 | 111 | 269 |
| 3840×2160 | 0.102 | 67 | 162 |

Windowed (`rl-game --frame-log --uncapped`, HUD included), presented through WSLg: WSLg presents at the compositor's rate (Fifo), not uncapped, so these frame rates are WSLg's present cap, not the game's cost. This path's cost is the offscreen table above; the native Windows paths' windowed runs are the real windowed numbers. Busy is a frame's CPU time without waiting to present.

| asked | got | present mode | frames | FPS | frame p50 / p99 (ms) | busy p99 (ms) |
|---|---|---|---|---|---|---|
| 1920×1080 | 1920×1080 | Fifo | 1490 | 124 | 7.86 / 11.49 | 8.68 |
| 2560×1440 | 2560×1440 | Fifo | 914 | 76 | 12.78 / 18.59 | 13.39 |
| 3440×1440 | 3440×1440 | Fifo | 702 | 58 | 16.88 / 20.42 | 15.69 |
| 3840×2160 | 3840×2160 | Fifo | 454 | 38 | 26.85 / 32.82 | 26.09 |

The sweeps' levels drawn offscreen:

| axis | value | size | tick + frame p99 (ms) | GPU p99 (ms) |
|---|---|---|---|---|
| map | 1024x1024 | 2560×1440 | 0.91 | 0.03 |
| map | 2048x2048 | 2560×1440 | 0.97 | 0.03 |
| map | 4096x2048 | 2560×1440 | 0.82 | 0.03 |
| map | 4096x4096 | 2560×1440 | 0.87 | 0.03 |
| art | a1 | 2560×1440 | 1.01 | 0.03 |
| art | a2 | 2560×1440 | 1.04 | 0.03 |
| art | a4 | 2560×1440 | 1.28 | 0.04 |
| art | a8 | 2560×1440 | 2.00 | 0.06 |

## OLX

Image `rl-olx-oracle:1958255f9797`, built Release, -O3 (CMakeOlxCommon.cmake:43-47, :369); CMAKE_CXX_FLAGS_RELEASE:STRING=-O3 -DNDEBUG; c++ (Ubuntu 11.4.0-1ubuntu1~22.04.3) 11.4.0. OLX runs in a Docker container on WSL's own kernel (no extra virtualisation, the same clock). Its timed span is the gus logic and the client's and the server's frame, without the trace writing; shot-list delivery and message parsing run in OLX's `betweenFrames()`, outside that span, while rl-sim does the equivalent inside `World::tick`, so the comparison slightly flatters OLX. rl-sim's span (`rl-cli scenario --time`) also holds a scenario's bonus placement (none in these scenarios). OLX draws on the CPU at 640 × 480 and is not timed here (not comparable like for like).

Load: heavy-spikeballs fills OLX's 3000-projectile pool (`MAX_PROJECTILES`); heavy-napalm is a mid-load point (829 projectiles at its peak). The others are the busiest oracle fixtures. Same work: the same projectiles and `rand()` calls every frame in both.

| scenario | frames | peak projectiles | OLX p50 / p99 (µs) | rl-sim p50 / p99 (µs) | rl-sim / OLX (p50) | same work | matches OLX |
|---|---|---|---|---|---|---|---|
| two-worms-fire | 200 | 114 | 10.3 / 66.2 | 4.0 / 13.4 | 0.39 | yes | yes |
| multi-shot | 150 | 114 | 5.4 / 22.6 | 2.8 / 9.8 | 0.52 | yes | yes |
| suicide-score | 660 | 23 | 3.3 / 28.8 | 0.6 / 6.1 | 0.17 | yes | yes |
| fire-minigun | 200 | 25 | 2.5 / 13.5 | 1.1 / 6.7 | 0.45 | yes | yes |
| weapons/classic/spikeballs | 368 | 23 | 3.7 / 14.3 | 1.3 / 5.4 | 0.34 | yes | yes |
| weapons/classic/uzi | 368 | 16 | 3.8 / 21.9 | 0.9 / 3.7 | 0.23 | yes | yes |
| heavy-spikeballs | 800 | 3000 | 442.3 / 1439.3 | 153.8 / 497.8 | 0.35 | yes | — (timing trace) |
| heavy-napalm | 800 | 829 | 162.2 / 302.6 | 48.6 / 85.4 | 0.30 | yes | — (timing trace) |

### Limits

| | OLX (`d1ad953f`) | rust-liero |
|---|---|---|
| Worms | 32 (`MAX_WORMS`, `include/Consts.h:34`) | 32 (`rl_sim::MAX_WORMS`) |
| Projectiles | 3000 (`MAX_PROJECTILES`, `include/CProjectile.h:36`) | 3000 by default, up to 65 536 (`MatchSetup::MAX_POOL_CAPACITY`) |
| Map side | 4096 (12-bit worm positions) | 4096 for imported levels (`rl-import-lxl`) |
| Effects | 1024 (`MAX_ENTITIES`, `include/Entity.h:30`) | 1024 (`rl_fx::MAX_ENTITIES`; `Fx::with_capacity` in benches) |
| Colour image | — | 256 MiB (`MAX_COLOR_MAP_BYTES`), art scale at most 8 |
