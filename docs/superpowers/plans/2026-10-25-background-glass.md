# Background Glass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Background elements can be half-transparent, tintable, frostable glass that shows the distant layers behind it, is lit as what is behind it, and (with a new `Filtered` glow colour) casts light coloured pane by pane; Liero Factory Reloaded's windows become clear glass with a moon behind them.

**Architecture:** The level format gains `Element::glass` (opacity, tint, frost) and a baked art-size RGBA map `look/glass.png` (RGB the filter, A the frost); the bake writes glass back pixels at partial alpha, laid correctly over what is below. The colour map keeps partial alpha in see-through levels; the lit terrain gives glass pixels the distant layer's shade and no self-light; `fs_distant_lit` multiplies by the filter and offsets by the frost. A glow group's `Filtered(colour)` casts the colour × each pixel's art colour normalised to its brightest channel.

**Tech Stack:** Rust (no_std terrain, std format/edit), wgpu 30 WGSL shaders, egui 0.36 editor, Python only for the throwaway content art edits.

**Spec:** `docs/superpowers/specs/2026-10-25-background-glass-design.md`

## Global Constraints

- Any change to a level without glass draws byte for byte as before: the render goldens, `fixtures/render/lit-hashes.ron` and the look goldens hold without `bless-render`.
- A light that depends on the camera is out; frost has no time and no camera input (a hash of the world pixel only).
- Glass is a back pixel of alpha 1 to 254 in a see-through level (one with distant layers). No other marker.
- `opacity` percent 1 to 99 (required), `tint` percent 0 to 100 (default 0), `frost` tenths of a world pixel 0 to 40 (default 0).
- Glass is allowed on an Image or Drawn element of material Background, not on a distant element, not on an asset's elements; on a mixed drawn layer it applies to its background pixels only.
- Tint and frost are lit-only; glass itself draws unlit too.
- `olr-terrain` stays `#![no_std]` and builds for `wasm32-unknown-unknown`; never panic on content (a misfit `look/glass.png` is left out with a warning in lenient reads).
- Run `cargo xtask check` before every commit (`--render` for the renderer task); small imperative commit subjects (`olr-render: glass takes the light behind it`).
- No OLX counterpart: record the decisions in `docs/porting/STATUS.md` ("Background glass"), not as OLX citations.
- Ask the maintainer before `cargo xtask full`, `bench` or `stress`.

## Review Focus

- A glass element over another opaque background element: the result must be an opaque tinted pixel (no see-through hole); the bake test pins it (Task 3).
- Glass under dirt that is dug in play: the dug pixel shows the glass at its alpha, not opaque and not a hole (Task 2's carve test).
- Blood or gibs landing on glass: they never land (Task 2's stain test); `on_air` gibs too.
- A hostile `look/glass.png` (wrong size, wrong colour type): lenient reads drop it with a warning, strict reads refuse it, nothing panics (Task 1).
- Two views (split screen) over glass with frost: each view's frost is the same per world pixel (no per-view drift); Task 4's frost test draws two camera positions and compares the glass pixels' pattern relative to the world.

---

### Task 0: Branch and spec

**Files:** `docs/superpowers/specs/2026-10-25-background-glass-design.md`, `docs/superpowers/plans/2026-10-25-background-glass.md`

- [ ] **Step 1:** `git checkout -b glass main`
- [ ] **Step 2:** Commit the spec and this plan: `git add docs/superpowers/specs/2026-10-25-background-glass-design.md docs/superpowers/plans/2026-10-25-background-glass.md && git commit -m "docs: background glass design and plan"`

### Task 1: Format: `Element::glass`, `look/glass.png`, `Filtered` glow colour

**Files:**
- Modify: `crates/olr-format/src/levels/composition.rs` (Element, `check_elements`)
- Modify: `crates/olr-format/src/levels/mod.rs` (`GLASS_FILE`, `LevelSource::glass`, `file_paths` list at `:473`)
- Modify: `crates/olr-format/src/levels/files.rs` (read/write/lenient read like `look/clear.png`)
- Modify: `crates/olr-format/src/look.rs` (`GlowColourSource::Filtered`, compile)
- Modify: `crates/olr-terrain/src/color.rs` or a new `crates/olr-terrain/src/glass.rs` (`GlassMap`)
- Modify: `crates/olr-look/src/lib.rs` (`GlowColour::Filtered`)
- Modify: every `LevelSource { .. }` and `Element { .. }` literal the compiler names (`glass: None`)
- Modify: `crates/olr-cli/src/look_cmd.rs` (`parse_glow_colour`: `filtered:#rrggbb`)
- Modify: `docs/formats/level.md` (Element `glass`, ElementGlass, `look/glass.png`, `back.png`'s alpha rule "0 see-through, 1 to 254 glass, 255 opaque", GlowColourSource `Filtered`)
- Test: `crates/olr-format/tests/roundtrip.rs`, `crates/olr-format/tests/look.rs`, `crates/olr-format/tests/hostile.rs`

**Interfaces:**
- Produces:
  - `olr_format::levels::ElementGlass { pub opacity: u8, pub tint: u8, pub frost: u8 }` (serde: `tint`, `frost` default 0 and skipped when 0; `deny_unknown_fields`); `Element::glass: Option<ElementGlass>` (default none, skipped when none); `pub const MAX_GLASS_FROST: u8 = 40`.
  - `olr_terrain::GlassMap { pub art: (u32, u32), pub texels: Vec<u8> }` (RGBA8 row-major, `art.0 * art.1 * 4` bytes; RGB the filter, 255 none; A the frost in tenths), `GlassMap::fits(&self, art: (u32, u32)) -> bool`, `GlassMap::none(art) -> GlassMap` (white, frost 0). Re-exported by `olr_format::levels`.
  - `olr_format::levels::GLASS_FILE: &str = "look/glass.png"`; `LevelSource::glass: Option<GlassMap>`.
  - `olr_format::look::GlowColourSource::Filtered(Colour)` (opaque); `olr_look::GlowColour::Filtered(Rgb)`.

- [ ] **Step 1: Write the failing tests**
  - `roundtrip.rs`: `glass_round_trips`: an Element with `glass: Some(ElementGlass { opacity: 35, tint: 0, frost: 15 })` writes `glass: (opacity: 35, frost: 15)` and reads back equal; a level with `glass: Some(GlassMap)` of its art size writes `look/glass.png` and reads back equal; `glass: None` writes no file.
  - `look.rs`: `glass_out_of_range_is_refused`: opacity 0 and 100, tint 101, frost 41 each fail `check_elements` with the field path in the message; `glass_needs_a_plain_background_element`: on a Rock image, a Dirt drawn layer, a distant element and a Raster each fail; on a mixed drawn layer it passes. `filtered_glow_colour_compiles`: `Filtered("#c8d4f0")` compiles to `GlowColour::Filtered([200, 212, 240])`; one with alpha fails like `Tint`.
  - `hostile.rs`: `a_misfit_glass_map_is_dropped_when_playing`: a `look/glass.png` of the wrong size fails `read_level_files` and `read_level_files_lenient` returns the level with `glass: None` and a warning containing "plays without its glass".
- [ ] **Step 2:** `cargo nextest run -p olr-format` → the new tests fail to compile.
- [ ] **Step 3:** Implement the types, the checks in `check_elements` (`glass` on `Image|Drawn` with `material: Background`, `distant: None`; ranges as Global Constraints), `read_glass(src, art)` (RGBA PNG through `image_io`, size checked against the art size before decoding, as `read_art`), the writer (`encode_rgba` of `texels`), the lenient read's warning `"{e}: the level plays without its glass"`, and `Filtered` in `compile` (as `Tint`, `rgb(c, …)`).
- [ ] **Step 4:** `cargo nextest run -p olr-format -p olr-terrain -p olr-look` → PASS.
- [ ] **Step 5:** `cargo xtask check`; commit `olr-format: glass elements, look/glass.png and filtered glow`.

### Task 2: Terrain: partial alpha, glass map on the colour map

**Files:**
- Modify: `crates/olr-terrain/src/color.rs` (`new_see_through` `:227-254`, `replace_rect` `:528-535`, `stain_block` `:668-680`, `stain_image`'s alpha checks, `set_glass`/`glass`)
- Modify: `crates/olr-content/src/color.rs` (`color_map` sets the glass)
- Modify: `crates/olr-game/src/editor/canvas.rs` (`:176` and `:292`: `map.set_glass(l.glass.clone())`)
- Modify: `crates/olr-terrain/CLAUDE.md`
- Test: `crates/olr-terrain/tests/see_through.rs`

**Interfaces:**
- Consumes: `GlassMap` (Task 1).
- Produces: `ColorMap::set_glass(&mut self, glass: Option<GlassMap>) -> bool` (false and unchanged for a misfit), `ColorMap::glass(&self) -> Option<&GlassMap>`. A see-through map keeps every back alpha below 255.

- [ ] **Step 1: Write the failing tests** in `see_through.rs`:
  - `glass_keeps_its_alpha`: back pixel alpha 90 over background → `back`, `front` and `pixels()` alpha 90; over a rock cell the front stays opaque.
  - `digging_in_front_of_glass_shows_the_glass`: dirt cell over a back pixel of alpha 90; carve it; the composite pixel is the back's RGB at alpha 90.
  - `stains_skip_glass`: `stain` and `stain_image` (with `on_air`) over glass pixels leave them unchanged.
  - `replace_rect_keeps_glass`: as `glass_keeps_its_alpha` through `replace_rect`.
  - `a_plain_map_stays_opaque`: `ColorMap::new` over the same art gives alpha 255.
  - `a_misfit_glass_map_is_refused`: `set_glass` of another art size returns false, `glass()` stays `None`.
- [ ] **Step 2:** `cargo nextest run -p olr-terrain` → FAIL (alpha 255; no `set_glass`).
- [ ] **Step 3:** Implement: `new_see_through` copies the back alpha when below 255 (0 as before); `replace_rect` keeps `p[3]` when `keep && p[3] < 255`; `stain_block` writes only where `p[3] == 255`, and `stain_image` likewise; `set_glass`/`glass` as `set_clear`.
- [ ] **Step 4:** `cargo nextest run -p olr-terrain -p olr-content`; `cargo build --target wasm32-unknown-unknown -p olr-terrain` → PASS.
- [ ] **Step 5:** `cargo xtask check`; commit `olr-terrain: keep glass's partial alpha`.

### Task 3: Bake: glass pixels, the glass map, the `Glass` property

**Files:**
- Modify: `crates/olr-edit/src/compose.rs` (`Layer` carries the element's glass; `back_at` `:464`; the pixel loop `:840-875`; `level.glass`; `stale` covers `look/glass.png`)
- Modify: `crates/olr-edit/src/level.rs` (`over` `:460`: a destination of alpha 0 gives no colour; `resize` re-lays `glass` as it re-lays the glow, the new area white and 0)
- Modify: `crates/olr-edit/src/element.rs` (`Property::Glass(Option<ElementGlass>)`, `field` name `"glass"`, `apply`: Raster refused; the composition check refuses the rest)
- Modify: `crates/olr-edit/CLAUDE.md`
- Create: `crates/olr-edit/tests/glass.rs` (a `mod glass;` in `tests/integration.rs`)

**Interfaces:**
- Consumes: Task 1's types.
- Produces: `olr_edit::element::Property::Glass(Option<ElementGlass>)`; baked `LevelSource::glass` (`Some` only when some glass has tint or frost above 0).

- [ ] **Step 1: Write the failing tests** in `tests/glass.rs` (a 16 × 8 level at art scale 2 with one distant element, built as `tests/distant_glow.rs` builds its levels):
  - `glass_bakes_at_its_opacity`: a Background drawn layer with `glass: (opacity: 35)` over nothing → its back pixels' alpha `round(35 × 255 / 100)` = 89, RGB its art; front equals back there.
  - `glass_over_an_opaque_element_is_opaque`: the same over a full Background layer → alpha 255, RGB `over(glass, below)` (straight alpha, rounded).
  - `glass_over_see_through_takes_no_tile_colour`: RGB exactly the glass art (the old `over` mixed the tile colour in).
  - `the_filter_and_frost_bake_per_art_pixel`: `glass: (opacity: 35, tint: 100, frost: 15)` with art `(200, 100, 50)` → `level.glass` texel `(200, 100, 50, 15)`; tint 50 → `(228, 178, 153, 15)` (`255 + (g − 255) × t / 100`, rounded); uncovered pixels `(255, 255, 255, 0)`; the topmost glass wins.
  - `clear_glass_writes_no_map`: tint 0 and frost 0 → `level.glass` is `None`, no `look/glass.png` written.
  - `glass_does_not_carry_distant_glow`: a glowing distant layer behind a glass pixel → its `glow.back` and `groups_back` are the glass element's own (0 when it does not glow), not the layer's.
  - `glass_in_a_level_without_distant_layers_is_opaque`: alpha 255.
  - `the_glass_property_sets_and_refuses`: `element::set(.., Property::Glass(Some(..)))` on a Background drawn layer sets it; on a Rock layer `BadValue`; on a raster `WrongKind`; `stale` lists `look/glass.png` after a tint change until baked.
- [ ] **Step 2:** `cargo nextest run -p olr-edit glass` → FAIL.
- [ ] **Step 3:** Implement. `back_at` walks the layers from the top: glass layers' backs are collected until the first opaque back (or none: the tile, alpha 0 when see-through, else opaque); the result is the collected glass laid over it bottom-up with the corrected `over`, each glass pixel at its element's alpha. The glass map takes, per art pixel, the topmost glass layer whose `back()` covers it. Bake the element's own glow as any background element's (its glow is how a window casts).
- [ ] **Step 4:** `cargo nextest run -p olr-edit` → PASS (every bundled level still bakes byte for byte: `compose.rs`' round trips).
- [ ] **Step 5:** `cargo xtask check`; commit `olr-edit: bake glass elements`.

### Task 4: Renderer: glass light, filter, frost

**Files:**
- Modify: `crates/olr-render/src/renderer/look.rs` (`LookTextures`: a `glass` `Rgba8Unorm` texture at art size, cropped like the glow by `look_texture_sizes`, written at `upload_terrain` and on each dirty rectangle, a 1 × 1 white `no_glass` when the map has none; binding 4 of the look layout (`layout`, group 2); `LookGlobals.distant.w` = 1 when the map has glass, else 0)
- Modify: `crates/olr-render/src/look/world_lit.wgsl` (`terrain_lit`, `fs_distant_lit`)
- Modify: `crates/olr-render/src/look/common.wgsl` (`distant` comment: w is the glass flag)
- Modify: `crates/olr-render/src/look/gi_emission.wgsl`, `crates/olr-look/src/frame.rs`, `crates/olr-render/src/renderer/look.rs:1013` (`Filtered`: `info.y` 2)
- Modify: `crates/olr-format/src/levels/thumb.rs:225` (`Filtered` as the emission)
- Modify: `crates/olr-render/CLAUDE.md`, `crates/olr-look/CLAUDE.md`
- Create: `crates/olr-render/tests/gpu/glass.rs` (`#[path = "gpu/glass.rs"] mod glass;` in `tests/gpu.rs`)
- Test: `crates/olr-look/tests/frame.rs`

**Interfaces:**
- Consumes: `ColorMap::glass()` (Task 2); `GlowColour::Filtered` (Task 1).
- Produces: `olr_look::FrameGroup::filtered: bool` (with `own_colour: true` and `cast` the tint × k).

- [ ] **Step 1: Write the failing tests.**
  - `olr-look/tests/frame.rs`: `a_filtered_group_casts_its_tint_by_the_art`: a group `Filtered([200, 212, 240])` at intensity 220 → `own_colour && filtered`, `cast` = `srgb_to_linear` of the tint × 2.2.
  - `tests/gpu/glass.rs` (a see-through level with one opaque white distant layer at depth 90 and a back strip of glass pixels; `look_frame` with a bright lamp group beside the strip; build as `distant_glow.rs` builds its level):
    - `glass_shows_the_layer_through_it`: a glass pixel of alpha 89 and colour `(200, 215, 235)` over the layer draws (full bright view) as the straight-alpha mix of the two, within 1 per channel.
    - `glass_takes_the_layers_light_not_the_rooms`: lit, the lamp lights an opaque back pixel next to the glass strongly; the glass pixel's light is the layer's (`1 − depth` of the light beyond the ambient), within 2 per channel of the same pixel with the lamp off.
    - `glass_shows_no_glow_but_casts`: a glass pixel with glow 255 in a group: its drawn colour equals the no-glow case; the light grid beside it is brighter than with glow 0.
    - `hot_glass_still_glows`: a glass pixel at a temperature on its material's ramp draws brighter than cold (its heat light at its alpha).
    - `tint_multiplies_what_shows_through`: a glass map texel `(235, 165, 50, 0)` over the white layer draws (full bright) the filter's colour blended under the glass at its alpha; over a blue layer `(30, 60, 140)` the result's red and green are below the white case's (dark olive).
    - `frost_is_fixed_to_the_world`: frost 15 over a striped layer; two frames at one camera give equal bytes; frames at cameras (100, 50) and (104, 50) give, at a world pixel of glass, texels displaced by at most `1.5 × art_scale` from the unfrosted fetch, and two runs at each camera equal.
    - `a_filtered_group_lights_by_each_pixels_colour`: two pixels of one `Filtered(white)` group, one red `(255, 0, 0)` and one dark red `(128, 0, 0)`: the light beside each is red and of the same strength (normalised).
    - `no_glass_map_changes_no_byte`: a see-through level with opaque backs draws equal bytes with `set_glass(None)` and with `GlassMap::none` (white, frost 0); today's bytes themselves are pinned by the goldens and lit hashes (Step 5).
- [ ] **Step 2:** `OLR_GPU_TESTS=1 cargo nextest run -p olr-render glass` and `cargo nextest run -p olr-look` → FAIL.
- [ ] **Step 3: Implement.**
  - `terrain_lit` (not the backing): with `look.distant.x > 0` and `0 < c.a < 1` the pixel is glass: self-light scale 0 (its glow casts, never shows), `out.shade.a = 0` (the shade stays the layer's), heat target as today (blended at `c.a`).
  - `fs_distant_lit`: with `look.distant.w > 0`, read the glass texel at the art pixel under the texel (`floor((look.frame.xy + in.pos.xy / look.frame.z) × look.level.z)`); frost `f = a × 255 / 10` world px: offset the layer texel by `f × (2 × hash2(world pixel) − 1) × art scale`, clamped to `in.rect`; multiply the colour's RGB by the filter's RGB. `hash2` is the spike's (`fract` of a dot of the cell with `0.1031`, second axis offset `(17, 3)`), in `world_lit.wgsl`.
  - `Filtered`: `FrameGroup::filtered`; the packed `info.y` 2; the emission takes the art colour divided by its brightest channel (none when 0) for `info.y > 1.5`, the art colour for `> 0.5`.
- [ ] **Step 4:** `OLR_GPU_TESTS=1 cargo nextest run -p olr-render`; `cargo nextest run -p olr-look -p olr-format` → PASS.
- [ ] **Step 5:** `cargo xtask check --render` (goldens, HUD snapshots, lit hashes unchanged: no re-bless); commit `olr-render: glass takes the light behind it, tints and frosts`.

### Task 5: Level editor: the Glass section and the Filtered colour

**Files:**
- Modify: `crates/olr-editor/src/ops.rs` (`with_property`: `Property::Glass`, a Raster refused)
- Modify: `crates/olr-editor/src/ui/properties.rs` (a `glass(ui, state, data, e, out)` after `distant(..)`, for a Background, plain or mixed, non-distant element)
- Modify: `crates/olr-editor/src/ui/look.rs` (the group colour control `:320`: Own / Tint / Filtered, the latter with its `#rrggbb` field)
- Modify: `crates/olr-editor/src/ui/labels.rs` (`GLASS`, `OPACITY`, `TINT`, `FROST`, `GLASS_HINT`, `GLASS_LOOK_HINT`, `FILTERED`)
- Modify: `crates/olr-editor/CLAUDE.md`
- Test: `crates/olr-editor/tests/flows.rs`

**Interfaces:**
- Consumes: `Property::Glass` (Task 3), `GlowColourSource::Filtered` (Task 1).

- [ ] **Step 1: Write the failing test** `flows.rs` `make_an_element_glass`: open a level with a distant layer, select a Background drawn layer, tick `labels::GLASS` → the element has `glass: (opacity: 35)` (`ops::DEFAULT_GLASS_OPACITY` 35) in one undo step; drag Opacity, Tint, Frost (committed on release) → the values land, one step each; the canvas's bake has alpha 89 there; a level without a distant layer shows `labels::GLASS_HINT`; without a look, `labels::GLASS_LOOK_HINT` under Tint and Frost. `a_group_casts_filtered`: the group colour switch's Filtered with `#c8d4f0` sets `GlowColourSource::Filtered`.
- [ ] **Step 2:** `cargo nextest run -p olr-editor make_an_element_glass a_group_casts_filtered` → FAIL.
- [ ] **Step 3:** Implement with the existing `slider` helper (Opacity 1-99 %, Tint 0-100 %, Frost 0-40 shown as `0.0` to `4.0 px`), hints in `warn_fg_color` as `DISTANT_HINT`; a locked element's sliders hidden as Glow's.
- [ ] **Step 4:** `cargo nextest run -p olr-editor` → PASS.
- [ ] **Step 5:** `cargo xtask check`; commit `olr-editor: glass and filtered glow in the properties`.

### Task 6: Liero Factory Reloaded's windows, docs

**Files:**
- Modify: `content/reloaded/olr-level-liero-factory-reloaded/` (`editor/layers/glass.png`, `editor/layers/background.png`, `editor/images/sky.png`, `editor/composition.ron`, `level.ron`, the baked files)
- Modify: `content/reloaded/README.md`, `crates/olr-cli/tests/reloaded.rs`
- Modify: `docs/porting/STATUS.md` ("Background glass": the spec's decisions and the planning calls: art-size filter map, tint and frost lit-only, editor hint instead of a check warning)

- [ ] **Step 1:** With a script in the session scratchpad (Python, PIL), edit the art:
  - `glass.png`: every pixel's RGB mixed halfway with `(200, 215, 235)`; in the hall window's fifth pane (world x 799-817, y 151-172) erase the jagged hole `(y − 160) > (x − 799) × 0.9 + 2 sin(1.7 x)` (the spike's).
  - A new drawn element `glass-rim` above `glass`: the 3 × 3 ring around the hole within the glass, colour `(215, 225, 240)`, `glass: (opacity: 82)`, glow and group as `glass`.
  - `background.png`: erase every art pixel under the glass and the hole (they show the sky).
  - `sky.png` (the original under `editor/images/`): the moon of the spike's `glass_level.py`, centred where a camera at (760, 215) sees it in the window's middle: compute the sky's reference offset from depth 90 (`DistantLayer::position`) rather than reusing the spike's baked `(646, 184)` blindly; check with a render.
- [ ] **Step 2:** `composition.ron`: `glass` gets `glass: (opacity: 35)`; `level.ron`'s group `glass` becomes `colour: Tint("#c8d4f0"), intensity: 220, reach: Pixels(160)` and the element's glow strength 67; `olr-cli level bake content/reloaded/olr-level-liero-factory-reloaded`.
- [ ] **Step 3:** `olr-cli look-preview content/reloaded/olr-level-liero-factory-reloaded --at 760,215 --out <scratchpad>/factory-glass.png` and at x 700 and 820; compare with the spike's `full-ripple.png` (moonlight in the hall, the moon in the window at 760, out of it at 700/820). Tune the group's intensity if the hall reads darker than the spike.
- [ ] **Step 4:** Update `reloaded.rs`'s expectations (the factory's changed art: the windows' pixels are glass, alpha below 255) and the README's note; `cargo nextest run -p olr-cli reloaded` → PASS.
- [ ] **Step 5:** `cargo xtask check`; commit `content: Liero Factory Reloaded's windows are glass under a moon`.

### Task 7: Finish

- [ ] **Step 1:** `cargo xtask check --render` → clean.
- [ ] **Step 2:** Whole-branch review (superpowers:executing-plans' final review).
- [ ] **Step 3:** Ask the maintainer about `cargo xtask full` (rendering change) before merging; show the factory renders (Task 6 Step 3) on the comparison page.
