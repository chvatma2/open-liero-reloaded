# Plan 5: Presentation, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the simulation visible, audible and playable. The plan covers content loading, the colour terrain, the scene, a wgpu renderer, the classic cosmetic effects, positional audio and keyboard and gamepad input. It ends with a sandbox `rl-game` in which you walk, rope and shoot on all four levels with either mod (milestone M5).

**Architecture:**
- **Loading.** `rl-content` (L4) loads packs into ready-to-run inputs: `SimMod`, terrain, holes, decoded images and sound bytes.
- **Pure CPU layers.**
  - `rl-terrain` (L1) gains the colour image and replays the sim's `TerrainEdit`s on it.
  - `rl-scene` (L5) builds a `DrawList` from `&World`, the previous tick's snapshot and an interpolation factor, including OLX's camera.
  - `rl-fx` (L5) runs OLX's cosmetic entities from `SpawnCosmetic` events with its own RNG.
- **Devices.**
  - `rl-render` (L5, wgpu) draws a `DrawList` on screen or offscreen.
  - `rl-audio` (L5, kira) plays `Sound` events.
  - `rl-input` (L5, winit and gilrs) turns devices into `Keys` latched per tick.
- **The app.** `rl-game` (L6) wires them into a window with a fixed 100 Hz loop.
- **Agent tooling.** `rl-cli render` draws any oracle scenario frame to a PNG, so agents can see what they change.

**Tech stack:** Rust 1.99. New dependencies:
- graphics: `wgpu 30.0.1`, `winit 0.30.13`, `egui`/`egui-wgpu`/`egui-winit 0.36.2`;
- audio and input: `kira 0.12.5`, `gilrs 0.11.2`;
- helpers: `pollster 1.0.1`, `bytemuck 1.25.2`, `directories 6.0.0`.

Pin exact versions. egui-wgpu 0.36.2 uses wgpu 30.0.1 and winit 0.30.13; keep them in lockstep.

**Spec:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`. Read §4, §8, §10 (M5) and the amendments §15–§18. This plan adds §19 (E1–E12) in Task 1.

**Porting reference:** `docs/porting/olx-notes/presentation.md` (camera §1, map §2, worms §3, rope §4, projectiles §5, bonuses §6, entities §7, sounds §8, input §10, frame order and layers §11, assets, open questions). The source wins over the note; fix the note when they disagree.

**How this plan was made:** like plan 4. Each task gives:
- the OLX behaviour to reproduce, with references;
- the Rust interface;
- named tests;
- acceptance.

Implementers work test-first. Rendering correctness is checked by scene snapshots (pure CPU, in the fast gate) and render goldens (GPU, in the `full` tier).

## Global Constraints

- **Toolchain and license.** Rust 1.99, edition 2024, GPL-3.0-or-later. The crate prefix is `rl-`, and every crate has a `CLAUDE.md` and is registered in `xtask/layers.ron`.
- **Crates.**
  - Layers: `rl-content` is L4; `rl-scene`, `rl-render`, `rl-fx`, `rl-audio` and `rl-input` are L5; `rl-game` is L6.
  - Same-layer dependency: `rl-fx` → `rl-render` is allowed if needed (spec §4). Add any other same-layer pair to `layers.ron` with a reason.
- **Purity.**
  - `rl-scene` and `rl-fx` hold no GPU or OS handles, so they are unit-testable on the CPU.
  - `rl-terrain` stays `#![no_std]` and builds for wasm32.
- **The simulation never sees presentation.**
  - Nothing in L5/L6 mutates `World` except through `tick`.
  - Interpolation never feeds back.
  - Cosmetics never draw from the sim's generators.
- **Never panic on content.** Missing or odd assets get a placeholder (a magenta square, silence), logged once.
- **Performance.**
  - 1080p and 1440p at ≥ 144 FPS on the dev machine's GPU, through WSL's d3d12 GL.
  - No per-frame allocation in the render loop's steady state.
  - Terrain colour updates upload dirty rectangles only.
- **`cargo xtask check` stays fast (≈ 60 s).**
  - GPU tests and render goldens run in a new `cargo xtask full` tier.
  - Scene snapshots and colour-terrain goldens (CPU) run in `check`.
- **WSL2 (dev platform) defaults.**
  - `rl-game` uses the wgpu GL backend with `GALLIUM_DRIVER=d3d12` when it detects WSL (`/proc/sys/kernel/osrelease` contains `microsoft`), unless the user set `WGPU_BACKEND` or `GALLIUM_DRIVER`.
  - Under Wayland, `Outdated` on the first frame means reconfigure and retry.
  - These are verified behaviours (see `docs/porting/olx-notes/presentation.md` "Open questions" 15, and the M5 probe notes in Task 6).

## Decisions (spec amendment §19, written in Task 1)

| # | Decision | Reason |
|---|---|---|
| E1 | **Faithful to OLX `d1ad953f`** wherever a choice is visible in play: <ul><li>hard-locked camera, staying on the dead worm and snapping on respawn;</li><li>shake as a random offset for 0.2 s, clamped inward;</li><li>no shadows;</li><li>OLX's colours, frames and entity physics;</li><li>large explosions start invisible;</li><li>the 1024-entity cap;</li><li>THROW and PICKUP play globally at gain 1.0, other sounds positionally.</li></ul> | The user wants the old feel; this OLX build is the reference. |
| E2 | **Rendering interpolates** between the last two ticks (spec §8.2), with an option to draw the latest state like OLX. | High-refresh displays. |
| E3 | **No antialiasing in the world pass:** the rope, beams and laser sight are exact 1-world-px lines at art scale. The upscale filter is selectable: integer-nearest, sharp-bilinear (default), or a pixel-art upscaler later. | Crisp pixels at any size. |
| E4 | **Field of view:** 240 world px of visible height (fork layout); width from the aspect ratio, clamped to `max_aspect` 16:9. | Spec §8.1; matches this OLX fork. |
| E5 | **Input:** the classic 8 keys only. OLX's defaults are arrows, Shoot `LCtrl`, Jump `LAlt`, Rope `X`, SelectWeapon `LShift` (hold to cycle with left and right). Rebindable; gamepad defaults through gilrs. | Classic feel; the sim has exactly these 8 keys. |
| E6 | **New sim event data** (no draws, no behaviour change; the oracle stays green): <ul><li>`Explosion.shake`;</li><li>`CosmeticKind::Gib { worm }`;</li><li>`Dirt` gets the probe pixel to sample colour from;</li><li>every event gets the index of the physics sub-step that emitted it, and `World::substeps_last_tick()`;</li><li>`Damage` gets the clamped amount (health before);</li><li>a per-projectile-type `rot_bumped` flag (OLX's sticky `RotIncrement` 0→1);</li><li>`World::physics_time()`;</li><li>`Projectile::serial` (a spawn counter), so interpolation never blends two projectiles that reused one slot.</li></ul> | The renderer cannot recover these otherwise (presentation note, open questions 1–5 and 7; final review of plan 4). |
| E7 | **Colour terrain lives in `rl-terrain`** (`ColorMap`, no_std). It applies the same `TerrainEdit`s as the sim, in order: <ul><li>carving copies back-image pixels and paints the brown rim;</li><li>`PlaceDirt` colours come from the theme's `FrontTile`;</li><li>green dirt uses OLX's shades;</li><li>beam holes;</li><li>blood and gib stains.</li></ul> | Spec §5.4, so the same code serves the renderer and headless goldens. |
| E8 | **Cosmetic entities (`rl-fx`)** step per sub-step: motion with dt 0.011, animation with 0.010 (OLX). They use their own seeded generator, and are capped at 1024 like OLX. | Faithful look, without touching sim randomness (spec §5.5). |
| E9 | **A core asset pack** `olx-core` (new pack kind `Core`) holds OLX's shared gfx and sounds: crosshair, muzzle, explosion, smoke, chemsmoke, spawn, sparkle, giblets, hook, bonus and health icons, game sounds as `.ogg`, and the `colours.cfg` values. Mod packs keep their own sounds. | One place for shared assets; the manifest stays the single list of imported OLX content. |
| E10 | **Text in the world and HUD** (names, damage popups, the sandbox HUD) uses egui at native resolution. OLX's bitmap fonts wait for M7's classic HUD style. | Spec §8.3; the bitmap fonts are deferred by A5. |
| E11 | **Sandbox mode** in `rl-game`: <ul><li>pick a level, a mod and a skin on the command line;</li><li>one human worm plus N idle target worms;</li><li>F5 restarts; F2 toggles the debug overlay (FPS, tick time, entity counts).</li></ul> No menus yet (M7). | The M5 "done" criterion: walk, rope and shoot on all 4 levels. |
| E12 | **Audio degrades to silence** when no output device opens, logging once. On WSL, sound needs the ALSA PulseAudio plugin (`libasound2-plugins` and an `~/.asoundrc` with `pcm.!default pulse`, `ctl.!default pulse`). The user installs these; the game never requires them. | Probe on the dev machine: no ALSA→Pulse plugin is installed. |

## Review Focus

1. **Interpolation across a slot reuse or a respawn.** A projectile freed and re-spawned in the same slot within a tick, or a worm that dies and respawns elsewhere, must not be drawn sliding between the two positions. Pinned by `interpolation_never_blends_reused_slots_or_respawns` (Task 3).
2. **Terrain colour drift.** After thousands of edits (carve, dirt, green dirt, beam), the colour image must agree with the material grid everywhere: no dirt colour on background, no background colour on dirt. Pinned by `color_map_agrees_with_material_after_a_match` (Task 2).
3. **A missing or odd asset** (a skin of the wrong size, a missing projectile image, an undecodable sound) must still produce a playable game. Pinned by `missing_assets_use_placeholders` (Tasks 1 and 5).
4. **Window resizes, minimising and DPI changes** must not crash or leave the surface outdated. On WSL/Wayland the first frame may be `Outdated`. Pinned by `surface_survives_resize_and_outdated` (Task 6) and a manual check in Task 8.
5. **Long sessions.** There must be no growth in GPU memory, entity count or audio voices over a 30-minute sandbox run. Pinned by `soak_sandbox_10_minutes_headless` (Task 8, `xtask full`).

## File structure after this plan

```text
crates/rl-content/     L4  load packs → SimMod, Terrain, ColorMap inputs, images, sounds, skins (moves rl-scenario's load_mod here)
crates/rl-terrain/     L1  + color.rs: ColorMap, apply TerrainEdit / stains, dirty rects
crates/rl-scene/       L5  camera, interpolation, DrawList (sprites, lines, terrain view), sprite frame math
crates/rl-fx/          L5  classic entities (ENT_*), own RNG, per-sub-step
crates/rl-render/      L5  wgpu: terrain textures, sprite atlas, instanced sprites, lines, upscale, offscreen
crates/rl-audio/       L5  kira: Sound events → positional playback, voice limits, silent fallback
crates/rl-input/       L5  winit/gilrs → Keys, bindings, latching
crates/rl-game/        L6  window, 100 Hz loop, sandbox mode, debug overlay (egui)
crates/rl-cli/              + `render <scenario> --frame N --out f.png`
crates/rl-import-gfx/       + core pack import (gfx, sounds, colours.cfg)
```

## After this plan

**Plan 6** covers Deathmatch, bots, replays and the match runner (M6). **Plan 7** covers menus, the HUD, profiles and results (M7, v0.1).

---

### Task 1: Sim event data, the core pack, `rl-content`

**Files:**
- Modify: `crates/rl-sim/src/{events.rs,world.rs,projectile.rs,effects.rs,damage.rs,mod_rt.rs,...}` (E6)
- Create: `crates/rl-content/` (L4); Modify: `crates/rl-scenario` (use `rl-content` instead of its own loader), `xtask/layers.ron`
- Modify: `crates/rl-data/src/manifest.rs` (`PackKind::Core`), `crates/rl-pack` (a core pack reader and writer), `crates/rl-import-gfx` (core import), `content/olx-manifest.ron` (add `data/frontend/colours.cfg` and anything else the core pack needs), `crates/rl-cli` (import writes `olx-core`), corpus snapshot
- Modify: the spec (append §19 with E1–E12)

**Interfaces:**
- Produces, in `rl-sim`:
  - the E6 additions:
    - `Explosion { pos, damage, shake }`;
    - `CosmeticKind::Gib { worm }`;
    - `Dirt { probe: (i32, i32) }`;
    - `Event` carries `substep: u8`;
    - `World::substeps_last_tick() -> u8`;
    - `Damage` gains `dealt: f32` (`min(health_before, damage)`);
    - `SimMod`/`World` expose `rot_bumped(projectile_type) -> bool`;
    - `World::physics_time() -> AbsTime`;
    - `Projectile::serial() -> u32`.
  - **Events leave `World` state.**
    - `TickEvents` (48 128 preallocated entries, about 1.15 MB) moves out of what `World::clone` copies: either a caller-owned buffer, `tick(&mut self, keys, out: &mut TickEvents)`, or a field excluded from `Clone`, `PartialEq` and the checksum.
    - The non-state `Draws::spawn_work` counter likewise leaves `PartialEq`.
    - `world_tests` clone equality and `no_alloc` keep passing.
  - All existing oracle and smoke tests stay green, and no draw is added.
- Produces, in `rl-content`:
  - `Content::open(root: &Path) -> Result<Content>`, which lists packs.
  - `Content::load_mod(id) -> Result<LoadedMod>`:
    - `LoadedMod { sim: Arc<SimMod>, images: BTreeMap<String, RgbaImage>, sounds: BTreeMap<String, Vec<u8>> }`;
    - mod sounds are keyed by `SimMod` sound name; `olx:` names resolve to the core pack.
  - `Content::load_level(id) -> Result<LoadedLevel>`: material, front and back art, `art_scale`, theme id.
  - `Content::load_theme(id) -> Result<LoadedTheme>`: holes, greenball, `front_tile`, `back_tile`.
  - `Content::load_skin(id) -> Result<LoadedSkin>`.
  - `Content::core() -> &CorePack`: sprites by name, game sounds, colours.
  - `Content::new_world(setup: &SandboxSetup) -> Result<World>`, where `SandboxSetup { mod_id, level_id, seed, worms: Vec<SpawnSpec> }`. Spawn positions come from the sim's `FindSpot` when not given.
  - Placeholders for missing assets (Review Focus 3).

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `events_carry_e6_data` (an explosion's shake equals its action's `shake`; gibs name the victim; every event's substep is below `substeps_last_tick()`; `dealt` ≤ the health before; `rot_bumped` flips after the first random-rotation spawn);
  - `projectile_serials_differ_across_slot_reuse`;
  - in `rl-content`: `loads_every_manifest_item` (skips without content, with a notice), `missing_assets_use_placeholders`, `new_world_spawns_worms_on_free_spots`.

  Then run `cargo nextest run -p rl-sim -p rl-content`: FAIL.
- [ ] **Step 2: GREEN.** Implement. Move `rl-scenario`'s `load_mod`/`check_frames` into `rl-content`, with `rl-scenario` depending on it.
- [ ] **Step 3: Core import.** Implement the core pack import (E9). Re-run `cargo xtask check`, which re-imports content; accept the corpus snapshot only for the intended additions.
- [ ] **Step 4: Commit.** Append spec §19, update STATUS (E6 rows), commit (`rl-content: load packs into worlds and assets; sim event data for rendering`).

### Task 2: Colour terrain (`rl-terrain::ColorMap`)

**Files:**
- Create: `crates/rl-terrain/src/color.rs`; Modify: `crates/rl-terrain/src/lib.rs`, `crates/rl-terrain/CLAUDE.md`

**Interfaces:**
- Produces:
  - `ColorMap::new(front: &RgbaImage, back: &RgbaImage, art_scale: u32, theme: &ThemeArt)`, holding the composite image at art scale (presentation §2.1).
  - `apply(&mut self, edit: &TerrainEdit, material_after: &Terrain)`, where `TerrainEdit` is `rl-sim`'s:
    - `rl-terrain` cannot depend on `rl-sim`, so define a mirror type `ColorEdit` in `rl-terrain`, and have `rl-scene` or `rl-content` convert;
    - carve: back pixels plus the brown rim;
    - dirt: `FrontTile` colours;
    - green dirt: OLX shades;
    - beam holes.
  - `stain(x, y, color)` for blood and gibs (presentation §2.2, §7).
  - `take_dirty() -> impl Iterator<Item = Rect>` (dirty rectangles at art scale).
  - `rebuild_area(rect, material: &Terrain)`: if a tick reports `gameplay_dropped() > 0`, the caller rebuilds `lost_area()` from the material grid: back pixels where it is background, `FrontTile` where it is dirt, the art where it is rock. This is the only fallback for dropped terrain edits (plan 4 ruling); the test is `rebuild_area_matches_material`.
  - `pixels() -> &[u8]`.
  - OLX's rules exactly as the note describes, including the order: the edit is applied after the sim changed the material, so the rim tests read the new material.
- Consumes: Task 1's `ColorEdit` source data (theme art from `rl-content`).

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `carve_copies_back_and_paints_rim` (a known 5×5 case);
  - `dirt_uses_front_tile_colours`;
  - `green_dirt_uses_olx_shades`;
  - `dirty_rects_cover_every_change`;
  - `color_map_agrees_with_material_after_a_match` (Review Focus 2): run a 3000-tick smoke match from `rl-content` (skip without content), apply every `TerrainEdit` and stain, then check every pixel's class against the material.

  Run `cargo nextest run -p rl-terrain -p rl-content`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Golden.** Add a CPU golden: after the `dig` oracle scenario, the colour image equals a committed PNG (`fixtures/render/dig-color.png`). A small helper in `rl-content` or `rl-scenario` produces it, and `insta`-style blessing goes through `cargo xtask bless-render --reason`.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-terrain: colour map follows terrain edits`).

### Task 3: `rl-scene` (camera, interpolation, `DrawList`)

**Files:**
- Create: `crates/rl-scene/` (L5) with `camera.rs`, `snapshot.rs`, `worm.rs`, `projectile.rs`, `draw_list.rs`, `lib.rs`, `CLAUDE.md`

**Interfaces:**
- Produces:
  - `Snapshot::capture(&World) -> Snapshot`: positions, angles, frames, serials and alive flags. Small, and reused without allocation.
  - `Camera`:
    - OLX hard lock on the followed worm, clamped to the map (presentation §1.2–1.3);
    - shake: random ±amount world px for 0.2 s after the last shake, clamped inward, using its own RNG (§1.4);
    - stays on a dead worm and snaps on respawn (§1.5);
    - viewport size from the field of view (E4).
  - `build(world: &World, prev: &Snapshot, alpha: f32, cam: &Camera, assets: &SceneAssets, out: &mut DrawList)`:
    - `DrawList` holds layers in OLX order (§11.2): the terrain view rect, entities (from `rl-fx`, appended by the caller), projectiles, bonuses, worms (rope, crosshair, skin frame, muzzle, health bar), then lines (laser sight, beams).
    - Sprites are `Sprite { atlas_id, frame_rect, pos, flip, tint }` at world px. Lines are `Line { a, b, colour }`.
  - The sprite math is exact OLX:
    - the skin frame from the aim angle, facing and walk frame (§3.1), including the facing-left mirror one column off;
    - projectile rotation, angle images and animation frames (§5), using `rot_bumped`;
    - bonus flashing (§6).
  - Interpolation:
    - linear between snapshot and current for worms and projectiles;
    - never across a serial change or a respawn (Review Focus 1);
    - E2's off switch.
- Consumes: Task 1.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `camera_locks_clamps_and_shakes_like_olx`;
  - `skin_frame_matches_olx_table` (angles −90..60 × facing × walk frame vs hand-computed OLX frames);
  - `projectile_frames_match_olx`;
  - `interpolation_never_blends_reused_slots_or_respawns`;
  - `draw_list_order_matches_olx_layers`;
  - scene snapshot tests: the `DrawList` for frame 120 of `rope`, `fire-minigun` and a weapon scenario per mod, serialised compactly and committed with `insta`.

  Run `cargo nextest run -p rl-scene`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-scene: camera, interpolation and draw lists`).

### Task 4: `rl-fx` (classic cosmetic entities)

**Files:**
- Create: `crates/rl-fx/` (L5)

**Interfaces:**
- Produces:
  - `Fx::new(seed)`.
  - `Fx::on_events(&TickEvents)`: spawns entities from `SpawnCosmetic`, `Explosion`, `Kill`, `WeaponFired` (the muzzle flash, not for beams) and `Spawned`.
  - `Fx::step_tick(substeps: u8, color: &mut ColorMap, material: &Terrain)`: advances each entity per sub-step with OLX's physics (presentation §7: per-type gravity, damping, lifetime, frame counts, map collisions and stains).
  - `Fx::draw(&mut DrawList)`.
  - Its own RNG (E8), and at most 1024 entities (E1).
- Consumes: Tasks 1–3.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `blood_falls_and_stains_like_olx`;
  - `explosion_frames_and_invisible_start`;
  - `gibs_use_the_victims_colour`;
  - `entity_cap_drops_new_ones`;
  - `fx_never_touches_sim_randomness` (the sim checksum stream is identical with and without `Fx` attached).

  Run `cargo nextest run -p rl-fx`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-fx: classic cosmetic entities`).

### Task 5: `rl-render` (wgpu)

**Files:**
- Create: `crates/rl-render/` (L5). `#![forbid(unsafe_code)]` unless wgpu requires otherwise; justify any exception.

**Interfaces:**
- Produces:
  - `Renderer::new(device, queue, surface_format, settings: RenderSettings)`.
  - `upload_terrain(&ColorMap)` (full) and `update_terrain(&mut ColorMap)` (dirty rectangles only).
  - `upload_atlas(&SceneAssets)`: skins colorised at load, projectile images, core sprites; one atlas, at art scale.
  - `render(&DrawList, target: &wgpu::TextureView, size)`:
    - a world pass at art scale into an offscreen texture with a 1-px margin;
    - instanced sprites and lines (E3);
    - an upscale pass to native size with the selected filter.
  - `Offscreen::new()` (no surface) and `render_to_png(...)` for goldens and `rl-cli render`.
  - Missing atlas entries draw a magenta square.
- Consumes: Tasks 2–4.

**Steps:**
- [ ] **Step 1: RED.** In `xtask full`, skipped without an adapter:
  - `offscreen_renders_dig_frame_like_golden` (perceptual diff under threshold against `fixtures/render/*.png`);
  - `dirty_rect_upload_equals_full_upload`;
  - `upscale_filters_are_selectable`.

  In `check`, CPU only: atlas packing and vertex building tests.

  Then run `cargo nextest run -p rl-render`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: `rl-cli render`.** Add `rl-cli render <scenario> --frame N --out f.png [--scale S]`, which replays the oracle scenario with `rl-fx` attached and renders offscreen. Add goldens for one scenario on each of the 4 levels (walk, rope, shoot), and add `cargo xtask full` (runs `check` plus the GPU and golden tests).
- [ ] **Step 4: Commit.** Commit (`rl-render: wgpu renderer, offscreen goldens, rl-cli render`).

### Task 6: `rl-audio` and `rl-input`

**Files:**
- Create: `crates/rl-audio/` (L5) and `crates/rl-input/` (L5)

**Interfaces:**
- Produces, in `rl-audio`:
  - `Audio::new() -> Audio` (silent fallback, E12).
  - `load(&LoadedMod, &CorePack)`, decoding OGG through kira.
  - `play_events(&TickEvents, listener: Vec2)`:
    - positional gain 0.7 with inverse-distance falloff (reference 50, rolloff 2, listener 20 px out at the view centre: presentation §8);
    - THROW and PICKUP globally at gain 1.0;
    - no firing sound for beams;
    - voice limits per sound and globally.
  - Pure parts (gain and pan maths, the voice-limit policy) are unit-tested without a device.
- Produces, in `rl-input`:
  - `Bindings` (E5 defaults, serde for M7's options).
  - `InputState::on_window_event(&WindowEvent)` (winit physical keys) and `on_gamepad(&gilrs::Event)`.
  - `InputState::latch_tick() -> Keys`: presses and releases between ticks are latched, so a tap is never lost (spec §8.5).
  - Browser-reserved keys are avoided in defaults.
- Consumes: Task 1.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `positional_gain_matches_olx_openal_defaults`;
  - `voice_limits`;
  - `silent_when_no_device`;
  - `tap_between_ticks_is_latched`;
  - `default_bindings_match_olx`;
  - `gamepad_maps_to_keys`.

  Then run `cargo nextest run -p rl-audio -p rl-input`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-audio, rl-input: positional sound and latched input`).

### Task 7: `rl-game` sandbox

**Files:**
- Create: `crates/rl-game/` (L6, binary `rl-game`)

**Interfaces:**
- Produces:
  - `rl-game [--level <id>] [--mod <id>] [--skin <id>] [--targets N] [--seed S] [--content <dir>] [--fullscreen] [--no-interp] [--filter nearest|sharp]`.
  - **Window and backend.** A winit window with the WSL backend defaults (Global Constraints), `Outdated` and resize handling, and mailbox present when available (FIFO otherwise).
  - **Loop.** A 100 Hz fixed-tick accumulator:
    - each tick: `latch_tick` → `world.tick` → `fx.on_events` → `audio.play_events` → `color.apply` per `TerrainEdit` → `renderer.update_terrain`;
    - each frame: `scene.build` with alpha → `fx.draw` → `renderer.render`;
    - catch-up is capped (no spiral of death).
  - **Overlay.** An egui overlay with F2 debug (FPS, tick ms, entities, projectiles) and names and damage popups (E10).
  - **Sandbox (E11).** One human worm with the mod's first five weapons (or a random set, as the sim's loadout allows), N idle targets, F5 restart, Esc quit.
  - `surface_survives_resize_and_outdated`: a test of the surface state machine with a fake surface.
- Consumes: everything above.

**Steps:**
- [ ] **Step 1: RED.** Write `surface_survives_resize_and_outdated`, `loop_never_runs_more_than_n_catchup_ticks` and `sandbox_setup_spawns_targets` (pure parts). Run `cargo nextest run -p rl-game`: FAIL.
- [ ] **Step 2: GREEN.** Implement.
- [ ] **Step 3: Manual check.** Run `rl-game` on each level with each mod: walk, rope, shoot, die, respawn; check that sound plays when the plugin is installed. Record what you checked, the FPS seen and screenshots (`rl-cli render` equivalents) in the report.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-game: sandbox on every level and mod`).

### Task 8: Acceptance, soak, docs

**Files:**
- Modify: `xtask` (`full` tier contents), `docs/porting/STATUS.md` (presentation rows, E-decisions), the crate `CLAUDE.md`s, `README.md` (how to run the sandbox, WSL notes, audio packages)

**Steps:**
- [ ] **Step 1: Soak.** `soak_sandbox_10_minutes_headless` (`xtask full`): 60 000 ticks with scripted chaos keys, `Fx` and an offscreen render every 10th tick. Entity count, projectile pool, audio voices (fake backend) and GPU buffers stay bounded, and nothing panics.
- [ ] **Step 2: Gates.** `cargo xtask check` (≈ 60 s) and `cargo xtask full` both pass.
- [ ] **Step 3: Docs.** Update STATUS and the README, then commit (`Presentation: sandbox playable on all levels`).
