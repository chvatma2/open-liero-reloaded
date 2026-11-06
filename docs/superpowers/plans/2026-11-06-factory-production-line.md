# The Factory's Production Line Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Liero Factory Reloaded's Dirt-ball line really works. The crane carries real loose dirt from the mine into the reservoir. A belt feeds it under the press. The press makes a ball only from dirt it crushed. A moved grain keeps its heat.

**Architecture:** Most of it is general engine physics in `olr-sim`'s live step:
- moving rock parts push, carry and crush loose grains;
- a new conveyor value makes a still rock layer carry what rests on it;
- every grain a part or a belt moves is an ordered edit event the colour map replays.

The script gets a `crushed_dirt(n)` handler. A layer can be clipped to its copy's rectangle. The renderer moves heat and surface material with each grain. Then the factory's crane, line and ground are rebuilt by the level scripts in `tools/levels/`.

**Tech Stack:** Rust (`olr-sim` `no_std`, wgpu renderer), Rhai level scripts, Python level tools (`python3 -I`, stdlib only), RON formats.

**Spec:** `docs/superpowers/specs/2026-11-06-factory-production-line-design.md`

## Global Constraints

- Simulation crates stay `#![no_std]` and build for `wasm32-unknown-unknown`. The live step allocates nothing once warm (`olr-sim`'s `tests/no_alloc.rs`).
- A change to the state transfer bumps `olr_proto::PROTOCOL_VERSION`. Nothing here should need one: the conveyor's phase comes from the tick count, and its value is an ordinary live value.
- Content is untrusted. A conveyor value is bounded, -100 to 100 px/s, on a Rock layer only. Every push chain is bounded (`MAX_PUSH_CHAIN` 64). Neither may panic or run unbounded on a hostile level.
- Plain (not loose) dirt is still crushed by a moving part (`0b1105f6`). Dirt never stops a part.
- The crane's claw keeps the engine's worm rules: it pushes a worm where there is room, and wedges and slowly crushes it (`CRUSH_CHUNK` 5 every `CRUSH_EVERY` 10 ticks) against dirt or rock. The crane script has no `hurt`.
- No script makes, counts out, deletes or moves dirt. Scripts only drive the machines.
- Each crate's `CLAUDE.md` is read before editing it and updated with what changed. The deliberate changes go in `docs/porting/STATUS.md`. No OLX citation applies.
- `tools/levels/*.py` run with `python3 -I` on the stdlib, and reproduce the packs byte for byte. Shipped paths stay within 80 characters.
- `cargo xtask check` passes before every commit (`check --render` for Tasks 4, 5 and 8). Docs-only commits are exempt.
- Goldens that change are re-blessed with reasons: `cargo xtask bless-regression --reason …` and `bless-render --reason …`.
- Commits end with the session's two attribution lines.

## Review Focus

1. **A rollback or state load in the middle of a carry or a belt run.** The colour map must stay the world's; no drift. Tests: Task 1's `a_restore_mid_carry_re_steps_the_same` and Task 8's `the_line_draws_in_step_with_the_world`.
2. **A grain pushed against the level's edge, ground rock or another part.** It must never land in rock or outside the level; it is crushed. Test: Task 1's `a_grain_pushed_against_rock_or_the_edge_is_crushed`.
3. **A hostile conveyor**: an out-of-range speed, a layer that is not Rock, or a level-wide rock layer at full speed. Expect a refusal or a bounded tick, never a panic. Tests: Task 3's `a_conveyor_out_of_range_or_off_rock_is_refused` and `a_level_wide_conveyor_stays_bounded`.
4. **A conveyed worm or grain meeting a wall, a part or the press's housing.** It stops; it is never shoved into rock. Test: Task 3's `a_conveyed_worm_stops_at_a_wall`.
5. **The line Stopped (the shift, a blackout) while dirt is on the belt and grains are in the claw.** The belt carries nothing and the claw's grains stay. Test: Task 8's `a_blackout_holds_the_belt_and_the_claws_grains`.

---

### Task 1: Moving parts push, carry and crush loose grains

**Files:**
- Modify: `crates/olr-sim/src/live_step.rs` (`Mover`, `crush_dirt`)
- Modify: `crates/olr-sim/src/events.rs` (`EventKind::GrainMoved`, `EventKind::DirtCrushed`, `TickEvents::grain_moved`)
- Modify: `crates/olr-sim/src/world.rs` (the loose world handed to the live step)
- Modify: `crates/olr-content/src/color.rs` (`color_edit`)
- Modify: `crates/olr-terrain/src/color.rs` (`ColorEdit::Move`)
- Test: `crates/olr-sim/src/live_step_tests.rs`, `crates/olr-content/tests/color.rs`, `crates/olr-sim/tests/no_alloc.rs`

**Interfaces:**
- Produces:
  - `EventKind::GrainMoved { from: (i32, i32), to: (i32, i32) }`, class `Edit`. It is pushed through `TickEvents::grain_moved(from, to)`. When dropped, both cells join `lost_area`.
  - `EventKind::DirtCrushed { copy: u16, cells: u32 }`, class `Gameplay`. There is one per mover step that crushed at least one cell, plain or loose, of that copy's parts.
  - `pub const MAX_PUSH_CHAIN: usize = 64` in `live_step.rs`.
  - `ColorEdit::Move { from: (u32, u32), to: (u32, u32) }`, replayed by `ColorMap::move_grain`. `color_edit` maps `GrainMoved` to it (out-of-map cells give `None`).
- The rules, applied by `Mover::step` once a step is `Taken`, part by part in `moves` order:
  - **Pushed.** A loose grain in a part's new footprint and not its old one heads one pixel the part's way. The run of loose grains ahead of it (at most `MAX_PUSH_CHAIN`) shifts one pixel when the first cell past the run is free: inside the level, background, and under no shown part. Process farthest first, so it is all `GrainMoved` events. Otherwise the grain in the footprint is crushed.
  - **Carried.** A loose grain whose pixel below lies in the part's old footprint, and not in its new one, moves by the part's delta when its new place is free. The column of loose grains stacked on it moves with it, with the same bound and order rule.
  - **Crushed.** Plain dirt in the new footprint is crushed as today (`crush_dirt`). A loose grain that cannot be pushed is crushed the same way: `Terrain::crush_row` and the `TerrainOp::Crush` event.
  - Every crushed cell counts toward that step's `DirtCrushed`.
  - Moves go through `Terrain::move_grain` (journaled). The moved grain's old `LooseState` slot is simply lost: it rests until the loose step wakes it, and live parts wake their boxes already.

- [ ] **Step 1: Write the failing tests** in `live_step_tests.rs`. Add a helper `loose_world(copies, loose: &[(u32, u32, u32, u32)])`: the 200 × 200 cave with loose kind 1 in each box, and `MatchSetup::loose` holding `LooseLevel::new(vec![LooseMaterial::DEFAULT])`. Add another, `cup(id, at, size)`: a rock part whose mask is the bottom row and the two side columns. `loose_count(&w)` counts the cells with `terrain().loose(x, y) != 0`, and `dirt_crushed(&TickEvents)` sums the tick's `DirtCrushed` cells.

```rust
#[test]
fn a_part_pushes_a_loose_row_as_one() {
    // A 20 × 10 block at (90, 100); loose grains x 110..115 on row 105.
    let mut w = loose_world(vec![block("pusher", (90, 100), (20, 10), 1)], &[(110, 105, 115, 106)]);
    move_to(&mut w, 0, X, 2, 1);
    ticks(&mut w, 4);
    assert_eq!(rect(&w, 0).0, 92);
    for x in 112..117 { assert_eq!(w.terrain().loose(x, 105), 1, "({x}, 105)"); }
    assert_eq!(loose_count(&w), 5, "none crushed");
}

#[test]
fn a_grain_pushed_against_rock_or_the_edge_is_crushed() {
    // Down onto 10 full rows of loose grains lying on the rock at 180.
    let mut w = loose_world(vec![block("press", (90, 160), (20, 10), 1)], &[(90, 170, 110, 180)]);
    move_to(&mut w, 0, Y, 5, 1);
    let mut crushed = 0;
    for _ in 0..8 { crushed += dirt_crushed(&w.tick_new(&[])); }
    assert_eq!(crushed, 100, "20 × 5 crushed, reported");
    assert_eq!(loose_count(&w), 100);
    // At the level's left edge a pushed grain is crushed, never moved out.
    let mut w = loose_world(vec![block("wall", (10, 50), (10, 10), 1)], &[(0, 55, 10, 56)]);
    move_to(&mut w, 0, X, -10, 1);
    ticks(&mut w, 12);
    assert_eq!(loose_count(&w), 0);
}

#[test]
fn a_rising_cup_carries_its_grains() {
    // A 12 × 8 cup at (94, 150), its inside (10 × 7) full of grains.
    let mut w = loose_world(vec![cup("claw", (94, 150), (12, 8))], &[(95, 150, 105, 157)]);
    move_to(&mut w, 0, Y, -30, 1);
    ticks(&mut w, 40);
    assert_eq!(loose_count(&w), 70, "all kept");
    for y in 120..127 { for x in 95..105 { assert_eq!(w.terrain().loose(x, y), 1); } }
    move_to(&mut w, 0, X, 20, 1);
    ticks(&mut w, 30);
    assert_eq!(loose_count(&w), 70, "carried sideways too");
}

#[test]
fn part_edits_replay_in_order() {
    // A copy of the starting terrain, replaying each tick's events in order
    // (Crush: crush_row, GrainMoved: move_grain, carves: carve_hole) and then
    // its loose moves, equals the world's terrain every tick: the cup
    // carrying, a grain pushed, and an explosion carving a grain the same
    // tick it was pushed.
}

#[test]
fn a_restore_mid_carry_re_steps_the_same() {
    // enable_rollback, save at tick 10 of the cup rising; 30 ticks of
    // checksums; restore; the same checksums again.
}
```

Also extend `crates/olr-content/tests/color.rs`'s `color_edit_converts_every_terrain_op`: `GrainMoved { from: (3, 4), to: (3, 5) }` gives `ColorEdit::Move { from: (3, 4), to: (3, 5) }`, and a negative cell gives `None`. Extend `no_alloc.rs`'s `moving_rock_parts_push_and_crush_without_allocating` with a cup of grains.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -q -p olr-sim --lib live_step && cargo test -q -p olr-content --test integration color_edit_converts`
Expected: compile errors (no `GrainMoved`, `DirtCrushed`, `ColorEdit::Move`). Once they compile, the push tests fail: the grains are crushed.

- [ ] **Step 3: Implement the rules above** in `Mover::step` (it gets `loose: bool` from the world, whether the terrain has a loose layer), the two event kinds, `ColorEdit::Move` and `color_edit`. Keep the existing `a_moving_part_crushes_the_dirt_it_moves_into` green: its dirt is plain.

- [ ] **Step 4: Run the tests**

Run: `cargo test -q -p olr-sim && cargo test -q -p olr-content`
Expected: PASS. Keep the existing loose and live tests green.

- [ ] **Step 5: Docs and commit.** Update `olr-sim/CLAUDE.md` (the live step bullet) and `olr-terrain/CLAUDE.md` (`ColorEdit::Move`), and STATUS.md's "Parts crush dirt (2026-11-06)" becomes "Parts push, carry and crush dirt". Run `cargo xtask check`; re-bless the live goldens if they moved.

```bash
git commit -m "olr-sim: moving parts push and carry loose grains, crushing only those with nowhere to go"
```

### Task 2: The script hears the dirt its copy crushed

**Files:**
- Modify: `crates/olr-sim/src/script.rs` (`ScriptEvent::DirtCrushed`, `ScriptEvent::of`)
- Modify: `crates/olr-script/src/runtime.rs` (handler table, dispatch), `crates/olr-script/src/names.rs` if handlers are listed there
- Test: `crates/olr-script/tests/` (the test file holding `crushed`'s dispatch test; find it with `grep -rn '"crushed"' crates/olr-script/tests`)

**Interfaces:**
- Consumes: `EventKind::DirtCrushed { copy, cells }` (Task 1).
- Produces: `ScriptEvent::DirtCrushed { copy: u16, cells: u32 }`, and a copy handler `fn crushed_dirt(n)` (`Handler::CrushedDirt`, name `"crushed_dirt"`, 1 argument), called once per event for the copy's own instance only.

- [ ] **Step 1: Write the failing test** `crushed_dirt_reaches_its_copy_only`. Two copies with a `crushed_dirt(n)` handler that adds `n` to `mem.dirt`. A `DirtCrushed { copy: 0, cells: 7 }` and a `{ copy: 0, cells: 3 }` give copy 0 `mem.dirt == 10`, and copy 1 none.
- [ ] **Step 2: Run it to see it fail.** Run: `cargo test -q -p olr-script crushed_dirt`. Expected: FAIL (an unknown handler, or no event).
- [ ] **Step 3: Implement** the event's mapping and the handler, as `crushed(worm)` is done. Document it where `crushed` is documented (`olr-script/CLAUDE.md`, the API names).
- [ ] **Step 4: Run.** `cargo test -q -p olr-script && cargo test -q -p olr-sim`. Expected: PASS.
- [ ] **Step 5: Commit** after `cargo xtask check`: `olr-script: crushed_dirt(n), the dirt a copy's parts crushed`.

### Task 3: A conveyor value carries what rests on a still rock layer

**Files:**
- Modify: `crates/olr-format/src/assets/states.rs` (`PropertySource::Conveyor(String)`, its checks)
- Modify: `crates/olr-format/src/levels/live.rs` and `crates/olr-content/src/live.rs` (compile to `LiveTarget::Conveyor`)
- Modify: `crates/olr-sim/src/live.rs` (`LiveTarget::Conveyor(u16)`), `crates/olr-sim/src/live_step.rs` (`convey`)
- Modify: `crates/olr-cli/src/asset_cmd.rs` (`asset expose … <layer>.conveyor`), `crates/olr-edit/src/live.rs`, `crates/olr-edit/src/asset/{posed,edit}.rs`, `crates/olr-editor/src/ui/parts.rs` (listed with the other properties, label "Conveyor", unit "px/s")
- Test: `crates/olr-format/src/assets.rs` tests, `crates/olr-sim/src/live_step_tests.rs`
- Generated: `docs/formats/asset.md` and `level.md` (`cargo xtask format-docs`)

**Interfaces:**
- Produces:
  - `PropertySource::Conveyor(layer)`: an `Int` from -100 to 100 px/s, positive rightwards. It names a Rock layer only; Background, mixed and other kinds are refused.
  - `LiveTarget::Conveyor(u16)`. Its value is in whole px/s and is not a position (`is_position` false).
  - `pub(crate) fn conveyed_px(tick: u64, speed: i64) -> i64 = floor((tick + 1) · speed / 100) − floor(tick · speed / 100)`, with the world's `server_frame` as the tick. There is no new state.
- The rule (`World::live_step`, after `push_out`, before `crush`): for each shown rock part whose layer has a nonzero conveyor value with `conveyed_px ≠ 0`, step one pixel at a time (`|px| ≤ 1` at 100 px/s):
  - **Worms** standing on its top (a `check_on_ground` pixel on the part) move one pixel that way where `free`.
  - **Loose grains** whose pixel below is the part's move, leading end first, with the column stacked on each, where free (Task 1's free rule and bound).
  - **Bonuses** resting on its top move where their place is clear, as the bonus fall reads `Solid`.
  - Grain moves are `GrainMoved` events. Nothing is pushed into rock, a part or the edge; a blocked thing stays.

- [ ] **Step 1: Write the failing tests.**

```rust
// olr-format
#[test]
fn a_conveyor_out_of_range_or_off_rock_is_refused() {
    // expose("feed", P::Conveyor("belt".into()), Some(V::Int(101))) is refused,
    // as is Conveyor on a Background layer; Some(V::Int(-100)) on a Rock layer checks.
}
// olr-sim, live_step_tests.rs (a block with a fourth prop Conveyor(0), -100..=100)
#[test]
fn a_running_belt_carries_a_standing_worm_and_grains() {
    // A 100 × 4 belt at (50, 120), conveyor 50 px/s: in 200 ticks a worm
    // standing on it moves 100 px right (± 1), a loose grain resting on it
    // too; the belt never moves; at 0 nothing moves.
}
#[test]
fn a_conveyed_worm_stops_at_a_wall() {
    // A rock block standing on the belt's right end: the worm stops
    // against it, inside no rock, and so does a grain.
}
#[test]
fn a_level_wide_conveyor_stays_bounded() {
    // A 200 × 4 belt across the cave at 100 px/s with 2000 grains on it:
    // 100 ticks run, no panic, every grain kept or stopped (none in rock).
}
```

- [ ] **Step 2: Run them to see them fail.** `cargo test -q -p olr-format && cargo test -q -p olr-sim --lib live_step`. Expected: compile errors, then FAIL.
- [ ] **Step 3: Implement** the property end to end and `convey`. Run `cargo xtask format-docs`.
- [ ] **Step 4: Run.** `cargo test -q -p olr-format -p olr-sim -p olr-content -p olr-edit -p olr-cli`. Expected: PASS.
- [ ] **Step 5: Docs and commit** after `cargo xtask check`. Update the crates' `CLAUDE.md` and STATUS.md ("Conveyors (2026-11-06)").

```bash
git commit -m "olr-sim: a conveyor value carries worms, grains and bonuses on a still rock layer"
```

### Task 4: A layer drawn only inside its copy's rectangle

**Files:**
- Modify: the asset element format (`crates/olr-format/src/assets.rs`: an element field `clip: bool`, serde default false, skipped when false, on Rock or Background drawn and image layers)
- Modify: `crates/olr-sim/src/live.rs` (`LivePart::clip: bool`), `crates/olr-content/src/live.rs` (`live_frame` sets it)
- Modify: `crates/olr-terrain/src/live.rs` (`LivePlaced::clip: Option<[i32; 4]>`, the copy's world rectangle `x, y, w, h`) and the colour map's live drawing (`ColorMap::set_live`: a clipped part's pixels outside the rectangle are not drawn and not marked)
- Modify: `crates/olr-cli/src/asset_cmd.rs` (`asset layer set … --clip true|false`; find the command that sets a layer's flags), the editor's asset preview where it draws parts (`crates/olr-edit/src/asset/posed.rs`)
- Test: `crates/olr-terrain/src/live.rs` tests (or its tests file), `crates/olr-content/tests/live.rs`

**Interfaces:**
- Produces: `clip: true` on an element means it is drawn only inside its copy's box (the copy's `rect()`, which follows `X` and `Y`). Solidity is unchanged: a clipped rock part is solid wherever it lies, so only Background layers are meant to use it. The format refuses `clip` on a Rock layer *(the plan's call: a solid you cannot see would be a trap)*.

- [ ] **Step 1: Write the failing tests.** In olr-terrain: `a_clipped_part_draws_only_inside_its_rectangle`, a 4 × 20 background picture placed half outside its rectangle shows only the inside half, and only the inside rows are dirty. In olr-content: `live_frame_clips_a_clip_layer_to_its_copy`. In olr-format: `clip_on_a_rock_layer_is_refused`.
- [ ] **Step 2: Run them to see them fail.** `cargo test -q -p olr-terrain -p olr-content -p olr-format`. Expected: compile errors, then FAIL.
- [ ] **Step 3: Implement** the flag end to end; run `cargo xtask format-docs`.
- [ ] **Step 4: Run.** The same, plus `cargo xtask check --render`. Expected: PASS, with no golden changed (no shipped asset sets `clip` yet).
- [ ] **Step 5: Docs and commit:** `olr-format: a layer clipped to its copy's rectangle`.

### Task 5: A moved grain keeps its heat and its surface material

**Files:**
- Modify: `crates/olr-render/src/renderer/heat.rs` (`HeatOp::Move`, a CPU copy of the surface texels kept and re-uploaded by dirty rectangle), `crates/olr-render/src/look/heat*.wgsl` (the move pass), `crates/olr-render/src/renderer.rs` (`Renderer::move_grains`)
- Modify: `crates/olr-present/src/lib.rs` (`Presentation` collects the presented ticks' grain moves in order: each `GrainMoved` event as it is replayed, then the tick's `loose_moves`; hands them over in `upload` before `update_terrain`), `crates/olr-present/src/net.rs` (the same per presented tick)
- Test: `crates/olr-render/tests/` (the GPU tier file with `a_dug_pixel_loses_its_heat`), `crates/olr-present/tests/present.rs`

**Interfaces:**
- Consumes: `EventKind::GrainMoved` (Task 1) and `World::loose_moves()`.
- Produces:
  - `pub const MAX_GRAIN_MOVES: usize = 16384` (per frame; past it a moved grain starts cold, as today).
  - `Renderer::move_grains(&mut self, moves: &[(u32, u32)])`: world cell indices `(from, to)` (`y × width + x`; the presentation converts each `GrainMoved`'s cells), in order. The surface texels' front byte at `to` takes `from`'s (the back byte stays), uploaded with the next `update_terrain`. While heat is active a `HeatOp::Move` queues before the terrain update's `HeatOp::Layer`: the temperature at `to` takes `from`'s, `from` goes cold, and `seen` takes both pixels' new classes, so the layer reset leaves them.
- A rollback that the colour map rebuilds leaves a moved front byte where it went. That is a cosmetic, presentation-only cost; record it in `olr-render/CLAUDE.md`.

- [ ] **Step 1: Write the failing tests.** In the GPU tier: `a_moved_grain_keeps_its_heat_and_material`. A dirt pixel of a heating material at `(10, 10)`, heated by a deposit; the colour map moves it to `(10, 14)`, which was open air whose front byte names no material; `move_grains(&[(from, to)])`, then `update_terrain`. After the frame the temperature at `(10, 14)` is within one heat step's cooling of `(10, 10)`'s before, and `(10, 10)` reads 0. The pixel's surface front byte reads the moved material. In olr-present: `grain_moves_reach_the_renderer_in_order`, which uses a recording hook (`Presentation::take_grain_moves_for_test` or an equivalent that the test can see): an event move and a loose move of one tick arrive in event order then loose order.
- [ ] **Step 2: Run them to see them fail.** `cargo xtask check --render` (the GPU tier), or the file's own `cargo test` with the device env the tier uses. Expected: FAIL (the grain lands cold).
- [ ] **Step 3: Implement** `move_grains`, `HeatOp::Move` and its pass (any form that works on every backend the renderer supports), and the presentation's feed.
- [ ] **Step 4: Run.** `cargo xtask check --render`. Expected: PASS, with no golden changed.
- [ ] **Step 5: Docs and commit:** `olr-render: a moved grain keeps its heat and its surface material`.

### Task 6: The crane grabs, carries and drops

**Files:**
- Modify: `content/assets/README.md` (the Crane's block: new layers `trolley` from the claw's top, `cable` Background with `clip`, `hub` Rock, `jaw-l` and `jaw-r` Rock, cut from the claw's picture by masks that `factory_works.make_art` writes; the box grows to 114 × 200 so its rectangle reaches the claw's lowest place; values `claw_x` (OffsetX of the trolley, cable, hub, jaws and lenses), `claw_y` (OffsetY of the cable, hub and jaws, 0 to `DROP`), `jaw_l_x` and `jaw_r_x` (each jaw's own OffsetX, `OPEN` outward to 0 closed))
- Modify: `tools/levels/factory_works.py` (`make_art`: the jaws' masks and pictures, closed tips meeting in a bowl; the cable picture, a 2 px rope about 190 px long)
- Modify: `tools/levels/factory/crane.rhai` (the round)
- Regenerated: `content/assets/crane/`, `content/reloaded/olr-level-liero-factory-reloaded/` (run `factory_assets.py`, then `factory_live.py`)
- Test: `crates/olr-cli/tests/factory_works.rs`

**Interfaces:**
- Consumes: Task 1's push and carry, Task 4's `clip`.
- Produces, in `crane.rhai`:
  - Constants: `HOLE_X()` (the `claw_x` over the shaft's middle, x ≈ 795), `DUMP_X()` (over the reservoir's middle, x ≈ 865), `DROP()` (the `claw_y` just above the tunnel's rock under the shaft, about 140 px), `OPEN()`, `SPEED()` 25, `LOWER()` 30 px/s, `JAW()` 10 px/s, `DUMP()` 100 ticks.
  - The round: to `HOLE_X()`; jaws open; down to `DROP()`; jaws closed; up to 0; to `DUMP_X()`; jaws open; `DUMP()` ticks; jaws closed; again.
  - `blocked` on `claw_y` going down counts as down, and on a jaw counts as closed or open. A wedged worm only holds it (the engine).
  - `Off` stops every value where it is; `Idle` resumes the round where it stopped. The beacon turns while it moves, as now.

- [ ] **Step 1: Write the failing tests** in `factory_works.rs`:
  - `the_crane_carries_the_mines_dirt_to_the_reservoir`: an empty match of two rounds' ticks. Track each grain's origin through `loose_moves` and `GrainMoved` events, as `the_spoil_stays_put_in_an_empty_match` does. At least 100 grains that started in the mine pile's box lie in the reservoir's box at the end.
  - `the_claw_crushes_a_worm_slowly_against_the_dirt`: a worm standing on the pile under the shaft, with the claw coming down on it. Its first two `Crushed` chunks leave it alive (health 100 − 2 × 5), and the crane deals no `hurt`.
- [ ] **Step 2: Run them to see them fail.** `cargo test -q -p olr-cli --test integration factory_works`. Expected: FAIL (the claw only slides).
- [ ] **Step 3: Implement** the art, the block, the script; regenerate. Check the packs reproduce with a second run (`git status` clean).
- [ ] **Step 4: Run.** The same tests and `cargo xtask check --render`. Expected: PASS. Then re-bless the factory goldens with the reason "the crane's round: it grabs the mine's dirt and drops it into the reservoir".
- [ ] **Step 5: Commit:** `olr-level-liero-factory-reloaded: the crane grabs the mine's dirt and drops it into the reservoir`.

### Task 7: The belt feeds the press; a ball only from crushed dirt

**Files:**
- Modify: `content/assets/README.md` (the Dirt-ball line's block: two Rock layers on the belt's top row (y 351), `feed` from x 909 to 962 and `run` from 963 to 989, extracted from the ground as the crate was; values `feed_speed` and `run_speed`, `Conveyor`, -100 to 100, default 0)
- Modify: `tools/levels/factory_works.py` (`ground`: the belt's top row onto the line's layers)
- Modify: `tools/levels/factory/line.rhai` (the rhythm and the count)
- Regenerated: `content/assets/ball-line/`, the factory pack
- Test: `crates/olr-cli/tests/factory_works.rs`

**Interfaces:**
- Consumes: Task 2's `crushed_dirt(n)`, Task 3's conveyor.
- Produces, in `line.rhai`:
  - At each countdown's start (`Lit 1`) both belts run at `BELT()` (12 px/s). When the seventh LED lights they go to 0, then the drop.
  - `crushed_dirt(n)` adds to `mem.dirt`, which is zeroed at each drop. When the die arrives down, `press()` (a ball) runs only if `mem.dirt > 0`.
  - `halt` sets both belts to 0.

- [ ] **Step 1: Write the failing tests:**
  - `the_belt_runs_only_between_strokes`: while the line runs, `feed_speed` is 12 from each `Lit 1` until `Lit 7`, and 0 from then until the die is up again.
  - `a_worm_rides_the_feed_belt`: a worm standing on the feed belt at x 915 is at least 5 px further right after one countdown.
  - `no_dirt_no_ball`: the pack copied to a tempdir, with the reservoir's and the belt's dirt carved out (`olr-cli level carve`). In an idle match over `RUN` ticks no ball is ever shown.
  - Keep `the_line_runs_its_shift` green: the prefilled reservoir makes balls.
- [ ] **Step 2: Run them to see them fail.** `cargo test -q -p olr-cli --test integration factory_works`. Expected: FAIL.
- [ ] **Step 3: Implement** the layers, the values, the script; regenerate.
- [ ] **Step 4: Run.** The same tests and `cargo xtask check --render`, then re-bless the factory goldens: "the belt feeds the press, a ball only from crushed dirt".
- [ ] **Step 5: Commit:** `olr-level-liero-factory-reloaded: the belt feeds the press; a ball only from dirt it crushed`.

### Task 8: The mine pile, the reservoir and a clean belt

**Files:**
- Modify: `tools/levels/factory_works.py`:
  - `SPOIL` and its boxes: a loose `spoil` pile on the tunnel floor under the shaft (x about 776-818, up from about row 448). It is sized for 5 to 8 full bites, and `resting` must keep it put in an empty match until bitten.
  - The reservoir: the press heap's box, with rock walls where it needs them and its foot open onto the feed belt's left end. It is filled loose for 3 to 5 press runs alone.
  - The plain dirt on the belt (rows 346-350, x 909-989) is removed, or under the die and left of it made the reservoir's loose dirt.
- Regenerated: the factory pack, `fixtures/render/factory-*.png`, `fixtures/matches-live/classic-factory-live-4dm.ron`
- Modify: `crates/olr-cli/tests/factory_works.rs` (`the_factory_has_spoil`'s boxes and counts; `the_spoil_stays_put_in_an_empty_match` counts only the grains away from the machines, the seams outside the pile's, the reservoir's and the belt's boxes), `crates/olr-cli/tests/reloaded.rs` if its `loose list` line changes
- Docs: `docs/porting/STATUS.md`, `CHANGELOG.md` (Unreleased), `docs/deferred-verification.md` (a "Production line" playtest row: the crane's bite, the belt's pace, the balls' rhythm, heat on falling dirt), `tools/levels/CLAUDE.md`, `content/assets/README.md`'s prose

**Interfaces:**
- Consumes: Tasks 1-7.
- Produces: the shipped level.

- [ ] **Step 1: Write the failing tests:**
  - `the_line_makes_balls_from_the_mines_dirt`: an idle match of `QUIET + HORN + RUN` ticks. At least two balls are shown, each launched in a stroke with a `DirtCrushed` for the line's copy; the crane delivered grains into the reservoir; no dirt lies on the belt right of x 966 at the start.
  - `every_loose_grain_is_kept_or_crushed`: over the same match, loose grains at the end plus the `DirtCrushed` cells equal the loose grains at the start. No plain dirt lies in any moving part's path on the level; assert that too by counting `TerrainOp::Crush` cells of plain dirt, which must be 0.
  - `the_line_draws_in_step_with_the_world`: the same match with a colour map fed by `olr_content::apply_edits`. Its material equals the world's terrain every 100 ticks.
  - `a_blackout_holds_the_belt_and_the_claws_grains`: force the blackout while the claw carries grains. The belt speeds are 0, the claw's grains stay in it for the blackout's length, and none are lost.
- [ ] **Step 2: Run them to see them fail.** `cargo test -q -p olr-cli --test integration factory_works`. Expected: FAIL.
- [ ] **Step 3: Implement** the ground edits; regenerate (a second run changes nothing).
- [ ] **Step 4: Run.** `cargo xtask check --render`; re-bless the factory goldens ("the mine pile, the reservoir and a clean belt"). Then, with the maintainer's consent, `cargo xtask full`.
- [ ] **Step 5: Commit:** `olr-level-liero-factory-reloaded: the mine pile, the reservoir and a clean belt`. Commit the docs.
