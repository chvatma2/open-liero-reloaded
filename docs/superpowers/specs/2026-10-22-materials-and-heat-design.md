# Surface materials and heat: design

Status: agreed with the maintainer on 2026-10-22, for milestone 0.2.0.
Implemented through runner issues (see "Issues" at the end).

## Intent

Explosions should leave the level **hot**: rock and dirt around a crater
glow like lava at the rim, then cool slowly through orange and dull red
back to their own art, lighting the cave while they glow. Near background
walls and near distant (parallax) layers heat up too, less the further back
they lie, as if the explosion were a 3D ball. This adds depth and drama to
the lit levels of 0.2.0 (look system, glowing light, distant layers).

Heat is the first effect of a new **surface material** system: every pixel
of a level can be given a material (stone, wood, metal, glass, …) whose
properties drive effects. The game ships a basic set of materials; players
make their own in a new **material editor** with live previews.

## Two kinds of "material"

- **Collision kind** (Rock, Dirt, Background): OLX's, the simulation's,
  `material.png`. **Unchanged.**
- **Surface material** (new): presentation only, like the rest of the look
  system. The simulation, bots, replays' checksums and online play never
  see it.

They are independent. Switching an element from Rock to Background in the
editor changes only whether worms pass through it: its pixels keep their
surface material and still heat like it (as a wall behind the worms).

## Goals

1. A material format and a library: a built-in basic set plus the user's
   own materials, and per-pixel material assignment in levels.
2. Heat: weapons deposit heat; heat spreads, soaks in and cools per pixel
   by the material's properties; hot pixels change colour and glow, and
   cast light through the glow light.
3. Distant layers near enough take heat as slices of the explosion's ball.
4. The level editor assigns materials (per element and painted per pixel).
5. A material editor with previews.
6. The Reloaded levels use materials.

## Non-goals

- **Any effect on play**: a surface material never changes collision,
  digging, damage, bots or the simulation (no unbreakable metal, no
  shattering glass). If gameplay materials come later, they are a separate
  design.
- Effects other than heat in this round (wet, frozen, scorch marks left
  after cooling, smoke). The format leaves room for them (see "Format").
- Heat on worms, sprites or projectiles; heat changing sound.
- Heat surviving a replay seek (seeking clears it) or matching between
  online players exactly (each client heats from the explosions it shows,
  as with every effect).

## Anti-goals (a failure even if it works)

- An OLX level, or any level without materials, looking or costing
  anything different: no heat texture, no pass, render goldens
  byte-identical.
- A per-player graphics toggle for heat. Looks are owned by the level and
  mod packs, hashed, the same for everyone.
- Heat hiding worms: a hot crater must not glare so much that a worm in it
  cannot be seen (the glow light's floors and caps hold).
- An idle level paying for heat: with nothing hot, the heat pass costs
  nothing.
- A panic or unbounded allocation from a hostile material, level or mod.

## Format

### A material

A material is RON, validated like every look file (bounds checked in
`olr-format`, clamped again where read in `olr-look`). Fields (names are
the builder's call; the meaning is fixed):

- `id` (slug), `name`, `notes`.
- `heat`: optional; none means the material never heats.
  - `sensitivity`: percent 0-100, how much of the deposited heat the pixel
    takes. 0 is the same as no `heat`.
  - `capacity`: how much heat it takes to warm the pixel (higher: warms
    less from the same blast and holds its heat longer).
  - `conductivity`: how fast heat spreads to neighbouring pixels of
    conducting materials, and so how deep it soaks in.
  - `cooling`: how fast an exposed pixel (beside air) loses heat to the
    air.
  - `look`: how it looks hot: a colour ramp of temperature to colour and
    glow strength (default: the engine's, dull red → orange → yellow-white
    lava), and `light`: percent of its glow cast as light (0: it shows
    hot but lights nothing).
- Room for later effects: further optional sections beside `heat`.

The units are artist-friendly integers (percent and named ranges), not
physical units; the builder picks the ranges and the engine's internal
scale, and documents them in `docs/formats/`.

### The library

- **Built-in**: `content/materials/<id>.ron`, beside `content/assets/`,
  read-only, bundled by `dist`. The basic set (*maintainer: "wood, rock,
  metal, glass and so on"*; the list is the builder's call within this):
  stone, dirt/earth, sand, brick, concrete, wood, metal, glass.
- **The user's**: `materials/` in the game's data directory (the store,
  beside the user's `assets/`), listed after the built-in ones, as the
  asset library is (`olr_edit::asset::list_library`, `Catalog::assets`).

### In a level

A level is self-contained (it downloads for online play): its look holds
**copies** of the materials it uses, by id, as it holds glow groups. Using
a library material copies it in; an asset carries the materials its
elements name and placing it joins them by id or adds them, exactly as
glow groups do (`docs/formats/asset.md`).

- `level.ron`'s `look`: `materials: [Material]`, at most 255.
- Per-pixel maps baked like the glow groups (`look/groups.png`,
  `look/groups-back.png`): `look/surface.png` (the front: rock and dirt)
  and `look/surface-back.png` (the back: what shows where the front is
  carved or Background), world resolution, value `v` = `materials[v-1]`,
  0 none.
- A composition element gets `surface` (its front pixels' material) and
  `back_surface` (its back art's), optional, plus optional painted
  per-pixel maps at **world** resolution (heat is per world pixel):
  `editor/surface/<id>.png` (where the element's own pixels land) and
  `editor/surface/<id>.back.png` (its back art, only with back art). A
  painted value overrides the element's default per pixel (0 there: the
  default). A Raster element is given materials only by painting. On a
  `mixed` Drawn layer each pixel's `surface` lands by that pixel's
  collision kind (Rock/Dirt front, Background back). *(Builder check; my
  call.)*
- Where an element's `surface` lands follows where its pixels land: a Rock
  or Dirt element's in `surface.png`, a Background element's in
  `surface-back.png` (its pixels are back art). `back_surface` is allowed
  only on elements with back art (a Drawn layer with `back`, a Raster) and
  lands in `surface-back.png`. *(Builder check; my call.)*
- An element with `distant` (a field on `Element`, allowed on a
  Background Image or a plain Drawn layer) takes one `surface` for the
  whole layer, and the baked `level.ron` `DistantEntry` gets the same
  field (today only `depth` and `at`). The "no material" wording changes
  to "no collision material" in `olr-format`'s
  `levels/composition.rs` and `levels/files.rs` docs, from which
  `cargo xtask format-docs` writes `level.md`.
- The maps are **static**: dirt added during play (dirt balls and other
  OLX effects) takes whatever the baked `surface.png` holds at that pixel,
  usually none where it was air, so the material map never changes in
  play and needs no CPU updates. *(Builder check; my call.)*
- A level without `materials` (every OLX level) has neither map and draws
  exactly as today.

### In a mod

Weapons only deposit heat; the level's materials decide what it does.

- A projectile's `look.explosion_heat`: `amount` (percent of the engine's
  default for that explosion) and `radius` (world pixels, bounded) of every
  explosion it makes. The mod's `look.explosion_heat` is the default of
  projectiles without one, as `look.explosion` is for light.
- A projectile's `look.heat`: heat it deposits around itself each tick
  while it lives (flames, napalm, a hot slug), `amount` and `radius`.
- Without either, every explosion deposits the **engine's** heat, scaled by
  its size, so any mod (Classic too) heats a level with materials.
  *(Builder check; my call:)* "every explosion" means the explosion
  entities the presentation already turns into lights (`olr-fx`'s, as
  `olr-present/src/look.rs` does): a projectile's explosion and a bonus
  blown up. **Not** explosive trails (cosmetic, every 50 ms) and not beams
  unless a mod gives them `heat`. The engine's heat grows with the size
  so a small impact (a minigun hit) warms the rock only faintly, under
  the visible glow threshold unless hits pile up in one place; the
  builder calibrates the curve so a Bazooka's rim glows a few pixels deep
  and a Big Nuke's far more.
- The mod editor shows these fields beside the explosion light.

## Heat simulation (behaviour)

- **Where**: in the renderer, on the GPU (`olr-render`), as a heat texture
  per level at world resolution plus one per heated distant layer. The
  material table is a small uniform/storage array indexed by the maps.
- **Deposit**: the presentation (`olr-present`, `src/look.rs`) passes heat
  deposits (position, radius, amount) to the renderer. *(Builder check; my
  call:)* an explosion deposits **once**, from the per-tick records (as
  `Scene::ended_projectiles` and `shots` are read), keyed through the
  existing `Once` memory so a rollback re-simulation never deposits twice;
  not from the live `olr-fx` explosion entities, which a frame can miss
  and which would make heat depend on the frame rate. A projectile's
  per-tick `heat` deposits `amount × ticks elapsed` at its drawn position. Heat falls off from the centre to the radius. It lands
  after the explosion carves: the crater's rim (front) and the exposed
  back wall inside it take it.
- **One heat value per world pixel.** The visible surface's material rules
  it: the front's where rock or dirt stands, else the back's. A pixel dug
  out later keeps its heat, now ruled by its back material (issue #67:
  no longer; a pixel that changes layer, dug out or covered by new dirt,
  starts cold, `docs/porting/STATUS.md` "Heat stays in its layer"). A
  pixel with no material holds no heat.
- **Step**: heat spreads between neighbouring pixels by their
  conductivity, scaled by capacity (issue #67: only within a layer, both
  solid or both background); pixels beside air lose heat by their
  `cooling`. The step runs on the match clock (never the wall clock),
  stable for every allowed value (substeps or a clamped coefficient, the
  builder's call). Paused or replay-paused, nothing changes. **It advances
  by the match time elapsed, ticks that were never drawn included** (the
  glow light steps once per drawn frame; heat must not: `olr-cli`'s
  renders run many ticks and draw one frame).
- **Look**: a hot pixel shows its colour from its material's ramp over its
  art, glowing whatever the darkness, and it emits into the glow light by
  its `light`, so a fresh crater lights the cave. A level with materials
  always plays lit (materials live in its look, and a level with a look
  plays lit: `olr-present/src/lib.rs`); in the editor's FullBright view
  the hot colour still shows.
- **Cost**: an idle level (nothing above the visible threshold) runs no
  heat pass; with heat, the pass covers only an active region tracked on
  the CPU (deposits grown by conduction, dropped once it must have cooled),
  never a GPU read-back. Hard caps keep a hostile mod (heat every tick
  everywhere) bounded.
- **Reset**: a new level, a restart or a replay seek clears the heat.
- **Split screen**: one heat simulation per frame for the whole level,
  like the glow light (issue #26), never per view.

### Distant layers: the ball

- An explosion is treated as a ball of its heat radius `R` centred on the
  level's plane. A distant layer at depth `d` percent lies at `z = d × K`
  world pixels behind the plane (`K` an engine constant, the builder's
  call, calibrated so a Big Nuke reaches a layer of depth ~20 and a
  Bazooka only the nearest). It takes heat in the circle of radius
  `sqrt(R² - z²)` (none when `z ≥ R`), with the amount falling off the
  same way.
  *(Issue #65 put the level's own back wall on the same line, 14 px
  behind the plane, and the layers behind it: `z = 14 + d × K`; dirt
  takes the blast whole, 6 px deep.)*
- The circle is centred on the layer pixel drawn behind the explosion at
  that moment, for the camera of the view nearest the explosion (one view
  outside split screen). Afterwards the heat stays on the layer and moves
  with it.
- A layer's heat cools by its material; it has no conduction to the level.
  A layer without a `surface`, whose material does not heat, or that no
  allowed radius (the heat radius cap) can reach at its `z`, gets no heat
  texture.
- A hot layer **shows** its ramp colour and glow but **casts no light**:
  distant layers only receive light today, and their placement differs
  per view while the glow light is one per frame. *(Builder check; my
  call.)*

## Editors

### Level editor (`olr-editor`)

- A **Materials** section in the level's look: the level's materials, add
  from the library (copies in), remove an unused one.
- Per element: `surface` and `back_surface` pickers (front/back), and on a
  distant layer one `surface`.
- A **Surface** brush that paints per-pixel material onto the selected
  element's map (its tools and zoom like the existing brushes).
- A **Materials view** that tints each pixel by its material (a legend),
  like the light-only view toggle.
- Save as asset carries the materials, as glow groups.
- Play shows the heat (it plays the level with the mod).

### Material editor (new crate, an Editor tab)

- Opened from the Editor screen's **Material** tab, beside Level, Skin and
  Mod. A new crate (`olr-material-editor`, layer 5, the skin and mod
  editors' pure-state pattern: `update(Action) -> Vec<Command>`, egui
  panels, `egui_kittest` tests), hosted by `olr-game`.
- Lists the built-in materials (read-only: "Edit a copy") and the user's;
  New, Copy, Rename, Delete; every field with its bounds; Save to the
  user's library (checked, no errors).
- **Previews**, rendered by the real renderer's heat pass (not a
  separate approximation). *(Builder check; my call:)* the host renders
  them, as the other editors' hosts do their work: `olr-game` owns a
  second, offscreen `Renderer` for the preview scene and hands the editor
  an egui texture; the editor crate depends on neither `olr-render` nor
  `olr-game` (the skin and mod editors' rule). The preview scene's
  picture is tested by GPU tests in `olr-game`/`olr-render`, the panels
  by `egui_kittest`.
  - a patch of the material (rock/dirt shape with a crater) over a dark
    look: click to blast it (a choice of engine explosion sizes), watch it
    heat, glow, light the patch and cool, with a time scale (1×, 4×) and a
    restart;
  - side by side with a second material for comparison;
  - the colour ramp drawn as a strip.

## Constraints

- `olr-look` stays `#![no_std]`/wasm and `olr-format` wasm (it is not
  `no_std`); the material types live there. The simulation crates never
  depend on them.
- Determinism rule 7: the heat steps on the match clock; output that is
  written (goldens, renders) is the same each run on one adapter.
- Rule 6: every bound checked in `olr-format` and clamped again in
  `olr-look`; a hostile file never panics. New limits are constants named
  in `docs/formats/`.
- `xtask/layers.ron` registers the new crate; `cargo xtask format-docs`
  regenerates the format pages.
- Memory: a level of 4096² world pixels at 4 bytes a pixel is 64 MiB of
  heat texture: the builder may use a 16-bit format if precision holds,
  or allocate only when the level has materials (always).
- Render goldens and match goldens unchanged for levels without
  materials; new GPU tests and goldens for heat are added.

## Testing (agentic loops)

- Heat seen from the command line through `olr-cli render-match` on a
  fixture level with materials (under `fixtures/look/levels/`), as issue
  #15's `look-blast.png` golden does (`olr-cli/tests/render_golden.rs`):
  real matches carve real craters. `olr-cli render` takes only oracle
  scenarios and `look-preview` runs no match, so neither can. Goldens at
  a few ticks after explosions (glowing rims, then cooled).
- GPU tests: no materials → byte-identical frame; heat spreads only into
  conducting materials; a material with sensitivity 0 never heats;
  cooling reaches zero; the distant slice is empty past `z ≥ R`; idle
  costs no pass; hostile values stay bounded.
- Format tests: bounds, canonical form, a level joining an asset's
  materials.
- Measurement (`needs-bench`, `docs/deferred-verification.md`): the
  Windows 1080p frame during a Big Nuke on a level with materials against
  the same level without, and a playtest of the look (rims readable, no
  glare hiding worms).

## Left to the builder

Field names and ranges, the heat equation's discretisation and internal
scale, the texture format, the active-region tracking, `K` for distant
layers, the default colour ramp, the basic set's exact values, the
editors' layouts within the existing editors' patterns.

## Issues

The implementation is split into runner-sized issues, in this order:

1. Materials 1/10: the format, the library and the per-pixel maps.
2. Materials 2/10: assets carry materials; `olr-cli` operations.
3. Materials 3/10: the renderer's heat (texture, step, look, emission,
   active region, caps), behind a direct deposit API.
4. Materials 4/10: explosions deposit heat (the engine's curve), the
   fixture level and the goldens.
5. Materials 5/10: heat on distant layers (the ball's slices).
6. Materials 6/10: mods' heat (`explosion_heat`, `heat`), mod editor
   fields.
7. Materials 7/10: the level editor (the Materials list, pickers, the
   Surface brush, the Materials view).
8. Materials 8/10: the material editor (crate, tab, list, fields, save).
9. Materials 9/10: the material editor's previews.
10. Materials 10/10: the Reloaded levels and mod get materials and heat.
    *(The maintainer, 2026-10-22: the runner does it.)* Both levels are
    Drawn layers (Liero Factory Reloaded split from the imported level,
    whose `material.png`, `front.png` and `back.png` stay byte for byte;
    Castle Strike Reloaded's background, background-2, dirt and rock):
    per-element defaults, then a committed, re-runnable rule that paints
    per pixel from collision kind and art colour (grey rock → stone or
    concrete, rusty or steel-coloured → metal, wood-brown → wood, earth →
    dirt, the factory's glass and screen layers → glass), with
    `render-match` renders left for the maintainer's review. The Liero
    v1.0 Reloaded mod gets heat values (flamer and napalm `heat`, big
    explosives' `explosion_heat`). The playtest is listed in
    `docs/deferred-verification.md`; `content/playtest/` is not part of
    it.
