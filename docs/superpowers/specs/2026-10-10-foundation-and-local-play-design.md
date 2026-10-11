# rust-liero: Foundation and Local Play Design

- **Date:** 2026-10-10
- **Status:** Draft, awaiting review
- **Scope:** Sub-projects 1 (Foundation) and 2 (Local play). Sub-projects 3 (Modernization) and 4 (Networking) are out of scope here and get their own brainstorms.

## 1. Purpose

rust-liero is a Rust reimplementation of [OpenLieroX](https://github.com/openlierox/openlierox) (OLX), modernized for current hardware.
It is also the author's first game in Rust.
The working name is `rust-liero`, and crates use the prefix `rl-`.
The final name must not contain "LieroX" or "Liero Xtreme", because the original LX license forbids it (see §11).

### 1.1 What the user asked for

- Old OLX levels, mods and skins must be **playable**, for nostalgia: they are years of community work.
  File-format compatibility is **not** required: content is converted into new native formats.
- Physics, movement and shooting must **feel identical** to OLX.
  Internal representations may change.
- Network compatibility with existing OLX servers and clients is **not** required.
- Modernize where it is feasible and performant, especially for large, modern displays.
  Graphical effects may be added later.
- The project is built **agentic-first**: small, unit-tested modules, rapid iteration, little context per task.
- No split-screen. The first milestone is local play against bots.
- Gusanos (the second engine bundled in OLX) is **dropped entirely**: no Gusanos engine, mods or levels.
- The network model gets its own brainstorm later.
  It must **not depend on a central master server**: OLX's community died when its master server went down.
- Platforms: desktop first, developed on a Linux build (WSL2).
  The architecture is web-ready from day one.
- Content in the OpenLieroX GitHub repository is treated as acceptable to use and bundle in this non-commercial, open-source project.
- Runtime performance has priority.
  A large amount of setup code is acceptable, because agents write it.

### 1.2 Initial content scope

| Kind | Item | OLX source (pinned commit `d1ad953f4f20610f2648cce26c9739e7c088409d`) |
|---|---|---|
| Level | Liero Factory | `share/gamedir/levels/Liero Factory.lxl` (1200×480) |
| Level | Castle Strike | `share/gamedir/levels/CastleStrike.lxl` (1350×550) |
| Level | Fossil Facility | `share/gamedir/levels/FossilFacility.lxl` (640×480) |
| Level | Dirt | `share/gamedir/levels/Dirt Level.lxl` (504×350) |
| Mod | Liero 1.0 | `share/gamedir/Classic/` (internal name "Liero v1.0", 38 weapons, LGS v7) |
| Mod | Modern Warfare 1.0 | `share/gamedir/MW 1.0/` (internal name "Modern Warfare1.0", 50 weapons, LGS v7) |
| Skins | All 409 | `share/gamedir/skins/*.png` (2.4 MB, one format; `default.png` is the default) |
| Theme | Dirt | `share/gamedir/data/themes/Dirt/` (hole stamps, used for carving) |
| Game mode | Deathmatch | — |

The importers are generic, so adding more OLX content later is a one-line change to the manifest (§9.3).

### 1.3 Success criteria for this spec

1. A local Deathmatch game against bots is playable on Linux on any of the 4 levels, with either mod (milestone M7).
2. Worm movement, ninja rope and every weapon of both mods match OLX **bit-exactly** in recorded oracle scenarios (§6).
   A documented float tolerance is used only where bit-exactness is proven impossible.
3. The fast test tier (`cargo xtask check`) runs in under about 60 s and covers unit, corpus, oracle and short bot-match tests.
4. The simulation crates build for `wasm32-unknown-unknown` in CI.

### 1.4 Non-goals for this spec

- Networking. Only constraints are recorded (§12.2).
- Game modes other than Deathmatch. The interface supports them (§5.8).
- Gusanos content, original Liero `.lev` levels, Teeworlds `.map` levels, and LX text-source mods.
- Enhanced visuals beyond the hooks in §8 (GPU particles, lighting, wider field of view). These belong to sub-project 3.
- Split-screen, a campaign mode (hooks only, §12.1), a web front end, Android.

## 2. Background: what OLX is

The analysis is summarized here so that implementers do not need to re-derive it.
Source paths are relative to the OLX repository root.

- **About 155k lines of C++ under `src/`.**
  The classic simulation (LX56 physics, rope, projectiles, actions, carving) is roughly 10–15k lines.
  The rest is not ported:
  - the 640×480 "DeprecatedGUI" menus, 23k lines;
  - networking with version-compatibility branches, about 30k lines;
  - Gusanos, 34k lines;
  - platform glue.
- **Timing.** The outer loop is a fixed 100 Hz (`src/game/Game.h:105`, `Game.cpp:917-957`).
  LX56 worm and projectile physics run at **84 Hz** (`include/PhysicsLX56.h:16-21`).
  The client has its own time accumulator (`src/client/CClient_Game.cpp:60-222`), and **each projectile has its own** accumulator (`src/common/PhysicsLX56_Projectiles.cpp:1376-1410`).
- **Numbers and randomness.** All state is `float`.
  There are two sources of randomness:
  - a 256-entry `RandomNumbers[]` table, indexed per shot (`include/RandomNumberList.h`);
  - libc `rand()` (`src/common/MathLib.cpp:58-76`).
- **Worm physics** is in `src/common/PhysicsLX56.cpp:272-469`.
  Collision is in `moveAndCheckWormCollision` (`:88-269`): recursive step halving, an x −3..+3 / y +5..−4 scan, and deliberate LX56 quirks.
- **Ninja rope** is in `PhysicsLX56.cpp:505-610` and `src/client/CNinjaRope.cpp`.
  It applies a constant pull force (not a spring) beyond the rest length.
- **Projectiles** are in `PhysicsLX56_Projectiles.cpp`. Notable details:
  - per-pixel terrain collision;
  - worm hits swept every 2 px;
  - the "wallshooting" grace window of roughly 11 ms (an LX56 bug that OLX keeps);
  - 27 action types (`include/ProjAction.h:140-175`);
  - Timer, Hit and PlayerHit events.
- **Terrain** is an 8-bit material map at 1× scale, plus front and back color images at 2× scale (`src/common/CMap.cpp:460-466`).
  `CarveHole` (`CMap.cpp:1007-1106`) uses the theme's hole stamps.
- **Mods** are `script.lgs` binaries, version 7 (LX 0.56) or 8 (OLX), read by `CGameScript::Load` (`src/common/CGameScript.cpp:582-828`).
  In v7 the projectiles are inline and recursive.
- **Levels** are `.lxl` files: a zlib-compressed block with back RGB, front RGB and per-pixel flags, plus optional chunks (`src/common/MapLoader_LieroX.cpp`).
- **Skins** are 672×36 sheets: the image on top and a colorization mask below (`src/client/CGameSkin.cpp:720-790`).
- **Test hooks** in `src/common/Command.cpp:2262-2330`: `simSetSeed`, `simStep`, `setWormInput` (move, shoot, jump and carve only) and `dumpGameState`.
  They are used by `tests/headless/test_physics.py`.
- **Weaknesses we fix by design:**
  - the simulation is tangled with rendering, sound and network (global singletons, `cClient->getGameLobby()` read about 57 times in the physics code);
  - rendering is done in software at 640×480;
  - networking is client-authoritative;
  - there are no replays.

## 3. Key decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | Rust; the simulation is a pure CPU crate with no engine dependency | Deterministic and testable; it is shared by the game, the CLI, bots and a future server |
| D2 | wgpu + winit + egui; kira for audio, gilrs for gamepads | Highest performance ceiling and full rendering control; web-ready through standard tooling; egui scales to any DPI |
| D3 | Physics/material grid fixed at 1× OLX world pixels | A higher grid would change the feel (collision code is pixel-granular) and break bit-exact verification |
| D4 | Art resolution is per content (2× for imports, up to 8× native); rendering at native resolution | Large-screen sharpness without changing the feel |
| D5 | Default field of view = OLX's (240 visible world pixels in height); a wider view is a match setting | Field of view affects gameplay on small maps |
| D6 | Native, text-first formats (RON plus PNG/OGG); OLX content is imported | Diffable, readable by agents, hot-reloadable, extensible |
| D7 | Gusanos dropped | 15–20k lines for 4 mods; the user decided against it |
| D8 | Desktop first, web-ready (`no_std` simulation, abstract content I/O, WASM build in CI) | Web constraints are cheap to adopt early and expensive to retrofit |
| D9 | License GPL-3.0-or-later | Compatible with OLX's LGPL-2+ and zlib code and with Apache/MIT dependencies; ported code is a derivative work |
| D10 | Behavior is verified against a pinned OLX build (the oracle) | Turns "feels identical" into an automated, objective test |
| D11 | No ECS framework; plain structs plus index handles | The simulation's data is a few flat pools; this avoids borrow-checker graphs and framework churn |

## 4. Workspace architecture

The project is a single Cargo workspace (Rust 2024 edition) with layered crates.
A crate may depend on crates in lower layers.
It may depend on a crate in its own layer only when the dependency table below lists it (for example `rl-terrain` → `rl-data`, `rl-modes` → `rl-sim`, `rl-fx` → `rl-render`).
Dependencies must not form cycles.
`cargo xtask check` enforces this with a layering check that compares `cargo metadata` against an allow-list in `xtask/layers.ron`.

```
L6 apps      rl-game (desktop binary) · rl-cli (headless runner) · xtask (dev workflows)
L5 present   rl-scene · rl-render · rl-fx · rl-audio · rl-input · rl-ui
L4 content   rl-content
L3 sim       rl-sim · rl-modes · rl-bots · rl-replay        ← #![no_std] + alloc
L2 formats   rl-pack · rl-import-lgs · rl-import-lxl · rl-import-gfx
L1 model     rl-data · rl-terrain · rl-vfs (ContentSource trait + in-memory backend)
L0 core      rl-math
test         rl-oracle (trace format + comparison); tools/olx-oracle/ (Docker + OLX patch, not a crate)
```

| Crate | Responsibility | Key dependencies |
|---|---|---|
| `rl-math` | `Vec2` (f32), angles, trig/sqrt through the `libm` crate, OLX's `RandomNumbers[]` table, a glibc-`rand()`-compatible generator | `libm` |
| `rl-data` | Content definitions: `ModDef`, `WeaponDef`, `ProjectileDef`, actions/events, `WormParams`, `RopeParams`, `GameSettings`, `LevelDef`, `SkinDef` (serde) | `serde` |
| `rl-terrain` | Material grid, stamp operations (carve/place dirt) on material **and** on color images, dirty-rectangle tracking, collision queries | `rl-math`, `rl-data` |
| `rl-vfs` | `ContentSource` trait (list/read), in-memory backend; feature-gated directory and zip backends | `zip` (feature) |
| `rl-pack` | Read, write and validate native packs (§9.1), with line/column errors and format-version migration | `rl-data`, `rl-vfs`, `ron`, `png` |
| `rl-import-lgs` | `script.lgs` v7/v8 → `ModDef`; detects repeated inline projectiles and shares them | `rl-data` |
| `rl-import-lxl` | `.lxl` → level pack (image type; pixmap type rasterized through its theme) | `rl-data`, `rl-terrain`, `flate2` |
| `rl-import-gfx` | OLX skins, bitmap fonts and themes → native packs | `rl-data`, `png` |
| `rl-sim` | `World`, worms, rope, projectiles, actions, bonuses, damage, tick timing, events, checksum, `GameMode` trait | `rl-math`, `rl-data`, `rl-terrain` |
| `rl-modes` | `GameMode` implementations; Deathmatch in this spec | `rl-sim` |
| `rl-bots` | Bot AI producing `PlayerInput` from a read-only `&World` | `rl-sim` |
| `rl-replay` | Replay format: pack ids and hashes, settings, seed, per-tick inputs | `rl-sim`, `serde` |
| `rl-content` | Resolves packs through the VFS, decodes images and sounds, builds ready-to-run `World` inputs and GPU-ready asset data | `rl-pack`, `rl-vfs`, `lewton`/`symphonia` |
| `rl-scene` | Pure CPU: `&World` plus interpolation → `DrawList`; camera logic | `rl-sim` |
| `rl-render` | wgpu: `DrawList` → pixels, on screen or offscreen | `wgpu` |
| `rl-fx` | Cosmetic particles from `SpawnCosmetic` events; classic CPU path, compute-shader hook | `rl-render` |
| `rl-audio` | `Sound` events → kira, positional, with voice limits | `kira` |
| `rl-input` | winit/gilrs → action bindings → `PlayerInput`, latched per tick | `winit`, `gilrs` |
| `rl-ui` | egui menus and HUD | `egui` |
| `rl-game` | Window, frame loop, wiring | everything in L5 |
| `rl-cli` | Headless commands (§7.4) | L3–L5 (`rl-render` offscreen) |
| `rl-oracle` | Oracle trace format, comparison, first-divergence reporting | `rl-sim` |
| `xtask` | `check`, `full`, `api`, `fetch-olx-content`, `oracle-record`, `bless-regression` | — |

### 4.1 Conventions for agentic development

1. **Each crate ships `CLAUDE.md`** (and `AGENTS.md` pointing to it).
   It states the crate's purpose, invariants, how to test it, and the OLX source it ports.
   Agents read that file plus the public APIs of dependencies, not the whole repository.
2. **Small files.** The target is under 500 lines per file, with one concept per module.
   `pub(crate)` is the default visibility.
3. **Compiler-enforced boundaries.** L0–L3 crates are `#![no_std]` + `alloc`, so file I/O, threads and wall-clock time cannot compile in the simulation.
   Heavy dependencies (wgpu, egui, kira, winit) live only in L5/L6.
4. **Porting references.** Each ported item carries a doc comment pointing at the OLX code it mirrors, such as `/// OLX: src/common/PhysicsLX56.cpp:272-469`.
   `docs/porting/STATUS.md` tracks each subsystem: ported / oracle-verified / deliberate deviations and kept quirks.
5. **API summaries.** `cargo xtask api` writes a compact list of each crate's public items to `target/api/<crate>.txt`.
6. **Low-noise feedback.** Every xtask command prints only failures, plus paths to artifacts (PNG diffs, replays, first-divergence reports).
   Test runs use `cargo nextest`.
7. **Fast builds.**
   - The `mold` linker.
   - `[profile.dev] opt-level = 1`, and `opt-level = 3` for dependencies.
   - Simulation crates have only small dependencies.
   - The goal for a crate-level edit → test loop is under about 10 s.
8. **Lints.** CI runs `rustfmt` and `clippy -D warnings`.
   `#![forbid(unsafe_code)]` applies everywhere except `rl-render` where needed, with a justification.

## 5. Simulation design (`rl-sim`)

### 5.1 Interface

```rust
impl World {
    pub fn new(setup: MatchSetup) -> World;            // mod, level terrain, settings, seed, players
    pub fn tick(&mut self, inputs: &[PlayerInput]) -> &TickEvents;   // fixed 100 Hz
    pub fn checksum(&self) -> u64;                      // hash of full state
    // read-only accessors: worms(), projectiles(), bonuses(), terrain(), time(), mode_state() ...
}
```

`tick` is the only way game state changes.
`World: Clone` holds: one clone is a full snapshot, for tests and future rollback.

### 5.2 Timing

- `tick` advances 10 ms of simulated time (100 Hz).
- Inside each tick, LX56 worm physics runs at 84 Hz from a world-level accumulator.
  Each projectile has its own accumulator and catches up on its own schedule, exactly as in OLX.
- Time uses the **same numeric representation and rounding as OLX**.
  M3 begins by tracing OLX's time types (`TimeDiff`, `AbsTime`, `fLastSimulationTime`) and records the findings in `docs/porting/timing.md` before any physics code is written.
- `GameSpeed` scales the frame delta as in OLX (clamped to 0.05–10).
- The game loop interpolates rendering between the last two ticks.
  Interpolation never feeds back into the simulation.

### 5.3 State layout

- **Worms:** up to 32 entries; `WormId(u8)` is an index. Each worm holds its own `NinjaRope`.
- **Projectiles:** a pre-allocated pool, with capacity defaulting to OLX's 3000 and a `GameSettings` knob to raise it.
  Slot allocation, reuse and iteration order **mirror OLX's `FastVector`**, because order determines random-number use and event order.
  Hot fields used in every physics step are stored together.
- **Bonuses:** at most 32, as in OLX.
- **No heap allocation inside `tick`.**
  The event buffer is reused between ticks.
  A test with a counting global allocator enforces this.

### 5.4 Terrain

- The simulation owns only the **material grid**: 1 byte per world pixel (rock, dirt, background, and so on).
- Carving or placing dirt updates the grid and emits `TerrainEdit { op, stamp, pos }`.
- `rl-terrain` applies the same edit to the color images, either in the renderer or in headless golden tests, using the same function.
- The terrain type records its scale (cells per world unit); this spec fixes it at 1.
  The field exists so that a later "HD physics" experiment can be added without changing the interface.

### 5.5 Randomness

The simulation has two sources of randomness, matching OLX:

1. **The shared table.** OLX's 256-entry `RandomNumbers[]` table, indexed by a per-shot seed byte, exactly as `Proj_SpawnInfo::apply` does.
2. **A glibc-compatible `rand()`.** The generator lives in `World` and is seeded from `MatchSetup`, so oracle runs seeded with `simSetSeed` match.

Cosmetic randomness lives in `rl-fx` with its own generator and never touches either source.
Any OLX `rand()` call that belongs to cosmetics (blood, gfx entities) is identified during porting.
Oracle runs use OLX's dedicated mode, which does not spawn those entities, so the comparison is not disturbed.

### 5.6 Input

`PlayerInput` contains:

- buttons: left, right, up, down, jump, fire, rope, change-weapon;
- an optional mouse-aim target.

Aim acceleration, angle limits and weapon selection are computed **inside** the simulation, as OLX does (`src/common/CWormHuman.cpp:223-265`).
Bots produce the same type.

### 5.7 Output events

`TickEvents` carries:

- `Sound`
- `TerrainEdit`
- `Explosion`
- `SpawnCosmetic` (blood, smoke, trails, gibs)
- `WeaponFired`
- `Damage`
- `Kill`
- `BonusPickup`
- `Message`
- `GameOver`

The renderer, audio, HUD and tests consume these events, together with read-only accessors.
The simulation never calls into them.

### 5.8 Settings and game modes

- `GameSettings` is a typed struct that defaults to OLX's "LieroX 0.56" preset.
  It is immutable during a match.
  It covers the gameplay features from `src/common/FeatureList.cpp` that are relevant to the classic engine.
- The `GameMode` trait is defined in `rl-sim` and implemented in `rl-modes`.
  - **Callbacks:** spawn, kill, damage, pickup, per-tick, is-over, scoring.
  - **Command interface** (hook H1, §12.1): spawn a worm or bot with a loadout, give a weapon, apply a terrain stamp, emit a message, end the match with a result.
  - Deathmatch is the only mode implemented in this spec.

### 5.9 Bots (`rl-bots`)

- **Approach:** port OLX's approach from `src/common/CWormBot.cpp`: area-based pathfinding over maximal free rectangles, a ballistic aim solver, heuristic weapon choice, rope use, and difficulty levels.
  Exact parity with OLX bots is **not** required.
- **Determinism:**
  - bots use their own seeded generator;
  - pathfinding is spread across ticks using a **work-unit** budget, never a wall-clock budget, so bot matches are reproducible.
- **Configuration:** bots take a behavior configuration (hook H3, §12.1). The default is the Deathmatch brain.
- **Testing personality:** a "chaos" personality cycles through every weapon, for test coverage.

## 6. Fidelity oracle

The oracle is a pinned build of OLX that records reference behavior for the Rust simulation to match.

- **`tools/olx-oracle/`** contains:
  - a Dockerfile that builds OLX at commit `d1ad953f` (Ubuntu 22.04, as OLX's own Docker image);
  - a patch file that adds test hooks.
- **The hooks patch adds two things:**
  - Full input injection per worm: all buttons, aim, rope and weapon selection.
    The existing `setWormInput` covers only move, shoot, jump and carve.
  - A recording mode that writes one JSON line per frame to a file:
    - frame number;
    - per worm: position, velocity, angle, health, weapon slots with charge and reload, and rope state;
    - per projectile: slot, type, position and velocity;
    - bonuses;
    - a hash of the material map.
- **Scenarios** are RON files: level, mod, settings, seed, worm spawns and per-frame input scripts.
  `cargo xtask oracle-record <scenario>` runs OLX in dedicated mode and stores the trace in `fixtures/oracle/<scenario>.jsonl.zst`, which is committed.
  Agents rebuild OLX only to record new scenarios.
- **Comparison.** `rl-oracle` replays the scenario in `rl-sim` and compares frame by frame.
  - On failure it reports **only the first diverging frame and field**: for example, `frame 412: worm[0].vel.y expected 31.250000 got 31.249998`.
  - Bit-exact comparison is the default.
  - A per-field tolerance is allowed only with a written justification in the scenario file.
- **Required scenarios for this spec:**
  - movement: walk, jump, fall, slope climbing, collision against rock and dirt, digging;
  - ninja rope: shoot, attach, swing, release, attach to a worm;
  - **every weapon of both mods**: fired in open air, into terrain, and at a dummy worm;
  - bonuses: spawn, fall, pickup;
  - damage and death.

## 7. Automated testing

There are two tiers:

- **`cargo xtask check`:** the fast tier, target under about 60 s, run on every agent iteration.
- **`cargo xtask full`:** nightly and on demand.

| Layer | Content | Tier |
|---|---|---|
| Unit | Per-crate tests: parsers, physics functions, stamps, colorization, input latching | check |
| Corpus | Every item in the content manifest imports, validates and round-trips (native write → read → equal); `insta` snapshot summaries. `xtask check` runs `fetch-olx-content` automatically on first use, and the result is cached in `target/content/` | check |
| Oracle | All recorded scenarios (§6) | check |
| Property and fuzz | `proptest`: simulation invariants under random inputs (no worm inside rock, health within bounds, pools within capacity, no panics). `cargo-fuzz` targets for every importer and the pack reader | properties in check, fuzzing in full |
| Bot matches | §7.1 | short matches in check, full grid in full |
| Render goldens | Offscreen wgpu rendering with a software adapter (lavapipe/llvmpipe) of fixed scenario states: each level, skin colorization, HUD states, classic effects. Perceptual diff against committed PNGs; diff images written on failure | full |
| Scene snapshots | `rl-scene` `DrawList` snapshots, which need no GPU | check |
| GUI flows | `egui_kittest` drives menus by label: Local Play → mod → level → bots → start; then the results screen. Asserts app state and takes snapshot images | full |
| End to end | `rl-game --autoplay <script.ron> --fake-clock` on a virtual display (Xvfb): menu → bot match → results, with screenshots at checkpoints. Asserts a clean structured log | full |
| Benchmarks | `divan`: tick time with 3000 projectiles, `DrawList` build, render frame time. CI fails on a regression over 10% against the stored baseline | full |

### 7.1 Bot matches

`rl-cli match --mod <id> --level <id> --mode deathmatch --bots 8 --seed <n> --ticks <n>` runs headless at maximum speed and verifies the following:

- No panic, and invariants hold every tick.
- The match ends through the mode's win condition, or the tick limit is reached as expected.
- **Determinism:** two runs produce identical checksum streams.
- **Replay round-trip:** record, then replay, gives identical checksums.
- **Performance:** 99th-percentile tick time within budget.
- **Coverage report:** weapons fired, projectile actions and events triggered, bonuses collected, rope attachments, kills by weapon.
  The nightly grid runs both mods × the 4 levels × several seeds with chaos and normal bots.
  It fails if coverage drops below the required list: every weapon of both mods fired, and every action type used by the mods triggered.

### 7.2 Golden match checksums

Checksum streams for fixed-seed matches are committed.
Any change in behavior shows up as a diff.
Accepting the new behavior requires `cargo xtask bless-regression --reason "<text>"`, which records the reason in `fixtures/blessings.log`.

### 7.3 Failure capture

A failing match automatically writes its replay and seed to `target/failures/`.
`rl-cli replay <file> --until <tick> --dump` reproduces it.
`cargo xtask bless-regression --from <failure>` turns it into a permanent regression test once it is fixed.

### 7.4 `rl-cli` commands

| Command | Purpose |
|---|---|
| `import <olx-gamedir> --out <dir>` | Import OLX content into native packs |
| `validate <pack>` | Check a pack and report errors |
| `match ...` | Run a headless bot match (§7.1) |
| `replay <file> [--until N] [--dump]` | Replay a recorded match |
| `scenario <file>` | Run an oracle scenario |
| `dump-state` | Write the game state as JSON |
| `render <scenario|replay> --tick N --out frame.png` | Render a frame offscreen, so agents can inspect it |

## 8. Presentation

### 8.1 Field of view and resolution

- The default field of view matches OLX: 240 world pixels of visible height, with width set by the aspect ratio.
  Aspect ratio is clamped by the `max_aspect` setting (default 16:9).
- Field of view is a `GameSettings` value, so it is fixed per match.
- Rendering is at the display's native resolution.

### 8.2 Render pipeline

1. **`rl-scene` (CPU, pure).** Builds a `DrawList` from `&World`, the previous tick's state and an interpolation factor.
   It includes camera logic: follow the worm, clamp to map edges, as OLX does.
2. **World pass** into an offscreen target at art resolution (2× for imported content):
   - **Terrain:** back and front RGBA textures plus an R8 material texture.
     `TerrainEdit` is applied to CPU copies by `rl-terrain`, and only the dirty rectangles are uploaded with `queue.write_texture`.
     The largest initial level, Castle Strike, is 2700×1100 at 2×, well within GPU limits.
   - **Sprites:** one instanced pipeline over a texture atlas: worms (skins colorized at load), projectiles (pixel, rect, circle, polygon, image), bonuses, rope, laser sight, beams.
3. **Upscale pass** to native resolution.
   The filter is selectable: integer-nearest, sharp-bilinear (default) or a pixel-art upscaler.
   Rendering includes a one-pixel margin, so the camera can move by sub-pixel amounts without shimmering.
4. **`rl-fx`.**
   - The classic CPU particle set recreates OLX's blood, smoke, gibs and trails, driven by `SpawnCosmetic`.
   - The compute-shader particle path and post effects (glow, dynamic light) are interface hooks only in this spec.
5. **HUD and menus** in egui, drawn at native resolution, scaled by DPI and a UI-scale setting.

### 8.3 HUD and menus

- **HUD:** health, weapon slots with reload and charge, kill feed, scoreboard, minimap.
  A "classic" style uses the imported OLX bitmap fonts.
- **Menus:**
  - main menu;
  - local play: mod, level with thumbnails, Deathmatch settings, bot count and difficulty;
  - player profiles: name, skin and color, with a live colorized preview;
  - options: video, audio, key bindings, gameplay;
  - content import;
  - match results.
- Menu logic is separated from widgets, so `egui_kittest` can test it.

### 8.4 Audio

- `Sound` events are played through kira, panned and attenuated relative to the camera.
- Voice limits apply per sound and globally.
- OGG files are decoded when content loads.

### 8.5 Input

- **Keyboard:** winit physical key codes, so bindings work on any layout.
- **Other devices:** mouse aiming, and gamepads through gilrs.
- **Sampling:** the current state is sampled at each 100 Hz tick boundary.
  Presses and releases that happen between ticks are latched, so short taps are never lost.
- **Defaults** avoid browser-reserved keys (for example Ctrl+W).

### 8.6 Frame loop

- The game is single-threaded on every platform.
- winit's event loop drives a tick accumulator, then interpolated rendering.
- Present mode: mailbox when available, otherwise FIFO.
  Options are uncapped frame rate or VSync, plus borderless or exclusive fullscreen.

## 9. Content

### 9.1 Native pack format

A pack is a directory, or a `.zip` file with the same layout.
Every pack has a `pack.ron` with these fields:

- `id`, `name`, `version`, `kind` (mod / level / skin / theme)
- `authors`, `origin`, `license`
- `format_version`
- `content_hash`, computed over the normalized contents

| Kind | Contents |
|---|---|
| Level | `level.ron` (name, size in world units, `art_scale`, theme, spawns, optional sidecar metadata for hook H2), `material.png` (8-bit), `front.png` and `back.png` at `art_scale`, `thumb.png` |
| Mod | `mod.ron` (worm and rope physics, weapon list), `weapons/*.ron`, `projectiles/*.ron` (named and shared), `gfx/`, `sfx/*.ogg` |
| Skin | `skin.png` (any resolution) and `skin.ron` (frame layout, colorization mask) |
| Theme | Hole stamps, stones, tiles |

- **Naming:** all file names in native packs are canonical lowercase.
- **Hot reload:** text packs reload while the game runs; the desktop build watches pack folders.
- **Format changes:** older `format_version`s are migrated forward when loaded.
- **Gameplay expressiveness:** the native weapon and projectile model can express everything LGS v7 and v8 can.
  This is checked by oracle parity on imported mods.

### 9.2 Importers

- **Determinism:** the same input produces byte-identical output, so snapshot tests can cover the importers.
- **Report:** each import writes a report, `import-report.ron`, listing warnings such as malformed skin sizes, ignored chunks and unknown fields.
- **LGS details:**
  - the 2-byte padding in the header;
  - fixed-size strings are read up to the first NUL, since the bytes after it are garbage;
  - `ROF /= 1000` and `Recharge /= 10` are applied when reading;
  - `.wav` references are resolved to `.ogg`, as OLX does (`src/sound/Sounds.cpp:113`);
  - mod file lookups are case-insensitive.
- **LXL details:**
  - the image type: a zlib block holding back RGB, then front RGB, then PX flags;
  - an "OLX hi-res data" chunk replaces the 2× art if present;
  - the pixmap type is rasterized through its theme;
  - CTF-variant headers are accepted.
- **Skin details:** colorization follows `Colorize_Execute` (`src/client/CGameSkin.cpp:720-790`) and is stored as mask metadata.

### 9.3 Bundled OLX content

- `content/olx-manifest.ron` lists each item from §1.2 with its source path, the pinned commit and a SHA-256.
- `cargo xtask fetch-olx-content` downloads exactly those files from GitHub at the pinned commit, verifies them and imports them into `target/content/`.
  CI caches the result, and release packaging bundles it.
- Converted assets are **not** committed.
- Removing or adding an item is a one-line manifest change.
- Small **synthetic fixtures** are generated in-repo: a tiny level, and a minimal mod with one weapon per action type.
  They let unit tests run without the OLX download.

### 9.4 Virtual file system (`rl-vfs`)

- The `ContentSource` trait has backends for a directory, a zip file and in-memory content.
  A browser-storage backend comes later.
- Sources are layered: user packs override bundled packs.
- Case-insensitive lookup is used only by the importers.
- User data lives in the platform data directory, found with the `directories` crate:
  - `profiles.ron`, `settings.ron`;
  - `replays/`, `packs/`.

## 10. Milestones

| # | Milestone | Done when |
|---|---|---|
| M0 | Toolchain (rustup, mold), workspace skeleton, layering check, `xtask check`, CI (Linux, plus `wasm32-unknown-unknown` build of L0–L3), `CLAUDE.md` template per crate, license file | `cargo xtask check` passes on the skeleton in CI |
| M1 | `rl-data`, `rl-vfs`, `rl-pack`, `rl-import-lgs`, `rl-import-lxl`, `rl-import-gfx`, `xtask fetch-olx-content`, synthetic fixtures | All §1.2 content imports, validates and round-trips; importers have fuzz targets |
| M2 | `tools/olx-oracle` (Docker, hooks patch), scenario format, `xtask oracle-record`, `rl-oracle` comparison, first movement scenarios | Recorded movement traces committed; the comparison tool reports first divergence |
| M3 | `docs/porting/timing.md`, `rl-math`, `rl-terrain`, core `rl-sim` (worm movement and collision, rope, 100/84 Hz timing, checksum), `rl-cli scenario/dump-state` | Movement and rope scenarios match OLX bit-exactly |
| M4 | Projectiles, all actions and events used by both mods, explosions and carving, beams, jetpack, damage, bonuses, death and respawn | Every weapon of both mods matches OLX; short bot-free smoke matches pass invariants |
| M5 | `rl-scene`, `rl-render`, `rl-fx` (classic), `rl-audio`, `rl-input`, a sandbox mode in `rl-game` | Walk, rope and shoot on all 4 levels; render goldens and scene snapshots pass |
| M6 | Deathmatch in `rl-modes`, `rl-bots`, `rl-replay`, `rl-cli match/replay`, the nightly bot grid | The grid passes with full weapon coverage; determinism and replay round-trip hold |
| M7 | `rl-ui` (menus, HUD, profiles, results), `--autoplay`, GUI and end-to-end tests, benchmarks | **v0.1: playable local Deathmatch against bots**; `xtask full` passes |

- M1 and M2 are independent and can run in parallel.
- Within M4, action types and weapons can be split across agents.
- M5 can start once M3 has stable read-only accessors, in parallel with M4.

## 11. Licensing

- The project is licensed **GPL-3.0-or-later**.
  - Most of OLX's code is LGPL-2+ ("code under LGPL" headers, `COPYING.LIB`, `debian/copyright`), and LGPL-2.x permits relicensing under the GPL.
  - The original Liero Xtreme code is zlib-style (`doc/original_source_release/License.txt`).
- **Credit.** The original LX license asks that the name Liero Xtreme not be used for new releases, and asks for credit.
  The README and the about screen credit Jason Boettcher (Liero Xtreme), the OpenLieroX team, and Joosa Riekkinen (Liero).
- **Bundled OLX content.** Per the user's decision, OLX content is acceptable to bundle.
  The manifest keeps it removable item by item.
- **Dependencies** must be GPL-3-compatible (MIT, Apache-2.0, BSD, zlib, MPL-2.0).
  `cargo deny` enforces this.

## 12. Future extensions (designed for, not built)

### 12.1 Campaign and scenario mode

A future `rl-campaign` crate would add a `ScenarioMode` with data-driven triggers (condition → actions), and optionally rhai scripting restricted to the simulation's deterministic API.
It would ship in the same binary.
This spec includes three hooks for it:

- **H1:** `GameMode` has a command interface (§5.8).
- **H2:** levels support an optional sidecar metadata file (§9.1).
- **H3:** bots take a behavior configuration (§5.9).

### 12.2 Networking constraints for the later brainstorm

- **No central master server dependency.**
- **Browser clients must be supported.** They use WebRTC data channels or WebTransport, not raw UDP.
- **The simulation already provides:**
  - determinism and per-tick checksums;
  - `World: Clone` snapshots;
  - events separated from state;
  - inputs as the only means of change.

### 12.3 Modernization (sub-project 3)

- GPU compute particles that collide with the material texture.
- Glow and dynamic lighting.
- Native content at 4×–8× art resolution.
- A wider field of view as a match setting.
- An "HD physics" experiment, using the terrain scale field and compared against classic with bot-match statistics.

## 13. Risks

| Risk | Mitigation |
|---|---|
| Hidden LX56 quirks (two clocks, step halving, wallshooting, float-to-int conversions) break bit-exactness | The oracle reports the first divergence; `docs/porting/STATUS.md` records deliberate quirks |
| OLX's `rand()` use interleaves gameplay and cosmetic draws | Oracle runs use dedicated mode (no cosmetic entities); cosmetic draws are audited during porting |
| Float differences between C++ and Rust (contraction, x87) | OLX is built for x86-64 with SSE and `-std=c++0x`, so no FMA contraction is expected; tolerance only with justification |
| The OLX oracle build becomes hard to maintain | Pinned commit, Docker, a small patch; traces are committed, so rebuilds are rare |
| wgpu breaking releases | Versions pinned; upgrades are deliberate tasks |
| Scope creep | Each sub-project gets its own spec; this spec is limited to v0.1 |

## 14. Open decisions (not blocking this spec)

- **Final project name**, which renames the crate prefix `rl-` mechanically.
- **Default bot difficulty**, and whether difficulty presets copy OLX's (Easy to Xtreme).
  This will be decided in M6 from playtesting.

## 15. Amendments made while writing plan 1 (2026-10-10)

These came out of building and verifying plan 1's code against the real OLX
content. Where they conflict with earlier sections, they take precedence.

| # | Amends | Change | Reason |
|---|---|---|---|
| A1 | §4.1 (3) | `#![no_std]` applies to `rl-math`, `rl-data`, `rl-terrain` and the L3 crates. `rl-vfs` and the L2 format crates use `std`. | Formats run only at load time and need `ron`, `png` and `std::fs`. The simulation's purity is unaffected. |
| A2 | §9.1 | `content_hash` is computed from the pack files on demand (`rl_pack::content_hash`), not stored in `pack.ron`. | A stored hash goes stale as soon as someone hand-edits a pack. |
| A3 | §9.3 | Fetched OLX content is verified by the pinned git commit (`git` checks every object hash). The manifest lists paths, not per-file SHA-256 values. | Same integrity guarantee, without a hand-maintained hash list. |
| A4 | §8.2, §9.1 | `art_scale` is per pack. Imported levels without an "OLX hi-res data" chunk use `art_scale` 1. | OLX's 2× surfaces are pixel doubling (`PutPixel2x2`), so 1× is lossless; the renderer scales anyway. |
| A5 | §9.2, §9.4, §7 | Deferred to the plans that first need them: LGS v8 import, pixmap `.lxl`, OLX bitmap fonts (M7), the zip VFS backend, level thumbnails (M7), and `cargo-fuzz` targets. Plan 1 uses stable-Rust `proptest` "never panics" tests on corrupted input instead of fuzzing. | No content in scope needs them (all 6 items are v7 and image-type). `cargo-fuzz` needs a nightly toolchain. |
| A6 | §7, M0 | No hosted CI. `cargo xtask check` runs locally, optionally from a git pre-push hook (`cargo xtask install-hooks`). The `xtask full` tier also runs locally. | The user: single developer, single machine; keep CI light. |
| A7 | §4.1 (7) | `mold` is dropped. | Rust 1.99 links with `rust-lld` by default on Linux. |
| A8 | §9.1 | Each skin is its own pack (`olx-skin-<slug>`). | Keeps one pack kind per directory; ids stay unique, with numeric suffixes on collision. |
| A9 | §4.1 (1) | Per-crate notes live in `CLAUDE.md`; a single root `AGENTS.md` points to them. `cargo xtask layers` fails if a crate lacks `CLAUDE.md`. | One source of truth per crate. |
| A10 | §9.3 | Committed synthetic fixture *packs* (a tiny level, and a mod with one weapon per action type) move to plan 3, where the simulation first consumes them. Plan 1's tests build their synthetic inputs in code: an LGS byte builder, an LXL builder, and sample packs. | Fixtures are most useful when they exercise simulation behavior; nothing in M1 reads them. |

## 16. Amendments made while writing plan 2 (2026-10-10)

These came out of building the oracle against a real OLX build. Where they
conflict with earlier sections, they take precedence.

| # | Amends | Change | Reason |
|---|---|---|---|
| B1 | §6 | Traces are gzip-compressed JSON lines (`fixtures/oracle/<name>.jsonl.gz`, via `flate2`), not zstd. Scenarios live next to them as `fixtures/oracle/<name>.ron`. | `flate2` is pure Rust and already in the workspace; zstd adds a C dependency for no real gain at 4–11 KB per trace. |
| B2 | §6 | A trace starts with a header line holding OLX's version and **every feature value in effect**, then frame −1 (the state when recording starts), then frames 0..n−1. | The simulation can check its default settings against exactly what OLX used, and frame −1 pins the start state. |
| B3 | §6 | Key input goes through OLX's real keyboard handler (`CWormHumanInputHandler`) with virtual keys. The hooks are `simLoadInputs` (key schedule) and `simRecord` (per-frame recording), on top of the existing `simSetSeed` / `simStep`. The extended `setWormInput` from §6 is not needed. | Aim acceleration, facing, rope and digging combos come from OLX's own code, so the oracle captures the real game feel. |
| B4 | §6 | Determinism measures: manual stepping from the lobby on; the LX56 physics clock is aligned with the frame clock when inputs load; worms spawn after that; `ForceLX56Aim` is forced. Every recording is made twice and must match byte for byte. | Without the clock alignment, frame 0 replayed a varying amount of setup time; three runs gave three different traces. |
| B5 | §6, §10 M2 | M2's scenarios cover movement only: fall, walk, jump, aim, rope, dig. Weapon scenarios, and a hook to set worm weapons, come with M4. | Weapons need their own hook and the M4 simulation work to be useful. |
| B6 | §4 | `rl-oracle` is a layer-2 crate with no simulation code. In M3 the simulation turns a `Scenario` into a `Trace` in its own tests. | Keeps the oracle independent of the simulation's layering. |
| B7 | §7 | `cargo xtask oracle-record --check` (Docker, about 1 minute) is not part of the fast gate. Run it after touching `tools/olx-oracle/`. The fast gate checks the committed traces' consistency and behaviour (`rl-oracle` fixtures test). | Keeps `cargo xtask check` fast and free of a Docker requirement. |

## 17. Amendments made while writing plan 3 (2026-10-10)

These came out of porting the LX56 simulation against the oracle until all
seven scenarios matched bit for bit. Where they conflict with earlier
sections, they take precedence.

| # | Amends | Change | Reason |
|---|---|---|---|
| C1 | §4 (`rl-math`) | `rl-math` holds `Vec2` (OLX `CVec`), `AbsTime`/`TimeDiff` (whole ms) and OLX's math macros. OLX's `RandomNumbers[]` table and the glibc-compatible `rand()` move to M4, where they are first used. | Nothing in M3 reads random numbers. |
| C2 | §5.1, §5.7, §5.8 | M3's `World` has `tick(&[Keys])` and derives `Clone`. `TickEvents`, `World::checksum`, the `GameMode` trait and bots arrive with their first users: M4 (events), M5 (rendering consumes `TerrainEdit`), M6 (checksum, modes, bots). | YAGNI: each needs a consumer to be tested against. |
| C3 | §5.2 | Two OLX behaviours are part of the port and documented in `docs/porting/timing.md`: the physics step is `1000 / LX56PhysicsFPS` **whole ms** (11 ms, not 1/84 s), and every frame starts with OLX's lossy Gusanos speed round trip on worm and rope velocities. | Without either, the oracle diverges within 10 frames. |
| C4 | §5.4 | `rl-terrain` carves the material grid only. Applying stamps to the colour images comes with the renderer (M5). | Only the material affects simulation. |
| C5 | §4, §7.4 | New layer-4 crate `rl-scenario` replays oracle scenarios in `rl-sim`. It is used by the M3 acceptance test and by `rl-cli scenario [name...] [--dump <frame>]`, which is the spec's `scenario` and `dump-state` commands in one. | One implementation for the test and for debugging. |
| C6 | §5.8 | `SimSettings` is read from OLX feature values (`from_olx_features`, the trace header); `SimSettings::lx56()` gives the same values. M3 supports LX56 aim only, and rejects wrap-around maps, Gusanos worm physics and `WormFriction > 0`. | These are what the oracle covers; the rest has no reference yet. |
| C7 | §6 | The oracle's material hash now uses the real FNV-1a 64 offset basis (14695981039346656037). The hook had a typo (1469598103934665603). All traces are re-recorded; only their `mh` values change. | Found while porting: the Rust hash did not match. |

## 18. Amendments made while writing plan 4 (2026-10-10)

These decide how M4's weapons, projectiles, damage and bonuses are recorded
by the oracle and reproduced by the simulation. Where they conflict with
earlier sections, they take precedence. The oracle hooks that implement
D1, D2, D3, D6, D8 and D10 are described in `tools/olx-oracle/CLAUDE.md`.

| # | Decision | Reason |
|---|---|---|
| D1 | **Shot delivery, Model R.** Shots added during frames F and F+1 form one list, which is encoded, decoded and processed before frame F+2 (after `simulationTime` advanced past F+1), exactly as in real play at one frame per main-loop iteration (`weapons-firing.md` §1.1). | It reproduces the latency and the lossy multi-shot encoding a player gets. |
| D2 | **Wall clock pinned:** `tLX->currentTime = simulationTime + 10 ms` (K = 10) for the whole frame and for delivery. | Real play has `currentTime` in (T, T+10]. The hit-test position recorder (`get(0)` quirk), explosion damage, spawn times, respawn and bonus timers all read it. |
| D3 | **Client animation semantics.** The oracle loads projectile images in dedicated mode, so `ANI_ONCE` deletes projectiles when their animation ends and action-less animated projectiles live until then. `SimMod` carries each image projectile's frame count (`width / height`). | That is what a player's client does; Classic's Flamer and many MW projectiles use `ANI_ONCE`. |
| D4 | **Cosmetic `rand()` draws are emulated** (dig, rope hook, explosion dirt, blood, bleeding, death, gibs, colour), including the dedicated server's 1024-entry entity pool, which is never freed (a saturating counter reset at match start). | They advance the same generator as gameplay draws. Emulating the dedicated server's count is the only verifiable choice; a real client frees entities and simulates them with `rand()`, which no port can reproduce. Recorded as a deviation. |
| D5 | **Cosmetic effects get their own generator** in `rl-fx` (M5), never the simulation's. | Spec §5.5. |
| D6 | **`FindSpot`'s boost `mt19937` instances are reseeded** by `simSetSeed` in the oracle; the sim seeds its copy from the match seed. | Spawns and bonuses need a reproducible position; OLX seeds 5489 once per process. |
| D7 | **The `VecTimeRecorder::get(0)` quirk is kept.** | It is real-play behaviour; worms are hit at stale positions in OLX too. |
| D8 | **All worms in a scenario use scripted input** (no bot AI during recording). | Bot AI draws `rand()` and reads the wall clock. |
| D9 | **Exact-zero health:** the Gusanos `think()` death path is ported (the worm dies next frame, credited to nobody), but its Gusanos particles' `rnd()` use is not; scenarios avoid exact-zero health. | Rare; the Gusanos particle system is out of scope. Recorded as a deviation. |
| D10 | **Other messages of the local connection are delivered between frames too:** every reliable message the server queues for the local client during frame F (bonus spawns and destruction, kill messages, ...) is parsed by the client after frame F, then the shot list (D1), then every reliable message the client queued for the server (bonus grabs, damage reports); none goes over the network. | One frame of latency, as a real local game at one frame per main-loop iteration sees them; over the network they arrived only after a whole `simStep` batch, which made respawn and bonus traces depend on the batching. |

## 19. Amendments made while writing plan 5 (2026-10-10)

These decide how M5 presents the simulation: rendering, effects, audio,
input and content loading. Where they conflict with earlier sections, they
take precedence. The OLX behaviours behind them are in
`docs/porting/olx-notes/presentation.md`.

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

Made precise while implementing E6 and E9 (plan 5 task 1):

- **Events are the caller's.** `World::tick(&keys, &mut TickEvents)` refills
  a buffer the caller owns (`TickEvents::new` sizes it once), so a world
  clone, such as the renderer's previous-tick snapshot, copies no events.
  Setup events (`spawn_worm`'s) wait in the world and come first in the next
  tick's buffer. This replaces §5.1's `tick(..) -> &TickEvents`.
- **An event's sub-step** is the number of the tick's physics sub-steps whose
  entity pass (OLX's `SimulateEntities`, after the worm loop) ran before it:
  0 for setup, shot delivery and the first worm loop, up to
  `World::substeps_last_tick()` for the last sub-step's projectiles and the
  server after the physics. A renderer that spawns each effect right before
  the tick's entity pass number `substep` (from 0; `substeps_last_tick()`
  means after the last one) steps it exactly as often as OLX's client does.
- **The core pack's files** are listed under `core` in
  `content/olx-manifest.ron`; `rl-content` gives a missing or differently
  sized core sprite a placeholder of its OLX size, and a missing sound silence.
- **One terrain texture (E7, deviating from §8.2).** §8.2 lists separate
  back, front and material textures with the blend in the shader. The
  renderer uploads one opaque RGBA texture, the `ColorMap` composite, because
  `rl-terrain` already composites on the CPU (E7) and the shader needs no
  material grid. Edits upload as dirty rectangles. A shader-side blend can
  return with a pixel-art upscaler or a lighting pass if one needs it.

## 20. Amendments made while writing plan 6 (2026-10-10)

These decide how M6 plays a Deathmatch against bots, records replays and
runs headless matches. Where they conflict with earlier sections, they take
precedence. The OLX behaviours behind them are in
`docs/porting/olx-notes/bots-and-deathmatch.md` and
`docs/porting/olx-notes/damage-death-bonuses.md`.

| # | Decision | Reason |
|---|---|---|
| F1 | **Bot loadouts:** OLX's presets by default (with Classic at `LoadingTime` 100 and "Standard 100lt", the fixed 100lt set and tactics: notes §2.2). A setting gives random weapons through the ported `GetRandomWeapons` instead. | Faithful default, with variety on demand. |
| F2 | **Bot aim:** a gravity-aware ballistic solver by default (the low root, correct sign, rejected when impossible), with OLX's per-difficulty noise and tolerance. `AimModel::OlxDirect` is kept for comparison. | OLX computes the ballistic angle and then discards it, so its bots lob grenades badly. |
| F3 | **Bots press keys like humans** (the 8 `Keys`), through a motor that turns intents into keys under the sim's input rules. They get no mouse aim. | The same physics and constraints as a player; replays store keys only. |
| F4 | **Weapon restriction files** (`.wps`) are imported into mod packs: the raw `name → state` lists and the preset names. Random weapons, bot presets and natural bonuses (`MatchSetup::bonus_weapons`) use them. | Needed for OLX's bot loadouts and bonus weapons; it closes the plan-4 deviation. |
| F5 | **Deathmatch needs ≥ 2 worms.** The sandbox stays for single-worm play. | OLX ends a one-worm local game at once. |
| F6 | **`World::new(MatchSetup)` and the `GameMode` trait live in `rl-sim`.** The callbacks are spawn, kill, damage, pickup, per-tick, is-over and scoring. The command interface: spawn a worm or bot with a loadout, give a weapon, apply a terrain stamp, emit a message, end the match. Deathmatch is in `rl-modes`. The world emits `Message` and `GameOver` events. | Spec §5.1, §5.8, H1; the final review of plan 4. |
| F7 | **Bots run once per 100 Hz tick.** Each owns a PRNG seeded from (match seed, worm, personality). Work is budgeted in units (terrain probes and traces), resumable across ticks. Free rectangles of the static rock are cached per level and shared. | Deterministic and bounded (notes §4.3). |
| F8 | **Replays store setup, pack ids, content hashes, settings, seed and every worm's keys per tick** (bots included), plus a checksum every N ticks. A format version is refused when unknown. | Bot code changes cannot break replays; mismatches are detected. |
| F9 | **Matches start at `T0 = 100 000 ms`,** as the oracle does. | Bleeding and bonus timers depend on absolute time (final review of plan 4). |
| F10 | **The chaos personality** changes loadout on respawn through the mode's "give weapon" command, ignores restrictions, and tracks fired weapons for coverage. | Spec §5.9 and §7.1 coverage. |

Made precise while implementing F6 and F9 (plan 6 task 1):

- **`World::new(MatchSetup, Box<dyn GameMode>) -> Result<World, SetupError>`**
  replaces §5.1's `new(setup) -> World` and the positional constructor.
  `MatchSetup` holds the settings, the `Arc<SimMod>`, the terrain, the
  theme's holes and green-dirt mask, `start` (`MatchSetup::T0` by default),
  the seed, the worms (`WormSetup { team, loadout, spawn }`, spawned in
  order, `None` drawing a `FindSpot` position), `pool_capacity` (OLX's 3000
  by default, 1 to 65 536) and `bonus_weapons`. `World::new` refuses
  settings outside OLX's ranges, more than 32 worms, a bad pool size and a
  loadout of more than 5 slots (`WeaponSlotsNum`).
- **Callbacks run where OLX runs its `CGameMode`:** `on_kill` replaces
  `CGameMode::Kill` inside `killWorm` and returns a `KillCredit` (kill,
  suicide, death) that the world applies before `CWorm::Kill`;
  `olx_kill_credit` is OLX's rule. `on_damage` after `InjureWorm`'s health
  change, `on_pickup` after `GiveBonus`, `on_spawn` after every spawn,
  `is_over` at the start of `GameServer::SimulateGame` (before the
  respawns), `on_tick` at its end (`CGameMode::Simulate`).
- **Commands are queued and applied at one point:** in queue order right
  after `on_tick`, before the frame clock advances (those queued during
  setup at the end of `World::new`). At most 128 a tick; each is checked
  when queued (`CommandError`). A spawned worm's loadout is copied into the
  command and its slots come from vectors made at setup, so no command
  allocates. `give_weapon` makes the slot full and ready (unlike a bonus,
  `LastFire` is reset). `message` carries a `TextId` and three numbers.
- **Game over is OLX's `GameServer::GameOver`:** the world records the
  result (`World::result`), emits `GameOver` once and clears every worm's
  input. From then on kills are ignored (`killWorm` returns), nobody
  spawns or respawns (`SpawnWorm` returns, for a spawn command too), no bonus spawns, worms read no input and do not shoot, pick up
  or aim their laser sight; the client clears its projectiles at the next
  delivery (`ParseGameOver`), and the physics stops 3 s after the end
  (`GAMEOVER_WAIT`).
- **Reading a mode:** `World::mode_state()` gives the `&dyn GameMode`;
  `GameMode::as_any` lets a caller downcast it to read the mode's own state.
- **`OracleMode`** (in `rl-sim`) is OLX's crediting with no end, no
  scoreboard and no commands: the oracle scenarios and the sandbox play it,
  so the oracle stays bit-exact. (The oracle's dedicated server runs with
  `AllowEmptyGames` and unlimited lives, and no scenario reaches the kill or
  time limit, so OLX never ends a recorded match either.)
- **The checksum** includes the mode's state (`GameMode::checksum`) and the
  match's end, and hashes every NaN as one canonical NaN.

Made precise while implementing Deathmatch (plan 6 task 2):

- **`rl_modes::Deathmatch::prepare(DeathmatchSettings, &mut MatchSetup)`**
  refuses fewer than two worms (F5) and copies the rules the simulation
  applies (lives, kill limit, scoring) into `MatchSetup::settings` and the
  worms' lives; the time limit and the spree thresholds are the mode's.
- **OLX checks for the end every frame of a match** (`recheckGame` is set
  by `BeginMatch` and never cleared while playing), so Deathmatch's
  `is_over` ends the match in the frame of the deciding kill or the first
  frame past the time limit, with no delay to model.
- **Messages are `TextId`s** of `rl_modes::text` with OLX's default texts
  (`olx_text`); the end's texts are sent in the `on_tick` of the frame that
  ended, after the `GameOver` event.

Made precise while implementing weapon restrictions and loadouts (plan 6 task 3):

- **Mod packs carry raw presets** (pack format 3, `restrictions.ron`): the
  `name → state` lists of every `.wps` OLX finds for the mod
  (`cfg/presets/<mod dir>/`, then `cfg/presets/`), as `ModDef::restrictions`.
  `rl_data::restrictions::WeaponRules` resolves a `WeaponRestrictionsFile`
  value against them like OLX's `CWpnRest` after `updateList`; natural
  bonuses (`MatchSetup::bonus_weapons`) get its not-banned ids, bots its
  enabled ids. `SandboxSetup::restrictions` defaults to `Standard 100lt`.
- **`rl_bots::choose_loadout(&BotConfig, &WeaponRules, &SimSettings, &mut
  BotRng) -> Loadout`** takes the restrictions and settings rather than a
  `MatchSetup` (a setup's worms need their loadouts first) and the bot's own
  generator: a Deathmatch bot plays OLX's presets (`OlxPreset`, default) or
  random weapons (`Random`); a chaos bot draws random weapons ignoring the
  restrictions until its mode's rotation (F10) changes them.

Made precise while implementing the bots' navigation, motor and aim (plan 6 task 4):

- **`rl_bots::Bot::new(worm, BotConfig, seed: u32, &World)`** (the match
  seed, as `MatchSetup::seed`) or `Bot::with_cache(.., Arc<AreaCache>)` to
  share one per-match area cache between a match's bots, which then think
  in a fixed order; `think(&World, &TickEvents) -> Keys` runs before each
  `World::tick` with the previous tick's events. `BotConfig` gains `params:
  Option<DifficultyParams>` (`None`: the difficulty's), `aim: AimModel` and
  `budget` (8192 units by default; `Bot::budget` raises it to what the
  level's longest worm line needs).
- **A work unit is one terrain pixel read.** Every trace checks its worst
  case before it starts, so a `think` never spends more than its budget;
  growing an area pauses per pixel, a search between checkpoints and
  traces. Bots allocate only in `new`.
- **The 7 px passage rule, as OLX has it:** the checkpoint's own row is not
  traced and an area must span 8 px, so a gap in a wall passes only from 8
  px, or at 7 px when a checkpoint (every 5 px along the area's edge) lies
  on the row just outside it; 6 px never.


Made precise while implementing the bots' brain, weapons, rope and chaos (plan 6 task 5):

- **The brain is OLX's `getInput`** for a Deathmatch bot (`Personality::
  Deathmatch`); `Bot::set_goal(Some(p))` makes it race to `p` (OLX's race:
  no shooting, no bonuses). Shooting gets at most a quarter of a think's
  budget, moving the rest of half, and the pathfinding search the rest;
  probes that do not fit wait a tick.
- **The chaos rotation is a Deathmatch setting fixed at the setup:**
  `DeathmatchSettings::respawn_loadout` is `RespawnLoadout::Keep` (OLX) or
  `ChaosCycle(ChaosCycle::new(chaos worm ids, weapon count))`, applied in
  `on_spawn` (the first spawn is life 0) with `give_weapon`, so bots only
  press keys and replays stay keys only. A chaos bot prefers the slots it
  has not fired; `Bot::fired_weapons` lists what it fired.
- **Aim inputs** (F2): `ballistic_from(from, to, &Launch, settings)`, where
  `Launch` is the projectile's speed, gravity, spawn distance and carried
  velocity; `DifficultyParams` gains `weaponCanHit`'s `gravity_noise` (5·f)
  and `speed_noise` (20·f), drawn with the target noise every 0.5 s.
- **An `AreaCache` knows its level's rock** (`rock_hash`): `Bot::with_cache`
  replaces a cache of another level even of the same size.

Made precise while implementing replays (plan 6 task 6):

- **`rl-replay` (L3) holds the format, the `Recorder` and the verifying
  player; `rl-content` resolves the packs** (`Content::replay_packs`,
  `world_for_replay`, `play_replay`: the plan's `Replay::play(&Content)`).
  A replay names the mod, the level and its theme by pack id and
  `rl_pack::content_hash`, and refuses content that hashes differently.
- **The setup is the `MatchSetup` as `World::new` gets it** (after
  `Deathmatch::prepare`; the mode is rebuilt with `Deathmatch::new`) plus the
  setup calls made before the first tick (`set_health`, `place_bonus`,
  `find_spot`, `align_physics_clock`), which go through `Recorder::apply`.
- **Keys are recorded as `World::tick` got them** (rows may be short or grow
  with worms spawned mid-match; missing keys are none pressed), checksums
  before the first tick (`Recorder::start`, after the setup calls), after
  every 100th tick and after the last; `Recorder::finish` takes the final
  world for that last checksum and checks it saw every tick. A mismatch at
  tick 0 is a different setup or content; a later one reports the
  checksum's tick: the divergence is in the 100 ticks before it.
- **Format:** `RLRP`, a `u16` version (1), postcard; keys stored by worm and
  deflated. One minute of 8 worms is at most 50 000 bytes whatever the keys
  (8 bots on Classic: about 12.6 KB). Other versions are refused.

Decided while closing plan 6 (task 9, the user's direction after the
playtest and the OLX bot baseline, `docs/porting/bot-baseline-2026-10-11.md`):

- **No difficulty aims perfectly** (amends F2's "OLX's per-difficulty noise
  and tolerance"): the aim errors (`AI_SetAim`'s offset, `weaponCanHit`'s
  target, gravity and speed noise) scale with at least
  `rl_bots::AIM_ERROR_FLOOR` = 1/6 of Easy's, so Xtreme, error-free in OLX,
  keeps half of Hard's error; the fire tolerance stays OLX's. Xtreme stays
  the strongest level (measurements in `docs/porting/STATUS.md`). Bot
  difficulty is to grow from strategy, quick reactions and predictive
  reloading (teamwork with team modes), never from perfect aim: a later
  bot-skill milestone, measured against OLX with the baseline harness.

## 21. Amendments made while writing plan 7 (2026-10-11)

These decide how M7 (v0.1) turns the playable Deathmatch into a game
started from menus. Where they conflict with earlier sections, they take
precedence. Binding user decisions: the window is not drag-resizable or
maximizable (sizes come only from options presets and startup flags,
through the paced size-change path), and bots offer OLX's four
difficulties, none aiming perfectly (§20).

| # | Decision | Reason |
|---|---|---|
| G1 | **`rl-ui` (L5): screen logic and view models separate from egui widgets.** Screens: Main, LocalPlay, Profiles, Options, WeaponSelect, InGame (HUD + pause), Results, Import, About. | Spec §8.3; testable by `egui_kittest` and plain unit tests. |
| G2 | **Persistence** through `directories` (`ProjectDirs` "rust-liero"): `settings.ron`, `profiles.ron`, `replays/` (auto-saved last matches, capped by count). Every format is versioned; an unknown version means defaults plus a warning. | Spec §9.4. |
| G3 | **Window size** comes from options presets (e.g. 1280×720, 1600×900, 1920×1080, the native desktop size) plus a fullscreen toggle, applied at most every 250 ms through the existing window code, and remembered. | User decision; the WSLg compositor crash. |
| G4 | **OLX's weapon selection before the match** (and when OLX allows re-selection): five slots, restricted by the preset, with a "random" button and keyboard/gamepad navigation. Bots keep their loadouts (F1). | OLX feel; replaces the random human loadout. |
| G5 | **HUD:** health, the five weapon slots with charge and reload, a kill feed, a Tab scoreboard, a time and kill-limit readout, and a minimap (OLX's colours and scale from `presentation.md` §2.4). Two styles: **Modern** (egui, the default) and **Classic** (OLX's bitmap fonts and layout). | Spec §8.3. |
| G6 | **Thumbnails and fonts are imported:** a level thumbnail (`thumb.png`) generated from the level art at import, and OLX's `font.png`/`out_font.png` in the core pack. Pack format bump per policy. | A5 deferred both to M7. |
| G7 | **`--autoplay <script.ron> --fake-clock`** drives the real app through menus and a match with scripted input and a fake clock, taking screenshots at checkpoints. It runs on Xvfb in `xtask full` and asserts a clean structured log. | Spec §7. |
| G8 | **Benchmarks** (`divan`): tick time with 3000 projectiles, an 8-bot tick, DrawList build and an offscreen render frame. Baselines are stored in `fixtures/bench/`; a regression over 10% fails `xtask full` (with a noise guard: medians over several runs). | Spec §7. |
| G9 | **About/credits:** Jason Boettcher (Liero Xtreme), the OpenLieroX team, Joosa Riekkinen (Liero), and the licenses. | Spec §11. |

Made precise while implementing `rl-ui`'s foundation (plan 7 task 1):

- **`UiState::update(UiAction) -> Vec<AppCommand>`** is the whole menu
  logic; `rl-game` feeds it the widgets' actions and its own reports
  (`MatchStarted`, `MatchFailed(why)`, `MatchOver`) and carries out the
  commands (`StartMatch(MatchOptions)`, `StartSandbox`, `EndMatch`,
  `SetPaused`, `ApplyVideo`, `ApplyAudio`, `ApplyBindings`,
  `SaveSettings`, `SaveProfiles`, `Quit`). A start emits one command and
  ignores Start and Back until the app answers; a failed start returns to
  local play with the reason; every match gets the next seed; a rematch
  ends the match over first. Options apply and save only what changed.
- **Files** are `(version: 1, settings: (...))` and `(version: 1,
  profiles: (...))` in RON, every struct `#[serde(default)]` (a missing
  field takes its default), written atomically (temporary file, sync,
  rename). A bad file (not UTF-8, over 1 MiB, unreadable RON, another
  version, values out of range) gives defaults or the values fixed, one
  warning per problem. An undecodable file is moved to `<name>.bak` (then
  `<name>.1.bak`, ...: earlier backups stay, up to 100, then the oldest
  goes); a decodable one with values fixed is copied there and the fixed
  values are saved in its place, so its other values survive. A file
  that cannot be backed up is never overwritten. Without a data
  directory nothing is read or saved.
- **The match preferences mirror** `DeathmatchSettings` (without the
  respawn loadout) and the bot choices (`BotLevel`, `BotLoadout`) in
  `rl-ui` instead of giving `rl-modes` and `rl-bots` serde: the file holds
  only what the menus edit (no budgets, aim models or chaos cycles), under
  names `rl-ui` owns, and the `no_std` crates stay unchanged (as
  `rl-replay`'s mirror, §20). `BotConfig::validate` refuses NaN, infinite
  or negative params, and `rl_content::build_match` checks every bot.
- **One source of rules** (plan 6's final review): `Deathmatch::new` is
  private; `Deathmatch::prepare` makes a new match's mode and
  `Deathmatch::resume(settings, &SimSettings)` a recorded one's (a replay,
  an oracle trace), refusing settings that disagree with the rules.
  `Deathmatch::time_left_ms` stops at the game-over frame.
- **The default profile** is OLX's first local player ("OpenLieroXor",
  green, the default skin); names are at most 20 characters (OLX's
  player menu).

Made precise while implementing the main menu and local play (plan 7
task 2):

- **Menus first.** `rl-game` without a flag that chooses a game opens the
  main menu (Local Play, Sandbox, Profiles, Options, Replays, Import,
  About, Quit); the flags of plans 5-6 (`--mode`, `--replay`, `--level`,
  ...) still play their game at once, for scripted runs and tests. The
  first match's seed comes from the clock; each match takes the next, and
  every seed is in its replay.
- **Screens are `rl-ui`'s.** `rl_ui::screens::show(ui, &UiState, &mut
  MenuData) -> Vec<UiAction>` draws them with egui (no GPU handle: level
  thumbnails are egui textures); `rl-game`'s `Shell` carries out the
  commands without a window, so starts with missing content and rapid
  actions are tested headless. Esc is Back everywhere (in game it pauses);
  F5 restarts in place.
- **Replays screen** (not in G1's list): the `.rlrp` files in the data
  directory's `replays/` and any file by path, played with the game's
  replay session. Every Deathmatch played from the menus is saved there
  when it ends, restarts or the game quits (`match-NNNNNN.rlrp`, the last
  20; a player's own files are never removed).
- **Local play** sets the mod, the level (a grid of thumbnails; a level
  without `thumb.png` shows its name and size), OLX's `Lives`,
  `KillLimit`, `TimeLimit`, `LoadingTime` (the menu ranges of
  `FeatureList.cpp`), `Bonuses` and `WeaponRestrictionsFile` (the mod's
  presets), 1 to 31 bots at one difficulty or each its own, their
  personality (OLX's bot or chaos) and loadout, and the player's profile
  (its name and colour play worm 0). Start loads after the frame that
  shows "Loading…"; until then another Start, Esc and the menu buttons do
  nothing. A start that fails (a pack missing or broken, a file that is no
  replay) returns to its screen with the reason.
- **Menus list packs without loading them:** `rl_content::Content::catalog`
  reads each mod's manifest and preset names and each level's `level.ron`
  and `thumb.png` (at most 512 × 512 and 4 MiB; else not shown, with a
  warning).

Made precise while implementing the profiles (plan 7 task 3):

- **Profiles screen** (OLX's player menu): the list (the one playing
  marked) with New ("Player N", OLX's local players' colours in turn),
  Delete (confirmed; never the last) and "Play as this"; the edited
  profile's name, OLX's red, green and blue sliders, and every imported
  skin, searchable (`Catalog::skins`, names from the packs' manifests).
  A name is trimmed, not empty, at most 20 characters, has no control
  character and is no other profile's (ignoring case); a refused name is
  shown with why and not applied.
- **Skin preview, one source of truth:** `rl_content` holds OLX's frame
  cut (`skin_source`, the left-facing column quirk) and
  `LoadedSkin::colorized`; `rl-scene` draws worms and `rl_ui::skin_preview`
  cuts the preview's frames from them, faced right and left, animated by
  OLX's menu formula. `rl-ui` asks for a preview by skin and colour; the
  app uploads the frames as egui textures and hands back their ids (no GPU
  handle in `rl-ui`).
- **In matches** the selected profile's skin and colour are the human's
  (worm 0); bots and every worm of a replay wear OLX's default skin.
- **Repairs:** a saved local-play mod or level no longer imported is
  replaced by the first listed; the replays list shows a file's header
  problem (not a replay, another format version) when it opens
  (`Replay::check_header` reads 6 bytes).

Made precise while implementing the options (plan 7 task 4):

- **Options screen** (OLX's options menu, laid out anew): Video (window
  preset, fullscreen, VSync or uncapped, upscale filter, interpolation, UI
  scale), Audio (OLX's sound volume, default 70 %, times an effects
  volume), Keys, Gameplay (HUD style; the match local play starts with,
  with a reset to OLX's settings). Every change applies at once and is
  saved (the file at most once a second).
- **Window changes** go only through `rl-game`'s paced requests, and only
  when the preset or fullscreen changed (`rl_game::video::Video`): toggling
  interpolation, the filter, VSync or the UI scale spends no window change.
  The saved window opens at its size and mode when the window is created
  (no resize follows); `--fullscreen`, `--uncapped`, `--filter` and
  `--no-interp` win over the saved options for the run, which the options
  show, and the file keeps its values until the player changes them there.
- **Key bindings** (Review Focus 2): every action keeps at least one key
  and one gamepad input. A captured input another action has is moved
  (refused when that action would have none of that device left) or shared,
  on the player's choice; the last input of a device cannot be removed;
  the game's keys (Esc, F2, F5, Tab, H) are refused; bound keys that make
  a browser shortcut are pointed out. Bindings from a file (`settings.ron`,
  a profile's own keys; either list may be missing) get OLX's defaults back
  for an action left unreachable, with a warning, and the rest of the file
  is kept. The capture is `rl_input::Capture`, which the app feeds the
  window's key presses and the gamepads' events (egui sees neither
  modifier keys alone nor gamepads). A profile with its own keys is
  rebound there.

Made precise while implementing thumbnails and the credits (plan 7 task 6):

- **G5 and G6 narrowed (the user's call):** the Classic HUD style was
  dropped. Drawing OLX's small bitmap fonts and HUD images cleanly at any
  DPI, UI scale (0.5-3) and window size, and keeping a second HUD in step
  with the first, costs more than it gives. The HUD has one style (Modern);
  OLX's `font.png`/`out_font.png` and HUD images are not imported, and the
  settings hold no HUD style (an old `hud:` value is ignored). §8.3's
  "classic" style is withdrawn.
- **Thumbnails:** each level pack's `thumb.png` (format 4) is its front
  art box-filtered and letterboxed into 160 × 96 at import. It is
  presentation only: `rl_pack::content_hash` leaves it out (ruling Q4).
  The menus list a thumbnail by its PNG header (bounded size and length)
  and decode it only when its card is in view, at most two a frame, once.
- **About** (G9): the credits from the main menu: Joosa Riekkinen (Liero),
  Jason Boettcher (Liero Xtreme, credited and its name not used, as its
  license asks), the OpenLieroX team as OLX's main menu names it; the
  licenses (GPL-3.0-or-later; OLX's LGPL code; LX's zlib-style code); and
  each bundled mod's and level's recorded origin.
