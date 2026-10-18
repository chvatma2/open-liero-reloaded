# Plan 14e: Textures, Fill, Crater Brushes, Clipboard and Multi-Select Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The editor's painting looks like the material it paints, and levels are built from reusable pieces.
- **Painting:**
  - every paint tool paints with a texture: a built-in set, the level theme's tiles, or the player's own textures imported from pictures;
  - a flood Fill tool;
  - the theme's crater shapes as brush shapes.
- **Building from pieces:**
  - copy, cut and paste of elements, across levels;
  - copy, cut and paste of a rectangle of pixels;
  - selecting several elements to move, copy, delete, lock and hide together;
  - `olr-cli level element duplicate` and `fill`.

**Architecture:**
- **Built-in textures** are generated in code (`olr_edit::textures`): deterministic integer noise and patterns, so there are no files, no pack kind and no licence to track (ruling R1).
- **The player's own textures** are PNGs in the data directory's `textures/`, kept by the host (`olr-game`).
- **Paint stays world-phased.** A texture is a `Paint::Tile`, which 14d already lays in step with the level's art origin.
- **Crater brushes** are a new `Shape::Stamp` holding a theme hole's cover mask. Every shape path (paint, erase, masks) therefore takes them as it is.
- **The clipboard** is `EditorState`'s and outlives the open document:
  - elements are copied as `Snapshot`s;
  - pixels are copied as a `PixelClip`, pasted as a new movable layer.
- **Multi-select** is `Selection::Many`.

**Tech Stack:** Rust 1.99, egui `=0.36.2`, egui_kittest `=0.36.2`, proptest, clap.

**Spec:** `docs/superpowers/specs/2026-10-18-level-editor-polish-design.md`. This plan covers, with plan 14d already done:
- goals 1-4 and 8-10;
- §Textures, §"Copy and paste of elements", §"Copy and paste of pixels", §"Flood fill", §"Hole stamps", §Multi-select;
- the texture library of §Formats;
- the §"Agent operations" `duplicate` and `fill … --texture`.

Plan 14d's decisions are in `docs/porting/STATUS.md`, "Level editor decisions (plan 14d)", and in the crates' `CLAUDE.md`.

## Global Constraints

- **Never panic on content** (repo rule 6). Texture files, pasted data, CLI arguments and pointer positions are untrusted. Every coordinate sum is `i64` or saturates.
- **Determinism** (repo rule 7): built-in textures and fills are integer only.
- **No format change.** The composition gains nothing; pasted elements are ordinary 14d elements. No pack kind.
- **Byte-identical:**
  - `content/packs`;
  - the oracle;
  - the match, render and UI goldens;
  - `fixtures/edit/composed-level`.
- **Limits:**
  - a texture is at most `MAX_TEXTURE_SIDE` = 512 art pixels a side;
  - the user library holds at most `MAX_USER_TEXTURES` = 256;
  - a texture file is read bounded, as an import is (`MAX_IMPORT_BYTES`);
  - a crater brush's scale is at most 16;
  - a fill visits at most the layer's box within the level;
  - the history cap stays `History::MAX_BYTES`.
- **Tolerance** is 14d's percentage rule (`olr_edit::level::similar`). Fill's default is 0 (exact), Remove colour's 10.
- **Verify once per batch:**
  - targeted `cargo nextest run -p <crate>` while working;
  - `rtk proxy cargo clippy -p <crate> --all-targets` before each commit;
  - `cargo xtask check` at a batch's end, with **no edits while it runs**.
- **The batches:**
  - Batch A is Tasks 1-2 (olr-edit, olr-cli);
  - Batch B is Tasks 3-5 (painting in the editor and the host);
  - Batch C is Tasks 6-8 (multi-select and the clipboard);
  - Batch D is Task 9 (docs).
- **Background review after each batch;** the three batch reviews are the whole-branch review (14d's practice).
- **`cargo xtask full` only after asking the user.** Report a failure and don't re-run.
- **Commits:** imperative subject, a blank line, then the two trailer lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Review Focus

1. **An image pasted into another level whose picture has the same file name but different bytes.**
   - Expected: the pasted image takes a new source name; both pictures stay intact after a save and reopen.
   - Test: Task 7, `a_pasted_image_never_overwrites_another_original`.
2. **Copying from a level at art scale 2 into one at art scale 1** (and the reverse).
   - Expected: drawn layers are resampled, images reprocessed, and nothing panics. A raster pasted into a level of another size is refused with a message.
   - Test: Task 7, `pasting_across_art_scales_resamples`.
3. **Flood fill on a whole open level, or from a seed outside the layer or the level.**
   - Expected: bounded by box ∩ level and fast; outside, nothing is changed.
   - Tests: Task 2, `fill_is_bounded_by_box_and_level` and the ignored `fill_speed`.
4. **A multi-selection holding a locked element.**
   - Expected: move and delete refuse, naming it, and change nothing. Unlocking applies to all. Each group action is one undo step.
   - Test: Task 6, `a_group_with_a_locked_member_refuses_moves`.
5. **Hostile texture files.**
   - Cases: a 600 × 600 PNG, a text file named `.png`, a 2 GB file, a user texture deleted outside the game.
   - Expected: refused with a message before it is read whole; the list is refreshed; never a panic.
   - Test: Task 5, `texture_imports_are_bounded`.

## Rulings made while writing this plan

- **R1 Built-in textures are code.**
  - `olr_edit::textures::builtin()` makes 64 × 64 tileable RGBA textures from a fixed integer hash: Dirt, Rock, Grass, Brick, Metal, Wood.
  - Tileable means the noise lattice repeats every 64 pixels, so the right edge meets the left.
  - Growing the set means adding a generator, or later a PNG embedded with `include_bytes!`.
  - No pack kind, no `content/packs` change, nothing to licence (spec §Formats leaves the means to the builder).
- **R2 The user library** is the data directory's `textures/`: one `<name>.png` per texture, named after the source file's stem and made unique (`name-2`, …).
  - The host lists it when the editor opens, adds (`AddTexture`) and removes (`RemoveTexture`), and answers `TexturesChanged`.
  - A file the host cannot read is listed as a warning, not offered.
- **R3 What paint can be:** `PaintChoice::Colour`, `FrontTile`, `BackTile` or `Texture(TextureRef)`, where `TextureRef` is `Builtin(usize)` or `User(String)`.
  - The state resolves it to a `Paint::Tile` when a stroke begins.
  - A user texture removed while chosen falls back to the default colour.
- **R4 Crater brushes:**
  - `BrushShape::Crater(u8)` uses the level theme's hole `i`. Its cover is every pixel that is neither black nor transparent (OLX's carve and paint pixels).
  - The scale is `(brush.size / max(hole side)).clamp(1, 16)`.
  - A stroke's joins between dabs are dabs every half crater along the segment, not a thick line.
- **R5 Fill:**
  - It covers the 4-connected world pixels of the selected drawn layer, from the clicked one, within box ∩ level.
  - Connected pixels share the seed's covered state; when covered, their block's centre art colour is `similar` to the seed's within the fill tolerance.
  - It paints whole blocks with the paint and "Paint as" material, as a brush does, and is one step (a conversion first when needed, as 14d's strokes).
  - Fill on a picture is refused with 14d's `PICTURE` message.
- **R6 The element clipboard** holds `Snapshot`s (items with their originals) and the art scale they came from.
  - Copies of drawn layers are cropped to their covered (or back-art) pixels, as spec C5 says, so they are small and movable.
  - Paste puts copies on top, offset `(4, 4)` world pixels from where they were copied; each paste offsets the clipboard by another `(4, 4)`, so repeated pastes cascade.
  - Each copy gets a new id (`new_element_id`) and a name `<name> 2`, `<name> 3`, … unique in the level. The copies are selected.
  - An original whose name the level uses for other bytes is renamed (`new_source_name`).
  - A raster pastes only into a level of its own size.
  - Cut is copy, then delete, as one step.
- **R7 Pixel paste makes a new layer.** The "Select pixels" tool (M) drags a rectangle on the selected drawn layer:
  - Ctrl+C copies its art and cells (`PixelClip`);
  - Ctrl+X copies, then erases it (14d's erase, so dirt over back art digs);
  - Delete erases it.

  Ctrl+V with pixels on the clipboard pastes them as a new boxed layer on top, at the copied place offset `(4, 4)`, selected (one step). It is moved with Select (14d R9) and merged into a layer with Merge down. The spec's floating paste is this: it allows "a simpler first version" and the step after it is Merge down.
- **R8 Multi-select:** `Selection::Many(Vec<String>)`, two or more elements, in stack order (bottom first).
  - **In the list:** Ctrl+click toggles an element in or out; Shift+click selects the range from the single selection.
  - **On the canvas, with Select:** Ctrl+click toggles too. A press on a member drags the whole group.
  - **Arrow keys** nudge the group.
  - **Delete, Copy, Cut, Lock, Hide and Material** apply to each. A move or delete with a locked member is refused, naming it.
  - **The properties panel** shows the count, Visible and Locked (on when all are), and Material when all are images or drawn layers (it reads "Different" when they disagree). Merge down and Split are not offered.
  - Each group action is one step.
- **R9 Shortcuts:**
  - Ctrl+C, Ctrl+X and Ctrl+V reach the editor both as key presses and as egui's `Event::Copy`, `Event::Cut` and `Event::Paste` (the window delivers the latter). Neither form acts while a text field has the keyboard.
  - G is Fill, M is Select pixels.
  - Escape drops the pixel rectangle.
- **R10 `olr-cli`:**
  - `duplicate <id>` puts the copy directly above the original at the same place and prints `added element <id>`; an image shares its original, and a mask or layer file is copied under the new id.
  - `fill <id> --at X,Y [--tolerance N] <paint> [--material M]` fills as R5.
  - `--texture` is an alias of `--tile` on `draw` and `fill`.

---

### Task 1: Built-in textures and crater shapes (`olr-edit`)

**Files:**
- Create: `crates/olr-edit/src/textures.rs` (and `pub mod textures;`).
- Modify: `crates/olr-edit/src/level.rs`:
  - `Shape` loses `Copy` (now `Clone`) and gains `Stamp`;
  - `StampMask`;
  - `extent`/`inside` for stamps.
- Modify: every `Shape` user that copied it (build errors guide; `.clone()` or `&`).
- Test: `crates/olr-edit/tests/textures.rs` (a `mod` of `tests/integration.rs`), `tests/level.rs`.

**Interfaces:**
- Produces:
  ```rust
  // olr_edit::textures
  pub struct Texture { pub name: &'static str, pub image: RgbaImage }
  pub const BUILTIN_SIDE: u32 = 64;
  /// R1's set, in this order: Dirt, Rock, Grass, Brick, Metal, Wood.
  pub fn builtin() -> Vec<Texture>;
  // olr_edit::level
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct StampMask { pub size: (u32, u32), pub cover: Vec<bool> }
  impl StampMask {
      /// A theme hole (OLX colours: black untouched, else carve or paint): covered where not black and not transparent.
      pub fn from_hole(img: &RgbaImage) -> StampMask;
  }
  pub enum Shape { /* Rect, Circle, Ellipse, Line as before */
      /// `mask` centred on `(x, y)` (its top-left at `x - w·scale/2`), each mask pixel `scale` world pixels.
      Stamp { x: i32, y: i32, scale: u32, mask: std::sync::Arc<StampMask> },
  }
  ```

- [ ] **Step 1: Write the failing tests.**
  - `textures.rs`:
    - `builtin_textures_are_six_opaque_tiles` (names, 64 × 64, every alpha 255);
    - `builtin_textures_are_deterministic_and_distinct` (two calls equal; no two equal; each has at least 8 colours);
    - `builtin_textures_tile` (for every row, the colour step from column 63 to column 0 is no larger than the largest step between neighbours inside the row; the same for columns).
  - `level.rs`:
    - `a_stamp_covers_its_mask_scaled`: a 3 × 2 mask `[1,0,1, 0,1,0]` at `(10, 10)` with scale 2; `draw_art` covers exactly the world pixels of the covered cells, its top-left at `(7, 8)`;
    - `a_stamp_far_or_huge_never_panics` (`x: i32::MAX`, scale 16, a 64 × 64 mask; clipped, no overflow);
    - `from_hole_takes_carve_and_paint`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit textures level`. Expected: FAIL (no module, no variant).
- [ ] **Step 3: Implement `textures.rs`, `StampMask`, `Shape::Stamp`.**
  - `extent` and `inside` for `Stamp` work in `i64`.
  - Fix every `Shape` copy across the workspace (`cargo build --workspace --all-targets`).
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit` and `cargo build --workspace --all-targets`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: built-in textures and stamp shapes`.

### Task 2: Flood fill, `duplicate` and the CLI (`olr-edit`, `olr-cli`)

**Files:**
- Modify: `crates/olr-edit/src/drawn.rs` (`fill`; the per-block paint shared with `draw`).
- Modify: `crates/olr-edit/src/element.rs` (`fill`, `duplicate`, `copy_name`).
- Modify: `crates/olr-cli/src/element_cmd.rs` (`fill`, `duplicate` subcommands); `level_cmd.rs` (`--texture` alias of `--tile`).
- Test: `crates/olr-edit/tests/drawn.rs`, `tests/element.rs`, `crates/olr-cli/tests/element.rs`.

**Interfaces:**
- Produces:
  ```rust
  // olr_edit::drawn
  /// R5: fills the region of world pixel `seed` in drawn data `d` (box at `at`) within the box and a
  /// level of `level`; the level rectangle changed, `None` when the seed is outside or nothing changed.
  pub fn fill(d: &mut Decoded, at: (i32, i32), s: u32, level: (u32, u32), seed: (i32, i32),
              paint: &Paint, material: Material, tolerance: u8) -> Option<Rect>;
  // olr_edit::element
  pub fn fill(pack: &mut Pack, id: &str, seed: (i32, i32), paint: &Paint, tolerance: u8,
              material: Option<ElementMaterial>) -> Result<(), ElementError>;
  /// R10: the copy directly above `id`; its new id.
  pub fn duplicate(pack: &mut Pack, id: &str) -> Result<String, ElementError>;
  /// `name` with the next free ` N` suffix among `names` ("Crate" → "Crate 2", "Crate 2" → "Crate 3").
  pub fn copy_name<'a>(name: &str, names: impl Iterator<Item = &'a str> + Clone) -> String;
  ```

- [ ] **Step 1: Write the failing tests.**
  - `drawn.rs`:
    - `fill_takes_the_connected_region`: a 5 × 1 layer, covered `[B, B, -, B, B]` (B brown); filling from `(0, 0)` with grey paints `0..2` only;
    - `fill_of_empty_space_stops_at_covered_pixels`;
    - `fill_tolerance_follows_similar`;
    - `fill_is_bounded_by_box_and_level`: a box half off the level fills only inside; seeds at `(-1, 0)` and `(i32::MAX, 0)` give `None`;
    - `fill_sets_cells_with_material`;
    - the ignored `fill_speed`: a 4096 × 2048 empty box, one fill, printed (target under 300 ms in release).
  - `element.rs`:
    - `duplicate_copies_files_and_shares_originals`: a drawn layer, a masked image and a raster each duplicated directly above. The new ids, `copy_name`d names, files under the new ids; the image's `source` equal to the original's; `check_composition` passes.
    - `copy_name_counts_on`.
    - `fill_paints_a_texture_into_a_layer`.
  - `olr-cli` `tests/element.rs`, `fill_and_duplicate_commands`:
    - `duplicate {pack} sign` prints `added element sign-2`;
    - `fill {pack} background --at 5,5 --texture <png>` exits 0;
    - `pack check` is clean.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit drawn element` and `-p olr-cli element`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The fill uses an explicit stack and a visited `Vec<bool>` the size of box ∩ level.
  - `element::fill` converts a plain layer to mixed for another material, as `element::draw` does.
- [ ] **Step 4:** Run the same commands, then `cargo nextest run -p olr-edit --release --run-ignored only fill_speed --no-capture`. Expected: PASS; under 300 ms.
- [ ] **Step 5:** Commit: `olr-edit, olr-cli: flood fill, duplicate, --texture`.

Batch A ends: `cargo xtask check` (no edits while it runs); a background review of `git diff <batch base>..HEAD`.

### Task 3: Textures, Fill and craters in the editor's state (`olr-editor`)

**Files:**
- Modify: `crates/olr-editor/src/catalog.rs`: `ThemeEntry::holes: Vec<Arc<StampMask>>`; `Catalog::textures: Vec<TextureEntry>` (the user library); `TextureEntry { name: String, image: RgbaImage }`.
- Modify: `crates/olr-editor/src/tools.rs`:
  - `PaintChoice::Texture(TextureRef)` (`PaintChoice` loses `Copy`);
  - `TextureRef`;
  - `BrushShape::Crater(u8)`;
  - `Tool::Fill` (G);
  - `Tool::ALL` with 10.
- Modify: `crates/olr-editor/src/state.rs`:
  - `builtin: Arc<Vec<olr_edit::textures::Texture>>` made in `new`;
  - `fill_tolerance: u8` (0);
  - `Tolerance(t)` sets the chosen tool's (Wand or Fill);
  - the commands `AddTexture(PathBuf)` and `RemoveTexture(String)`, answered by the action `TexturesChanged(Vec<TextureEntry>)`;
  - `Browse(Purpose::ImportTexture)`: a pick sends `AddTexture`.
- Modify: `crates/olr-editor/src/browser.rs` (`Purpose::ImportTexture`: the image filter).
- Modify: `crates/olr-editor/src/pointer.rs`: `paint_of` resolves textures; crater dabs (R4); `Tool::Fill` press.
- Modify: `crates/olr-editor/src/ops.rs`:
  ```rust
  /// R5 as one step (its conversion first); the edits applied.
  pub fn fill(&mut self, id: &str, seed: (i32, i32), paint: &Paint, material: Option<Material>, tolerance: u8)
      -> Result<Vec<Edit>, DocError>;
  ```
- Test: `crates/olr-editor/tests/tools.rs`, `tests/state.rs`, `tests/common.rs` (the catalog's holes from the theme).

- [ ] **Step 1: Write the failing tests.**
  - `tools.rs`:
    - `a_texture_paints_in_step_with_the_world`: Brush with `PaintChoice::Texture(TextureRef::Builtin(3))` on a layer moved by `(3, 2)`; each painted art pixel equals the texture at `(ax mod 64, ay mod 64)` in level art coordinates.
    - `the_fill_tool_fills_one_step`: Fill on the fixture's `tunnels`; `Undo` restores.
    - `a_crater_brush_paints_the_hole_shape`: `BrushShape::Crater(0)` paints exactly hole 0's cover at the click.
    - `fill_on_a_picture_is_refused`.
  - `state.rs`:
    - `textures_changed_replaces_the_user_library`;
    - `a_removed_texture_falls_back_to_the_colour`;
    - `a_texture_pick_asks_the_host_to_add_it`: Browse ImportTexture, then Pick, gives `[AddTexture(path), SaveSettings(..)]`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor`. Expected: FAIL.
- [ ] **Step 3: Implement.** Fix `PaintChoice` copies (`state.paint()` returns `&PaintChoice`).
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: textures, fill and crater brushes`.

### Task 4: The palette and tool options (`olr-editor` UI)

**Files:**
- Modify: `crates/olr-editor/src/ui/toolbar.rs`:
  - in the brush and paint row, a "Crater" picker after Square (its entries `Crater 1`… from the theme; none: hidden);
  - a "Textures…" button showing the current paint's swatch, opening a popup of swatches grouped "Theme" (front, back), "Built-in" and "Yours" (plus "Import texture…"; a right-click on one of yours offers "Remove");
  - Fill's Tolerance in the options slot.
- Modify: `crates/olr-editor/src/ui/mod.rs`:
  - labels `TEXTURES` "Textures…", `IMPORT_TEXTURE` "Import texture…", `CRATER` "Crater";
  - `EditorData` caches the swatches' egui textures by key (`builtin/<i>`, `user/<name>`, `theme/front|back`), dropping those no longer listed.
- Test: `crates/olr-editor/tests/flows.rs`.

- [ ] **Step 1: Write the failing tests** (`flows.rs`).
  - `the_palette_offers_every_texture`: click "Textures…"; six built-in swatches labelled by name, the theme's two; clicking "Brick" sets `PaintChoice::Texture(Builtin(3))`.
  - `import_texture_opens_the_browser_for_images`.
  - `the_crater_picker_lists_the_themes_holes`.
  - The toolbar tests (`the_toolbar_stays_in_the_window_and_in_place`, `a_toolbar_row_is_one_straight_line`) pass with Fill chosen and a texture chosen. "Remove colour" and "Fill" are in their name lists.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor flows`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: the texture palette and crater picker`.

### Task 5: The host's texture library and theme holes (`olr-game`, `olr-ui`)

**Files:**
- Modify: `crates/olr-ui/src/store.rs`: `pub fn textures_dir(&self) -> Option<PathBuf>` (`<data>/textures`).
- Modify: `crates/olr-game/src/editor/mod.rs`:
  - the catalog's theme holes (`StampMask::from_hole` of `LoadedTheme::holes`);
  - the user library listed at `new` (`files::list_textures`);
  - `AddTexture`, `RemoveTexture`.
- Modify: `crates/olr-game/src/editor/files.rs`:
  ```rust
  /// The PNGs of `dir` (at most MAX_USER_TEXTURES, name order), each within MAX_TEXTURE_SIDE;
  /// unreadable ones as warnings.
  pub fn list_textures(dir: &Path) -> (Vec<TextureEntry>, Vec<String>);
  /// Reads `path` bounded (as read_image), refuses a side above MAX_TEXTURE_SIDE, writes it as
  /// `<dir>/<unique stem>.png`; its name.
  pub fn add_texture(dir: &Path, path: &Path) -> Result<String, String>;
  pub fn remove_texture(dir: &Path, name: &str) -> Result<(), String>; // plain names only
  ```
- Test: `crates/olr-game/tests/editor_host.rs`.

- [ ] **Step 1: Write the failing tests** (`editor_host.rs`).
  - `textures_are_added_listed_and_removed`: add a 32 × 32 PNG twice, giving `tex` and `tex-2`; a new host lists both; remove `tex`.
  - `texture_imports_are_bounded`: a 600 × 600 PNG, a text file named `.png` and a sparse file of 2 GB are refused with messages, nothing written. `remove_texture("../x")` is refused.
  - `the_catalog_has_the_themes_holes`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-game editor_host`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run `cargo nextest run -p olr-game -p olr-ui`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-game: the editor's texture library and the themes' craters`.

Batch B ends: `cargo xtask check`; a background review.

### Task 6: Multi-select (`olr-editor`)

**Files:**
- Modify: `crates/olr-editor/src/state.rs`:
  - `Selection::Many(Vec<String>)`;
  - `EditorAction::Pointer` gains `toggle: bool` (Ctrl);
  - `EditorAction::SetMany(Vec<String>, Property)`;
  - `Remove` for `Many`.
- Modify: `crates/olr-editor/src/pointer.rs`: Ctrl+press toggles; `Drag::MoveMany { befores: Vec<Element>, from }`; nudge of a group; overlays of each member's box.
- Modify: `crates/olr-editor/src/ops.rs`:
  ```rust
  /// One property on each of `ids`, checked together: one step's edits (applied).
  pub fn set_many(&mut self, ids: &[String], property: Property) -> Result<Vec<Edit>, DocError>;
  /// Removes `ids` (top first): one step's edits (applied); refused when any is locked.
  pub fn remove_many(&mut self, ids: &[String]) -> Result<Vec<Edit>, DocError>;
  ```
- Modify: `crates/olr-editor/src/ui/elements.rs` (Ctrl and Shift clicks), `ui/properties.rs` (the group panel), `ui/canvas.rs` (Ctrl to `toggle`).
- Test: `crates/olr-editor/tests/tools.rs`, `tests/state.rs`, `tests/flows.rs`.

- [ ] **Step 1: Write the failing tests.**
  - `state.rs`:
    - `a_group_moves_and_undoes_as_one_step`: `Many([tiny, gradient])` nudged by `(2, 0)`; both moved; one `Undo` restores both.
    - `a_group_with_a_locked_member_refuses_moves`: `tiny` locked; a nudge and a Remove change nothing and name `tiny`; `SetMany(Locked(false))` unlocks both in one step.
    - `remove_many_is_one_step`.
  - `tools.rs`, `ctrl_clicks_build_a_group_on_the_canvas`: Ctrl-click `tiny`, then `gradient`, gives `Many`; Ctrl-click `tiny` again gives `Element(gradient)`.
  - `flows.rs`:
    - `shift_click_selects_a_range_in_the_list`;
    - `the_group_panel_shows_the_count`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor`. Expected: FAIL.
- [ ] **Step 3: Implement R8.** Actions that need one element (Merge down, Split, properties' fields) see `Many` as no single selection.
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: select several elements`.

### Task 7: The element clipboard (`olr-editor`)

**Files:**
- Create: `crates/olr-editor/src/clipboard.rs`:
  ```rust
  pub enum Clipboard {
      Elements { art_scale: u32, items: Vec<Snapshot> },
      Pixels { art_scale: u32, at: (i32, i32), clip: PixelClip },
  }
  ```
- Modify: `crates/olr-editor/src/ops.rs`:
  ```rust
  /// Copies of `ids` (drawn layers cropped to their pixels, R6), with their originals.
  pub fn copy(&self, ids: &[String]) -> Vec<Snapshot>;
  /// R6: pastes `items` (from art scale `from`) on top, moved by `offset`; the edits (applied) and the new ids.
  pub fn paste(&mut self, items: &[Snapshot], from: u32, offset: (i32, i32))
      -> Result<(Vec<Edit>, Vec<String>), DocError>;
  ```
  `olr_edit::drawn::crop(p: &Placed, s: u32) -> Option<Placed>` crops to the opaque art or back-art bounding box.
- Modify: `crates/olr-editor/src/state.rs`: `clipboard: Option<Clipboard>`, kept across Open/New/Close; actions `Copy`, `Cut`, `Paste`.
- Modify: `crates/olr-editor/src/ui/mod.rs`: R9's shortcuts.
- Test: `crates/olr-editor/tests/document.rs`, `tests/state.rs`, `tests/flows.rs`.

- [ ] **Step 1: Write the failing tests.**
  - `document.rs`:
    - `a_copy_pastes_as_an_independent_element`:
      - copy `badge` (masked) and paste it;
      - the new element `badge-2`, named "Badge 2", at `(-2, 104)`, sharing `badge.png`, with its own mask file on save;
      - painting the copy's mask leaves the original's;
      - one undo removes it.
    - `a_drawn_copy_is_cropped_to_its_pixels`: copying the fixture's `tunnels` gives a boxed layer covering its painted pixels only.
    - `a_pasted_image_never_overwrites_another_original`: copy the fixture's `tiny` (`tiny.jpg`) into a Dirt Level document that already has an image `tiny.jpg` of other bytes. The paste's source is `tiny-2.jpg`, and both decode as they were after a save and reopen.
    - `pasting_across_art_scales_resamples`: drawn and image copies from the fixture (art scale 1) into a level at art scale 2 bake their art at double size. A raster into a level of another size is `DocError::Bad`.
  - `state.rs`:
    - `cut_and_paste_are_one_step_each`;
    - `the_clipboard_survives_opening_another_level`;
    - `pastes_cascade` (two pastes at `+4` and `+8`).
  - `flows.rs`: `ctrl_c_and_ctrl_v_copy_and_paste` (key presses, and egui's `Event::Copy` / `Event::Paste`).
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor`. Expected: FAIL.
- [ ] **Step 3: Implement R6 and R9.**
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: copy, cut and paste elements`.

### Task 8: Pixel copy and paste (`olr-editor`)

**Files:**
- Modify: `crates/olr-editor/src/tools.rs`: `Tool::SelectPixels` (M, "Select pixels"); `Tool::ALL` with 11.
- Modify: `crates/olr-editor/src/state.rs`:
  - `pixel_rect: Option<(String, Rect)>`, cleared on a tool change, Escape, Open and Close;
  - `Copy`, `Cut`, `Paste` and `Remove` act on it first.
- Modify: `crates/olr-editor/src/ops.rs`:
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct PixelClip { pub size: (u32, u32), pub art: RgbaImage, pub cells: Vec<Material> }
  /// `rect` (level) of drawn layer `id`, within its box: its art and cells (a plain layer's material).
  pub fn copy_pixels(&self, id: &str, rect: Rect) -> Option<(PixelClip, (i32, i32))>;
  /// A new boxed layer of `clip` at `at` (from art scale `from`), on top: the edit (applied) and its id.
  pub fn paste_pixels(&mut self, clip: &PixelClip, from: u32, at: (i32, i32)) -> Result<(Edit, String), DocError>;
  ```
- Modify: `crates/olr-editor/src/pointer.rs` (the drag, its outline overlay).
- Test: `crates/olr-editor/tests/tools.rs`, `tests/document.rs`.

- [ ] **Step 1: Write the failing tests.**
  - `tools.rs`:
    - `pixels_copied_paste_as_a_layer`: select a 10 × 10 rectangle of `tunnels`, Ctrl+C, Ctrl+V. A new layer named "Pasted" at the rectangle offset by `(4, 4)` holds those pixels and is selected.
    - `cut_pixels_erases_them_in_one_step`.
    - `delete_erases_the_pixel_rectangle`.
    - `escape_drops_the_pixel_rectangle`.
  - `document.rs`:
    - `copied_rock_pixels_stay_rock` (spec C3: the pasted layer is plain rock, or mixed when the clip mixes);
    - `copy_pixels_outside_the_layer_is_none`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor`. Expected: FAIL.
- [ ] **Step 3: Implement R7.**
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor -p olr-game`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: copy and paste pixels`.

Batch C ends: `cargo xtask check` and `cargo xtask e2e`; a background review.

### Task 9: Docs (Batch D)

**Files:**
- Modify: `docs/porting/STATUS.md`: "Level editor decisions (plan 14e)", with R1-R10.
- Modify:
  - `crates/olr-edit/CLAUDE.md` (`textures`, `Shape::Stamp`, `drawn::fill`/`crop`, `element::fill`/`duplicate`);
  - `crates/olr-editor/CLAUDE.md` (palette, tools, clipboard, multi-select, shortcuts);
  - `crates/olr-game/CLAUDE.md` (the texture library, the holes);
  - `crates/olr-cli/CLAUDE.md` (`fill`, `duplicate`, `--texture`);
  - `crates/olr-ui/CLAUDE.md` (`textures_dir`).
- Modify: the spec's Status line ("14e done …").

- [ ] **Step 1:** Write the docs. No check is needed for docs only.
- [ ] **Step 2:** Commit: `docs: plan 14e's decisions`.
- [ ] **Step 3:** Ask the user before `cargo xtask full`; then offer `cargo xtask dist`.
