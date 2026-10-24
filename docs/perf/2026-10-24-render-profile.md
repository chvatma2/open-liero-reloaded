# The first render profile: a nuke fight (2026-10-24)

`cargo xtask profile` (spec
`docs/superpowers/specs/2026-10-24-render-profile-design.md`, plan
`docs/superpowers/plans/2026-10-24-render-profile.md`) on its first runs.
These numbers find the bottlenecks; they are not targets.

## Setup

- **Scene:** the staged fight, Liero v1.0 Reloaded on Castle Strike
  Reloaded. One scripted local player and three Medium bots carry only
  the Big Nuke and Mini Nuke (loading 20 %). Seed 7, 2000 ticks (20 s);
  16 Big Nukes and 11 Mini Nukes went off, the first at tick 200.
- **How it is drawn:** the way the game draws it. The level's own look
  and heat, the game's playfield at each size (the HUD's `Auto` layout),
  the sharp filter, linear interpolation, blood 100 %. There are 2 frames
  a tick (about 200 FPS), and the HUD is not drawn.
- **Machine:** the maintainer's PC (Ryzen 5 7600X, RTX 5070 Ti), the
  native Windows build run from WSL. The RTX was measured on Vulkan and
  DX12 at 1920 × 1080, 3440 × 1440 and 3840 × 2160.
- **Integrated GPU:** the CPU's own AMD Radeon graphics (2 compute units)
  stood in for a laptop, on DX12 at 1080p. It drew ticks 150 to 450 at
  1 frame a tick. Its Vulkan timestamps are unreliable
  (`2026-10-0.2-windows-1080p.md`).
- **Phases:**
  - *blast*: within 0.5 s of a nuke going off;
  - *hot*: any heat box is active;
  - *calm*: everything else (only ticks 0 to 200 here).

## Where the time goes

GPU ms a frame (mean), RTX 5070 Ti, Vulkan, 3440 × 1440:

| Phase | Frames | GPU mean / p99 / max | Cascades | Jump flood | Copies and gaps | Everything else | Heat (all six) | CPU mean |
|---|---|---|---|---|---|---|---|---|
| calm | 400 | 2.02 / 2.26 / 2.66 | 1.58 (78 %) | 0.18 | 0.13 | 0.13 | – | 0.27 |
| blast | 1854 | 2.96 / 3.50 / 3.76 | 2.49 (84 %) | 0.18 | 0.14 | 0.15 | 0.01 | 0.33 |
| hot | 1746 | 3.07 / 3.55 / 3.79 | 2.60 (85 %) | 0.18 | 0.14 | 0.15 | 0.01 | 0.33 |

DX12 on the RTX gives the same picture. The cascades take 1.61, 2.51 and
2.61 ms (86 to 90 %), the jump flood 0.14 ms, and the gaps 0.02 ms;
DX12's CPU side is a little slower (0.5 to 0.6 ms).

The integrated GPU at 1080p, DX12:

| Phase | GPU mean / p99 | Cascades | Largest single cascade pass | Jump flood | Bloom | Emission | Heat (all six) |
|---|---|---|---|---|---|---|---|
| calm | 82.27 / 83.23 | 72.24 (88 %) | – | 5.82 | – | – | – |
| blast | 85.27 / 89.93 | 74.82 (88 %) | – | 5.82 | – | – | – |
| hot | 87.41 / 89.01 | 76.90 (88 %) | 22.85 | 5.82 | 0.89 | 0.74 | 0.07 |

That is about 12 frames a second; every other group is under 1 ms.

## What scales with what

- **Not the output size.** On the RTX, calm is 2.01 ms at 1080p, 2.02 ms
  at 3440 × 1440 and 2.05 ms at 4K. Only the upscale grows (0.01 to
  0.03 ms). The world target is 860 × 390 texels at every size (the view
  at art scale 2), and nothing else draws at the screen's resolution.
- **The level: the glow light.** Its grid is the whole level, 1350 × 550
  cells of one world pixel, with 10 cascades. It is recomputed every
  frame, wherever the camera is. That is what the cascades and the jump
  flood cost, and why the integrated GPU spends 72 ms even in calm
  frames.
- **The fight's lights, not its heat.**
  - On the RTX the cascades go from 1.58 ms (calm, up to 2 glowing discs)
    to 2.5 to 2.6 ms in the fight (up to 101 and 140 discs), +60 %.
  - On the integrated GPU's early window (up to 19 discs) they grow only
    +6 %.
  - So the extra cost follows the number of discs: the nukes' and their
    shrapnel's lights. The heat's glow (the heat area covers about the
    whole level, 0.75 to 0.78 million pixels on average, the distant
    layer included) may add some; one profile with the heat off would
    tell them apart.
- **The heat's own passes are nearly free:** 0.01 ms in all on the RTX
  and 0.07 ms on the integrated GPU, whatever the area. The heat costs
  frames only through the light it adds to the cascades.
- **The CPU is idle:** 0.3 ms a frame to build, upload and submit, and
  0.035 ms a tick.

## The game against the profile

The same fight's replay in `olr.exe`, fullscreen at 3440 × 1440, Vulkan,
uncapped (Immediate present):

| | Mean or p50 | p99 | Max |
|---|---|---|---|
| Game, frame to frame | 3.62 ms (286 FPS) | 5.13 ms | 6.04 ms |
| Game, CPU busy | 0.42 ms | 1.69 ms | – |
| Profile, GPU, blast / hot | 2.96 / 3.07 ms | 3.50 / 3.55 ms | 3.76 / 3.79 ms |

The game is GPU-bound: its CPU is busy 0.42 ms of 3.62. The profile
accounts for about 80 % of the frame at p50 and 70 % at p99. The rest
(about 0.6 ms typical, 1.5 ms at p99) is what the profile does not draw:
the HUD and side panels (egui), the swap chain, and the game loop. That
matches the low 200s the maintainer saw. Timing that rest inside the game
is a follow-up, worth it only once the glow light is cheaper.

## Candidates for cheap wins

The glow light is the frame: 78 to 90 % of it on every GPU, phase and
size, and with the jump flood that feeds it, 85 to 95 %. Everything else
together is under 0.2 ms on the RTX and about 4 ms on the integrated GPU.
Ranked by gain against effort and look risk. The letters are
`2026-10-graphics-candidates.md`'s ideas, which these numbers now size.

1. **The glow light once per tick, not every frame (D).**
   - *Points at it:* the light is fixed to the world and its inputs
     change once a tick. At the game's 286 FPS (2.9 frames a tick) about
     two thirds of the cascades are recomputing an unchanged light.
   - *Gain:* about 1.7 ms of the game's 3.6 ms on the RTX at that frame
     rate. Nothing at 60 Hz, so no help for the laptop.
   - *Look:* the discs' light steps at 100 Hz (the sprites still
     interpolate); the bounce should then step per tick too.
   - *Effort and risk:* small and low.
2. **Coarser glow cells (F).** A `GI_CELL_BUDGET` of 2^19 or less makes
   Castle Strike's cells 2 world pixels: a quarter of the probes.
   - *Points at it:* a single cascade pass is 22.85 ms on the integrated
     GPU; the measured 2^18 → 2^16 step took it from 13.3 to 8.6 ms
     (`envelope.md`).
   - *Gain:* plausibly most of the 72 to 77 ms on the integrated GPU.
     This is the one lever here that moves the laptop case.
   - *Look:* a 1-pixel wall can leak light and thin glowing strips dim.
     That is the regression plan 15c fixed by choosing 1-pixel cells; with
     some loss now acceptable, it should be judged from real frames side
     by side.
   - *Effort:* small, a constant.
3. **The number of discs.**
   - *Points at it:* the cascades grow 60 % on the RTX between 2 and
     140 discs.
   - *Next step:* one more profile, the heat off or the discs capped, to
     confirm. If it holds, merging or capping the small shrapnel lights
     (or giving them a shorter reach) is cheap.
   - *Look:* the shrapnel's sparkle of light thins.
4. **Fewer jump-flood passes (B).**
   - *Gain:* 0.18 ms of 3 on the RTX and 5.8 ms of 85 on the integrated
     GPU; capping at a 64-cell march saves about a third of that.
   - *Look:* not bit-identical, within the thin-wall tests' bounds.
   - *Effort:* small.
5. **Static and dynamic light split (A).**
   - *Gain:* the largest structural lever, the level's own light
     recomputed only where the terrain changes. It needs its own design.
   - *Points at it:* the calm frames show the static share, 1.58 ms of
     2.02 on the RTX and 72 of 82 ms on the integrated GPU, all spent on
     a light that mostly does not change.
6. **The light only round the views** (F's two-scale variant): a fine
   grid over each view and its reach, a coarse one elsewhere.
   - *Gain:* modest on Castle Strike (about a third of the cells), large
     on big levels.
   - *Look:* the light near a view's edge depends on the camera, which
     the design has avoided so far.

**Not worth chasing now:**
- the heat's passes (0.01 ms; 0.07 ms on the integrated GPU);
- the output resolution, the upscale and the bloom (each 0.03 ms on the
  RTX, under 1 ms on the integrated GPU);
- the CPU (0.3 ms).

For a steady 60 FPS on a laptop the glow light has to shrink about five
times on this integrated GPU, which is weaker than a typical laptop's.
That is a guess: no laptop has been measured. The per-tick cap does
nothing there. Coarser cells (2) together with the jump flood (4) look
like the shortest path, and the split (5) the thorough one.

## Rerunning

`cargo xtask profile` (Vulkan) or `--backend dx12` compares with the base
saved by `--set-base` in `target/render-profile/` (bases were set by
these runs). Pass `-- --ticks FROM..TO` to look at one stretch, or `--
--adapter integrated --sizes 1920x1080 --frames-per-tick 1 --ticks
150..450` for the integrated GPU, about a minute. `--game` adds the game
check, fullscreen on the display for about 22 s. A warm run (the Windows
build up to date) takes 43 s for all three sizes, and a rerun of the same
code lands within about 1 % of the base (calm 2.03 ms against 2.02). A
first run builds the Windows binaries, about 4 minutes.
