# olr-terrain

The simulation's terrain (spec §5.4): one `Material` byte per world pixel,
and the colour image that follows it (`ColorMap`).

- **Layer 1, `#![no_std]` + `alloc`.**
- `flags(x, y)` is OLX's `GetPixelFlag`: rock outside the map.
- `carve_hole(stamp, x, y)` ports `CMap::CarveHole` for the material grid,
  including OLX's edge-clipping quirk: the stamp is read from its top-left
  corner even when its left or top part is clipped.
- `place_dirt(stamp, x, y)` and `place_green_dirt(mask, x, y, rand)` port
  `CMap::PlaceDirt` and `CMap::PlaceGreenDirt`. Unlike `carve_hole`, they
  offset the stamp by the clipped amount. `place_dirt` counts a background cell
  under a non-magenta colour twice, as OLX does. `place_green_dirt` calls
  `rand` once for each green pixel it places (`GreenMask`, from `greenball.png`).
- Positions are C `int`s: no position, however far outside, may panic.
- `material_hash()` is the oracle's `mh`: FNV-1a 64 over the material bytes.
- **`ColorMap`** (`color.rs`, spec §19 E7) is the visible colour image at art
  scale (`art_scale` art px per world px), kept in step by replaying the
  simulation's edits in order (`apply(ColorEdit)`; `olr-content` converts
  `olr-sim`'s `TerrainEdit`): carving copies back pixels and paints the rim,
  `PlaceDirt` paints `FrontTile` (tiled from the origin in world px) and the
  rim over former background, green dirt paints `GREEN_SHADES` (own
  generator) and the mask's rim. It keeps its own copy of the material
  (`material()`) and edits it after painting, so every rule reads the
  material before the edit, like OLX. `stain` (blood) and `stain_image`
  (gibs, OLX's `DrawImageStretch2` clipping quirk; `on_air` paints open
  air as OLX does, else only rock and dirt: the floating gib pixels)
  change only colour.
  `take_dirty()` drains art-px rectangles covering every change (at most
  `MAX_DIRTY`, merged beyond). `rebuild_area(area, &material)` is the only
  fallback for edits a tick dropped (`lost_area`). `replace_rect(r, front,
  back, material)` (the level editor's canvas, plan 14c) copies art
  rectangle `r` of new art (front into the composite too) and the world
  cells under it from the level's cells (`&[Material]`), reading only the
  rectangle, then marks it dirty; images or a material of another size
  change nothing, `r` is clipped. The image is always
  opaque; odd art sizes are cropped or padded, never a panic.
- **Glow** (plan 15a, look system §Glow): `set_glow(Some(GlowLayers))` gives the map
  a glow channel (strength per art pixel, group per world pixel, of the front
  as loaded and of the backdrop); layers of another size are refused
  (`false`). Edits keep it in step: a carve reveals the back's glow and
  group, the rim keeps the pixel's; placed and green dirt glow none; stains
  keep glow; `rebuild_area` takes back (background), none (dirt) or front
  (rock). `glow()` and `glow_groups()` are the composites;
  `replace_glow_rect` is `replace_rect`'s glow (the editor). Without glow
  nothing changes and nothing is spent. `GlowLayers` (`olr-format`
  re-exports it as the level's four `look/` images) holds `art` and
  `world` sizes, `front` and `back` (strength 0-255 per art pixel) and
  `groups` and `groups_back` (one byte per world pixel); `empty(art,
  world)` glows nowhere, `fits(art, world)` checks the sizes and every
  buffer's length. The colour map never reads a group's meaning: what a
  value names (0 none, `v` the look's `glow_groups[v - 1]`) is the
  renderer's.
- **Clear solids** (plan 15a Task 14b, spec §Clear solids):
  `set_clear(Some(flags))` gives the level's clear flags, one byte per
  world pixel, row-major (a solid cell whose flag is not 0 lets light
  through); flags of another size are refused (`false`), `None` removes
  them. `clear()` returns them as loaded: play edits never change them, and
  a flag counts only where the cell is solid.
- **Journal** (`journal.rs`, networking spec §3.3): off by default; `start_journal` records each mutator's clipped pre-image and keeps the order-independent cell hash. Every new write to `cells` must go through `set_cell` after `record`, or rollback breaks.
- **Restored areas**: `undo_to` reports each undone edit's rectangle and a `load_cells` the whole map; `take_restored_areas()` drains them (`Area`, world px, at most `MAX_RESTORED`, merged into the bounding box beyond), so the colour map, which only replays the ticks' edits, can `rebuild_area` what an undo or a load changed.
- **State transfer**: `cells()` and `load_cells` (all-or-nothing, valid materials only). A load is not undoable: it seals the journal, so every older mark is `Forgotten`.

- **Tests** (`tests/`, one binary since plan 15a Task 4: `journal.rs`,
  `glow.rs`; unit tests in the modules): `glow.rs` covers the channel
  rules (a new map shows the front glow, carving reveals the backdrop's,
  placed dirt glows none, stains keep glow, `rebuild_area` and
  `replace_glow_rect`), a misfit glow refused, `no_glow_costs_nothing`, the
  proptest `random_edits_keep_glow_consistent` (background shows the back
  glow, rock the front, dirt the front or none; Task 4 ruling: not equality
  with a rebuild, which gives all dirt none) and
  `clear_flags_are_the_level_size`.

Test: `cargo nextest run -p olr-terrain`.
