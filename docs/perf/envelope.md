# Performance envelope

Made by `cargo xtask envelope --report` from the results measured on 2026-10-13 (3 runs per point). Everything but the summary is regenerated; edit only the summary.

## Headroom summary

<!-- summary:start -->
**Budget left today.** At the reference configuration with today's settings (16 bots, Classic on Castle Strike, OLX's 3000-projectile pool), the simulation tick's p99 is 0.13 ms: 1 % of the 10 ms budget (worms sweep, 16). The sweeps other than the effects sweep still run with their effects capped at 1024 (their table headers), so that figure and the other sweep rows do not carry the larger effects pool; the effects sweep and the GPU tables do. The heaviest scene drawn, the heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool), costs 4.52 ms at p99 for a tick and a frame at 2560 × 1440 offscreen on WSL, 3.42 ms on Windows DX12 and 2.92 ms on Vulkan (2.95, 2.34 and 2.18 ms before the effects pool went to 16 384). That is still inside both frame budgets, 6.9 ms (144 Hz) and 16.7 ms (60 Hz), and leaves about 2.4–4.0 ms of the 144 Hz frame depending on the path (2.42 ms on WSL, 3.52 ms on DX12, 4.02 ms on Vulkan). In a real window on Windows, with the HUD, the game runs uncapped at 992–1062 FPS on DX12 (frame p99 3.36–3.68 ms) and 1419–1530 FPS on Vulkan (2.52–2.67 ms) at every size up to 3840 × 2160, holding both budgets everywhere. For networking, which will re-simulate several ticks within one frame, the frame is the limit that counts. Within what a 144 Hz frame leaves after today's heavy scene (offscreen), about 18 (WSL) to 27–30 (DX12, Vulkan) re-simulated reference ticks (0.13 ms p99 each) fit, but only about 2–3 at the heaviest sweep point's tick (1.28 ms p99, a 4096 × 4096 map); at 60 Hz (12.15 ms left on WSL, 13.25 ms on DX12, 13.75 ms on Vulkan) about 93–105 reference ticks fit, or 9–10 at the 4096 × 4096 point. Per tick alone, the 10 ms tick budget would hold about 76 reference ticks or 7 at the 4096 × 4096 point, but re-simulated ticks share the frame with drawing, so the per-frame counts are the ones to plan with. What this means: today the game uses about 1 % of its tick budget and, in the heaviest offscreen scene, 42–65 % of a 144 Hz frame (65 % on WSL, 49 % on DX12, 42 % on Vulkan), so nothing has to be optimised before adding features; rollback networking on very large maps at 144 Hz, most of all on WSL, is the one combination that would be tight.

Rollback: restoring and re-simulating 12 ticks costs 1.26 ms p99 at the reference configuration and 0.72 ms at 4096 × 4096 (budgets 2 and 4 ms).

**Playing under WSL.** In a window under WSL the picture is different, and it is not the game's drawing. WSL presented every run with Fifo, and its presentation costs CPU time per pixel: a frame's busy time (without waiting to present) grows from 7.2 ms p50 at 1920 × 1080 to 11.6 ms at 2560 × 1440, 15.2 ms at 3440 × 1440 and 25.6 ms at 3840 × 2160, about 3.0 ms per million pixels, while the same scene's offscreen frame stays at 1.90–1.95 ms p50 at every size. So WSL windowed play misses the 144 Hz budget at every size (frame p99 13.72 ms at 1920 × 1080) and the 60 Hz budget from 2560 × 1440 up (frame p99 19.75 ms at 2560 × 1440, 24.01 ms at 3440 × 1440, 38.28 ms at 3840 × 2160; 105, 66, 51 and 32 FPS). At 3440 × 1440, the user's monitor, WSL gives about 51 FPS. The native Windows build is the game's real windowed cost and holds both budgets at every size (above). What this means: at high resolutions, play the native Windows build; WSL is fine for development and for 1920 × 1080 at 60 Hz.

**Shaders.** One full-screen blur pass of the drawn frame costs, measured on the GPU with timestamp queries on all three paths (real GPU times, not CPU estimates): on WSL GL 0.027 ms at 1920 × 1080, 0.046 ms at 2560 × 1440, 0.062 ms at 3440 × 1440 and 0.102 ms at 3840 × 2160; on Windows DX12 0.034, 0.060, 0.080 and 0.133 ms; on Windows Vulkan 0.028, 0.049, 0.065 and 0.108 ms. The 16 384-effect pool did not change them (within 0.3 % of the previous measurement at every size and path, the bless's comparison). Beside today's frame that leaves room, depending on the path, for 202–259 such passes at 1080p and 144 Hz (487–624 at 60 Hz), 115–149 at 1440p and 144 Hz (278–359 at 60 Hz), 86–111 at 3440 × 1440 and 144 Hz (208–269 at 60 Hz), and 51–65 at 4K and 144 Hz (124–160 at 60 Hz). The game's own GPU work is tiny: the heavy replay's GPU p99 is 0.02–0.04 ms at every size on Windows and 0.03–0.05 ms on WSL up to 3440 × 1440 (0.20 ms at 3840 × 2160). These counts are GPU-side headroom: the tick and the draw list run on the CPU beside them and share the frame (see the budget above), and under WSL the presentation's cost comes on top. What this means: a bloom, colour grading and a CRT look together, say 10 full-screen passes (about 1.3 ms at 4K on DX12), fit at 4K and 144 Hz many times over; shaders are not where the game will run out.

**Larger maps.** The map sweep holds at every size up to OLX's largest, 4096 × 4096: no breaking point at 144 Hz or at 60 Hz, and the bots' area cache never filled. At 4096 × 4096 the tick p99 is 1.28 ms (13 % of the budget), memory peaks at 413 MiB, and the whole frame at 2560 × 1440 costs 2.12 ms p99 on WSL: the level drawn (0.85 ms tick + frame, `gpu-bench`, whose replayed tick has no route finding) plus the bots' thinking (1.27 ms p99), a sum of p99s; on DX12 the same sum is 1.59 ms (0.32 + 1.27) and on Vulkan 1.50 ms (0.23 + 1.27). What grows with the map is the bots' route finding: their p99 goes from 0.13 ms (1024 × 1024) to 0.21, 0.57 and 1.27 ms (4096 × 4096), nearly the whole tick; the frame with bots is 1.52, 1.18, 1.40 and 2.12 ms on WSL (the 1024 × 1024 point's higher figure is its drawn level, 1.39 ms, not its bots); memory grows with the area, 38, 113, 213 and 413 MiB. Drawing does not grow with the size (0.83–1.39 ms drawn on WSL, 1.39, 0.97, 0.83 and 0.85 ms from the smallest map to the largest), since the screen shows the same area of the world on any map. The bots' projectile peaks stay small on every map (76–86), so a busy large map adds the projectile cost of the next part on top. The 4096 side is a limit of the level format (OLX's 12-bit worm positions), not of speed. What this means: maps as large as OLX allows are affordable today at about an eighth of the tick budget and under a third of a 144 Hz frame; past that, the bots' route finding is what to watch, and a side beyond 4096 needs a format change rather than a faster machine.

**More detailed art.** At 2048 × 2048, memory peaks at 113 MiB (art scale 1), 353 MiB (2), 1313 MiB (4) and 5153 MiB (8), against the 2048 MiB budget. Art scale 8 is the breaking point for two reasons: memory (5153 MiB) and the content cap, since its colour image is 1024 MiB, four times `MAX_COLOR_MAP_BYTES` (256 MiB), so the game would refuse such a level today (the sweep generates it in memory to measure it anyway). Drawing is not the problem: the whole frame with the bots' thinking at 2560 × 1440 is 1.22, 1.11, 1.23 and 2.82 ms p99 on WSL (the level drawn 0.89–1.02 ms up to art scale 4 and 2.60 ms at scale 8, the bots 0.21 ms), the level drawn 0.36–0.44 ms on DX12 and 0.27–0.31 ms on Vulkan, the GPU p99 at most 0.06 ms; the tick does not change (0.21–0.22 ms p99). Raising the cap needs memory work first: the peak is about five times the colour image, likely because the full-size art is held several times over (an inference from the code, not measured: the colour map's picture, its front and back art, and the loaded level's own copy); keeping fewer copies, or tiling the art, would bring art scale 8 towards the budget. What this means: art four times as detailed as the packs' fits today in about 1.3 GiB; eight times needs a memory diet before the cap can be lifted, not a faster GPU.

**More players.** Each worm adds about 5 µs of tick at p50 and 12 µs at p99 (the worms sweep, chaos bots on Castle Strike). At 32 worms, OLX's and our `MAX_WORMS`, the tick p99 is 0.36 ms, 4 % of the budget. At that rate the tick p99 would reach 10 ms at about 870 worms (the report's least-squares line over the sweep's p99s): a projection only, `MAX_WORMS` stays 32. More players fire more. The projectile-pool sweep (MW 1.0, 16 bots) holds at every pool size up to 65 536, its tick p99 0.51–0.59 ms, but its bots never had more than 4324 projectiles alive at once, so the larger pools were not filled; OLX's full 3000-projectile pool (heavy-spikeballs) costs olr-sim 154.3 µs at p50 and 503.5 µs at p99 per tick. The effects sweep holds at every cap up to 65 536: with the cap lifted the game reaches 7572 live effects, the presentation p99 rises from 0.02 to 0.05 ms and the draw list's p99 from 0.03 to 0.08 ms. None of these three sweeps (worms, pool, effects) has a breaking point; only the art sweep breaks, at a8 (above). What this means: 32 players use 4 % of the tick, and neither more projectiles nor more effects came near the budget; the simulation is not what would stop a bigger game.

**Against OLX.** On the same scenarios olr-sim's tick takes 0.15 to 0.49 of OLX's frame time at p50, two to seven times faster. On heavy-spikeballs, which fills OLX's 3000-projectile pool, OLX takes 426.8 / 1372.9 µs (p50 / p99) and olr-sim 154.3 / 503.5 µs (ratio 0.36); heavy-napalm is a mid-load point at 829 projectiles (ratio 0.30). The work was the same: in all eight scenarios both had the same projectiles and `rand()` calls every frame, and the six with a full trace match OLX. OLX is a Release build at -O3 running in Docker on WSL's own kernel (no extra virtualisation, the same clock); its timed span leaves out the shot-list delivery and message parsing that olr-sim does inside its tick, so the comparison slightly flatters OLX. OLX's rendering is not timed. What this means: the port does exactly OLX's work faster than OLX does, so being faithful costs no speed.

**Lit levels (plan 15a, 2026-10-19).** `olr-cli gpu-bench --look` draws a replay with a representative look (`bench_look`: a dark ambient, a placed light every 96 world pixels, glowing strips in one pulsing group, soft shadows, the solid floor and soak, and the engine's lights of the replay's own explosions and shots), measured offscreen with timestamps beside the same replay unlit. On the RTX 5070 Ti the heavy replay (MW 1.0, 32 chaos bots, Castle Strike) costs 0.33–0.40 ms of GPU at p50 and about 1 ms at p99 lit, against 0.01–0.04 ms unlit, at every size from 1920 × 1080 to 3840 × 2160 (the look passes run at world resolution, so they hardly grow with the screen); the tick and frame p99 is 4.0–4.6 ms on Vulkan, 6.6–7.6 ms on DX12 and 12–21 ms on WSL GL. Most of the added frame time is CPU-side, encoding and submitting the look's passes: +1.1 ms on Vulkan, +2.3 ms on DX12, +5 ms on WSL GL at p50. A generated 2048 × 2048 level at art scale 2 costs 0.39–0.44 ms of GPU lit, a 1024 × 1024 one at art scale 8 0.47–0.60 ms (both at 2560 × 1440). Against the spec's budgets this is over on the RTX (≤ 0.25 ms at art scale 2, ≤ 0.5 ms at art scale 8) but well inside the 144 Hz frame on Vulkan and close to its limit on DX12. The machine's integrated GPU (AMD Radeon, the laptop proxy, `--adapter integrated`) misses: 10.3–12.2 ms of GPU at p50 and a frame p99 of 26–31 ms at 1920 × 1080 and 2560 × 1440 (unlit 0.5–1.2 ms, frame p99 2.5–4.6 ms), against a budget of 4 ms holding 60 Hz. There the GPU time splits (Vulkan, 1080p, 13.3 ms p50) into the whole-level glow light about 6.5 ms, the soft shadow filter about 3.6 ms and most of the p99 spikes (a busy match's many explosion lights), the 20 placed lights about 2.4 ms and the rest (engine lights, soak, blur, composite) about 4.5 ms. Two levers were measured once: a glow light of 65 536 cells (`GI_CELL_BUDGET`, from 262 144) brings it to 8.6 ms, and with hard-edged shadows for the engine's radius-1 lights to 6.7 ms (p99 10.3 ms, frame p99 13.9 ms: 60 Hz held), the RTX then 0.25 ms (p99 0.33 ms); the coarser glow light dims thin glowing strips' light, since a cell averages their emission. The user kept both as they are for now (2026-10-19): laptop performance of lit levels is later optimisation work (recomputing the glow light only when the terrain or the glow changes, the spec's other lever, is the candidate that keeps the look). What this means: lit levels are cheap on a desktop GPU, fine at 144 Hz on Vulkan, near the 144 Hz limit on DX12 because of CPU-side pass submission, and too heavy today for an integrated GPU in a busy match.

**Cheap fixes noticed (for a later plan).**

- The effects cap is lifted: play's pool is 16 384 effects, with 1024 kept for deaths (OLX's 1024 stays only for the comparison tests). It showed more debris than OLX does, a look decision the user took. What it cost: in the fx table the presentation p99 goes from 0.02 to 0.05 ms and the draw list's p99 from 0.03 to 0.08 ms at 16 384, with the tick unchanged (0.13 ms p99) and 7572 live effects at the peak, so the new cap was not reached in that run. In the GPU tables the heavy replay's tick + frame p99 at 2560 × 1440 rose by 34–53 %: 2.34 → 3.42 ms on DX12, 2.18 → 2.92 ms on Vulkan, 2.95 → 4.52 ms on WSL (at 3840 × 2160 on DX12, 2.40 → 4.19 ms); the GPU p99 stays 0.02–0.04 ms on Windows and the blur pass costs did not move, so the cost is CPU-side. 144 Hz still holds on every path: offscreen at every size (at most 3.12 ms on DX12 across the sizes, 2.92 ms on Vulkan, 4.75 ms on WSL, against 6.9 ms), and windowed on Windows at every size (frame p99 3.36–3.68 ms on DX12, 2.52–2.67 ms on Vulkan). WSL windowed misses 144 Hz at every size and 60 Hz from 2560 × 1440 up, exactly as before (Fifo presentation). No path misses a budget it held before; the margin is what shrank (2.4 ms of the 144 Hz frame left on WSL offscreen at 2560 × 1440). The sweeps other than fx still run with a 1024 cap; a reference run with the 16 384 pool would measure it for the worms, pool, map and art rows.
- The bots' route finding is nearly the whole tick on large maps (bots p99 1.27 of the tick's 1.28 ms at 4096 × 4096; map table): the first place to optimise if maps or bot counts grow.
- Art scale 8 peaks at 5153 MiB for a 1024 MiB colour image (art table): holding fewer full-size copies of the art is the precondition for raising `MAX_COLOR_MAP_BYTES`.
- WSL's windowed presentation costs about 3.0 ms of CPU per million pixels (wsl-gl windowed table), where the native Windows busy time stays flat with the size (0.60–0.63 ms p50 on DX12, 0.41–0.45 ms on Vulkan): if WSL play at high resolutions matters, its present path is the place to look.
- The pool sweep's bots never pass 4324 live projectiles (pool table), so pools above it are not exercised; a pool point with 32 bots, as `cargo xtask stress` plays, would measure them.
- The report's least-squares line on the flat sweeps ("Tick p99 grows 0.1470 µs per unit of art"; art, fx and pool tables) is noise; the report could leave it out when the tick does not grow.
<!-- summary:end -->

## Method

- Budgets: tick p99 ≤ 10 ms (OLX's 100 Hz frame), frame p99 ≤ 6.9 ms (144 Hz) and ≤ 16.7 ms (60 Hz), peak memory below 2048 MiB. A point holds when all of them do; the breaking point is the first value that does not, with the cause (tick, frame, memory, or a cap).
- Sweeps: `olr-cli stress`, one process a point, medians of the runs; the reference is Classic on Castle Strike, 16 bots of mixed difficulty (chaos bots; Deathmatch bots on the map and art sweeps, so route finding counts), OLX's pool, 20 s, seed 1, `LoadingTime` 0, bonuses every 5 s. The pool sweep plays MW 1.0. Map and art levels are generated in memory (`gen-<w>x<h>-a<scale>-s1`).
- Frames: `olr-cli gpu-bench` offscreen on the hardware adapter (timestamps when the adapter has them), and `olr --frame-log --uncapped` windowed; on WSL (GL over Mesa's d3d12, presented through WSLg) and native Windows (DX12, Vulkan) built with mingw-w64 and run through interop.
- Rollback: `olr-cli rollback-cost`, a Classic match of 16 chaos bots (500 ticks of warm-up, then a sample every 10 ticks of play): save, play `depth` ticks, then time the restore and the re-simulation of those ticks with their recorded keys (the bots' thinking is not timed: online, bots think on the host only). Budgets: restore plus 12 ticks p99 ≤ 2 ms at the reference (Castle Strike) and ≤ 4 ms at 4096 × 4096; depth 28 (`MAX_PREDICTION`) is information.
- OLX: the oracle's patched OLX (Docker; a Release build, -O3) timing each simulated frame to a file of its own; olr-sim timed on the same scenarios, the same work checked frame by frame.

## The machine

- CPU: AMD Ryzen 5 7600X 6-Core Processor (12 logical CPUs)
- windows-dx12: NVIDIA GeForce RTX 5070 Ti (Dx12, DiscreteGpu)
- windows-vulkan: NVIDIA GeForce RTX 5070 Ti (Vulkan, DiscreteGpu)
- wsl-gl: D3D12 (NVIDIA GeForce RTX 5070 Ti) (Gl, Other)

## Sweeps

### art

art varies; otherwise 16 Deathmatch bots (OLX's bot: routes through the area cache) of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-classic on gen-2048x2048-a1-s1).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame with bots p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| a1 | 0.04 / 0.22 | 2 % | 0.01 | 0.21 | 0.01 | 0.01 | 76 | 959 | 113 | 1.22 | yes |
| a2 | 0.04 / 0.22 | 2 % | 0.01 | 0.21 | 0.02 | 0.01 | 76 | 959 | 353 | 1.11 | yes |
| a4 | 0.04 / 0.21 | 2 % | 0.01 | 0.21 | 0.02 | 0.01 | 76 | 959 | 1313 | 1.23 | yes |
| a8 | 0.05 / 0.22 | 2 % | 0.02 | 0.21 | 0.04 | 0.01 | 76 | 959 | 5153 | 2.82 | memory 5153 MiB > 2048 MiB; cap: the content loader refuses a 2048x2048 level at art scale 8: a 1024 MiB colour image, above MAX_COLOR_MAP_BYTES (256 MiB) |

Frame with bots: the point's level drawn offscreen at 2560×1440 on wsl-gl (`olr-cli gpu-bench`: a tick and a frame; it replays recorded inputs, so its tick has no bots' thinking) plus the point's bots p99 above: a sum of p99s, so a bound. This is the frame the 144 Hz and 60 Hz budgets judge.

Breaking point (144 Hz): **a8** (memory 5153 MiB > 2048 MiB; cap: the content loader refuses a 2048x2048 level at art scale 8: a 1024 MiB colour image, above MAX_COLOR_MAP_BYTES (256 MiB)). Breaking point (60 Hz): **a8** (memory 5153 MiB > 2048 MiB; cap: the content loader refuses a 2048x2048 level at art scale 8: a 1024 MiB colour image, above MAX_COLOR_MAP_BYTES (256 MiB)).

Tick p99 grows 0.1470 µs per unit of art (least squares).

### fx

fx varies; otherwise 16 chaos bots of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-classic on olx-level-castlestrike).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame with bots p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1024 | 0.05 / 0.13 | 1 % | 0.13 | 0.00 | 0.02 | 0.03 | 891 | 1024 | 31 | — | yes (effects full (1024) for 27 ticks) |
| 4096 | 0.06 / 0.13 | 1 % | 0.13 | 0.00 | 0.04 | 0.06 | 891 | 3997 | 31 | — | yes |
| 16384 | 0.06 / 0.13 | 1 % | 0.13 | 0.00 | 0.05 | 0.08 | 891 | 7572 | 31 | — | yes |
| 65536 | 0.06 / 0.13 | 1 % | 0.13 | 0.00 | 0.05 | 0.08 | 891 | 7572 | 34 | — | yes |

Breaking point (144 Hz): none: every value holds. Breaking point (60 Hz): none: every value holds.

Tick p99 grows 0.0000 µs per unit of fx (least squares).

### map

map varies; otherwise 16 Deathmatch bots (OLX's bot: routes through the area cache) of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-classic on gen-1024x1024-a1-s1).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame with bots p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1024x1024 | 0.03 / 0.13 | 1 % | 0.01 | 0.13 | 0.01 | 0.01 | 77 | 960 | 38 | 1.52 | yes |
| 2048x2048 | 0.04 / 0.21 | 2 % | 0.01 | 0.21 | 0.01 | 0.01 | 76 | 959 | 113 | 1.18 | yes |
| 4096x2048 | 0.06 / 0.57 | 6 % | 0.01 | 0.57 | 0.02 | 0.01 | 77 | 960 | 213 | 1.40 | yes |
| 4096x4096 | 0.34 / 1.28 | 13 % | 0.01 | 1.27 | 0.02 | 0.01 | 86 | 892 | 413 | 2.12 | yes |

Frame with bots: the point's level drawn offscreen at 2560×1440 on wsl-gl (`olr-cli gpu-bench`: a tick and a frame; it replays recorded inputs, so its tick has no bots' thinking) plus the point's bots p99 above: a sum of p99s, so a bound. This is the frame the 144 Hz and 60 Hz budgets judge.

Breaking point (144 Hz): none: every value holds. Breaking point (60 Hz): none: every value holds.

### pool

pool varies; otherwise 16 chaos bots of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-mw-1-0 on olx-level-castlestrike).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame with bots p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3000 | 0.23 / 0.51 | 5 % | 0.51 | 0.01 | 0.03 | 0.11 | 3000 | 1024 | 45 | — | yes (pool full (3000) for 22 ticks; effects full (1024) for 54 ticks) |
| 10000 | 0.24 / 0.57 | 6 % | 0.56 | 0.01 | 0.03 | 0.14 | 4324 | 1024 | 45 | — | yes (effects full (1024) for 51 ticks) |
| 20000 | 0.24 / 0.59 | 6 % | 0.59 | 0.01 | 0.04 | 0.14 | 4324 | 1024 | 47 | — | yes (effects full (1024) for 51 ticks) |
| 50000 | 0.24 / 0.57 | 6 % | 0.57 | 0.01 | 0.03 | 0.14 | 4324 | 1024 | 52 | — | yes (effects full (1024) for 51 ticks) |
| 65536 | 0.24 / 0.58 | 6 % | 0.58 | 0.01 | 0.03 | 0.14 | 4324 | 1024 | 54 | — | yes (effects full (1024) for 51 ticks) |

Breaking point (144 Hz): none: every value holds. Breaking point (60 Hz): none: every value holds.

Tick p99 grows 0.0006 µs per unit of pool (least squares).

### worms

worms varies; otherwise 16 chaos bots of mixed difficulties, pool 3000, effects 1024, 20 s, seed 1, LoadingTime 0, bonuses every 5 s (olx-classic on olx-level-castlestrike).

| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | projectiles | effects | MiB | frame with bots p99 (ms) | holds at 144 Hz |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 2 | 0.01 / 0.03 | 0 % | 0.03 | 0.00 | 0.01 | 0.01 | 357 | 1024 | 29 | — | yes (effects full (1024) for 1 ticks) |
| 4 | 0.01 / 0.03 | 0 % | 0.02 | 0.00 | 0.01 | 0.01 | 283 | 1024 | 29 | — | yes (effects full (1024) for 2 ticks) |
| 8 | 0.02 / 0.05 | 1 % | 0.05 | 0.00 | 0.01 | 0.02 | 564 | 1024 | 30 | — | yes (effects full (1024) for 4 ticks) |
| 16 | 0.05 / 0.13 | 1 % | 0.13 | 0.00 | 0.02 | 0.02 | 891 | 1024 | 31 | — | yes (effects full (1024) for 27 ticks) |
| 24 | 0.10 / 0.25 | 3 % | 0.25 | 0.01 | 0.02 | 0.09 | 1762 | 1024 | 31 | — | yes (effects full (1024) for 53 ticks) |
| 32 | 0.15 / 0.36 | 4 % | 0.36 | 0.01 | 0.02 | 0.10 | 1879 | 1024 | 32 | — | yes (effects full (1024) for 80 ticks) |

Breaking point (144 Hz): none: every value holds. Breaking point (60 Hz): none: every value holds.

Cost per worm: 5 µs of tick at p50, 12 µs at p99. At this rate the tick p99 reaches 10 ms at about 870 worms (a projection: `MAX_WORMS` stays 32).

## Rollback

Restoring the world and re-simulating `depth` ticks (the bots' keys recorded, not thought again), 16 chaos bots of mixed difficulties, Classic, pool 3000, 300 samples a run, seed 1.

| point | worms | depth | restore p50 / p99 (µs) | re-simulation p50 / p99 (µs) | total p99 (µs) | snapshot (B) | journal, at most (B) | budget |
|---|---|---|---|---|---|---|---|---|
| castlestrike-12 | 16 | 12 | 21 / 45 | 646 / 1231 | 1259 | 95066 | 20135 | holds (2000 µs budget) |
| castlestrike-28 | 16 | 28 | 31 / 63 | 1555 / 2543 | 2600 | 95066 | 42423 | no budget |
| 4096x4096-12 | 16 | 12 | 26 / 53 | 371 / 670 | 720 | 56016 | 20723 | holds (4000 µs budget) |

Every budgeted point holds.

## GPU

### windows-dx12: NVIDIA GeForce RTX 5070 Ti (Dx12, DiscreteGpu)

GPU times: timestamp queries.

The heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool) offscreen, a tick and a frame each (the replay's recorded inputs: its tick has no bots' thinking):

| size | tick (no bots) + frame p50 / p99 (ms) | frame p99, CPU then GPU (ms) | GPU p99 (ms) |
|---|---|---|---|
| 1920×1080 | 1.96 / 3.12 | 1.59 | 0.02 |
| 2560×1440 | 1.99 / 3.42 | 1.67 | 0.02 |
| 3440×1440 | 1.99 / 3.50 | 1.62 | 0.03 |
| 3840×2160 | 2.03 / 4.19 | 1.97 | 0.04 |

Shader headroom, GPU side (a full-screen blur pass, `olr_render::Probe`): the GPU time one pass costs, and how many passes fit beside the frame's GPU p99 (its frame p99 without timestamps) within the 6.9 ms (144 Hz) and 16.7 ms (60 Hz) budgets. This is GPU-side headroom, not the whole frame: the tick and the draw list run on the CPU beside it.

| size | GPU ms per pass | passes that fit at 144 Hz | at 60 Hz |
|---|---|---|---|
| 1920×1080 | 0.034 | 202 | 487 |
| 2560×1440 | 0.060 | 115 | 278 |
| 3440×1440 | 0.080 | 86 | 208 |
| 3840×2160 | 0.133 | 51 | 124 |

Windowed, asked for uncapped (`olr --frame-log --uncapped`, HUD included). Frame is frame to frame (what the player sees); busy is a frame's CPU time without waiting to present.

Against the budgets (frame p99): 144 Hz (6.9 ms) holds at every size; 60 Hz (16.7 ms) holds at every size.

| asked | got | present mode | frames | FPS | frame p50 / p99 (ms) | busy p50 / p99 (ms) | holds 144 / 60 Hz |
|---|---|---|---|---|---|---|---|
| 1920×1080 | 1920×1080 | Immediate | 12741 | 1062 | 0.79 / 3.42 | 0.62 / 3.24 | yes / yes |
| 2560×1440 | 2560×1440 | Immediate | 12074 | 1006 | 0.82 / 3.41 | 0.63 / 3.20 | yes / yes |
| 3440×1440 | 3440×1440 | Immediate | 12248 | 1021 | 0.82 / 3.36 | 0.61 / 3.13 | yes / yes |
| 3840×2160 | 3840×2160 | Immediate | 11898 | 992 | 0.81 / 3.68 | 0.60 / 3.43 | yes / yes |

The sweeps' levels drawn offscreen (their replays: the tick has no bots' thinking; the sweeps' tables add it):

| axis | value | size | tick (no bots) + frame p99 (ms) | GPU p99 (ms) |
|---|---|---|---|---|
| map | 1024x1024 | 2560×1440 | 0.36 | 0.02 |
| map | 2048x2048 | 2560×1440 | 0.40 | 0.02 |
| map | 4096x2048 | 2560×1440 | 0.34 | 0.02 |
| map | 4096x4096 | 2560×1440 | 0.32 | 0.02 |
| art | a1 | 2560×1440 | 0.36 | 0.02 |
| art | a2 | 2560×1440 | 0.38 | 0.02 |
| art | a4 | 2560×1440 | 0.39 | 0.02 |
| art | a8 | 2560×1440 | 0.44 | 0.04 |

### windows-vulkan: NVIDIA GeForce RTX 5070 Ti (Vulkan, DiscreteGpu)

GPU times: timestamp queries.

The heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool) offscreen, a tick and a frame each (the replay's recorded inputs: its tick has no bots' thinking):

| size | tick (no bots) + frame p50 / p99 (ms) | frame p99, CPU then GPU (ms) | GPU p99 (ms) |
|---|---|---|---|
| 1920×1080 | 1.53 / 2.62 | 0.98 | 0.02 |
| 2560×1440 | 1.62 / 2.92 | 1.08 | 0.02 |
| 3440×1440 | 1.59 / 2.85 | 1.01 | 0.03 |
| 3840×2160 | 1.61 / 2.80 | 0.98 | 0.04 |

Shader headroom, GPU side (a full-screen blur pass, `olr_render::Probe`): the GPU time one pass costs, and how many passes fit beside the frame's GPU p99 (its frame p99 without timestamps) within the 6.9 ms (144 Hz) and 16.7 ms (60 Hz) budgets. This is GPU-side headroom, not the whole frame: the tick and the draw list run on the CPU beside it.

| size | GPU ms per pass | passes that fit at 144 Hz | at 60 Hz |
|---|---|---|---|
| 1920×1080 | 0.028 | 246 | 593 |
| 2560×1440 | 0.049 | 142 | 343 |
| 3440×1440 | 0.065 | 107 | 257 |
| 3840×2160 | 0.108 | 63 | 153 |

Windowed, asked for uncapped (`olr --frame-log --uncapped`, HUD included). Frame is frame to frame (what the player sees); busy is a frame's CPU time without waiting to present.

Against the budgets (frame p99): 144 Hz (6.9 ms) holds at every size; 60 Hz (16.7 ms) holds at every size.

| asked | got | present mode | frames | FPS | frame p50 / p99 (ms) | busy p50 / p99 (ms) | holds 144 / 60 Hz |
|---|---|---|---|---|---|---|---|
| 1920×1080 | 1920×1080 | Immediate | 18135 | 1511 | 0.55 / 2.52 | 0.41 / 2.35 | yes / yes |
| 2560×1440 | 2560×1440 | Immediate | 18354 | 1530 | 0.54 / 2.57 | 0.41 / 2.38 | yes / yes |
| 3440×1440 | 3440×1440 | Immediate | 17032 | 1419 | 0.59 / 2.67 | 0.45 / 2.42 | yes / yes |
| 3840×2160 | 3840×2160 | Immediate | 18181 | 1515 | 0.55 / 2.64 | 0.41 / 2.38 | yes / yes |

The sweeps' levels drawn offscreen (their replays: the tick has no bots' thinking; the sweeps' tables add it):

| axis | value | size | tick (no bots) + frame p99 (ms) | GPU p99 (ms) |
|---|---|---|---|---|
| map | 1024x1024 | 2560×1440 | 0.28 | 0.02 |
| map | 2048x2048 | 2560×1440 | 0.23 | 0.02 |
| map | 4096x2048 | 2560×1440 | 0.30 | 0.02 |
| map | 4096x4096 | 2560×1440 | 0.23 | 0.02 |
| art | a1 | 2560×1440 | 0.27 | 0.02 |
| art | a2 | 2560×1440 | 0.28 | 0.02 |
| art | a4 | 2560×1440 | 0.27 | 0.02 |
| art | a8 | 2560×1440 | 0.31 | 0.04 |

### wsl-gl: D3D12 (NVIDIA GeForce RTX 5070 Ti) (Gl, Other)

GPU times: timestamp queries.

The heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool) offscreen, a tick and a frame each (the replay's recorded inputs: its tick has no bots' thinking):

| size | tick (no bots) + frame p50 / p99 (ms) | frame p99, CPU then GPU (ms) | GPU p99 (ms) |
|---|---|---|---|
| 1920×1080 | 3.02 / 4.71 | 3.02 | 0.03 |
| 2560×1440 | 3.07 / 4.52 | 3.07 | 0.04 |
| 3440×1440 | 3.03 / 4.69 | 3.03 | 0.05 |
| 3840×2160 | 3.03 / 4.75 | 3.09 | 0.20 |

Shader headroom, GPU side (a full-screen blur pass, `olr_render::Probe`): the GPU time one pass costs, and how many passes fit beside the frame's GPU p99 (its frame p99 without timestamps) within the 6.9 ms (144 Hz) and 16.7 ms (60 Hz) budgets. This is GPU-side headroom, not the whole frame: the tick and the draw list run on the CPU beside it.

| size | GPU ms per pass | passes that fit at 144 Hz | at 60 Hz |
|---|---|---|---|
| 1920×1080 | 0.027 | 259 | 624 |
| 2560×1440 | 0.046 | 149 | 359 |
| 3440×1440 | 0.062 | 111 | 269 |
| 3840×2160 | 0.102 | 65 | 160 |

Windowed, asked for uncapped (`olr --frame-log --uncapped`, HUD included). Frame is frame to frame (what the player sees); busy is a frame's CPU time without waiting to present.

Every run presented with Fifo: the surface offered no uncapped mode, so a frame also waits for the display's refresh and the frame rate is at most the refresh rate.

The busy time grows with the window's pixels: 7.2 ms p50 at 1920×1080 (2.1 Mpx) to 25.6 ms at 3840×2160 (8.3 Mpx), about 3.0 ms per million pixels (least squares over the p50s), while the offscreen frame's p50 stays 1.90–1.95 ms at the same sizes (table above). Presenting costs this path CPU time per pixel, so its windowed frame rate falls with the resolution: this is the cost of playing on this path, not of drawing the game.

Against the budgets (frame p99): 144 Hz (6.9 ms) missed at every size; 60 Hz (16.7 ms) missed at 2560×1440, 3440×1440, 3840×2160.
On windows-dx12 and windows-vulkan (the native Windows build) the windowed runs hold both budgets at every size (their tables): at high resolutions, play there rather than on this path.

| asked | got | present mode | frames | FPS | frame p50 / p99 (ms) | busy p50 / p99 (ms) | holds 144 / 60 Hz |
|---|---|---|---|---|---|---|---|
| 1920×1080 | 1920×1080 | Fifo | 1254 | 105 | 9.42 / 13.72 | 7.22 / 10.44 | no / yes |
| 2560×1440 | 2560×1440 | Fifo | 787 | 66 | 14.94 / 19.75 | 11.63 / 15.88 | no / no |
| 3440×1440 | 3440×1440 | Fifo | 618 | 51 | 19.22 / 24.01 | 15.15 / 18.48 | no / no |
| 3840×2160 | 3840×2160 | Fifo | 381 | 32 | 30.99 / 38.28 | 25.59 / 30.22 | no / no |

The sweeps' levels drawn offscreen (their replays: the tick has no bots' thinking; the sweeps' tables add it):

| axis | value | size | tick (no bots) + frame p99 (ms) | GPU p99 (ms) |
|---|---|---|---|---|
| map | 1024x1024 | 2560×1440 | 1.39 | 0.03 |
| map | 2048x2048 | 2560×1440 | 0.97 | 0.03 |
| map | 4096x2048 | 2560×1440 | 0.83 | 0.03 |
| map | 4096x4096 | 2560×1440 | 0.85 | 0.03 |
| art | a1 | 2560×1440 | 1.01 | 0.03 |
| art | a2 | 2560×1440 | 0.89 | 0.04 |
| art | a4 | 2560×1440 | 1.02 | 0.03 |
| art | a8 | 2560×1440 | 2.60 | 0.06 |

## OLX

Image `olr-olx-oracle:1958255f9797`, built Release, -O3 (CMakeOlxCommon.cmake:43-47, :369); CMAKE_CXX_FLAGS_RELEASE:STRING=-O3 -DNDEBUG; c++ (Ubuntu 11.4.0-1ubuntu1~22.04.3) 11.4.0. OLX runs in a Docker container on WSL's own kernel (no extra virtualisation, the same clock). Its timed span is the gus logic and the client's and the server's frame, without the trace writing; shot-list delivery and message parsing run in OLX's `betweenFrames()`, outside that span, while olr-sim does the equivalent inside `World::tick`, so the comparison slightly flatters OLX. olr-sim's span (`olr-cli scenario --time`) also holds a scenario's bonus placement (none in these scenarios). OLX draws on the CPU at 640 × 480 and is not timed here (not comparable like for like).

Load: heavy-spikeballs fills OLX's 3000-projectile pool (`MAX_PROJECTILES`); heavy-napalm is a mid-load point (829 projectiles at its peak). The others are the busiest oracle fixtures. Same work: the same projectiles and `rand()` calls every frame in both.

| scenario | frames | peak projectiles | OLX p50 / p99 (µs) | olr-sim p50 / p99 (µs) | olr-sim / OLX (p50) | same work | matches OLX |
|---|---|---|---|---|---|---|---|
| two-worms-fire | 200 | 114 | 11.1 / 55.4 | 3.8 / 17.4 | 0.35 | yes | yes |
| multi-shot | 150 | 114 | 6.1 / 18.5 | 2.7 / 14.0 | 0.44 | yes | yes |
| suicide-score | 660 | 23 | 3.2 / 28.8 | 0.5 / 5.8 | 0.15 | yes | yes |
| fire-minigun | 200 | 25 | 2.9 / 20.2 | 1.4 / 6.1 | 0.49 | yes | yes |
| weapons/classic/spikeballs | 368 | 23 | 4.0 / 12.1 | 1.2 / 4.6 | 0.29 | yes | yes |
| weapons/classic/uzi | 368 | 16 | 4.7 / 25.1 | 0.8 / 4.5 | 0.17 | yes | yes |
| heavy-spikeballs | 800 | 3000 | 426.8 / 1372.9 | 154.3 / 503.5 | 0.36 | yes | — (timing trace) |
| heavy-napalm | 800 | 829 | 159.4 / 302.1 | 48.5 / 81.9 | 0.30 | yes | — (timing trace) |

### Limits

| | OLX (`d1ad953f`) | OLR |
|---|---|---|
| Worms | 32 (`MAX_WORMS`, `include/Consts.h:34`) | 32 (`olr_sim::MAX_WORMS`) |
| Projectiles | 3000 (`MAX_PROJECTILES`, `include/CProjectile.h:36`) | 3000 by default, up to 65 536 (`MatchSetup::MAX_POOL_CAPACITY`) |
| Map side | 4096 (12-bit worm positions) | 4096 for imported levels (`olr-import-lxl`) |
| Effects | 1024 (`MAX_ENTITIES`, `include/Entity.h:30`) | 16 384 in play, 1024 of them kept for deaths (`olr_fx::PLAY_ENTITIES`, `DEATH_RESERVE`); OLX's 1024 with `olx_quirks` (`olr_fx::MAX_ENTITIES`); `Fx::with_capacity` in benches |
| Colour image | — | 256 MiB (`MAX_COLOR_MAP_BYTES`), art scale at most 8 |
