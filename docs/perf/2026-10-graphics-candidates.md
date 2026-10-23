# Graphics performance: what was done, and the bigger ideas (issue #35)

Issue #35 asked for due diligence on graphics performance: cheap wins
that leave the picture **bit-identical**, and a written choice of the
bigger ideas that do not. Part 1 below is what was done; Part 2 is the
candidates, for the maintainer to turn into issues or drop.

All the numbers here are from the software adapter (lavapipe, on the
Ryzen 5 7600X), drawn by `cargo xtask bench-quick`'s lit Castle Strike
Reloaded frame at 1080p (8 chaos bots after 2000 ticks, 32 glowing
discs; `docs/perf/graphics-tools.md`). A CPU rasteriser weighs passes
otherwise than a GPU: they rank the work and show a change's direction,
never real milliseconds. The real per-pass times are the Windows 1080p
measurement (`2026-10-0.2-windows-1080p.md`, 2026-10-23), summed up in
"On hardware" below.

## The frame today

One lit frame of Castle Strike Reloaded (1350 × 550 world pixels, one
glow-light cell a world pixel: 742 500 cells):

| Pass group | Passes | lavapipe ms before | after #35 | What it does |
|---|---|---|---|---|
| world | 2 | 3.0 | 3.0 | the world pass (albedo, shade) and the occluder map |
| emission | 3 | 5.7 | 5.1 | the level's occluder map, play's discs, the emission |
| jump flood | 12 | 36.5 | 32.4 | the seed and 11 flood passes (steps 1024 down to 1) |
| cascades | 10 | 122.2 | 119.5 | the radiance cascades, top down |
| fluence | 1 | 2.1 | 2.1 | cascade 0's directions averaged |
| light sum | 1 | 1.3 | 1.2 | ambient and glow light into the light grid |
| soak | 2 | 15.8 | 10.0 | light into solids, across and down |
| blur | 2 → 1 | 2.1 | 1.0 | the softness blur, toned and banded |
| bloom | 2 | 6.5 | 6.5 | the bloom, across and down |
| composite | 1 | 2.8 | 2.8 | the lit picture |
| upscale | 1 | 2.3 | 2.3 | into the output |
| copies, gaps | | 6.5 | 6.1 | the occluder copy (and, before, the history copy) |
| **frame** | **37 → 36** | **188.5** | **174.6** | the fastest of 5 frames, wall time |

The glow light (emission, jump flood, cascades, fluence) is about 90 % of
the lavapipe frame; on the integrated GPU it was about half of 13.3 ms
(`envelope.md`, plan 15a) when `GI_CELL_BUDGET` was 2^18 and Castle
Strike's cells 2 world pixels (185 625 cells); plan 15c's 2^20 gives it
one cell a world pixel, four times the cells. It is the lever.

## Part 1: done, picture-identical

Each change is its own commit, proven bit-identical by #34's
`lit_frames_keep_their_bytes` (the exact bytes of a lit match's light
grid and lit target, 30 frames), the renderer's GPU tests and the render
goldens. The gains are three quiet A/B runs of each commit's binary
against the one before (fastest of 5 frames each).

1. **No glow-light history copy.** The cascades read the fluence itself
   as last frame's light: they run before `fs_fluence` rewrites it; a
   fresh start clears the fluence. Saves a 742 500 × 8 B copy a frame
   (12 MB of traffic): copies and gaps 6.5 → 6.2 ms, about 8 k fewer
   instructions of encoding.
2. **Smaller formats.** The flood's seeds are whole cells in `Rg16Uint`
   (were `Rgba16Float` holding each coordinate in two f16 halves): half
   the traffic of all 12 flood passes and of every march's distance read.
   The emission's reach is `Rg16Float`, the disc layer's `R16Float`. Jump
   flood 36.6 → 32.3 ms (−12 %), cascades 122.6 → 119.6 ms (−2.5 %). GL
   draws into both (`a_lit_frame_draws_on_gl` ran on llvmpipe's GL).
3. **The disc layer is no longer cleared whole.** Each frame zeroes the
   quads of the discs it drew last (the same vertex shader, two disc
   buffers in turn) and adds its own; with no disc either frame the pass
   is skipped. Saves clearing 742 500 × 10 B a frame: emission 5.7 →
   5.3 ms. `discs_leave_nothing_behind` fails without the zeroing.
4. **The soak's worm search exits early.** `fs_soak_h`'s `near_worm` read
   121 occluder texels per light-grid texel; now each view uploads its
   worms' occluder boxes (a texel to spare, at most 32) and a texel more
   than the search's reach from every box skips it. Soak 15.8 → 10.0 ms
   (−37 %); 10 k more instructions to make the boxes.
5. **Terrain uploads by dirty tiles.** The colour map keeps dirty tiles of
   16 world pixels instead of a list of rectangles (which, past 256, fold
   into one box that can be most of the map), drained as each row's runs;
   a run only stains touched is `colour_only`, and the renderer uploads its
   colour alone, skipping the glow, groups, solid and region uploads and
   the flicker relabel. Over 100 ticks of the lit match: 46.6 M → 6.05 M
   instructions (−87 %), 4360 → 482 texture writes; the texels written
   grow from 38 KB to 490 KB (whole tiles; each old one-pixel write cost a
   256-byte staging row anyway). Tiles of 8 gave 8.1 M instructions and
   574 writes, of 32 6.7 M and 435: 16 is the least work.
6. **No across pass at a softness or a soak of 0.** At 0 the across blur
   and the across soak give back their source texel for texel; the down
   pass reads the source instead. Castle Strike's softness is 0: blur
   2.1 → 1.0 ms, one pass fewer. A soak of 0 (a copy of the level) drops
   the soak from 8.6 to 1.1 ms. Both checked bit-identical on copies of
   the level with a soak of 0 and a softness of 2 (`olr-cli frame-hashes`
   before and after); `no_soak_or_softness_runs_no_across_pass` counts
   the passes.
7. **CPU encoding.** The glow light's passes keep their attachments on
   the stack (no `Vec` a pass), and the look's globals hold each group's
   three vec4s together, so a frame packs and writes only its groups:
   uploads 75.2 KB → 51.1 KB a frame, the lit frame's encoding 1.342 M →
   1.292 M instructions.

All together (A/B of the commit before #35's optimisations and the
last): the lavapipe lit frame 188.5 → 174.6 ms (−7.4 %), encoding
1.359 M → 1.291 M instructions (−5 %), 37 → 36 passes, 75.2 → 51.1 KB
uploaded a frame. None was dropped.

### On hardware (2026-10-23)

The Windows 1080p measurement (`2026-10-0.2-windows-1080p.md`): the
native olr-cli at a13c69c9 and at 01503b4a, each drawing its own
recording of the 8-bot Castle Strike Reloaded match, Vulkan (DX12 could
not compile the soak at these commits), GPU ms p50:

| Scene | GPU | before | after #35 | jump flood | cascades | soak |
|---|---|---|---|---|---|---|
| own look | RTX 5070 Ti | 2.51 | 2.39 (−5 %) | 0.20 → 0.18 | 1.56 → 1.49 | 0.46 → 0.45 |
| own look | integrated (AMD) | 75.5 | 71.1 (−6 %) | 8.27 → 7.30 | 59.4 → 57.7 | 3.62 → 2.41 |
| `--look` | RTX 5070 Ti | 3.09 | 2.98 (−4 %) | 0.20 → 0.18 | 2.15 → 2.07 | 0.46 → 0.44 |
| `--look` | integrated (AMD) | 114.8 | 109.1 (−5 %) | 8.17 → 7.28 | 98.5 → 95.5 | 3.53 → 2.29 |

The hardware shares: the cascades are 75 to 92 % of the GPU frame and
the jump flood 5 to 10 %, on both GPUs; every other group is under
0.05 ms on the RTX. So the cascades weigh more than lavapipe's 68 %, the
flood less than its 19 %: A and D are the levers, B saves a few percent
at most.

## Part 2: the bigger ideas (not done)

Each changes the picture or the code's shape. Savings are lavapipe
shares of the 174.6 ms frame above (cascades 68 %, jump flood 19 %)
until the Windows table replaces them.

### A. Static and dynamic glow light, split (brainstorm direction D)

The glow light of the level's own glow (sky, lamps, lava) is recomputed
only when the terrain or the glow changes, or a slice of it a frame;
play's discs and the worms run every frame over a light of their own,
added.

- **Saves:** the largest lever. The static part is most of the 89 %;
  the dynamic part needs only cascades from the discs (most frames: 20 to
  40 small discs) and the worms' short shadows. Plausibly half to three
  quarters of the glow light, every frame the terrain is still.
- **Changes:** the bounce (it mixes both lights: either bounce only the
  static light, or keep a dynamic bounce at its own cost); after a big
  dig the static light lags a frame or more (a dirty-region recompute
  keeps it local); a flickering group's light is not static, so flames
  must be dynamic or flicker by a scale over a static shape (a
  different look up close); worms stopping static light need the
  dynamic pass to subtract, or worms only shadow the dynamic light.
- **Risk:** high: a second set of cascades, the merge of two lights, the
  dirty-region scheduling (Liero digs every tick), and the tests that pin
  today's light. The camera independence holds.
- **Recommendation:** yes, as its own design (spec first), after the
  Windows table confirms the glow light's share on the integrated GPU.

### B. Fewer jump-flood passes from the largest real distance

The flood runs steps 1024 down to 1 (11 passes) whatever the level holds.
Cap it at the largest distance a march needs (say 64: 7 passes), and
read a cell with no seed as "at least that far", never 1e9, or a ray
leaps walls.

- **Saves:** 4 of 12 flood passes, about 11 ms of 32 on lavapipe; a
  little more marching in open sky (jumps of at most the cap).
- **Changes:** not bit-identical: rays jump by other lengths, so their
  float positions and a few corner cases differ by a cell.
- **Risk:** low to medium; the thin-wall and diagonal-wall tests guard it.
- **Recommendation:** yes, cheap; measure the cascades' growth with it.

### C. Cached merge visibility

Each open ray of a cascade merges up to 8 upper probes, each behind a
`clear_between` march (`gi_cascade.wgsl`). The visibility between a
probe's ray end and an upper probe's ray start depends on the stoppers
only: cache it per cascade, probe and direction (a bit mask), rebuilt
where the terrain changed.

- **Saves:** an unknown share of the cascades (the merges' marches are
  short but many); measure first by timing a cascade with the merge off.
- **Changes:** discs and worms stop rays too, and move every frame: the
  cache can only hold the static stoppers, with the dynamic ones checked
  live (or ignored in the merge, a picture change near them).
- **Risk:** medium; memory (bits per probe-direction-neighbour) and the
  dirty rebuild.
- **Recommendation:** measure the merge's share first; worth it only
  above a quarter of the cascades. Pairs with A (static stoppers).

### D. The glow light per tick, or the upper cascades every other frame

The glow light at most once per 100 Hz tick (a 144 Hz display redraws
it 1.44 times a tick), or cascades 3 and up (the long rays) every other
frame, reused between.

- **Saves:** per tick: about 30 % of the glow light at 144 Hz, nothing at
  60 Hz. Upper cascades every other frame: about half of their share (the
  cascades cost much alike, so 7 of 10 halved: about a third).
- **Changes:** the light steps at 100 Hz (interpolated frames still see
  one light), or far light lags a frame; and the bounce, a per-frame
  feedback, converges at a frame-rate-dependent speed already: a lagging
  upper cascade makes that visible (a flash's afterglow differs at 60
  and 144 Hz). Make the bounce step per tick if this is done.
- **Risk:** medium (the history's step count, `glow_light_steps`, and
  split screen's one step a frame).
- **Recommendation:** the per-tick cap is cheap and safe for the
  bounce's convergence: yes. The every-other-frame cascades only if A is
  not done.

### E. Small folds: the seed into the emission; precomputed weights

- **The seed folded into the emission:** the emission pass writes the
  seeds as a fourth target; one pass and one full read of the emission
  fewer (about 1 of the flood's 12 passes). Bit-identical; low risk.
  **Recommendation:** yes, small.
- **Precomputed blur, bloom and occlusion weights** (the Gaussians'
  `exp` per tap, computed once on the CPU into a uniform): fewer ALU
  instructions in the bloom (6.5 ms) and occlusion, identical only to
  ULPs (the CPU's `exp` is not the GPU's), so a re-blessed picture.
  **Recommendation:** only with a measured ALU-bound bloom on the GPU.
- **Static emission** (the emission's per-cell sums of glow, groups and
  albedo cached, only the flicker and discs applied each frame): saves
  most of the emission pass (5 ms); identical to ULPs at best (a sum's
  order changes); part of A. **Recommendation:** with A.

### F. A smaller `GI_CELL_BUDGET`, or two-scale cascades (direction C)

Halving the work by the grid: a budget of 2^19 makes Castle Strike's
cells 2 world pixels (cells are powers of two: a quarter of the cells),
or a fine 1-pixel grid over each view with a coarse one over the level.

- **Saves:** about three quarters of the glow light at 2-pixel cells
  (the measured 2^18 → 2^16 step took the integrated GPU from 13.3 to
  8.6 ms, `envelope.md`); two-scale: the fine grid of a view is about a
  third of Castle Strike's cells, the coarse one little.
- **Changes:** 2-pixel cells let a 1-pixel wall leak (the plan 15c
  ruling that set 2^20) and dim thin glowing strips; two scales make
  the light near a view's edge depend on the camera
  (`light_is_camera_independent` pins today's), and split screen runs
  the fine grid per view.
- **Risk:** the leak is a known look regression; two scales are a large
  change.
- **Recommendation:** no for the budget (it undoes plan 15c's thin-wall
  fix); two-scale only if A is not enough for the integrated GPU.

## Order suggested

B and E's seed fold first (cheap, measurable on the next bench-quick);
then the Windows table (done: "On hardware" above) to size A against D; then A as
a design. C waits on a measurement of the merge's share; F stays a
fallback.
