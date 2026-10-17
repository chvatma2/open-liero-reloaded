# Plan 14b: The Level Editor (Headless) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate `olr-editor` holds the level editor:
- its state, its document and its undo history;
- its tools and its egui panels.

All of it is tested headless with `egui_kittest`. No window and no GPU: plan 14c hosts it in `olr-game`, draws the canvas with the real renderer, and adds the file browser, recovery and the e2e script.

**Architecture:**
- **`olr-ui`'s pattern.** `EditorState::update(EditorAction) -> Vec<EditorCommand>` is pure. `ui::show(ui, &state, &mut EditorData) -> Vec<EditorAction>` draws egui and returns actions. A host carries out the commands and answers with actions: upload a dirty rectangle, save, open, play.
- **The document.** A `Document` holds the open level's elements *decoded*: drawn layers, rasters and processed image grids, through `olr-edit`'s new `Decoded` and `ImageCache`. A brush dab paints in memory and rebakes one rectangle. PNGs are encoded only on save.
- **`olr-edit` refactor.** Its 14a `Baker` is rebuilt on public decode, cache and bake-from-decoded functions, so the CLI and the editor share one bake.

**Tech Stack:** Rust 1.99, egui `=0.36.2`, egui_kittest `=0.36.2` (dev, features `wgpu`, `snapshot`), thiserror, proptest.

**Spec:** `docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md` §3 (cache), §4, §5 and §8 (14b's part), §9. Plan 14a, `docs/superpowers/plans/2026-10-17-plan-14a-level-composition.md`, and the "Level composition decisions" in `docs/porting/STATUS.md` hold the model this plan edits.

## Global Constraints

- Never panic on content (repo rule 6). Packs, images, pointer positions and typed values are untrusted.
- Keep output deterministic (repo rule 7). No `HashMap` iteration in output, and no wall clock in logic. The editor has no timers in 14b.
- **Integer arithmetic only** in `olr-format`, `olr-edit` and the document's pixel operations. Floats are allowed only in the view (screen↔world mapping) and in egui code.
- **Layers.** `olr-editor` is layer 5, `std`, `#![forbid(unsafe_code)]`. Register it in `xtask/layers.ron`.
  - It depends on `olr-data`, `olr-vfs`, `olr-format`, `olr-pack`, `olr-edit`, `egui` and `thiserror`.
  - It does **not** depend on `olr-ui`, `olr-render` or `olr-present`, so it needs no `same_layer` pair.
  - It is not in the `wasm` list.
- These stay byte-identical:
  - the oracle traces;
  - the match goldens;
  - the render goldens and `fixtures/ui/`;
  - `crates/olr-cli/tests/snapshots/`;
  - `content/packs`;
  - `fixtures/edit/composed-level` (its bake digest).
- `PACK_FORMAT_VERSION` stays 5. `composition.ron`'s format is unchanged.
- Limits:
  - history at most `History::MAX_BYTES` = 256 MiB;
  - brush size 1..=`MAX_BRUSH` (256) world pixels;
  - play bots 0..=8;
  - zoom levels `ZOOMS` = `[0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0]` screen points per art pixel.
- Learn a dependency's API from `cargo xtask api` and `target/api/<crate>.txt` (egui's from its source in `~/.cargo/registry/src/*/egui-0.36.2/`).
- Verify once per batch: targeted `cargo nextest run -p <crate>` while working, then the online `cargo xtask check` at each batch's end.
  - Batch A is Tasks 1-3.
  - Batch B is Tasks 4-5.
  - Batch C is Tasks 6-7.
  - Batch D is Tasks 8-9; Task 9 is docs, so only `format-docs --check` and the guide test.
- **No `cargo xtask full`, bench or stress** without the user's consent.
- Commits are small, with an imperative subject (`olr-editor: …`), and end with the session's trailer lines.

## Review Focus

1. **Undo and redo across structural edits.** Example: remove an image element, undo, redo, then make a new edit.
   - Undo restores the element, its original bytes and the bake exactly.
   - Redo removes it again.
   - The new edit clears the redo list.

   Test: Task 5 (`undo_and_redo_restore_everything_exactly`).
2. **Opening and saving without edits changes no byte** of a composed pack. A plain level gains only `editor/` (its raster base), and its baked files stay byte-identical. Test: Task 4 (`opening_and_saving_changes_nothing`).
3. **Painting at or beyond the level's edge, a huge brush, a line between far-off points.** Clipped, never a panic, cost bounded by the level. Tests: Task 2 (the `art_painting_is_clipped_and_total` proptest), Task 7 (`a_stroke_off_the_level_is_clipped`).
4. **A locked element.** Canvas drags, painting and Delete are refused with a status message. Nothing changes and no undo step is recorded. Stack order still changes (ruling R4). Test: Task 7 (`locked_elements_are_not_changed_by_the_canvas`).
5. **The history cap.**
   - Steps past 256 MiB drop the oldest first.
   - A single step larger than the cap is kept alone.
   - Undo after drops still restores the newest steps correctly.

   Test: Task 5 (`history_drops_the_oldest_steps_over_the_cap`).

## Rulings made while writing this plan

- **R1 14b's scope** (spec §9: "the framework and the level editor's state and panels, all tested headless").
  - **In 14b:** the document, the history, the tools, the panels, the shortcuts, New/Open/Save/close with the unsaved-changes dialog, and Play as a command.
  - **In 14c:**
    - hosting in `olr-game` and the canvas drawn by `olr-present`;
    - the file browser, so 14b takes image and pack paths through actions;
    - the 2-minute recovery copy;
    - the bundled-pack copy offer;
    - `olr --edit` and the e2e script.
- **R2 The document works on decoded data.**
  - `Document` keeps each element decoded (`olr_edit::compose::Decoded`).
  - A paint dab changes a drawn layer in memory and rebakes one world rectangle.
  - `Document::to_pack` encodes the composition and the bake only for saving.
- **R3 The image cache** (spec §3: "moving or reordering an element never reprocesses it").
  - `olr_edit::compose::ImageCache` keeps each original decoded once.
  - Processed grids are keyed by source, box art size and pixelation.
  - Within one session an original's bytes never change under the same name: sources are only added, under new names (14a R7).
  - Each source keeps its `GRIDS_PER_SOURCE` = 4 most recently used grids.
- **R4 Locks protect content, not order.** A locked element refuses property changes (except `Locked`), canvas moves and resizes, painting and Delete. Raise, lower and drag-to-reorder still work (as `olr_edit::element::raise|lower` do).
- **R5 EXIF orientation is applied at decode.**
  - `image_io::decode_image_within` turns a JPEG by its EXIF `Orientation` (tag `0x0112`, values 2-8).
  - The original keeps its bytes (14a R7). `image_size` stays the stored frame's size; the bounds are the same either way round.
  - No bundled fixture has the tag, so no bake changes.
- **R6 Live diagnostics are the editor's own cheap checks**, not a full `pack check` per edit, which would encode every PNG:
  - `check_elements` (the metadata part of `check_composition`);
  - spawns outside the level;
  - a level without spawns (a warning).

  `pack check`'s full validation runs on save (`pack::save` validates).
- **R7 Pointer actions are in world pixels.** The UI maps screen to world through `View`; the state never sees screen coordinates, except for pan and zoom.
- **R8 Resize of an image box** previews its outline during the drag and applies on release (one reprocess). A move is live: no reprocess, a rebake of the old and new boxes.
- **R9 Play defaults:** the mod `olx-classic` ("Liero v1.0", the spec's default) and 1 bot. Both are remembered in `EditorSettings`, saved through `EditorCommand::SaveSettings`. 14c stores them.
- **R10 Shapes.** Ellipse and line are new `olr_edit::level::Shape` variants, with integer coverage:
  - an ellipse covers a pixel by its centre;
  - a line covers the pixels within `width / 2` of the segment.

  Both are measured in doubled coordinates, so halves stay integers. A brush stroke is dabs at each pointer sample, joined by `Line`s of the brush's width, so fast strokes leave no gaps.

---

### Task 1: EXIF orientation and metadata-only composition checks (`olr-format`)

**Files:**
- Modify: `crates/olr-format/src/image_io.rs`; `crates/olr-format/src/levels/composition.rs`.
- Test: unit tests in both.

**Interfaces:**
- Produces:

  ```rust
  // image_io
  /// The EXIF `Orientation` (1-8) of a JPEG's APP1 segment; `None` when absent,
  /// out of range, or not a JPEG. Reads only the first IFD; every index checked.
  pub fn jpeg_orientation(bytes: &[u8]) -> Option<u8>;
  /// `img` turned as EXIF orientation `o` asks (1 unchanged; 5-8 swap width and height).
  pub fn orient(img: &RgbaImage, o: u8) -> RgbaImage;
  // levels (composition.rs)
  /// `check_composition` without the files: the count, ids (valid, unique, their layer
  /// files unshared and portable), image fields, sources unique case aside, each box's
  /// art within `LEVEL_ART`, the pixel budget. `check_composition` calls it, then
  /// checks the files.
  pub fn check_elements(c: &Composition, size: (u32, u32), art_scale: u32) -> Result<(), FormatError>;
  ```

  `Dither` gains `PartialOrd, Ord, Hash` (for `ImageCache` keys).
- Orientation mapping. A new pixel `(x, y)` takes the old pixel below. `W` and `H` are the old width and height; for 5-8 the new image is `H × W`.

  | Orientation | Old pixel |
  |---|---|
  | 1 | `(x, y)` |
  | 2 | `(W-1-x, y)` |
  | 3 | `(W-1-x, H-1-y)` |
  | 4 | `(x, H-1-y)` |
  | 5 | `(y, x)` |
  | 6 | `(y, H-1-x)` |
  | 7 | `(W-1-y, H-1-x)` |
  | 8 | `(W-1-y, x)` |
- `decode_image_within` applies `orient(img, jpeg_orientation(bytes))` to JPEGs.
- APP1 layout:
  - `FF E1`, then a 2-byte length;
  - `Exif\0\0`;
  - a TIFF header: `II` (little-endian) or `MM`, `0x002A`, then a `u32` offset to IFD0 from the TIFF header's start;
  - IFD0: a `u16` count, then 12-byte entries (tag `u16`, type `u16`, count `u32`, value 4 bytes). Orientation is type 3 (SHORT), its value in the first 2 bytes.

- [ ] **Step 1: Write the failing tests.**
  - In `image_io.rs`, add a test helper `with_orientation(jpeg: &[u8], o: u8, big_endian: bool) -> Vec<u8>`. It inserts an APP1 segment right after SOI holding one IFD0 entry `0x0112` = `o`.
  - `jpeg_orientation_is_read_in_both_byte_orders`:
    - `jpeg_orientation(&with_orientation(GRADIENT_JPG, 6, false)) == Some(6)`, and the same with `true`;
    - `jpeg_orientation(GRADIENT_JPG) == None`;
    - `jpeg_orientation(&with_orientation(GRADIENT_JPG, 9, false)) == None`;
    - the helper's output cut at every length from 0 to its length gives no panic.
  - `decoding_applies_every_orientation`: for `o` in 1..=8, decode `with_orientation(GRADIENT_JPG, o, false)`. It equals `orient(&decode_image_within(GRADIENT_JPG, IMAGES)?, o)`. For 5-8 the size is 48 × 64, and spot pixels follow the table above (check `(0, 0)` and `(1, 0)` for each `o` against the table on the unturned decode).
  - In `composition.rs`, `check_elements_needs_no_files`: `sample()` with `files` emptied passes `check_elements` and fails `check_composition` ("missing"). `sample()` with a duplicate id fails `check_elements` with "duplicate id".
- [ ] **Step 2:** Run `cargo nextest run -p olr-format image_io:: composition`. Expected: FAIL (functions missing).
- [ ] **Step 3: Implement.**
  - `jpeg_orientation` walks the markers with the existing `jpeg_marker` walker (stop at `0xE1` whose data starts with `Exif\0\0`; keep walking past other APP1s).
  - `check_elements` is the element loop of `check_composition` without the file lookups; `check_composition` calls it first.
- [ ] **Step 4:** Run `cargo nextest run -p olr-format`, `cargo build -p olr-format --target wasm32-unknown-unknown` and `cargo nextest run -p olr-edit the_fixture`. Expected: PASS, the fixture unchanged.
- [ ] **Step 5:** Commit: `olr-format: EXIF orientation at decode; composition checks without files`.

### Task 2: Ellipses, lines and painting art in memory (`olr-edit`)

**Files:**
- Modify: `crates/olr-edit/src/level.rs`, `crates/olr-edit/src/compose.rs` (`pub use crate::level::Rect;`), `crates/olr-edit/src/element.rs` (`draw`/`erase` call the new functions).
- Test: `crates/olr-edit/tests/level.rs`.

**Interfaces:**
- Produces, in `olr_edit::level`:

  ```rust
  /// A rectangle of world pixels (moved from `compose`, which re-exports it).
  pub struct Rect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }
  impl Rect {
      /// The smallest rectangle holding both; `None` and `x` give `x`.
      pub fn union(a: Option<Rect>, b: Rect) -> Rect;
  }
  pub enum Shape {
      Rect { x: i32, y: i32, w: i32, h: i32 },
      Circle { cx: i32, cy: i32, r: i32 },
      /// The ellipse inscribed in the box; covers a pixel by its centre.
      Ellipse { x: i32, y: i32, w: i32, h: i32 },
      /// Pixels within `width / 2` of the segment; `width` 0 covers nothing.
      Line { from: (i32, i32), to: (i32, i32), width: u32 },
  }
  /// The world pixels `shape` covers, clipped to a level of `size`: `None` when none.
  pub fn shape_bounds(shape: &Shape, size: (u32, u32)) -> Option<Rect>;
  /// Paints `shape`'s whole art blocks into a level-sized art layer, opaque (a transparent
  /// paint pixel leaves the art). Returns the world rectangle changed.
  pub fn draw_art(art: &mut RgbaImage, art_scale: u32, size: (u32, u32), shape: &Shape, paint: &Paint) -> Option<Rect>;
  /// Clears `shape`'s art blocks (alpha 0); the world rectangle changed.
  pub fn erase_art(art: &mut RgbaImage, art_scale: u32, size: (u32, u32), shape: &Shape) -> Option<Rect>;
  /// `img` (a layer at art scale `from`) at art scale `to`, nearest neighbour:
  /// new pixel (ax, ay) takes old (ax * from / to, ay * from / to).
  pub fn rescale_art(img: &RgbaImage, from: u32, to: u32) -> RgbaImage;
  ```

- Coverage, in i128. Doubled coordinates: a pixel centre is `(2px + 1, 2py + 1)`.
  - **Ellipse:** covered iff `((2px+1-2x-w)·h)² + ((2py+1-2y-h)·w)² ≤ (w·h)²`, for `w, h > 0`.
  - **Line:** endpoints at `(2ax+1, 2ay+1)`. Covered iff the squared distance from the centre to the segment is `≤ width²`. Compute it with the clamped projection, comparing `cross² ≤ width²·L²` inside the segment and endpoint distances outside.
  - Iterate only over the bounding box, expanded by the width and clipped to the level. A far-off line costs at most the level.
- `covered` (crate-private) handles all four shapes. `carve`, `fill`, `paint`, the CLI and `element::draw|erase` keep their behaviour for `Rect` and `Circle`.

- [ ] **Step 1: Write the failing tests** in `tests/level.rs`:
  - `ellipse_and_line_cover_by_centre`:
    - `Ellipse { x: 0, y: 0, w: 4, h: 2 }` covers exactly `(0..4, 0..2)` minus the four corners;
    - `Line { from: (0, 0), to: (4, 0), width: 1 }` covers `(0..=4, 0)`;
    - `Line { from: (0, 0), to: (0, 0), width: 3 }` covers the 3 × 3 block around `(0, 0)` clipped to the level (`(0..=1, 0..=1)`);
    - `width: 0` covers nothing.
  - `draw_and_erase_art_return_their_rectangles`: on an 8 × 6 level at art scale 2, `draw_art` with `Rect { x: 1, y: 1, w: 2, h: 2 }` returns `Some(Rect { x: 1, y: 1, w: 2, h: 2 })`. Art pixels `(2..6, 2..6)` are opaque, others clear. `erase_art` of the same shape returns the same rectangle and clears them.
  - `rescale_art_is_nearest`: a 2 × 2 layer with four colours at scale 1 rescaled to 2 gives 4 × 4 blocks of each; back to 1 it is the original.
  - Proptest `art_painting_is_clipped_and_total` (64 cases):
    - shapes with coordinates in `-10_000..10_000`, sizes up to 10 000, widths `0..300`, levels `1..=16` at art scales 1-3;
    - no panic, and the returned rectangle lies inside the level;
    - every changed art pixel lies in the returned rectangle's blocks;
    - include `i32::MIN` and `i32::MAX` endpoints in a fixed case.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit level`. Expected: FAIL.
- [ ] **Step 3:** Implement as specified. `element::draw` and `element::erase` call `draw_art` and `erase_art` on the decoded layer.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit -p olr-cli --test integration element:: level::`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: ellipses, lines and painting art layers in memory`.

### Task 3: Decoded elements, the image cache and one bake (`olr-edit`)

**Files:**
- Modify: `crates/olr-edit/src/compose.rs`, `crates/olr-edit/src/element.rs`, `crates/olr-edit/src/pixelate.rs` (`Pixelation` derives `PartialOrd, Ord, Hash`).
- Test: `crates/olr-edit/tests/compose.rs`, `crates/olr-edit/tests/element.rs`.

**Interfaces:**
- Consumes: Task 1's `Dither: Ord`; Task 2's `Rect`, `Shape`, `rescale_art`.
- Produces, in `olr_edit::compose`:

  ```rust
  /// Originals decoded once per source, and processed grids per (source, box art size,
  /// pixelation), at most GRIDS_PER_SOURCE (4) per source, the least recently used dropped.
  pub struct ImageCache { /* private */ }
  pub const GRIDS_PER_SOURCE: usize = 4;
  impl ImageCache {
      pub fn new() -> ImageCache;
      /// The processed element grid (`pixelate::process_grid`) for `source` whose bytes are
      /// `original`; decodes `original` (within IMAGES, EXIF-oriented) on first use.
      pub fn grid(&mut self, source: &str, original: &[u8], art_box: (u32, u32), p: &Pixelation)
          -> Result<Arc<RgbaImage>, ComposeError>;
      /// The decoded original (its oriented size gives an imported box's default size).
      pub fn original(&mut self, source: &str, bytes: &[u8]) -> Result<Arc<RgbaImage>, ComposeError>;
      /// How many grids were processed since `new` (tests: a move never processes).
      pub fn processed(&self) -> usize;
      /// Drops everything of sources not in `keep`.
      pub fn retain_sources(&mut self, keep: &BTreeSet<String>);
  }
  /// An element's data decoded for baking.
  #[derive(Clone, Debug, PartialEq)]
  pub enum Decoded {
      /// `None`: hidden and not processed (its original decoded, for its errors).
      Image { grid: Option<Arc<RgbaImage>> },
      Drawn { art: RgbaImage },
      Raster { material: Vec<Material>, front: RgbaImage, back: RgbaImage },
  }
  impl Decoded { pub fn bytes(&self) -> usize; } // memory held, for the history cap
  pub fn decode_element(src: &CompositionSource, e: &Element, size: (u32, u32), art_scale: u32,
      cache: &mut ImageCache) -> Result<Decoded, ComposeError>;
  /// Rebakes `rect` (clipped) from decoded elements, bottom to top; rulings R3-R5 of 14a.
  /// A visible image whose grid is `None` is skipped. Never fails or panics on any sizes.
  pub fn bake_elements(level: &mut LevelSource, items: &[(&Element, &Decoded)], back_tile: &RgbaImage, rect: Rect);
  /// The files a drawn or raster element's decoded data encodes to (as `write_level_files`
  /// encodes art and material); none for an image (its original is kept).
  pub fn encode_decoded(e: &Element, d: &Decoded) -> Vec<(String, Vec<u8>)>;
  /// Re-lays a drawn or raster element for a level resize (as `level::resize`: the new area
  /// uncovered for drawn layers, `material` and `paint` for rasters); moves an image's `at`.
  pub fn resize_decoded(e: &mut Element, d: &mut Decoded, old: (u32, u32), art_scale: u32,
      size: (u32, u32), offset: (i32, i32), material: Material, paint: &Paint) -> Result<(), EditError>;
  ```

  `Baker` keeps `(Element, Decoded)` pairs, and `Baker::new` decodes with a fresh `ImageCache`. Add `Baker::new_cached(…, cache: &mut ImageCache)`. `bake_region` calls `bake_elements`.

- Produces, in `olr_edit::element` (the private `new_id` and `new_source` made public over plain data):

  ```rust
  /// A new id from `name` (ruling R8 of 14a, at most 64 bytes) not taken by `elements`.
  pub fn new_element_id(elements: &[Element], name: &str, fallback: &str) -> String;
  /// `file_name`, or `-2`, `-3`, … before its extension, not among `sources` (case aside)
  /// and portable (ruling R7 of 14a).
  pub fn new_source_name<'a>(sources: impl Iterator<Item = &'a str> + Clone, file_name: &str) -> String;
  ```

  `element::resize` uses `resize_decoded`.

- [ ] **Step 1: Write the failing tests** in `tests/compose.rs`:
  - `a_move_never_reprocesses`:
    - Bake a composition holding one image with a cache. `processed() == 1`.
    - Change only `at`, and rebuild with `Baker::new_cached` on the same cache. Still 1.
    - Reorder the elements. Still 1.
    - Change `pixel_size`. 2.
    - Change it back. Still 2 (the cached grid).
  - `the_cache_keeps_four_grids_per_source`: five pixel sizes in turn, then the first again, gives `processed() == 6`.
  - `bake_elements_equals_the_baker`: a proptest over `random_stack` (14a's helper). `bake_elements` over all decoded items gives the same level as `bake`.
  - `encode_decoded_round_trips`: for the fixture `fixtures/edit/composed-level`, decoding each element and re-encoding it gives back the files byte for byte.
  - `resize_decoded_matches_element_resize`: an 8 × 6 pack (as `tests/element.rs`'s `pack(8, 6, 1)`) with an image at (3, 2) and a drawn layer with one painted pixel, resized to (13, 9) with offset (5, 3). Composition files from `element::resize` equal those from `resize_decoded` over each element followed by `encode_decoded`.

  In `tests/element.rs`: `new_element_id_and_source_name_follow_r7_r8`:
  - `new_element_id(&[ground], "Ground", "layer") == "ground-2"`;
  - `new_source_name(["a.png"].into_iter(), "A.PNG") == "A-2.PNG"`;
  - `new_source_name([].into_iter(), "CON.png") == "CON-2.png"`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit compose element`. Expected: FAIL.
- [ ] **Step 3: Implement.** The `Layer` enum of 14a becomes the private view built from `(&Element, &Decoded)` at bake time (borrowed, no copy). Keep every 14a test passing unchanged.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit -p olr-cli --test integration element:: level::`, and `cargo nextest run -p olr-edit --release --run-ignored only bake_speed --no-capture`. Expected: PASS, and the bake no slower than in 14a (about 0.3-0.4 s); note the times in the commit message.
- [ ] **Step 5:** Commit: `olr-edit: decoded elements, an image cache, one bake for the CLI and the editor`.
- [ ] **Step 6 (batch A):** Run the online `cargo xtask check`. Expected: every step `ok`; the determinism bake digest unchanged.

### Task 4: The crate and the document (`olr-editor`)

**Files:**
- Create:
  - `crates/olr-editor/{Cargo.toml, CLAUDE.md, src/lib.rs, src/catalog.rs, src/document.rs}`;
  - `crates/olr-editor/tests/{integration.rs, common.rs, document.rs}`.
- Modify: workspace `Cargo.toml` (member, `olr-editor = { path = "crates/olr-editor" }`); `xtask/layers.ron` (layer 5).

**Interfaces:**
- Produces:

  ```rust
  // catalog.rs: what the host knows about installed packs.
  #[derive(Clone, Debug, Default)]
  pub struct Catalog { pub themes: Vec<ThemeEntry>, pub mods: Vec<ModEntry> }
  #[derive(Clone, Debug)]
  pub struct ThemeEntry { pub id: String, pub name: String, pub front_tile: RgbaImage, pub back_tile: RgbaImage }
  #[derive(Clone, Debug)]
  pub struct ModEntry { pub id: String, pub name: String }
  /// Remembered between sessions (the host stores it; serde, every field defaulted).
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  #[serde(default)]
  pub struct EditorSettings { pub play_mod: String, pub play_bots: u8 } // "olx-classic", 1 (R9)

  // document.rs
  #[derive(Clone, Debug, PartialEq)]
  pub struct Item { pub element: Element, pub decoded: Decoded }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Problem { pub severity: olr_pack::Severity, pub text: String }
  #[derive(Debug, thiserror::Error)]
  pub enum DocError {
      #[error("not a level pack")] NotALevel,
      #[error("theme {0:?} is not installed")] NoTheme(String),
      #[error("no element {0:?}")] NoElement(String),
      #[error("element {0:?} is locked")] Locked(String),
      #[error("element {id:?} is not {want}")] WrongKind { id: String, want: &'static str },
      #[error("{0}")] Bad(String),
      #[error(transparent)] Compose(#[from] olr_edit::compose::ComposeError),
      #[error(transparent)] Edit(#[from] olr_edit::level::EditError),
  }
  pub struct Document { /* private: dir, manifest, other, level, items, originals, cache, theme tiles, baked flag */ }
  impl Document {
      /// A level pack as the editor edits it: a plain level becomes its raster base
      /// (`compose_from`); every element decoded; the theme from `catalog`.
      pub fn open(pack: Pack, dir: PathBuf, catalog: &Catalog) -> Result<Document, DocError>;
      pub fn dir(&self) -> &Path;
      pub fn level(&self) -> &LevelSource;
      pub fn items(&self) -> &[Item];
      pub fn index_of(&self, id: &str) -> Option<usize>;
      /// Rebakes `rect` from the items (`bake_elements`) and marks the thumbnail stale.
      pub fn rebake(&mut self, rect: Rect);
      /// The whole level's rectangle.
      pub fn all(&self) -> Rect;
      /// The pack to save: the composition (`encode_decoded`, originals as kept), the bake,
      /// the thumbnail refreshed only if anything was rebaked since `open`.
      pub fn to_pack(&self) -> Pack;
      /// Ruling R6's live checks.
      pub fn diagnostics(&self) -> Vec<Problem>;
  }
  ```

- [ ] **Step 1: Set up the crate.**
  - Cargo: `egui.workspace`, `serde` (std), `thiserror`, the olr crates above. Dev-dependencies: `egui_kittest` (`wgpu`, `snapshot`), `tempfile`, `proptest`.
  - `autotests = false`, with one `[[test]] integration`.
  - `tests/common.rs` holds:
    - `content() -> Option<PathBuf>` (as `olr-edit`'s);
    - `catalog(root) -> Catalog`, with `olx-theme-dirt`'s tiles read by `olr_format::themes::read_theme_files`, and `olx-classic` and `olx-mw-1-0`;
    - `copy_pack(root, path) -> (TempDir, PathBuf)`.
- [ ] **Step 2: Write the failing tests** in `tests/document.rs`:
  - `opening_and_saving_changes_nothing`:
    - Copy `fixtures/edit/composed-level`, open it and take `pack::files(&doc.to_pack())`. It equals the files on disk byte for byte.
    - Open a copy of `olx-level-dirt-level` (plain). Its baked files are unchanged. It gains exactly `editor/composition.ron` and `editor/layers/base.{material,front,back}.png`.
  - `a_missing_theme_is_an_error`: a catalog without `olx-theme-dirt` gives `DocError::NoTheme("olx-theme-dirt")`.
  - `diagnostics_list_spawns_outside_and_none`:
    - the fixture with its objects cleared gives a warning containing "no spawn";
    - with a spawn at `(-5, 0)`, a warning containing "outside the level".
- [ ] **Step 3:** Run `cargo nextest run -p olr-editor`. Expected: FAIL (types missing).
- [ ] **Step 4:** Implement `Catalog`, `EditorSettings` (`Default` as R9) and `Document`. Register the crate in `xtask/layers.ron`, and run `cargo xtask layers`.
- [ ] **Step 5:** Run `cargo nextest run -p olr-editor` and `cargo xtask layers`. Expected: PASS.
- [ ] **Step 6:** Commit: `olr-editor: the crate and the document`.

### Task 5: Edits and the history (`olr-editor`)

**Files:**
- Create: `crates/olr-editor/src/history.rs`; `crates/olr-editor/src/ops.rs` (the document's operations); `crates/olr-editor/tests/history.rs`.

**Interfaces:**
- Consumes: Task 3's `ImageCache`, `Decoded`, `decode_element`, `resize_decoded`, `new_element_id`, `new_source_name`; Task 2's `draw_art`, `erase_art`, `rescale_art`, `Shape`, `Paint`, `Rect`; `olr_edit::element::Property`.
- Produces:

  ```rust
  /// A reversible change of the document.
  #[derive(Clone, Debug)]
  pub enum Edit {
      /// A drawn layer's art over `rect` (world), before and after (art pixels of the rect).
      Pixels { id: String, rect: Rect, before: RgbaImage, after: RgbaImage },
      /// An element's metadata (a property, a rename, a move of its box).
      Element { index: usize, before: Element, after: Element },
      Insert { index: usize, item: Box<Snapshot> },
      Remove { index: usize, item: Box<Snapshot> },
      Move { from: usize, to: usize },
      Objects { before: Vec<LevelObject>, after: Vec<LevelObject> },
      Meta { before: LevelMeta, after: LevelMeta },
      /// A resize or an art-scale change: the whole document.
      Whole { before: Box<Whole>, after: Box<Whole> },
  }
  /// An element with its decoded data and, for an image, its original's name and bytes.
  #[derive(Clone, Debug)]
  pub struct Snapshot { pub item: Item, pub original: Option<(String, Arc<Vec<u8>>)> }
  #[derive(Clone, Debug, PartialEq)]
  pub struct LevelMeta { pub name: String, pub notes: String, pub theme: String }
  #[derive(Clone, Debug)]
  pub struct Whole { pub level: LevelSource, pub items: Vec<Item> }
  impl Edit { pub fn bytes(&self) -> usize; }
  pub struct History { /* undo, redo, bytes */ }
  impl History {
      pub const MAX_BYTES: usize = 256 << 20;
      pub fn new() -> History;
      /// Records one step (one stroke, one drag, one property change); clears redo; drops
      /// the oldest steps while over MAX_BYTES, keeping at least this step.
      pub fn push(&mut self, step: Vec<Edit>);
      pub fn undo(&mut self) -> Option<Vec<Edit>>; // the step, to revert (last edit first)
      pub fn redo(&mut self) -> Option<Vec<Edit>>; // to apply again
      pub fn can_undo(&self) -> bool;
      pub fn can_redo(&self) -> bool;
      pub fn bytes(&self) -> usize;
  }
  /// A change made: its edit, and the world rectangle to rebake and upload.
  pub struct Change { pub edit: Edit, pub dirty: Rect }
  impl Document {
      /// Applies (`forward`) or reverts an edit; returns the rectangle it rebaked.
      pub fn apply(&mut self, edit: &Edit, forward: bool) -> Rect;
      /// An image element on top, centred on `centre` (world), its box the oriented
      /// original's size ÷ art scale (rounded up, at least 1): pixel size 1, no reduction,
      /// threshold 128, material dirt (spec §5). The id from the file stem.
      pub fn add_image(&mut self, file_name: &str, bytes: Vec<u8>, centre: (i32, i32)) -> Result<Change, DocError>;
      pub fn add_layer(&mut self, material: ElementMaterial, name: &str) -> Result<Change, DocError>;
      /// A property (R4: refused on a locked element unless it is `Locked`), checked with
      /// `check_elements`; an image reprocesses through the cache only when its pixelation
      /// or box size changes.
      pub fn set(&mut self, id: &str, property: Property) -> Result<Change, DocError>;
      pub fn remove(&mut self, id: &str) -> Result<Change, DocError>;
      /// Moves the element to `index` (bottom 0); allowed when locked (R4).
      pub fn move_to(&mut self, id: &str, index: usize) -> Result<Change, DocError>;
      /// Paints into drawn layer `id` (unlocked) without recording: strokes do (Task 7).
      pub fn paint(&mut self, id: &str, shape: &Shape, paint: &Paint) -> Result<Option<Rect>, DocError>;
      pub fn erase(&mut self, id: &str, shape: &Shape) -> Result<Option<Rect>, DocError>;
      /// Drawn layer `id`'s art (for a stroke's "before" copy and the eyedropper).
      pub fn layer_art(&self, id: &str) -> Option<&RgbaImage>;
      /// The Pixels edit of a finished stroke: `before` is the layer as the stroke began.
      pub fn stroke_edit(&self, id: &str, rect: Rect, before: &RgbaImage) -> Edit;
      pub fn add_spawn(&mut self, at: (i32, i32)) -> Result<Change, DocError>;
      pub fn move_object(&mut self, index: usize, at: (i32, i32)) -> Result<Change, DocError>;
      pub fn remove_object(&mut self, index: usize) -> Result<Change, DocError>;
      /// Name, notes and theme (`catalog` gives a new theme's tiles; a whole rebake).
      pub fn set_meta(&mut self, meta: LevelMeta, catalog: &Catalog) -> Result<Change, DocError>;
      /// As `olr-cli level resize`: the elements follow (`resize_decoded`), then the level.
      pub fn resize(&mut self, size: (u32, u32), offset: (i32, i32), material: Material, colour: Rgba) -> Result<Change, DocError>;
      /// Rescales drawn and raster layers (`rescale_art`) and reprocesses images.
      pub fn set_art_scale(&mut self, to: u32) -> Result<Change, DocError>;
      /// The cache's `processed()` (tests: a move never reprocesses).
      #[doc(hidden)]
      pub fn cache_processed(&self) -> usize;
  }
  ```

  Every operation checks before changing anything (`check_elements` for element metadata, `EditError` bounds for objects and sizes). A refused operation leaves the document unchanged. Originals live in the document's `originals` map, by source name, and leave the pack only when no element names them.

- [ ] **Step 1: Write the failing tests** in `tests/history.rs`, on the fixture document:
  - `undo_and_redo_restore_everything_exactly`:
    - Record `to_pack` files `A`.
    - `remove("tiny")`, then apply and push. Undo (apply each edit with `forward = false`) gives files `== A`. Redo gives the files after the removal.
    - Then a new `add_layer` leaves `can_redo() == false`.
  - `every_operation_undoes_to_the_same_files` (proptest, 24 cases): a random sequence of 1-6 operations drawn from `set` (name, visible, at, pixel size, colours), `move_to`, `add_layer`, `paint` (as a one-dab stroke), `add_spawn`, `set_meta` (name) and `resize`. Undoing all of them gives the starting files.
  - `history_drops_the_oldest_steps_over_the_cap`:
    - Push steps of `Pixels` edits of known size until over `MAX_BYTES`. The oldest are gone, and `bytes() <= MAX_BYTES` unless only one step remains.
    - One step larger than the cap is kept alone.
  - `a_locked_element_refuses_changes_but_moves_in_the_stack`:
    - `set(Locked(true))`, then `set(Name)` gives `DocError::Locked`, and so do `remove`, `paint` and `erase`.
    - `move_to(id, 0)` is `Ok`.
  - `importing_turns_by_exif_and_centres_the_box`: a JPEG with orientation 6 (Task 1's helper, copied into the test) at centre `(100, 50)` gives an element `size == (48, 64)` and `at == (76, 18)`.
  - `a_property_change_reprocesses_only_its_image`: `set(at)` keeps the cache's `processed()`; `set(pixel_size)` adds 1.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor history`. Expected: FAIL.
- [ ] **Step 3:** Implement `history.rs` and `ops.rs`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: reversible edits and a bounded history`.
- [ ] **Step 6 (batch B):** Run the online `cargo xtask check`. Expected: every step `ok`.

### Task 6: The editor state: files, undo, play (`olr-editor`)

**Files:**
- Create: `crates/olr-editor/src/state.rs`; `crates/olr-editor/tests/state.rs`.

**Interfaces:**
- Consumes: Tasks 4-5.
- Produces:

  ```rust
  pub struct EditorState { /* private */ }
  impl EditorState {
      pub fn new(settings: EditorSettings, catalog: Catalog) -> EditorState;
      pub fn update(&mut self, action: EditorAction) -> Vec<EditorCommand>;
      pub fn document(&self) -> Option<&Document>;
      pub fn settings(&self) -> &EditorSettings;
      pub fn catalog(&self) -> &Catalog;
      pub fn dirty(&self) -> bool;
      pub fn playing(&self) -> bool;
      pub fn dialog(&self) -> Option<&Dialog>;
      pub fn status(&self) -> &Status;
      pub fn selection(&self) -> &Selection;
      pub fn can_undo(&self) -> bool;
      pub fn can_redo(&self) -> bool;
  }
  #[derive(Clone, Debug, PartialEq)]
  pub enum Selection { None, Element(String), Object(usize) }
  #[derive(Clone, Debug, Default)]
  pub struct Status { pub problems: Vec<Problem>, pub message: Option<String>, pub cursor: Option<(i32, i32)>, pub material: Option<Material> }
  #[derive(Clone, Debug, PartialEq)]
  pub enum Dialog {
      /// Unsaved changes before `then`: Save, Don't save, Cancel.
      Unsaved { then: Pending },
      /// An art-scale change rescales drawn and raster layers (spec §5): Change, Cancel.
      ArtScale { to: u32 },
      Error(String),
  }
  #[derive(Clone, Debug, PartialEq)]
  pub enum Pending { Open(PathBuf), New(NewLevel), Close }
  #[derive(Clone, Debug, PartialEq)]
  pub struct NewLevel { pub dir: PathBuf, pub id: String, pub name: String, pub size: (u32, u32), pub theme: String }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Choice { Save, Discard, Cancel, Confirm }
  pub enum EditorAction {
      // The host's answers.
      Opened(Box<Document>), OpenFailed(String), Saved(Result<Vec<String>, String>), PlayEnded,
      // Files.
      Open(PathBuf), New(NewLevel), Save, Close, Choose(Choice),
      // Elements and objects.
      ImportImage { file_name: String, bytes: Vec<u8> }, AddLayer, Select(Selection),
      Set(String, Property), Remove, MoveElement { id: String, to: usize },
      // Level.
      Meta(LevelMeta), ArtScale(u32), Resize { size: (u32, u32), offset: (i32, i32), material: Material, colour: Rgba },
      Undo, Redo,
      // Play.
      Play, PlayMod(String), PlayBots(u8),
      // Canvas and tools: Task 7.
  }
  pub enum EditorCommand {
      /// Re-upload this world rectangle of the bake (the canvas, in 14c).
      Upload(Rect),
      /// Read the pack at this path (`pack::load`) and answer Opened / OpenFailed
      /// (`Document::open` with the catalog).
      Open(PathBuf),
      /// Make a new level pack (`olr_edit::new::new_pack`, `Template::Level`), then Open it.
      Create(NewLevel),
      /// `pack::save(dir, pack)`; answer Saved.
      Save { dir: PathBuf, pack: Box<Pack> },
      /// Start a sandbox match on this bake; answer PlayEnded when it stops.
      Play { level: Box<LevelSource>, mod_id: String, bots: u8 },
      SaveSettings(EditorSettings),
      /// The editor is done (after any unsaved-changes dialog).
      Close,
  }
  ```

  Rules:
  - Every committed change sets `dirty`, pushes one history step, refreshes `status.problems` (`Document::diagnostics`), and emits `Upload(dirty)`.
  - A refused change sets `status.message` to the error's text, with no command and no step.
  - `Open`, `New` and `Close` while dirty open `Dialog::Unsaved`:
    - `Save` saves, then carries out the pending action once `Saved(Ok)` arrives;
    - `Discard` carries it out;
    - `Cancel` closes the dialog.
  - `Saved(Err(e))` keeps `dirty` and opens `Dialog::Error(e)`. `Saved(Ok(warnings))` clears `dirty`, and the warnings join the status message.
  - `ArtScale(to)` opens `Dialog::ArtScale`; `Choose(Confirm)` applies it.
  - `Play` emits `Play` with the current bake, `play_mod` and `play_bots`. While `playing`, every action but `PlayEnded` is ignored, so the document is exactly as before.
  - `PlayMod` and `PlayBots` (clamped to 0..=8) emit `SaveSettings`.
  - `Remove` deletes the selected element or object (R4 for locks).
  - `ImportImage` centres the new box on the level's centre; Task 7 makes it the view's centre.
  - `Opened` resets the history and the selection and clears `dirty`, and emits `Upload(doc.all())`.

- [ ] **Step 1: Write the failing tests** in `tests/state.rs`. A test host `fn run(state, action) -> Vec<EditorCommand>` carries out `Open` (`pack::load` + `Document::open`), `Save` (`pack::save`) and `Create` synchronously, feeding the answers back:
  - `open_edit_save_round_trip`: open a fixture copy; `AddLayer`; `Save`. On disk `editor/composition.ron` names the new layer and `olr_pack::validate` has no error; `dirty() == false`.
  - `unsaved_changes_ask_before_close`:
    - an edit, then `Close` gives `Dialog::Unsaved`;
    - `Choose(Cancel)` closes the dialog with no command;
    - `Close` then `Choose(Discard)` gives `[Close]`;
    - again, with `Choose(Save)`, gives `Save` and then, after `Saved(Ok)`, `Close`.
  - `a_failed_save_keeps_the_changes`: `Saved(Err("disk full"))` keeps `dirty()` and shows `Dialog::Error`.
  - `play_hands_over_the_bake_and_changes_nothing`:
    - `Play` gives `Play { mod_id: "olx-classic", bots: 1, .. }` whose level equals `document().level()`;
    - an `AddLayer` while playing is ignored;
    - after `PlayEnded`, the document is unchanged;
    - `PlayBots(12)` stores 8 and emits `SaveSettings`.
  - `undo_redo_through_actions`: `Set(name)`, `Undo` and `Redo` change the element's name back and forth. Each emits `Upload`.
  - `a_refused_edit_says_why`: `Set("base", Property::PixelSize(2))` sets a status message containing "is not an image element" (`WrongKind`), and emits no command.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor state`. Expected: FAIL.
- [ ] **Step 3:** Implement `state.rs`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: the editor state: files, undo, play`.

### Task 7: The view, the tools and the canvas (`olr-editor`)

**Files:**
- Create: `crates/olr-editor/src/view.rs`, `crates/olr-editor/src/tools.rs`; `crates/olr-editor/tests/tools.rs`.
- Modify: `state.rs` (the actions below).

**Interfaces:**
- Produces:

  ```rust
  // view.rs: screen points ↔ world pixels (floats here only; R7).
  pub const ZOOMS: [f32; 10] = [0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0];
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct View { pub zoom: usize, pub offset: [f32; 2] } // offset: the art origin, canvas-relative
  impl View {
      /// The largest zoom showing the whole level (art size) in `canvas`, centred.
      pub fn fit(canvas: [f32; 2], art: (u32, u32)) -> View;
      pub fn scale(&self) -> f32; // ZOOMS[zoom]
      pub fn to_world(&self, p: [f32; 2], art_scale: u32) -> (i32, i32); // floor
      pub fn to_screen(&self, world: (i32, i32), art_scale: u32) -> [f32; 2];
      /// Zoom by `steps` index steps about canvas point `about` (the world point under it stays);
      /// at integer scales the offset is rounded to whole points (pixel-snapped).
      pub fn zoom_about(&mut self, steps: i32, about: [f32; 2]);
      pub fn pan(&mut self, delta: [f32; 2]);
  }
  // tools.rs
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Tool { Select, Brush, Eraser, Rect, Ellipse, Line, Spawn, Eyedropper } // V B E R O L P I
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum BrushShape { Round, Square }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct Brush { pub shape: BrushShape, pub size: u32 } // 1..=MAX_BRUSH, default Round 8
  pub const MAX_BRUSH: u32 = 256;
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum PaintChoice { Colour(Rgba), FrontTile, BackTile } // default Colour(140, 90, 40)
  /// What the canvas draws over the bake, in world pixels (the UI maps them to the screen).
  #[derive(Clone, Debug, PartialEq)]
  pub enum Overlay {
      Box { rect: Rect, selected: bool, locked: bool },  // image elements' boxes
      Handle { at: (i32, i32) },                          // 8 per selected unlocked image
      Outline { rect: Rect },                             // a resize or rect/ellipse preview
      Segment { from: (i32, i32), to: (i32, i32) },       // a line preview
      Brush { centre: (i32, i32), size: u32, shape: BrushShape },
      Spawn { at: (i32, i32), selected: bool },
  }
  // New EditorAction variants (state.rs):
  //   Tool(Tool), Brush(Brush), Paint(PaintChoice),
  //   Pointer { kind: PointerKind, world: (i32, i32), shift: bool },   // PointerKind: Down, Move, Up
  //   Nudge { dx: i32, dy: i32 }, Canvas([f32; 2]), Pan([f32; 2]), Zoom { steps: i32, about: [f32; 2] },
  //   MaterialView(bool), Grid(bool)
  // New EditorState accessors: tool(), brush(), paint(), view(), material_view(), grid(), overlays() -> Vec<Overlay>.
  ```

  The pointer, per tool:
  - **Select.**
    - Down on a handle of the selected unlocked image starts a resize: an `Outline` preview, applied on Up as one `Set(Size)` and `Set(At)` step (R8). Shift keeps the aspect ratio.
    - Down inside a visible image's box (the topmost under the point) selects it and starts a move. The move is live: `at` changes each Move and the old and new boxes rebake. On Up it becomes one `Element` edit.
    - Down within 3 world pixels of a spawn selects it and drags it (one `Objects` edit).
    - Anywhere else clears the selection.
    - `Nudge` moves the selected image's box by `(dx, dy)`, one step each.
  - **Brush and Eraser** need a selected unlocked drawn layer; otherwise `status.message` = "select a drawn layer to paint".
    - Down copies the layer (`layer_art`) and dabs a `Circle { r: size / 2 }` (Round) or a `size`-wide `Rect` (Square) centred on the point.
    - Each Move dabs and joins the previous point with a `Line { width: size }`.
    - Each dab rebakes and emits `Upload`.
    - Up pushes one `stroke_edit` step.
  - **Rect, Ellipse and Line:** Down records the start, Move updates the preview overlay, Up paints the shape (one step). Line's width is the brush size.
  - **Spawn:** Down adds a spawn at the point.
  - **Eyedropper:** Down sets `PaintChoice::Colour` to the bake's front at the art pixel `(x·s, y·s)` of the world point (made opaque), then returns to the previous tool.
  - `ImportImage` centres the new box on the world point at the canvas's centre (`View::to_world` of half the canvas size), or on the level's centre before the first `Canvas`.
  - Paint `FrontTile` and `BackTile` use the current theme's tiles (`Paint::Tile`).
  - Every Pointer sets `status.cursor` and `status.material` (inside the level only).
  - `Canvas(size)` on the first frame after `Opened` fits the view.

- [ ] **Step 1: Write the failing tests** in `tests/tools.rs` (the state driven by actions, with the test host from Task 6):
  - `view_maps_both_ways_and_zooms_about_the_cursor`:
    - `to_world(to_screen(p))` equals `p` at every zoom, for art scales 1-3;
    - `zoom_about` keeps the world point under `about`;
    - at scale ≥ 1 the offset is a whole number.
  - `a_brush_stroke_is_one_step`: select a new drawn layer, `Tool(Brush)`, then Down at (10, 10), Move to (40, 10) and Up.
    - The layer is opaque along the segment, with no gaps: every world x in 10..=40 at y 10.
    - One `Undo` clears it all.
  - `a_stroke_off_the_level_is_clipped`: a stroke from (-1000, -1000) to (100 000, 5) with size 256 does not panic. Undo restores the layer, and only in-level pixels changed.
  - `moving_an_image_is_live_and_one_step`: Select tool; Down inside "tiny"'s box, Move by (+5, +3), Up. `at` grew by (5, 3), and `cache_processed()` is unchanged. One `Undo` restores it.
  - `resizing_previews_then_applies`: drag the bottom-right handle by (+10, +10).
    - During the drag the element is unchanged and an `Overlay::Outline` shows the new box.
    - After Up the size grew by (10, 10).
    - With `shift: true`, the aspect ratio is kept (to within one pixel).
  - `locked_elements_are_not_changed_by_the_canvas`: lock "tiny".
    - A move drag, a `Nudge` and `Remove` leave it unchanged, record no step, and set a message containing "locked".
    - Selecting and painting a locked drawn layer is refused the same way.
  - `spawn_and_eyedropper`:
    - `Tool(Spawn)` and Down at (50, 60) add a spawn. Undo removes it.
    - `Tool(Eyedropper)` and Down on a painted pixel set `paint()` to its colour, and the tool goes back to the previous one.
  - `shapes_paint_on_release`: `Tool(Ellipse)`, Down (0, 0), Move (20, 10). The layer is unchanged, with an `Outline` overlay. Up paints it (one step).
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor tools`. Expected: FAIL.
- [ ] **Step 3:** Implement `view.rs`, `tools.rs` and the state's pointer handling.
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Also add an ignored `brush_dab_speed`: one size-16 dab and its rebake on Castle Strike, in release, printed. The spec's target is "within a frame"; note the time in the commit. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: the view, the tools and the canvas interaction`.
- [ ] **Step 6 (batch C):** Run the online `cargo xtask check`. Expected: every step `ok`.

### Task 8: The panels, the shortcuts and the flows (`olr-editor`)

**Files:**
- Create:
  - `crates/olr-editor/src/ui/{mod.rs, toolbar.rs, elements.rs, properties.rs, level.rs, status.rs, canvas.rs, dialogs.rs}`;
  - `crates/olr-editor/tests/flows.rs`.

**Interfaces:**
- Produces:

  ```rust
  /// Transient UI state: text drafts, the canvas rect last drawn, the Space key held.
  #[derive(Default)]
  pub struct EditorData { /* private */ }
  impl EditorData { pub fn canvas_rect(&self) -> Option<egui::Rect>; }
  /// Draws the editor into `ui` and returns the actions of this frame (as olr_ui::screens::show).
  pub fn show(ui: &mut egui::Ui, state: &EditorState, data: &mut EditorData) -> Vec<EditorAction>;
  /// The labels tests and 14c's e2e script find widgets by.
  pub mod labels {
      pub const UNDO: &str = "Undo";            pub const REDO: &str = "Redo";
      pub const SAVE: &str = "Save";            pub const PLAY: &str = "Play";
      pub const ADD_LAYER: &str = "Add layer";  pub const OBJECTS: &str = "Objects";
      pub const PIXEL_SIZE: &str = "Pixel size"; pub const COLOURS: &str = "Colours";
      pub const DITHER: &str = "Dither";        pub const THRESHOLD: &str = "Threshold";
      pub const MATERIAL: &str = "Material";    pub const VISIBLE: &str = "Visible";
      pub const LOCKED: &str = "Locked";        pub const NAME: &str = "Name";
      pub const DONT_SAVE: &str = "Don't save"; pub const CANCEL: &str = "Cancel";
  }
  ```

  Layout (spec §4):
  - **Toolbar (top):** the eight tools (their letter in the tooltip), brush shape and size, the paint (colour picker, Front tile, Back tile), Undo and Redo (disabled when not possible), Save, Play with its mod combo and bots (0-8), and the material view and grid toggles.
  - **Element list (left):** top first. Each row shows the name, eye and lock toggles, and its kind's icon text. Rows are reordered by drag (`egui::Ui::dnd_drag_source` and `dnd_drop_zone`, emitting `MoveElement`). Buttons: Add layer, and Import image (disabled with the tooltip "the file browser comes with the game's editor": 14c). An **Objects** section below lists "Spawn N at (x, y)".
  - **Properties (right):** the selection's fields with live edits: one `Set` per committed value; `DragValue`s commit on release. Level settings when nothing is selected: name, notes, theme combo, art scale, size and offset with Resize.
  - **Status bar (bottom):** the problems (errors first), the message, the cursor's world position and material.
  - **Canvas (centre):**
    - allocates the remaining rect and reports `Canvas(size)` when it changes;
    - maps egui pointer events through `View` to `Pointer` actions (shift from modifiers);
    - Space held plus a drag gives `Pan`, and the wheel gives `Zoom`;
    - paints `overlays()` with `ui.painter()` (boxes, handles, outlines, the brush cursor, spawn markers) and a pixel grid at scale ≥ 8 when enabled.

    The level itself is drawn by the host (14c); in 14b the canvas is an empty frame under the overlays.
  - **Dialogs:** a centred modal per `Dialog`, with Save / Don't save / Cancel, Change / Cancel, and OK.
  - **Shortcuts,** read from `ui.input` when no text field has focus:
    - Ctrl+Z → `Undo`, Ctrl+Y → `Redo`, Ctrl+S → `Save`, Delete → `Remove`;
    - V B E R O L P I → `Tool`;
    - the arrows → `Nudge` by 1.

- [ ] **Step 1: Write the failing flows** in `tests/flows.rs`, with an `App { state, data, commands }` harness as `olr-ui`'s `tests/flows.rs` builds it, 1280 × 720. The test host from Task 6 carries out the commands each frame. Each flow opens a copy of the fixture first.
  - `import_change_undo_redo`:
    - `ImportImage` with `gradient.jpg`'s bytes, as an action: the file browser is 14c's;
    - the list shows "gradient";
    - set Pixel size to 4 through the properties `DragValue` (type into it, then press Enter);
    - click Undo: the element's `pixel_size` is 1;
    - click Redo: it is 4.
  - `paint_with_the_brush`:
    - click the "Tunnels" row, press `B`;
    - drag across the canvas with raw `egui::Event::PointerButton` and `PointerMoved` events (at least 3 moves);
    - the layer changed, and one Undo restores it.
  - `place_a_spawn`: press `P`, click the canvas. The Objects section shows one more "Spawn".
  - `reorder_by_drag`: drag the "Badge" row onto "Gradient". The element order changes; the same works with "Tiny" locked (R4).
  - `save_then_check_the_files`: make an edit, click Save. On disk, `olr_pack::validate` reports no error, `editor/composition.ron` has the change, and the Save button stays enabled with `dirty() == false`.
  - `close_with_unsaved_changes`: make an edit, send `Close`. The dialog shows "Don't save"; clicking it records `EditorCommand::Close`.
  - `shortcuts_select_tools`: pressing each of `V B E R O L P I` sets the matching tool; Ctrl+Z undoes the last edit.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor flows`. Expected: FAIL.
- [ ] **Step 3:** Implement the `ui` modules. Follow `olr-ui`'s spacing and theme: the host applies `olr_ui::theme`, and `olr-editor` does not depend on `olr-ui`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor` and `cargo clippy -p olr-editor --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: the panels, the shortcuts and the headless flows`.

### Task 9: Docs and records

**Files:**
- Modify:
  - `crates/olr-editor/CLAUDE.md` (written in Task 4; complete it): the pattern, the document, the cache, the history, the tools, the labels, and what 14c adds.
  - The `CLAUDE.md` of `olr-format` (EXIF, `check_elements`) and `olr-edit` (`Decoded`, `ImageCache`, `bake_elements`, the shapes and art functions, `new_element_id`, `new_source_name`).
  - `docs/formats/README.md`: in "Composed levels", one sentence that JPEG originals are turned by their EXIF orientation when baked.
  - `CHANGELOG.md` `Unreleased`, Added: "The level editor's core (`olr-editor`): elements, painting, spawns, undo and redo, tested headless; the game hosts it in a later step."
  - `docs/porting/STATUS.md`: a row for the level editor core (plan 14b), and "Level editor decisions (plan 14b)" with rulings R1-R10.
  - The spec's §9 (14b done) and §3 (the cache's rule as R3 states it).
- [ ] **Step 1:** Write the records above. Run `cargo xtask format-docs --check` and `cargo nextest run -p olr-edit guide`. Expected: PASS.
- [ ] **Step 2:** Commit: `docs: the level editor's core: crate notes and plan 14b's records`.
