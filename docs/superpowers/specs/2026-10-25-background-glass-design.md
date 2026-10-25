# Background glass: windows onto the distant layers

Date: 2026-10-25. Status: design, approved by the maintainer in chat;
reviewed before planning.

## Intent

Liero Factory Reloaded is a factory at night with a starry sky as a
distant (parallax) layer. Its large hall window, and its small office
windows, are opaque blue paint today. The maintainer wants them to be
**half-transparent glass**: the sky shows through, tinted. A moon hangs in
the sky, and because the sky scrolls slower than the level, the moon shows
in the window **only when the camera is in the right place**. One pane is
broken: a small jagged hole shows the sky with no tint.

The window should look **lit only by what is behind it**, the way a real
window at night looks: the hall's lamps do not light the glass, the sky
does. The window still casts a pale moonlight into the hall.

A throwaway spike (2026-10-25, branch `spike/glass`, kept in `git stash`
as "spike/glass: see-through glass, distortions (throwaway)") rendered
all of this from real frames. The comparison page the maintainer judged
from: https://claude.ai/artifact/N5ycYHGM8JZpWtA4DForo5. Of five
distortions (none, wave, frost, ripple, lens) the maintainer picked
**clear glass**, and wants **frost** as an option a level author sets,
with a strength, to try it in motion in the game. Wave, ripple and lens
were rejected: smooth distortions do not suit pixel art.

The maintainer then asked for **tintable glass** whose light, too, is
coloured by the glass. A second spike stained two panes amber and red
(the page's "Stained glass" section; its shader changes are in the
session scratchpad's `spike-stained.patch`, throwaway). Of two pictures,
**filter** (the sky multiplied by the pane's colour, as real glass) and
hue (the sky's brightness in the pane's colour), the maintainer chose
**filter**: the moon turns red behind red glass, and amber over a blue
night sky goes dark olive, as real amber glass would.

## Goals

1. A level author can make any background element **glass**: its covered
   pixels are see-through at an opacity the author sets, in levels that
   have distant layers.
2. Glass shows the distant layers behind it, scrolling with their
   parallax: its own colour laid over them at its opacity (a sheen), and
   what shows through filtered by its colour at an author-set **tint**
   (0 clear: the sky as it is; 100: the sky multiplied by the glass's
   colour).
3. Glass is **lit as what is behind it**: it takes the light the distant
   layer behind it takes (a sky at depth 90 takes a tenth of the light
   beyond the ambient), never the hall's light, and it shows no glow of
   its own.
4. Glass may **frost**: an author-set strength scatters what shows through
   it, each glass pixel showing the distant layer from a nearby pixel,
   fixed to the glass (it does not change over time; it shimmers only as
   the camera moves).
5. A **stained window casts coloured light**: a glow group may cast its
   colour filtered by each pixel's art colour, so each pane lights the
   room in its own colour.
6. The **level editor** sets a glass element's opacity, tint and frost (sliders
   in its properties), and its canvas shows the glass as the game does.
7. **Liero Factory Reloaded** gets it: every window is glass, a moon in the
   sky, a broken pane in the hall window, and the window casts moonlight.

## Non-goals

- Glass in front of the level (rock or dirt glass). The level already has
  clear solids for light passing through rock; glass is background only.
- Glass that breaks in play. The broken pane is painted, not shot.
- Wave, ripple, lens or any other smooth distortion; any distortion that
  moves over time.
- A cast light that follows the moon. Level light is made once for the
  whole level and never depends on the camera (split screen, the glow light
  once a tick). The window casts the same light whether or not the moon
  shows in it. The maintainer accepted this.
- Glass in a level without distant layers (there is nothing behind it to
  see; see Rules).
- A brush that paints opacity per pixel. Opacity is per element; a hole or
  an edge is made by erasing or by a second element.

## Anti-goals

- Any change to a level without glass. Every level today draws byte for
  byte as before: the render goldens, `lit-hashes.ron` and the look
  goldens hold without re-blessing. Only Liero Factory Reloaded's own
  tests and pictures change.
- A light that depends on the camera.
- Frost that crawls or flickers while the camera stands still.

## What exists today

- A level with distant layers is **see-through**:
  `olr_terrain::ColorMap::new_see_through` keeps back pixels of alpha 0
  see-through and makes every other pixel opaque. The renderer draws an
  opaque backing, then the distant layers far to near (`fs_distant_lit`,
  unblended), then the terrain over them with alpha blending on all three
  lit targets (colour, shade, heat light). `olr-render/CLAUDE.md`,
  "Distant layers".
- The bake (`olr-edit`'s `compose.rs`) makes every covered pixel opaque
  (`opaque(p)`); images are thresholded (`alpha_threshold`); the level's
  `back.png` has only alpha 0 and 255.
- Liero Factory Reloaded already has its windows as their own element,
  `glass` (a Drawn Background layer, `editor/layers/glass.png`, glow 45 in
  group `glass`: `Tint("#a0c0ff")`, intensity 80, reach 24 px), over the
  `background` element, which has its own wall pixels under the glass.
  Its sky is the distant element `sky` at depth 90, glowing 35 in group
  `sky`.
- The spike proved the mechanism: keeping partial alpha through
  `new_see_through` and `replace_rect`; the lit terrain writing shade
  alpha 0 for glass so the distant layer's shade (and so its light) stays;
  glass self-light 0; frost as a per-world-pixel hash offset of the
  distant texel fetched in `fs_distant_lit`, clamped to the layer's rect.

## Decisions

### Authoring

- An element gains `glass: Option<ElementGlass>` (`olr-format`'s
  composition), with:
  - `opacity`: percent, 1 to 99, required. How much of the glass's own
    colour covers what is behind it.
  - `tint`: percent, 0 to 100, default 0 (clear). How strongly what
    shows through is filtered by the glass pixel's colour.
  - `frost`: tenths of a world pixel, 0 to 40, default 0 (clear). How far
    a glass pixel may shift what shows through it.
- Allowed on an Image or Drawn element whose material is Background, not
  on a distant element, not on an asset's elements (as `distant`). On a
  mixed drawn layer it applies to its background pixels only. A level that
  breaks these rules fails its check like any other bad composition.
- The bake writes a glass element's covered back pixels with alpha
  `round(opacity × 255 / 100)` (its art's own alpha ignored, as today) and
  composites them over what lies below in `back.png` with straight-alpha
  `over` (`level.rs`'s `over`): glass over an opaque element below is
  simply an opaque tinted pixel; glass over nothing stays see-through. So
  the author removes the wall under a window to let the sky show.
- Tint and frost are baked into a new `look/glass.png` (planning: RGBA,
  the level's art size): each art pixel's RGB is its **filter**,
  `mix(255, g, t / 100)` per channel with `g` the glass pixel's art colour
  (white: no filter), and its alpha the frost in tenths, from the topmost
  glass element giving the back there; white and 0 elsewhere. Written only
  when some glass tints or frosts; absent otherwise. Storing the filter
  baked keeps the renderer to one texture and no read of the terrain.
- `docs/formats/level.md` documents both, and the alpha rule of
  `back.png` changes from "0 or opaque" to "0 see-through, 1 to 254 glass,
  255 opaque".
- The level editor's properties panel shows a **Glass** section for an
  allowed element: a toggle, an Opacity slider (1 to 99 %), a Tint
  slider (0 to 100 %) and a Frost slider (0 to 4.0 px). Changing them re-bakes as other properties do.

### Terrain

- `ColorMap::new_see_through` keeps the back art's alpha whatever it is,
  in the map's back, and in its front and composite where the material is
  background. Carving copies the back, so dirt dug in front of glass shows
  the glass at once. `replace_rect` keeps alpha (the editor canvas).
- Stains: blood and gibs never land on glass, as they never land on
  alpha-0 pixels (simplest; the builder may choose to let them land and
  keep the glass's alpha, if that is as simple).
- `ColorMap::new` (no distant layers) is unchanged: every pixel opaque, so
  glass in such a level draws as its own colour, opaque (the bake lays
  glass over the opaque back tile there). The editor's Glass section says
  so while the level has no distant layer (planning: a hint, as the
  Distant switch's, instead of a check warning).

### Look

- **A glass pixel is a back pixel with alpha 1 to 254** in a see-through
  level. No other marker is needed.
- **Picture:** with `a` the pixel's alpha, `t` its tint, `g` its art
  colour and `b` what the distant layers draw behind it:
  `shown = g × a + b × mix(1, g, t) × (1 − a)`. With `t` 0 this is the
  plain alpha blend of the first spike. The filter needs the destination
  multiplied by a per-pixel colour, which the terrain pass's alpha
  blending cannot do in one draw (and dual-source blending is not on GL):
  planning chose the distant pass: `fs_distant_lit` multiplies its texel
  by the baked filter at the art pixel it lies under (the backing behind
  glass no layer covers stays unfiltered). The multiply is on the stored
  bytes, as the rest of the world pass's blending (the spike's look).
- **Light:** the lit terrain writes shade alpha 0 for a glass pixel, so the
  shade the composite reads is the distant layer's: the glass takes the
  light of the layer behind it, by its depth. Its self-light is 0 whatever
  its glow (a glass pixel never shows a glow of its own). Where no distant
  layer covers the pixel the opaque backing's shade stays (lit as the
  level's back).
- **Cast:** a glass pixel's glow (its element's `glow`) still casts as any
  glow does: that is how a window casts moonlight. **Coloured light**: a
  new glow colour source `Filtered(colour)` beside `Own` and `Tint`
  (`docs/formats/level.md`'s GlowColourSource) casts `colour` × each
  pixel's art colour, the art colour scaled so its brightest channel is 1
  (my call: the pane's hue at the group's strength; the author sets how
  bright with the group's intensity; the spike's normalised colours read
  well at 600). A clear window keeps `Tint`; a stained one uses
  `Filtered` with the moonlight's colour. It is a level look value, so it
  is hashed with the look like the others. `Own` and `Tint` cast exactly
  as before. The distant layers'
  own glow (issue #62) is baked only where the back is alpha 0, as today:
  it does not pass through glass. (Passing it would light the factory
  hall with the sky's unlimited-reach group; the window's light stays the
  author's, through the glass element's glow.)
- **Frost:** `fs_distant_lit` reads `look/glass.png` at the art pixel
  the texel lies under; with a frost `f` above 0 (its alpha, in tenths)
  it fetches the layer's
  texel offset by a hash of that world pixel, each axis in `[-f, f]` world
  pixels (times the art scale), clamped to the layer's rect. The hash is
  of the world pixel only: no time, no camera, so the pattern is fixed to
  the glass. Without a frost map nothing is bound or read that changes a
  byte.
- **Heat** (my call): glass is background; it heats as its surface
  material says, as today, and **hot glass still shows its heat**: its
  heat light (and, in full bright and light only, its heat self-light) is
  kept at the glass's alpha, so a hot pane glows over the sky behind it.
  Only the light the glass *takes* is the layer's; what it emits from heat
  stays its own. Liero Factory Reloaded's `glass` material ("glows amber
  when hot") keeps working. A GPU test: hot glass glows; cold glass is lit
  as the layer behind it. The worm halo falls on glass as on the layer
  behind it (the shade is the layer's).
- **Unlit frames** (my call): glass draws in unlit frames too (the
  terrain's alpha blending), but tint and frost are lit-only. A level
  without a look shows its glass clear and untinted, and the editor's
  Tint and Frost sliders say so there, as it hints for distant layers.
  Every Reloaded level has a look.
- **The bake's `over` onto a see-through pixel:** `level.rs`'s `over`
  mixes the glass colour with the colour an alpha-0 pixel still carries.
  Glass needs a correct straight-alpha `over` (a fully transparent
  destination contributes no colour).

### Liero Factory Reloaded

- `glass` element: `glass: (opacity: 35)` (the spike's 90 of 255), no
  tint, no frost: clear glass (my call; stained panes in the factory are
  the maintainer's to ask for). Its colour mixed halfway towards a pale blue-grey (spike:
  `(200, 215, 235)`), so the sky reads through it rather than a strong blue.
- The `background` element's wall pixels under every glass pixel are
  erased, so the sky shows (as the roof garden fix did).
- A broken corner in the hall window's fifth pane (world x 799 to 817,
  y 151 to 172): a jagged hole erased from the glass layer, and a rim of
  lighter, more opaque glass around it (spike: colour `(215, 225, 240)`,
  about 82 % opacity: a second glass element `glass-rim`, or the builder's
  equivalent).
- A moon in `editor/images/sky.png`: a disc of radius about 6.5 world px
  with a soft halo and a few craters, placed so it is centred in the hall
  window for a camera at about (760, 215).
- The window's light: the `glass` glow group retuned so the hall takes a
  pale moonlight from the window as in the spike's frame (spike:
  `Tint("#c8d4f0")`, intensity 220, reach `Pixels(160)`, glass glow 170 of
  255 ≈ 67 %). The builder tunes it against a rendered frame.
- `content/reloaded/README.md` notes the glass; `crates/olr-cli/tests/
  reloaded.rs` is updated for the changed art.

## Tests the builder adds

- Format: `glass` round-trips; out-of-range opacity, tint and frost, glass on a
  rock, dirt, distant or asset element are refused.
- Bake: a glass element's covered pixels get the opacity's alpha; glass
  over an opaque element is opaque; `look/glass.png` holds the topmost
  frost and is absent without frost.
- Terrain: `new_see_through` keeps partial alpha in back, front and
  composite; carving in front of glass reveals it with its alpha; stains
  skip glass; `new` stays opaque.
- GPU: a glass pixel shows the distant layer blended with its colour;
  tinted glass multiplies what shows through by its colour (tint 100 over
  a white layer shows the glass colour; over blue, amber glass is dark);
  tint 0 draws the plain blend's bytes; `Filtered` casts the group colour
  × the pixel's normalised art colour; a
  bright lamp in the level does not light the glass while it lights an
  opaque back pixel beside it; glass shows no self-light while its glow
  still casts; frost shifts what shows through and is the same frame after
  frame with the camera still; a level without glass draws today's bytes
  (the goldens and lit hashes hold unchanged).

## Left to the builder

- Shader and pipeline layout (where the frost map binds, how the lit
  terrain detects glass).
- The hash used for frost (the spike's per-pixel hash looked right at
  1.5 px).
- Whether the broken pane's rim is a second element or another way.
- Exact moonlight values, judged from a rendered frame.
- Whether this ships as a GitHub issue for the runner or a plan carried
  out in a session (the maintainer's call at handover).
