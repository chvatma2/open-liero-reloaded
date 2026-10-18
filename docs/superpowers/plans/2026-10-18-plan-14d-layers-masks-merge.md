# Plan 14d: Movable and Mixed Layers, Masks, Merge and Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The level editor's layer model grows, so that:
- painted layers move;
- a layer holds a material per pixel (a "mixed" layer) and a back art;
- imported pictures are erased through a mask, with a restore brush and "remove this colour";
- any layer merges into the one below it by material priority;
- any layer splits into Rock, Dirt and Background layers.

All of it works in the editor and from `olr-cli`.

**Architecture:**
- **Format** (`olr-format`). The composition gains optional fields:
  - `Drawn { at, size, mixed, back }`, with the files `<id>.material.png` and `<id>.back.png`;
  - `Image { mask }`, with the file `editor/masks/<id>.png`.
- **Bake** (`olr-edit::compose`):
  - It places a drawn layer at its box.
  - It reads a mixed layer's per-pixel material and back art.
  - It processes an image's original through its mask, cached by the mask's fingerprint.
- **Pixel operations** live in two new `olr-edit` modules, shared by the editor and the CLI:
  - `drawn`: painting and erasing a placed layer (erasing digs to the back art), `Placed`, `merge`, `split`;
  - `mask`: the mask, brush masking and the colour flood.
- **Editor** (`olr-editor`):
  - records the changes as new `Edit`s: a `Pixels` patch of art, cells and back; `Layer`, an item swap; `Mask`;
  - drives them from new tools and properties.

**Tech Stack:** Rust 1.99, egui `=0.36.2`, egui_kittest `=0.36.2`, proptest, clap.

**Spec:** `docs/superpowers/specs/2026-10-18-level-editor-polish-design.md`. This plan covers the following of it:
- decisions 3, 4, 5, 7 and 8, and calls C1-C6;
- §"Masks on image elements", §"Layers with per-pixel material", §"Moving drawn layers", §"Merge down", §"Split by material";
- §Formats (all but the texture library);
- the §"Agent operations" for merge, split and mask.

The rest of the spec comes in the plans after this one:
- **14e:** textures and the library, flood fill, the hole stamps.
- **14f:** copy and paste of elements and pixels (with `olr-cli level element duplicate`), and multi-select.

Earlier decisions are in `docs/porting/STATUS.md` ("Level composition decisions", "Level editor decisions" of 14b and 14c) and in the crates' `CLAUDE.md`.

## Global Constraints

- **Never panic on content** (repo rule 6). Composition files, masks, layer files, CLI arguments and pointer positions are untrusted. Every coordinate sum saturates or is done in `i64`.
- **Deterministic, integer-only bake** (repo rule 7, spec §Constraints). No floats in `drawn`, `mask` or the bake.
- **Layers stay as they are:**
  - `olr-format` 2;
  - `olr-edit` 3;
  - `olr-editor` 5 (depends on no `olr-ui`);
  - `olr-cli` (as today).
- **These stay byte-identical:**
  - the oracle traces;
  - the match goldens;
  - the render goldens;
  - `fixtures/ui/`;
  - `content/packs`;
  - every bundled level's round trip through three layers.
- **`fixtures/edit/composed-level` changes, once.** Task 6 adds a mixed layer with back art, a moved layer and a mask, and re-bakes it, as its README says.
- **No pack format bump.** Every new field is optional with a default, so `PACK_FORMAT_VERSION` stays 5 (ruling R1).
- **Limits:**
  - a drawn layer's box is 1 to `MAX_SIDE` (4096) world pixels per side, and its art within `LEVEL_ART`;
  - a mask equals its oriented original's size, within `IMAGES`;
  - the elements' decoded pixels together are within `MAX_COMPOSED_PIXELS`, masks included;
  - the history stays within `History::MAX_BYTES` (256 MiB).
- **Tolerance** is a percentage, 0 to 100. Colours `a` and `b` are similar when `max(|a.c − b.c| over r, g, b, a) × 100 ≤ tolerance × 255`.
- **Material priority** is Rock 2, Dirt 1, Background 0. On a tie, the upper layer wins (C1).
- **Verify once per batch:**
  - targeted `cargo nextest run -p <crate>` while working;
  - `rtk proxy cargo clippy -p <crate> --all-targets` before each commit;
  - the online `cargo xtask check` at each batch's end.
- **The batches:**
  - Batch A is Tasks 1-3 (format and bake);
  - Batch B is Tasks 4-6 (operations, CLI, fixture);
  - Batch C is Tasks 7-8 (the editor);
  - Batch D is Task 9 (docs).
- **`cargo xtask full` runs once at plan end, and only after asking the user.** It shares their PC.
- **Commits** are small, with an imperative subject (`olr-edit: …`). Each ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Review Focus

1. **Digging a merged map.**
   - Setup: open a plain level (three layers), merge Dirt into Background, then erase dirt with the Eraser.
   - Expected: the level's own background shows there, and the material is background. Neither transparency nor the theme's tile appears.
   - Test: Task 7, `erasing_a_merged_map_digs_to_its_background`.
2. **An undone conversion leaves no trace.**
   - Setup: a stroke of another material into a plain layer, which converts it to mixed; then Undo; then Save.
   - Expected: the layer is plain again and the save writes no `<id>.material.png`.
   - Test: Task 7, `an_undone_conversion_saves_a_plain_layer`.
3. **Hand-made or hostile files.**
   - Cases: a mask of the wrong size; a material file with palette index 7; a back file of the wrong size; a box of 0 or 5000 pixels; a mask on a raster.
   - Expected: an error naming the file or field, never a panic.
   - Tests: Task 1, `check_composition_reports_each_problem` (new cases); Task 2, `a_mask_of_another_size_is_an_error_naming_it`.
4. **Layers off the level.**
   - Cases: merging an image that hangs past the level's edge, splitting it, moving a layer half off and back, and then saving and reopening.
   - Expected: pixels off the level are kept, and nothing panics. A union beyond `LEVEL_ART` is refused with a message.
   - Tests: Task 5, `a_merge_keeps_pixels_off_the_level`; Task 7, `a_layer_moved_off_and_back_is_unchanged`.
5. **A moved layer paints in step with the world.**
   - Setup: the theme's front tile painted into a layer moved by `(3, 5)`.
   - Expected: each world pixel takes the same texel as in an unmoved layer. A level resize or an art-scale change keeps a moved layer where it was.
   - Tests: Task 4, `tiles_stay_aligned_on_a_moved_layer`; Task 3, `a_resize_moves_a_boxed_layer_and_relays_a_level_sized_one`.

## Rulings made while writing this plan

- **R1 No format bump.**
  - Every new composition field has a default and is left out when default, so every composition written today reads unchanged.
  - A composition using the new fields does not open in an older build. This is fine at this stage (compatibility breaks are fine).
  - `content/packs` therefore stays byte-identical.
- **R2 The drawn layer's box.** `at` (world pixels, default `(0, 0)`) and `size: Option<(u32, u32)>`:
  - `None` is the level's size, and follows a level resize (re-laid as today).
  - `Some` is a box of its own: a resize moves it by the offset, as it moves an image.
  - A new layer is `None`. A merge or split result is `None` exactly when its box is the level at the origin.
- **R3 A drawn layer's decoded data:**
  ```rust
  Decoded::Drawn { art: RgbaImage, cells: Option<Vec<Material>>, back: Option<RgbaImage> }
  ```
  - `art` is box × art scale, and alpha 0 means not covered.
  - `cells` is one per world pixel of the box. It is `Some` exactly when the element is `mixed`.
  - `back` is box × art scale. It is `Some` exactly when the element has `back`.
  - The element's flags and the data change together. Only `Edit::Layer` (a whole item swap) turns a plain layer mixed or back.
  - A plain layer's material is its element's.
- **R4 Bake of a drawn layer** (extends plan 14a R3-R5):
  - it covers an art pixel inside its box where `art` alpha is not 0;
  - its material at a covered block's centre is `cells[local]`, else its element's material;
  - its backdrop at an art pixel is its art where the pixel is covered and the block's material is background, else its back art where that is opaque, else none.
- **R5 Erasing digs.** An eraser block of a mixed layer whose cell is dirt or rock, and whose back art has an opaque pixel in that block, becomes background:
  - the block's art becomes the back art;
  - the block's back art is cleared.

  Every other erased block is cleared (alpha 0). Erasing again clears the dug background, as erasing the Background layer does today. A plain layer with back art is converted to mixed when an erase stroke begins (`begin_stroke`). This carries decision 7's promise into the editor: erasing dirt on a merged map shows the level's background, as erasing the Dirt layer does today.
- **R6 Painting another material.**
  - The brush's material ("Paint as": the layer's, Rock, Dirt or Background) is set on each world pixel whose block the stroke painted.
  - A stroke of another material into a plain layer first converts it to mixed, in the same history step (`begin_stroke`).
  - Painting does not change back art.
- **R7 Masks:**
  - a grey PNG, `editor/masks/<id>.png`, 0 erased and anything else kept (written as 0 and 255);
  - the oriented original's size;
  - applied before `process_grid` (masked pixels transparent);
  - the grid cache keyed by the mask's fingerprint as well;
  - mapping: a brush masks original pixel `(ox, oy)` when the world pixel holding its centre, `at + ⌊(2·ox + 1) · box / (2 · original)⌋` per axis, is covered by the shape;
  - one mask stroke reprocesses the grid on each dab (`mask_dab_speed` measures it; a cheaper preview is a follow-up if it is slow);
  - the first mask stroke on an unmasked image makes a full mask (an `Edit::Layer`, in the stroke's step).
- **R8 Remove this colour** (the Wand tool, W):
  - a click floods the oriented original from the clicked pixel, 4-neighbour;
  - the flood takes pixels similar to the clicked pixel's colour (tolerance, default 10 %), whatever their mask state;
  - it masks them;
  - it is one step.
- **R9 Selecting and moving drawn layers.** Select's press tests, from the top down:
  - an image's box (as today);
  - a visible drawn layer's covered pixel (its art alpha at the pointer's block centre).

  A press on a drawn layer that was already selected starts a move. A press on a drawn layer that was not selected only selects it, so a click on the map never drags a whole level-sized layer by accident. Arrow keys nudge the selected image or drawn layer. Drawn layers have no resize handles. The selected drawn layer's box is drawn as an `Overlay::Box`.
- **R10 Merge down** (`id` merges into the element directly below it):
  - The lower element must be a drawn layer, and both must be visible and unlocked; otherwise the merge is refused with the reason.
  - The upper element may be an image (its processed grid), a drawn layer or a raster.
  - The result keeps the lower element's id, name, place in the stack and material. It is mixed when any covered cell differs from that material, and has back art when any back pixel is opaque.
  - It is one step: `Remove` of the upper element, then `Layer` of the lower one. Ctrl+E merges the selection down.
- **R11 Split** applies to any visible, unlocked element:
  - an image (its processed pixels), a drawn layer (plain or mixed) or a raster;
  - each output keeps the source's box and is plain;
  - bottom to top, Background, Dirt, Rock;
  - an output with no opaque pixel is left out;
  - each output's id comes from `new_element_id` over its name;
  - it is one step.

  A raster's split equals `split_layers` except that empty outputs are left out. `split_from` (a plain level opened) keeps all three layers, so `opening_and_saving_changes_nothing` holds.
- **R12 History of masks.**
  - A mask is `Arc<Mask>` in `Decoded::Image`, and edited copy-on-write.
  - `Edit::Mask` holds the changed rectangle of original pixels, before and after: the bounding box of the bytes that differ.
  - A `Layer` edit's masks are counted in full by `Edit::bytes`; they are not shared the way grids are. An overcount only shortens the history.
- **R13 The new tool options sit at the drawing row's end.** A tool's options show only while that tool is chosen, so the controls before them never move:
  - "Paint as" for the painting tools;
  - "Restore" for the Eraser;
  - "Tolerance" for the Wand.

  The row must still fit 1280 points (`the_toolbar_stays_in_the_window_and_in_place`).

---

### Task 1: The composition's new fields and files (`olr-format`)

**Files:**
- Modify: `crates/olr-format/src/levels/composition.rs` (the types, `element_files`, `check_elements`, `check_composition`, `check_file`).
- Test: its unit tests.

**Interfaces:**
- Produces:
  ```rust
  ElementKind::Drawn {
      material: ElementMaterial,
      /// The layer's top-left, world pixels; may lie outside the level. Default (0, 0).
      #[serde(default, skip_serializing_if = "is_origin")] at: (i32, i32),
      /// The box's width and height, world pixels, each 1 to MAX_SIDE; none: the level's size.
      #[serde(default, skip_serializing_if = "Option::is_none")] size: Option<(u32, u32)>,
      /// A material per world pixel in `editor/layers/<id>.material.png`.
      #[serde(default, skip_serializing_if = "d::is_false")] mixed: bool,
      /// A back art in `editor/layers/<id>.back.png`: what shows where its dirt or rock is carved.
      #[serde(default, skip_serializing_if = "d::is_false")] back: bool,
  }
  ElementKind::Image { /* as today */ ...,
      /// A mask over the original in `editor/masks/<id>.png` (0 erased).
      #[serde(default, skip_serializing_if = "d::is_false")] mask: bool }
  pub fn drawn_files(id: &str, mixed: bool, back: bool) -> Vec<String>; // layer_file, then .material.png, .back.png
  pub fn mask_file(id: &str) -> String;                                  // "editor/masks/<id>.png"
  pub fn drawn_box(kind: &ElementKind, level: (u32, u32)) -> Option<((i32, i32), (u32, u32))>; // Drawn only
  ```
  - `element_files(e)` is as follows:
    - an image: its original, then its mask file when `mask`;
    - a drawn layer: `drawn_files`;
    - a raster: as today.
  - `check_composition`'s expected sizes:
    - a drawn layer's art and back: its box × art scale;
    - its material: its box;
    - a mask: within `IMAGES`, by its header (its exact size is checked when decoding, Task 2).

- [ ] **Step 1: Write the failing tests** (in `composition.rs`'s tests).
  - Extend `sample()` with a mixed, backed drawn layer `wall`:
    - `at: (1, -1)`, `size: Some((3, 2))`;
    - files: art 6 × 4, material 3 × 2, back 6 × 4.
  - Give the image `sky` `mask: true`, with a 10 × 10 grey mask file.
  - `composition_round_trips_canonically` must pass with these.
  - Add one test, `defaults_are_left_out_of_drawn_and_image`: a plain drawn layer writes `Drawn(material: Dirt)` with no `at`, `size`, `mixed`, `back` or `mask` in the text.
  - Add these cases to `check_composition_reports_each_problem`, each matched by the substring after the arrow:
    - a drawn `size: Some((0, 2))` → `"size"`;
    - `Some((5000, 2))` → `"size"`;
    - `wall`'s material file 2 × 2 → `"editor/layers/wall.material.png"`;
    - its back file 4 × 4 → `"editor/layers/wall.back.png"`;
    - `mixed: true` with the material file removed → `"missing"`.
  - `a_drawn_layers_files_never_collide`: a drawn `a` with `mixed: true` beside a drawn `a.material` gives `"two elements"`.
  - `the_elements_pixels_together_are_bounded` grows one case: a drawn layer's budget counts:
    - its box's art;
    - plus its back art when `back`;
    - plus its box's cells when `mixed`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-format composition`. Expected: FAIL (no fields `at`, `mixed`, `mask`).
- [ ] **Step 3: Implement the fields, `drawn_files`, `mask_file`, `drawn_box`, the `element_files` change and the checks.**
  - Each new field gets a doc comment, which is the schema docs' source.
  - A drawn `size` is checked like an image's: each side within `MAX_SIDE`, and its art within `LEVEL_ART`.
  - Mask files are checked by header within `IMAGES`.
  - `check_composition` adds each mask's header pixels to the composed-pixels total and compares it again with `MAX_COMPOSED_PIXELS`.
  - Fix every `ElementKind::Drawn { material }` pattern in the workspace to `Drawn { material, .. }`, and every `Image { … }` constructor (`add_image`, tests, `olr-cli`) to set `mask: false`. Run `cargo build --workspace --all-targets` to find them.
- [ ] **Step 4:** Run `cargo nextest run -p olr-format` and `cargo xtask format-docs`. Expected: PASS; `docs/formats/` regenerated with the new fields.
- [ ] **Step 5:** Commit: `olr-format: movable, mixed and backed drawn layers, image masks`.

### Task 2: Masks and the masked grid (`olr-edit`)

**Files:**
- Create: `crates/olr-edit/src/mask.rs` (and `pub mod mask;` in `lib.rs`).
- Modify: `crates/olr-edit/src/compose.rs` (`ImageCache::grid`, `Decoded::Image`, `decode_element`, `encode_decoded`).
- Modify: `crates/olr-edit/src/level.rs` (expose `pub(crate) fn covers(shape: &Shape, px: i64, py: i64) -> bool`, the extent test plus `inside`; and `pub fn similar`).
- Test: `crates/olr-edit/tests/mask.rs` (a `mod mask;` of `tests/integration.rs`) and `tests/compose.rs`.

**Interfaces:**
- Produces:
  ```rust
  // olr_edit::mask
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Mask { pub size: (u32, u32), pub keep: Vec<u8> } // row-major; 0 erased
  impl Mask {
      pub fn full(size: (u32, u32)) -> Mask;
      /// `original` with erased pixels transparent; `None` when the sizes differ.
      pub fn apply(&self, original: &RgbaImage) -> Option<RgbaImage>;
      pub fn fingerprint(&self) -> u64; // never 0 (0 is "no mask")
      pub fn encode(&self) -> Vec<u8>;  // grey PNG, 0 and 255
      pub fn decode(file: &str, bytes: &[u8], original: (u32, u32)) -> Result<Mask, ComposeError>;
      /// The bounding box (original pixels) of the bytes that differ from `other`; `None` when equal or of another size.
      pub fn diff(&self, other: &Mask) -> Option<Rect>;
      pub fn crop(&self, r: Rect) -> Vec<u8>;
      /// Writes `bytes` (a `crop` of `r`) back; false and unchanged when it does not fit.
      pub fn patch(&mut self, r: Rect, bytes: &[u8]) -> bool;
  }
  /// Masks (`keep` false) or restores the original pixels whose centre's world pixel `shape` covers,
  /// for an image box `at`, `size`. Returns the original-pixel rectangle visited, `None` when none.
  pub fn mask_shape(mask: &mut Mask, at: (i32, i32), size: (u32, u32), shape: &Shape, keep: bool) -> Option<Rect>;
  /// The original pixel under world pixel `world` for that box (`None` outside it).
  pub fn original_pixel(at: (i32, i32), size: (u32, u32), original: (u32, u32), world: (i32, i32)) -> Option<(u32, u32)>;
  /// Masks the 4-connected region from `seed` whose colours are `similar` to the seed's (R8).
  pub fn mask_colour(mask: &mut Mask, original: &RgbaImage, seed: (u32, u32), tolerance: u8) -> Option<Rect>;
  // olr_edit::level
  pub fn similar(a: Rgba, b: Rgba, tolerance: u8) -> bool; // Global Constraints' rule; > 100 is 100
  // olr_edit::compose
  Decoded::Image { grid: Option<Arc<RgbaImage>>, mask: Option<Arc<Mask>> }
  impl ImageCache {
      pub fn grid(&mut self, source: &str, original: &[u8], art_box: (u32, u32), p: &Pixelation, mask: Option<&Mask>)
          -> Result<Arc<RgbaImage>, ComposeError>; // key adds the mask's fingerprint (0 none)
  }
  ```
  - `decode_element` decodes an image's mask, and checks it against the oriented original's size, when the element has `mask`, hidden or not.
  - `encode_decoded` returns an image's mask file when it has one.

- [ ] **Step 1: Write the failing tests.**
  - In `tests/mask.rs`:
    - `a_mask_round_trips_and_applies`: a 4 × 3 mask with two erased pixels encodes, decodes, and `apply`s to a solid image with exactly those two pixels transparent. `apply` of a 5 × 3 image is `None`.
    - `mask_shape_maps_original_pixels_to_the_box`:
      - an 8 × 8 original in a 4 × 4 box at `(10, 10)`;
      - `Shape::Rect { x: 10, y: 10, w: 1, h: 1 }` erases exactly original pixels `(0..2, 0..2)`;
      - `keep: true` restores them.
    - `a_far_or_huge_shape_masks_within_the_box_only`: `Shape::Rect` at `i32::MIN` with `w: i32::MAX` returns `None`, changes nothing and never panics.
    - `mask_colour_floods_similar_connected_pixels`:
      - a 6 × 1 original `[red, red, (250,0,0), blue, red, red]` at tolerance 0 from `(0, 0)` erases `0..2`;
      - at tolerance 5 it erases `0..3`;
      - blue stops it, so `4..6` stay at both tolerances.
    - `diff_crop_and_patch_restore_a_mask`.
    - `similar_follows_the_percentage_rule`: `similar((0,0,0,255), (13,0,0,255), 5)` is true, since 13 × 100 ≤ 5 × 255 = 1275. `similar(…, (14,0,0,255), 5)` is false.
  - In `tests/compose.rs`:
    - `a_masked_image_bakes_without_its_erased_pixels`: an image with its centre pixel masked bakes background there and dirt around it.
    - `the_grid_cache_keys_masks`: the same source and settings with two different masks gives two `processed()`.
    - `a_mask_of_another_size_is_an_error_naming_it`: a 3 × 3 mask on a 4 × 4 original gives an error containing `"editor/masks/"`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit mask compose`. Expected: FAIL (no module `mask`).
- [ ] **Step 3: Implement `mask.rs`, `similar`, `covers`, and the cache and decode changes.**
  - `mask_colour` uses an explicit stack, never recursion: an original may be 4096².
  - `mask_shape` visits only the original-pixel range of the shape's extent clipped to the box.
  - Fix the `Decoded::Image { grid }` patterns (`olr-editor`, tests) to `{ grid, .. }` and constructors to `mask: None`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit` and `cargo build --workspace --all-targets`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: image masks applied before the pipeline`.

### Task 3: The bake of placed, mixed and backed layers (`olr-edit`)

**Files:**
- Modify: `crates/olr-edit/src/compose.rs` (`Decoded::Drawn`, `Layer::Drawn`, `Layer::new`, `front`, `back`, `material`, `decode_element`, `encode_decoded`, `resize_decoded`, `Decoded::bytes`).
- Modify: `crates/olr-editor/src/ops.rs` (`set_art_scale`: rescales `back` with `rescale_art`; `cells` unchanged), so the workspace builds.
- Test: `crates/olr-edit/tests/compose.rs`.

**Interfaces:**
- Consumes: Task 1's fields and `drawn_box`.
- Produces:
  - `Decoded::Drawn { art: RgbaImage, cells: Option<Vec<Material>>, back: Option<RgbaImage> }` (R3).
  - `decode_element` reads `<id>.material.png` (palette indices, each a `Material`, the box's size) and `<id>.back.png`.
  - `encode_decoded` writes `drawn_files`' files. A material file goes through `encode_material(box, cells)`.
  - `Decoded::bytes` counts `cells` and `back`.
  - `resize_decoded` follows R2:
    - a level-sized layer re-lays `art`, `cells` (new area: the element's material) and `back` (new area: transparent);
    - a boxed layer moves its `at` by the offset.
  - The bake follows R4.

- [ ] **Step 1: Write the failing tests.**
  - `a_placed_layer_bakes_at_its_box`:
    - a 6 × 4 level at art scale 2;
    - a dirt layer with `at: (2, 1)` and `size: Some((2, 2))`, fully covered;
    - material is dirt exactly at world `(2..4, 1..3)`, and front there equals the layer's art.
  - `a_mixed_layer_bakes_its_cells_and_back`:
    - a level-sized mixed layer whose cells are rock in column 0, dirt in column 1 and background in column 2, all covered, with back art blue where the cell is dirt;
    - the bake's material is per cell;
    - back is blue at column 1;
    - front equals back at column 2.
  - `a_resize_moves_a_boxed_layer_and_relays_a_level_sized_one`: `resize_decoded` with offset `(3, 1)`:
    - moves a boxed layer's `at` by `(3, 1)`;
    - re-lays a level-sized mixed layer's cells, with the new area set to its material.
  - Region proptest `region_bakes_equal_the_full_bake_and_keep_the_invariant`: `random_stack` gains:
    - drawn layers with a random `at` in `-4..4`, a random `size` (or `None`) and random `mixed` cells and `back`;
    - images with random masks.

    Raise the stack kinds from `0u8..3` to `0u8..5`: 3 is a mixed or backed drawn layer, 4 a masked image.
  - `every_bundled_level_round_trips_through_three_layers` and `the_fixture_is_baked` stay green unchanged.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit compose`. Expected: FAIL (no field `cells`).
- [ ] **Step 3: Implement R2-R4.**
  - `Layer::Drawn` carries:
    - `origin` (art px, `i64`);
    - `size` (art px);
    - `art`, `cells`, `back`;
    - the element's material;
    - the box width in world pixels (to index `cells`).
  - A layer whose data does not match its box (a caller's mistake) is left out, never a panic.
  - Fix `Decoded::Drawn { art }` patterns elsewhere (`olr-editor`, `olr-cli`) with `cells: None, back: None` or `..`, keeping behaviour.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit` and `cargo build --workspace --all-targets`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: bake drawn layers at their box, with cells and back art`.

Batch A ends: run `cargo xtask check`. Expected: no failures.

### Task 4: Painting and erasing placed layers (`olr-edit::drawn`)

**Files:**
- Create: `crates/olr-edit/src/drawn.rs` (and `pub mod drawn;`).
- Modify: `crates/olr-edit/src/level.rs`. `Paint::at` takes world art coordinates as `i64`, using `rem_euclid` for tiles. `draw_art` and `erase_art` keep their signatures and behaviour.
- Test: `crates/olr-edit/tests/drawn.rs` (a `mod drawn;` of `tests/integration.rs`).

**Interfaces:**
- Produces:
  ```rust
  // olr_edit::drawn
  /// Material priority: Rock 2, Dirt 1, Background 0.
  pub fn priority(m: Material) -> u8;
  /// Paints `shape` (world pixels) into drawn data `d` whose box is `at` (+ its art's size),
  /// clipped to the box and to a level of `level`: whole art blocks, opaque, the paint's phase
  /// from the level's art origin; `cells` (when `Some`) take `material` where a block was painted.
  /// Returns the world rectangle of the level changed; `None` when nothing (or `d` is not drawn).
  pub fn draw(d: &mut Decoded, at: (i32, i32), s: u32, level: (u32, u32), shape: &Shape, paint: &Paint, material: Material) -> Option<Rect>;
  /// Erases `shape` as R5 (digging a mixed layer's dirt or rock to its back art).
  pub fn erase(d: &mut Decoded, at: (i32, i32), s: u32, level: (u32, u32), shape: &Shape) -> Option<Rect>;
  ```

- [ ] **Step 1: Write the failing tests.**
  - `drawing_a_moved_layer_paints_where_it_shows`:
    - a 2 × 2 box at `(3, 3)` in an 8 × 8 level at art scale 1;
    - `Shape::Rect { x: 3, y: 3, w: 1, h: 1 }` paints local `(0, 0)`;
    - the result is `Some(Rect { x: 3, y: 3, w: 1, h: 1 })`;
    - a shape at `(0, 0)` returns `None`.
  - `tiles_stay_aligned_on_a_moved_layer`: a 3 × 2 tile painted over a whole level-sized layer and over a layer at `(3, 5)` gives the same colour at every shared world art pixel.
  - `painting_sets_cells_where_it_paints`: a mixed layer painted rock over a dirt area has rock cells exactly under the shape.
  - `erasing_digs_dirt_to_its_back_then_clears`:
    - a mixed layer with a dirt cell over an opaque blue back block;
    - the first erase makes the cell background, the art blue and the back clear;
    - the second erase makes the art transparent.
  - `erasing_without_back_clears`: a plain dirt layer behaves as `erase_art` does today.
  - `huge_or_far_shapes_never_panic`: draw and erase with `i32::MIN`/`i32::MAX` shapes and `at` values return `None` or a rect within the level.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit drawn`. Expected: FAIL (no module).
- [ ] **Step 3: Implement `drawn.rs`'s `priority`, `draw` and `erase`, and `Paint::at`'s world phase.** Shapes iterate the world pixels of their extent clipped to box ∩ level. Local cells are indexed in `i64` and checked.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit`. Expected: PASS (the `level.rs` proptest included).
- [ ] **Step 5:** Commit: `olr-edit: paint and erase placed and mixed layers`.

### Task 5: Merge and split (`olr-edit::drawn`)

**Files:**
- Modify: `crates/olr-edit/src/drawn.rs`.
- Test: `crates/olr-edit/tests/drawn.rs`.

**Interfaces:**
- Consumes: Task 3's `Decoded`; Task 1's `drawn_box`.
- Produces:
  ```rust
  /// A layer's pixels placed in the world: what merge and split work on.
  #[derive(Clone, Debug, PartialEq)]
  pub struct Placed {
      pub at: (i32, i32),        // world
      pub size: (u32, u32),      // world
      pub art: RgbaImage,        // size × s; alpha 0 uncovered
      pub cells: Vec<Material>,  // size, row-major
      pub back: Option<RgbaImage>,
  }
  /// A visible element as placed pixels: a drawn layer (cells from its material when plain),
  /// an image's processed grid (covered where alpha is 255, made opaque; cells its material),
  /// a raster (the level; art its front, back its back). `None` for a hidden element or data
  /// that does not match.
  pub fn placed(e: &Element, d: &Decoded, level: (u32, u32), s: u32) -> Option<Placed>;
  /// The decision-4 merge over the union of both boxes (C1, C2, decision 7; algorithm below).
  pub fn merge(lower: &Placed, upper: &Placed, s: u32) -> Placed;
  /// Background, Dirt and Rock outputs (R11), each with `p`'s box; `None` when empty.
  pub fn split(p: &Placed, s: u32) -> [Option<Placed>; 3];
  /// `p` as a drawn element's kind and data: `material` its material; `mixed` when a covered
  /// cell differs from it (uncovered cells set to it); `back` when a back pixel is opaque;
  /// `size: None` when the box is the level at the origin.
  pub fn into_layer(p: Placed, material: ElementMaterial, level: (u32, u32)) -> (ElementKind, Decoded);
  #[derive(Debug, thiserror::Error, PartialEq)]
  pub enum LayerError {
      #[error("element {0:?} is not a drawn layer: merge into a drawn layer")] NotDrawn(String),
      #[error("element {0:?} is hidden: show it first")] Hidden(String),
      #[error("element {0:?} has no pixels to work on")] NoPixels(String),
  }
  /// R10's merged element: `lower`'s id, name, visibility and material, unlocked.
  pub fn merge_down(lower: (&Element, &Decoded), upper: (&Element, &Decoded), level: (u32, u32), s: u32)
      -> Result<(Element, Decoded), LayerError>;
  /// R11's outputs, bottom to top, named "Background", "Dirt", "Rock"; `id(name, fallback)` gives each id.
  pub fn split_element(e: &Element, d: &Decoded, level: (u32, u32), s: u32, id: impl FnMut(&str, &str) -> String)
      -> Result<Vec<(Element, Decoded)>, LayerError>;
  ```
- The merge, for each world pixel of the union box:
  - Each layer *covers* the block when its art at the block's centre art pixel is opaque (outside its box: no).
  - The *winner* is the covering layer of higher `priority`, the upper one on a tie, the only covering one when one covers, and none when neither covers.
  - **Art:** each art pixel of the block is the winner's where opaque, else the other layer's. With no winner, it is the upper layer's where opaque, else the lower one's.
  - **Cell:** the winner's material; with no winner, the lower layer's cell (or Background outside its box).
  - **Back, per art pixel, the first opaque one of:**
    1. the winner's back;
    2. the loser's art, when the loser covers and its cell is Background;
    3. the loser's back.

    With no winner it is the upper layer's back, else the lower one's. It is `None` when no pixel is opaque.

- [ ] **Step 1: Write the failing tests.**
  - `merging_dirt_into_background_bakes_the_same`: for `olx-level-dirt-level`, `olx-level-castlestrike` and `olx-level-liero-factory` (skip without content):
    - make `split_from`'s three layers;
    - `merge_down(background, dirt)`;
    - the stack (merged, rock) bakes to the same material, front and back as the three layers.
  - `a_merge_follows_rock_then_dirt_then_background`:
    - on one 3 × 1 level: lower dirt `[D, D, -]` and upper rock `[R, -, R]` (`-` uncovered) give cells `[R, D, R]`;
    - the art of each comes from the layer that won it;
    - a background upper over a dirt lower keeps dirt.
  - `a_tie_goes_to_the_upper_layer`: two dirt layers of different colours give the upper colour.
  - `a_merge_keeps_pixels_off_the_level`:
    - a layer at `(-2, 0)`, size `(4, 1)`, merged onto a level-sized layer of a 3 × 1 level;
    - the result's box is `(-2, 0)`, `(5, 1)`;
    - the two off-level pixels keep their art.
  - `splitting_a_merge_gives_its_parts_back`:
    - `split` of the merged stack from the first test gives Background equal to the original background layer's art, and Dirt equal to the dirt layer's art;
    - an empty Rock output is `None`.
  - `split_of_an_image_is_its_processed_pixels`: a dirt image splits into one Dirt output whose art equals the bake's front over the box.
  - `merge_down_refuses_hidden_and_non_drawn`: `LayerError::Hidden`, and `NotDrawn` when the lower element is an image.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit drawn`. Expected: FAIL (no `merge`).
- [ ] **Step 3: Implement `Placed`, `placed`, `merge`, `split`, `into_layer`, `merge_down` and `split_element`.** Arithmetic on boxes is in `i64`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: merge layers by material priority, split any layer by material`.

### Task 6: The CLI's operations and the fixture (`olr-edit::element`, `olr-cli`)

**Files:**
- Modify: `crates/olr-edit/src/element.rs`:
  - `paint_layer` works through `decode_element`, then `drawn::draw`/`erase`, then `encode_decoded`;
  - `draw` and `erase` with material;
  - new `merge_down`, `split`, `mask`;
  - `set(Property::At)` accepted on drawn layers.
- Modify: `crates/olr-cli/src/element_cmd.rs` (subcommands).
- Modify: `fixtures/edit/composed-level/` (re-made) and `fixtures/edit/README.md`.
- Test: `crates/olr-edit/tests/element.rs`, `crates/olr-cli/tests/element.rs`.

**Interfaces:**
- Consumes: Tasks 2, 4 and 5.
- Produces:
  ```rust
  // olr_edit::element
  pub fn draw(pack: &mut Pack, id: &str, shape: &Shape, paint: &Paint, material: Option<ElementMaterial>) -> Result<(), ElementError>;
  // `material` other than a plain layer's makes it mixed first
  pub fn merge_down(pack: &mut Pack, id: &str) -> Result<(), ElementError>;
  pub fn split(pack: &mut Pack, id: &str) -> Result<Vec<String>, ElementError>; // new ids, bottom to top
  pub enum MaskOp { Shape(Shape), ColourAt { at: (i32, i32), tolerance: u8 } }
  pub fn mask(pack: &mut Pack, id: &str, op: &MaskOp, restore: bool) -> Result<(), ElementError>;
  ```
  - New CLI subcommands of `olr-cli level element`:
    - `merge-down <pack> <id>`;
    - `split <pack> <id>`, which prints the new ids;
    - `mask <pack> <id> (<shape args> | --colour-at X,Y [--tolerance N]) [--restore]`.
  - `draw` gains `--material rock|dirt|background`; `set --at` works on drawn layers.
  - Each command bakes, as the others do.
  - A `ColourAt` restore is refused, with the message "--restore takes a shape".

- [ ] **Step 1: Write the failing tests.**
  - `element.rs`:
    - `merge_down_and_split_round_trip_a_plain_level`: compose the dirt level as three layers, then `merge_down("dirt")`; `split("background")` gives back layers whose bake equals the original's.
    - `drawing_rock_into_a_dirt_layer_makes_it_mixed`: the composition says `mixed: true` and `<id>.material.png` exists.
    - `mask_by_shape_and_colour`: both write `editor/masks/<id>.png` and set `mask: true`.
    - `set_at_moves_a_drawn_layer`.
  - `olr-cli`'s `tests/element.rs`: `merge_split_and_mask_commands` runs `merge-down`, `split` and `mask --colour-at` on a copy of the fixture, as that file's other tests run commands. It asserts exit 0 and the printed ids.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit element` and `cargo nextest run -p olr-cli element`. Expected: FAIL.
- [ ] **Step 3: Implement the operations and subcommands.** Doc comments state each argument's meaning (`--help`).
- [ ] **Step 4: Re-make the fixture.** Run the commands from `fixtures/edit/README.md`. Before its last line (`level bake`), add:
  ```
  olr-cli level element add-layer fixtures/edit/composed-level --material background --name Wall
  olr-cli level element draw fixtures/edit/composed-level wall --rect 380,40,60,40 --colour 506070
  olr-cli level element add-layer fixtures/edit/composed-level --material dirt --name Crate
  olr-cli level element draw fixtures/edit/composed-level crate --rect 390,50,30,30 --colour 8a5a2a
  olr-cli level element draw fixtures/edit/composed-level crate --rect 400,60,8,8 --colour 909090 --material rock
  olr-cli level element merge-down fixtures/edit/composed-level crate
  olr-cli level element set fixtures/edit/composed-level wall --at 4,0
  olr-cli level element mask fixtures/edit/composed-level badge --circle 18,124,6
  olr-cli level element mask fixtures/edit/composed-level gradient --colour-at 310,210 --tolerance 20
  ```
  Then:
  - add the lines to the README;
  - run `cargo nextest run -p olr-edit the_fixture_is_baked`; expected: PASS;
  - run `cargo nextest run -p olr-format`; expected: PASS (the fixture is canonical).

  `wall` must end up mixed and backed. Assert this in `the_fixture_is_baked`: the fixture holds `mixed: true`, `back: true` and `mask: true` once each at least.
- [ ] **Step 5:** Run `cargo nextest run -p olr-edit -p olr-cli`. Expected: PASS.
- [ ] **Step 6:** Commit: `olr-edit, olr-cli: merge, split and mask commands; the fixture gains them`.

Batch B ends: run `cargo xtask check`. Expected: no failures.

### Task 7: The document's new edits (`olr-editor`)

**Files:**
- Modify: `crates/olr-editor/src/history.rs` (`Edit`, `Patch`, `bytes`, `is_noop`, `grids`).
- Modify: `crates/olr-editor/src/ops.rs`:
  - `paint` and `erase` (drawn: `drawn::draw`/`erase`; image: the mask);
  - `begin_stroke`, `stroke_edit`, `mask_edit`, `remove_colour`, `merge_down`;
  - `split` for any element;
  - `set` for `Material` on mixed layers and `At` on drawn ones;
  - `element_rect` for drawn boxes;
  - `apply` for the new edits;
  - `resize` and `set_art_scale`.
- Modify: `crates/olr-editor/src/document.rs`: `open` keeps every non-original file in `stored`, mask files included; `pack_of`.
- Test: `crates/olr-editor/tests/document.rs` and `tests/history.rs`.

**Interfaces:**
- Consumes: Tasks 2-5.
- Produces:
  ```rust
  // history.rs
  /// A drawn layer's pixels over a rectangle of its box (world px, local to the box).
  #[derive(Clone, Debug, PartialEq)]
  pub struct Patch { pub art: RgbaImage, pub cells: Option<Vec<Material>>, pub back: Option<RgbaImage> }
  pub enum Edit {
      Pixels { id: String, rect: Rect /* local */, before: Patch, after: Patch }, // was art only
      /// A whole item swapped (a conversion to mixed or plain, a new mask, a merge's result).
      Layer { index: usize, before: Box<Item>, after: Box<Item> },
      /// An image's mask over `rect` (original pixels).
      Mask { id: String, rect: Rect, before: Vec<u8>, after: Vec<u8> },
      /* the others as today */
  }
  // ops.rs
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Stroke { Paint(Material), Erase, Restore }
  impl Document {
      /// Makes `id` ready for `stroke` (R6, R5, R7): a plain layer painted another material, or
      /// erased with back art, becomes mixed; an unmasked image erased gets a full mask. The
      /// conversion is applied and returned for the stroke's step.
      pub fn begin_stroke(&mut self, id: &str, stroke: Stroke) -> Result<Option<Edit>, DocError>;
      /// Paints a drawn layer; `material` `None` is the layer's; another material on a plain layer
      /// is `DocError::Bad` (call `begin_stroke` first). An image refuses with
      /// "erase on a picture; merge it into a layer to paint it".
      pub fn paint(&mut self, id: &str, shape: &Shape, paint: &Paint, material: Option<Material>) -> Result<Option<Rect>, DocError>;
      /// Erases a drawn layer (R5) or masks an image (`restore`: unmasks); the level rectangle rebaked.
      pub fn erase(&mut self, id: &str, shape: &Shape, restore: bool) -> Result<Option<Rect>, DocError>;
      /// A copy of what a stroke changes: a drawn layer's data or an image's mask.
      pub fn stroke_before(&self, id: &str) -> Option<StrokeBefore>;
      /// The edit of a finished stroke (`Pixels` or `Mask`), `None` for a stale copy.
      pub fn stroke_edit(&self, id: &str, rect: Rect /* level */, before: &StrokeBefore) -> Option<Edit>;
      pub fn remove_colour(&mut self, id: &str, at: (i32, i32), tolerance: u8) -> Result<Vec<Edit>, DocError>; // [Layer?, Mask]
      pub fn merge_down(&mut self, id: &str) -> Result<Vec<Edit>, DocError>; // [Remove upper, Layer lower]
      pub fn split(&mut self, id: &str) -> Result<Vec<Edit>, DocError>;      // any kind (R11)
  }
  #[derive(Clone, Debug)]
  pub enum StrokeBefore { Layer(Decoded), Mask(Arc<Mask>) }
  ```
  - `set(id, Property::Material(m))` on a mixed layer is an `Edit::Layer` to the plain layer of `m`: cells `None`, `mixed: false` (C4). On a plain layer it is as today.
  - `set(id, Property::At(_))` works on a drawn layer too.
  - Paint and pixel edits rebake the level rectangle under the box's changed part.

- [ ] **Step 1: Write the failing tests** (`document.rs`).
  - `erasing_a_merged_map_digs_to_its_background`:
    - open `olx-level-dirt-level` and `merge_down("dirt")`;
    - at a dirt world pixel, `begin_stroke("background", Stroke::Erase)` then `erase(…, false)`;
    - the bake's material there is background, and front equals the opened level's back there.
  - `an_undone_conversion_saves_a_plain_layer`:
    - `begin_stroke("dirt", Stroke::Paint(Material::Rock))` and a `paint`;
    - apply the step's edits backwards (as `undo` does);
    - `to_pack()`'s files hold no `editor/layers/dirt.material.png`, and its composition says `Drawn(material: Dirt)`.
  - `a_layer_moved_off_and_back_is_unchanged`:
    - `set("rock", At((-500, 0)))`, then `At((0, 0))`;
    - the bake and `to_pack()` equal the opened ones.
  - `masking_an_image_erases_it_from_the_bake`: on the fixture, `begin_stroke("badge", Stroke::Erase)`, `erase` a circle at the badge's centre, then `stroke_edit`; the bake changes there, and undoing (apply backwards) restores it.
  - `remove_colour_masks_a_region`: on the fixture's `gradient`.
  - `a_mixed_layers_material_makes_it_plain`.
  - Rewrite `a_raster_splits_into_three_layers` for R11: same ids and bake; empty outputs left out.
  - `merge_down_is_refused_on_the_bottom_or_onto_an_image`.
  - `a_locked_image_is_not_masked`: `begin_stroke` and `erase` on a locked image give `DocError::Locked`, and `remove_colour` does too.
  - `history.rs`:
    - the undo proptest's operations gain `begin_stroke` + `paint` with a random material, `merge_down`, `split` and `set At` on drawn layers;
    - `Edit::bytes` counts `Patch` cells and back, `Layer` items and `Mask` bytes (one unit test of each).
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor document history`. Expected: FAIL.
- [ ] **Step 3: Implement the edits and operations** above.
  - `apply` of a `Pixels` or `Layer` edit whose sizes do not fit (a forged edit) changes nothing.
  - `pack_of` writes `encode_decoded`'s files for every element, masks included.
  - The pointer and the tools in `pointer.rs` keep compiling, through `stroke_before`/`stroke_edit` and `paint(…, None)`. Task 8 adds the new behaviour.
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: mixed layers, masks, merge and split in the document's history`.

### Task 8: The tools and panels (`olr-editor`)

**Files:**
- Modify: `crates/olr-editor/src/tools.rs`: `Tool::Wand` (W, "Remove colour"), `Tool::ALL` with 9 entries, `paints()` unchanged.
- Modify: `crates/olr-editor/src/state.rs`:
  - fields `paint_material: Option<ElementMaterial>`, `restore: bool`, `tolerance: u8` (default 10);
  - actions `PaintMaterial(Option<ElementMaterial>)`, `Restore(bool)`, `Tolerance(u8)`, `MergeDown(String)`;
  - `Split(String)` for any kind.
- Modify: `crates/olr-editor/src/pointer.rs`:
  - strokes call `begin_stroke` and push its edit first;
  - the Eraser masks the selected image (`restore`);
  - `Wand` on an image;
  - R9's selection and moving, `nudge` for drawn layers, the drawn box overlay.
- Modify: `crates/olr-editor/src/ui/toolbar.rs` (R13), `ui/properties.rs`, `ui/mod.rs`:
  - labels `MERGE_DOWN` "Merge down", `PAINT_AS` "Paint as", `RESTORE` "Restore", `TOLERANCE` "Tolerance";
  - Ctrl+E.
- Test: `crates/olr-editor/tests/tools.rs`, `tests/state.rs`, `tests/flows.rs`.

**Interfaces:**
- Consumes: Task 7.
- The properties panel shows:
  - "Merge down" for any element but the bottom one;
  - "Split into layers" for every kind;
  - for a drawn layer, the material combo (it reads "Mixed" on a mixed layer) and X/Y position fields.
- The toolbar:
  - "Paint as" (a fixed-width combo: "Layer's", Rock, Dirt, Background) while a painting tool is chosen;
  - "Restore" (a checkbox) with the Eraser;
  - "Tolerance" (a 0-100 % `DragValue`) with the Wand.

- [ ] **Step 1: Write the failing tests.**
  - `tools.rs`:
    - `a_stroke_of_rock_into_dirt_is_one_step`: Brush with `PaintMaterial(Some(Rock))` on the `dirt` layer of a plain level; one `Undo` restores the bake and the plain layer.
    - `the_eraser_masks_a_selected_image`: an Eraser stroke on the fixture's `badge` changes the bake there; with `Restore(true)`, a second stroke brings it back.
    - `the_wand_removes_a_colour`.
    - `a_click_on_a_drawn_layer_selects_before_it_moves`: a press and drag on the unselected `rock` layer selects it and moves nothing; a second press and drag on it moves it by the drag, as one step.
    - `arrows_nudge_a_selected_drawn_layer`.
  - `state.rs`: `merge_down_and_split_are_one_step_each` (via actions; `Undo` restores the items).
  - `flows.rs`:
    - `merge_down_from_the_properties`: kittest clicks "Merge down" on the selected `dirt` and the list shows one fewer element.
    - `the_tool_options_follow_the_tool`: "Paint as" shows for Brush, "Restore" for Eraser, "Tolerance" for Wand.
    - `a_toolbar_row_is_one_straight_line` and `the_toolbar_stays_in_the_window_and_in_place` pass with each tool chosen.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor`. Expected: FAIL (no `Tool::Wand`, no labels).
- [ ] **Step 3: Implement the tools, actions, pointer behaviour and panels.**
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor` and `cargo nextest run -p olr-game editor`. Expected: PASS.
- [ ] **Step 5:** Run `cargo nextest run -p olr-edit --release --run-ignored only mask_dab_speed --no-capture`. This is a new ignored test in `tests/mask.rs`: a size-16 dab on a 1024 × 1024 original in a 256 × 256 box, no colour reduction. Expected: under 20 ms. Record the time in `crates/olr-editor/CLAUDE.md`. If it is slower, add a ruling to the docs: a dab preview is a follow-up.
- [ ] **Step 6:** Commit: `olr-editor: paint as, the masking eraser and wand, merge down, moving layers`.

Batch C ends: run `cargo xtask check` and `cargo xtask e2e`. Expected: no failures; the editor autoplay script still passes.

### Task 9: Docs (Batch D)

**Files:**
- Modify: `docs/porting/STATUS.md`: a new section, "Level editor decisions (plan 14d)", with R1-R13 and the spec's C1-C6 as kept.
- Modify:
  - `crates/olr-format/CLAUDE.md` (the composition's new fields and files);
  - `crates/olr-edit/CLAUDE.md` (`mask`, `drawn`, the bake's R4, `element` commands);
  - `crates/olr-editor/CLAUDE.md` (edits, `begin_stroke`, tools, R9, R13);
  - `crates/olr-cli/CLAUDE.md` (the commands).
- Modify: `docs/superpowers/specs/2026-10-18-level-editor-polish-design.md`: its Status line gains "14d done: layers, masks, merge, split; next 14e (textures, fill, stamps), 14f (clipboard, multi-select)".
- Modify: `docs/formats/` (`cargo xtask format-docs`, if not already current).

- [ ] **Step 1:** Write the docs above. Run `cargo xtask format-docs`. Expected: no change, or only the generated pages.
- [ ] **Step 2:** No check is needed for a docs-only change. Commit: `docs: plan 14d's decisions`.
- [ ] **Step 3:** Ask the user before `cargo xtask full`. Report failures and do not re-run. Then offer a playtest build (`cargo xtask dist`).
