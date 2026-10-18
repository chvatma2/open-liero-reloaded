# Level Editor Polish: Design

- **Status:** agreed with the user on 2026-10-18, after the playtest of plan
  14c's editor. Planned as plan 14d, before the look system (milestone 0.2
  sub-project 3).
- **Builds on:** `docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md`
  (the element model, the bake, the editor), plans 14a-14c, and the
  decisions in `docs/porting/STATUS.md` ("Level composition decisions",
  "Level editor decisions" of 14b and 14c).

## Why

The user wants to build real levels in the editor (Liero Factory Reloaded,
an office map with friends). The playtest showed what stands in the way:

- painting in one flat colour looks bad;
- levels are built from repeated parts (a crate, a pipe, a desk), and there
  is no copy and paste;
- imported pictures carry backgrounds around the part that is wanted, and
  only drawn layers can be erased;
- a finished part cannot be merged into one layer, and only a Base layer
  can be split into rock, dirt and background.

The goal: painting that looks like the material it paints, building from
reusable pieces, and cleaning imported pictures, all as quick as the
existing tools.

## Goals

1. **Textured painting.** Every painting tool (brush, rectangle, ellipse,
   line, and the new flood fill) paints with a texture as easily as with a
   colour.
2. **A texture library:**
   - a small library shipped with the game, grown over time;
   - the level's theme tiles (front, back), as today;
   - the user's own textures, imported from any image into a personal
     library that every level offers afterwards.
3. **Copy and paste of elements.** Copy one or more elements and paste
   independent copies; each copy is its own element from then on (editing
   the original never changes the copies).
4. **Copy and paste of pixels.** Select a rectangle of a drawn layer's
   pixels, copy, cut and paste it (as a new layer or into a layer).
5. **Masks on imported pictures.** Erase the unwanted parts of an image
   element without losing its resizing, pixelation, colour reduction or
   dithering: a restore brush brings parts back, and a "remove this
   colour" helper masks out a connected region of similar colour.
6. **Merge down.** Merge a layer into the layer below it, by material
   priority (below).
7. **Split by material for any layer.** A Base (raster) layer, a mixed
   layer or a picture splits into up to three layers: Rock, Dirt and
   Background.
8. **Flood fill**, with the current paint (colour or texture).
9. **The theme's hole stamps as brush shapes** (the craters weapons
   make), for natural-looking caves and edges.
10. **Multi-select** of elements: move, copy, delete, lock and hide several
    at once.

## Non-goals

- Emission, lights and any look property: the look system (sub-project 3)
  comes next and adds them.
- Linked copies (instances): pasted copies are independent, the user's
  choice.
- Tablet pressure.
- Changing the simulation, the runtime level format the game reads
  (`material.png`, `front.png`, `back.png`, `thumb.png`), the oracle or
  the match goldens.

## Anti-goals

- **A level that needs the texture library to play or to be shared.**
  Painting writes pixels into the layer; textures matter only while
  painting. A co-author opening the pack without the user's library sees
  the same level.
- **Edits that silently change a level's look.** Only merge may change
  what plays (its material priority, by the user's rule), and it is one
  undoable step.
- **Destroying an imported picture by erasing.** Masks are non-destructive
  until the user merges the picture.

## The decisions the user made

1. **Textures:** a shipped library grown over time, the theme's tiles, and
   imported images. Imported textures go into the user's **own library**,
   offered in every level.
2. **Copies are independent** once pasted.
3. **Erasing on a picture keeps it editable:** resizing, pixelation and
   colour settings still work afterwards.
4. **Merge rule.** Per pixel, a transparent pixel is empty and loses to
   anything. Among the two layers' covering pixels the material decides,
   always **rock, then dirt, then background**, whatever the stacking
   order. The merged pixel shows the art of the layer whose pixel won.
   Example: a rock building merged onto the Dirt layer stays rock where the
   building is opaque; where it is transparent, the dirt layer's pixel
   (dirt, or empty) remains. Where a dirt pixel covered a rock pixel, the
   merged pixel is rock (this may change what plays there; intended).
5. **Split by material for any layer** (Rock, Dirt, Background; a material
   with no pixels makes no layer).
6. **This round also includes** flood fill, pixel copy and paste, the
   theme's hole stamps and multi-select.
7. **Merging never changes what a tunnel shows.** When dirt or rock merges
   over a background pixel, the merged layer remembers that background
   art, and digging the dirt away in play shows it, as on a loaded map
   today.
8. **Painted layers move.** Drawn and mixed layers have a position and can
   be dragged, nudged, copied and pasted at an offset, like pictures, so a
   painted crate can be duplicated and dragged to its next spot.

## Calls made in writing this (not the user's)

Marked here so they can be checked:

- **C1. Same material in a merge:** the upper layer's pixel wins.
- **C2. Material per world pixel.** A mixed layer stores one material per
  world pixel, as `material.png` does. Merge decides per world pixel:
  - The covering state of each layer comes from the block's centre art
    pixel, as the bake does today.
  - The winning layer gives the block's art pixels. Where its art is
    transparent inside the block, the other layer's art fills in.
- **C3. Pasted pixels keep their material.** Pasting rock into the Dirt
  layer keeps it rock, and the target becomes mixed. "Paste as layer"
  makes a plain layer when the pixels share one material, and a mixed
  layer otherwise.
- **C4. The material property:**
  - On a plain layer, setting it reassigns every pixel, as today.
  - On a mixed layer, it reads "Mixed", and choosing a material makes the
    layer plain with that material (one step).
  - The material of new paint is a brush option: "the layer's" by default,
    or Rock, Dirt or Background.
  - On a multi-selection, the property sets each element as above.
- **C5. A drawn layer's extent:**
  - A new layer covers the level. Paint outside a layer's box is clipped
    to the box.
  - An element copy of a drawn layer is cropped to its covered pixels.
  - A merge covers both boxes. A split keeps the source's box.
  - Pixels moved off the level stay in the layer and do not bake.
- **C6. Pasting into another level** (the clipboard spans levels): a
  layer whose art scale differs is resampled to the target's scale by
  nearest neighbour.
- The CLI commands, the texture library's place and format, and the
  composition format's additions below.

## Behaviour

### Textures

- A texture is a small RGBA image that repeats from the level's art origin,
  in art pixels, like the theme's tiles today (`PaintChoice::FrontTile`):
  strokes join without seams, and the same world pixel always takes the
  same texel.
- The paint is a colour, the theme's front or back tile, or a texture from
  the library. The paint does not decide the material: the layer does (a
  dirt-textured stroke on the Rock layer is rock), as for colours today.
- **The palette** shows the textures as small swatches, grouped: the
  level's theme, the shipped library, the user's library. The current
  paint is highlighted; the eyedropper still picks a colour.
- **Importing a texture:** a "New texture…" action opens the file browser
  (PNG or JPEG, the import's bounds), with the same preview, and adds the
  picked image to the user's library (copied in; the source file is not
  needed afterwards). Removing a texture from the user's library is
  offered too.
- A texture is at most 512 × 512 art pixels (larger images are refused with
  a message; nothing is resized silently).

### Copy and paste of elements

- Ctrl+C copies the selected elements; Ctrl+X cuts; Ctrl+V pastes copies on
  top of the stack, offset by a few world pixels, selected, as one undo
  step. A copy gets a new id and name ("Crate 2").
- Copies of an image share its original file in the pack (no duplicate
  file); each copy has its own box, settings and mask.
- The clipboard is the editor's own (within one session, across levels
  opened in it); the system clipboard is not used.

### Copy and paste of pixels

- A **Select pixels** tool drags a rectangle on the selected drawn layer;
  Ctrl+C copies its art (and per-pixel material, when the layer has one),
  Ctrl+X cuts (clears it), Delete clears it.
- Ctrl+V with pixels on the clipboard pastes them as a floating selection
  over the layer: dragged into place, then applied (Enter or a click
  outside) into the selected layer, or as a new layer ("Paste as layer").
  One undo step each.

### Masks on image elements

- Every image element may have a mask over its original, in the
  original's pixel grid (so resizing the box keeps it aligned).
- **Eraser** on an image element clears the mask under the brush; the
  **restore** brush (the eraser with a modifier, or its own toggle) sets
  it back. Strokes are one undo step each, as painting is.
- **Remove this colour:** on an image element, a click masks out the
  connected region of similar colour around the clicked pixel of the
  original (a tolerance slider, 0-100 %); one undo step.
- The image pipeline applies the mask before resampling (spec §3): masked
  pixels are transparent, then the alpha threshold, colours and dither run
  as today. The mask changes the processed grid's cache key.
- A locked element's mask is not changed (R4 of plan 14b).

### Layers with per-pixel material

- A drawn layer keeps its material (`ElementKind::Drawn { material }`) as
  the material of new paint, and may in addition hold a **material per
  pixel** (a mixed layer). A plain drawn layer is a mixed layer whose every
  covering pixel has the layer's material.
- Merge produces mixed layers, and so does pasting pixels of another
  material (C3). The brush's material choice (C4) paints any material
  into any layer; painting another material into a plain layer makes it
  mixed.
- A mixed layer may also hold a **back** art (decision 7): what shows
  where its dirt or rock is carved. Where it has none, what lies below
  shows, as for any layer today.
- The bake reads a mixed layer's per-pixel material where it covers. The
  background rule (§1 of the editor spec: background elements are the
  backdrop) applies per pixel: a mixed layer's background pixels, and its
  back art, are backdrop.

### Moving drawn layers

- A drawn layer (plain or mixed) has a position in world pixels and a box
  of its own (C5). Select drags it, the arrow keys nudge it, as for an
  image box today. A move is one step and changes no pixels.
- Painting, erasing, fill and pixel selection work in the layer's own
  coordinates, so a moved layer paints where it shows.

### Merge down

- "Merge down" on an element merges it into the element directly below,
  which must be a drawn layer (mixed or not); otherwise the action is
  refused with the reason. Hidden elements are refused too ("show it
  first"); a locked pair is refused.
- The result replaces both: a mixed drawn layer with the lower one's id,
  name and stack position, covering both boxes (C5). Its pixels follow
  the merge rule (decision 4, C1, C2). Where dirt or rock won over a
  background pixel, or over a pixel with back art, that art becomes the
  merged layer's back art (decision 7). An image merges as its processed
  pixels (its mask applied). One undo step.
- Example: merging Dirt into Background on a loaded map gives one mixed
  layer. It bakes byte for byte as before, and digging its dirt in play
  shows the level's own background.

### Split by material

- "Split into layers" on any element: a raster (Base), a mixed layer, a
  plain drawn layer (trivially one layer), or an image (its processed
  pixels). The result is up to three drawn layers, bottom to top
  Background, Dirt, Rock, in the element's place, each holding only its
  material's pixels; a material without pixels makes no layer. A mixed
  layer's or a raster's back art becomes part of the Background layer, so
  a split also bakes as before. One undo step. (Plan 14c's raster split,
  generalised.)

### Flood fill

- The **Fill** tool (key G) fills the connected region of the clicked
  pixel in the selected drawn layer with the paint. Connected means: the
  same covered state and, for covered pixels, the same colour within a
  tolerance (a slider, default exact), 4-neighbour. Bounded to the level.
  One undo step.

### Hole stamps

- The brush offers the theme's hole stamps (`hole1`-`hole5`, the craters
  weapons carve) as shapes beside round and square. A stamp paints (or,
  with the eraser, clears) its covering pixels at its own size, scaled by
  the brush size in whole steps. The shapes come from the level's theme
  (`LoadedTheme::holes`).

### Multi-select

- Ctrl+click and Shift+click in the element list (and Ctrl+click on the
  canvas with Select) select several elements.
- With several selected: move (drag or arrow keys, all together), copy,
  cut, paste, delete, lock and hide apply to all, each as one undo step.
  The properties panel shows the shared properties (visible, locked,
  material when they agree) and the count.

## Formats

The composition format (`editor/composition.ron`, spec 2026-10-17 §2) grows,
version bumped (compatibility breaks are fine at this stage):

- **A drawn layer's position and box:** `Drawn { material, at, size }`
  (world pixels; absent means the whole level, at the origin).
- **A mixed drawn layer:** `Drawn { material, mixed: true, … }` (or a
  separate kind, the builder's call). Its per-pixel material is in
  `editor/layers/<id>.material.png` (C2: one per world pixel, the level's
  `MATERIAL_PALETTE`, indexed). Its optional back art is in
  `editor/layers/<id>.back.png`. Both sit beside its art,
  `editor/layers/<id>.png`.
- **An image's mask:** `editor/masks/<id>.png`, a 1-bit (or grey) image of
  the original's size; absent means no mask.
- `pack check`, `pack fmt` and `format-docs` cover the new fields and files;
  `pack check`'s stale-bake check covers mixed layers and masks.

**The texture library** is not part of a level pack:
- the shipped library: PNGs in a pack of its own in `content/packs`
  (a new pack kind, `Textures`, or the builder's equivalent), named and
  listed in a manifest; built and committed like the other content;
- the user's library: the data directory's `textures/`, one PNG per
  texture plus its name, managed by the editor;
- their contents: the builder's call. The user's ruling is "a basic
  library, grown over time". A first set covering dirt, rock, grass,
  brick, metal and wood is enough; their origin (made procedurally by a
  committed tool, or drawn) must be licence-clean and recorded.

## Agent operations

Spec 2026-10-16 formats §6 requires the composition to be scriptable. The
new composition-changing operations get `olr-cli level element` commands:
`duplicate <id>`, `merge-down <id>`, `split <id>`, `mask <id> (--rect … |
--circle … | --colour-at X,Y [--tolerance N]) [--restore]`, and
`draw|fill … --texture F.png`. Pixel copy and paste, the palette and
multi-select are editor-only.

## Constraints

- Everything in the editor's pattern: pure state (`olr-editor`), the host
  (`olr-game`) reads and writes files; tests headless with `egui_kittest`.
- Never panic on content; textures, masks and mixed layers are bounded
  (a texture 512²; masks and layers the level's art size, `LEVEL_ART`).
- Deterministic bake (integer arithmetic), as today: `pack check` must
  agree across Linux and Windows (`cargo xtask determinism`'s bake digest
  covers a fixture with a mixed layer and a mask).
- The history cap (256 MiB) counts masks and per-pixel materials.
- The oracle, the match goldens and `content/packs` (but the new texture
  pack) stay byte-identical; every bundled level still round-trips.
- Region bakes equal full bakes (the existing proptest grows mixed layers
  and masks).

## Left to the builder

- The UI layout of the palette, the tool options and the multi-selection's
  properties, within the existing panels.
- Keys for the new tools (Fill G is suggested; Select pixels, the restore
  toggle).
- The exact composition syntax for mixed layers and masks.
- The shipped textures' contents and how they are made.
- Whether pixel paste floats or applies at once (floating is the
  described behaviour; a simpler first version is acceptable if the
  floating step is recorded as a follow-up).
- How the work splits into plans (one per batch of features is expected:
  for example textures; masks and mixed layers with merge and split;
  clipboard and multi-select; fill and stamps).
