# Plan 15a: The Look System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Levels and mods carry an authored look: darkness, glowing pixels that light the world (up to a sun), placed lights with shadows, animated light, weapon glows and flashes, and a colour grade. It's built in the level editor's live preview and cheap enough for an office laptop.

**Architecture:**
- **Data.** Authoring types in `olr-format` compile to runtime types in a new `no_std` crate, `olr-look`, which also evaluates animations.
- **Terrain.** Glow is a channel of the colour terrain (`olr-terrain`'s `ColorMap`), so every terrain edit keeps it in step.
- **Renderer.** `olr-render` gains a second world-pass target (self-light and marker), a whole-level glow-light pass (radiance cascades), analytic placed lights with 1D shadow maps, and a composite-and-grade pass before the upscale. All of them run only when a `LookFrame` is given.
- **Per frame.** `olr-present` builds the `LookFrame` from the world, the scene, the effects, the level, the mod and the engine.
- **Editor.** It authors glow, groups, lights and the Look panel over the same pieces.

**Tech Stack:** Rust 1.99, wgpu 30.0.1 (WGSL), egui `=0.36.2`, egui_kittest `=0.36.2`, serde/RON, proptest, clap, divan.

**Spec:** `docs/superpowers/specs/2026-10-18-look-system-design.md` (all of it). Read its "Calls made" C1-C16 before starting.

## Global Constraints

- **Byte-identical:**
  - the oracle traces;
  - the match goldens;
  - `content/packs`;
  - the four render goldens `fixtures/render/frame-*.png` and `dig-color.png`;
  - the UI goldens;
  - `fixtures/edit/composed-level`.

  A change that moves any of them is a bug, not a re-bless. Only the new `fixtures/render/look-*.png` are blessed, each with a reason.
- **The simulation:**
  - `olr-sim` never sees look data, and no simulation event changes;
  - `olr-look` and `olr-terrain` stay `#![no_std]` and build for `wasm32-unknown-unknown` (repo rule 5).
- **Never panic on content** (repo rule 6). Look files, look fields, CLI arguments and pointer positions are untrusted. Integer sums are `i64` or saturate. Floats in shaders are clamped before indexing.
- **Determinism** (repo rule 7):
  - nothing reads the wall clock;
  - the look clock is the world's time plus `alpha × 10 ms`;
  - lights are culled and capped in insertion order;
  - no `HashMap` iteration in output.
- **No per-player setting changes the picture's information** (spec §Fair play).
- **Limits** (spec §Data; values verbatim):

  | What | Limit |
  |---|---|
  | `intensity` | 0-1000 % |
  | `reach` | `Pixels(1..=4096)` or `Unlimited` |
  | light `radius` | 1-16 world pixels, default 2 |
  | `max_light` | 100-200 |
  | `light_steps` | 0 or 2-32 |
  | `softness` | 0-4 |
  | `exposure` | -200..200 (1/100 stop) |
  | `contrast`, `saturation` | 0-200 |
  | ambient intensity | 0-100 |
  | element glow strength | 0-100 |
  | projectile glow | 0-100 |
  | glow groups | at most 255 |
  | light objects | at most 1024 per level (R14) |
  | `Keys` | at most 64 keys |
  | `period_ms` | 1-600 000 |
  | `MAX_LIGHTS` | 256 per frame |
  | `GI_CELL_BUDGET` | 262 144 cells (R13) |
- **Integers in every look file** (spec C12).
- **Verify once per batch:**
  - targeted `cargo nextest run -p <crate>` while working;
  - `rtk proxy cargo clippy -p <crate> --all-targets` before each commit;
  - `cargo xtask check` at a batch's end, with **no edits while it runs**.
- **GPU tests** need `OLR_GPU_TESTS=1` (lavapipe); run the touched ones by name, e.g. `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu look`.
- **Benchmarks, stress, envelope and `cargo xtask full` only after asking the user** (the user shares the PC). Report a failure and don't re-run it.
- **The batches:**

  | Batch | Tasks | Covers |
  |---|---|---|
  | A | 1-8 | data, terrain, bake, loading, CLI, the own name |
  | B | 9-14 | the renderer, `look-preview`, the goldens |
  | **Stop 1** | | the contact sheet (Task 14): wait for the user's verdict |
  | C | 15-16 | play wiring: the mod's and engine's lights |
  | D | 17-19 | the editor |
  | **Stop 2** | | a playtest build, with the user's consent (R10) |
  | E | 20-21 | measurements (consent) and docs |

- **Background review after each batch;** the batch reviews are the whole-branch review (14d's practice).
- **Commits:** imperative subject, a blank line, then the two trailer lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Review Focus

1. **Hostile look files.**
   - Cases:
     - `look/glow.png` of the wrong size;
     - only two of the four glow images;
     - a group index of 200 with 3 groups;
     - a 16-bit PNG;
     - `Keys` with 65 keys;
     - `period_ms: 0`;
     - `reach: Pixels(0)`.
   - Expected: `pack check` errors naming the file or field; loading drops the glow (or the look) with one warning; never a panic.
   - Tests: Task 2, `hostile_look_files_are_errors_never_panics`; Task 6, `a_broken_glow_loads_without_glow`.
2. **The glow staying in step with the terrain through every path that rewrites it.**
   - Paths:
     - F5 restart;
     - an online loaded state (`state_loaded`, restored areas through `rebuild_area`);
     - the editor's `replace_rect` after a resize;
     - an art-scale change.
   - Expected: after each, the glow equals a glow built fresh from the material.
   - Tests: Task 4, `glow_follows_rebuild_and_replace`; Task 15, `a_loaded_state_restores_the_glow`.
3. **Lights at the edges.**
   - Cases:
     - a light outside the level;
     - a light inside rock;
     - `Unlimited` reach on a 4096 × 4096 level;
     - 2000 lights in view.
   - Expected: no panic; a light inside rock lights nothing; the cap keeps the first `MAX_LIGHTS` in order; the frame stays inside the budget's order of magnitude.
   - Tests: Task 11, `placed_lights_at_the_edges`; Task 15, `the_light_cap_is_deterministic`.
4. **A level larger than the device's texture limit** (the colour terrain is cropped with a warning today).
   - Expected: the glow, group and solid textures crop the same way; the glow-light grid is sized from the cropped level; no wgpu validation error.
   - Test: Task 9, `look_textures_crop_like_the_terrain` (CPU geometry, plus a GPU check under a forced small limit).
5. **Online corrections.**
   - Cases:
     - a projectile that a rollback erases;
     - a projectile that reappears after a rollback;
     - a re-simulated explosion.
   - Expected: an end flash shows at most once per (slot, serial); a re-simulated explosion's light is not doubled (it keys on the effect entity, which the net presenter spawns once).
   - Test: Task 15, `end_flashes_once_per_projectile_through_rollbacks`.

## Rulings made while writing this plan

- **R1 `olr-look`:**
  - a new crate, layer 1, `#![no_std]` with `alloc`, wasm;
  - depends only on `olr-math`;
  - registered in `xtask/layers.ron` (layer 1, `no_std`, `wasm`), with a `CLAUDE.md`;
  - it holds the runtime look types, animation evaluation, `LookFrame` and the engine's look (spec C7, C15).
- **R2 The look stays out of `olr_data::level::Level`.**
  - `LevelSource` gains `look` and `glow`.
  - `LoadedLevel` gains `look: Option<olr_look::LevelLook>` and `glow: Option<GlowLayers>`.
  - `compile_level` is unchanged.
  - The mod's look compiles beside `compile_mod`, as `compile_mod_look`.
- **R3 Glow lives inside `ColorMap`** (`set_glow`), so every edit path updates it and it shares the dirty rectangles. There is no parallel map.
- **R4 The solid map** is the colour map's material cells (`Terrain::cells()`), uploaded as an `R8Uint` world-resolution texture. A cell is solid when its flags say rock or dirt.
- **R5 Item shading.** `olr_scene::Shade { self_light: u8, marker: bool }` (percent; default `Shade::LIT` = 0, false) is a field of `Sprite` and `Line`. `describe` prints it only when it differs from `LIT`, so the insta snapshots stay as they are.
- **R6 The renderer's entry points take `look: Option<&LookFrame>`.**
  - Affected: `render`, `render_in`, `Offscreen::render_to_image`, `render_to_png`.
  - `None` is today's path, byte for byte.
  - Look textures are carried only after `Renderer::set_lighting(true)`.
- **R7 When the passes run.** `olr-present` gives a `LookFrame` exactly when the level has a `look` or the mod has any look. Engine lights and floors are added only then (spec §The engine).
- **R8 End flashes** are detected in `olr-present` by a projectile slot whose serial was drawn last frame and is gone or changed. Each (slot, serial) flashes at most once; a `BTreeSet` is pruned by age.
- **R9 Light order** in a `LookFrame`:
  1. level lights in file order;
  2. projectile lights in slot order;
  3. end flashes in start order;
  4. muzzle lights in worm order;
  5. explosion lights in effect slot order.

  `LookFrame::cull(view)` keeps those whose reach touches the view (plus `MARGIN`), then the first `MAX_LIGHTS`.
- **R10 Stop 2 comes after the editor batch, not after the play batch** (the spec says after play). A playtest of the look needs the editor to make lit levels; the fixture level alone is too thin.
- **R11 The look clock:** `t_ms = world.time()` in milliseconds as `f64`, plus `alpha × 10.0`. The editor's preview clock is its own `f64`, advanced by the frame time while playing.
- **R12 Editor keys:**
  - `H` the Glow brush;
  - `K` the Light tool (V B E R O L P I W G M are taken).
- **R13 The first numbers** (tuned at Stop 1 and in Task 20):

  | What | Value |
  |---|---|
  | `GI_CELL_BUDGET` | 262 144 |
  | shadow map rays per light | 512 |
  | `MAX_LIGHTS` | 256 |
  | engine floor (worms, projectiles, bonuses) | 50 % |
  | explosion light | colour (255, 200, 140); intensity `min(60 + 8 × size, 250)` %; reach `24 + 8 × size` px; `Keys([(0, 100), (200, 0)], looped: false)` |
  | muzzle light | (255, 220, 160), 80 %, reach 40, `Keys([(0, 100), (50, 0)])` |

  Self-light 100 for:
  - effects: `Explosion`, `Sparkle`, `Spawn`;
  - the muzzle flash sprite;
  - beams.

  Markers:
  - the crosshair;
  - the health bar;
  - laser sight dots.
- **R14 At most 1024 light objects per level** (spec silent; one per world pixel row of a big level is far more than needed).
- **R15 Look files in a pack's validation:** `look/glow.png`, `look/glow-back.png`, `look/groups.png` and `look/groups-back.png` are format files of a level (no "not read by the format" warning).
- **R16 Test content:**
  - `fixtures/look/levels/look-test/`: a 512 × 256 level at art scale 2, made with `olr-cli`;
  - `fixtures/look/mods/look-test-mod/`: one weapon, "Glow Rocket", made with `olr-cli pack new mod` and edited;
  - `fixtures/look/scenarios/rocket.ron`: a scenario on both that `olr-cli render --fixtures fixtures/look` plays.

  They're committed; `content/packs` is untouched.

---

## Batch A: Data, terrain, bake, loading, CLI

### Task 1: `olr-look`: types, animation, fade, the engine's look

**Files:**
- Create: `crates/olr-look/{Cargo.toml,CLAUDE.md,src/lib.rs,src/anim.rs,src/frame.rs,src/engine.rs}`.
- Create: `crates/olr-look/tests/{integration.rs,anim.rs,frame.rs}` (`autotests = false`).
- Modify: `Cargo.toml` (workspace member), `xtask/layers.ron` (R1).

**Interfaces:**
- Produces:
  ```rust
  pub type Rgb = [u8; 3];
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Reach { Pixels(u16), Unlimited }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PulseShape { Sine, Triangle, Square }
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct Key { pub at_ms: u32, pub level: u16, pub colour: Option<Rgb> }
  #[derive(Clone, Debug, PartialEq, Eq)] pub enum Animation {
      Steady,
      Pulse { period_ms: u32, low: u16, high: u16, shape: PulseShape },
      Flicker { speed: u16, depth: u16 },
      Keys { keys: Vec<Key>, looped: bool },
  }
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct Light { pub colour: Rgb, pub intensity: u16, pub reach: Reach, pub animation: Animation, pub phase_ms: Option<u32> }
  #[derive(Clone, Copy, Debug, PartialEq)] pub struct Sample { pub scale: f32, pub colour: Option<Rgb> } // scale 1.0 = 100 %
  impl Animation {
      pub fn sample(&self, t_ms: f64, phase_ms: u32, identity: u64) -> Sample;
      /// A non-looping `Keys` past its last key.
      pub fn ended(&self, t_ms: f64) -> bool;
  }
  pub fn default_phase(identity: u64) -> u32;          // splitmix64(identity) % 65_536
  pub fn fade(distance: f32, reach: Reach) -> f32;     // (1 - (d/r)²)², 0 at or past r; 1 for Unlimited
  pub struct Grade { pub exposure: i16, pub contrast: u16, pub saturation: u16, pub tint: Rgb }  // Default: 0, 100, 100, white
  pub struct Ambient { pub colour: Rgb, pub intensity: u16 }
  pub enum GlowColour { Own, Tint(Rgb) }
  pub struct GlowGroup { pub id: String, pub colour: GlowColour, pub intensity: u16, pub reach: Reach, pub animation: Animation, pub phase_ms: Option<u32> }
  pub struct PlacedLight { pub at: (i32, i32), pub radius: u8, pub light: Light }
  pub struct LevelLook { pub ambient: Ambient, pub max_light: u16, pub light_steps: u8, pub softness: u8, pub grade: Grade, pub groups: Vec<GlowGroup>, pub lights: Vec<PlacedLight> }
  pub struct ProjectileLook { pub glow: Option<u8>, pub light: Option<Light>, pub end_flash: Option<Light> }
  pub struct WeaponLook { pub muzzle_light: Option<Light> }
  /// Indexed like `SimMod::projectiles` and `SimMod::weapons`.
  pub struct ModLook { pub projectiles: Vec<Option<ProjectileLook>>, pub weapons: Vec<Option<WeaponLook>> }
  impl ModLook { pub fn is_empty(&self) -> bool; }

  // frame.rs
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ViewMode { Lit, FullBright, LightOnly }
  #[derive(Clone, Copy, Debug, PartialEq)] pub struct FrameGroup { pub cast: [f32; 3], pub own_colour: bool, pub self_scale: f32, pub reach: f32 } // linear; reach f32::INFINITY = unlimited
  #[derive(Clone, Copy, Debug, PartialEq)] pub struct FrameLight { pub pos: [f32; 2], pub radius: f32, pub colour: [f32; 3], pub reach: f32 } // colour linear × intensity × animation
  #[derive(Clone, Debug, Default)] pub struct LookFrame {
      pub ambient: [f32; 3], pub max_light: f32, pub light_steps: u8, pub softness: u8,
      pub grade: Grade, pub groups: Vec<FrameGroup>, pub lights: Vec<FrameLight>, pub mode: ViewMode,
  }
  pub const MAX_LIGHTS: usize = 256;
  impl LookFrame {
      /// Clears and fills the level's part at clock `t_ms` (ambient, grade, settings, groups, level lights); keeps capacity.
      pub fn begin(&mut self, level: Option<&LevelLook>, t_ms: f64);
      pub fn push(&mut self, light: FrameLight);
      /// Keeps the lights whose reach touches `view` (world rect x, y, w, h), then the first MAX_LIGHTS, order kept.
      pub fn cull(&mut self, view: [f32; 4]);
  }
  pub fn light_at(light: &Light, t_ms: f64, identity: u64, pos: [f32; 2], radius: f32) -> Option<FrameLight>; // None when ended or zero
  pub fn srgb_to_linear(c: u8) -> f32;

  // engine.rs (R13)
  pub const FLOOR: u8 = 50;
  pub fn explosion_light(size: i32) -> Light;
  pub fn muzzle_light() -> Light;
  ```

- [ ] **Step 1: Write the failing tests.**
  - `anim.rs`:
    - `pulse_sine_at_quarter_points`: `Pulse(1000, 60, 100, Sine)` at 0 / 250 / 500 / 750 ms, phase 0, gives 0.80 / 1.00 / 0.80 / 0.60 ± 1e-4;
    - `square_and_triangle_shapes`;
    - `keys_interpolate_and_end`: `[(0,100),(150,40),(400,0)]` not looped: 75 ms → 0.70; 400 ms → 0.0; `ended(401.0)`; looped: 450 ms equals 50 ms;
    - `keys_carry_colour`;
    - `flicker_is_deterministic_and_bounded`: 10 000 samples with `depth: 25` all in [0.75, 1.0]; two runs equal; identities 1 and 2 differ;
    - `phase_shifts_time`;
    - `default_phase_spreads` (ten identities, ten different phases);
    - `hostile_animations_never_panic` (`period_ms: 0` is treated as 1, empty keys, `u32::MAX` times, NaN `t_ms` gives `Steady`).
  - `frame.rs`:
    - `fade_profile` (0 → 1, r/2 → 0.5625, r → 0, `Unlimited` → 1);
    - `begin_fills_level_part`;
    - `cull_keeps_order_and_cap` (300 lights, 10 off-view: 256 kept, in order, none off-view);
    - `a_light_far_off_view_whose_reach_touches_it_is_kept`;
    - `engine_numbers_match_r13`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-look`. Expected: FAIL (crate missing).
- [ ] **Step 3: Implement.**
  - Sine is `mid + amp × sin(2π t / period)`, where `mid = (low + high) / 2` and `amp = (high - low) / 2`, so 0 ms is the midpoint, rising.
  - Flicker: smoothstep-interpolated value noise over buckets of `1000 / speed` ms, from `splitmix64(identity ^ bucket)`.
  - `olr_math` provides `sinf`/`cosf`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-look`, `cargo build -p olr-look --target wasm32-unknown-unknown` and `cargo xtask layers`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-look: look types, animation and the engine's lights`.

### Task 2: Level look in `olr-format`

**Files:**
- Create: `crates/olr-format/src/levels/look.rs` (authoring types, checks, `compile_look`, `lift_look`).
- Modify:
  - `levels/mod.rs`: `LevelSource.look`, `.glow`, `LevelObject::Light`;
  - `levels/files.rs`: `LevelFile.look`, reading and writing `look/*.png`, `file_paths`;
  - `canonical_text`;
  - `olr-pack`'s validation (R15).
- Test: `crates/olr-format/tests/look.rs` (a `mod` of `tests/integration.rs`).

**Interfaces:**
- Consumes: Task 1's types; `olr_terrain::GlowLayers` (Task 4). **Do Task 4 before this task;** Batch A's order is 1, 4, 2, 3, 5, 6, 7, 8.
- Produces:
  ```rust
  // authoring (serde, deny_unknown_fields, every field doc-commented for format-docs)
  pub struct LookSource { ambient: AmbientSource, max_light: u16 /*default 100*/, light_steps: u8, softness: u8, grade: GradeSource, glow_groups: Vec<GlowGroupSource> }
  pub struct LightSource { colour: (u8,u8,u8), intensity: u16, reach: ReachSource, animation: AnimationSource /*default Steady*/, phase_ms: Option<u32> }
  pub enum ReachSource { Pixels(u16), Unlimited }
  pub enum AnimationSource { Steady, Pulse { period_ms, low, high, shape }, Flicker { speed, depth }, Keys { keys: Vec<KeySource>, looped: bool } }
  pub enum GlowColourSource { Own, Rgb(u8, u8, u8) }
  // LevelObject::Light { at: (i32,i32), radius: u8 /*default 2*/, colour, intensity, reach, animation, phase_ms, notes }
  pub struct LevelSource { /* … */ pub look: Option<LookSource>, pub glow: Option<olr_terrain::GlowLayers> }
  pub const LOOK_FILES: [&str; 4] = ["look/glow.png", "look/glow-back.png", "look/groups.png", "look/groups-back.png"];
  pub fn compile_look(src: &LevelSource) -> Result<Option<olr_look::LevelLook>, FormatError>;
  pub fn check_look(src: &LevelSource) -> Result<(), FormatError>;
  ```
  - The RON in the spec's §Level packs is the canonical form. Defaults are skipped when writing.
  - Glow images are 8-bit grey PNGs: `glow` and `glow-back` at the art size, `groups` and `groups-back` at the world size.

- [ ] **Step 1: Write the failing tests.**
  - `the_spec_example_round_trips`: the spec's `look` and `Light` RON parse, `canonical_text` gives them back byte for byte, and `compile_look` gives the expected `LevelLook`.
  - `levels_without_look_are_byte_identical`: every `content/packs` level reads and writes to the same bytes.
  - `glow_images_round_trip`.
  - `limits_are_enforced` (each Global Constraints limit, one past it: an error naming the field).
  - `hostile_look_files_are_errors_never_panics` (Review Focus 1's cases).
  - `look_files_are_hashed_and_known` (`content_hash` changes when `look/glow.png` changes; `validate` gives no unknown-file warning for them).
- [ ] **Step 2:** Run `cargo nextest run -p olr-format look`. Expected: FAIL.
- [ ] **Step 3: Implement.** Group indices are checked against `glow_groups.len()` (an index ≥ len is an error naming `look/groups.png` and the first bad pixel). Regenerate the schema docs with `cargo xtask format-docs`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-format -p olr-pack`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-format: the level look and light objects`.

### Task 3: Mod look in `olr-format`

**Files:**
- Modify: `crates/olr-format/src/mods/types.rs` (`Projectile.look`, `Weapon.look`), `mods/compile.rs` (`compile_mod_look`).
- Test: `crates/olr-format/tests/look.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub struct ProjectileLookSource { glow: Option<u8>, light: Option<LightSource>, end_flash: Option<LightSource> }
  pub struct WeaponLookSource { muzzle_light: Option<LightSource> }
  // Projectile.look / Weapon.look: Option<…>, #[serde(default, skip_serializing_if = "Option::is_none")]
  /// Keyed by ModDef projectile id and weapon index; olr-content indexes it like SimMod (Task 6).
  pub struct ModLookSource { pub projectiles: BTreeMap<String, olr_look::ProjectileLook>, pub weapons: Vec<Option<olr_look::WeaponLook>> }
  pub fn compile_mod_look(src: &ModSource) -> Result<ModLookSource, FormatError>;
  ```

- [ ] **Step 1: Write the failing tests.**
  - `the_spec_mod_example_round_trips`;
  - `bundled_mods_are_unchanged` (the equivalence tests and snapshots pass untouched; `compile_mod_look` of each is empty);
  - `an_inline_projectile_look_compiles_under_its_key`;
  - `mod_look_limits`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-format look equivalence`. Expected: FAIL.
- [ ] **Step 3: Implement.** `lift_mod` keeps `look`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-format`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-format: projectile and weapon looks`.

### Task 4: Glow in the colour terrain (`olr-terrain`)

**Files:**
- Modify: `crates/olr-terrain/src/color.rs` (or a new `color/glow.rs`).
- Test: `crates/olr-terrain/tests/glow.rs`.

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct GlowLayers { pub art: (u32, u32), pub world: (u32, u32), pub front: Vec<u8>, pub back: Vec<u8>, pub groups: Vec<u8>, pub groups_back: Vec<u8> }
  impl GlowLayers { pub fn empty(art: (u32, u32), world: (u32, u32)) -> Self; pub fn fits(&self, art: (u32, u32), world: (u32, u32)) -> bool; }
  impl ColorMap {
      /// None removes the glow; a layers value that does not fit the map is refused (returns false), never a panic.
      pub fn set_glow(&mut self, glow: Option<GlowLayers>) -> bool;
      /// The composite strength, art size, row-major.
      pub fn glow(&self) -> Option<&[u8]>;
      /// The composite group per world pixel.
      pub fn glow_groups(&self) -> Option<&[u8]>;
      /// `replace_rect`'s glow part (the editor), same rectangle.
      pub fn replace_glow_rect(&mut self, r: Rect, glow: &GlowLayers);
  }
  ```
- **Rules** (spec §Glow):
  - `Carve` copies the backdrop's glow and group where it copies back art; the rim colour keeps the pixel's glow.
  - `Dirt` and `GreenDirt` set glow 0.
  - `stain` and `stain_image` keep glow.
  - `rebuild_area` takes front or back glow by material.
  - The material's `Terrain` drives which.
  - Composite memory: 3 bytes per art pixel plus 3 per world pixel.

- [ ] **Step 1: Write the failing tests:**
  - `carving_reveals_the_backdrop_glow`;
  - `placed_dirt_has_no_glow`;
  - `stains_keep_glow`;
  - `glow_follows_rebuild_and_replace` (Review Focus 2);
  - `a_misfit_glow_is_refused`;
  - a proptest: `random_edits_keep_glow_equal_to_a_rebuild` (random carves and dirt; then the composite glow equals one rebuilt from the material over the whole map);
  - `no_glow_costs_nothing` (`set_glow(None)` maps behave and hash exactly as before).
- [ ] **Step 2:** Run `cargo nextest run -p olr-terrain glow`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo nextest run -p olr-terrain` and `cargo build -p olr-terrain --target wasm32-unknown-unknown`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-terrain: glow as a channel of the colour terrain`.

### Task 5: Glow in the composition and the bake (`olr-format`, `olr-edit`)

**Files:**
- Modify:
  - `crates/olr-format/src/levels/composition.rs`: `glow` on `Image` and `Drawn`; `glow_file(id) = "editor/glow/<id>.png"`; `element_files`; checks;
  - `crates/olr-edit/src/compose.rs`: `Decoded` gains the glow mask; the bake writes `LevelSource.glow`;
  - `crates/olr-edit/src/element.rs`: `set_glow`, `paint_glow`;
  - `crates/olr-cli/src/element_cmd.rs`: `set --glow --glow-group`, `glow`.
- Test: `crates/olr-edit/tests/glow.rs`, `crates/olr-cli/tests/element.rs`.

**Interfaces:**
- Produces:
  ```rust
  // olr_format::levels::composition
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct ElementGlow { pub strength: u8 /*1..=100*/, pub group: String, #[serde(default, skip_serializing_if = "d::is_false")] pub mask: bool }
  // ElementKind::Image { …, glow: Option<ElementGlow> }, ElementKind::Drawn { …, glow: Option<ElementGlow> }
  pub fn glow_file(id: &str) -> String;
  // olr_edit::element
  pub fn set_glow(src: &mut CompositionSource, level: &mut LevelSource, id: &str, glow: Option<ElementGlow>) -> Result<(), EditError>;
  /// Paints the element's glow mask (art resolution of its box): `strength` 0-100 over `shape`; makes the mask (all 100) first when missing.
  pub fn paint_glow(src: &mut CompositionSource, id: &str, shape: &Shape, strength: u8) -> Result<Rect, EditError>;
  ```
- **The bake** (spec §Composition):
  - per world pixel, the topmost covering element's group (the index of `group` in `look.glow_groups`; an unknown id is a `pack check` error and bakes no glow);
  - per art pixel, the strength `strength × mask / 100` → 0-255;
  - back glow by the backdrop rule;
  - none when nothing glows (`LevelSource.glow = None`).

  `stale()` covers the look files. `level element glow <pack> <id> (--rect|--circle) --strength PCT`.

- [ ] **Step 1: Write the failing tests:**
  - `glow_follows_the_channel_rules` (topmost wins; a background element is the backdrop; carving reveals it);
  - `glow_region_bake_equals_full_bake` (proptest, extending the existing one);
  - `no_glow_writes_no_look_files`;
  - `composed_fixture_is_byte_identical` (`fixtures/edit/composed-level`);
  - `an_unknown_group_is_an_error`;
  - `cli_sets_and_paints_glow`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit glow` and `cargo nextest run -p olr-cli element`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo nextest run -p olr-format -p olr-edit -p olr-cli`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: glow in the composition and the bake`.

### Task 6: Loading the look (`olr-content`)

**Files:**
- Modify:
  - `crates/olr-content/src/maps.rs`: `LoadedLevel.look`, `.glow`; `color_map` sets the glow;
  - `mods.rs`: `LoadedMod.look`.
- Test: `crates/olr-content/tests/look.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub struct LoadedLevel { /* … */ pub look: Option<olr_look::LevelLook>, pub glow: Option<olr_terrain::GlowLayers> }
  pub struct LoadedMod { /* … */ pub look: olr_look::ModLook }   // indexed like SimMod (by the id → index map used to build SimMod)
  impl LoadedLevel { pub fn has_look(&self) -> bool; }
  ```

- [ ] **Step 1: Write the failing tests:**
  - `bundled_content_has_no_look` (every level `look == None`, `glow == None`; every mod `look.is_empty()`);
  - `a_level_look_loads`;
  - `a_broken_glow_loads_without_glow` (Review Focus 1: one warning in `warnings`, the level plays);
  - `mod_look_is_indexed_like_simmod`;
  - `the_color_map_carries_the_glow`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-content look`. Expected: FAIL.
- [ ] **Step 3:** Implement. `from_source` (the editor's levels) carries look and glow too.
- [ ] **Step 4:** Run `cargo nextest run -p olr-content`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-content: load level and mod looks`.

### Task 7: Look operations and their CLI (`olr-edit`, `olr-cli`)

**Files:**
- Create: `crates/olr-edit/src/look.rs`.
- Modify: `crates/olr-cli/src/level_cmd.rs` (`level look`, `level group`, `level light`).
- Test: `crates/olr-edit/tests/look.rs`, `crates/olr-cli/tests/level.rs`.

**Interfaces:**
- Produces:
  ```rust
  // olr_edit::look (each validates with Task 2's checks; errors name the field)
  pub fn set_look(level: &mut LevelSource, look: Option<LookSource>) -> Result<(), EditError>;
  pub fn add_group(level: &mut LevelSource, group: GlowGroupSource) -> Result<usize, EditError>;   // refuses a duplicate id
  pub fn set_group(level: &mut LevelSource, id: &str, group: GlowGroupSource) -> Result<(), EditError>;
  /// Removing a group renumbers the group maps; elements naming it lose their glow (composition updated by the caller via the returned ids).
  pub fn remove_group(level: &mut LevelSource, id: &str) -> Result<Vec<String>, EditError>;
  pub fn add_light(level: &mut LevelSource, at: (i32, i32), light: LightSource, radius: u8) -> Result<usize, EditError>;
  pub fn set_light(level: &mut LevelSource, index: usize, at: (i32, i32), light: LightSource, radius: u8) -> Result<(), EditError>;
  ```
  - `remove_light` is the existing `remove_object`.
  - The CLI is the spec's §Agent operations block, verbatim flags. `--animation` parses `AnimationSource` RON. Each command saves the pack (and rebakes a composed level).

- [ ] **Step 1: Write the failing tests:**
  - `group_ids_are_unique`;
  - `removing_a_group_renumbers_and_unglows`;
  - `lights_round_trip_through_the_cli`;
  - `look_cli_sets_every_field`;
  - `bad_cli_values_are_errors` (a 101 % ambient; an animation that doesn't parse).
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit look` and `cargo nextest run -p olr-cli level`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the same commands. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-cli: level look, group and light commands`.

### Task 8: No name over one's own worm (`olr-game`)

**Files:**
- Modify: `crates/olr-game/src/overlay.rs` (`paint`, `worm_label`).
- Test: its unit tests.

**Interfaces:**
- Produces: `pub fn worm_label<'a>(i: usize, local: Option<usize>, name: &'a str, weapon: Option<&'a (usize, String)>) -> Option<(&'a str, Color32)>`.
  - The local worm gets `None`, except its weapon name while changing (spec decision 5).
  - The reload bar keeps its place under where the label sits.

- [ ] **Step 1: Write the failing tests:**
  - `own_worm_has_no_name_label`;
  - `own_worm_shows_its_weapon_while_changing`;
  - `other_worms_keep_their_names`;
  - `the_reload_bar_stays`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-game overlay`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo nextest run -p olr-game overlay`. Expected: PASS. The UI goldens are unchanged (names are egui over the world, not in `hud_snapshots`); confirm with `cargo xtask check`.
- [ ] **Step 5:** Commit: `olr-game: no name over the player's own worm`.

Batch A ends: `cargo xtask check`; a background review.

---

## Batch B: The renderer

### Task 9: Shades, the second target and the look textures (`olr-scene`, `olr-fx`, `olr-render`)

**Files:**
- Modify:
  - `crates/olr-scene/src/draw_list.rs`: `Shade`, the `shade` fields (every item `Shade::LIT` here), `describe`;
  - the item constructors across `olr-scene` and `olr-fx` (build errors guide). Setting markers, glows and floors is Task 15's, and happens only in lit matches;
  - `crates/olr-render/src/frame.rs`: `Instance.shade`;
  - `world.wgsl`: the second output;
  - `renderer.rs`: `set_lighting`, the glow, group and solid textures, cropping;
  - `renderer/pass.rs`: the `look` parameter (R6).
- Modify every caller of `render`, `render_in`, `render_to_image` and `render_to_png` (`olr-game`, `olr-cli`, tests) to pass `None`.
- Test: `crates/olr-render/tests/gpu.rs` (`look_*`), `src/frame.rs` unit tests, `crates/olr-scene` snapshot tests (unchanged).

**Interfaces:**
- Consumes: `olr_look::LookFrame` (Task 1), `ColorMap::glow`/`glow_groups` (Task 4).
- Produces:
  ```rust
  // olr_scene
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Shade { pub self_light: u8, pub marker: bool }
  impl Shade { pub const LIT: Shade; pub const GLOW: Shade /*100, false*/; pub const MARKER: Shade /*0, true*/; }
  // Sprite { …, pub shade: Shade }, Line { …, pub shade: Shade }
  // olr_render
  pub struct Instance { pub dst: [f32; 4], pub src: [f32; 4], pub tint: [f32; 4], pub shade: [f32; 2] }  // raw() -> [f32; 14]
  impl Renderer {
      pub fn set_lighting(&mut self, on: bool);   // off by default: no look textures, today's passes
      pub fn render(&mut self, list: &DrawList, look: Option<&LookFrame>, target: &wgpu::TextureView, size: (u32, u32));
      pub fn render_in(&mut self, list: &DrawList, look: Option<&LookFrame>, target: &wgpu::TextureView, area: [u32; 4]);
  }
  pub fn look_texture_sizes(map_world: (u32, u32), art_scale: u32, max_dim: u32) -> LookTextureSizes; // cropped like the terrain
  ```
- `upload_terrain` and `update_terrain` also upload, while lighting is on:
  - the glow strength (`R8Unorm`, art);
  - the groups and the solid cells (`R8Uint`, world);
  - for the same dirty rectangles (art rectangles mapped to world ones).

  A map without glow uploads zeros.

- [ ] **Step 1: Write the failing tests:**
  - `instance_shade_encoding` (unit);
  - `look_textures_crop_like_the_terrain` (Review Focus 4; unit geometry and GPU with `max_texture_dimension_2d` forced small through `Offscreen::open`'s limits);
  - `look_none_draws_todays_bytes` (GPU: Dirt Level's `dig` frame with `look: None` equals `fixtures/render/frame-dig.png` exactly, and with lighting on but `look: None` too);
  - `look_textures_follow_dirty_rects` (GPU readback after a carve).
- [ ] **Step 2:** Run `cargo nextest run -p olr-render` and `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu look`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The second target is `Rg8Unorm`.
  - The terrain's self-light is `strength × group self_scale`. Without a look the terrain writes 0 and nothing reads the target.
  - Without a look the second target is not attached at all.
- [ ] **Step 4:** Run `cargo nextest run -p olr-scene -p olr-fx -p olr-render`, the GPU tests, and `cargo xtask check` (the goldens and the insta snapshots unchanged). Expected: PASS.
- [ ] **Step 5:** Commit: `olr-render: item shades and the look textures`.

### Task 10: The light grid, composite and grade (`olr-render`)

**Files:**
- Create: `crates/olr-render/src/look/{mod.rs,composite.rs,finish.rs}`, `look/composite.wgsl`, `look/finish.wgsl`.
- Modify: `renderer/pass.rs` (the passes when `look` is `Some`; the upscale reads the lit target), `timing.rs` (one more slot pair, bench only).
- Test: `crates/olr-render/tests/gpu.rs` (`look_composite_*`), unit tests of the grade math.

**Interfaces:**
- Consumes: Task 9.
- Produces:
  ```rust
  /// The view's light at world resolution (view + MARGIN), RGBA16Float, rebuilt each frame.
  pub struct LightGrid { pub origin: (i32, i32), pub size: (u32, u32) }
  impl Renderer { pub fn light_grid(&self) -> Option<(&wgpu::Texture, LightGrid)>; }
  impl Offscreen { pub fn read_light(&self, renderer: &Renderer) -> Option<(LightGrid, Vec<[f32; 3]>)>; }
  /// CPU reference of the per-texel model (tests): spec §The picture.
  pub fn shade_texel(albedo: [u8; 4], light: [f32; 3], self_light: f32, marker: bool, grade: &Grade, max_light: f32) -> [u8; 4];
  ```
- **The finish pass:**
  - light = ambient (+ the contributions Tasks 11-12 add);
  - then `softness` (a separable box blur of radius `softness` world pixels);
  - then `light_steps` bands each channel (`floor(l × steps + 0.5) / steps` over `[0, max_light]`);
  - then the cap.
- **The composite pass:**
  - per art texel, the light sampled bilinearly at the texel centre in world coordinates;
  - `shade_texel`'s model in linear colour;
  - the grade: exposure as `2^(e/100)`, contrast around 0.5, saturation around Rec. 709 luma, tint multiply;
  - into an `Rgba8Unorm` lit target.
- **The view modes:**
  - `FullBright` skips the light (light = 1);
  - `LightOnly` uses albedo white.

- [ ] **Step 1: Write the failing tests:**
  - `shade_texel_table` (unit; full light is identity for every byte; self-light 1 is identity in the dark; markers ignore the grade; exposure +100 doubles a mid-grey in linear);
  - `look_full_ambient_is_identity` (GPU: the `dig` frame with a `LookFrame` of ambient (255,255,255) at 100 %, no groups, no lights, default grade equals `frame-dig.png` byte for byte);
  - `look_ambient_darkens` (ambient 30 %: a background texel's bytes match `shade_texel`, ± 1);
  - `look_steps_band_the_light`;
  - `look_softness_blurs`;
  - `look_modes` (`FullBright` equals no-look bytes; `LightOnly` is grey where the ambient is);
  - `same_state_same_bytes` (render twice).
- [ ] **Step 2:** Run `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu look_composite` and the unit tests. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the same commands. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-render: the light grid, composite and grade`.

### Task 11: Placed lights with shadow maps (`olr-render`)

**Files:**
- Create: `crates/olr-render/src/look/lights.rs`, `look/shadow.wgsl`, `look/lights.wgsl`.
- Test: `crates/olr-render/tests/gpu.rs` (`look_lights_*`).

**Interfaces:**
- Consumes: `LookFrame::lights` (already culled by the caller), the solid texture.
- Produces: no new public items. A storage buffer of up to `MAX_LIGHTS` lights.
- **The shadow map:** one row per light of `SHADOW_RAYS` (512) distances, from the light's radius outward in world-pixel steps through the solid texture, to its reach or the level's edge (`Unlimited`: the level's diagonal).
- **The light pass:** adds `colour × fade(d, reach)` to every world pixel of the light grid nearer than the shadow distance at its angle (full within `radius`). A light whose centre is solid adds nothing.

- [ ] **Step 1: Write the failing tests** (a generated level: a background room with a rock wall, built in the test):
  - `a_wall_blocks_a_light` (behind the wall: ambient exactly; in front: more);
  - `reach_bounds_a_light` (inside reach > ambient; at and past reach: ambient);
  - `an_off_view_light_reaching_the_view_lights_it`;
  - `a_light_against_a_wall_lights_the_room` (radius 2, its centre 1 px from rock);
  - `placed_lights_at_the_edges` (Review Focus 3: outside the level, inside rock, `Unlimited` on a 4096 × 4096 generated level, `MAX_LIGHTS` at once: no validation error, deterministic bytes);
  - `light_is_camera_independent` (the same world pixel's light with the camera moved 1, 7, 31 and 200 px).
- [ ] **Step 2:** Run `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu look_lights`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the same command. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-render: placed lights with shadow maps`.

### Task 12: Glow light, radiance cascades over the whole level (`olr-render`)

**Files:**
- Create: `crates/olr-render/src/look/glow_light.rs`, `look/emission.wgsl`, `look/cascades.wgsl`.
- Test: `crates/olr-render/tests/gpu.rs` (`look_glow_*`), unit tests of the cell size.

**Interfaces:**
- Consumes: the glow, group and solid textures; `LookFrame::groups`.
- Produces:
  ```rust
  pub const GI_CELL_BUDGET: u32 = 262_144;
  /// The smallest power of two s in 1, 2, 4, 8, … with ceil(w/s) × ceil(h/s) ≤ GI_CELL_BUDGET.
  pub fn gi_cell(world: (u32, u32)) -> u32;
  ```
- **The emission grid** (spec §Glow light):
  - per cell, the mean over its world pixels of each glowing pixel's centre-texel colour (or tint) × strength × the group's `cast`;
  - its opacity is the share of solid world pixels;
  - its reach is the largest of its pixels' group reaches;
  - a glowing cell stops a ray.
- **Radiance cascades:** fixed to the level's grid (cell `s`), recomputed every frame. The finish pass adds the result, sampled bilinearly at world-pixel centres. Skipping the recompute when the inputs did not change is allowed (spec).

- [ ] **Step 1: Write the failing tests:**
  - `gi_cell_sizes` (504 × 350 → 1, 1350 × 550 → 2, 4096 × 4096 → 8, 0 × 0 → 1);
  - `glow_lights_its_surroundings` (a lava strip lights the room above it);
  - `a_wall_blocks_glow_light`;
  - `glow_reach_fades`;
  - `a_deeper_sky_lights_the_same` (a sky 16 and 64 rows deep give the same light below it, ± 1 %);
  - `no_popping` (a sky 600 px above the view lights a room through a shaft identically wherever the camera is);
  - `group_animation_scales_glow` (`self_scale` and `cast` at 50 % halve the self-light and the cast light);
  - `glow_light_is_deterministic`.
- [ ] **Step 2:** Run `OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu look_glow` and `cargo nextest run -p olr-render gi_cell`. Expected: FAIL.
- [ ] **Step 3:** Implement. Record the cascade parameters chosen in the module docs.
- [ ] **Step 4:** Run the same commands. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-render: whole-level glow light with radiance cascades`.

### Task 13: `look-preview` and the level part of the frame (`olr-present`, `olr-cli`)

**Files:**
- Create: `crates/olr-cli/src/look_preview.rs`.
- Modify: `crates/olr-cli/src/main.rs`; `crates/olr-present/src/lib.rs` (`Presentation::look_frame`, level part only here; `upload` calls `set_lighting`).
- Test: `crates/olr-cli/tests/look_preview.rs`, `crates/olr-present/tests/present.rs`.

**Interfaces:**
- Produces:
  ```rust
  impl Presentation {
      /// Some exactly when the level or mod has a look (R7); built into a reused LookFrame, no allocation once warm.
      pub fn look_frame(&mut self, world: &World, alpha: f32) -> Option<&LookFrame>;
      pub fn has_look(&self) -> bool;
  }
  ```
  - `olr-cli look-preview <pack> --at X,Y [--time MS] [--view lit|full|light] [--size WxH] [--content DIR] --out f.png` draws the level's terrain alone (no worms) with `LookFrame::begin(level.look, time)` at `Viewport::GAME`, through `Offscreen`.
  - The spec writes `--view full|light`; `lit` is added as the default.

- [ ] **Step 1: Write the failing tests:**
  - `look_frame_is_none_for_bundled_content`;
  - `look_frame_level_part` (a level with one group and one light: the groups' values at t, the light);
  - `look_frame_does_not_allocate_when_warm` (with `olr-scene`'s no-alloc counter pattern);
  - `look_preview_writes_a_png` (GPU tier; skips without an adapter).
- [ ] **Step 2:** Run `cargo nextest run -p olr-present look` and `cargo nextest run -p olr-cli look_preview`. Expected: FAIL.
- [ ] **Step 3:** Implement. `olr-game`'s `Gpu::draw` and `olr-cli render` pass `present.look_frame(..)`.
- [ ] **Step 4:** Run the same commands. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-cli: look-preview, and the level's look in play`.

### Task 14: The look test content, goldens and the contact sheet (Stop 1)

**Files:**
- Create: `fixtures/look/levels/look-test/` (R16).
  - Build it with `olr-cli level element` and `level look/group/light` commands, recorded in `fixtures/look/make-look-test.sh` so it can be rebuilt.
  - Contents: a building of rock with two windows under a glowing sky group (`Unlimited`, warm tint), a flickering lava strip (reach 96), two lamps (one pulsing), ambient 25 %, a slight grade.
- Create: `crates/olr-cli/tests/look_golden.rs` (a `mod` of the integration binary) and `fixtures/render/look-{lit,light,full,t500}.png`.
- Modify: `xtask/src/bless.rs` (`GOLDEN_TESTS` gains `look_golden`).

- [ ] **Step 1:** Write `look_golden.rs`: `look-preview` of the test level at one position, in the three views and at `--time 500`, compared as the render goldens are. Run it with `OLR_GPU_TESTS=1 cargo nextest run -p olr-cli look_golden`. Expected: FAIL (no goldens).
- [ ] **Step 2:** Bless: `cargo xtask bless-render --reason "plan 15a: the look test level's goldens"`. Then confirm the existing goldens are byte-identical: `git diff --stat fixtures/render` shows only the new files.
- [ ] **Step 3:** Make the contact sheet into `target/look-sheet/` (not committed). Include:
  - the test level lit, light-only and full-bright at three positions;
  - six frames of 100 ms (the flicker and pulse);
  - Liero Factory and Castle Strike with a temporary dark look (ambient 20 %, a lamp) to judge world-pixel shadow edges.

  Send the sheet to the user (SendUserFile) with one paragraph on what to judge:
  - shadow coarseness;
  - glow spread;
  - lamp falloff;
  - the grade.
- [ ] **Step 4:** **Stop 1.** Wait for the user's verdict. Apply the tuning they ask for (R13's numbers, 2 × 2 shadow sampling if they want it), re-bless with the reason, and record the verdict in `docs/porting/STATUS.md`'s new section (Task 21 completes it).
- [ ] **Step 5:** Commit: `fixtures: the look test level and its goldens`.

Batch B ends: `cargo xtask check`; a background review (after Stop 1's changes).

---

## Batch C: Play wiring

### Task 15: The mod's and engine's lights in play (`olr-present`)

**Files:**
- Create: `crates/olr-present/src/look.rs` (the dynamic part of `look_frame`; the end-flash tracker).
- Modify:
  - `crates/olr-present/src/lib.rs`, `net.rs` (state loads, rollbacks);
  - `crates/olr-scene`'s `worm.rs`, `marks.rs`, `projectile.rs` and bonuses: R13's markers (crosshair, health bar, laser sight dots) and glows (muzzle flash, beams); projectile shades from `ModLook.glow` and the engine floor; worm and bonus floors;
  - `crates/olr-fx/src/draw.rs`: R13's glowing effects.

  All of these apply only when the match has a look.
- Test: `crates/olr-present/tests/look.rs`, `tests/net.rs`.

**Interfaces:**
- Consumes:
  - `LoadedMod.look` (Task 6);
  - `olr_look::engine` (Task 1);
  - `Scene::marks().muzzle(worm)`;
  - `Fx::entities()`;
  - the world's projectiles (`proj`, `serial`, drawn position).
- Produces: `SceneOptions` (or `PresentSetup`) gains `shades: Option<ShadeRules>` so the scene and effects shade only for a lit match. No new public API beyond Task 13's.
  ```rust
  pub struct ShadeRules { pub floor: u8, pub projectile_glow: Vec<Option<u8>> /* by SimMod index */ }
  ```
- **The sources**, in R9's order:
  - a projectile's light: identity = serial; its own clock = `t - spawn_time`;
  - an end flash: R8; its clock from the frame it ended; dropped when `ended`;
  - a muzzle light: the weapon's, else the engine's, while the scene's muzzle mark shows; clock from the mark's start;
  - an explosion light: `engine::explosion_light(type2)` per `Explosion` effect; identity = slot ⊕ spawn tick.

- [ ] **Step 1: Write the failing tests:**
  - `a_glowing_projectile_lights_and_glows` (the look test mod's rocket: its `FrameLight` at its drawn position, and its sprite's `Shade::self_light` 80);
  - `an_end_flash_follows_the_projectile` (shows at the last drawn position, fades by its keys, gone after);
  - `end_flashes_once_per_projectile_through_rollbacks` (Review Focus 5, with `tests/net.rs`'s hand rollback);
  - `a_loaded_state_restores_the_glow` (Review Focus 2);
  - `engine_lights_only_in_lit_matches` (a bundled match: no frame; the look test level with `olx-classic`: explosion and muzzle lights);
  - `the_light_cap_is_deterministic` (2000 lights: R9's order, the first 256 kept);
  - `floors_apply_only_in_lit_matches`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-present -p olr-scene -p olr-fx`. Expected: FAIL.
- [ ] **Step 3:** Implement. The scene and effects keep `Shade::LIT` when `shades` is `None`, so bundled matches' draw lists are unchanged (the snapshots prove it).
- [ ] **Step 4:** Run the same command and `cargo xtask check`. Expected: PASS, the goldens unchanged.
- [ ] **Step 5:** Commit: `olr-present: projectile, flash, muzzle and explosion lights`.

### Task 16: The look test mod and a play golden

**Files:**
- Create: `fixtures/look/mods/look-test-mod/` (R16: "Glow Rocket", its projectile with the spec's example `look`, its weapon with a `muzzle_light`), `fixtures/look/scenarios/rocket.ron` (a worm fires at the lava room on the look test level).
- Create: `fixtures/render/look-rocket.png`; extend `look_golden.rs`.
- Modify: `crates/olr-cli/src/gpu_bench.rs` (a `--look` scene: the look test level, 64 lights, the rocket scenario), for Task 20.

- [ ] **Step 1:** Write the failing golden test `look_rocket` (`olr-cli render rocket --fixtures fixtures/look --frame 120`).
- [ ] **Step 2:** Run it. Expected: FAIL (no golden).
- [ ] **Step 3:** Bless with `--reason "plan 15a: a glowing rocket in play"`. Check that only `look-rocket.png` is new.
- [ ] **Step 4:** Run `cargo nextest run -p olr-cli` and `cargo xtask check`. Expected: PASS.
- [ ] **Step 5:** Commit: `fixtures: the look test mod and the rocket golden`.

Batch C ends: `cargo xtask check` and `cargo xtask e2e` (the menus' autoplay plays on); a background review.

---

## Batch D: The editor

### Task 17: Look editing in the editor's state (`olr-editor`)

**Files:**
- Modify:
  - `crates/olr-editor/src/history.rs`: `Edit::Look`, `Edit::GlowMask`;
  - `ops.rs`: glow, groups, look, lights;
  - `tools.rs`: `Tool::Glow`, `Tool::Light`, R12;
  - `pointer.rs`: the brush paints the glow mask; the Light tool places and moves lights;
  - `state.rs`: actions and the preview state;
  - `document.rs`: glow masks decoded and saved.
- Test: `crates/olr-editor/tests/{state.rs,document.rs,history.rs}`.

**Interfaces:**
- Consumes: Tasks 5 and 7 (`olr_edit::element::set_glow`, `paint_glow`, `olr_edit::look::*`).
- Produces:
  ```rust
  pub enum Edit { /* … */ Look { before: Option<LookSource>, after: Option<LookSource> }, GlowMask { id: String, rect: Rect, before: Vec<u8>, after: Vec<u8> } }
  pub enum EditorAction { /* … */
      SetGlow { id: String, glow: Option<ElementGlow> },
      SetLook(Option<LookSource>),                       // the Look panel's commit
      SetGroup { id: String, group: GlowGroupSource }, AddGroup(GlowGroupSource), RemoveGroup(String),
      SetLight { index: usize, at: (i32, i32), light: LightSource, radius: u8 },
      SetViewMode(olr_look::ViewMode), Clock(ClockAction),
  }
  pub enum ClockAction { Play, Pause, Scrub(f64) }
  impl EditorState {
      pub fn view_mode(&self) -> ViewMode;
      pub fn clock_ms(&self) -> f64;
      pub fn advance_clock(&mut self, dt_ms: f64);   // the host each frame
  }
  ```
- **The rules** (spec §The level editor):
  - A new glow makes a group named after the element (`<element id>`, made unique) with `Own`, 100 %, `Pixels(64)`, `Steady`.
  - Removing a group asks first (`Dialog`). Its elements lose their glow in the same step.
  - The Glow brush uses the brush's size and shape. Strength is a tool option (0 erases).

- [ ] **Step 1: Write the failing tests:**
  - `a_new_glow_makes_a_group`;
  - `glow_brush_paints_the_mask_in_one_step`;
  - `removing_a_group_unglows_in_one_step`;
  - `lights_place_move_and_undo`;
  - `look_edits_undo`;
  - `the_clock_plays_pauses_and_scrubs`;
  - `look_survives_save_and_reopen`;
  - `locked_elements_refuse_glow`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor`. Expected: FAIL.
- [ ] **Step 3:** Implement. A glow change rebakes its rectangle and sends `Upload` as any pixel edit; a look, group or light change sends no upload (uniforms).
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: glow, groups, lights and the look in the document`.

### Task 18: The editor's look panels (`olr-editor` UI)

**Files:**
- Modify:
  - `crates/olr-editor/src/ui/properties.rs`: the Glow section; a light's properties;
  - `ui/level.rs`: the Look panel;
  - `ui/toolbar.rs`: the two tools, the view toggle, the clock;
  - `ui/canvas.rs`: light markers and the selected light's reach circle;
  - `ui/elements.rs`: lights under Objects;
  - `ui/labels.rs`.
- Test: `crates/olr-editor/tests/flows.rs`.

**Interfaces:**
- Consumes: Task 17's actions.
- Produces: `labels::{GLOW_STRENGTH, GLOW_GROUP, GROUP_COLOUR, GROUP_INTENSITY, GROUP_REACH, GROUP_ANIMATION, LOOK_AMBIENT, LOOK_MAX_LIGHT, LOOK_STEPS, LOOK_SOFTNESS, LOOK_EXPOSURE, LOOK_CONTRAST, LOOK_SATURATION, LOOK_TINT, VIEW_LIT, VIEW_FULL, VIEW_LIGHT, CLOCK_PLAY, CLOCK_SCRUB, TOOL_GLOW, TOOL_LIGHT}`.
  - The animation editor is a kind picker plus the kind's fields; `Keys` shows a small list with add and remove.

- [ ] **Step 1: Write the failing `egui_kittest` flows (by label):**
  - `glow_an_element_and_save` (check `level.ron`'s group and the bake's look files);
  - `paint_glow_with_the_brush`;
  - `place_a_light_and_edit_it`;
  - `edit_the_look_panel`;
  - `toggle_views_and_scrub_the_clock`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor flows`. Expected: FAIL.
- [ ] **Step 3:** Implement within the existing panel patterns (drafts commit once; the properties keep 260 points).
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: the look panels and tools`.

### Task 19: The canvas draws the look; Play and e2e (`olr-game`)

**Files:**
- Modify:
  - `crates/olr-game/src/editor/canvas.rs`: the `ColorMap` with glow (`replace_glow_rect` with each upload); a `LookFrame` per frame from the document's look, its lights and the editor's clock and view mode;
  - `app.rs`: `set_lighting` and the frame passed to `render_in`; the clock advanced by the frame time;
  - `setup.rs`: `load_edited` carries the look and glow;
  - `fixtures/autoplay/editor.ron`: add a glow and a light, Play, Stop.
- Test: `crates/olr-game/tests/editor_host.rs`, `tests/canvas_gpu.rs` (GPU tier), `cargo xtask e2e`.

- [ ] **Step 1: Write the failing tests:**
  - `the_canvas_frame_follows_the_document` (a light added shows in the next `LookFrame`; `FullBright` gives the no-look frame);
  - `a_glow_edit_uploads_its_rectangle`;
  - `play_carries_the_look` (the edited match's `Presentation::has_look`);
  - in `canvas_gpu.rs`: `the_canvas_draws_lit`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-game editor_host`. Expected: FAIL.
- [ ] **Step 3:** Implement. Zoomed out, the canvas uses the same whole-level glow light (no popping); placed lights are culled to the canvas's view.
- [ ] **Step 4:** Run `cargo nextest run -p olr-game`, `OLR_GPU_TESTS=1 cargo nextest run -p olr-game --test canvas_gpu` and `cargo xtask e2e`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-game: the editor's canvas draws the look`.

Batch D ends: `cargo xtask check` and `cargo xtask e2e`; a background review. Then **Stop 2**: ask the user before `cargo xtask dist`. Build it, give the Flatpak's and the Windows zip's paths, and say what to try:
- the look test level, copied and edited;
- a dark Liero Factory copy;
- the glowing rocket;
- the laptop, if they have one.

Wait for their playtest notes.

---

## Batch E: Measurements and docs

### Task 20: Measure the budget (with consent)

**Files:**
- Modify: `crates/olr-cli/src/gpu_bench.rs` (Task 16's `--look` scene; GPU timestamps around the look passes), `docs/perf/envelope.md` (a "Look" section), `fixtures/bench/baselines.ron` only through `cargo xtask bench --bless --reason`.

- [ ] **Step 1:** Ask the user before running anything in this task.
- [ ] **Step 2:** With consent, run `olr-cli gpu-bench --look` on:
  - WSL GL;
  - Windows DX12 and Vulkan (the RTX);
  - the machine's integrated GPU when `--list-adapters` shows one.

  Sizes: 1920 × 1080, 2560 × 1440 and 3840 × 2160; art scale 2 and 8.
- [ ] **Step 3:** Compare with the spec's budget table.
  - Where a budget is missed, lower `GI_CELL_BUDGET` or the shadow rays (R13) and measure again once.
  - Report to the user, as a trade-off, whether a smaller maximum map size (e.g. 2048 × 2048) or smaller maps on laptops would help. The user said limits are negotiable; don't change a map limit without their ruling.
- [ ] **Step 4:** Write the "Look" section of `docs/perf/envelope.md` with the numbers and what they mean.
- [ ] **Step 5:** Commit: `docs: the look system's cost`.

### Task 21: Docs (Batch E)

**Files:**
- Modify: `docs/porting/STATUS.md`. Add a new section, "Look system decisions (plan 15a)", with:
  - R1-R16;
  - Stop 1's and Stop 2's verdicts;
  - the cascade parameters;
  - the removal of the own name label (closer to OLX, which draws none).
- Modify: the `CLAUDE.md` of:
  - `olr-look` (new);
  - `olr-format`;
  - `olr-terrain`;
  - `olr-edit`;
  - `olr-content`;
  - `olr-scene`;
  - `olr-fx`;
  - `olr-render` (the passes, `set_lighting`, `look` parameters, budgets);
  - `olr-present`;
  - `olr-editor`;
  - `olr-game`;
  - `olr-cli`.
- Modify: the spec's Status line ("15a done …").
- Modify: `docs/formats/*.md` (regenerated in Task 2; check them).

- [ ] **Step 1:** Write the docs. No check is needed for docs only.
- [ ] **Step 2:** Commit: `docs: plan 15a's decisions`.
- [ ] **Step 3:** Ask the user before `cargo xtask full` (it covers the render goldens, the soak and the e2e on Xvfb).
