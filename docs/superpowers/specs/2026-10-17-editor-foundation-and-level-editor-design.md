# Editor Foundation and Level Editor: Design (0.2, sub-project 2)

- **Status:** design agreed with the user on 2026-10-17, section by section.
- **Milestone:** 0.2 (`docs/superpowers/specs/2026-10-16-milestone-0.2-scope.md`),
  sub-project 2. It builds on the formats (sub-project 1, plans 13a-13d) and
  the operations library `olr-edit`.
- **Why:**
  - make levels from scratch;
  - compose them from imported pictures (the user's office levels);
  - playtest them without leaving the editor;
  - give the look work (sub-project 3) a preview that runs the real
    renderer and simulation.

  Liero Factory Reloaded is made in this editor.
- **Uses this editor serves first:** drawing a level from scratch,
  composing it from imported images, and playtesting while editing.
  Restyling an imported OLX level works too (§1, raster elements); it is
  not the first focus.

## 1. The model: a level is a stack of elements

A level is composed of **elements**, stacked bottom to top. Everything is
an element: an imported picture, and a layer drawn with brushes and
shapes alike. The level the game plays is what the elements make
together. It is **baked** into the files the runtime reads today
(`material.png`, `front.png`, `back.png`, `thumb.png`), so neither the
runtime nor the simulation changes.

- **Kinds:**
  - **Image:** an imported picture, placed in a box in world pixels and
    processed by its pixelation settings (§2).
  - **Drawn:** a level-sized layer of art, painted with brushes and
    shapes; its alpha says which pixels it covers.
  - **Raster:** a level's own material, front and back, per pixel. A
    level without a composition (an imported OLX level, one made only
    with `olr-cli`) opens as one raster element at the bottom, so every
    level is editable.
- **Material:** each image or drawn element has one material: rock, dirt
  or background. A raster element carries its own per pixel.
- **Topmost wins.** At each pixel the topmost visible element covering it
  decides the material and the visible art.
- **Background elements are the backdrop.** Where dirt is carved away in
  play, the topmost background element beneath it shows. Where none does,
  a raster element's back shows, else the theme's back tile.
- **Channels.** An element contributes per-pixel *channels*. Today these
  are material and colour; the look (sub-project 3) adds others, such as
  emission. Every channel follows the same rules: topmost wins, background
  elements are the backdrop, and carving reverts a pixel to the backdrop.
  Glowing destructible dirt is a dirt element with emission; a lamp is an
  element whose bulb pixels emit. Light falling on the surroundings (point
  lights as level objects, or light derived from emissive pixels) is
  sub-project 3's to decide; the model has room for both. This spec
  defines material and colour, and leaves the property set open.

## 2. The files

A level pack gains an `editor/` folder; the rest is unchanged:

```
my-level/
  pack.ron, level.ron              as today (level.ron: name, notes, size, art scale, theme, objects)
  material.png, front.png, back.png, thumb.png    the bake: what the game reads
  editor/
    composition.ron                the element stack, bottom to top
    images/<id>.png|.jpg           an image element's original, as imported
    layers/<id>.png                a drawn element's art (art resolution; alpha = covered)
    layers/<id>.material.png       a raster element's material,
    layers/<id>.front.png          front
    layers/<id>.back.png           and back
```

**`composition.ron`.** Each element has:
- a stable `id` (a slug, unique in the level), a `name`, `visible` and
  `locked`;
- `material` (not on a raster element);
- its kind's fields:
  - **Image:**
    - `source`: the file under `images/`;
    - `at` and `size`: the box, in world pixels;
    - `pixel_size`: art pixels per element pixel, at least 1;
    - `colours`: none, or 2-256;
    - `dither`: none, ordered or diffusion;
    - `alpha_threshold`: 0-255.
  - **Drawn** and **Raster:** their files under `layers/`.

The file has room for the look properties sub-project 3 adds.

**Rules:**
- **Not hashed, not downloaded.** `editor/` is left out of the content hash
  and of content downloads and archives for players: a player needs only
  the bake. Co-authors who copy the pack get everything.
- **The composition is the source of truth.** `olr-cli pack check` reports
  a bake that differs from what the composition bakes to as an error, so a
  hand edit is never silently lost at the next save.
- **CLI edits on a composed level.** `olr-cli level carve|fill|paint|stamp`
  are refused on a composed level (plan 14a ruling R1: carve and paint
  act on several materials at once, which one drawn element cannot hold);
  agents paint drawn elements with `level element draw|erase` (§7).
  `level resize` resizes the whole composition. A level without `editor/`
  behaves as it does today.
- **One canonical form.** `composition.ron` is canonical RON like every
  authoring file: `pack check` validates it, `pack fmt` formats it, and
  `format-docs` documents it.
- **Limits:**
  - at most 256 elements;
  - originals decoded within `image_io::IMAGES`;
  - layers and processed art within `LEVEL_ART`;
  - an element box's sides within `MAX_SIDE`.

## 3. The bake and the image pipeline (`olr-edit`)

All pure functions, tested without a window.

**Bake.** `bake(&Composition, &BakeInputs) -> LevelSource` makes the
material, front, back and thumbnail. `BakeInputs` carries the decoded
originals and layers, and the theme's back tile; the caller loads the
theme. For each world pixel:
- **Material and front:** the topmost visible covering element's material,
  and its art over the pixel's whole art block.
- **Back** (per art pixel), the first of these that exists:
  1. the topmost background element covering it (its art);
  2. a raster element's back;
  3. the theme's back tile, repeated from the art origin.
- **Invariant:** `front == back` on background holds by construction.
- **Region bake.** `bake_region(rect)` rebakes one rectangle. The editor
  uses it for every edit, and a test holds it equal to the full bake.

**The image pipeline** runs when an image element's settings change. Its
result is cached until they change again: moving or reordering an element
never reprocesses it. The steps:
1. Decode the original (PNG; JPEG through one new, exactly pinned decoder).
2. Resample into the element's pixel grid (box size × art scale ÷
   `pixel_size`) with an area filter.
3. Apply the alpha threshold: a pixel is covered or not, so edges are crisp.
4. Reduce the colours to `colours` by median cut, over covered pixels only.
5. Dither: none, ordered (Bayer 8 × 8) or diffusion (Floyd-Steinberg).
6. Scale up by `pixel_size` with nearest neighbour, and place at `at` × art
   scale.

**Determinism.** Integer arithmetic throughout: the same composition bakes
to the same bytes on every platform, or `pack check`'s stale-bake error
would differ between Linux and Windows.

**Speed targets (release):** a full bake of an OLX-sized level well under a
second; a brush-sized region bake within a frame.

## 4. The editor framework (`olr-editor`, new crate, layer 5)

The mod and skin editors (sub-project 4) reuse this framework. It follows
`olr-ui`'s pattern: pure state, and a host that carries out commands.

- **State.** `EditorState::update(EditorAction) -> Vec<EditorCommand>`, with
  no GPU and no window.
  - Actions are mouse, keyboard and widget events.
  - Commands go to the host: upload a dirty rectangle, save files, start a
    playtest with a baked level, quit.
  - Flows are tested headless with `egui_kittest`, by label.
- **Layout:**
  - a toolbar (tools, Play, undo/redo, view toggles);
  - the element list on the left (drag to reorder; visibility and lock;
    add an image, a drawn layer or a spawn);
  - the selection's properties on the right;
  - a status bar: live diagnostics from `pack check`'s rules, and the
    cursor's world position and material;
  - the canvas in the centre.
- **Canvas.**
  - The real renderer draws the baked level: what you see is what plays.
  - egui overlays sit on top: selection boxes and handles, the brush
    cursor, spawn markers, an optional pixel grid.
  - Pan and zoom, pixel-snapped at integer zoom.
  - A material view shows `MATERIAL_PALETTE` instead of the art.
- **Undo/redo.** A history of reversible edits:
  - a property change keeps its old value;
  - a pixel edit keeps its dirty rectangle's previous pixels;
  - one stroke or one drag is one step.

  The history is at most 256 MiB, dropping the oldest steps first, and is
  not saved.
- **Files and safety.**
  - New, open and save level packs. Saving writes the composition and the
    bake through `olr_edit::pack::save`, so the whole pack is saved at once
    and validated.
  - Unsaved changes are flagged and confirmed on quit or open.
  - A recovery copy is written to the data directory every 2 minutes and
    offered after a crash.
  - A small egui file browser opens files and folders: no native-dialog
    dependency, the same on WSL and Windows, and tested headless.

## 5. The level editor (first version)

- **Level settings:**
  - name, notes, theme (from the installed themes) and art scale;
  - size: a resize with an offset, as `olr-cli level resize` does, and the
    composition follows;
  - an art-scale change rescales drawn and raster layers by nearest
    neighbour, with a warning.
- **Elements:**
  - Select, move and resize an image element's box with handles. Arrow keys
    nudge 1 px; Shift keeps the aspect ratio.
  - Import an image (PNG, JPEG). It becomes an image element at the view's
    centre: pixel size 1, no colour reduction, threshold 128, material dirt.
  - Edit properties with a live preview: material, pixel size, colours,
    dither, threshold, name, visible, locked.
  - Add, rename and reorder drawn layers.
- **Painting** (into the selected drawn layer):
  - brush and eraser, round or square, sized in world pixels;
  - filled rectangle, ellipse and line;
  - paint is a colour, the theme's front or back tile, or a colour picked
    from the canvas.
- **Objects:** place, move and delete spawn points, listed under Objects in
  the element list.
- **Play:**
  - bakes, then starts a sandbox match on the level in place;
  - your worm starts at the first spawn, or a free spot;
  - the mod is chosen (default Liero v1.0), with 0-8 bots;
  - Stop or Esc returns to the editor exactly where it was, and the match's
    damage is thrown away;
  - the mod and bot count are remembered.
- **Shortcuts:**
  - Ctrl+Z / Ctrl+Y, Ctrl+S, Delete;
  - V select, B brush, E eraser, R rectangle, O ellipse, L line, P spawn,
    I eyedropper;
  - Space-drag pans, and the wheel zooms.
- **Not in this version:**
  - the theme's stamps (holes) as tools;
  - flood fill;
  - pixel selection and copy/paste;
  - multi-select and groups;
  - look properties (sub-project 3);
  - tablet pressure.

## 6. Hosting in the game (`olr-game`)

- **Opening.** The main menu gains **Editor**: a list of level packs, New
  level, and New from a copy. `olr --edit <pack dir>` opens a pack
  directly.
- **Where packs are edited.** In the data directory's `packs/`. Opening a
  bundled level (`content/packs`) offers to save a copy there first, so
  committed content is never edited by accident.
- **The canvas** goes through `olr-present`'s terrain path; each edit
  uploads only its dirty rectangle, which the renderer already supports.
- **Play** builds the match from the baked level as the sandbox does, runs
  it in the game's usual loop, and returns on Stop.

## 7. Agent operations (`olr-cli level element …`)

The composition can be scripted, as spec 2026-10-16 formats §6 requires
for agent operations. Each command saves the whole pack and rebakes;
`level preview` shows the result.

```
olr-cli level element list      <pack>
olr-cli level element add-image <pack> <file> --at X,Y --size WxH [--pixel-size N] [--colours N]
                                [--dither none|ordered|diffusion] [--threshold N] [--material M] [--name N]
olr-cli level element add-layer <pack> --material M [--name N]
olr-cli level element set       <pack> <id> --<property> <value>
olr-cli level element remove|raise|lower <pack> <id>
olr-cli level element draw      <pack> <id> (--rect X,Y,W,H | --circle X,Y,R) (--colour RRGGBB | --tile F.png)
olr-cli level element erase     <pack> <id> (--rect X,Y,W,H | --circle X,Y,R)
olr-cli level bake              <pack>
```

Each command takes `--content DIR` (default `content/packs`) for the
theme's back tile. `level carve|fill|paint|stamp` are refused on a
composed level (§2).

## 8. Testing

- **`olr-format`:** the composition's canonical round trip; its limits;
  hostile files give errors, never panics.
- **`olr-edit`:**
  - every bundled level, opened as one raster element and baked, gives
    back its files byte for byte, so opening and saving an OLX level never
    alters it;
  - proptests over random stacks: the background invariant holds, and
    region bakes equal full bakes;
  - unit tests of the pipeline: threshold edges, at most N colours, dither
    patterns, pixel size;
  - the bake is the same on Linux and Windows (`cargo xtask determinism`'s
    Windows leg bakes a fixture).
- **`olr-editor`:** `egui_kittest` flows: import an image, change its
  properties, undo and redo, paint, place a spawn, save, then check the
  files.
- **`olr-cli`:** the element commands; `pack check`'s stale-bake error.
- **End to end:** an autoplay script in `cargo xtask e2e` opens the editor,
  adds a layer, paints, places a spawn, plays, stops and saves.
- **Unchanged:** the oracle traces, the match goldens and `content/packs`.

## 9. Plans

- **14a:**
  - the composition format and the bake;
  - the image pipeline, with JPEG;
  - leaving `editor/` out of the hash and downloads;
  - `pack check`'s stale-bake check;
  - the CLI element commands and `level bake`.

  No GUI; it is useful to agents on its own. Done
  (`docs/superpowers/plans/2026-10-17-plan-14a-level-composition.md`;
  decisions in `docs/porting/STATUS.md`, "Level composition decisions").
- **14b:** `olr-editor`: the framework and the level editor's state and
  panels, all tested headless.
- **14c:**
  - hosting in `olr-game`: the menu entry, the canvas, Play, the file
    browser, recovery;
  - the e2e script.

  Then a playtest zip.

## 10. Out of scope

- The look (sub-project 3): emission, lights, colour grading. The model
  only leaves room for them (§1).
- The mod and skin editors (sub-project 4).
- Editing a level while a match runs on it (a live world); playtests start
  from the bake.
- Keeping a playtest's craters as a drawn element.
