# Pack Formats: Design (0.2, sub-project 1)

- **Status:** design agreed with the user on 2026-10-16, section by section.
- **Milestone:** 0.2 (`docs/superpowers/specs/2026-10-16-milestone-0.2-scope.md`).
  Formats come first: the look files, the editors and the Reloaded packs all
  read and write them.
- **Goals, in order:**
  1. **Editor round trips.** Editors load and save files losslessly, with
     stable output.
  2. **Room beyond OLX.** Look data, finer art, level objects, and new
     weapon behaviours after 0.2.
  3. **LLM agents can read and write packs.**

  Hand-editing and sharing were not goals; `.olpk` archives already cover
  sharing.

## 1. Architecture and pipeline

```
 OLX files ──import (offline)──▶ authoring files ──compile──▶ runtime data ──▶ olr-sim / olr-content
                                 (in packs, hashed)     ▲     (olr-data, as today)
 editors, agents, olr-cli ──────read / write────────────┘
```

**The authoring format is the pack format.** Loading compiles it into
the runtime structs `olr-sim` already uses (`olr-data`'s `ModDef`,
`LevelDef`, `SkinDef`, `ThemeDef`). Neither `olr-sim` nor those runtime
structs change in 0.2, so the oracle is protected by construction.

### The new crate `olr-format`

Layer 2, beside `olr-pack`, registered in `xtask/layers.ron`. It holds:

- **The authoring types** for mods, levels, skins, themes and the core pack.
- **`compile`:** authoring data to `olr-data` runtime structs.
  - It is pure and deterministic.
  - It never panics.
  - An invalid or unsupported input is an error naming the file, the line
    and column, and the field path.
- **The canonical writer:** one text form per value.
  - Fields are written in a fixed order, and fields equal to their default
    are left out.
  - Numbers are formatted stably (Rust's shortest round-trip form).
  - **Loading a canonical file and saving it gives back the same bytes.**

### What the other crates do

- **`olr-pack`** keeps the pack layout, manifests, content hashing and
  `.olpk` archives, and reads and writes pack files through `olr-format`.
- **`olr-content`** loads packs through `olr-pack` and `olr-format::compile`,
  then its runtime path continues unchanged.

### Rules

- **Syntax: RON**, as across the project.
  - Canonical files hold no comments, because a save would drop them.
  - Notes for people or agents go in optional `notes:` fields, which
    survive round trips.
- **Units are named, never rescaled.** A number keeps OLX's value and
  scale, and the generated schema states its unit. Converting to seconds
  or degrees would add float conversions that could break the oracle's
  bit-exactness. A friendlier unit can come when the runtime has its own.
- **The look is part of the authoring format.**
  - Reserved `look` sections and slots are compiled into render-side data
    that `olr-sim` never sees.
  - They are hashed with the rest of the pack.
  - Their contents are designed in the graphics sub-project; the 0.2
    format version includes them.
- **Versioning:** `PACK_FORMAT_VERSION` goes from 4 to 5.
  - The importers write the new format, and `content/packs` is re-imported.
  - A pack of another version is refused with "re-import the pack".
  - There are no migrations (compatibility breaks are fine at this stage).
  - Content hashes change, so any replay, fixture or match golden that
    pins a hash is re-blessed once, with that reason. Amended by plan 13c
    (D15): nothing committed pins a pack content hash and no replay is
    committed, so nothing needed a re-bless. Old replays and cached packs
    are refused or dropped, because the format version and the protocol
    version both changed.

## 2. The mod format

### Layout

```
mod.ron              name, worm physics, rope, weapon order
weapons/<id>.ron     one weapon with its whole projectile tree, nested
projectiles/<id>.ron only projectiles shared by several weapons, referenced by id
restrictions.ron     the weapon restriction presets (as today)
gfx/, sfx/           assets
```

### Projectiles react to events with lists of effects

- **Events:** `hit_terrain`, `hit_worm`, `timer` (delay and variation) and
  `trail` (periodic, with its kind).
- **Effects:** every OLX action kind under a readable name: `Explode`,
  `Carve`, `Bounce`, `Dirt`, `GreenDirt`, `Injure`, `Halt` (OLX's
  `Disappear`, which does not remove the projectile), `Remove`,
  `GoThrough`, `Sound`, `Spawn(<name>)`, and OLX's speed, heading and
  radius actions.
- **LX56 is a restricted case.** An event's effect list compiles to LX56's
  single action plus its "spawn projectiles" flag, and to the projectile's
  one shared spawn. A list LX56 cannot express is a compile error ("not
  supported by this runtime yet"), never a silent change. New mechanics
  after 0.2 add events and effects to both the format and the runtime,
  with no new format overhaul.
- **Spawns.**
  - **Event spawns are named.** A projectile's `spawns:` map names each
    one (`"shrapnel"`), and `Spawn("shrapnel")` refers to it.
  - **A trail's spawn sits inside the trail:**
    `trail: Projectile(every_s: …, spawn: (…))`.
  - **What 0.2 compiles** is what LX56 has: one spawn shared by the events
    (`ProjectileDef::event_spawn`) and one for the trail
    (`TrailDef::spawn`). A named spawn that no event uses is allowed, since
    LX56 files can carry one.
  - **Defaults depend on the context.** LX56 fixes several spawn flags per
    context: a weapon's shot, an event spawn and a trail spawn. Each
    context has its own defaults, so imported files carry none of these
    flags (amended 2026-10-16 from the research,
    `docs/porting/olx-notes/mod-fields.md` §1.5).

### Example: OLX's bazooka (today 131 lines in 3 files; now one file)

```ron
(
    name: "Bazooka",
    recharge: 0.171,
    drain: 33.334,
    between_shots_s: 1.071,
    fire: Projectile(
        recoil: 15,
        sound: "sfx/bazooka.ogg",
        shot: (
            speed: 140,
            projectile: (
                gfx: Image(
                    file: "gfx/missile.png",
                    frames_follow_heading: true,
                    heading_frames: 18,
                ),
                gravity: 0,
                velocity_factor: 1.001,
                trail: Smoke,
                hit_terrain: [
                    Explode(
                        damage: 4,
                        shake: 2,
                        sound: "sfx/exp2.ogg",
                    ),
                    Spawn("spawn"),
                ],
                hit_worm: [
                    Explode(
                        damage: 27,
                    ),
                    Spawn("spawn"),
                ],
                spawns: {
                    "spawn": (
                        amount: 12,
                        speed: 105,
                        speed_var: 14.0,
                        spread: 360.0,
                        shared: "bazooka.spawn",
                    ),
                },
            ),
        ),
    ),
)
```

This is the canonical writer's output (plan 13a's snapshot).
`bazooka.spawn` is shared because nine places use it, so it lives in
`projectiles/bazooka.spawn.ron`. A spawn's parameters come before its
projectile.

The names follow plan
13a's final table (`docs/porting/olx-notes/mod-fields.md` §13).

### Rules

- **Defaults are OLX's own,** such as `bounce: 0.5` and the velocity
  factors at 1.0. A field equal to its default is never written, so
  imported content stays exact and short.
- **Readable names replace OLX's internal ones.** Each name describes what
  OLX's code does with the field, and the schema cites the OLX source. For
  example, `use_special11_vec_for_speed_var` gets a name that says what it
  changes.
- **Colours are hex strings:** `"#rrggbb"`, or `"#rrggbbaa"` with alpha.
- **Weapons:** `Projectile`, `Beam` and `Special(Jetpack)`, as in the
  runtime, under readable names.
- **Projectile identity.** `olr_sim::SimMod::compile` expands projectiles
  per occurrence, in OLX's load-order tree, and names each by its path
  from the weapon id (`bazooka`, `bazooka.spawn`). `ModDef::projectiles`'
  keys still matter, because `SimMod` keeps copies of the definitions that
  name their children by key.
  - **Keys are reproduced exactly.**
    - A shared projectile's key is its file id.
    - An inline projectile's key is derived from its place: the weapon id
      for a shot, then `<parent key>.spawn` or `<parent key>.trail`.
      These follow `olr-import-lgs`'s naming.
    - Where today's import used another name (it de-duplicates identical
      definitions), the projectile carries an explicit `id`.
  - **Inline or shared.** A projectile used once is written inline. One used
    twice or more (or never) is shared, in `projectiles/<id>.ron`.
  - **Cycles between shared projectiles are errors.** OLX's files cannot
    express one.
  - The import-equivalence test (§6) checks all of this, `SimMod` included.
- **`gfx` is simulation data, not only looks.** The simulation reads the
  pixel colour count, the rotation and the animation. The name `look`
  stays reserved for render-only data, which the graphics sub-project
  adds as optional slots on projectiles and effects.

### Research task: names and units

The spec phase settles each field's name and unit from OLX's code at
`d1ad953f`, for every field of `WeaponDef`, `ProjectileDef`, `ActionDef`,
`SpawnDef`, `TimerDef`, `TrailDef`, `BeamDef`, `WormParams` and
`RopeParams`. The result is a table: OLX name, new name, unit, default,
OLX source. It lives in the generated schema doc.

## 3. The level format

### Layout

```
level.ron      name, size, art scale, theme, objects, look
material.png   world pixels; indexed, with the canonical palette
front.png      RGBA at art scale: how rock and dirt look
back.png       RGBA at art scale: what shows where dirt is dug, and the open background
thumb.png      menu thumbnail (presentation only, outside the content hash as today)
look/          optional images for the look (glow mask, …): graphics sub-project
```

### `level.ron`

```ron
(
    name: "Liero Factory",
    size: (1200, 480),
    art_scale: 1,
    theme: "olx-theme-dirt",
    objects: [
        Spawn(at: (140, 312)),
        Spawn(at: (1050, 96), notes: "top right ledge"),
    ],
    look: ( /* graphics sub-project */ ),
)
```

### Rules

- **Coordinates** are world pixels, origin top-left, y down, at any art
  scale.
- **`material.png`:**
  - It has exactly the size `size`.
  - Its index is the material: 0 rock, 1 background, 2 dirt; the other
    indices are reserved, and in 0.2 such an index is a compile error.
  - Its palette is canonical (rock grey, background dark, dirt brown), so
    any pixel editor shows a readable map.
  - New materials (water, bouncy, …) later take reserved indices together
    with runtime support.
- **`front.png` and `back.png`** have exactly `size × art_scale`
  (`art_scale` 1..=8). A size mismatch is an error, never a panic.
- **Objects** are one typed list.
  - `Spawn` compiles to `LevelDef::spawns`. The 0.2 runtime does not read
    spawns yet: worms spawn with OLX's `FindSpot`, as OLX's `.lxl` levels
    carry no spawns (amended by plan 13b).
  - The graphics sub-project adds look objects (light sources, emitters).
    The editor may add `Region` (named rectangles or polygons; notes now,
    triggers later).
  - An object kind the runtime does not know is a compile error, never
    ignored, so a newer pack never plays wrong on an older build.
- **The theme** stays a separate pack the level references, by id. It
  decides how carving looks, and its stamp shapes, which the simulation
  uses. A Reloaded level that wants its own carving look ships its own
  theme pack.
- **OLX `.lxl` levels** (including their hi-res chunk) convert to this
  layout, with §5's asset conventions.
- **Large levels** use one PNG per layer for now. Tiling the art (the
  envelope report's art-scale-8 memory note) is a later optimisation and
  keeps the format's meaning.

## 4. Skins, themes and the core pack

### Skins

```
skin.ron   name, art_scale, frame_size, aim_frames, walk_frames
skin.png   RGBA grid: columns = aim angles, rows = walk steps; cells exactly frame_size, no spacing
tint.png   greyscale, same size: 0 = keep, 255 = full player colour (values between: partial tint)
left.png   optional: explicit left-facing frames; without it the right-facing frames are mirrored
```

```ron
(name: "afro", art_scale: 2, frame_size: (20, 18), aim_frames: 7, walk_frames: 3)
```

- **OLX skins are 2× art** (drawn at two art pixels per world pixel).
- **What the 0.2 runtime draws:** OLX's layout only (art scale 2, 20 × 18
  frames, 7 aim and 3 walk frames), full tint only, no `left.png`.
  Anything else is a compile error ("not supported by this runtime yet")
  until the graphics work extends the renderer (amended by plan 13b).
- **Finer skins** are `art_scale` and more frames. The renderer picks a
  frame from the continuous aim angle and the walk phase. Skins are
  presentation only: the worm's size in the simulation is independent of
  them.
- **Full tint is exact.** A tint of 255 reproduces OLX's `Colorize_Execute`
  (`src/client/CGameSkin.cpp:720-790`), checked pixel for pixel on the
  imported skins.
- **OLX's mirrored-frame quirk is dropped.** OLX draws a left-facing worm
  from the source columns one to the right
  (`src/client/CGameSkin.cpp:584-601`); 0.2 mirrors exactly. This is
  recorded as a deviation in `docs/porting/STATUS.md`, and the render
  goldens are re-blessed for it.
- **Skins are never downloaded** (the user's ruling, 2026-10-16). A player
  whose game lacks another player's skin, by id and content hash, sees
  that worm with the default skin.
  - This replaces plan 11's ruling N31: the host no longer fetches
    members' skins and serves them to the others.
  - Mods, levels and themes are still downloaded, as required content.
  - Odd or nearly invisible skins therefore need no validation policy:
    others see a skin only if they installed it themselves.

### Themes

```
theme.ron                                    name, image counts
holes/<n>.png + holes/<n>.mask.png           paint colours (RGBA) + meanings (indexed: untouched / carve / paint)
green_ball.png + green_ball.mask.png         rim colours (RGBA) + meanings (indexed: untouched / green / rim)
stones/<n>.png, misc/<n>.png                 RGBA with alpha
front_tile.png, back_tile.png                the tiles for placed dirt and for dug background
```

Amended by plan 13b:

- **Theme images are world pixels.** Stamps and tiles apply per world
  pixel, so they declare no art scale.
- **There is no dirt colour field in 0.2.** OLX derives the dirt
  particles' colour from `front_tile` and the first hole, and so does
  `olr-fx`. A field can come with the graphics work.
- **Paint colours that would change meaning are errors:** black or
  magenta in a hole, or exactly `#00ff00ff` or `#ff00ffff` in the green
  ball's rim.

The stamps keep their exact effect on carving and on dirt counts, which
the simulation uses (`olr-terrain`'s stamp, carve and dirt code).

### The core pack

- **What it holds:** the engine's assets, OLX's shared sprites
  (`explosion`, `smoke`, `giblets`, `muzzle`, `hook`, `crosshair`,
  `bonus`, …), the game sounds and the UI colours.
- **Conversion:** sprites get alpha and declared frame grids and art scale
  (the `explosion` strip is described in `core.ron` rather than guessed).
  OLX's `giblets.png` holds two grids at two scales. It is split into
  `giblets` (8 × 8 frames at 2×) and `giblet-stains` (4 × 4 frames at
  1×), which `compile` composes back (amended by plan 13b).
  `colours.cfg` values become hex.
- **Later art:** the graphics sub-project may add new engine art, for
  example as an `olr-core` pack beside the imported `olx-core`; the format
  allows it.

### One sprite description

Mod `gfx/`, core sprites and theme images share one description:

- a file;
- a frame grid (`frames: (columns, rows)`, or `angles: n` for rotation
  frames);
- an art scale;
- an animation rate where one applies.

The editors then have one sprite widget, and agents have one pattern to
learn.

## 5. Asset conventions and where conversion lives

**Principle:** meaning lives in explicit channels, masks or data, never in
magic colours. Standard tools (Aseprite, GIMP, Krita) show and edit every
asset correctly.

| OLX convention | After import |
|---|---|
| Magenta or `254,0,254` corner-rule colour key (`RgbaImage::olx_color_key`, `src/client/GfxPrimitives.cpp:116-150`) | A real alpha channel (RGBA PNG) |
| Skin `mask.png`: black keep, magenta transparent, other colours tint | Transparency in the skin's alpha; the tint in a greyscale `tint.png` |
| Theme hole stamps: magenta carve, black untouched, colour paints the edge | An indexed meaning mask plus an RGBA paint layer |
| `material.png` palette indices | Same data, canonical documented palette |
| Frame counts guessed from image sizes | Frame grid and count declared in data |
| OLX sprites implicitly at 2× art | Art scale declared on every image set |
| `colours.cfg` RGB triples | Hex strings |
| Colour space unstated | Images are sRGB (any change to blending belongs to the graphics sub-project) |
| Sounds | Ogg Vorbis as today; WAV accepted; nothing re-encoded lossily |
| PNG encoding | Canonical, through one encoder; the `png` crate is pinned exactly, because encoded bytes are content (plan 13b) |
| OLX file and folder names | Lowercase kebab-case ids, a fixed folder layout per pack kind (amended by plan 13c, Ruling 14: asset files keep OLX's lowercased names, such as `cluster_bits.png`; renaming them to kebab-case is deferred, since it changes no behaviour and would churn snapshots) |

**Audit task.** The spec phase checks the remaining OLX conventions in
the same way (the dirt tiles, the green dirt shades, `iDefaultColour`,
the bonus and health sprites). Any meaning that is not explicit gets an
explicit form. OLX's bitmap fonts and HUD images stay unimported.

### Conversion runs once, offline

- **Where:** legacy conversion lives only in `olr-cli import`,
  `cargo xtask fetch-olx-content` and the editors' "Import OLX…" command.
  Its output is ordinary native packs.
- **The game reads only the native format.**
  - **The game's Import screen is removed;** importing becomes an editor
    command.
  - **`olr-content` drops its dependencies** on `olr-import-lgs`,
    `olr-import-lxl` and `olr-import-gfx`. The `olr` binary and the release
    zips carry no legacy code.
  - `xtask/layers.ron` follows.
- **Nothing is converted at load.** `olr-scene` and `olr-fx` stop colour
  keying their sprites (`key_transparent`); loaders decode PNGs and use
  them as they are. The colour-key code lives on only in the importer.
- **Compiling is not converting.** Authoring files compile to runtime
  structs in memory at load, a structural mapping of already-native data.
  A cache can come later if large mods make it slow.

## 6. Validation, schema docs, agent operations and testing

### One validator

- **The command:** `olr-cli pack check <dir>` parses, compiles and
  validates a pack.
- **Shared code:** the loader and the editors' live error list use the
  same code. It never panics.
- **Errors** name the file and, for text that does not parse, the line
  and column. For a valid file with an invalid value, they give the field
  path (RON gives no positions after parsing):

  ```
  weapons/bazooka.ron:14:9: expected a string
  weapons/bazooka.ron: fire.shot.projectile: no spawn named "shrapnel2" (spawns: spawn)
  ```

- **Warnings:** unused files, unreferenced shared projectiles, objects
  outside the level.
- **Bounds:**
  - sizes, counts and nesting depth;
  - cycles between shared projectiles;
  - image sizes checked from the PNG header before decoding (as
    `olr-pack::image_io` does today);
  - every bound named as a constant.
- **Formatting:** `olr-cli pack fmt [--check] <dir>` rewrites files in
  canonical form. `cargo xtask check` runs `--check` over `content/packs`.

### Schema docs

- **Generated reference:** `docs/formats/` holds one page per pack kind,
  generated from the authoring types. Each field gets its meaning, unit,
  default, limits, OLX source and an example. `cargo xtask check` fails
  when the pages drift from the code.
- **Hand-written guide:** `docs/formats/README.md` explains the model
  (events and effects, objects, sprites, canonical form, `notes`) with one
  complete example per pack kind. An agent can learn the format from
  these pages alone.

### Agent operations

The operations live in one library, which the editors use too (their
undo stack comes with the editor sub-project):

- `olr-cli pack new <kind> <id>`
- `olr-cli level carve|fill|paint|stamp|resize <pack> …`: shapes and
  coordinates in world pixels; the art layers follow the art scale.
- `olr-cli level object add|move|remove <pack> …`
- `olr-cli render` (as today) and a level preview to PNG, so an agent can
  see its result.
- Mod and skin data are edited as text, then checked with `pack check`.
- `olr-cli import <OLX path>` stays, offline.

### Tests

| Test | Guards |
|---|---|
| Canonical round trip: every pack in `content/packs`, and a proptest over random authoring values: load then save gives the same bytes | Editors never rewrite what they did not change |
| Import equivalence: every bundled pack's compiled runtime data equals today's import's. Compared against the old path during the switch, then pinned as committed snapshots | A lossless format change |
| `SimMod` equivalence: the bundled mods compile to the same `SimMod` (weapons, projectile order and path names, sounds) | Projectile identity (§2) |
| Asset equivalence: alpha sprites, tint masks and stamps decode to exactly what today's load-time conversion produces | §5's conversions |
| Oracle and match goldens | The simulation is unchanged (the goldens are re-blessed only for the content-hash change) |
| Render goldens | Re-blessed once, for the dropped mirror quirk; otherwise unchanged |
| Fuzzed authoring files, images and archives | Never panic on content |

## 7. Changes outside the formats

- The Import screen leaves the game, and the importers leave
  `olr-content` (§5).
- Skin fetching (N31) is removed; a missing or different skin draws as
  the default skin (§4).
- OLX's mirrored-skin quirk is dropped (§4).
- `docs/porting/STATUS.md` records each deviation; the `CHANGELOG.md`
  entry says that packs need re-importing and that skins are no longer
  downloaded.

## 8. Out of scope

- The look's contents (graphics sub-project).
- The editors' own design (sub-projects 2 and 4). This spec gives them
  their file format and operations library.
- New simulation features: new materials, effects or events beyond LX56.
  The format reserves room for them.
- Friendlier units than OLX's.
- Tiling large level art.

## 9. Plans

Probably three plans, settled when they are written:

1. **13a:** the `olr-format` crate, the mod authoring model, its compiler
   and canonical writer, and the equivalence tests against today's
   import.
2. **13b:** levels, skins, themes and the core pack in the new format,
   with the asset conversions and their equivalence tests.
3. **13c:** the switch:
   - the importers write format 5, and `content/packs` is re-imported;
   - loaders read only the native format, and the Import screen,
     load-time keying and skin fetching are removed;
   - the STATUS and CHANGELOG entries.

   Amended by plan 13c: `olr-cli pack check|fmt|new`, the level operations
   and the schema docs move to plan 13d, and there is no hash re-bless
   (§1). Keying moved to the importers, skins are matched by id and
   content hash and never transferred (protocol 3), and the import
   pipeline lives in the new crate `olr-import`.
