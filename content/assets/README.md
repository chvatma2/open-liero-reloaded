# The level editor's built-in assets

Ready-made pieces of a level (issue #9; the format is
`docs/formats/asset.md`) that the level editor's Assets tab offers under
**Built-in**, read-only, beside the user's own (`<data>/assets`). The
programs find this directory beside the content directory
(`olr_content::assets_dir`: `content/assets` beside `content/packs`);
`cargo xtask dist` copies it into the Windows zip and the Flatpak (this
README stays behind). `cargo xtask check` reads every one whole
(`olr-cli asset check`, in `olr-cli`'s `tests/asset.rs`).
They are in the 0.3 asset format (issue #71) and stateless (one state,
nothing exposed, no sounds, health or script) but for the Crane and the
Console (issue #97), the Strip light (issue #98), the Computer
screen, the Control desk, the Lit sign and the Gauge (issue #99) and
the Terminal (issue #100), which Liero Factory Reloaded brings alive
(all but the Console, its copy static since issue #100). All glow; the Crane since issue
#99 (its warning beacon).

Each was cut from OLR's own Reloaded levels (`content/reloaded/`) with
`olr-cli asset save`. Its entry below holds one block of `olr-cli` lines
(issue #96) that makes it: its `asset save … --out
content/assets/<folder>`; lines before it may prepare the source level
(`level element …` on After Dark's path), lines after it may edit the
folder (`asset state`, `asset expose`, `asset sound`, `asset script`).
Blocks read only committed inputs: the Reloaded packs and files under
`tools/levels/factory/`. `olr-cli`'s `tests/asset.rs`
(`built_in_assets_are_what_their_commands_make`) runs each block on fresh
copies of the packs it names and compares the result with the folder
(`asset.ron`, the element files, sounds and script; never `thumb.png`),
one block per asset. To run one by hand, from the repository's root, run
its preparing lines on a copy of the level, never on the committed pack
(or restore it with `git checkout` after). They carry the surface
materials their pixels have there (issue #46), so placed in a level they
heat as they did in it. Liero Factory Reloaded (`content/reloaded/`) is
built from them.

A level places one as a copy (issue #75): `olr-cli level copy add <pack>
content/assets/<id> --at X,Y` (the level holds its own copy of the
folder, `editor/<key>/`, and records it as built-in); `olr-cli level
copy list|move|resize|set|remove` edit the copies. `olr-cli asset place`
is gone.

- `torch/`: **Torch**, a cavern torch of Castle Strike Reloaded, its
  flame glowing in group `flames` (`Flicker`). Cut from the layers under
  the torch only: `background-2` holds no pixels there (the torch shows
  through its hole) and would carry the `sky` group.

      olr-cli asset save content/reloaded/olr-level-castlestrike-reloaded --rect 203,423,14,16 --layers background,dirt,rock --name Torch --out content/assets/torch

- `ceiling-lamp/`: **Ceiling lamp**, a round office lamp of Liero Factory
  After Dark under three rows of its ceiling (rock), group `lamps`.

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 319,137,13,6 --layers rock,lamps --name "Ceiling lamp" --out content/assets/ceiling-lamp

- `strip-light/`: **Strip light**, one of the factory's white ceiling
  strips, group `lamps`. Issue #98: a faulty fluorescent tube, states
  `On` (start: the picture and glow as cut), `Dim` (the tube at 60 %
  brightness, `lamps.glow` 35) and `Off` (the tube unlit, pale grey
  `#96969c`, `lamps.glow` 0), their pictures of `lamps` from
  `tools/levels/factory/art/` (`tools/levels/factory_live.py`); `On` sets
  nothing, so entering it restores the glow (100) and the picture; no
  state has a solid pixel (the layer is Background). `On` and `Dim` loop
  `buzz.wav` and entering `On` plays `tick.wav` (the starter), both
  synthesised for OLR by `tools/sounds/factory_sounds.py`, CC0 (the
  maintainer may replace them); its default script
  `tools/levels/factory/tube.rhai` stays On 3-12 s, then stutters in a
  burst of 2-6 flickers, now and then staying dark or Dim a while after
  (`content/reloaded/README.md`). A static copy bakes steady On.

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 778,80,29,2 --layers lamps --name "Strip light" --out content/assets/strip-light
      olr-cli asset state set content/assets/strip-light Normal --name On
      olr-cli asset state add content/assets/strip-light Dim
      olr-cli asset state picture content/assets/strip-light Dim lamps --art tools/levels/factory/art/strip-dim.png
      olr-cli asset state value content/assets/strip-light Dim lamps.glow 35
      olr-cli asset state add content/assets/strip-light Off
      olr-cli asset state picture content/assets/strip-light Off lamps --art tools/levels/factory/art/strip-off.png
      olr-cli asset state value content/assets/strip-light Off lamps.glow 0
      olr-cli asset sound add content/assets/strip-light tools/levels/factory/sounds/buzz.wav
      olr-cli asset sound add content/assets/strip-light tools/levels/factory/sounds/tick.wav
      olr-cli asset state set content/assets/strip-light On --loop-sound buzz.wav --enter-sound tick.wav
      olr-cli asset state set content/assets/strip-light Dim --loop-sound buzz.wav
      olr-cli asset script content/assets/strip-light --set tools/levels/factory/tube.rhai

- `computer-screen/`: **Computer screen**, the control room's upper
  screen on its wall, group `control` (blue-white, `Tint("#d0e0ff")`;
  its own layer `ctrl-scr` since 2026-10-25). Issue #99: a security
  camera, states `Truck` (start: the truck's miniature as cut),
  `Crane`, `Offices` and `Garden`, each a painted miniature of
  `ctrl-scr` from `tools/levels/factory/art/`
  (`tools/levels/factory_live.py`); its default script
  `tools/levels/factory/screen.rhai` shows each 250 ticks in turn.

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 906,83,14,14 --layers backdrop,ctrl-scr --name "Computer screen" --out content/assets/computer-screen
      olr-cli asset state set content/assets/computer-screen Normal --name Truck
      olr-cli asset state add content/assets/computer-screen Crane
      olr-cli asset state picture content/assets/computer-screen Crane ctrl-scr --art tools/levels/factory/art/screen-crane.png
      olr-cli asset state add content/assets/computer-screen Offices
      olr-cli asset state picture content/assets/computer-screen Offices ctrl-scr --art tools/levels/factory/art/screen-offices.png
      olr-cli asset state add content/assets/computer-screen Garden
      olr-cli asset state picture content/assets/computer-screen Garden ctrl-scr --art tools/levels/factory/art/screen-garden.png
      olr-cli asset script content/assets/computer-screen --set tools/levels/factory/screen.rhai

- `control-desk/`: **Control desk**, the control room's console: its
  three screens (group `screens`) and lamps (group `displays`). Issue
  #99: states `A` (start: the four lamp dots as cut), `B` (the first
  and third lit) and `C` (the second, third and fourth lit, the third
  red), pictures of `displays` with their glow masks (a dark dot 0) from
  `tools/levels/factory/art/` (the layer given a whole mask before the
  save, so its states can have masks of their own); its default script
  `tools/levels/factory/desk.rhai` changes to another state every
  100-400 ticks.

      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark displays --locked false
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark displays --rect 903,119,30,11 --strength 100
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 903,119,30,11 --layers backdrop,screens,displays --name "Control desk" --out content/assets/control-desk
      olr-cli asset state set content/assets/control-desk Normal --name A
      olr-cli asset state add content/assets/control-desk B
      olr-cli asset state picture content/assets/control-desk B displays --art tools/levels/factory/art/desk-b.png --glow tools/levels/factory/art/desk-b-glow.png
      olr-cli asset state add content/assets/control-desk C
      olr-cli asset state picture content/assets/control-desk C displays --art tools/levels/factory/art/desk-c.png --glow tools/levels/factory/art/desk-c-glow.png
      olr-cli asset script content/assets/control-desk --set tools/levels/factory/desk.rhai

- `lit-sign/`: **Lit sign**, the "13-37 -LD-" board set in rock, group
  `displays` (`Flicker`). Issue #99: states `Normal` (start), `No dash`
  (the dash between the digits dark), `No LD` (the "-LD-" line dark) and
  `Dying 7` (every other pixel of the 7 dark, a failing LED); a dark
  pixel stays in `rock-disp`, solid, painted the board's unlit face
  (`#727272`) with its glow mask 0 (pictures and masks from
  `tools/levels/factory/art/`; `rock-disp` given a whole mask before the
  save); its default script
  `tools/levels/factory/sign.rhai` stays Normal 300-900 ticks, then
  shows one fault 20-80 ticks.

      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark rock-disp --locked false
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark rock-disp --rect 934,299,31,21 --strength 100
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 934,299,31,21 --layers rock,rock-disp --name "Lit sign" --out content/assets/lit-sign
      olr-cli asset state add content/assets/lit-sign "No dash"
      olr-cli asset state picture content/assets/lit-sign "No dash" rock-disp --art tools/levels/factory/art/sign-no-dash.png --glow tools/levels/factory/art/sign-no-dash-glow.png
      olr-cli asset state add content/assets/lit-sign "No LD"
      olr-cli asset state picture content/assets/lit-sign "No LD" rock-disp --art tools/levels/factory/art/sign-no-ld.png --glow tools/levels/factory/art/sign-no-ld-glow.png
      olr-cli asset state add content/assets/lit-sign "Dying 7"
      olr-cli asset state picture content/assets/lit-sign "Dying 7" rock-disp --art tools/levels/factory/art/sign-dying-7.png --glow tools/levels/factory/art/sign-dying-7-glow.png
      olr-cli asset script content/assets/lit-sign --set tools/levels/factory/sign.rhai

- `warning-beacon/`: **Warning beacon**, a roof speaker's red ring on its
  box, group `beacons` (`Pulse`).

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 951,45,25,28 --layers backdrop,beacons --name "Warning beacon" --out content/assets/warning-beacon

- `crane/`: **Crane** (issue #96), the conveyor deck's crane: its rail
  (`rail`, rows 269-271 of After Dark, x 765-872, with the trolley's
  pixels in those rows) and its trolley, cable and claw (`claw`, rows
  272-318, x 772-800), two rock layers that never overlap, cut from
  After Dark's `rock` with `level element extract`. Its box covers the
  claw's whole travel along the rail. It does not glow. Issue #97: the
  rail's middle row is hollow over the trolley (row 270, x 775-797; rows
  269 and 271 there `#737373`) and the trolley's top (row 272) is drawn
  on `claw`, so the trolley rides in the rail; states `Idle` (start) and
  `Moving`, one shared picture; `claw_x` exposed as `claw.x` (−10..75,
  default 0); `Moving` loops `hum.wav`, synthesised for OLR by
  `tools/sounds/factory_sounds.py`, CC0 (the maintainer may replace
  it); its default script `tools/levels/factory/crane.rhai` crosses the
  rail every couple of seconds (`content/reloaded/README.md`). Issue
  #99, its **warning beacon** (issue #95's pick, the design's "The
  spotlight's look"): under the trolley, astride the cable, a housing
  (rows 282-283, x 783-789, on `claw`) and the dome's middle (row 284,
  x 785-787, on `claw`, the cable's two pixels there given to it,
  glowing in group `halo`, `#ff6a10`, reach 60, through `claw`'s glow
  mask, the dome only), and two lenses, Background layers of their own
  (`lens` at x 788-789, beam 0°; `lens-b` at x 783-784, beam 180°;
  spread 10, group `beacon`, `#ff6a10`, 400, reach 360); exposed
  `lens_x`, `lens_b_x` (the lenses' `x`, the claw's limits),
  `beam_spread`, `beam_spread_b`, `reach`, `light`, `halo_glow` (`claw`'s
  glow) and `halo_light` (`halo`'s intensity); `Idle` puts it out (both
  lenses' glow, `beacon`'s and `halo`'s intensity 0, `claw`'s glow 0).
  The script keeps the lenses at the claw's offset and turns the beacon
  while `Moving`.

      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock --rect 762,272,114,47 --name Claw
      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock --rect 762,269,114,3 --name Rail
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark rail --rect 775,269,23,1 --colour 737373
      olr-cli level element erase content/reloaded/olr-level-liero-factory-after-dark rail --rect 775,270,23,1
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark rail --rect 775,271,23,1 --colour 737373
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark claw --rect 776,272,21,1 --colour 2c2c2c
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark claw --rect 783,282,7,1 --colour 3e3e3e
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark claw --rect 783,283,7,1 --colour 2c2c2c
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark claw --rect 785,284,3,1 --colour 7a3a14
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark halo --colour ff6a10 --intensity 0 --reach 60
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark beacon --colour ff6a10 --intensity 400 --reach 360
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark claw --glow 100 --glow-group halo
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark claw --rect 762,272,114,47 --strength 0
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark claw --rect 785,284,3,1 --strength 100
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name Lens
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark lens --rect 788,284,2,1 --colour ff9a40
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark lens --glow 100 --glow-group beacon --glow-beam 0,10
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Lens B"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark lens-b --rect 783,284,2,1 --colour ff9a40
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark lens-b --glow 100 --glow-group beacon --glow-beam 180,10
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 762,269,114,50 --layers rail,claw,lens,lens-b --name Crane --out content/assets/crane
      olr-cli asset state set content/assets/crane Normal --name Idle
      olr-cli asset state add content/assets/crane Moving
      olr-cli asset expose content/assets/crane claw_x claw.x --default 0 --min -10 --max 75
      olr-cli asset expose content/assets/crane lens_x lens.x --default 0 --min -10 --max 75
      olr-cli asset expose content/assets/crane lens_b_x lens-b.x --default 0 --min -10 --max 75
      olr-cli asset expose content/assets/crane beam_spread lens.beam-spread --default 10 --min 1 --max 90
      olr-cli asset expose content/assets/crane beam_spread_b lens-b.beam-spread --default 10 --min 1 --max 90
      olr-cli asset expose content/assets/crane reach lens.reach --default 360 --min 1 --max 360
      olr-cli asset expose content/assets/crane light lens.light --default 400 --min 0 --max 400
      olr-cli asset expose content/assets/crane halo_glow claw.glow --default 0 --min 0 --max 100
      olr-cli asset expose content/assets/crane halo_light claw.light --default 0 --min 0 --max 400
      olr-cli asset state value content/assets/crane Idle lens.glow 0
      olr-cli asset state value content/assets/crane Idle lens-b.glow 0
      olr-cli asset state value content/assets/crane Idle lens.light 0
      olr-cli asset state value content/assets/crane Idle claw.glow 0
      olr-cli asset state value content/assets/crane Idle claw.light 0
      olr-cli asset sound add content/assets/crane tools/levels/factory/sounds/hum.wav
      olr-cli asset state set content/assets/crane Moving --loop-sound hum.wav
      olr-cli asset script content/assets/crane --set tools/levels/factory/crane.rhai

- `console/`: **Console** (issue #96), the box hanging under the conveyor
  deck (`rock`) with its column of yellow LEDs (`rock-disp`, group
  `displays`, x 942). The Lit sign's rectangle is left empty: the sign
  is a copy of its own on top of it in Liero Factory Reloaded. Issue
  #97: states `Normal` (start: the LEDs as in After Dark) and `Alarm`
  (every LED red, `#ff3020`, and the slot under the sign a lit red lamp,
  `rock-disp.light` 400 and `rock-disp.animation` `Pulse(period_ms: 600,
  low: 40, high: 100)`), their pictures of `rock-disp` from
  `tools/levels/factory/art/` (`tools/levels/factory_live.py`);
  `rock-disp`'s box grows to cover the slot (drawn into it before the
  save, then the saved starting state swapped for one with the LEDs
  alone). Issue #100: Liero Factory Reloaded's copy is static again,
  the crane's alarm is the Terminal's.

      olr-cli level element erase content/reloaded/olr-level-liero-factory-after-dark rock --rect 934,299,31,21
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark rock-disp --locked false
      olr-cli level element erase content/reloaded/olr-level-liero-factory-after-dark rock-disp --rect 934,299,31,21
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark rock-disp --rect 941,326,17,4 --colour ff3020
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 933,269,33,64 --layers rock,rock-disp --name Console --out content/assets/console
      olr-cli asset state add content/assets/console Plain
      olr-cli asset state picture content/assets/console Plain rock-disp --art tools/levels/factory/art/console-normal.png
      olr-cli asset state set content/assets/console Plain --start
      olr-cli asset state remove content/assets/console Normal
      olr-cli asset state set content/assets/console Plain --name Normal
      olr-cli asset state add content/assets/console Alarm
      olr-cli asset state picture content/assets/console Alarm rock-disp --art tools/levels/factory/art/console-alarm.png
      olr-cli asset state value content/assets/console Alarm rock-disp.light 400
      olr-cli asset state value content/assets/console Alarm rock-disp.animation "Pulse(period_ms: 600, low: 40, high: 100)"

- `terminal/`: **Terminal** (issue #100), the small desk terminal left
  of the crane: its grey body and keyboard (`rock`) and its cyan screen
  edge and red lamp (`rock-bcn`, group `beacons`, pulsing slowly as in
  After Dark). States `Normal` (start: as cut) and `Alarm` (every
  `rock-bcn` pixel red, `#ff3020`, `rock-bcn.light` 400 and
  `rock-bcn.animation` `Pulse(period_ms: 600, low: 40, high: 100)`), its
  picture of `rock-bcn` from `tools/levels/factory/art/`
  (`tools/levels/factory_live.py`). Liero Factory Reloaded's level script
  puts it in Alarm while the crane moves.

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 723,351,15,14 --layers rock,rock-bcn --name Terminal --out content/assets/terminal
      olr-cli asset state add content/assets/terminal Alarm
      olr-cli asset state picture content/assets/terminal Alarm rock-bcn --art tools/levels/factory/art/terminal-alarm.png
      olr-cli asset state value content/assets/terminal Alarm rock-bcn.light 400
      olr-cli asset state value content/assets/terminal Alarm rock-bcn.animation "Pulse(period_ms: 600, low: 40, high: 100)"

- `gauge/`: **Gauge** (issue #96), the teal device of the upper floor:
  its yellow gauge and lit buttons (`rock-disp`, group `displays`) and
  its small screen (`rock-scr`, group `screens`); the grey brackets
  beside it stay in the level. Issue #99: states `A` (start: the needle
  as cut), `B` and `C` (its cyan needle, `rock-disp`'s row 4, repainted 2 and 4
  pixels further right over the dial's own pixels, the dial's yellow
  where it was; `C` swaps the two buttons' colours), pictures of
  `rock-disp` from `tools/levels/factory/art/`, no solid pixel changed;
  its default script `tools/levels/factory/gauge.rhai` changes to
  another state every 150-500 ticks.

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 1059,144,48,27 --layers rock,rock-scr,rock-disp --name Gauge --out content/assets/gauge
      olr-cli asset state set content/assets/gauge Normal --name A
      olr-cli asset state add content/assets/gauge B
      olr-cli asset state picture content/assets/gauge B rock-disp --art tools/levels/factory/art/gauge-b.png
      olr-cli asset state add content/assets/gauge C
      olr-cli asset state picture content/assets/gauge C rock-disp --art tools/levels/factory/art/gauge-c.png
      olr-cli asset script content/assets/gauge --set tools/levels/factory/gauge.rhai

## Editing an asset from the command line

`olr-cli asset state|expose|unexpose|sound|script|health` (issue #72)
edit an asset folder in place: the folder is read whole, changed by one
operation, checked and written anew with its thumbnail, or left as it
was with the reason (`<dir>: <why>`, status 1). `olr-cli asset info`
lists what a folder holds. Work on a copy: these built-in assets stay
as their blocks make them (the factory's get states of their own,
issues #97-#99). For
a strip light of your own that can be switched off, with a 29 × 2 PNG of
its dark tube (the built-in one already has `Dim` and `Off`, issue #98):

    cp -r content/assets/strip-light my-strip-light
    olr-cli asset state add my-strip-light Off
    olr-cli asset state picture my-strip-light Off lamps --art dark-tube.png
    olr-cli asset state value my-strip-light Off lamps.glow 0
