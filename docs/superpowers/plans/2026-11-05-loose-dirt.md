# Loose Dirt Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A level may mark dirt pixels loose, of named loose materials.
Loose dirt falls and slides like sand, deterministically in the
simulation. It is drawn, authored, transferred and rolled back like
the rest of the terrain.

**Architecture:**
- **Terrain:** `olr-terrain`'s `Terrain` gains an optional per-cell
  loose-kind layer, journaled with the cells.
- **World:** `olr-sim` gains `loose.rs`. It holds the materials' rates
  (`LooseLevel`, from `MatchSetup`) and a small `LooseState` (moving
  grains, awake chunks, last part boxes), stepped at the end of
  `World::frame`.
- **Presentation:** each tick's moves reach `ColorMap::move_grain`
  through `FxTick`.
- **Format and authoring:** `level.ron`'s `loose` list and `loose.png`,
  baked from elements' `loose` field by `olr-edit`. `olr-cli` and the
  editor author them.

**Tech Stack:** Rust (`no_std` simulation crates), wgpu renderer, RON
formats, Python stdlib level scripts.

**Spec:** `docs/superpowers/specs/2026-11-05-loose-dirt-design.md`.
The spike's code is `/tmp/claude-1000/-home-martin-rust-liero/f872d925-9fa1-4bf1-a1be-23415997760e/scratchpad/loose-dirt-spike.patch`
(`crates/olr-cli/src/spike_loose.rs`). It is reference for the step's
rules only, never to be merged.

## Global Constraints

- `cargo xtask check` before every commit; `--render` for the renderer
  task and for the factory content task. Docs-only commits are exempt.
- Simulation crates (`olr-terrain`, `olr-sim`, `olr-data`) stay
  `#![no_std]` + `alloc` and build for wasm32.
- Never panic on content. `loose.png` and `level.ron`'s `loose` are
  untrusted.
- Deterministic: no `HashMap` iteration, no floats in the step's
  decisions beyond the fixed `tan` thresholds computed at setup, and no
  wall clock.
- A level without loose dirt changes no byte. The oracle traces, every
  match golden, the render goldens, `lit-hashes.ron`, the checksums and
  state transfer all stay as they are.
- Limits:
  - at most 16 loose materials;
  - `repose` 5-85, default 45;
  - `gravity` 1-2000, default 200 px/s²;
  - `top_speed` 1-500, default 50 px/s;
  - `slide_speed` 0-100, default 25 px/s;
  - `MAX_LOOSE_MOVES` 20,000 a tick;
  - chunks of 16 × 16 px.
- Sub-pixel units: 256 a pixel.
  - gravity: `round(g/10000×256)`, at least 1;
  - top speed: `round(v/100×256)`;
  - slide chance: `min(slide/100, 1)` × 65536.
- Shipped paths within 80 characters. Commits end with the session's
  `Co-Authored-By` and `Claude-Session` lines.
- Every crate edited: read its `CLAUDE.md` first, and update it with
  what changed.

## Review Focus

1. A hostile `loose.png` (a value past the list, the wrong size, loose on
   rock or background) is refused by `pack check`. Played leniently, the
   level plays without its loose dirt, with a warning. Never a panic.
   *(Task 4.)*
2. A grain dug or carved while it is moving: its kind clears and its
   moving entry is dropped on the next visit. Nothing is resurrected by a
   later `place_dirt`. *(Task 1 and Task 3.)*
3. Grains at the map's edges (x 0, x w−1, the bottom row) never index
   out of bounds. Outside the map counts as rock. *(Task 3.)*
4. A mid-collapse snapshot restored and re-stepped, and a mid-collapse
   state transfer, give the same checksums as uninterrupted stepping.
   *(Task 3.)*
5. A world frozen after the match's end moves no grain, and a level with
   loose dirt but no living worm still settles. *(Task 3.)*

---

### Task 1: The terrain's loose layer (`olr-terrain`)

**Files:**
- Modify: `crates/olr-terrain/src/lib.rs`, `crates/olr-terrain/src/journal.rs`, `crates/olr-terrain/src/dirt.rs`, `crates/olr-data/src/level.rs` (Task 3's `LooseMaterial` may land here first)
- Test: `crates/olr-terrain/tests/loose.rs` (new, `mod loose;` in the
  crate's integration binary)

**Interfaces:**
- Produces:
  - `Terrain::with_loose(self, kinds: Vec<u8>) -> Option<Terrain>`. A
    kind per cell; `None` on a wrong length. A kind on a non-dirt cell
    is cleared.
  - `Terrain::has_loose(&self) -> bool`.
  - `Terrain::loose(&self, x: i64, y: i64) -> u8`: 0 outside or not
    loose.
  - `Terrain::move_grain(&mut self, from: u32, to: u32)`: cell indices.
    `to` becomes dirt of `from`'s kind and `from` becomes background,
    both journaled.
  - `Terrain::take_touched(&mut self) -> impl Iterator<Item = Area>`:
    the rectangles of `carve_hole`, `place_dirt` and `place_green_dirt`
    since the last call. Kept only with a loose layer; not journaled.
  - `Terrain::loose_cells(&self) -> Option<&[u8]>`.
  - `Terrain::load_loose(&mut self, kinds: Option<&[u8]>) -> bool`: state
    transfer, all or nothing, beside `load_cells`.

- [ ] **Step 1: Write the failing tests.** Each one in `tests/loose.rs`:
  - `a_carve_clears_the_kinds_it_digs`: a 4×4 dirt grid with kinds all
    1; carve a 1-px stamp; `loose(x, y) == 0` there and 1 elsewhere.
  - `placed_dirt_is_not_loose`: after a carve, `place_dirt` over the same
    pixel gives `loose == 0`.
  - `undo_restores_kinds_and_cells`: `start_journal`, mark,
    `move_grain(0, 4)`, `undo_to(mark)`. Cells and kinds equal the
    start's, and `cell_hash` equals the start's.
  - `kinds_of_zero_hash_as_before`: `cell_hash` of a grid equals that of
    the same grid after `with_loose(vec![0; n])` (a kind of 0 adds
    nothing to a cell's term), and that terrain's `has_loose()` is
    true.
  - `kinds_on_rock_are_cleared`: `with_loose` keeps kinds only on dirt.
  - `take_touched_reports_edits`: a carve's clipped rectangle comes out
    once, then nothing.
- [ ] **Step 2: Run** `cargo nextest run -p olr-terrain loose`. Expected:
  fails to compile (the methods don't exist).
- [ ] **Step 3: Implement.**
  - `loose: Option<Box<[u8]>>` on `Terrain`, beside `cells`.
  - `PartialEq` compares it.
  - `record` saves its pre-image with the cells' (extend the journal
    entry's bytes: the cell bytes, then the kind bytes when the layer
    exists).
  - `set_cell` gets a sibling `set_kind` that updates the hash term.
  - `journal::term(i, cell)` becomes `term(i, cell | kind << 2)`. A kind
    of 0 keeps today's value, so a level without loose hashes as before.
  - `carve_hole`, `place_dirt` and `place_green_dirt` zero the kind of
    every cell they change, and push their clipped rectangle into
    `touched` (a `Vec<Area>`, at most 256 merged as
    `take_restored_areas` does) when the layer exists.
  - `material_hash` (the oracle's) is unchanged.
- [ ] **Step 4: Run** `cargo nextest run -p olr-terrain`. Expected: all
  pass, the old journal tests too.
- [ ] **Step 5: Commit** `olr-terrain: an optional loose-kind layer, journaled with the cells`.

### Task 2: Grains move their colour (`ColorMap::move_grain`)

**Files:**
- Modify: `crates/olr-terrain/src/color.rs`
- Test: `crates/olr-terrain/src/color_tests.rs`, `crates/olr-terrain/tests/glow.rs`

**Interfaces:**
- Consumes: Task 1's `Terrain::move_grain`.
- Produces: `ColorMap::move_grain(&mut self, from: (u32, u32), to: (u32, u32))`.
  It is the spike's `spike_move`:
  - the art block is copied to `to`;
  - `to` glows none;
  - `from` shows the backdrop with the back glow;
  - the map's own material does the move;
  - both pixels are marked dirty (not colour-only).

- [ ] **Step 1: Write the failing tests.**
  - `a_moved_grain_takes_its_colour`: a 3×3 map at art scale 2. The dirt
    pixel's block is painted red, then moved down one pixel. The
    destination block is red and the source block equals the back art.
    `take_dirty` covers both.
  - In `tests/glow.rs`, `a_moved_grain_glows_none_and_reveals_the_backs_glow`.
- [ ] **Step 2: Run** `cargo nextest run -p olr-terrain move`. Expected:
  fails to compile.
- [ ] **Step 3: Implement** it as the spike's `spike_move`, with a
  bounds check (a pixel outside the map changes nothing).
- [ ] **Step 4: Run** `cargo nextest run -p olr-terrain`. Expected: pass.
- [ ] **Step 5: Commit** `olr-terrain: ColorMap::move_grain carries a grain's colour`.

### Task 3: The step (`olr-sim`)

**Files:**
- Create: `crates/olr-sim/src/loose.rs`, `crates/olr-sim/src/loose_tests.rs`
- Modify:
  - `crates/olr-sim/src/lib.rs` (exports);
  - `crates/olr-sim/src/setup.rs` (`MatchSetup::loose`);
  - `crates/olr-sim/src/world.rs` (the field, the phase after
    `script_phase`, `loose_moves()`);
  - `crates/olr-sim/src/checksum.rs`;
  - `crates/olr-sim/src/snapshot.rs`;
  - `crates/olr-sim/src/state.rs`.

**Interfaces:**
- Consumes: Task 1.
- Produces:
  - In `olr-data` (`level.rs`), `pub struct LooseMaterial { pub repose: u8, pub gravity: u16, pub top_speed: u16, pub slide_speed: u8 }`,
    in the format's units, and `LooseMaterial::DEFAULT` =
    `{45, 200, 50, 25}`. `olr-sim` re-exports it.
  - `pub struct LooseLevel { pub materials: Vec<LooseMaterial> }`, with
    `LooseLevel::new(materials) -> Option<LooseLevel>`, which refuses
    more than 16 or out-of-range values.
  - `MatchSetup::loose: Option<Arc<LooseLevel>>`, default `None`. Setup
    refuses a terrain kind past the list (`SetupError`).
  - `pub const MAX_LOOSE_MOVES: u32 = 20_000`.
  - `World::loose_moves(&self) -> &[(u32, u32)]`: this tick's moves,
    `(from, to)` cell indices in order; empty without loose dirt.
  - `World::loose_state_len(&self) -> usize`: moving grains, for tests.

- [ ] **Step 1: Write the failing tests** in `loose_tests.rs`. Each builds
  a world with `MatchSetup` on a hand-made terrain (rock frame, dirt
  kinds) and no worms unless named:
  - `a_grain_falls_from_rest_and_reaches_its_top_speed`: one grain 100 px
    above a floor. After tick 1 it has not moved (0.02 px fallen). Its
    fall over the first 100 ticks is within 1 px of
    `Σ min(t·g, top)/100` from the spec's units. After landing it is
    still.
  - `a_column_falls_as_one`: a 1×10 column over a 40 px drop. Every
    tick the column's pixels stay contiguous (no gap).
  - `a_dug_hole_fills`: a 20×20 loose block on rock. Carve a 5-px hole
    in its middle; within 300 ticks the hole's pixels hold dirt again.
  - `piles_settle_at_their_repose`: pour 400 grains (a 20×20 block
    dropped onto a flat floor) at 25°, 45° and 65°, run 2000 ticks.
    Fit the pile's slope from its column heights. The angle is within
    ±8° of the repose.
  - `a_worm_holds_grains_up`: a worm under a loose block keeps the
    grains above its box (`|dx| ≤ 3, -4 ≤ dy ≤ 5`) for 100 ticks.
  - `a_live_rock_part_holds_grains_up_until_it_goes`: use the live test
    helpers of `live_step_tests.rs` (a copy with a rock part). Grains on
    the part stay. Hide the part (`Visible` 0) and they fall within 100
    ticks.
  - `a_frozen_world_moves_none`: after the match's end (the referee
    frozen), `loose_moves()` stays empty.
  - `the_cap_holds`: 30,000 unsupported grains. Tick 1 moves at most
    `MAX_LOOSE_MOVES`, and the rest move on later ticks.
  - `edges_never_index_out`: grains at every edge and corner, including
    the bottom row, over 200 ticks. No panic, and they stay in the map.
  - `settled_grains_wake_on_a_nearby_edit`: a settled pile; after 50
    still ticks `loose_moves()` is empty. Carve beside it and grains
    move.
  - `two_worlds_agree`: two worlds stepped alike over 500 ticks of
    collapse have equal `checksum()` every tick.
  - `a_restore_mid_collapse_re_steps_the_same`: `enable_rollback`,
    snapshot at tick 20, step to 60, restore, step to 60 again. The
    checksums are equal.
  - `a_state_transfer_mid_collapse_plays_on_alike`: `WorldState` taken
    at tick 20 and loaded into a fresh world. Both step to 100 with
    equal checksums.
  - `a_level_without_loose_hashes_as_before`: a world without
    `MatchSetup::loose` has today's `checksum()`. Compare with a world
    built before the change, by its known value from an existing
    checksum test, or by `hash_state` with the loose branch absent.
- [ ] **Step 2: Run** `cargo nextest run -p olr-sim loose`. Expected:
  fails to compile.
- [ ] **Step 3: Implement `loose.rs`.**
  - `LooseState`, `Clone`, `PartialEq`:
    - `moving: Vec<Grain>`, sorted by cell, with
      `Grain { cell: u32, vel: u16, frac: u8 }`;
    - `awake` and `next`: `Vec<u64>` bitsets, one bit per chunk;
    - `parts: Vec<Area>`, last tick's live rock part boxes.
  - Scratch outside the state: `moves: Vec<(u32, u32)>`, and a per-cell
    index into `moving` rebuilt each step, not hashed.
  - `World::loose_phase(frozen)`:
    - Drain `terrain.take_touched()`, worm boxes and part footprint
      changes (old ∪ new box, grown by 1) into `next`.
    - Run the spec's step exactly, reading `Solid` for openness. It moves
      grains through `Terrain::move_grain` and records `moves`.
    - Stop moving at `MAX_LOOSE_MOVES` and leave unfinished chunks awake.
    - At setup, every chunk holding a loose pixel is awake.
  - `tan` and `1/tan` thresholds are computed at setup as fixed-point
    (integer and fraction in 1/65536) from `repose`. The per-tick logic
    uses integers only.
  - `hash(x, y, t, salt)`: the spike's mix. `dither(x, y)` is
    `hash(x, y, 0, 7) & 0xffff`.
  - Checksum: `hash_state` hashes `LooseState` (moving, awake, parts)
    only when `self.loose.is_some()`, as `live` is.
  - Snapshot: `LooseState` is part of the cloned world. `WorldState`
    gains `loose: Option<(Vec<u8>, LooseState)>`, the kinds and the
    state, `None` without loose; its load restores both.
- [ ] **Step 4: Run** `cargo nextest run -p olr-sim`. Expected: all
  pass, every existing test unchanged.
- [ ] **Step 5: Run** `cargo xtask check`. Expected: clean, with every
  golden unchanged.
- [ ] **Step 6: Commit** `olr-sim: loose dirt falls, rides and slides at its angle of repose`.

### Task 4: The format (`olr-format`, `olr-pack`)

**Files:**
- Modify:
  - `crates/olr-format/src/levels/files.rs` (`LevelFile::loose`,
    `loose.png` read, write and lenient);
  - `crates/olr-format/src/levels/mod.rs` (`LevelSource::loose`,
    `loose_map`, `LOOSE_FILE`, `check`);
  - `crates/olr-format/src/levels/composition.rs` (`Element::loose`,
    `check_elements`);
  - `crates/olr-format/src/assets.rs` (`AssetFile::loose`);
  - `crates/olr-format/src/levels/placed.rs` (`gather_loose`);
  - `crates/olr-pack/src/level_pack.rs` (`LevelPack::loose`);
  - `docs/formats/*.md` (regenerated).
- Test: `crates/olr-format/tests/loose.rs` (new), `tests/hostile.rs`.

**Interfaces:**
- Produces:
  - `pub struct LooseSource { id: String, name: String, notes: String, repose: u8, gravity: u16, top_speed: u16, slide_speed: u8 }`,
    with serde defaults 45/200/50/25 and the `deny_unknown_fields` docs.
  - `pub const LOOSE_FILE: &str = "loose.png"`.
  - `pub const MAX_LOOSE: usize = 16`.
  - `LevelSource { loose: Vec<LooseSource>, loose_map: Option<Vec<u8>> }`.
  - `compile_loose(src: &LevelSource) -> Vec<olr_data::level::LooseMaterial>`
    (Task 3's type; `olr-format` depends on `olr-data`, never on
    `olr-sim`).
  - `Element::loose: Option<String>` (an id of the asset's or level's
    `loose`).
  - `AssetFile::loose: Vec<LooseSource>`.
  - `gather_loose` mirrors `gather_materials`.

- [ ] **Step 1: Write the failing tests.**
  - `loose_round_trips`: a level with two loose materials and a map; the
    canonical text and the files round-trip.
  - `a_level_without_loose_writes_none`: no `loose` key and no
    `loose.png`.
  - `loose_limits`: 17 materials, `repose` 4 and 86, `gravity` 0 and
    2001, `top_speed` 0 and 501, `slide_speed` 101, a duplicate id, a
    bad id. Each is refused, naming its field.
  - `a_hostile_loose_map_is_refused_and_played_without`: a value past
    the list, a wrong size, a 16-bit grey, and loose on rock. `check`
    refuses each. `read_level_files_lenient` drops the map with a
    warning "…: the level plays without its loose dirt".
  - `element_loose_follows_its_rules`: only on a `Dirt` element or a
    mixed layer, naming a known id. On rock or background it is refused.
  - In `tests/hostile.rs`, add `loose.png` mutations to the hostile
    level proptest.
- [ ] **Step 2: Run** `cargo nextest run -p olr-format loose`. Expected:
  fails to compile.
- [ ] **Step 3: Implement.**
  - Follow `clear`'s path (`CLEAR_FILE`, `read_clear`, the lenient
    reader, `file_paths`) for `loose.png`.
  - Follow `materials`' path for the lists.
  - `LevelPack::loose` and `loose_map` carry them.
  - `olr_pack::validate` knows the file (hashed: it is a play file).
- [ ] **Step 4: Run** `cargo xtask format-docs`, then
  `cargo nextest run -p olr-format -p olr-pack`. Expected: pass, with
  the docs regenerated.
- [ ] **Step 5: Run** `cargo xtask check`. **Commit** `olr-format: loose materials and loose.png`.

### Task 5: Loading and drawing (`olr-content`, `olr-fx`, `olr-present`, `olr-net*`)

**Files:**
- Modify:
  - `crates/olr-content/src/maps.rs` (`LoadedLevel::loose`, `loose_sim()`,
    `terrain()`);
  - `crates/olr-content/src/sandbox.rs`, `replay.rs`, `matches.rs`
    (setup passes `loose` and the kinds);
  - `crates/olr-replay/src/lib.rs` (`Assets::loose`);
  - `crates/olr-fx/src/lib.rs` (`FxTick::loose_moves`, applied after the
    tick's edits);
  - `crates/olr-present/src/lib.rs` and `net.rs` (recorded ticks carry
    the moves).
- Test: `crates/olr-content/tests/loose.rs` (new), and the fixture level
  `fixtures/live/levels/loose-test/` (new: a 200 × 120 rock box with a
  loose heap over a dug gap, theme `olx-theme-dirt`).

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces:
  - `LoadedLevel::terrain(&self) -> Terrain`: `from_grid` plus
    `with_loose`, the one way setups make the terrain.
  - `LoadedLevel::loose_sim() -> Option<Arc<LooseLevel>>`.
  - `FxTick { loose_moves: &'a [(u32, u32)] }`.

- [ ] **Step 1: Write the failing tests.**
  - `a_loose_level_loads_its_kinds_and_materials`.
  - `colour_follows_the_grains`: play `loose-test` with 2 chaos bots for
    600 ticks, applying edits as the game does. Every pixel's colour
    class matches the material (the `tests/color.rs` check: background
    shows back art, dirt never shows back art).
  - `a_level_without_loose_builds_the_same_world`: the checksum of a
    world for `olx-level-dirt-level` equals the checksum before (use
    `fixtures/matches` golden equality via `cargo xtask check`).
- [ ] **Step 2: Run** `cargo nextest run -p olr-content loose`. Expected:
  fails.
- [ ] **Step 3: Implement.**
  - Each `Terrain::from_grid(&level.material)` in setup paths becomes
    `level.terrain()`.
  - `match_setup` and `replay_assets_from` pass `loose_sim()`.
  - `Fx::step_tick` applies `loose_moves` through `ColorMap::move_grain`
    after the events' terrain edits.
  - `NetPresenter`'s recorded tick stores the moves with the events.
- [ ] **Step 4: Add** a live golden for `loose-test`:
  `cargo xtask bless-regression --reason "loose dirt: the loose-test level's golden"`.
  First confirm `git diff --stat fixtures/` shows only the new golden
  and the log line. Any other golden changing is a bug.
- [ ] **Step 5: Run** `cargo xtask check`. **Commit** `olr-content: loose levels load, play and draw`.

### Task 6: Baking and the command line (`olr-edit`, `olr-cli`)

**Files:**
- Modify:
  - `crates/olr-edit/src/compose.rs` (bake `Element::loose` into
    `loose_map`, gather the lists);
  - `crates/olr-edit/src/element.rs`, `level.rs`;
  - `crates/olr-cli/src/level_cmd.rs`, `element_cmd.rs`.
- Test: `crates/olr-edit/tests/loose.rs` (new),
  `crates/olr-cli/tests/level_cmd.rs`.

**Interfaces:**
- Produces:
  - `olr-cli level loose add <pack> --id ID --name NAME [--repose N] [--gravity N] [--top-speed N] [--slide-speed N]`,
    plus `loose set <pack> ID [same flags]` and `loose remove <pack> ID`
    (refused while an element names it).
  - `olr-cli level element set <pack> <element> --loose ID|none`.
  - `olr-cli level loose probe <pack> --box X,Y,W,H --ticks N`, hidden:
    for each 4-connected dirt region in the box, the share of its pixels
    that moved in N ticks with every dirt pixel of the box loose at the
    first material. Output is one line per region,
    `x,y,w,h pixels moved%`, in a fixed order (top-left first).

- [ ] **Step 1: Write the failing tests.**
  - `a_loose_dirt_element_bakes_its_kinds`: a drawn dirt layer with
    `loose: "sand"` bakes `loose.png` = 1 on its dirt pixels, 0
    elsewhere.
  - `a_mixed_layer_bakes_loose_only_on_its_dirt`.
  - `a_higher_rock_element_covers_loose`: covering rock leaves kind 0.
  - `assets_loose_materials_are_gathered` (identical ones shared, else
    `-2`).
  - CLI: `loose_add_set_remove_round_trip`, `remove_refuses_a_named_one`,
    `element_set_loose`, `probe_reports_a_hanging_region_as_moving`.
- [ ] **Step 2: Run** `cargo nextest run -p olr-edit -p olr-cli loose`.
  Expected: fails.
- [ ] **Step 3: Implement** the bake (the clear flags' path in
  `compose.rs`) and the commands. `probe` builds a world with
  `MatchSetup::loose`, no worms, and counts moved cells per region.
- [ ] **Step 4: Run** `cargo xtask check`. **Commit** `olr-edit, olr-cli: author loose dirt`.

### Task 7: The editor (`olr-editor`)

**Files:**
- Modify: `crates/olr-editor/src/look_ops.rs` (or the level settings'
  ops file), `crates/olr-editor/src/state.rs`, the layer panel under
  `crates/olr-editor/src/ui/`.
- Test: the crate's state tests (`crates/olr-editor/tests/`).

**Interfaces:**
- Consumes: Task 6's `olr-edit` operations.
- Produces:
  - A "Loose dirt" section in the level settings: a list with add,
    remove, and name, angle, gravity, top speed and slide speed fields.
  - On a dirt layer, a "Loose" choice: none or one of the materials.
  - Every change goes through the history (undo).

- [ ] **Step 1: Write the failing tests.**
  - `adding_a_loose_material_is_undoable`.
  - `a_dirt_layer_takes_a_loose_material`.
  - `removing_a_used_material_is_refused_with_a_notice`.
- [ ] **Step 2: Run** `cargo nextest run -p olr-editor loose`. Expected:
  fails.
- [ ] **Step 3: Implement** with the existing surface-material
  controls as the model (`surface_ops.rs`).
- [ ] **Step 4: Run** `cargo xtask check`. **Commit** `olr-editor: loose dirt materials and layers`.

### Task 8: Beam shadows redraw only what a change touches (`olr-render`)

**Files:**
- Modify: `crates/olr-render/src/renderer/beam_shadow.rs`, `crates/olr-render/src/renderer/look.rs` (`write`)
- Test: `crates/olr-render/tests/gpu/glow_beams.rs`

**Interfaces:**
- Produces: `BeamShadow::stale_rect(&mut self, world: Rect)`. It marks
  dirty each beam of the last packed frame whose reach box touches
  `world`. The box is the axis-aligned box of its source span and its
  far end, `at ± half` and `at + dir·reach ± (half + reach·tan)`, grown
  by 2 px. `look.rs`'s per-rectangle `write` calls it instead of
  `stale()`. Whole uploads keep `stale()`.

- [ ] **Step 1: Write the failing GPU tests.**
  - `a_far_terrain_change_keeps_a_beams_rows`: two beams; dig a pixel
    far outside both reaches. `beam_shadow_rows_drawn` is 0 for the
    next frame.
  - `a_change_in_reach_redraws_only_that_beam`: dig in beam A's reach.
    Only A's rows are redrawn (`beam_shadow_rows_drawn` equals A's row
    count).
  - The existing `a_terrain_change_makes_it_again` stays green.
- [ ] **Step 2: Run** `OLR_GPU_TESTS=1 cargo nextest run -p olr-render glow_beams`.
  Expected: the two new tests fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo xtask check --render`. Expected: clean, and
  `lit-hashes.ron` unchanged.
- [ ] **Step 5: Commit** `olr-render: a terrain change redraws only the beam shadows it touches`.

### Task 9: Liero Factory Reloaded's spoil (content)

**Files:**
- Modify: `tools/levels/factory_works.py` (or a new
  `tools/levels/factory_loose.py` imported by `factory_assets.py`),
  `tools/levels/CLAUDE.md`, `content/assets/README.md` only if an asset
  needs it.
- Regenerated: `content/reloaded/olr-level-liero-factory-reloaded/`.
- Goldens: Liero Factory Reloaded's render goldens and
  `fixtures/matches-live/classic-factory-live-4dm.ron`.
- Test: `crates/olr-cli/tests/reloaded.rs`,
  `crates/olr-cli/tests/factory_works.rs`.

**Interfaces:**
- Consumes: Task 6's commands.

- [ ] **Step 1: Write the failing tests.**
  - `the_factory_has_spoil`: the level's `loose` is
    `[spoil (Mine spoil), repose 35]` with the other values at their
    defaults. `loose.png` is non-zero inside the press heap's box
    `(820, 320, 90, 50)` and nowhere outside it or the mine's box
    `(590, 370, 380, 110)`.
  - `the_spoil_stays_put_in_an_empty_match`: a world on the level with
    no worms; fewer than 2 % of the loose pixels move in 300 ticks.
  - `tests/reloaded.rs`: the bake tests compare `material.png` and the
    art as before. A new assertion covers the `loose` list.
- [ ] **Step 2: Run** `cargo nextest run -p olr-cli factory`. Expected:
  fails.
- [ ] **Step 3: Implement.** In the factory script, after the ground's
  edits:
  - Add `spoil`.
  - For the press heap, mark the dirt of the box's element loose. This
    is the heap's dirt layer; extract it to its own layer first if it
    shares one.
  - For the mine, run `level loose probe` over `(590, 370, 380, 110)`
    for 300 ticks, and loosen each region under 2 % moved, by mask
    extraction onto a dirt layer with `loose: spoil`.
- [ ] **Step 4: Regenerate** with the factory pipeline (`cargo build -p olr-cli`,
  `python3 -I tools/levels/factory_assets.py target/debug/olr-cli`,
  `python3 -I tools/levels/factory_live.py target/debug/olr-cli`). Run
  it again and confirm nothing changes.
- [ ] **Step 5: Re-bless.**
  - `cargo xtask bless-render --reason "Liero Factory Reloaded: loose mine spoil"`
  - `cargo xtask bless-regression --reason "Liero Factory Reloaded: loose mine spoil"`

  Look at the factory render goldens.
- [ ] **Step 6: Run** `cargo xtask check --render`. **Commit** `olr-level-liero-factory-reloaded: the press heap and the mine's floor seams are loose spoil`.

### Task 10: Docs

**Files:** `docs/porting/STATUS.md` (a "Loose dirt" section, no OLX
counterpart), `CHANGELOG.md`, `docs/deferred-verification.md` (playtest
the feel; tune speeds and angle), and every touched crate's `CLAUDE.md`
if not already updated in its task.

- [ ] **Step 1: Write the docs.**
- [ ] **Step 2: Commit** `docs: loose dirt` (docs only: no check needed).

### Final

- [ ] Measure the step's cost: run `olr-cli match` on Liero Factory
  Reloaded, as shipped, with tick times, before and after. Report the
  numbers; no benchmark run without asking.
- [ ] Ask the maintainer before `cargo xtask full`, which the rules want
  for rendering and performance-sensitive changes.
