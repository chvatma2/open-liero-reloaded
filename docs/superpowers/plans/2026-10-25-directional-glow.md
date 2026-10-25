# Directional Glow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A glowing element can shine in one direction and spread. Each connected patch of its glowing pixels becomes a crisp, soft-shadowed beam, as wide as the patch and visible in the air as far as the level's haze lets it. Liero Factory Reloaded's side PCs and truck headlights use it, matching the spike's picked frames.

**Architecture:**
- **Format.** `ElementGlow` gains `beam: Option<GlowBeam { direction, spread }>`. The bake writes it per world pixel into `look/glow-beam.png` and `look/glow-beam-back.png`, beside the glow and group maps. `LookSource` gains `haze` (percent), and `look/haze.png` is an optional per-world-pixel scale of it.
- **Derivation.** `olr_look::glow_beams::derive` turns the maps into beam sources (8-connected patches) when a level loads. The format's check caps them at 64.
- **Renderer.**
  - The radiance cascades weight directional pixels' light by their cone.
  - The composite adds each beam's light from the spike's analytic wedge with 8-point soft shadows. A surface term goes into the light; an air term, scaled by the haze, is added over open pixels and the sky.

**Tech Stack:**
- Rust: `olr-look` and `olr-terrain` are no_std; `olr-format` and `olr-edit` are std.
- wgpu WGSL shaders.
- egui 0.36 editor.
- clap CLI.
- Python (PIL), only to inspect the reference renders.

**Spec:** `docs/superpowers/specs/2026-10-25-directional-glow-design.md`

## Global Constraints

- **A level without directional glow and without haze draws byte for byte as before.** The render goldens, `fixtures/render/lit-hashes.ron`, the look goldens and the HUD snapshots hold without `bless-render`.
- **Names.** "Beam" already names the Beam weapon's look (issue #69, `BeamLookSource`, `olr-look/tests/beam.rs`). This feature's code and files say **glow beam**: `GlowBeam`, `glow_beams`, `look/glow-beam.png`.
- **Ranges.**
  - `direction`: integer degrees 0–359, 0 pointing right, clockwise on screen (y down), so 90 points down.
  - `spread`: the half-angle, integer degrees 1–90.
  - `haze`: percent 0–100, default 0 (left out of the file).
  - `look/haze.png`: grey per world pixel, 0 none to 255 the level's haze.
- **`MAX_GLOW_BEAMS` = 64 per level.** More is an error from `check`. The lenient reader plays the level without beams and warns "…: the level plays without its glow beams".
- **Which pixels may be directional.** A directional element's pixels must be Rock or Background: `check_elements` refuses `beam` on an element of material Dirt, and the bake writes no beam texel on a dirt world pixel. Its group's reach must be `Pixels`: `Unlimited` is refused with an error naming the element.
- **What blocks a beam.** Shadows come from opaque rock and dirt only, read from the live solid map. Worms block nothing (issue #53: `worms_change_no_light` stays green).
- **Strength.** A beam's strength is `BEAM_SCALE` × the group's `cast` (intensity × animation) × the patch's mean glow strength (0–1, mask included). The colour is the patch's mean linear art colour normalised to a brightest channel of 1 for `Own` and `Filtered`, and white for `Tint` (`cast` already carries the tint).
  - `BEAM_SCALE` = 0.08: intensity 1000 at glow 100 gives the reference headlights' 0.8.
  - There is no separate beam-strength setting (the maintainer's ruling, 2026-10-25).
- **The beam's shape** (the spike's constants, `composite.wgsl` `beams_at`):
  - width `half + along × tan(spread)`;
  - edge `1 − smoothstep(width − pen/2, width + pen, |side|)` with `pen = 0.5 + along × 0.03`;
  - falloff `(1 − along/reach)³`;
  - 8 shadow points spread evenly across the source, offset half a pixel behind its face;
  - the cascade cone `smoothstep(cos(spread + 0.26), cos(spread), dot(−ray, dir))`.
- **Purity.** No noise, time or camera input in a beam's geometry: still scenes give steady bytes, and the light is camera-independent.
- **Crates.** `olr-look` and `olr-terrain` stay `#![no_std]` and build for `wasm32-unknown-unknown`. Never panic on content.
- **Commits.**
  - Run `cargo xtask check` before every commit; Task 5 runs `cargo xtask check --render`.
  - Subjects are small and imperative (`olr-render: glow beams from directional glow`).
  - Read each crate's `CLAUDE.md` before editing it, and update it in the same task.
  - There is no OLX counterpart, so the decisions go in `docs/porting/STATUS.md` ("Directional glow").
- **Asking the maintainer.**
  - Ask before `cargo xtask full`, `bench` or `stress`.
  - No `cargo xtask profile` either: the maintainer deferred all measurement (2026-10-25, the PC in use).
  - Task 6's render checkpoint is a stop: show the maintainer the renders and wait.

## Review Focus

- **A patch split by the world grid at a tilted direction**, such as the headlights at 166°. The beam must start flush with the patch: no gap, no overlap beyond half a pixel. Task 2's `a_tilted_patch_starts_flush` pins it.
- **Two adjacent patches of different directions in one group** (the left- and right-facing PCs as two elements next to each other). They must be two beams, not one. Task 2's `different_directions_are_different_patches` pins it.
- **Dirt dug in front of a beam during play.** The light must pass where the dirt was, the next frame. Task 5's `digging_opens_a_shadow` pins it.
- **A hostile `look/glow-beam.png`** (wrong size, spread 0 or above 90, direction 360 or above). Strict reads refuse it and lenient reads drop it with a warning; nothing panics. Task 1's `hostile_glow_beam_maps` pins it.
- **Split screen over a beam.** Two views give each world pixel the same beam light. Task 5's `beams_are_camera_independent` draws two camera positions.

---

### Task 0: Branch and spec

**Files:** `docs/superpowers/specs/2026-10-25-directional-glow-design.md`, `docs/superpowers/plans/2026-10-25-directional-glow.md`

- [ ] **Step 1:** `git checkout -b glow-beams main`
- [ ] **Step 2:** `git add` both files; `git commit -m "docs: directional glow design and plan"`

### Task 1: Format: `GlowBeam`, the beam maps, haze

**Files:**
- Modify: `crates/olr-format/src/levels/composition.rs`: `ElementGlow::beam` and `GlowBeam`; `check_elements` (no beam on Dirt or on a distant element); `check_glow_groups` (a beam needs a `Pixels` reach).
- Modify: `crates/olr-format/src/look.rs`: `LookSource::haze` and its compile into `olr_look::LevelLook::haze`.
- Modify: `crates/olr-format/src/levels/mod.rs` and `files.rs`: `GLOW_BEAM_FILE`, `GLOW_BEAM_BACK_FILE`, `HAZE_FILE`, `LevelSource::{glow_beams, haze}`, `file_paths`, and the strict and lenient reads (as `look/glass.png`).
- Create: `crates/olr-terrain/src/glow_beam.rs`: `GlowBeamMap`, re-exported like `GlassMap`.
- Modify: `crates/olr-look/src/lib.rs`: `LevelLook::haze: u8`.
- Modify: every `LevelSource { .. }`, `ElementGlow { .. }` and `LookSource { .. }` literal the compiler names.
- Modify: `crates/olr-format/CLAUDE.md`, `crates/olr-terrain/CLAUDE.md`; `cargo xtask format-docs` regenerates `docs/formats/level.md`.
- Test: `crates/olr-format/tests/roundtrip.rs`, `tests/look.rs`, `tests/hostile.rs`.

**Interfaces:**
- Produces:
  - `olr_format::levels::GlowBeam { pub direction: u16, pub spread: u8 }` (serde, `deny_unknown_fields`).
  - `ElementGlow::beam: Option<GlowBeam>`, left out when none.
  - `olr_terrain::GlowBeamMap { pub world: (u32, u32), pub front: Vec<u8>, pub back: Vec<u8> }`:
    - two bytes per world pixel per layer: byte 0 the direction / 2 (0–179) + 1, 0 meaning not directional; byte 1 the spread (1–90);
    - `fits(world) -> bool`;
    - `get(layer_back: bool, x, y) -> Option<(u16, u8)>`, giving the direction as `(byte0 − 1) × 2`.
  - Files: grey-alpha PNGs `look/glow-beam.png` and `look/glow-beam-back.png`, both or none.
  - `LevelSource::glow_beams: Option<GlowBeamMap>`.
  - `LookSource::haze: u8` and `LevelLook::haze: u8`.
  - `LevelSource::haze: Option<Vec<u8>>`: world size, grey `look/haze.png`.

  *Planning call:* the map holds even directions only (2° steps). Its byte layout fits a grey-alpha PNG, and an odd direction bakes rounded down to the even one below. The element's own `direction` keeps whole degrees. The spike's cones were all within that resolution. Cost if wrong: one byte more per pixel.

- [ ] **Step 1: Write the failing tests.**
  - `roundtrip.rs` `glow_beams_round_trip`:
    - an element whose glow has `beam: Some(GlowBeam { direction: 166, spread: 15 })` writes `beam: (direction: 166, spread: 15)` and reads back equal;
    - a level with a `GlowBeamMap` writes both files and reads back equal;
    - `glow_beams: None` writes neither file;
    - `haze: 40` round-trips;
    - `haze: 0` is left out of `level.ron`;
    - `LevelSource::haze` writes `look/haze.png` and reads back.
  - `look.rs` `glow_beam_limits`:
    - each of these fails, naming the field path: direction 360, spread 0, spread 91, haze 101;
    - a beam on a Dirt drawn layer fails;
    - a beam on a distant element fails (distant glow stays undirected);
    - a beam whose group's reach is `Unlimited` fails, naming the element;
    - on Rock and on Background it passes.
  - `hostile.rs` `hostile_glow_beam_maps`:
    - a beam map of the wrong size, one with a byte 0 above 180, and one with a spread byte above 90 each fail `read_level_files`;
    - `read_level_files_lenient` returns `glow_beams: None` with a warning containing "plays without its glow beams";
    - a misfit `look/haze.png` drops with "plays without its haze map".
- [ ] **Step 2:** `cargo nextest run -p olr-format` fails: the types are missing.
- [ ] **Step 3:** Implement. The checks use the ranges in Global Constraints. Reads go through `image_io`, with the size checked from the header first. Add doc comments on every field (they are the schema source), then run `cargo xtask format-docs`.
- [ ] **Step 4:** `cargo nextest run -p olr-format -p olr-terrain -p olr-look` passes. `cargo build --target wasm32-unknown-unknown -p olr-terrain -p olr-look` passes.
- [ ] **Step 5:** `cargo xtask check`, then commit `olr-format: directional glow and haze in the level format`.

### Task 2: Derivation: `olr_look::glow_beams`

**Files:**
- Create: `crates/olr-look/src/glow_beams.rs` (`pub mod glow_beams;` in `lib.rs`).
- Modify: `crates/olr-format/src/levels/files.rs`: `check` counts them; the lenient read drops more than `MAX_GLOW_BEAMS`.
- Modify: `crates/olr-look/CLAUDE.md`.
- Create: `crates/olr-look/tests/glow_beams.rs` (a `mod glow_beams;` in `tests/integration.rs`).

**Interfaces:**
- Consumes: Task 1's map layout. The map is passed as slices, because `olr-look` does not depend on `olr-terrain`.
- Produces:
  - `pub const MAX_GLOW_BEAMS: usize = 64`
  - `pub struct BeamLayer<'a>`:
    - `world: (u32, u32)`;
    - `art_scale: u32`;
    - `groups: &'a [u8]` (world);
    - `glow: &'a [u8]` (art);
    - `art: &'a [u8]` (RGBA, art);
    - `beams: &'a [u8]` (two bytes per world pixel).
  - `pub struct GlowBeamSource`:
    - `group: u8` (map value, 1-based);
    - `at: [f32; 2]`, `dir: [f32; 2]`, `half: f32` (world pixels);
    - `spread: f32` (radians);
    - `glow: f32` (0–1);
    - `colour: [f32; 3]` (linear, brightest channel 1; black gives zeros);
    - `first: u32` (row-major index of the patch's first pixel, for its flicker region);
    - `back: bool`.
  - `pub fn derive(front: &BeamLayer, back: &BeamLayer) -> Vec<GlowBeamSource>` returns every patch, front layer first, each layer in row-major order of its first pixel. Callers apply the cap.

- [ ] **Step 1: Write the failing tests** in `tests/glow_beams.rs`, on small hand-built layers:
  - `a_patch_is_one_beam_as_wide_as_it`: a 1 × 5 column of directional pixels facing right (0°) at x 10, y 20–24 gives one beam: `at` (11.0, 22.5), `dir` (1, 0), `half` 2.5.
  - `a_tilted_patch_starts_flush`: the same column at 166°. `at` lies on the face, the furthest projection of the patch's pixel corners on `dir`. Every pixel corner of the patch projects to at most `dot(at, dir) + 1e-4`, and at least one projects to exactly that.
  - `different_directions_are_different_patches`: two touching columns, one at 0° and one at 180°, give two beams.
  - `different_groups_are_different_patches`.
  - `diagonal_pixels_join` (8-connected).
  - `the_beam_takes_the_patchs_mean_glow_and_colour`: a glow of 128 and 255 over two pixels gives `glow` ≈ 0.75. Art (200, 100, 50) gives a colour of linear (1, x, y) normalised.
  - `the_order_is_row_major_front_first`: the same layers twice give equal vectors.
  - `undirected_pixels_make_no_beam`.
  - `a_misfit_layer_makes_none`: slices of the wrong length give an empty result, never a panic.
  - In `olr-format`'s `tests/look.rs`, `too_many_glow_beams`: 65 separate one-pixel patches fail `check` with a message naming 65 and 64. The lenient read gives `glow_beams: None` and the warning.
- [ ] **Step 2:** `cargo nextest run -p olr-look glow_beams` fails.
- [ ] **Step 3:** Implement:
  - a flood fill over world pixels whose beam byte 0 is not 0 and whose group is not 0, keyed by (group, beam bytes), using an explicit stack and a visited `Vec<bool>`;
  - `dir = (cos θ, sin θ)`, with θ the direction in radians (y down);
  - `perp = (−dir.y, dir.x)`;
  - face = the maximum of `dot(corner, dir)` over the patch pixels' four corners;
  - `mid` and `half` from the minimum and maximum of `dot(corner, perp)`;
  - `at = dir × face + perp × mid`;
  - `glow` and `colour` averaged over each patch pixel's art block (`art_scale²` art pixels).

  Use `libm` for the trig.
- [ ] **Step 4:** `cargo nextest run -p olr-look -p olr-format` passes; the wasm build of `olr-look` passes.
- [ ] **Step 5:** `cargo xtask check`, then commit `olr-look: glow beams from directional glow patches`.

### Task 3: Bake: beam texels, haze kept

**Files:**
- Modify: `crates/olr-edit/src/compose.rs`:
  - `LayerGlow` carries its element's beam;
  - each world cell's beam is the one of the layer `cell_group` picks, written only when the cell's material is Rock or Background;
  - `level.glow_beams` is `None` when no texel is set;
  - `stale` covers both beam files;
  - `level.haze` passes through untouched.
- Modify: `crates/olr-edit/src/level.rs`: `resize` re-lays `glow_beams` and `haze` (the new area none).
- Modify: `crates/olr-edit/src/look.rs`: `set_haze_map(level, Option<Vec<u8>>)`, refusing a misfit.
- Modify: `crates/olr-edit/CLAUDE.md`.
- Create: `crates/olr-edit/tests/glow_beams.rs` (a `mod` of `tests/integration.rs`).

**Interfaces:**
- Consumes: Task 1's types.
- Produces: baked `LevelSource::glow_beams`; `olr_edit::look::set_haze_map`.

*Planning call:* the haze map is level-wide and has no element source, so it lives only in `look/haze.png`. It is edited in place, and the bake keeps it. The spec suggested `editor/` plus a bake; this drops the copy. Cost if wrong: one extra file and a copy step.

- [ ] **Step 1: Write the failing tests:**
  - `a_directional_rock_layer_bakes_its_beam`: a Rock drawn layer glowing with `beam (166, 15)` gives texels `(84, 15)` on its glowing world pixels (166 / 2 + 1 = 84) and `(0, 0)` elsewhere.
  - `a_dirt_pixel_takes_no_beam`: a mixed layer's dirt cells give `(0, 0)`.
  - `the_topmost_glowing_element_gives_the_beam`.
  - `no_beam_writes_no_files`: every bundled level still bakes byte for byte (the existing round trips stay green).
  - `stale_lists_the_beam_map_after_a_direction_change`.
  - `resize_relays_beams_and_haze`.
  - `the_haze_map_survives_a_bake`.
- [ ] **Step 2:** `cargo nextest run -p olr-edit glow_beams` fails.
- [ ] **Step 3:** Implement, following the group pick exactly (the centre art pixel, else the first glowing pixel of the block) so the beam and group maps agree per cell.
- [ ] **Step 4:** `cargo nextest run -p olr-edit` passes.
- [ ] **Step 5:** `cargo xtask check`, then commit `olr-edit: bake directional glow`.

### Task 4: CLI and thumbnail

**Files:**
- Modify: `crates/olr-cli/src/element_cmd.rs`:
  - `set`'s `--glow-beam DIR,SPREAD` and `--no-glow-beam` (only on a glowing element: "the element does not glow" otherwise);
  - `list` prints ` beam <dir>° <spread>°` after the glow.
- Modify: `crates/olr-cli/src/look_cmd.rs`: `--haze N` (0–100).
- Modify: `crates/olr-cli/src/level_cmd.rs` (`LevelCommand`): `level haze paint <shape args as element glow's> --strength 0..=100` and `level haze clear`, both through `olr_edit::look::set_haze_map`.
- Modify: `crates/olr-format/src/levels/thumb.rs`: a directional pixel's cell casts only inside its cone, weighted as the cascade cone. No beams, no haze.
- Modify: `crates/olr-cli/CLAUDE.md` and the `olr-format` CLAUDE.md thumbnail note.
- Test: `crates/olr-cli/tests/element.rs`, `crates/olr-format/tests/look.rs`.

- [ ] **Step 1: Write the failing tests:**
  - `cli_sets_a_glow_beam`: `level element set <lvl> rock-pcs --glow-beam 0,35`, then `list` shows `beam 0° 35°`. `--no-glow-beam` removes it, and the baked map follows. On a non-glowing element the command exits non-zero with the message.
  - `cli_paints_haze`: `look --haze 40` and `level haze paint --rect 0,0,10,10 --strength 50` give `look/haze.png` with 128 in the rectangle and 0 elsewhere.
  - `the_thumbnail_keeps_directional_light_in_its_cone`: one directional patch facing right in a dark level; the thumbnail is brighter to its right than to its left. An undirected one is symmetric.
- [ ] **Step 2:** `cargo nextest run -p olr-cli element -p olr-format thumbnail` fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Tests pass.
- [ ] **Step 5:** `cargo xtask check`, then commit `olr-cli: directional glow and haze`.

### Task 5: Renderer: cones, beams, haze

**Files:**
- Modify: `crates/olr-terrain/src/color.rs`: `ColorMap::set_glow_beams(Option<GlowBeamMap>) -> bool`, `glow_beams()`, `set_haze(Option<Vec<u8>>) -> bool`, `haze()` (as `set_glass`).
- Modify: `crates/olr-content/src/color.rs` and `crates/olr-game/src/editor/canvas.rs` (both `set_glass` sites): set them.
- Create: `crates/olr-render/src/renderer/glow_beams.rs`:
  - derives the sources once per `upload_terrain` via `olr_look::glow_beams::derive`, with the cap: past 64 none, which `check` already refuses;
  - each frame packs a storage buffer of `GpuBeam { at, dir, half, tan_spread, cos_spread, reach, light: [f32; 3], group, region }`, where `light` is `BEAM_SCALE` × `FrameGroup::cast` × `colour` × `glow` (Global Constraints).
- Modify: `crates/olr-render/src/renderer/look.rs`:
  - the beam map texture (`Rg8Uint`, world size; 1 × 1 none);
  - the haze texture (`R8Unorm`, world size; 1 × 1 at 255 when there is no map);
  - `LookGlobals`: the haze (percent / 100) and the beam count.
- Modify: `crates/olr-render/src/look/gi_cascade.wgsl`: at a hit, a cell whose beam texel is set weights its emission by the cone.
- Modify: `crates/olr-render/src/look/composite.wgsl`:
  - `glow_beams_at(p)`, the spike's `beams_at` with its flicker through `region_flicker(group.flicker, region)`;
  - its rejects first: `along` outside (0, reach], then the wedge;
  - the shadow march through `solid_px` last.
- Modify: `crates/olr-render/CLAUDE.md`.
- Create: `crates/olr-render/tests/gpu/glow_beams.rs` (`#[path]` mod in `tests/gpu.rs`).

**The proposal for the open split** (the spec's "Left to the builder"):
- the **surface term**: the beam light is added to `lit` before `light × art`, on every pixel that is not see-through, not glowing, in the level;
- the **air term**: `beam × haze × haze_map(w)` is added after `light × art`, over open pixels that are not glowing and over the sky.

Task 6 tunes this split against the reference with the maintainer.

- [ ] **Step 1: Write the failing tests** (a 128 × 64 lit level, a dark ambient, one Rock patch 1 × 5 facing right at x 20 with a group of intensity 1000, reach 60; build as `distant_glow.rs` does):
  - `a_beam_lights_ahead_and_not_behind`: the light 20 px to its right is above 3 × the ambient; 5 px to its left it is below 1.1 × the ambient. With the beam texels removed (undirected), the left side is above 1.5 × the ambient: the cone is what darkens it.
  - `a_beam_starts_as_wide_as_its_source`: 2 px ahead, the pixels across the 5 rows are lit within 10 % of each other; 2 rows outside are below half.
  - `a_wall_stops_a_beam`: a one-pixel rock wall at x 40 leaves x 45 at the ambient.
  - `shadow_edges_are_smooth`: a rock step cuts the beam's lower half. Along a column 30 px past the step, the light changes between neighbouring rows by at most 25 % of its range (no staircase).
  - `digging_opens_a_shadow`: carve the wall through `ColorMap`/solid, and the next frame's light at x 45 is lit.
  - `still_glow_draws_steady_bytes`: two frames of a steady group are equal.
  - `beams_are_camera_independent`: views at (0, 0) and (16, 8) give equal light per world pixel.
  - `haze_shows_a_beam_over_the_sky`: a see-through level whose beam runs over a distant layer. At haze 0 the sky pixels equal the no-beam frame; at haze 60 they are brighter. A haze map of 0 there brings them back to equal.
  - `no_glow_beams_no_haze_changes_no_byte`: a lit level drawn with `set_glow_beams(None)`/`set_haze(None)` and with empty maps gives equal bytes. Today's bytes are pinned by the goldens (Step 5).
  - `worms_change_no_light` stays green.
- [ ] **Step 2:** `OLR_GPU_TESTS=1 cargo nextest run -p olr-render glow_beams` fails.
- [ ] **Step 3:** Implement. Use the spike (`git -C /home/martin/olr-spike-light diff`, `crates/olr-render/src/spike.rs`) as the reference for the WGSL, minus its env-var plumbing, blockers and haze pass.
- [ ] **Step 4:** `OLR_GPU_TESTS=1 cargo nextest run -p olr-render` passes; `cargo nextest run -p olr-terrain -p olr-content` passes.
- [ ] **Step 5:** `cargo xtask check --render`: goldens, lit hashes and HUD snapshots unchanged, with no re-bless. Commit `olr-render: glow beams from directional glow`.

### Task 6: Liero Factory Reloaded, and the render checkpoint

**Files:**
- Modify: `content/reloaded/olr-level-liero-factory-reloaded/`: `editor/composition.ron`, the new layers, `level.ron`, the baked files.
- Modify: `content/reloaded/README.md`, `crates/olr-cli/tests/reloaded.rs`.
- Reference: the session scratchpad `/tmp/claude-1000/-home-martin-rust-liero/626c08a1-84ee-4117-b3ad-8974fc0cfdbe/scratchpad/`: `lf-s2/`, `factory_lights.py`, `factory_pcs.py`, `spike/p-pcs.png` (bottom row), `spike/p-zoom.png`, `spike/p-truck.png` (right column).

- [ ] **Step 1:** Rebuild the held factory changes in `content/reloaded/…` with `olr-cli level element` and `level look` commands. `lf-s2`'s files show the pixels.
  - Glass `frost: 10`.
  - Rock layer `rock-pcs` (group `pcs`), split into:
    - `rock-pcs-right`: the PCs at x 344 and 513, y 259–263, `--glow-beam 0,35`;
    - `rock-pcs-left`: x 498, `--glow-beam 180,35`;
    - `rock-pcs`: the blue PC at 523–533 × 209–213, undirected.
  - The blue PC's pixels removed from `rock-screens`.
  - `rock-headlights` (42–44 × 410–414, group `headlights`, Tint `#fff0c8`), `--glow-beam 166,15`.
  - Groups: `screens` 450/48; `pcs` 1000/56; `headlights` 1000/120; `control` Tint `#d0e0ff` 450/48 on the split-out `control-screen` (Background, glow 90, the `screens.png` pixels with y < 110 and x 880–950).
  - The side PCs' glow strength 15.
- [ ] **Step 2:** `olr-cli look-preview content/reloaded/olr-level-liero-factory-reloaded --at <the reference crops' centres> --out <scratchpad>/lf-built-*.png`. Make side-by-side sheets with the references (PIL), at the same crops as `p-pcs`, `p-zoom` and `p-truck`. Set `--haze` to the value that best matches the reference truck beam over the sky. Adjust the surface and air terms' weights only within the Task 5 proposal.
- [ ] **Step 3: STOP: show the maintainer the sheets.** Use the artifact page or the paths. Wait for their verdict on the look (the split, the haze, the PCs' screens at glow 15) before going on. Record their rulings in `docs/porting/STATUS.md`.
- [ ] **Step 4:** Apply their rulings and re-render if asked. Update `reloaded.rs`'s expectations and the README. `cargo nextest run -p olr-cli reloaded` passes.
- [ ] **Step 5:** `cargo xtask check --render`, then commit `content: Liero Factory Reloaded's PCs and headlights cast beams`.

### Task 7: Level editor: direction, spread, haze, Haze brush

**Files:**
- Modify: `crates/olr-editor/src/ui/look.rs`:
  - `element_glow` gets a "Directional" checkbox (default direction 0, spread 35), then "Direction °" (0–359) and "Spread °" (1–90) numbers. They are hidden when locked, and absent on Dirt elements, with a hint.
  - `level_look` gets "Haze %" under the occlusion.
- Modify: `crates/olr-editor/src/tools.rs`: `Tool::Haze` (`ALL` becomes 14), named "Haze brush". Its key is `F` if no shortcut uses it (check `Tool::key` and the editor's key handlers), else the first free letter.
- Modify: `crates/olr-editor/src/look_ops.rs` and `history.rs`:
  - `paint_haze(shape, strength) -> Rect`;
  - `Edit::Haze { before, after }` (the touched rectangle's bytes): one step a stroke, none when unchanged.
- Modify: `crates/olr-editor/src/pointer.rs` and `ui/toolbar.rs`: the brush strokes, and its strength under the toolbar as the Glow brush's.
- Modify: `crates/olr-editor/src/ui/mod.rs` labels: `DIRECTIONAL`, `DIRECTION`, `SPREAD`, `HAZE`, `TOOL_HAZE`, `DIRECTIONAL_DIRT_HINT`.
- Modify: `crates/olr-editor/CLAUDE.md`.
- Test: `crates/olr-editor/tests/flows.rs`.

- [ ] **Step 1: Write the failing tests:**
  - `make_a_glow_directional`: tick `labels::DIRECTIONAL` on a glowing Rock layer and the glow has `beam (0, 35)` in one undo step. Set Direction 166 and Spread 15 (one step each); the canvas's bake has the beam texels. On a Dirt layer the checkbox is absent and the hint shows.
  - `set_the_haze`: Haze 40 lands in `look.haze`.
  - `paint_haze`: a Haze brush stroke at 50 % writes 128 under it, as one undo step; undo restores the map; a stroke changing nothing adds no step.
  - The fits test (`every_menu_screen_fits`, olr-ui) is unaffected; run it.
- [ ] **Step 2:** `cargo nextest run -p olr-editor make_a_glow_directional set_the_haze paint_haze` fails.
- [ ] **Step 3:** Implement with the existing `number` helper and the Glow brush's stroke pattern (`glow_stroke_before`/`paint_glow`/`glow_stroke_edits`).
- [ ] **Step 4:** `cargo nextest run -p olr-editor -p olr-ui -p olr-game` passes.
- [ ] **Step 5:** `cargo xtask check`, then commit `olr-editor: directional glow and the Haze brush`.

### Task 8: Measure, document, finish

- [ ] **Step 1: Measurement deferred** (the maintainer, 2026-10-25: the PC is in use). Run no `cargo xtask profile`, bench or stress. Add a row to `docs/deferred-verification.md`:
  - **What to run:** `cargo xtask profile` on Liero Factory Reloaded with its glow beams in view, the composite's cost against the last base; if it grows more than 15 %, per-tile culling of the beams (a CPU list per 32 × 32 screen tile); confirm the 64-beam cap and the 8 shadow points.
  - **Why:** the session did not measure.
- [ ] **Step 2:** In `docs/porting/STATUS.md`, add "Directional glow": the spec's decisions, the planning calls (2° map resolution, the haze map in `look/` only, the surface and air split as ruled in Task 6) and the deferred measurement.
- [ ] **Step 3:** `cargo xtask check --render` is clean.
- [ ] **Step 4:** Whole-branch review (superpowers:executing-plans' final review).
- [ ] **Step 5:** Ask the maintainer about `cargo xtask full` before merging: this is a rendering change.
