# Look System: Design (0.2, sub-project 3)

- **Status:** agreed with the user on 2026-10-18, question by question.
  Built as plan 15a (`docs/superpowers/plans/2026-10-18-plan-15a-look-system.md`),
  with the playtest's changes of 2026-10-19 (soft shadows by default, glow
  on selected pixels, blood and gibs on the solid floor); its decisions are
  in `docs/porting/STATUS.md`, "Look system decisions (plan 15a)". Worms in
  the light follow in plan 15b (`2026-10-19-worm-lighting-design.md`).
- **Milestone:** 0.2 (`docs/superpowers/specs/2026-10-16-milestone-0.2-scope.md`),
  sub-project 3, "Graphics groundwork and look format".
- **Builds on:**
  - the formats (`2026-10-16-formats-design.md`: the reserved `look`, the
    content hash);
  - the level composition and editor (`2026-10-17-editor-foundation-and-level-editor-design.md`
    §1 channels, and `2026-10-18-level-editor-polish-design.md`);
  - the render pipeline (`2026-10-10-foundation-and-local-play-design.md`
    §8.2, §12.3, §19 E3 and E7);
  - the performance envelope (`docs/perf/envelope.md`).

## Why

0.2 gives OLR a modern look while it stays pixel art and plays the same.
The look is authored content: a level's darkness, its lamps, its lava and
its sky are the level's identity, and a mod's glowing rockets are the
mod's. This sub-project builds the system those looks are made with: the
data, the place in the renderer, the authoring in the level editor, and
the tests. Sub-project 5 then adds specific effects on top of it, tuned in
the editor's preview.

What the user wants to see: dynamic lights, flickering flames, glowing
weapons, a sun outside a building lighting the room through its windows,
and lava lighting the cave above it.

## Goals

1. **Darkness:** a level sets its ambient light.
2. **Light from glowing pixels:** any level pixel can glow. Glowing pixels
   show at their own brightness whatever the darkness, and they light the
   world around them with an intensity and a reach the author sets, up to
   unlimited (a sun is a glowing sky).
3. **Placed lights:** light objects in the level, where no art should glow.
4. **Shadows:** rock and dirt block light; digging lets it through, as the
   terrain changes during play.
5. **Animated light:** steady, pulsing, flickering (a fireplace) and
   hand-keyed curves, on any light and on groups of glowing pixels.
6. **Weapon looks:** a mod's projectile can glow, carry a light, and flash
   when it ends; a weapon can light its muzzle flash.
7. **Engine lights:** explosions and muzzle flashes light dark levels even
   with old mods.
8. **Colour grading** per level.
9. **Authoring and preview** in the level editor, and agent operations in
   `olr-cli`.
10. **Cheap enough for an office laptop:** smooth at 60 Hz on an integrated
    GPU at 1920 × 1080, with no quality setting.

## Non-goals (this sub-project)

- Darkness as gameplay. Darkness is purely aesthetic: it hides nothing on
  purpose. Bots, names over other worms, damage numbers and minimap dots are
  unchanged, and every marker draws unlit (the user's ruling).
- Bloom and halos, heat haze and distortion, GPU particles, weather,
  parallax backgrounds, normal-mapped terrain, showpiece weapon effects:
  sub-project 5.
- Moving or animated backgrounds: a future milestone. A pulsing glow group
  stands in for them now.
- Colour lookup tables (LUT images) for grading: sub-project 5.
- Beam and laser-sight looks owned by a mod: sub-project 5 (beams draw as
  glowing, below).
- The mod editor (sub-project 4): mod looks are written by hand or by
  agents in this sub-project.

## Anti-goals

- **Changing the simulation.** The oracle traces and the match goldens stay
  byte-identical; `olr-sim` never sees look data.
- **Changing old content.** A level or mod without look data draws exactly
  as today: the four render goldens stay byte-identical.
- **Per-player graphics settings that change the picture's information.**
  No "low quality" that drops shadows or darkness.
- **Leaving the pixel grid.** No light, glow or grade breaks the art-scale
  texel grid: every art texel shows one flat colour.
- **Inference.** Nothing guesses lights for content that has none.

## The decisions the user made

1. **Scope:** the groundwork builds darkness, lights, shadows, glow, light
   animation and colour grading end to end; the rest waits for
   sub-project 5 (Goals, Non-goals).
2. **Target hardware:** an office laptop at smooth FPS where feasible; the
   development machine's RTX is not the measure.
3. **Grain:** light and shadow are computed per world pixel, and each art
   texel takes the light interpolated at its centre. Shadow edges are a
   world pixel coarse; to be re-evaluated after seeing it, with smoothing
   (a softness setting now, 2 × 2 shadow sampling as the planned fix).
4. **Old OLX levels look exactly as today.** Light is capped at full, so
   engine lights change nothing on a level without darkness; a level may opt
   in to over-bright light.
5. **Darkness is aesthetic only** (Non-goals). The name above the player's
   own worm is removed (OLX draws none); the weapon name while switching and
   the reload bar stay.
6. **Glowing pixels light the world**, with a settable intensity and reach,
   up to unlimited, so a sun is a glowing sky element.
7. **One light shape everywhere** (colour, intensity, reach, animation), and
   the animation kinds Steady, Pulse, Flicker, Keys, timed by the match
   clock. Glow animates by groups.
8. **2D global illumination** carries the light.
9. **Editor authoring:** glow properties per element, a glow brush, light
   objects, a Look panel, and preview toggles. Adjustments follow after
   seeing and playing it.
10. **No popping:** light from far away never appears as the camera
    approaches; rethink only if performance degrades too much.
11. **Solids show their art** (Stop 1, 2026-10-18): only rock faces that
    saw a light were lit, so rock read black. Light soaks a few pixels into
    solids, and a level sets the least light a solid shows (§Light in
    solids). Some solids may be **clear**: light passes them as air while
    worms and projectiles still collide (§Clear solids).

## The model

### The picture

Per art texel, in linear colour (the art is stored in sRGB bytes, as
today):

```
light  = min(ambient + gathered, max_light)          (per world pixel, interpolated)
shown  = albedo × max(light, self_light)
final  = mix(grade(shown), albedo, marker)           then back to sRGB bytes
```

- **`albedo`** is today's world pass: the terrain and every sprite, in
  OLX's order, with OLX's blending.
- **`ambient`** is the level's (a colour and an intensity), the same
  everywhere: deep caves get it too.
- **`gathered`** is the light that reaches this world pixel from every
  light source, through the global illumination below.
- **`max_light`** is 100 % by default; a level may raise it to 200 %
  (over-bright: flashes brighten a lit level).
- **`self_light`** (0-100 %) is how bright a texel shows on its own,
  whatever the darkness:
  - a glowing terrain pixel's glow strength × its group's animation;
  - a glowing sprite's glow strength (a mod's projectile, the engine's
    explosions and muzzle flashes, beams);
  - the engine's floor for play objects (worms, projectiles, bonuses), so a
    dark level never hides play (decision 5): a call, C6.
- **`marker`** (0 or 1): HUD-like world items draw exactly as their art,
  unlit and ungraded: the crosshair, the health bars, the laser sight.
  egui's names, popups and reload bar are drawn after the world, as today.
- A level with full ambient, no glow, no grade and no lights shows its
  albedo unchanged: `albedo × max(1, s) = albedo`. Old content goes further:
  with no look at all, the lighting passes do not run (§Pipeline).

### Light in solids

A solid pixel (rock or dirt that is not clear) gets no light of its own
past its face, so:

```
light(solid p) = max(soaked(p), solid_floor)          before softness, steps and cap
soaked(p)      = max over q within soak of  lit(q) × fade(|p - q|, soak + 1)
```

- **`lit(q)`** is `ambient + gathered` at q (air or solid); `fade` is the
  lights' `(1 - (d/r)²)²`. The maximum is taken in two separable passes
  (across, then down), so the reach is a soft square, not a disc: a call.
- **`light_soak`** (world pixels, 0-12) is how deep light soaks into solids.
  A wall thinner than twice the soak shows light from both sides, dimmed.
- **`solid_floor`** (a colour and an intensity, as the ambient) is the
  least light an opaque solid pixel shows: deep rock still shows its art.
  Blood and gibs (in flight, and the droppers) never show below it either,
  so they do not turn black in dark air (the user's playtest, 2026-10-19;
  `Shade::solid_floor`); thrown dirt and other debris stay lit as air.
- Air and clear pixels are not changed by either.
- **Defaults** (the user's verdict at Stop 1b, 2026-10-18): soak 8, floor
  white at 30 %. Provisional: they are expected to move after playtests.

### Light sources: one shape

Every light source has the same fields:

| Field | Meaning |
|---|---|
| `colour` | RGB |
| `intensity` | percent, 0-1000 |
| `reach` | `Pixels(n)`, world pixels (1-4096), or `Unlimited` |
| `animation` | `Steady` (default), `Pulse`, `Flicker` or `Keys` |
| `phase_ms` | optional; by default derived from the source's identity |

- **Reach:** light from a source fades to nothing at its reach
  (`(1 - (d/reach)²)²`); `Unlimited` has no fade (§Light transport says
  what intensity means for each kind of source).
- **The sources:**
  - **Glow groups** (a level's glowing pixels, below): a group's light
    comes from each of its pixels, coloured by the pixel's art or the
    group's tint.
  - **Light objects:** `LevelObject::Light` in `level.ron`, a light at a
    world position. Its `radius` (1-16 world pixels, default 2) is the
    source's size: full intensity within it, and its shadow rays start at
    its edge, so a lamp touching a wall still lights the room. It draws
    nothing itself.
  - **Projectile lights** (a mod's), following the drawn (interpolated)
    projectile.
  - **End flashes** (a mod's), where a projectile ended.
  - **Muzzle lights** (a weapon's, else the engine's) and **explosion
    lights** (the engine's).

### Animation

```ron
animation: Pulse(period_ms: 2000, low: 60, high: 100, shape: Sine),   // Sine | Triangle | Square
animation: Flicker(speed: 8, depth: 25),         // value noise, changes per second, percent
animation: Keys(keys: [(0, 100), (150, 40), (400, 0)], looped: false),
animation: Keys(keys: [(0, 100, "#ffa03c"), ...], looped: true),   // colour too
```

- The animation scales intensity (and, for `Keys` with colours, sets the
  colour). Values are percent; times are milliseconds.
- **The clock** is the match's: `tick × 10 ms + alpha × 10 ms` (alpha the
  frame's interpolation), so every player sees the same value at the same
  moment and replays repeat it. Never the wall clock.
- **A source's own clock** for a mod's lights: a projectile light runs on
  the projectile's age, an end flash and a muzzle light on the time since
  they started. A non-looping flash ends with its last key.
- **Phase:** `phase_ms` shifts the clock; by default it is a hash of the
  source's identity (a light object's index, a group's id, a projectile's
  serial), so ten torches never pulse together.
- **Flicker** is deterministic value noise from a hash of (identity, time
  bucket), smoothly interpolated: the same at the same time on every
  machine.
- Evaluation is plain `f32` arithmetic on the CPU, once per source per
  frame; the GPU gets the results.

### Glow (glowing pixels)

Glow is a channel of the level, as the composition spec §1 reserved:
topmost wins, background elements are the backdrop, and digging reverts a
pixel to the backdrop's glow.

- **Per art pixel:** a glow strength, 0-255 (0 none).
- **Per world pixel:** a glow group index into the level's `glow_groups`.
- **A glow group** (`level.ron`): `id`, `colour` (`Own`: each pixel lights
  in its art colour, or a tint `Rgb(r, g, b)`), `intensity`, `reach`,
  `animation`, `phase_ms`. At most 255 groups.
- **What a glowing pixel does:**
  - it shows at self-light `strength × animation` (its own art colour,
    whatever the group's tint);
  - it emits light into the world: its colour (art or tint) × strength ×
    the group's intensity × animation, with the group's reach.
- **Play:** a carved pixel takes its backdrop's glow (strength and group);
  dirt placed by the simulation has none; stains keep the pixel's glow.

### Clear solids

A clear pixel is a solid (rock or dirt) that light treats as open air: it
stops no lamp ray and no glow light, it is lit as air is, and neither soak
nor floor touches it. The simulation still sees its material: worms and
projectiles collide as before (glass, ice, force fields).

- **Per world pixel**, from the level as loaded: a pixel is clear while its
  level flag is set and its cell is solid. Carving makes it air; dirt the
  simulation places on a flagged cell is clear too (a call: the flag is
  static, so rollbacks and rebuilds need nothing).
- **Authoring:** an element's `clear` (composition), baked into
  `look/clear.png`.

## Light transport

Light never pops in as the camera moves (the user's ruling): what a world
pixel receives does not depend on where the camera is. Two paths carry it.

### Glow light: 2D global illumination over the whole level

Glowing pixels (glow groups: a sky, lava, neon) light the world through 2D
global illumination with radiance cascades: their light goes into an
emission grid, rock and dirt block it, and each cell gathers what reaches
it from every direction. The cost depends on the grid's size, not on how
many pixels glow.

- **The grid covers the whole level, fixed to the world.** Its cell is the
  smallest power-of-two number of world pixels `s` (1, 2, 4, 8) for which
  the level fits `GI_CELL_BUDGET` cells (about 256 Ki; the plan measures
  it). OLX-sized levels get `s` 1 or 2 (Liero Factory 1, Castle Strike 2);
  a 4096 × 4096 level gets 8. A level's `s` is fixed when it loads.
- **Occluders:** a cell's opacity is the share of rock and dirt among its
  world pixels (at `s` 1 a world pixel blocks or passes). The solid map is
  a whole-level texture at world resolution, updated by the same dirty
  rectangles as the colour terrain.
- **Emitters:**
  - a glowing world pixel emits its centre art texel's colour (or its
    group's tint) × strength × group intensity × animation, with the
    group's reach, averaged into its cell;
  - **intensity is the glowing surface's brightness:** a wide lava lake
    lights more than a few bulb pixels;
  - **a glowing cell stops a gathering ray** and gives its light, so a sky's
    brightness does not grow with its depth;
  - **reach:** a ray applies the fade for the distance it travelled when it
    meets an emitting cell; `Unlimited` has no fade.
- **Computed every frame** (glow animates), with no temporal accumulation:
  the light follows the state exactly, and the same state gives the same
  light. Skipping a recompute when nothing it reads changed (no terrain
  edit, no group value changed) is allowed: the result is the same.
- **Big levels trade grain for cost:** their glow light (not placed lights)
  is as coarse as their cell, smoothed by interpolation.

### Placed lights: analytic, with shadow maps

Light objects, projectile lights, end flashes, muzzle and explosion lights
are evaluated directly, so their numbers mean what the editor shows:

- **`intensity` is the light at the source;** it fades to nothing at `reach`
  (`(1 - (d/reach)²)²`), whatever the source's `radius`. `Unlimited` reach
  has no fade.
- **Shadows:** each light gets a 1D shadow map (the distance to the first
  rock or dirt along each of its rays, at world-pixel steps, out to its
  reach or the level's edge), then each world pixel is lit when it is
  nearer than that distance. Glowing background never blocks them.
- **Every light whose reach touches the view is evaluated,** wherever its
  source is: no popping. At most `MAX_LIGHTS` (256) per frame, chosen in a
  deterministic order (level lights, then the mod's, then the engine's,
  each by identity).
- **The planned fix for coarse shadows** (decision 3): the shadow maps
  sampled 2 × 2 per world pixel. Not built until the look calls for it.

### Shadow edges (after the playtest, 2026-10-19)

- A level's `shadows` (`Hard`, `Smooth`, `Soft`; default `Soft`, the
  user's choice from a comparison): `Hard` cuts at whole world pixels;
  `Smooth` tests four points a world pixel; `Soft` finds the occluders
  among the shadow-map rays a penumbra could reach and lights the share
  of rays over a penumbra `radius × (d - s) / s` wide (`s` the occluder's
  distance, `d` the point's): sharp near the occluder, softer farther, and
  wider for a bigger light. Glow light is soft already.

### After both

- `light = ambient + glow light + placed lights`, per world pixel; then, in
  solids, the soak and the solid floor (§Light in solids).
- The level's `softness` (0-4 world pixels) blurs it, `light_steps` (0 =
  off, 2-32) bands it, and `max_light` caps it.
- Each art texel takes the light interpolated at its centre (decision 3).

## Pipeline (`olr-render`)

1. **World pass**, as today, with a second target: `albedo` (today's
   `Rgba8Unorm` world target, unchanged) and `RG8` (self-light, marker),
   written by every item with the same order and alpha. The terrain
   writes self-light from the glow textures; each sprite instance carries
   its self-light and marker.
2. **Glow light:** the emission grid and the radiance cascades over the
   whole level's cells, into a glow-light grid (`RGBA16Float`).
3. **Placed lights:** their shadow maps, then their light per world pixel of
   the view (plus `MARGIN`), added to the glow light sampled there.
4. **Light finish:** ambient, softness, steps, cap, into the view's light
   grid at world resolution.
5. **Composite** over the world target's texels: the light sampled bilinearly
   at each texel's centre in world coordinates, the model above, the grade,
   into a lit world target (`Rgba8Unorm`).
6. **Upscale**, as today, from the lit target.

- **Passes 2-5 run only when the match has a look:** the level's `look`, or
  any look in the mod. Otherwise pass 1 is today's and pass 6 reads it:
  old content draws byte-identically, at today's cost.
- **Inputs per frame:** a `LookFrame` (ambient, grade, settings, the
  evaluated group values, the placed lights touching the view with
  positions, colours, intensities, reaches, radii), built on the CPU by `olr-present`
  from the world, the scene, the effects, the level and the mod. It
  allocates nothing per frame once warm.
- **Textures per level:** glow strength (art resolution, `R8`), glow group
  and solid (world resolution, `R8` each), uploaded whole for a new level
  and by dirty rectangles after edits, as the colour terrain is.
- **Debug views** (editor and tests): *Full bright* (passes 2-5 skipped),
  *Light only* (albedo replaced by white).

## Data

### Level packs

```
level.ron         + look, + Light objects
look/glow.png     art resolution, 8-bit grey: the glow strength of the composite (front over solid, back elsewhere)
look/glow-back.png  art resolution, 8-bit grey: the backdrop's glow strength (what digging reveals)
look/groups.png   world resolution, 8-bit grey: the composite's glow group per world pixel (0 none, n the look's group n - 1)
look/groups-back.png  world resolution, 8-bit grey: the backdrop's group
look/clear.png    world resolution, 8-bit grey: 0 opaque, any other value clear (optional, on its own)
```

```ron
look: Some((
    ambient: (colour: "#ffffff", intensity: 30),
    max_light: 100,                    // 100-200
    light_steps: 0,                    // 0 off, 2-32
    softness: 0,                       // world pixels, 0-4
    light_soak: 8,                     // world pixels, 0-12
    solid_floor: (colour: "#ffffff", intensity: 30),  // the least light of an opaque solid
    grade: (exposure: 0, contrast: 100, saturation: 100, tint: "#ffffff"),   // exposure in 1/100 stop, -200..200
    glow_groups: [
        (id: "sky", colour: Tint("#fff0d2"), intensity: 100, reach: Unlimited, animation: Steady),
        (id: "lava", colour: Own, intensity: 150, reach: Pixels(96), animation: Flicker(speed: 6, depth: 20)),
    ],
)),
objects: [
    Spawn(at: (40, 300)),
    Light(at: (512, 140), colour: "#ffbe78", intensity: 120, reach: Pixels(160),
          radius: 2, animation: Flicker(speed: 8, depth: 25)),
],
```

- **`look/clear.png`** stands alone: present or not, whatever the glow
  images. A size mismatch is a `pack check` error and a load warning that
  drops it.
- **All four glow images or none.** Without them a level has no glow. Sizes
  must match the level (art or world resolution); a mismatch or a group past
  the list is a `pack check` error and a load warning that drops the glow;
  a look value out of range is a `pack check` error and a load warning that
  drops the look (its lights and glow too); a `level.ron` that does not
  parse (an unknown field) fails the level, as any authoring file does; a
  bad look in a mod fails the mod, as any bad mod field does. Never a panic
  (repo rule 6).
- **`look: None`** (absent) is a level without a look: the passes do not
  run.
- **Hashed:** `level.ron` and `look/` are in the content hash already (only
  `editor/`, hidden files, `pack.ron` and `thumb.png` are left out), so
  content download, replays and the lobby's checks carry them unchanged.
- **Integers only** (percent, milliseconds, world pixels), so canonical
  RON round-trips byte-identically. Colours are the format's hex strings
  (`"#rrggbb"`), as every other colour in a pack (plan 15a, Task 2).

### Composition (`editor/composition.ron`, not hashed)

- Image and drawn elements gain `glow: Option<(strength: 0-100, group: "<id>")>`.
- A painted glow mask, `editor/glow/<id>.png` (art resolution of the
  element's box, 8-bit grey), multiplies the strength per pixel; without it
  the element glows uniformly where it covers.
- Raster elements carry no glow (split one into layers to make it glow).
- Image and drawn elements gain `clear: bool` (default false): their solid
  pixels let light through. The bake writes `look/clear.png` where the
  element that sets a cell's material is clear and the cell is solid, and
  none when nothing is clear.
- **The bake** writes the four look images from the elements by the
  channel rules, through the same region bake (a glow edit rebakes its
  rectangle), and writes none when nothing glows. `pack check`'s stale-bake
  check covers them.
- Glow groups, light objects and the rest of `look` are in `level.ron`
  (the runtime reads them); the editor writes them there.

### Mod packs

```ron
// projectiles/<id>.ron
look: Some((
    glow: Some(80),                                   // self-light of its sprite or pixels, percent
    light: Some((colour: "#ff8c28", intensity: 80, reach: Pixels(48), animation: Flicker(speed: 10, depth: 15))),
    end_flash: Some((colour: "#ffc878", intensity: 200, reach: Pixels(96),
                     animation: Keys(keys: [(0, 100), (250, 0)], looped: false))),
)),
// weapons/<id>.ron
look: Some((muzzle_light: Some((colour: "#ffdca0", intensity: 120, reach: Pixels(64),
                                animation: Keys(keys: [(0, 100), (60, 0)], looped: false))))),
```

- **End flash:** shown where a projectile was last drawn when it ends (the
  scene sees its slot's serial go), so it needs no simulation event.
- **Old mods** have no `look`: their projectiles get only the engine's floor.
- Optional fields, skipped when absent: `lift_mod`, the canonical text and
  the import-equivalence tests are unchanged.

### The engine

- **Code, not data:** the engine's look lives in a crate's constants
  (explosion light by explosion size, the muzzle light, the floors, the
  self-light of explosions, muzzle flashes, sparkles and spawn effects).
  `olx-core` stays exactly the importer's output, which `cargo xtask
  check` verifies against a fresh import.
- It acts only where the passes run (a level or mod with a look), so old
  content is unchanged.

### Format version

No bump: every addition is optional and absent from old packs, so
`content/packs` stays byte-identical (14d's practice). An older build
refuses a pack using the new fields with `deny_unknown_fields`, which the
pack-format rules accept.

## Fair play and determinism

- What a frame shows depends only on the world, the scene's interpolation,
  the level's, mod's and engine's look, and the match clock. No wall clock,
  no per-player setting (the upscale filter and window size stay
  per-player: they change no information).
- No temporal state: the same state draws the same picture.
- The camera's position never changes a world pixel's light: the glow
  light covers the whole level, and every placed light reaching the view
  is evaluated wherever it is.
- GPUs may differ in float rounding by a step of a byte; that changes no
  information. Goldens are drawn on the software rasteriser (lavapipe), as
  today.
- Animation values are computed on the CPU from integers and the tick, with
  `f32` the same on every platform for these operations.

## Authoring

### The level editor

- **Element properties, a Glow section:** strength (0 = none) and group.
  The section also edits the chosen group's colour (Own or a tint),
  intensity, reach and animation, so for most elements it reads as
  per-element. A new glow makes a group named after the element; groups can
  be shared, renamed and removed (an element whose group goes loses its
  glow, with a confirmation).
- **The Glow brush:** paints and erases an element's glow mask (round or
  square, sized in world pixels, as the eraser). Undo as any pixel edit.
- **Light objects:** a Light tool places them (like spawns), listed under
  Objects; properties: colour, intensity, reach, radius, animation, phase.
  Drawn on the canvas as a marker with a reach circle when selected.
- **The Look panel** (level settings): ambient, max light, light steps,
  softness, the grade, and the glow groups' list.
- **Preview:**
  - the canvas draws through the real renderer with the look;
  - a view toggle: *Lit*, *Full bright* (to paint in a dark level), *Light
    only*;
  - a clock: play, pause, and scrub, driving the animations (the canvas's
    own clock; Play uses the match's);
  - every look change shows on the next frame: settings and groups are
    uniforms, glow edits upload their rectangle.
- **Play** runs the real thing.

### Agent operations (`olr-cli`)

```
olr-cli level look      <pack> [--ambient RRGGBB:PCT] [--max-light PCT] [--steps N] [--softness N]
                               [--exposure N] [--contrast PCT] [--saturation PCT] [--tint RRGGBB] [--clear]
olr-cli level group     add|set|remove <pack> <id> [--colour own|RRGGBB] [--intensity PCT] [--reach N|unlimited] [--animation SPEC]
olr-cli level light     add|set|remove <pack> [<index>] --at X,Y [...the light's fields]
olr-cli level element set <pack> <id> --glow PCT --glow-group G
olr-cli level element glow <pack> <id> (--rect X,Y,W,H | --circle X,Y,R) --strength PCT   (paints the mask; 0 erases)
olr-cli look-preview    <pack> --at X,Y [--time MS] [--view full|light] [--size WxH] --out f.png
```

`look-preview` draws a level with its look and no match, centred on a world
position at a clock time: the fast feedback loop for agents and for the
goldens. `--animation SPEC` is the RON of an animation.

## Performance

Budgets (the plan measures them, with the user's consent for benchmark runs):

| Where | Budget |
|---|---|
| All look passes, RTX 5070 Ti, 2560 × 1440, art scale 2, Castle Strike-sized level, 64 lights | ≤ 0.25 ms GPU |
| The same at 3840 × 2160, art scale 8 | ≤ 0.5 ms GPU |
| An integrated GPU, 1920 × 1080, art scale 2 | ≤ 4 ms GPU, the frame holding 60 Hz |
| Building the `LookFrame` (CPU) | ≤ 0.05 ms p99 |
| Memory | + 3 bytes per art pixel (CPU glow), + 1 per art pixel and 2 per world pixel (GPU) |

- **Why it fits:** the glow light runs on at most `GI_CELL_BUDGET` cells
  whatever the level, screen or art scale; placed lights cost their shadow
  maps (rays × reach) and one pass over the view's world pixels (about
  430 × 195); only the composite pass touches every art texel, one pass
  like the probe's (0.1 ms at 4K on the RTX).
- **If whole-level glow light costs too much** (the user's ruling allows
  rethinking then): a smaller `GI_CELL_BUDGET` (coarser cells), or
  recomputing only on change. Popping light (a region around the view)
  is the last resort, and the user's call.
- **The integrated GPU:** measured on the development machine's integrated
  GPU if it has one (`olr-cli gpu-bench --list-adapters`), else at the
  playtest on a real laptop.
- **The levers, if a budget is missed,** are global and the same for every
  player: `GI_CELL_BUDGET`, the cascade count, the shadow maps' ray count,
  `MAX_LIGHTS`. Never a per-player setting.
- **Map size is negotiable** (the user, 2026-10-18): 4096 × 4096 pushed
  the limits; a 2048 × 2048 maximum, or smaller maps on low-powered
  laptops, is acceptable if the measurements call for it. No earlier limit
  is set in stone; the plan reports the trade-off rather than degrading the
  look.
- **Art scale 8** stays refused by `MAX_COLOR_MAP_BYTES` as today; glow adds
  a quarter to the colour terrain's memory.

## Testing

Fast, seeded, headless where possible (the user's agentic-loops ruling):

- **Formats (`olr-format`):** canonical round trips of every look field;
  limits; hostile files (wrong sizes, a group past the list, huge values)
  give errors, never panics; every bundled pack reads and writes
  byte-identically.
- **Animation and light lists (pure CPU):** tables of `Pulse`, `Flicker`,
  `Keys` values at given times and phases; flicker identical across runs;
  the light list's order, cap and identities; the engine's floors.
- **Glow terrain (`olr-terrain`):** carving reverts to the backdrop's glow,
  placed dirt has none, stains keep it, dirty rectangles; a proptest against
  a rebuild from the material.
- **The bake (`olr-edit`):** glow follows the channel rules; region bakes
  equal full bakes; a level without glow writes no look files; the existing
  fixtures bake byte-identically.
- **The renderer (GPU tier, readbacks of the light grid):**
  - a wall blocks light: behind it the light is the ambient;
  - reach: a placed light's light within its reach, none past it;
    `Unlimited` reaches the level's far side;
  - **no popping:** the camera moved anywhere gives every shared world
    pixel the same light (a sky far above the view still lights the room);
  - a placed light off-screen whose reach touches the view lights it;
  - a glowing cell stops a ray: a sky twice as deep lights the same;
  - the same state drawn twice gives the same bytes;
  - a group's animation scales its light and self-light;
  - **identity:** a level with full ambient and nothing else draws the same
    bytes as with no look; the four existing goldens stay byte-identical,
    unblessed.
- **New render goldens** (`fixtures/render/look-*.png`, blessed with a
  reason) from a look test level in `fixtures/look/` (a glowing sky over a
  building with windows, flickering lava, lamps, a dark ambient, a grade)
  and a look test mod (a glowing projectile with a light and an end flash),
  through `look-preview` and a short scripted match.
- **The editor (`egui_kittest`):** set a glow, paint a glow mask, place a
  light, edit the Look panel, toggle the views, scrub the clock, save, check
  the files.
- **End to end:** the editor's autoplay script adds a glow and a light, plays
  and stops.
- **Benchmarks:** a look scene in `olr-cli gpu-bench` and `cargo xtask
  bench`, run only with the user's consent.
- **Unchanged:** the oracle traces, the match goldens, `content/packs`.

## Calls made in writing this (not the user's)

Marked here so they can be checked:

- **C1. Self-light, not added emission:** a glowing texel shows
  `albedo × max(light, self_light)`, so a fully glowing pixel looks exactly
  its art colour and never over-brightens a lit level.
- **C2. Reach and colour live on the glow group,** so the per-pixel data
  stays one byte of strength and one of group; the editor makes a group per
  glowing element by default, which keeps it per-element in feel.
- **C3. Glow shows its own art colour;** a group's tint colours only the
  light it casts (a blue sky can cast warm sunlight).
- **C4. Light is computed in linear colour,** the grade after it; the art
  stays sRGB bytes.
- **C5. Markers** (crosshair, health bars, laser sight) draw unlit and
  ungraded; beams, explosions, muzzle flashes, sparkles and spawn effects
  glow fully; blood, gibs, dirt, smoke, rope, bonuses and worms are lit.
- **C6. An engine floor for play objects:** worms, projectiles and bonuses
  have a minimum self-light (default 50 %), so a dark level never hides
  play (decision 5). Tuned in sub-project 5 if it looks wrong.
- **C7. The engine's look is code** in a crate, not data in `olx-core`.
- **C8. End flashes come from the scene** (a projectile's slot serial
  ending), not a new simulation event.
- **C9. Whole-level glow light at a coarser cell on big levels** (the
  user's no-popping ruling): cells of 1-8 world pixels by
  `GI_CELL_BUDGET`, so cost stays bounded and large levels' glow light is
  coarser.
- **C10. Placed lights are analytic with 1D shadow maps,** not part of the
  global illumination: `intensity` and `reach` mean what they say, and small
  lights never ring. Glowing pixels' `intensity` is a surface brightness
  (physical spread), and a glowing cell stops a gathering ray.
- **C16. The user judges the looks at two stops:** after the renderer batch,
  a contact sheet of the look test level (lit, light only, animation frames,
  world-pixel against 2 × 2 shadows, the engine's floors and lights) waits
  for the user's verdict; after the editor batch, a playtest build (plan
  15a R10: a playtest needs the editor to make lit levels).
- **C17. Soak is separable** (a maximum across, then down): a soft square
  reach, two cheap passes; the light grid's padding grows to 16 world
  pixels (soak 12 plus softness 4) so the soaked light never depends on
  the camera.
- **C18. The clear flag is static** (the level as loaded, solid cells
  only), so play edits, rollbacks and rebuilds need no clear bookkeeping.
- **C11. No format version bump;** the fields are optional.
- **C12. Integers** for every look value in the files.
- **C13. A muzzle light per weapon** (the mod's) overrides the engine's.
- **C14. The glow and group files** are four separate grey PNGs; the
  backdrop's copies make digging reveal the right glow.
- **C15. The runtime types and animation evaluation** live in a new crate,
  `olr-look` (layer 1, `no_std`, wasm), which `olr-format` compiles into and
  `olr-content`, `olr-present`, `olr-render` and the editor read.
- The CLI commands' names and flags, and the file names in `look/`.

## Left to the builder

- The radiance cascades' parameters (cascade count, rays, interval lengths),
  within the budget, and the GPU formats of the intermediate grids.
- The shadow maps' ray count and layout.
- The `LookFrame`'s layout and how `olr-present` collects the sources.
- The editor's widgets' layout, within §Authoring.
- How the look goldens' match frame is scripted.

## Out of scope

- Everything in Non-goals.
- Glow on raster elements.
- A visibility model for bots.

## Plans

One plan, **15a**, in batches:
- formats, `olr-look`, glow terrain, the bake, loading, the CLI, and the own
  name removed;
- the renderer: the second target, the glow light, the placed lights, the
  composite and grade, the goldens and the identity tests; then the user's
  contact-sheet stop;
- wiring into play: `olr-present`'s `LookFrame`, the mod's and engine's
  lights, `look-preview`;
- the editor: properties, the brush, lights, the Look panel, the preview;
- measurements (with consent) and docs.
