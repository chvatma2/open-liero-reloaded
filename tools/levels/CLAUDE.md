# tools/levels

Scripts that edit OLR's own levels (`content/reloaded/`) through
`olr-cli`'s level commands. Each runs with `python3 -I` and only the
standard library (its own PNG reader: 8-bit, non-interlaced), from the
repository's root, the `olr-cli` binary its first argument. The commands
are in `content/reloaded/README.md`; they reproduce the committed packs
byte for byte, and running one again changes nothing.

- `reloaded_materials.py` (issue #46): the surface materials of Liero
  Factory After Dark and Castle Strike Reloaded: each element's default,
  then a rule painting its pixels by collision kind and art colour
  (`factory` and `castle`), handed to `olr-cli level element surface` as
  one exact-colour `--rule` per colour. `--preview DIR` writes each ruled
  element's materials as false-colour PNGs instead. Never changes a
  level's `material.png`, `front.png` or `back.png`.
- `reloaded_skies.py` (issue #63): the two levels' skies onto glowing
  distant layers: the sky region erased from every back element
  (`level element erase --mask`), a level-sized image at (0, 0) showing
  the old sky at its reference placement and the sky's radial gradient
  beyond it (fitted centre, colours by distance), Liero Factory's with
  stars (SplitMix64, seed 63). Writes its images to
  `target/reloaded-skies/`; a level already holding a distant layer is
  left alone. `reloaded_materials.py` leaves distant elements out.
- `factory_assets.py` (issue #96): Liero Factory Reloaded from assets:
  the Crane, the Console and the Gauge made by their blocks in
  `content/assets/README.md` (each on a fresh copy of After Dark), then
  the level: a copy of After Dark (`pack new --keep-origin`), its notes,
  the pieces' rectangles erased from the ground's layers (empty layers
  removed), the sky as its own distant copy and each built-in asset
  placed 1:1 where its pixels were. Its scratch is
  `target/factory-assets/`. The bake stays After Dark's outside the
  live copies (`olr-cli`'s `tests/reloaded.rs`). Issue #97: it also
  paints the ground's `backdrop` behind the crane's claw (`paint_wall`:
  the pixels the ground lost in the crane's box and `WALL_EDGE` 2 around
  them, blended along each row), before any copy is placed (ground
  edits need the ground alone, issue #75).
- `factory_live.py` (issue #97; #98 and #99 extend it): brings Liero
  Factory Reloaded alive after `factory_assets.py`: the Console's state
  pictures into `tools/levels/factory/art/` (from After Dark's
  `front.png`), the Crane and the Console made again by their blocks,
  then `level library update`, the copies switched Live, the level
  script, notes, `level bake`, `level check-scripts` and `pack check`.
  Its inputs are under `tools/levels/factory/`: `crane.rhai` (the
  Crane's default script), `level.rhai` (the level script), `art/` and
  `sounds/` (`tools/sounds/factory_sounds.py`).
