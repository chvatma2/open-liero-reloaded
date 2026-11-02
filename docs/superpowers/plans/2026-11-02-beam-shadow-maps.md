# Beam Shadow Maps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Glow beams' shadows are traced once a frame into a shadow map, outward from each beam's 8 source points along a fan of world-fixed rays, and the composite looks a texel's visibility up instead of walking to the source, so a beam's cost no longer grows with the screen area it lights.

**Architecture:** A new `renderer/beam_shadow.rs` owns the map (`R16Uint`, a row per beam and source point, a texel per ray), its rows' spans (CPU, pure, unit-tested) and its pass (`look/beam_shadow.wgsl`, a fragment shader marching each ray with `beam_sees`' rules and option 1's block map). `pass.rs` encodes it once a frame after `pack_glow_beams`, before the views' composites; `composite.wgsl`'s `glow_beams_at` keeps the exact walk within a short near zone of each point and reads the map beyond it.

**Tech Stack:** Rust, wgpu 30 (render passes only, GL backend included), WGSL.

**Spec:** `docs/superpowers/specs/2026-11-02-beam-shadow-maps-design.md`

## Global Constraints

- Render passes only (no compute, no storage textures, no `R32Float` render targets): the GL backend must draw it (`a_lit_frame_draws_on_gl`).
- Every texture side ≤ 2048 (wgpu's downlevel limit; `OffscreenOptions::max_texture_side` tests at 2048).
- No per-frame allocation once warm; the map is sized for the cap (`MAX_GLOW_BEAMS` 64 × 8 points), never by content.
- A level without beams runs no new pass and changes no byte (`no_glow_beams_no_haze_changes_no_byte`, the goldens, `fixtures/render/lit-hashes.ron`).
- Light stays camera-independent (`beams_are_camera_independent`): the map is in world coordinates.
- Budget: on the AMD integrated GPU at 1080p the beam work (the frame against the same frame with `set_glow_beam_light(false)`) ≤ **2 ms** mean, for the beacon (`--at 870,300`, ticks 200..500) and the many-beam scene, one view and two; on the RTX 5070 Ti the beacon frames' mean within about 10 % of the crane resting (ticks 500..700), p99 within 1 ms of its p99.
- Many-beam scene, all in view: 6 street lamps (spread 40°, reach 120 px, still), 2 searchlights (10°, 400 px, sweeping each tick), 4 flashlights (20°, 200 px, moving each frame).
- `cargo xtask check --render` before every commit; subjects `olr-render: … (#100)` (or the crate touched); commits end with the session's attribution lines.

## Review Focus

- **The angle grid's wrap:** a beam facing right (0°) spans ray indices across `BEAM_RAYS - 1 → 0`; its rows and lookups must wrap, not drop half the beam. Pinned in Task 1 (`a_span_across_angle_zero_wraps`) and Task 3 (`a_beam_lights_ahead_and_not_behind` faces 0°).
- **Rays leaving the level:** outside the map is open, as `solid_px` reads it; a ray must neither stop at the edge nor read out of bounds. Task 2: `rays_leaving_the_level_run_open`.
- **A device at the 2048-texel minimum:** the map's width is `SHADOW_WIDTH` ≤ 2048. Task 2: `the_map_fits_a_2048_device`.
- **64 beams at the longest reach (hostile content):** draws, slower, never wrong or out of bounds. Task 3: `sixty_four_beams_draw`.
- **Split screen:** the map is made once a frame, however many views. Task 4: `two_views_make_the_map_once`.

---

### Task 1: Rays and rows (CPU)

**Files:**
- Create: `crates/olr-render/src/renderer/beam_shadow.rs` (register `mod beam_shadow;` in `renderer.rs` beside `mod glow_beams;`)
- Modify: `crates/olr-render/src/renderer/glow_beams.rs` (`wgsl()` prepends the new constants)

**Interfaces:**
- Produces:
  - `pub(super) const BEAM_RAYS: u32 = 4000;` rays over the full circle, ray `i` at world angle `i × 2π / BEAM_RAYS` (y down), fixed in the world.
  - `pub(super) const SHADOW_WIDTH: u32 = BEAM_RAYS / 2 + 2;` (2002) the map's width: a half-plane's rays and one each side.
  - `pub(super) const BEAM_POINTS: u32 = 8;` (`composite.wgsl`'s `BEAM_SHADOW_POINTS`, now prepended from Rust).
  - `pub(super) const SHADOW_SCALE: f32 = 8.0;` stored distance units a world pixel; `pub(super) const SHADOW_CLEAR: u16 = u16::MAX;` nothing stops the ray within its limit.
  - `pub(super) fn point(at: [f32; 2], dir: [f32; 2], half: f32, k: u32) -> [f32; 2]`: `composite.wgsl`'s source point `k`: `at + perp × (u × half) − dir × 0.5`, `u = (k + 0.5) / BEAM_POINTS × 2 − 1`, `perp = (−dir.y, dir.x)`.
  - `pub(super) fn near(half: f32) -> f32`: the near zone's radius around each point, within which the composite keeps the exact walk.
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub(super) struct ShadowRow { pub start: u32, pub count: u32, pub limit: f32 }`: ray indices `start..start + count` (mod `BEAM_RAYS`), `count ≤ SHADOW_WIDTH`, marched to `limit` world pixels.
  - `pub(super) fn rows(packed: &[f32], out: &mut Vec<ShadowRow>)`: for each beam of `glow_beams::pack`'s output (`BEAM_FLOATS` each: at, dir, half, tan, reach, group, light, region), its `BEAM_POINTS` rows in point order; `out` cleared first, its capacity reserved for the cap once (no growth after).

- [ ] **Step 1: Write the failing unit tests** in `beam_shadow.rs`'s `mod tests`:
  - `every_lit_point_beyond_the_near_zone_has_its_rays`: for beams over dir angles {0°, 37°, 90°, 181°, 300°}, half {0.5, 3, 8}, spread {1°, 10°, 40°, 70°, 89.5°}, reach `glow_beams::walk_reach(half, tan).min(512)`, sample lit points `p` (along in (0, reach], |side| < half + along·tan + 0.5 + 0.03·along, a 1-px grid) and for every point `k` with `|p − point(k)| ≥ near(half)`: both rays bracketing `p`'s angle from `point(k)` lie in row `k`'s span, and `|p − point(k)| ≤ limit`.
  - `a_span_across_angle_zero_wraps`: a beam facing (1, 0), spread 10°: `start + count > BEAM_RAYS` and the angle 0 is inside.
  - `a_narrow_beam_takes_few_rays`: half 2, spread 10°, reach 400: every row's `count < 200`.
  - `a_wide_beam_takes_the_half_plane`: spread 89.5°: `count ≤ SHADOW_WIDTH`.
  - `rows_follow_the_packed_beams`: two packed beams give 16 rows, beam 1's at 8..16; an empty pack none.

- [ ] **Step 2: Run** `cargo nextest run -p olr-render beam_shadow` — FAIL (nothing defined).

- [ ] **Step 3: Implement** the constants, `point`, `near`, `ShadowRow` and `rows`. The span: the angles from the point of the wedge's part beyond `near` — conservatively the beam's direction ± (spread + the angle the source's width and the penumbra subtend at `near`), one ray of margin each side, capped at the half-plane ahead. `limit`: the farthest lit point from the point (`along = reach`, side at the wedge's edge plus its penumbra), plus 2.

- [ ] **Step 4: Run** the tests — PASS; `cargo xtask check` passes.

- [ ] **Step 5: Commit** `olr-render: beam shadow rows over a world-fixed ray fan (#100)`.

---

### Task 2: The shadow-map pass

**Files:**
- Modify: `crates/olr-render/src/renderer/beam_shadow.rs`
- Create: `crates/olr-render/src/look/beam_shadow.wgsl`
- Modify: `crates/olr-render/src/renderer/look.rs` (`LookState` holds a `BeamShadow`), `pass.rs` (encode it), `timing.rs` (`PassGroup::BeamShadow`, after `Fluence`, named `beam shadow`)
- Test: `crates/olr-render/tests/gpu/beam_shadow.rs` (a `mod` of `tests/gpu.rs`)

**Interfaces:**
- Consumes: Task 1's `rows`, `ShadowRow`, constants; `glow_beams.rs`' packed beams (`LookState::packed_beams`), the solid map view and the block map (`GlowBeams::blocks`).
- Produces:
  - `pub(super) struct BeamShadow` with `pub(super) fn new(device, queue, layouts) -> BeamShadow`, `pub(super) fn encode(&mut self, encoder, queue, packed: &[f32], binds: (&wgpu::BindGroup /* solid + blocks */), timer) -> bool` (true when a pass ran) and `pub(super) fn map_view(&self) -> &wgpu::TextureView`.
  - The map: `R16Uint`, `SHADOW_WIDTH × (MAX_GLOW_BEAMS × BEAM_POINTS)` (2002 × 512), made once. Texel `(j, beam × 8 + k)`: ray `(start + j) mod BEAM_RAYS` from point `k`; the distance to where the ray enters the first pixel that stops it, × `SHADOW_SCALE`, or `SHADOW_CLEAR`.
  - The rows' uniform (`ShadowRows`, `array<vec4<u32>, 512>`, 8 KB: per row `start`, `count`, `limit` as `f32` bits, the beam index), written each frame; one instanced quad a row, `count` texels wide.
  - Hidden test hook: `#[doc(hidden)] pub fn Renderer::beam_shadow_map(&self) -> Option<&wgpu::Texture>`.

- [ ] **Step 1: Write the failing GPU tests** (helpers as in `tests/gpu/glow_beams.rs`; read back with `olr_render::read_bytes`):
  - `a_ray_stops_at_the_wall`: the 128 × 64 level of `glow_beams.rs` with a rock column at x 60 and the patch at x 20 facing right: in row `k = 4` (just below the middle), the ray at angle 0 reads `(60 − p.x) × 8 ± 8`.
  - `an_open_ray_runs_to_its_limit`: no wall: that ray reads `SHADOW_CLEAR`.
  - `a_covered_backdrop_source_sees_nothing`: a backdrop patch under dirt: every ray of its rows reads 0.
  - `rays_leaving_the_level_run_open`: the patch at x 2 facing left: its rays read `SHADOW_CLEAR`.
  - `the_map_fits_a_2048_device`: `OffscreenOptions { max_texture_side: Some(2048), .. }`: a beam level draws, no validation error (`device.push_error_scope`).

- [ ] **Step 2: Run** `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu beam_shadow` — FAIL.

- [ ] **Step 3: Implement** `beam_shadow.wgsl`'s `fs_march`: from the row's point along the ray's unit vector, the grid walk of `composite.wgsl`'s `beam_sees` run outward (its crossing counts `n`, `first + n × delta`; option 1's open blocks in one step): a front source passes the cells behind its face (`behind_face`) without testing them, then stops at the first opaque cell (the corner rule: either pixel beside an exact corner, unless behind the face); a backdrop source tests its own pixel first; at `limit` it writes `SHADOW_CLEAR`. Share `solid_px`, `behind_face`, `block_open` and `block_behind_face` with the composite (move them to a prelude both shaders include). `pass.rs` encodes it after the glow light, before the first view's look passes, when `look.beams.x > 0` (the frame packed beams); one draw of `rows.len()` instances.

- [ ] **Step 4: Run** the tests — PASS; the whole GPU suite unchanged (the composite does not read the map yet).

- [ ] **Step 5: Commit** `olr-render: trace each beam's shadow rays once a frame (#100)`.

---

### Task 3: The composite reads the map

**Files:**
- Modify: `crates/olr-render/src/look/composite.wgsl` (`glow_beams_at`), `glow_beams.rs` (composite group 1 gains binding 3, the map, and binding 4, the rows' uniform), `look.rs` (the layout)
- Test: `crates/olr-render/tests/gpu/glow_beams.rs`, `tests/gpu/beam_shadow.rs`

**Interfaces:**
- Consumes: Task 2's map and rows' uniform; Task 1's `near`, constants.
- Produces: `fn shadow_seen(beam: i32, k: i32, origin: vec2<f32>, src: vec2<f32>) -> f32` in `composite.wgsl`: 0 to 1.

- [ ] **Step 1: Write the failing GPU tests:**
  - `shadows_hold_still_as_a_beam_turns` (`beam_shadow.rs`): a live beam (`LookFrame::live_beams`) of spread 30° beside a rock column, drawn at direction 0° and 1°: every world pixel more than 3 px inside both wedges is lit in both or shadowed in both (lit: light > 3 × ambient).
  - `sixty_four_beams_draw`: 64 live beams of spread 89.5° at the longest reach: a frame draws without a validation error.
  - `a_frame_with_beams_runs_one_shadow_pass`: with GPU timing on (`enable_gpu_timing`; skipped without timestamps), a beam frame's `Renderer::last_passes()` holds exactly one `PassGroup::BeamShadow`, and a frame of a level without beams none.

- [ ] **Step 2: Run** them — FAIL (the composite still walks; the turning test may already pass: keep it, it guards the change).

- [ ] **Step 3: Implement** `shadow_seen`: within `near(half)` of the point, today's exact `beam_sees`; beyond, the angle and distance from the point, the two bracketing rays' entries (`(i − start) mod BEAM_RAYS`; outside the span, unseen), each seen when the distance is below its stored distance, blended linearly by the angle's fraction. `glow_beams_at` uses it for each of the `BEAM_POINTS` points instead of `beam_sees`. Solid pixels keep looking from `face_start`'s neighbour.

- [ ] **Step 4: Run** the whole GPU suite (`OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu`) — PASS. A test pinning exact geometry (`a_diagonal_wall_stops_a_beam`, `a_wall_touching_the_face_stops_a_beam`, `shadow_edges_are_smooth`, `a_long_beam_stays_smooth`, `new_dirt_stops_a_beam`, the live-beam tests) may move a threshold only where the accepted inexactness (about a pixel far from the source) requires, with a comment saying so; no leak through a wall is accepted.

- [ ] **Step 5: Run** `cargo xtask check --render`. The factory goldens (`factory-crane.png`, `factory-beacon-side.png`) may differ: draw before and after side by side (`olr-cli render-match` with each build, memory `look-decisions-from-real-frames`), show the maintainer, and only on their yes `cargo xtask bless-render --reason "issue #100: beam shadows from a shadow map"`.

- [ ] **Step 6: Commit** `olr-render: beams look their shadows up in the map (#100)`.

---

### Task 4: Make the map only when it changes; once for all views

**Files:**
- Modify: `beam_shadow.rs`, `pass.rs`, `look.rs` (`write` marks it stale), `renderer.rs` (hidden counter)
- Test: `tests/gpu/beam_shadow.rs`

**Interfaces:**
- Produces: `#[doc(hidden)] pub fn Renderer::beam_shadow_builds(&self) -> u64`; `BeamShadow::stale(&self)` (a terrain write), the kept key: the packed beams' bytes (compared, not hashed) and no terrain write since.

- [ ] **Step 1: Write the failing GPU tests:** `a_still_frame_keeps_the_shadow_map` (two identical frames: 1 build), `a_terrain_change_makes_it_again` (a carve and `update_terrain` between: 2), `a_moved_beam_makes_it_again` (a live beam turned: 2), `two_views_make_the_map_once` (`render_views` of two views, three frames of a turning beam: 3 builds, not 6).

- [ ] **Step 2: Run** — FAIL.

- [ ] **Step 3: Implement** the keep: `encode` returns early when the rows' beams equal the last built and nothing marked it stale; `LookState::write` (any rectangle), `upload` and `sync_glow_beams` mark it stale.

- [ ] **Step 4: Run** the GPU suite and `cargo xtask check --render` — PASS.

- [ ] **Step 5: Commit** `olr-render: keep the beam shadow map while nothing it reads changes (#100)`.

---

### Task 5: The many-beam scene and the measurement

**Files:**
- Modify: `crates/olr-cli/src/render_profile.rs`, `render_profile_cmd.rs`
- Test: `render_profile.rs`' unit tests

**Interfaces:**
- Produces: `render-profile --beams` (the many-beam scene, needs `--at`), `--views 1|2` (two views through `gpu_bench::view_areas` and `draw`'s second list), `--no-beam-light` (`Renderer::set_glow_beam_light(false)`: the baseline the beam work is measured against); `pub fn many_beams(at: (f32, f32), tick: u32, alpha: f32) -> Vec<GlowBeamSource>` and the `FrameGroup` they use (appended to a clone of the frame's `LookFrame`, the beams' `group` its value). The scene names `--beams`, `--views` and `--no-beam-light`, so bases compare only like with like.

- [ ] **Step 1: Write the failing unit tests:** `many_beams_carry_the_specs_load`: 12 beams: 6 of spread 40° (radians) reach 120 identical at ticks 0 and 50; 2 of 10° reach 400 whose directions differ between ticks 0 and 1 and not between alphas of one tick; 4 of 20° reach 200 whose directions differ between alphas 0.0 and 0.5 of one tick; all within 200 × 90 world px of `at`.

- [ ] **Step 2: Run** `cargo nextest run -p olr-cli render_profile` — FAIL.

- [ ] **Step 3: Implement** `many_beams` and the flags; each drawn frame clones the presentation's `LookFrame`, appends the group and `many_beams(at, tick, alpha)` when `--beams`.

- [ ] **Step 4: Run** the tests and `cargo xtask check` — PASS.

- [ ] **Step 5: Commit** `olr-cli: render-profile --beams, --views 2 and --no-beam-light (#100)`.

---

### Task 6: Measure, tune to the budget, record

**Files:**
- Modify: `beam_shadow.rs` / `beam_shadow.wgsl` (only if tuning is needed), `docs/porting/STATUS.md`, `crates/olr-render/CLAUDE.md`, `crates/olr-cli/CLAUDE.md`, `CHANGELOG.md`

- [ ] **Step 1: Measure** on the integrated GPU and the RTX, each with and without `--no-beam-light`:
  `cargo xtask profile -- --adapter integrated --level olr-level-liero-factory-reloaded --at 870,300 --sizes 1920x1080 --ticks 200..500 [--no-beam-light]`, the same with `--beams --views 1` and `--views 2`; on the RTX (default adapter) also `--ticks 500..700`. Beam work = mean GPU frame minus its `--no-beam-light` twin.

- [ ] **Step 2: If over the budget**, tune in this order, re-measuring after each: rows only for beams whose wedge meets some view's light grid; fewer rays for rows whose `limit` is short (keeping ≤ 1 px apart at `limit`); keep each beam's rows across frames while it and the terrain in its reach are unchanged. Each keeps the GPU suite passing.

- [ ] **Step 3: Record** in `docs/porting/STATUS.md` (beside "The beacon's beams") the method and every number of Step 1; `crates/olr-render/CLAUDE.md`'s "Glow beams" the map, its pass, its keep and its tests; `crates/olr-cli/CLAUDE.md` the new flags; `CHANGELOG.md` one line (wide beams cost the same however much they light).

- [ ] **Step 4: Run** `cargo xtask check --render` — PASS.

- [ ] **Step 5: Commit** `docs: beam shadow maps' cost and method (#100)`.
