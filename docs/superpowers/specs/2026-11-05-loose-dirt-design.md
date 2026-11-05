# Loose dirt

**Date:** 2026-11-05. **Status:** design, for approval.

## Intent

A level may mark some of its dirt **loose**: that dirt falls and slides
like sand (Noita's falling sand, without its chemistry), while every other
pixel behaves as Liero's always has. The maintainer wants it first for
Liero Factory Reloaded: piles that collapse when dug under, and later
(not in this design) a crane that scoops real dirt from the mine and a
press that consumes the dirt under it.

A throwaway spike (2026-11-05, an `olr-cli spike-loose` command that ran
the step after each tick of a real bot match on Liero Factory Reloaded)
settled the behaviour and the cost:

- Grains start from rest, gain speed at 200 px/s² up to 50 px/s, and
  slide down slopes steeper than their angle of repose at 25 px/s. The
  maintainer judged an earlier fixed 300 px/s "almost instant", and the
  slower values "good, we can tweak parameters later".
- An angle of repose per material reads clearly: 25° leaves wide, shallow
  craters, 65° steep, jagged peaks.
- A grain resting on a falling grain must accelerate with it, or a
  falling body splits into horizontal stripes.
- In the dev build a moved grain costs about 100 ns. The worst tick seen
  was about 1 ms, the opening collapse of all of Liero Factory's dirt made
  loose (about 9,300 grains in one tick). A tick is 10 ms.
- Marking a whole box loose also crumbled dirt objects (the warehouse's
  crates are dirt) into piles of scrambled art, so loose dirt is marked
  pixel by pixel through the editor's layers, never by box.

## Goals

1. A level defines **loose materials**. Each has an id, a name, an
   **angle of repose** (degrees, 5 to 85, default 45), a **gravity**
   (px/s², default 200), a **top speed** (px/s, default 50) and a **slide
   speed** (px/s, default 25), so the parameters can be tuned per material
   without code.
2. Any dirt pixel may be loose, of one of the level's loose materials.
3. **Loose dirt is dirt to everything else.** Worms dig it, weapons carve
   it, ropes hook it, worms and projectiles collide with it, and bots
   treat it as dirt. Only its own movement is new.
4. Grains fall, ride on falling grains and slide, deterministically, in
   the simulation (`olr-sim`, `no_std`, wasm). Rollback, the checksums,
   state transfer, replays and the bots all stay correct.
5. A moving grain carries its colour: what players see moves with it.
6. The cost stays bounded. Settled grains cost nothing, and a tick moves
   at most `MAX_LOOSE_MOVES` grains.
7. The glow beams' shadow map is redrawn only for beams whose reach a
   terrain change touches. This is the one GPU cost moving grains add.
   Today any terrain change redraws every beam.
8. Authors can make loose dirt in the level editor and with `olr-cli`.
9. Liero Factory Reloaded's dirt heap at the press and the mine's seams
   that rest on the mine's floor become loose. Seams that hang from rock
   ceilings stay ordinary dirt, so nothing collapses when a match starts.

## Non-goals

- Chemistry, liquids, gases, fire.
- Dirt placed in play (dirt balls, green dirt, the referee's dirt) is
  ordinary dirt, as in OLX.
- Grains interacting with **moving** live parts. A grain resting on a
  live rock part (the belt) stays on it. A part that moves into grains
  leaves them where they are, under it. Belts that carry grains, a die
  that pushes or consumes them and a crane that scoops them are the
  factory follow-up.
- Grains burying, pushing or hurting worms. A worm holds grains up and
  stands in them as in dirt.
- Settling loose dirt when a level loads.
- Sideways momentum, bouncing, clumping.
- Running the step on more than one thread.

## Anti-goals

- A level without loose dirt changes no byte. That covers:
  - the oracle traces;
  - the match goldens (`fixtures/matches`, `fixtures/matches-live`);
  - the render goldens and `lit-hashes.ron`;
  - the checksums;
  - state transfer;
  - its cost, beyond one `Option` check a tick.
- No new per-player setting: loose dirt is level content.
- No OLX level or OLX import changes.

## Behaviour

Coordinates are world pixels and `y` grows downward. "Open" means the
pixel is background in the one terrain query (`olr-sim`'s `Solid`: the
ground plus the visible live rock parts) and no living worm's collision
box covers it. That box is the cross `moveAndCheckWormCollision` reads:
`|dx| ≤ 3` on the worm's row and `-4 ≤ dy ≤ 5` on its column. For grains
it is taken as the box `|dx| ≤ 3, -4 ≤ dy ≤ 5`.

**State.**
- Each loose pixel has a kind: its loose material's index + 1, with 0
  meaning not loose. A pixel is loose only while it is dirt. Carving a
  pixel or changing it to rock or background clears its kind.
- A **moving** grain also has a fall speed and a sub-pixel remainder.
  Every other grain is resting.
- Speeds use 256 sub-pixel units per pixel, per 100 Hz tick:
  - gravity: `round(g / 10000 × 256)`, at least 1;
  - top speed: `round(v / 100 × 256)`;
  - slide chance per tick: `slide / 100`, clamped to 1, as a share of
    65536.

**The step**, once a tick:
- It runs at the end of `World::frame`, after the script phase.
- It is skipped in a frozen world.
- It runs only where the level has loose dirt.

Rows run from the bottom up. Within a row the direction alternates with
`(tick + y) & 1`, and each grain of an awake chunk is visited once:

1. **Falling or riding.** If the pixel below is open, or holds a moving
   grain:
   - Speed becomes `min(speed + gravity, top)`.
   - The remainder adds the speed, and the whole pixels in it are the
     fall `d`.
   - The grain moves down as many of those pixels as are open.
   - If it stopped short of `d` against anything but a moving grain, it
     rests: speed and remainder become 0. Otherwise it keeps them.
   - Because rows run bottom-up, a grain riding on a falling grain sees
     the pixels its carrier vacated this tick, so a falling body moves as
     one.
2. **Resting.** Otherwise speed and remainder become 0, and the grain
   may slide. With `t = tan(repose)`, the sides are tried in the order
   given by bit 0 of `hash(x, y, tick, 1)`:
   - **`t ≥ 1`:** the grain moves to `(x+s, y+1)` when `(x+s, y)` and
     `(x+s, y+1 … y+k)` are all open. `k` is `floor(t)`, plus 1 when
     `dither(x, y) < fract(t)`.
   - **`t < 1`:** walking `j = 1 … n` along the row, the walk stops at the
     first `(x+s·j, y)` that is not open. At the first `j` where
     `(x+s·j, y+1)` is open, the grain moves to `(x+s, y+1)` if `j = 1`,
     else to `(x+s, y)`. `n` is `floor(1/t)`, plus 1 when
     `dither(x, y) < fract(1/t)`, with `t` at least 0.05.
   - A grain that can slide keeps its chunk awake. It slides only when
     `hash(x, y, tick, 2) & 0xffff < slide chance`.

`hash` and `dither` are fixed integer mixes of position (and tick). The
spike's `hash` was a multiply-xorshift; any fixed integer mix will do.
`dither` uses position only, so a settled pile keeps its shape.

**Moving a grain** sets the destination to dirt of the grain's kind and
the source to background:
- through the terrain's journaled writes;
- carrying speed and remainder with it;
- recording the move for the presentation;
- waking the chunks around both pixels.

**Sleeping.**
- Chunks are 16 × 16 pixels. Only awake chunks are visited.
- A chunk wakes for the next tick when:
  - a grain moves within a pixel of it;
  - a grain in it is moving or could slide;
  - any carve or dirt edit's rectangle touches it, grown by a pixel;
  - a living worm's box, grown by 4 pixels, touches it. This frees grains
    a worm held up;
  - a live rock part's footprint changes (it moves, shows, hides, is
    wrecked, or changes state) and its old or new box, grown by a pixel,
    touches the chunk. This frees grains it held up or propped.
    `LooseState` keeps the last tick's part boxes, so the wake is
    deterministic after a restore or a state load.
    *(My call: the part doesn't carry grains, but grains fall once it
    has gone.)*
- At the start a chunk is awake if it holds a loose pixel.
- When a tick has made `MAX_LOOSE_MOVES` (20,000) moves, it stops moving
  grains. The chunks it did not finish stay awake, and their grains wait
  until the next tick.

## Data and formats

- **Loose materials.**
  - `level.ron` gets `loose: [LooseSource]`, at most 16 entries:
    - `id`: as a surface material's;
    - `name`;
    - `notes`;
    - `repose`: 5 to 85, default 45;
    - `gravity`: 1 to 2000, default 200;
    - `top_speed`: 1 to 500, default 50;
    - `slide_speed`: 0 to 100, default 25.

    Out-of-range values are refused by `pack check`, never a panic.
  - An asset's `asset.ron` gets the same list. Baking gathers the assets'
    lists into the level's, the way surface materials are gathered.
    *(My call: it mirrors surface materials.)*
- **Loose map.**
  - The pixels are stored in an optional `loose.png`: 8-bit grey, the
    level's size, 0 for not loose, `k + 1` for the level's `loose[k]`.
  - It is read only where `material.png` is dirt. A value past the list,
    or a map of another size, fails `pack check` and the level does not
    load.
  - No file means no loose dirt.
  - *(My call: a map of its own, like `look/clear.png`. `material.png`
    and OLX's three collision kinds stay as they are.)*
- **Editor layers.**
  - An element of material `Dirt` gets `loose: Option<String>`, the id of
    one of the asset's loose materials.
  - Its dirt pixels bake as that material. In a mixed layer, that means
    only its dirt pixels.
  - Rock and background elements refuse it.
- **Format docs** (`cargo xtask format-docs`) gain the new fields.

## Where it lives

- **`olr-data`:** the level model holds the loose materials and the
  optional kind grid beside the `MaterialGrid`.
- **`olr-terrain` (`Terrain`):**
  - An optional loose layer: one kind byte per cell, kept in the same
    record and undo as the cells.
  - A cell's hash term includes its kind, so a level without loose dirt
    hashes as before.
  - `carve_hole`, `place_dirt` and `place_green_dirt` clear the kind of
    every cell they change.
  - Each of those also marks its rectangle in a touched list, which the
    world drains each tick to wake chunks. It is not journaled; the step
    recomputes it.
  - `cells()` and `load_cells` carry the kinds in state transfer (in the
    transferred byte, or in a parallel slice; builder's choice), only
    when the level has loose dirt.
- **`olr-sim` (`World`):**
  - A `LooseState`, holding:
    - the materials' rates;
    - the moving grains, as a list sorted by cell index;
    - the awake chunks, as two bitsets.
  - Snapshots clone it (it is small: settled grains are not in it).
  - The checksum and the sync checksum hash it, but only in a world whose
    level has loose dirt, as with the live copies.
  - The step reads `Solid` and the worms' positions.
  - It writes only through `Terrain`'s journaled setter.
  - It records the tick's moves as `(from, to)` cell indices, read by
    `World::loose_moves()`. This is output, not state, like events.
- **`olr-terrain` (`ColorMap`):**
  - `move_grain(from, to)` copies the grain's art block to `to`.
  - The destination's glow becomes none.
  - The source shows the backdrop, as a carve does, with the back glow.
  - It updates its own material copy and marks both pixels dirty.
- **`olr-present`:** after a tick's terrain edits, the presentation
  applies `loose_moves()` in order. For online re-simulation the
  recorded-tick path (`step_fx`) carries the moves too. An undo's
  restored areas are rebuilt by `rebuild_area` as today: a grain there
  shows the front art at its new place, a cosmetic difference accepted
  after a rollback.
- **`olr-render`:** each beam's reach box (its `at`, direction, reach and
  spread) is computed once a frame. A dirty terrain rectangle marks
  dirty only the beams whose box it touches, instead of `stale()`
  marking all 64. The other passes already follow dirty rectangles.
  Moved grains keep the static surface map's material at their new
  place (heat and soak); this is accepted.
- **`olr-bots`:** no change. Their area cache covers rock alone, and they
  read dirt live.
- **`olr-editor` and `olr-cli level`:**
  - A loose-materials list in the level's settings (add, edit, remove;
    name, angle, gravity, top speed, slide speed).
  - A dirt layer's material control offers "Loose: <material>".
  - `olr-cli level loose add|set|remove` and `level element set
    --loose ID|none`.
- **Content:** Liero Factory Reloaded gets one loose material, `spoil`
  ("Mine spoil"), at 35° *(my call)*. The scripts in `tools/levels/`
  regenerate it, as the plant works did.
  - **Which pixels.** Two kinds:
    - the press heap: the dirt in its bin, the box (820, 320, 90, 50);
    - the mine's dirt regions (4-connected) in the box
      (590, 370, 380, 110) that rest on rock.

    A region rests on rock when, with nothing dug and no worms, fewer
    than 2 % of its pixels move in 300 ticks of the step. The script
    measures this with a hidden `olr-cli level loose probe` that runs
    the step. Regions hanging from ceilings fail the test and stay
    ordinary dirt.
  - **Settling at the start.** A few pixels of slump at a heap's top in
    the first second of a match are accepted *(my call)*: no settle at
    load, no settle tool.
  - **Goldens and tests.** Liero Factory Reloaded's render goldens and
    its live golden are re-blessed, with the reason logged.
    `tests/reloaded.rs` is updated for the new `loose.png` and the
    `loose` list. The bake comparisons there still hold for
    `material.png` and the art.

## Testing

- **`olr-terrain`:**
  - The loose layer's journal round trip, with undo restoring the kinds.
  - A carve clears the kinds.
  - The cell hash is unchanged without loose dirt.
  - `ColorMap::move_grain` moves colour and glow.
- **`olr-sim`:**
  - A grain falls from rest and accelerates to its top speed.
  - A column of grains falls as one body (no stripes).
  - A dug hole fills.
  - Piles settle at their angle of repose, with tolerance; checked at 25°,
    45° and 65°.
  - A worm holds grains up.
  - A live rock part holds grains up.
  - A frozen world moves none.
  - The move cap holds.
  - Settled grains wake on a nearby edit.
  - Determinism: two worlds stepped alike have equal checksums.
  - Rollback: snapshot, step, restore and re-step give the same bytes.
  - State transfer carries the kinds and `LooseState`.
- **Goldens:**
  - A new live match golden on a small loose-dirt test level in
    `fixtures/live/levels`.
  - Every existing golden unchanged.
  - Liero Factory Reloaded's live golden is re-blessed, with the reason
    logged.
- **`olr-render` (GPU):** a terrain change far from a beam leaves its
  rows kept, and one in its reach redraws them.
- **Cost:** the step's tick time is measured on Liero Factory Reloaded,
  both as shipped and with all its dirt loose. This uses `olr-cli
  match`'s tick times, not a benchmark run.

## Left to the builder

- Data-structure details: the moving list's layout, the bitsets, how
  kinds travel in state transfer.
- The hash and dither mixes.
- The exact names of the editor's controls.
- How the factory scripts paint `spoil`: by layer extraction or by mask.
