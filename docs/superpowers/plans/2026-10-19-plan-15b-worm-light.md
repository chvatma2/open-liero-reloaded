# Plan 15b: Worms in the Light Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In a lit match, worms show at a lower floor in the level's hue, block light like rock, and are lit on the side a light comes from.

**Architecture:** The scene lists every living worm's body frame as an *occluder* in the draw list. The renderer draws them each frame into a world-pixel `R8Uint` occluder map that covers the light grid (each texel holds a worm id + 1, or 0). The shadow pass and the glow light's emission pass treat occupied texels as solid, except a light ignores the worm its centre sits in. The composite lights a worm pixel with the light just outside the worm on that pixel's side, and never below the worm floor. That floor is a new globals colour, picked by the shade's floor kind.

**Tech Stack:** Rust, wgpu 30 (WGSL), egui-free. Crates: `olr-look`, `olr-scene`, `olr-fx`, `olr-render`, `olr-present`.

**Spec:** `docs/superpowers/specs/2026-10-19-worm-lighting-design.md` (on top of `docs/superpowers/specs/2026-10-18-look-system-design.md`).

**Branch:** `plan-15b-worm-light`, cut from `plan-15a-look` once plan 15a's Task 21 and its final review are done (15a's ledger, `.superpowers/sdd/2026-10-18-plan-15a-look-system/progress.md`).

## Global Constraints

- The oracle and the match goldens stay byte-identical. Render goldens are re-blessed only with a reason (`cargo xtask bless-render --reason "<why>"`).
- The simulation stays `no_std` and wasm-clean: nothing here touches `olr-sim`.
- No per-player visual settings that change information.
- Ask before running `cargo xtask full`, benchmarks or stress runs (the user shares the PC).
- GL limits: no `R32Float` or `Rg32Float` render targets; `R8Uint` is fine (the solid map already is one).
- Worm floor: 35 % (`olr_look::engine::WORM_FLOOR = 35`) of the ambient hue at full brightness; black ambient → white; minimum luminance half of 35 % white's.
- Projectiles and bonuses keep `olr_look::engine::FLOOR` (50 % white). The crosshair and health bars stay markers.
- Only living worms occlude. A light ignores the worm its centre is in. A light inside rock still lights nothing.
- The shading is per world pixel, never finer than the art; a steady scene draws the same bytes twice.
- Cite the spec in doc comments for new behaviour ("the user's playtest 2026-10-19" or "plan 15b").
- `cargo xtask check` per batch (none for docs-only commits).

## Review Focus

1. **A worm just outside the view.** A worm between an off-view light (inside `LIGHT_CULL_PAD`) and the view should still throw its shadow into the view. Pinned by `an_off_view_worm_still_shadows_the_view` (Task 2).
2. **A worm pressed against rock.** Its facing reads light from a texel that is rock, so the floor and soak decide. It must never be black and never panic. Pinned by `a_worm_against_a_wall_keeps_its_floor` (Task 4).
3. **Many and overlapping worms.** With 32 worms, some overlapping and ids up to 32, there should be no panic; a muzzle light ignores only its own worm. Pinned by `overlapping_worms_block_each_others_lights` (Task 2).
4. **A mod with a look on a level without one.** The ambient is white at 100 %, so the worm floor is 35 % white. Pinned by `worm_floor_hues` (Task 1).
5. **The GL backend.** The occluder pass must draw on GL (lavapipe's GL or llvmpipe). Pinned by extending `a_lit_frame_draws_on_gl` (Task 2).

---

### Task 1: The worm floor

**Files:**
- Modify: `crates/olr-look/src/engine.rs` (`WORM_FLOOR`), `crates/olr-look/src/frame.rs` (`LookFrame::worm_floor`, `worm_floor_of`)
- Modify: `crates/olr-scene/src/draw_list.rs` (`Shade`: `solid_floor: bool` becomes `floor: Floor`), `crates/olr-scene/src/worm.rs`, `crates/olr-scene/src/lib.rs`
- Modify: `crates/olr-fx/src/draw.rs`, `crates/olr-render/src/frame.rs` (`shade()`), `crates/olr-render/src/look/common.wgsl`, `crates/olr-render/src/look/composite.wgsl`, `crates/olr-render/src/renderer/look.rs` (globals)
- Test: `crates/olr-look/tests/frame.rs`, `crates/olr-render/tests/gpu.rs`, `crates/olr-present/tests/look.rs`, `crates/olr-fx/tests/entities.rs`

**Interfaces:**
- Produces:
  - `olr_look::engine::WORM_FLOOR: u8 = 35`.
  - `olr_look::worm_floor_of(ambient_srgb: [u8; 3]) -> [f32; 3]` (linear).
  - `LookFrame::worm_floor: [f32; 3]` (linear), set by `begin`: `worm_floor_of(level ambient colour)`, or 35 % white without a level look.
  - `olr_scene::Floor { None, Solid, Worm }`, the default `None`.
  - `Shade { self_light: u8, marker: bool, floor: Floor }`. `Shade::FLOORED` becomes `Shade { floor: Floor::Solid, ..LIT }`; a new `Shade::WORM` is `Shade { floor: Floor::Worm, ..LIT }`.
  - The instance's `shade[2]`: 0 none, 1 solid, 2 worm.
  - WGSL `LookGlobals` gains `worm_floor: vec4<f32>` (rgb, 0) after `solid`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/olr-look/tests/frame.rs
#[test]
fn worm_floor_hues() {
    let k = olr_look::engine::WORM_FLOOR as f32 / 100.0;
    let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-3);
    assert!(near(olr_look::worm_floor_of([255, 255, 255]), [k; 3]), "white");
    assert!(near(olr_look::worm_floor_of([0, 0, 0]), [k; 3]), "black: white");
    // #203048 at full brightness is #6ea5ff (by linear channels, the brightest at 1).
    let f = olr_look::worm_floor_of([0x20, 0x30, 0x48]);
    assert!((f[2] - k).abs() < 1e-3 && f[0] < f[1] && f[1] < f[2], "{f:?}");
    // Pure blue's 35 % is too dark: mixed towards white to half of 35 % white's luminance.
    let b = olr_look::worm_floor_of([0, 0, 255]);
    let lum = 0.2126 * b[0] + 0.7152 * b[1] + 0.0722 * b[2];
    assert!((lum - k * 0.5).abs() < 1e-3 && b[2] > b[0], "{b:?} {lum}");
    // A frame without a level look (a mod's look alone): 35 % white.
    let mut frame = olr_look::LookFrame::default();
    frame.begin(None, 0.0);
    assert!(near(frame.worm_floor, [k; 3]));
}
```

```rust
// crates/olr-render/tests/gpu.rs: beside a_floored_item_never_falls_below_the_solid_floor,
// the same setup with look.worm_floor = [0.2, 0.3, 0.4] and a third dot at x 30
// with Shade::WORM:
//   dot 20 (Shade::FLOORED) shows the solid floor, dot 30 (Shade::WORM) shows
//   model([0.2, 0.3, 0.4]), dot 10 (LIT) the ambient.
#[test]
fn a_worm_item_never_falls_below_the_worm_floor() { /* as above */ }
```

In `crates/olr-present/tests/look.rs`, `floors_apply_only_in_lit_matches`: the skin's sprites in a lit match have `Shade::WORM` (was `self_light: FLOOR`). A projectile keeps `self_light: FLOOR`.

In `crates/olr-fx/tests/entities.rs`, `blood_and_gibs_take_the_solid_floor_when_lit` keeps `Shade::FLOORED` (now `Floor::Solid`): it should pass unchanged once `Shade::FLOORED` is redefined.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo nextest run -p olr-look worm_floor_hues; OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu a_worm_item; cargo nextest run -p olr-present floors_apply`
Expected: compile errors (`worm_floor_of`, `Shade::WORM` and `Floor` do not exist).

- [ ] **Step 3: Implement**

`worm_floor_of`, in linear light:
- `c` = the colour's channels made linear, divided by their maximum; all zero gives `[1.0; 3]`.
- `f = c × WORM_FLOOR/100`.
- If `lum(f) < 0.5 × WORM_FLOOR/100`, mix `f` towards `WORM_FLOOR/100` white until `lum` equals that. That is a linear blend `f + t × (w − f)`, with `t` solved from the luminances.

`begin` sets `worm_floor` from `level.ambient.colour` (the ambient as set, not as graded). Its intensity is ignored.

`Shade`: replace `solid_floor: bool` with `floor: Floor`. The `describe` suffixes become " floored" (Solid) and " worm floor" (Worm). Update every literal (`rtk proxy grep -rn "Shade {" crates`).

`WormFrame::body()` returns `Shade::WORM` in a lit match, else `LIT`. The rope (`draw_rope`) and the hook use `body()` already. `WormFrame::floor: Option<u8>` becomes `lit: bool` (only presence was read for worms). `ShadeRules::floor` stays for projectiles and bonuses.

Composite: let `fl = select(vec3(0), look.solid.rgb, shade.z > 0.5 && shade.z < 1.5)`, or `look.worm_floor.rgb` when `shade.z > 1.5`, then `l = max(l, fl)`. `frame.rs` `shade()` gives 0, 1 or 2. The globals packing in `renderer/look.rs` adds `[wf.r, wf.g, wf.b, 0.0]` after the solid floor; `common.wgsl`'s struct gets the field in the same place.

- [ ] **Step 4: Run them and the crates' tests**

Run: `cargo nextest run -p olr-look -p olr-scene -p olr-fx -p olr-render -p olr-present; OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu`
Expected: all pass.

- [ ] **Step 5: Commit** (`olr: worms take a lower floor in the level's hue`), then `cargo xtask check`.

---

### Task 2: Worms block placed and engine lights

**Files:**
- Modify: `crates/olr-scene/src/draw_list.rs` (`Occluder`, `DrawList::occluders`), `crates/olr-scene/src/lib.rs` (`Scene::build` lists them), `crates/olr-scene/src/worm.rs` (the body's frame, factored out)
- Create: `crates/olr-render/src/look/occluder.wgsl`
- Modify: `crates/olr-render/src/frame.rs` (`occluder_instances`), `crates/olr-render/src/renderer/look.rs` (the occluder target, pipeline and bindings), `crates/olr-render/src/renderer/pass.rs` (the pass, before GI), `crates/olr-render/src/look/common.wgsl` (`occluders` globals, `worm_at`), `crates/olr-render/src/look/shadow.wgsl`
- Test: `crates/olr-render/tests/gpu.rs`, `crates/olr-present/tests/look.rs`, `crates/olr-scene` unit tests

**Interfaces:**
- Consumes: Task 1's globals layout.
- Produces:
  - `pub struct Occluder { pub atlas: AtlasId, pub src: Rect, pub pos: [f32; 2], pub size: [f32; 2], pub flip: bool, pub worm: u8 }`, where `worm` is the worm index + 1, at most 255.
  - `DrawList::occluders: Vec<Occluder>`, cleared by `DrawList::clear` (capacity kept: no per-frame allocation, as the no-alloc tests require).
  - WGSL: `LookGlobals.occluders: vec4<f32>` holds (origin x, origin y) in world pixels and (width, height) in texels. `fn worm_at(w: vec2<i32>) -> u32` gives 0 outside the map.
  - The renderer's occluder texture, `R8Uint`, one texel per world pixel, covers the light grid: origin = frame origin − `LIGHT_PAD`, size = the light grid's.

- [ ] **Step 1: Write the failing tests**

GPU tests (`gpu.rs`). Each uses `room` and `lamp`, a list whose `occluders` hold a solid 4 × 6 world-pixel occluder made from an uploaded opaque image, and `light_at` to read the light:

- `a_worm_shadows_the_wall_behind_it`: `ground()` (rock from y 20 down) with the lamp at (50.5, 10.5). An occluder at x 50..54, y 13..19, worm 1. `light_at(50, 21)`, the rock face under the worm, is lower than without the occluder and equals the ambient within 1e-3 under hard shadows. `light_at(30, 21)` is unchanged.
- `a_light_inside_a_worm_ignores_it`: the lamp at the occluder's centre, radius 1. Air around the occluder on every side, 3 px out, is lit above the ambient + 0.1.
- `overlapping_worms_block_each_others_lights`: two occluders, worm 1 and worm 2, side by side touching, and a lamp inside worm 1. The far side of worm 2 is at the ambient; the near side of worm 1 is lit. Then 32 occluders with ids 1..32 at random seeded positions: no panic, the frame draws twice to the same bytes.
- `an_off_view_worm_still_shadows_the_view`: the view at x 100..200. The lamp at x 90, outside the view but within `LIGHT_CULL_PAD`, and the occluder at x 94..98. The view's rock face at x 101 behind it is at the ambient.
- Extend `a_lit_frame_draws_on_gl`: add one occluder; the frame draws without a validation error.

Scene test (`crates/olr-present/tests/look.rs`): `living_worms_are_occluders`. In a lit `dig` match, `list.occluders` holds one entry per living worm, with `worm = i + 1`, `src` equal to the skin sprite's `src`, and `flip` the same. A worm that is dead or not yet spawned has none. An unlit match has none.

- [ ] **Step 2: Run them and watch them fail**

Run: `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu worm; cargo nextest run -p olr-present living_worms`
Expected: compile errors (no `occluders`).

- [ ] **Step 3: Implement**

- **Scene:** after drawing the worms, in a lit match only, push an `Occluder` for each living worm. Use the same frame `skin_frame`/`clip` and position as its body sprite, whatever the view (the renderer culls).
- **Renderer:**
  - `occluder_instances` maps each occluder to an `Instance`: dst in occluder-texture texels (world pixels relative to the map's origin), `shade[0] = worm`.
  - `occluder.wgsl`: the vertex shader as `world_lit.wgsl`'s. `fs_occluder` reads the atlas texel and discards alpha ≤ 0, else writes `u32(round(shade.x))`.
  - The map is cleared to 0 each lit frame and drawn before the GI and shadow passes. Bind it to the shadow, emission (Task 3) and composite (Task 4) groups.
- **Shadow pass:** `let own = worm_at(light centre)`. `solid_at(p)` becomes `solid(p) || (worm_at(p) != 0 && worm_at(p) != own)`.

- [ ] **Step 4: Run them and the crates' tests**

Run: `cargo nextest run -p olr-scene -p olr-render -p olr-present; OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu`
Expected: all pass.

- [ ] **Step 5: Commit** (`olr: living worms block lights`).

---

### Task 3: Worms block glow light

**Files:**
- Modify: `crates/olr-render/src/look/gi_emission.wgsl`, `crates/olr-render/src/renderer/gi.rs` (bind the occluder map)
- Test: `crates/olr-render/tests/gpu.rs`

**Interfaces:**
- Consumes: Task 2's occluder map and `worm_at`.

- [ ] **Step 1: Write the failing test**

`a_worm_blocks_glow_light`: `glowing_room` with a glowing column at x 10 and floor rock at y ≥ 30, and `glow_look(1.0, 64.0)`. An occluder at x 20..24, y 22..29, beside the column. The light at (26, 27), behind the worm, is lower than without the occluder by at least 0.05. The light at (18, 27), in front of it, changes by less than 0.02.

- [ ] **Step 2: Run it and watch it fail**

Run: `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu a_worm_blocks_glow`
Expected: FAIL (the light behind the worm is unchanged).

- [ ] **Step 3: Implement:** in `fs_emission`, a world pixel `w` of the cell counts as solid when `worm_at(w) != 0` too. The occluder map's origin and size are in `look.occluders`; cells outside it have no worms.

- [ ] **Step 4: Run** the GPU tests. Expected: all pass.

- [ ] **Step 5: Commit** (`olr-render: worms block glow light`), then `cargo xtask check`.

---

### Task 4: The lit side

**Files:**
- Modify: `crates/olr-render/src/look/composite.wgsl` (bind the occluder map), `crates/olr-render/src/renderer/look.rs` (the composite bind group)
- Test: `crates/olr-render/tests/gpu.rs`

**Interfaces:**
- Consumes: Task 1's `Floor::Worm` shade (`shade.z > 1.5`), Task 2's map.

- [ ] **Step 1: Write the failing tests**

Each draws one 4 × 6 occluder and the same shape as a `Shade::WORM` white sprite at the same place, in a dark room (ambient 0.05, worm floor 0.2):

- `a_worm_is_lit_on_the_lamps_side`: the lamp 6 px left of the worm. The worm's leftmost column of output pixels is brighter than its rightmost by at least 30 (sRGB). The rightmost shows the worm floor (`model([0.2; 3])` within 1).
- `a_worm_in_anothers_shadow_keeps_its_floor`: a second worm between the lamp and the first. Every pixel of the far worm is at least `model([0.2; 3])` − 1.
- `a_worm_against_a_wall_keeps_its_floor`: the worm touching rock on its right, the lamp on its left. No pixel is below the floor; the left column is lit.
- `worm_shading_is_steady`: the same frame drawn twice gives equal bytes. The worm moved by one world pixel: each column's brightness changes by at most the difference between two neighbouring columns of the first frame.
- `a_rope_pixel_has_no_side`: a `Shade::WORM` pixel with no occluder under it shows `max(light here, worm floor)`, the plain composite.

- [ ] **Step 2: Run them and watch them fail**

Run: `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu worm`
Expected: `a_worm_is_lit_on_the_lamps_side` fails (both sides equal); the floor tests may pass already.

- [ ] **Step 3: Implement** in `fs_composite`, for a texel with `shade.z > 1.5` whose world pixel `w` has `id = worm_at(w) != 0`:
  - The outward direction `n`: the normalised sum, over the 7 × 7 window around `w`, of `w − q` for every `q` with `worm_at(q) == id`. When it is near zero, `n = (0, −1)` (up).
  - Step from `w` along `n` in whole world pixels (at most 4) to the first `q` with `worm_at(q) != id`. Read the light there with `light_at`, one pixel further out.
  - Use that light in place of `light_at(g)`, then take the max with self-light and the worm floor as before.

  Every texel of one world pixel shares the result: per world pixel, never finer than the art.

- [ ] **Step 4: Run** the GPU tests and `cargo nextest run -p olr-render`. Expected: all pass.

- [ ] **Step 5: Commit** (`olr-render: worms are lit on the side a light comes from`).

---

### Task 5: Goldens, docs and the playtest build

**Files:**
- Modify: `fixtures/render/look-*.png` (re-blessed), `fixtures/blessings.log`
- Modify: `docs/porting/STATUS.md` (look decisions: the worm floor, occluders, the lit side), `docs/superpowers/specs/2026-10-19-worm-lighting-design.md` (status: implemented, with the plan's rulings), `crates/olr-render/CLAUDE.md`, `crates/olr-scene/CLAUDE.md`, `crates/olr-present/CLAUDE.md`, `CHANGELOG.md` (Unreleased: worms in the light)

- [ ] **Step 1:** Run `OLR_GPU_TESTS=1 cargo nextest run -p olr-cli --test render_golden`. Expected: the look goldens differ (worms now darker and occluding); the unlit goldens pass.
- [ ] **Step 2:** Run `cargo xtask bless-render --reason "plan 15b: worms take the worm floor, block light and are lit on the light's side"`. Then view each changed PNG: a worm's lit side faces a light, and no worm is black.
- [ ] **Step 3:** Update the docs listed above. Run `cargo xtask check`. Expected: no failures.
- [ ] **Step 4:** Ask the user before `cargo xtask full` and before a `gpu-bench` measurement of the occluder pass and the composite's facing lookups. Run them only with consent.
- [ ] **Step 5:** Commit (`olr: worms in the light, docs and goldens`). Build `cargo xtask dist` and tell the user the paths and what to try: a worm walking past a lamp, a worm shadowing another, a muzzle flash in the dark.
