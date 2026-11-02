# Beam shadow maps

Issue #100's second playtest, 2026-11-02. Builds on the directional-glow
design (`2026-10-25-directional-glow-design.md`) and live light (issue
#93). No OLX counterpart.

## Why

A glow beam is shadowed by rock and dirt: today the composite, for every
texel a beam lights, walks the world pixels from that texel to each of 8
points across the beam's source (`beam_sees` in
`crates/olr-render/src/look/composite.wgsl`). The cost grows with the
screen area a beam covers times its reach. Liero Factory Reloaded's
crane beacon, end-on (89° spread, 360 px reach), covers nearly the whole
view and made the GPU frame's p99 27 ms on an RTX 5070 Ti (STATUS.md,
"The beacon's beams"). Option 1 (an 8 × 8 block map the walk crosses an
open block of in one step, already built) brought it to 5.5 ms, still
too much for weaker GPUs.

The maintainer expects many more wide beams: street lamps (wide still
cones, several a level), searchlights (long beams sweeping every tick)
and flashlights carried by worms (a beam turning with a worm's aim every
frame, several at once, split screen too). A beam's cost must stop
depending on how much of the screen it lights.

## Goal

Each beam's shadows are traced once a frame, outward from its source
points along a fan of rays, into a shadow map; the composite looks a
texel's visibility up in it instead of walking.

- **Target:** the beams' own work fits a typical office laptop (Intel
  Iris Xe) at 1080p and 60 frames a second, so a level full of wide beams
  plays there once the rest of the lit frame does. A budget of its own
  (the maintainer, 2026-11-02): on the development PC's AMD integrated
  GPU (about half an Iris Xe, memory `igpu-vs-office-laptops`; `cargo
  xtask profile -- --adapter integrated`), the beam work (the new passes
  plus the composite's beam share: the frame against the same frame with
  the beams off) stays within **2 ms a frame**, about 1 ms on an Iris Xe.
  That GPU draws lit levels at 12 to 17 frames a second without any beam
  today, nearly all of it the glow light's cascades
  (`docs/perf/2026-10-24-render-profile.md`): making the glow light
  cheaper is a separate project, not this one.
- **Shared work:** the shadow map is made once a frame for every view
  (split screen draws each view's composite; today each walks again).
- **All beams alike:** the level's static beams and live copies' beams
  take the same path, so a street lamp and a flashlight look the same.

## Non-goals

- The flashlight as a weapon or tool (how a mod declares it, ammo, its
  placement on a worm): a project of its own. This one makes such beams
  cheap to draw.
- More than `MAX_GLOW_BEAMS` (64) beams a frame: the cap stays.
- The glow light (radiance cascades), its cost on integrated GPUs
  included (a separate project), heat deposits, discs and skin
  lights: they keep their own methods (they already share their rays, or
  cost too little to matter; the brainstorm, 2026-11-02).
- The cascades' `beam_cone` (the beam map's cone weighting of a glowing
  cell's light): unchanged.

## Anti-goals

- Light leaking through walls: a 1-pixel wall, straight or diagonal,
  still stops a beam completely, as the current GPU tests demand.
- Shadow edges crawling, shimmering or stair-stepping while a beam
  sweeps or a camera pans over still scenery.
- A cost that comes back with screen area: the composite's per-texel
  beam work must be a constant number of texture reads per beam.
- Changing a level without beams: no pass runs, no byte changes
  (`no_glow_beams_no_haze_changes_no_byte`, the goldens, `lit-hashes.ron`).

## Constraints

- Render passes only: the renderer has no compute pipelines and runs on
  wgpu's GL backend (`a_lit_frame_draws_on_gl`): no storage textures, no
  `R32Float` render targets (`Rg16Float`, `R16Uint`, `Rg16Uint` are used
  today).
- Hostile content: every beam field is already clamped by `pack`
  (`MAX_BEAM_REACH` 512, spread below 90°, non-finite beams dropped);
  the map's size is bounded by the cap, never by content.
- No per-frame allocation once warm (`olr-render`'s rule).
- Camera-independent light (`beams_are_camera_independent`): the map is
  in world coordinates.
- The beam keeps today's rules where it can (the directional-glow design
  and `crates/olr-render/CLAUDE.md`, "Glow beams"): a front source's own
  rock never shadows it (`behind_face`); a backdrop source (`back`) is
  hidden by rock or dirt over its patch until dug; a corner passed
  exactly closes the ray when either pixel beside it is opaque; a solid
  pixel takes its beam at its centre, from its open 4-neighbour facing
  the source (`face_start`); 8 points across the source give the
  penumbra (`BEAM_SHADOW_POINTS`).

## Decided

- **Shadow maps, not per-pixel lighting or a smaller flare** (options 2
  and 4 of the analysis): the maintainer chose to start with option 1,
  then asked for this.
- **Accepted inexactness:** shadows far from a source may be slightly
  softer or off by up to about a pixel where adjacent rays diverge (the
  maintainer agreed, 2026-11-02).
- **Option 1 stays:** the rays' march crosses open blocks of the block
  map in one step too.

## The shape (the builder's to refine)

- **The map:** one row per (beam, source point): at most 64 × 8 = 512
  rows, `N` texels a row, each the distance from the point to the first
  pixel that stops the ray (`R16Uint` in fixed point, or a split format
  as `common.wgsl`'s `split`), drawn by a fragment pass over the rows of
  the frame's beams only.
- **The rays:** a fixed angular grid in world angles (not relative to the
  beam's direction), so a sweeping beam's rays do not move over still
  scenery and its shadows do not crawl; covering the half-plane ahead of
  the source, or only the beam's wedge plus a margin (see below). `N`
  such that adjacent rays are at most about a pixel apart at
  `MAX_BEAM_REACH` (1024 to 2048 over 180°).
- **The march:** the same pixel rules as `beam_sees`, run outward from
  the point: past the source's own rock behind its face for a front
  source, into the source's pixel for a backdrop one, the corner rule,
  the block map's open blocks in one step, at most the beam's reach.
- **The lookup:** a texel's angle and distance from each point; seen
  when within the stopping distance, blended between the two nearest
  rays so edges stay smooth; the 8 points' shares averaged as today. A
  solid pixel looks from `face_start`'s open neighbour as today.
- **When it is made:** once a frame before the first view's composite,
  shared by all views; skipped when nothing it reads changed (the beams
  packed, the solid map), as the glow light keeps its light within a
  world step (`GiState::holds`).

## Left to the builder

- Ray count, angular coverage and texel format, with the measured cost
  on the integrated GPU: a narrow beam need not spend 180° of rays. One
  way: rays only over the wedge (spread plus the penumbra plus the
  source's width as seen from a point) beyond a short distance from the
  source, and today's exact walk within it, where walks are short.
- Whether the map is made per frame or kept across frames of a world
  step, and how a change is detected.
- How the GPU tests change: those pinning exact geometry
  (`a_diagonal_wall_stops_a_beam`, `a_wall_touching_the_face_stops_a_beam`,
  `shadow_edges_are_smooth`, `a_long_beam_stays_smooth`,
  `new_dirt_stops_a_beam`, the live-beam tests) keep their intent; a
  threshold may move where the accepted inexactness requires, said in
  the test.
- How the many-beam scene is made (a fixture level of live copies, or
  beams set on the renderer directly), as long as it carries the load
  below and `render-profile` (or a test beside it) can draw it in one
  view and in two-player split screen.

## The many-beam scene

The maintainer's representative worst case (2026-11-02), all in view:

- 6 street lamps: spread 40°, reach 120 px, still;
- 2 searchlights: spread 10°, reach 400 px, sweeping every tick;
- 4 flashlights: spread 20°, reach 200 px, moving every frame.

12 beams, 6 of them changing all the time; also in two-player split
screen. A level at the cap of 64 wide beams at the longest reach need not
hold the budget: it only degrades gracefully (slower, never wrong).

## Done when

- On the AMD integrated GPU at 1080p, the beam work stays within 2 ms a
  frame (mean; p99 noted) for the factory's beacon end-on (`--at
  870,300`, ticks 200..500) and for the many-beam scene, one view and
  two.
- On the RTX 5070 Ti, the factory's beacon frames' mean is within about
  10 % of the crane resting (ticks 500..700) and their p99 within 1 ms of
  its p99 (how "about 10 %" is read: a call made writing this design).
- `cargo xtask check --render` passes; the factory goldens are
  re-blessed only for the accepted inexactness, looked at side by side
  (memory `look-decisions-from-real-frames`).
- `docs/porting/STATUS.md` records the method and the numbers;
  `crates/olr-render/CLAUDE.md` describes the map.
