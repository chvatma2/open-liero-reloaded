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
  edits need the ground alone, issue #75). The night shift (2026-11-03):
  `night_shift` adds the groups `GROUPS` and, on new ground layers
  (`added` reads each new id), the `MINE_LAMPS` (plates and bulbs drawn,
  the bulbs' glow by `level element glow --mask`), the `LAB_STRIPS`
  extracted from `lamps` into `cool`, the `HIGH_BAYS` (a Rock layer on
  the deck's rows 267-268), the flue (the rock's saturated reds in
  `FLUE_BOX`, extracted, glow `FLUE_GLOW`) and the `AVIATION` light; and
  `make_skyline` makes `SKYLINE`'s two layers the level's own assets (an
  `add-image` on a fresh After Dark with its group and mask, `asset
  save`) placed after the sky, distant 75 and 45.
- `factory_skyline.py` (the night shift): the skyline's pictures and
  glow masks (`write(out)`: `city.png`, `city-glow.png`, `industry.png`,
  `industry-glow.png`, level-sized, alpha 0 the sky), drawn from fixed
  seeds (23 and 9); `factory_assets.py` imports it (into
  `target/factory-assets/skyline/`).
- `factory_live.py` (issue #97; #98 and #99 extend it): brings Liero
  Factory Reloaded alive after `factory_assets.py`: the Console's state
  pictures into `tools/levels/factory/art/` (from After Dark's
  `front.png`), the Crane and the Console made again by their blocks,
  then `level library update`, the copies switched Live, the level
  script, notes, `level bake`, `level check-scripts` and `pack check`.
  Its inputs are under `tools/levels/factory/`: `crane.rhai` (the
  Crane's default script), `level.rhai` (the level script), `art/` and
  `sounds/` (`tools/sounds/factory_sounds.py`). Issue #98: it also
  paints the Strip light's `strip-dim.png` (60 %) and `strip-off.png`
  (the unlit grey `OFF`) from After Dark's first strip, makes the Strip
  light again by its block (states `On`, `Dim`, `Off`, `buzz.wav` and
  `tick.wav`, the default script `tube.rhai`), updates `s` and switches
  `LIVE_STRIPS` (`strip-2`, `strip-4`, `strip-7`) live. Issue #99: it
  draws the Computer screen's camera pictures (`make_screen_art`: each
  `SCREEN_PARTS` part of After Dark by `olr-cli look-preview --view
  full`, box-downsampled to 12 x 11, a box's most saturated colour
  winning where it covers a quarter of it, saturation x 1.6, mapped to
  the truck picture's colours; the garden touched up in code,
  `GARDEN_*`), the Lit sign's, Control desk's and Gauge's state pictures
  and glow masks (`make_display_art`, from the assets' own first
  picture), makes the Crane (its warning beacon), the Computer screen,
  the Lit sign, the Control desk and the Gauge again by their blocks
  (scripts `screen.rhai`, `sign.rhai`, `desk.rhai`, `gauge.rhai`;
  `crane.rhai` turns the beacon), updates `c t d g` and switches
  `LIVE_DISPLAYS` live. The night shift: the Strip light's block exposes
  `colour` (`lamps.glow-colour`), and `STRIP_COLOURS` sets it per copy.
- `factory_works.py` (the plant works, 2026-11-04; imported by
  `factory_assets.py`): `make_art()` writes the pass's new assets' inputs
  into `tools/levels/factory/art/` from After Dark's layers (level-sized
  `<name>-mask.png`/`<name>-tile.png` pairs for `level element draw
  --mask … --tile …`, level-sized grey `<name>-glow.png` masks, and state
  pictures the size of their layer's box); `ground(run, out)` edits the
  ground while it is alone (the belt's stripe, the drive's casing,
  `DRIVE`, and the incline's painted dirt balls, which the Dirt-ball line
  takes; the line's end crate, `CRATE`, its dirt moved onto a Rock layer
  of wood (2026-11-05); the lab flask's liquid; the
  lift's shaft right of the ramp's mouth, x 1186-1197, from the hall
  floor through the mine's big chamber's wall to a landing; the mine
  cart's rails on the left tunnel's floor, row 473; the doorway's
  threshold plate behind the forklift, `THRESHOLD`, its art moved onto
  the backdrop so the wheels roll over it; the truck's hazard lights, a Rock layer on
  the truck's own rock, and lit cab; the loose dirt design: the loose
  material `spoil` ("Mine spoil", repose 35) and, by `resting`, the dirt
  regions of `SPOIL`'s boxes, the press heap's and the mine's, that
  `level loose probe` moves under 2 % in 300 ticks (the heap and the
  mine's floor seam; the seams over the tunnel would fall), each box's
  extracted from `dirt` onto a layer of its own set `--loose spoil`);
  `place(run, out, assets)` places the Lift, the Mine cart and the
  Forklift. `factory_assets.py` makes the Dirt-ball line, Lift, Mine cart,
  Forklift, Generator, Siren, Lab flask and Window pane by their blocks
  (`NEW_ASSETS`); its `PIECES` give the line the Console's place (placed
  at its own top-left, the third item of a copy), the Siren the Warning
  beacon's, and add the Generator, the Lab flask and three window panes.
  `factory_live.py` switches every strip live (`STRIPS`: the faulty
  `LIVE_STRIPS` run `tube.rhai`, the others start `Steady` with
  `steady.rhai` as their own script) and the pass's copies
  (`LIVE_WORKS`); `level.rhai` runs the shift, the alarm, the blackout,
  the maintenance and the strips' flicker on blasts (one `exploded`
  handler for all, the scripts' tick budget). A copy script that takes
  orders from it (`Off`, `Dark`, `Stopped`) sets the state again when a
  step of its own overwrote it in the same tick, and ignores a resume in
  that tick (`mem.cut`).
