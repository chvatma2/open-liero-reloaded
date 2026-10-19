# Plan 15c: Glowing Light Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Light comes only from glowing pixels and glowing discs through the radiance cascades. That gives soft shadows from shapes, bounce, independent flicker and glare. Point lights are removed. There is an idle bot, and a daylight Castle Strike as the showcase.

**Architecture:**

- **Emitters.** The glow light (`olr-render`'s `gi.rs`, `gi_*.wgsl`) becomes the only light model. Its emission takes the level's glow plus a per-frame layer of dynamic discs (engine and mod lights). Its cascade rays take bounce from the last frame's light at the solid cells they hit.
- **What goes.** The shadow-map pass, the placed lights and `shadows` are removed: renderer, `olr-look`, `olr-format`, `olr-edit`, `olr-cli`, the editor.
- **The light grid.** It keeps soak, the blur that stops at walls, and the composite with its floors. It gains a tone curve and a bloom.

**Tech Stack:** Rust, wgpu 30 (WGSL). Crates: `olr-bots`, `olr-ui`, `olr-online`, `olr-game`, `olr-cli`, `olr-look`, `olr-format`, `olr-edit`, `olr-editor`, `olr-present`, `olr-render`.

**Spec:** `docs/superpowers/specs/2026-10-19-glowing-light-design.md`. Its "My calls" section is binding. Background: `2026-10-19-lighting-evolution-brainstorm.md`, `2026-10-18-look-system-design.md`, `2026-10-19-worm-lighting-design.md`.

**Branch:** `plan-15c-glowing-light`, cut from `main` at the commit that adds this plan.

## Global Constraints

- **Byte-identical:** the oracle, the match goldens and the unlit render
  goldens. Look goldens are re-blessed only with a reason (`cargo xtask
  bless-render --reason "<why>"`).
- **The simulation stays `no_std`** and wasm-clean. Nothing here touches
  `olr-sim`'s logic. The idle bot lives in `olr-bots`.
- **No per-player visual settings and no quality tiers.** Every player
  sees the same frame.
- **GL limits:** no `R32Float` or `Rg32Float` render targets.
  `Rgba16Float`, `Rgba8Unorm` and `R8Uint` are fine. Large values are
  split into f16 halves (`split`/`joined`).
- **Light never passes opaque rock or dirt, however thin.** That
  includes bounced light. A clear solid lets light through as today.
- **The worm floor** (`WORM_FLOOR` 35 % of the ambient hue) and the lit
  side of worms hold over every new light.
- **Camera independence:** moving the view never changes the light of what
  stays in view (`light_is_camera_independent`).
- **Steady scenes:** steady glow with no animation draws the same bytes
  frame after frame once settled. Flicker comes only from animations.
- **Compatibility breaks are fine:** a level file with a `Light` object
  or `shadows` is an error naming the field.
- **Consent first:** ask before `cargo xtask full`, any benchmark, stress
  run or `gpu-bench`, and before `cargo xtask dist`.
- **`cargo xtask check` per batch** of tasks; none after docs-only commits.
- **Cite the spec in doc comments** for new behaviour ("plan 15c").

## Review Focus

1. **A huge explosion** (size 500, reach 4096): its disc radius and
   radiance stay finite and bounded, with no NaN and no blown-out
   texture. Pinned by `huge_and_hostile_emitters_stay_finite` (Task 2).
2. **A busy match** (thousands of explosions and shots at once): at most
   `MAX_EMITTERS` are drawn, the same ones in the same order every time;
   no panic, no per-frame allocation. Pinned by
   `emitters_past_the_cap_change_nothing` (Task 2).
3. **Bounce off white walls everywhere** (albedo 1): the feedback
   converges and never grows without bound. Pinned by
   `bounce_never_runs_away` (Task 5).
4. **A new or restarted level, a big dig, or the editor changing glow
   mid-session:** no history or region map of the wrong size is ever
   read, and the light starts afresh. Pinned by
   `a_new_level_starts_its_light_afresh` (Task 5) and
   `regions_follow_glow_edits` (Task 6).
5. **A dynamic light just off the view, kept by its reach:** it lights the
   view exactly as if the camera had moved to include it. Pinned by
   `an_off_view_flash_lights_the_view_the_same` (Task 2).

---

### Task 1: The idle bot

**Files:**
- Modify: `crates/olr-bots/src/config.rs` (`Personality::Idle`), `bot.rs` (think, intent, respawn arms), `rng.rs` (stream id), `weapons.rs`, `loadout.rs`
- Modify: `crates/olr-cli/src/spec.rs` (`#[value(name = "idle")] Idle`, mapping, file tag `"idle"`)
- Modify: `crates/olr-ui/src/match_prefs.rs` (`BotPersonality::Idle`, `ALL`, `personality()`), `crates/olr-ui/src/screens/local_play.rs` (combo label `"Idle (does nothing)"`)
- Modify: `crates/olr-online/src/matches.rs` (`PERSONALITIES` gains `Idle` at wire index 2), `crates/olr-game/src/net.rs` (both mappings)
- Test: `crates/olr-bots` unit tests, `crates/olr-cli/tests/cli.rs`, `crates/olr-ui/tests/flows.rs`

**Interfaces:**
- Produces: `olr_bots::Personality::Idle`, `olr_ui::BotPersonality::Idle`, and `--personality idle` (olr-cli).

- [ ] **Step 1: Write the failing tests**
  - `an_idle_bot_presses_nothing` (olr-bots): an idle bot in a seeded Castle Strike match with 3 other bots thinks for 2000 ticks and returns `Keys(0)` every tick, through a respawn.
  - `idle_is_a_personality_choice` (olr-ui flows): Local Play's personality combo offers "Idle (does nothing)", and `MatchPrefs::options` gives `Personality::Idle`.
  - `match_takes_idle_bots` (olr-cli): `olr-cli match --personality idle --bots 2 --ticks 500` succeeds and verifies.
- [ ] **Step 2: Run them.** Expected: compile errors (no `Idle`).
- [ ] **Step 3: Implement.**
  - `Idle` thinks to `Keys(0)` without touching its rng or the area cache.
  - Its loadout is the Deathmatch default.
  - Every existing `match` on `Personality` gets the arm.
  - The online wire index 2 is new; the old indices are unchanged.
- [ ] **Step 4: Run** `cargo nextest run -p olr-bots -p olr-ui -p olr-cli -p olr-online -p olr-game`. Expected: all pass. The match goldens are unchanged, since no golden uses `Idle`.
- [ ] **Step 5: Commit:** `olr-bots, olr-ui: an idle bot that does nothing`.

---

### Task 2: Dynamic lights as glowing discs

**Files:**
- Modify: `crates/olr-look/src/frame.rs` (`LookFrame::emitters`, `MAX_EMITTERS`, `push_emitter`, cull), `crates/olr-look/src/engine.rs` (`disc_radius`, `disc_radiance`, `explosion_radius`)
- Modify: `crates/olr-present/src/look.rs` (push emitters instead of lights)
- Create: `crates/olr-render/src/look/gi_dynamic.wgsl` (discs into the dynamic emission layer)
- Modify: `crates/olr-render/src/renderer/gi.rs`, `look/gi_emission.wgsl`, `renderer/look.rs`, `renderer/pass.rs` (`casting` also when emitters exist)
- Test: `crates/olr-look/tests/frame.rs`, `crates/olr-present/tests/look.rs`, `crates/olr-render/tests/gpu.rs`

**Interfaces:**
- Produces:
  - `olr_look::MAX_EMITTERS: usize = 1024`.
  - `LookFrame::emitters: Vec<FrameLight>`; `LookFrame::push_emitter(FrameLight)`. `cull` treats `emitters` as it treats `lights`.
  - `olr_look::engine::disc_radius(reach: f32) -> f32`: `(reach / 8.0).clamp(1.0, 6.0)`, and 1.0 for a non-finite reach.
  - `olr_look::engine::explosion_radius(size: i32) -> f32`: `(2 + size / 2).clamp(2, 12)` as f32, size clamped to 0..=500.
  - `olr_look::engine::disc_radiance(colour: [f32; 3], radius: f32, reach: f32) -> [f32; 3]`. It is the calibration (spec, "My calls"): today's point light at `d0 = max(reach / 3, 2·radius)` in the open, `colour · fade(d0 − radius, reach)`, equals the disc's light there, `L · min(1, 2·radius / (TAU · d0)) · fade(d0, reach)`. Solved for `L`, finite, at most `colour · 64`.
  - Renderer: a per-frame dynamic emission target (`Rgba16Float`, grid-sized: rgb radiance; and a reach target with max blending, split halves). `fs_emission` adds them; a cell lit by a disc is not counted as a worm.

- [ ] **Step 1: Write the failing tests**
  - `disc_calibration_matches_todays_light` (olr-look): for (colour 1, reach 40, radius 5) and (reach 300, radius 6), `disc_radiance(..)` × the disc formula at `d0` equals `fade(d0 − r, reach)` within 1e-4; a reach of `INFINITY` and a radius of 0 give finite values.
  - `huge_and_hostile_emitters_stay_finite` (olr-render GPU): emitters of size-500 explosions, NaN positions, infinite reach and zero colour in one frame. The light grid read back has no NaN or inf, and the frame draws.
  - `a_flash_lights_its_surroundings_softly` (GPU): one emitter (reach 60, radius 4) in an open room. The light at `d0` is within 25 % of today's point light there. Across a 1-pixel wall in front of it, the light behind is the ambient (`a_one_pixel_wall_stops_light_right_behind_it`'s check).
  - `a_worm_casts_a_soft_shadow` (GPU): an emitter of radius 4, a 4 × 6 worm 10 px away, rock 20 px behind it. Along the rock face, at least 3 pixels lie strictly between the lit level and the shadow's level on each side of the shadow: no hard column.
  - `emitters_past_the_cap_change_nothing` (GPU): `MAX_EMITTERS + 500` emitters, then the first `MAX_EMITTERS` alone, give equal light.
  - `an_off_view_flash_lights_the_view_the_same` (GPU): an emitter 15 px left of the view (inside `LIGHT_CULL_PAD`). The view's light equals that of a frame whose view includes it.
  - `engine_lights_are_emitters` (olr-present): in a lit bazooka match, explosions and muzzle flashes appear in `look().emitters`, not in `lights`, with radii from `explosion_radius` and `disc_radius`.
- [ ] **Step 2: Run them.** Expected: compile errors (`emitters`, `disc_radiance`).
- [ ] **Step 3: Implement.**
  - `Dynamic::push` builds the same `FrameLight`s, with radius from `explosion_radius` (explosions) or `disc_radius` (the rest), and calls `push_emitter`.
  - The renderer packs the first `MAX_EMITTERS` emitters (pos, radius, radiance from `disc_radiance`, reach) and draws each as a quad over its disc's cells into the dynamic layer before the emission pass, additive blending for rgb. The world-pixel grid is fixed to the level (camera-independent).
  - Muzzle lights' `owner` is no longer needed: a disc's cells stop counting as the worm.
- [ ] **Step 4: Run** `cargo nextest run -p olr-look -p olr-present; OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu`. Expected: all pass. Point-light tests still pass (level lights unchanged here).
- [ ] **Step 5: Commit:** `olr: play's lights are glowing discs in the glow light`. Then run `cargo xtask check`.

---

### Task 3: No point lights in the data and the tools

**Files:**
- Modify:
  - `crates/olr-look/src/lib.rs`: remove `PlacedLight`, `Shadows`, `LevelLook::{lights, shadows}`.
  - `crates/olr-look/src/frame.rs`: remove `shadow_filter` and the level-light loop in `begin`; `LookFrame::lights` goes.
  - `crates/olr-format/src/levels/mod.rs`: remove `LevelObject::Light`, `light`, `as_light`, the light checks in `compile_look`. Also `MAX_LIGHT_OBJECTS`.
  - `crates/olr-format/src/look.rs`: remove `ShadowsSource`, `LookSource::shadows`. `LightSource` stays for mod looks; edit its doc.
  - `crates/olr-format/src/levels/thumb.rs`: no placed lights.
  - `crates/olr-edit/src/look.rs`, `level.rs`: remove `add_light`, `set_light` and the resize note.
  - `crates/olr-cli/src/look_cmd.rs`, `level_cmd.rs`, `look_preview.rs` (`--shadows`), `gpu_bench.rs` (`bench_look`: glowing strips and play's discs only), `render_match.rs` (print emitters).
  - `crates/olr-editor/src/{tools.rs, pointer.rs, look_ops.rs, state.rs, document.rs, clipboard.rs, ui/look.rs, ui/mod.rs, ui/properties.rs, ui/elements.rs, ui/canvas.rs}`: no Light tool (K), no light objects, no Shadows combo.
  - `crates/olr-game/src/editor/canvas.rs`: `lit()` from the look alone.
  - `fixtures/look/levels/look-test/level.ron`, and its `look/` glow: the two lamps become glowing patches of a new group `lamps` at (200, 150) and (300, 150), radius 4, one steady and one pulsing as before.
  - `content/playtest/olr-level-castlestrike-night/level.ron`: drop the light objects.
  - `docs/formats/*.md`: regenerate with `cargo xtask format-docs`.
- Test: the tests listed in "Tests" below are removed or rewritten.

**Interfaces:**
- Consumes: Task 2's `LookFrame::emitters`.
- Produces: `LookFrame` without `lights` or `shadow_filter`, and `LevelLook` without `lights` or `shadows`.

- [ ] **Step 1: Write the failing tests**
  - `a_light_object_is_an_error` (olr-format `tests/look.rs`): a `level.ron` with a `Light(..)` object fails to read, the error naming `objects`.
  - `shadows_is_an_unknown_field` (olr-format): a `look: (shadows: Hard)` fails, naming `shadows`.
  - `the_editor_has_no_light_tool` (olr-editor `tests/look.rs`): `Tool::ALL` holds no tool named "Light", and K selects nothing.
- [ ] **Step 2: Run them.** Expected: FAIL (both parse; the tool exists).
- [ ] **Step 3: Remove** everything listed under Files.
  - **Tests:** remove `shadows_default_to_soft`, `lights_round_trip`, `a_resize_keeps_lights_outside_the_level`, `groups_and_lights_round_trip_through_the_cli`, `lights_place_move_and_undo`, `place_a_light_and_edit_it`, and the lights half of the copy/paste test.
  - **Fixtures:** in `begin_fills_level_part`, `look_frame_level_part`, `the_light_cap_is_deterministic`, `look_frame_does_not_allocate_when_warm` and the spec example, drop the lights and `shadows` and keep the rest.
  - **The CLI thumbnail test** adds a glow group instead of a light.
- [ ] **Step 4: Run** `cargo nextest run --workspace` (no GPU). Expected: all pass. The renderer's placed-light path is now fed nothing.
- [ ] **Step 5: Commit:** `olr: glowing pixels are a level's only light`.

---

### Task 4: The renderer without shadow maps

**Files:**
- Remove: `crates/olr-render/src/look/shadow.wgsl`
- Modify:
  - `crates/olr-render/src/renderer/look.rs`: no shadow pipeline, layout, texture, lights buffer or `light_count`; `LIGHT_FLOATS` goes.
  - `renderer/pass.rs`: no shadow pass.
  - `look/light.wgsl`: `fs_sum` is `ambient + glow_at(c)`; `placed`, `visible`, `lit_on`, `ray_to` and `past_rock` go. `solid_px` stays for the blur.
  - `look/common.wgsl`: `Lights` and `SHADOW_RAYS` go; `mode.w` and `frame.w` are unused.
- Test: `crates/olr-render/tests/gpu.rs`

**Interfaces:**
- Consumes: Task 2's dynamic layer, Task 3's `LookFrame`.
- The test helper `lamp(at, reach) -> FrameLight` stays (radius 2), pushed with `push_emitter`.

- [ ] **Step 1: Rewrite the tests** (the helpers `lit_frame`, `lit_frame_with`, `worm_frame` and `ground` push emitters).
  - **Keep:** `a_wall_blocks_a_light`, `reach_bounds_a_light`, `an_off_view_light_reaching_the_view_lights_it`, `a_light_against_a_wall_lights_the_room` (a light inside rock lights nothing), `light_is_camera_independent`, the soak and floor tests, `look_softness_blurs`, `a_lamp_shines_through_a_clear_wall`, `a_lit_frame_draws_on_gl`, the plan 15b worm tests, `a_one_pixel_wall_stops_light_right_behind_it` and `a_diagonal_wall_one_pixel_thin_stops_light` (no filter loop).
  - **Remove:** `soft_shadows_have_a_penumbra`, `a_far_unlimited_light_ends_its_shadow_march` and `lights_past_the_cap_change_nothing` (Task 2's cap test replaces it).
  - **Change:** where a value was exact from a point light, assert the side of the threshold instead (lit `> ambient + 0.1`, shadow within 1e-3 of the ambient).
- [ ] **Step 2: Run** `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu` before removing the pass. Expected: the rewritten tests pass or fail only where a placed light still draws: none should be pushed.
- [ ] **Step 3: Remove** the shadow pass and the placed path.
- [ ] **Step 4: Run** the GPU tests and `cargo nextest run -p olr-render`. Expected: all pass. Then run `OLR_GPU_TESTS=1 cargo nextest run -p olr-cli --test render_golden`. Expected: only the look goldens differ. Look at each changed PNG: the lamps are now soft glowing patches.
- [ ] **Step 5: Commit:** `olr-render: one light model, no shadow maps`. Then run `cargo xtask check`.

---

### Task 5: Bounce

**Files:**
- Modify:
  - `crates/olr-render/src/renderer/gi.rs`: an albedo target (`Rgba8Unorm`, grid-sized, written by `fs_emission`); two fluence textures used alternately, with `last` read by the cascades.
  - `look/gi_emission.wgsl`, `look/gi_cascade.wgsl`: at a solid, unlit cell, `ray.rgb = albedo × last_fluence(cell) × bounce`.
  - `look/gi.wgsl`: drop the stale `fs_down` comment.
  - `crates/olr-look/src/engine.rs`: `BOUNCE: f32 = 0.6`.
  - `crates/olr-look/src/frame.rs`: `LookFrame::bounce: f32`, set by `begin` to `engine::BOUNCE`; 0 in a frame without a look.
- Test: `crates/olr-render/tests/gpu.rs`

**Interfaces:**
- Produces:
  - `olr_look::engine::BOUNCE`, `LookFrame::bounce` (the renderer clamps it to 0..=0.9).
  - `Renderer::reset_light_history()`, also called by `upload_terrain`.

- [ ] **Step 1: Write the failing tests**
  - `light_bounces_round_a_corner` (GPU): an L-shaped corridor, the emitter in one leg; a point in the other leg that no ray from the emitter reaches directly. After 30 frames with `bounce` 0.6, its light is above the ambient + 0.02; with `bounce` 0, it is the ambient within 1e-3.
  - `bounce_tints_with_the_wall` (GPU): the same with red art on the corner wall. The bounced light's red exceeds its green by at least 2×.
  - `bounce_never_runs_away` (GPU): a closed white room (albedo 1) with one emitter, 200 frames. Every texel stays finite and below 4 × the emitter's direct light at 2 px.
  - `bounce_settles_to_steady_bytes` (GPU): a steady scene, frames 60 and 61 read back equal.
  - `no_bounce_through_a_thin_wall` (GPU): a lit room and a dark room sharing a 1-pixel wall, 30 frames. The dark room is at the ambient within 1e-3.
  - `a_new_level_starts_its_light_afresh` (GPU): 30 frames on level A, then `upload_terrain(B)` of another size. B's first frame equals a fresh renderer's first frame on B.
- [ ] **Step 2: Run them.** Expected: `light_bounces_round_a_corner` and `bounce_tints_with_the_wall` fail; the rest may pass.
- [ ] **Step 3: Implement.**
  - The cascade's bounce reads the last frame's fluence at the hit cell bilinearly within that cell only, never across a wall.
  - A frame without a look (`render_lit(None)`) resets the history.
- [ ] **Step 4: Run** the GPU tests. Expected: all pass.
- [ ] **Step 5: Commit:** `olr-render: light bounces off rock and dirt`.

---

### Task 6: Independent flicker

**Files:**
- Modify:
  - `crates/olr-render/src/renderer/look.rs`: a region map (`R16Uint` world-sized, sampled only). Each 4-connected region of glowing world pixels in a `Flicker` group gets an id: 1 + the index of its first pixel in row-major order, folded into 1..=65 535 by `% 65 535 + 1`; 0 elsewhere. It is computed on the CPU in `upload` and in `write` for the dirty rectangles' regions.
  - `look/common.wgsl`: `fn flicker(t_ms, speed, depth, seed) -> f32`, value noise as in `olr_look::anim`, same bucket rule.
  - `look/gi_emission.wgsl`, `look/world_lit.wgsl`: a `Flicker` group's scale per region.
  - `crates/olr-look/src/frame.rs`: `FrameGroup` gains `flicker: Option<(f32, f32)>` (speed, depth) and `t_ms`.
- Test: `crates/olr-render/tests/gpu.rs`

**Interfaces:**
- Produces: `FrameGroup::flicker`. `Pulse` and `Keys` groups keep their group-wide `self_scale` and `cast`.

- [ ] **Step 1: Write the failing tests**
  - `two_flames_flicker_on_their_own` (GPU): one `Flicker` group, two separate 3 × 3 glowing patches 40 px apart. Over 20 clock values, their self-lit brightness differs at 10 or more.
  - `one_flame_flickers_as_one` (GPU): within each patch, every pixel shows the same brightness at every clock value (no seams).
  - `pulse_groups_stay_in_step` (GPU): the same patches in a `Pulse` group are equal at every clock value.
  - `regions_follow_glow_edits` (GPU): join the two patches with a glowing bridge through `update_terrain` of a dirty rectangle. The joined region then flickers as one, and nothing reads out of range.
- [ ] **Step 2: Run them.** Expected: `two_flames_flicker_on_their_own` fails.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the GPU tests. Expected: all pass, with `group_animation_scales_glow` and `no_popping` unchanged.
- [ ] **Step 5: Commit:** `olr-render: each flame flickers on its own`. Then run `cargo xtask check`.

---

### Task 7: Glare

**Files:**
- Modify:
  - `look/light.wgsl`: in `fs_blur_v`, the tone curve replaces the cap.
  - `renderer/look.rs`, `renderer/pass.rs`: bloom, a bright pass of the lit target, a 3-level downsampled blur chain, and an add before the upscale.
  - `look/composite.wgsl`: writes the bright pass's source.
  - `crates/olr-render/src/look/mod.rs`: the CPU model `tone(v, max_light)`.
- Create: `crates/olr-render/src/look/bloom.wgsl`
- Test: `crates/olr-render/tests/gpu.rs`, `crates/olr-render/src/look/mod.rs` (unit)

**Interfaces:**
- Produces: `olr_render::tone(v: f32, max_light: f32) -> f32`.
  - The identity for `v ≤ 1`.
  - Above 1, a smooth shoulder rising to `max_light` as `v → ∞`, continuous with slope 1 at 1:
    `1 + (m − 1)(1 − e^{−(v − 1)/(m − 1)})`, with `m = max_light`, and the identity when `m ≤ 1`.

- [ ] **Step 1: Write the failing tests**
  - `tone_is_the_identity_up_to_one` (unit): `tone(v, m) == v` for v in 0..=1 and m in {1, 1.5, 2}; monotonic, below `m`, continuous at 1.
  - `a_dim_level_draws_as_before` (GPU): a frame whose light stays ≤ 1 everywhere, bloom included, equals the `shade_texel` model (bytes within 1).
  - `bright_light_blooms` (GPU): an emitter driving the light to 2 near it. Pixels 6 px outside its lit area are brighter than with bloom off (the test's internal switch `Renderer::set_bloom(false)`, test-only, doc-hidden).
  - `bloom_is_steady_and_camera_independent` (GPU): two frames give equal bytes; a view moved by 7 px gives equal bloom on the overlap.
- [ ] **Step 2: Run them.** Expected: compile errors (`tone`).
- [ ] **Step 3: Implement.** The bloom adds only light above 1 and glowing pixels' self-light above 1. The light steps band after the tone curve.
- [ ] **Step 4: Run** the GPU tests and `cargo nextest run -p olr-render`. Expected: all pass.
- [ ] **Step 5: Commit:** `olr-render: bright light glows`.

---

### Task 8: Castle Strike by day, docs and goldens

**Files:**
- Create: `content/playtest/olr-level-castlestrike-day/`, made with `olr-cli level …` from `content/packs/olx-level-castlestrike`:
  - a daylight sky glow group of unlimited reach on a copy of the background layer;
  - the cavern torches and candles glowing in a `Flicker` group;
  - an ambient low enough that the halls depend on bounce.
- Modify:
  - `content/playtest/README.md`; `crates/olr-cli/tests/playtest.rs` (both packs).
  - `fixtures/render/look-*.png` (re-blessed), `fixtures/blessings.log`.
  - `docs/porting/STATUS.md`: "Glowing light (plan 15c)": the decisions and the spec's calls as built.
  - The spec's status line.
  - `CHANGELOG.md` (Unreleased).
  - The `CLAUDE.md` of every touched crate: `olr-render`, `olr-look`, `olr-format`, `olr-edit`, `olr-editor`, `olr-cli`, `olr-present`, `olr-bots`, `olr-ui`.

- [ ] **Step 1:** Build the day pack. Render, for the user to judge, with `look-preview` and `render-match --personality idle`:
  - the courtyard;
  - the hall (light from its openings);
  - a tower room;
  - the cellar torches;
  - a worm beside a torch;
  - an explosion in a cavern.

  Retune until each shows the spec's intent. `olr-cli pack check content/playtest --content content/packs` is clean.
- [ ] **Step 2:** Run `OLR_GPU_TESTS=1 cargo nextest run -p olr-cli --test render_golden`, then `cargo xtask bless-render --reason "plan 15c: glowing light only, bounce, flicker and glare"`. View every changed PNG.
- [ ] **Step 3:** Update the docs listed above. Run `cargo xtask check`. Expected: no failures.
- [ ] **Step 4:** Ask the user before `cargo xtask full` and before measuring on Windows at 1080p (a representative idle-or-dm match on the day Castle Strike, not the heavy replay). Run them only with consent.
- [ ] **Step 5:** Commit: `olr: glowing light, docs and goldens`.
