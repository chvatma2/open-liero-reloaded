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
the Terminal (issue #100), and the plant works' Dirt-ball line, Lift,
Mine cart, Forklift, Generator, Siren, Lab flask and Window pane
(2026-11-04), which Liero Factory Reloaded brings alive (all but the
Console, its copy static since issue #100 and replaced by the Dirt-ball
line since the plant works). All glow but the Window pane; the Crane
since issue #99 (its warning beacon).

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
  (`content/reloaded/README.md`). A static copy bakes steady On. The
  night shift exposes its glow colour as `colour` (default `#ffe6c0`,
  the group's), which Liero Factory Reloaded sets by area. The plant works
  (2026-11-04): `Dark` (as `Off`, for a power cut: the script leaves it
  alone until it is On again) and `Broken` (the tube shattered,
  `strip-broken.png`, entered with `pop.wav`), health 20 (`hp`),
  `Broken` at 0; set `Off` from outside while lit (Liero Factory
  Reloaded's level script does so for a blast near it), the script
  bursts; `Steady` is `On` without the buzz (a sound tube).

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
      olr-cli asset expose content/assets/strip-light colour lamps.glow-colour --default "#ffe6c0"
      olr-cli asset state add content/assets/strip-light Dark --from Off
      olr-cli asset state add content/assets/strip-light Broken
      olr-cli asset state picture content/assets/strip-light Broken lamps --art tools/levels/factory/art/strip-broken.png
      olr-cli asset state value content/assets/strip-light Broken lamps.glow 0
      olr-cli asset sound add content/assets/strip-light tools/levels/factory/sounds/pop.wav
      olr-cli asset state set content/assets/strip-light Broken --enter-sound pop.wav
      olr-cli asset health content/assets/strip-light --points 20 --at-zero Broken
      olr-cli asset expose content/assets/strip-light hp health --min 0 --max 20
      olr-cli asset state add content/assets/strip-light Steady --from On
      olr-cli asset state set content/assets/strip-light Steady --loop-sound none --enter-sound tick.wav

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
  while `Moving`. The plant works (2026-11-04): `Off` (as `Idle`, for a
  power cut: the claw stops where it is until it is Idle again).

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
      olr-cli asset state add content/assets/crane Off --from Idle
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

- `ball-line/`: **Dirt-ball line** (the plant works, 2026-11-04), the
  lower hall's line in one copy, so its script keeps the stamp and the
  balls in step: the stamp under the 13-37 sign (its housing, `housing`,
  and its die, `die`, the cylinder and pressing plate, cut from After
  Dark's `rock`; the sign is a copy of its own on top), a ram behind the
  housing (`ram`, drawn from the die's cylinder, a Background layer that
  shows as the die drops), the LED column (`leds`, group `displays`) as
  the stamp's countdown, the belt's red stripe on the flat run and the
  incline (`stripe`, its rock pixels, group `belt`, `#ff2a18`) as the
  line's status light, six dirt balls (`ball-1` … `ball-6`, the
  incline's painted ball, Background, hidden; glowing at 60 in `balls`, a
  group that casts nothing, so they show their art at least at 60 % as
  front dirt does by its floor), the drive's casing over them (`drive`,
  rock, metal, drawn from After Dark's `rock`: the balls roll through it,
  the user's playtest, 2026-11-05) and its running lamp (`drive-lamp`,
  group `running`). States `Stopped` (start: the LEDs
  dark, the stripe red, the lamp out) and `Lit 1` … `Lit 7` (running:
  that many LEDs lit from the bottom, the stripe white, `#f4f0ff`, the
  lamp lit; each ticks, `tick.wav`, and loops `rumble.wav`). Exposed
  `die_y` and `ram_y` (0-18: the plate onto the belt), and each ball's
  `b<i>_x` (from each ball's own place, 4 (i − 1) px along the flat
  run: −4 (i − 1) to 96 − 4 (i − 1)), `b<i>_y` (−29-0) and `b<i>_on`. Its pictures and
  masks are from `tools/levels/factory_works.py`; `thump.wav`,
  `tick.wav`, `rumble.wav` and `horn.wav` are synthesised for OLR by
  `tools/sounds/factory_sounds.py`, CC0. Its default script
  `tools/levels/factory/line.rhai` counts down, drops the die (killing a
  worm it wedges, `crushed` and `hurt`), sends a ball along the belt,
  through the drive and up the incline into the crate, and again, while
  not Stopped; Liero Factory Reloaded's level
  script starts and stops it (the shift).

      olr-cli level element erase content/reloaded/olr-level-liero-factory-after-dark rock --rect 934,299,31,21
      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock --rect 933,269,33,57 --name Housing
      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock --rect 933,326,33,7 --name Die
      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock --mask tools/levels/factory/art/line-stripe-mask.png --name Stripe
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark rock-disp --locked false
      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock-disp --rect 942,279,1,19 --name Leds
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark leds --rect 942,279,1,19 --strength 100
      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark backdrop --rect 941,308,17,18 --name Ram
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark ram --mask tools/levels/factory/art/line-ram-mask.png --tile tools/levels/factory/art/line-ram-tile.png
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark belt --colour ff2a18 --intensity 300 --reach 26
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark stripe --glow 100 --glow-group belt
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark stripe --mask tools/levels/factory/art/line-stripe-glow.png --strength 100
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark running --colour 50ff70 --intensity 160 --reach 12
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Ball 1"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark ball-1 --mask tools/levels/factory/art/line-ball-1-mask.png --tile tools/levels/factory/art/line-ball-1-tile.png
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Ball 2"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark ball-2 --mask tools/levels/factory/art/line-ball-2-mask.png --tile tools/levels/factory/art/line-ball-2-tile.png
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Ball 3"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark ball-3 --mask tools/levels/factory/art/line-ball-3-mask.png --tile tools/levels/factory/art/line-ball-3-tile.png
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Ball 4"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark ball-4 --mask tools/levels/factory/art/line-ball-4-mask.png --tile tools/levels/factory/art/line-ball-4-tile.png
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Ball 5"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark ball-5 --mask tools/levels/factory/art/line-ball-5-mask.png --tile tools/levels/factory/art/line-ball-5-tile.png
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Ball 6"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark ball-6 --mask tools/levels/factory/art/line-ball-6-mask.png --tile tools/levels/factory/art/line-ball-6-tile.png
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark balls --colour own --intensity 0 --reach 1
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark ball-1 --glow 60 --glow-group balls
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark ball-2 --glow 60 --glow-group balls
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark ball-3 --glow 60 --glow-group balls
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark ball-4 --glow 60 --glow-group balls
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark ball-5 --glow 60 --glow-group balls
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark ball-6 --glow 60 --glow-group balls
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material rock --name Drive
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark drive --mask tools/levels/factory/art/line-drive-mask.png --tile tools/levels/factory/art/line-drive-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark drive --surface metal
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Drive lamp"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark drive-lamp --mask tools/levels/factory/art/line-lamp-mask.png --tile tools/levels/factory/art/line-lamp-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark drive-lamp --glow 100 --glow-group running
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 909,269,150,88 --layers ram,housing,die,leds,stripe,ball-1,ball-2,ball-3,ball-4,ball-5,ball-6,drive,drive-lamp --name "Dirt-ball line" --out content/assets/ball-line
      olr-cli asset state add content/assets/ball-line Plain
      olr-cli asset state picture content/assets/ball-line Plain leds --art tools/levels/factory/art/line-leds-0.png --glow tools/levels/factory/art/line-leds-0-glow.png
      olr-cli asset state set content/assets/ball-line Plain --start
      olr-cli asset state remove content/assets/ball-line Normal
      olr-cli asset state set content/assets/ball-line Plain --name Stopped
      olr-cli asset state value content/assets/ball-line Stopped drive-lamp.glow 0
      olr-cli asset sound add content/assets/ball-line tools/levels/factory/sounds/thump.wav
      olr-cli asset sound add content/assets/ball-line tools/levels/factory/sounds/tick.wav
      olr-cli asset sound add content/assets/ball-line tools/levels/factory/sounds/rumble.wav
      olr-cli asset sound add content/assets/ball-line tools/levels/factory/sounds/horn.wav
      olr-cli asset state add content/assets/ball-line "Lit 1"
      olr-cli asset state picture content/assets/ball-line "Lit 1" leds --art tools/levels/factory/art/line-leds-1.png --glow tools/levels/factory/art/line-leds-1-glow.png
      olr-cli asset state picture content/assets/ball-line "Lit 1" stripe --art tools/levels/factory/art/line-stripe-run.png
      olr-cli asset state picture content/assets/ball-line "Lit 1" drive-lamp --art tools/levels/factory/art/line-lamp-on.png
      olr-cli asset state value content/assets/ball-line "Lit 1" stripe.glow-colour "#f4f0ff"
      olr-cli asset state value content/assets/ball-line "Lit 1" drive-lamp.glow 100
      olr-cli asset state set content/assets/ball-line "Lit 1" --loop-sound rumble.wav --enter-sound tick.wav
      olr-cli asset state add content/assets/ball-line "Lit 2" --from "Lit 1"
      olr-cli asset state picture content/assets/ball-line "Lit 2" leds --art tools/levels/factory/art/line-leds-2.png --glow tools/levels/factory/art/line-leds-2-glow.png
      olr-cli asset state add content/assets/ball-line "Lit 3" --from "Lit 1"
      olr-cli asset state picture content/assets/ball-line "Lit 3" leds --art tools/levels/factory/art/line-leds-3.png --glow tools/levels/factory/art/line-leds-3-glow.png
      olr-cli asset state add content/assets/ball-line "Lit 4" --from "Lit 1"
      olr-cli asset state picture content/assets/ball-line "Lit 4" leds --art tools/levels/factory/art/line-leds-4.png --glow tools/levels/factory/art/line-leds-4-glow.png
      olr-cli asset state add content/assets/ball-line "Lit 5" --from "Lit 1"
      olr-cli asset state picture content/assets/ball-line "Lit 5" leds --art tools/levels/factory/art/line-leds-5.png --glow tools/levels/factory/art/line-leds-5-glow.png
      olr-cli asset state add content/assets/ball-line "Lit 6" --from "Lit 1"
      olr-cli asset state picture content/assets/ball-line "Lit 6" leds --art tools/levels/factory/art/line-leds-6.png --glow tools/levels/factory/art/line-leds-6-glow.png
      olr-cli asset state add content/assets/ball-line "Lit 7" --from "Lit 1"
      olr-cli asset state picture content/assets/ball-line "Lit 7" leds --art tools/levels/factory/art/line-leds-7.png --glow tools/levels/factory/art/line-leds-7-glow.png
      olr-cli asset expose content/assets/ball-line die_y die.y --default 0 --min 0 --max 18
      olr-cli asset expose content/assets/ball-line ram_y ram.y --default 0 --min 0 --max 18
      olr-cli asset expose content/assets/ball-line b1_x ball-1.x --default 0 --min 0 --max 96
      olr-cli asset expose content/assets/ball-line b1_y ball-1.y --default 0 --min -29 --max 0
      olr-cli asset expose content/assets/ball-line b1_on ball-1.visible --default false
      olr-cli asset expose content/assets/ball-line b2_x ball-2.x --default 0 --min -4 --max 92
      olr-cli asset expose content/assets/ball-line b2_y ball-2.y --default 0 --min -29 --max 0
      olr-cli asset expose content/assets/ball-line b2_on ball-2.visible --default false
      olr-cli asset expose content/assets/ball-line b3_x ball-3.x --default 0 --min -8 --max 88
      olr-cli asset expose content/assets/ball-line b3_y ball-3.y --default 0 --min -29 --max 0
      olr-cli asset expose content/assets/ball-line b3_on ball-3.visible --default false
      olr-cli asset expose content/assets/ball-line b4_x ball-4.x --default 0 --min -12 --max 84
      olr-cli asset expose content/assets/ball-line b4_y ball-4.y --default 0 --min -29 --max 0
      olr-cli asset expose content/assets/ball-line b4_on ball-4.visible --default false
      olr-cli asset expose content/assets/ball-line b5_x ball-5.x --default 0 --min -16 --max 80
      olr-cli asset expose content/assets/ball-line b5_y ball-5.y --default 0 --min -29 --max 0
      olr-cli asset expose content/assets/ball-line b5_on ball-5.visible --default false
      olr-cli asset expose content/assets/ball-line b6_x ball-6.x --default 0 --min -20 --max 76
      olr-cli asset expose content/assets/ball-line b6_y ball-6.y --default 0 --min -29 --max 0
      olr-cli asset expose content/assets/ball-line b6_on ball-6.visible --default false
      olr-cli asset script content/assets/ball-line --set tools/levels/factory/line.rhai

- `lift/`: **Lift** (the plant works), a goods lift's cage: its floor
  (`cage-floor`, Rock, hazard striped, a blinking light at each end in
  group `lift`, `Pulse`) and its frame and lamp (`cage`, Background,
  group `cage`). States `Idle` (start: the blinkers out), `Moving` (lit,
  looping `whine.wav`) and `Off` (the lamp out too); `floor_y` and
  `cage_y` (0-87) move it down. Its default script
  `tools/levels/factory/lift.rhai` rests at each end, then rides to the
  other; `Off` stops it where it is.

      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark cage --colour fff0d0 --intensity 380 --reach 30
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark lift --colour ffb040 --intensity 300 --reach 26 --animation "Pulse(period_ms: 1000, low: 30, high: 100, shape: Square)"
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material rock --name "Cage floor"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark cage-floor --mask tools/levels/factory/art/lift-floor-mask.png --tile tools/levels/factory/art/lift-floor-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark cage-floor --glow 100 --glow-group lift
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark cage-floor --mask tools/levels/factory/art/lift-floor-glow.png --strength 100
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name Cage
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark cage --mask tools/levels/factory/art/lift-frame-mask.png --tile tools/levels/factory/art/lift-frame-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark cage --glow 100 --glow-group cage
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark cage --mask tools/levels/factory/art/lift-frame-glow.png --strength 100
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 1186,352,12,104 --layers cage,cage-floor --name Lift --out content/assets/lift
      olr-cli asset state set content/assets/lift Normal --name Idle
      olr-cli asset state value content/assets/lift Idle cage-floor.glow 0
      olr-cli asset state add content/assets/lift Moving
      olr-cli asset state value content/assets/lift Moving cage-floor.glow 100
      olr-cli asset state add content/assets/lift Off
      olr-cli asset state value content/assets/lift Off cage.glow 0
      olr-cli asset expose content/assets/lift floor_y cage-floor.y --default 0 --min 0 --max 87
      olr-cli asset expose content/assets/lift cage_y cage.y --default 0 --min 0 --max 87
      olr-cli asset sound add content/assets/lift tools/levels/factory/sounds/whine.wav
      olr-cli asset state set content/assets/lift Moving --loop-sound whine.wav
      olr-cli asset script content/assets/lift --set tools/levels/factory/lift.rhai

- `mine-cart/`: **Mine cart** (the plant works), a cart loaded with dirt
  on rails (`cart`, Rock; the load is its art, not dirt) and a lamp at
  each end (`lamp`, beam 0°, and `lamp-b`, 180°, group
  `cartlamp`). States `Stopped` (start: lamps out), `Right` and `Left`
  (the lamp ahead lit, looping `cart.wav`) and `Off`; `cart_x`,
  `lamp_x` and `lamp_b_x` (0-167) move it. Its default script
  `tools/levels/factory/cart.rhai` runs it to the end of its rails and
  back, turning where it is blocked.

      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark cartlamp --colour ffd090 --intensity 420 --reach 60
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material rock --name Cart
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark cart --mask tools/levels/factory/art/cart-mask.png --tile tools/levels/factory/art/cart-tile.png
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Lamp"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark lamp --mask tools/levels/factory/art/cart-lamp-mask.png --tile tools/levels/factory/art/cart-lamp-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark lamp --glow 100 --glow-group cartlamp --glow-beam 0,18
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Lamp B"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark lamp-b --mask tools/levels/factory/art/cart-lamp-b-mask.png --tile tools/levels/factory/art/cart-lamp-b-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark lamp-b --glow 100 --glow-group cartlamp --glow-beam 180,18
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 12,456,187,17 --layers cart,lamp,lamp-b --name "Mine cart" --out content/assets/mine-cart
      olr-cli asset state set content/assets/mine-cart Normal --name Stopped
      olr-cli asset state value content/assets/mine-cart Stopped lamp.glow 0
      olr-cli asset state value content/assets/mine-cart Stopped lamp-b.glow 0
      olr-cli asset state add content/assets/mine-cart Right
      olr-cli asset state value content/assets/mine-cart Right lamp.glow 100
      olr-cli asset state add content/assets/mine-cart Left
      olr-cli asset state value content/assets/mine-cart Left lamp-b.glow 100
      olr-cli asset state add content/assets/mine-cart Off
      olr-cli asset expose content/assets/mine-cart cart_x cart.x --default 0 --min 0 --max 167
      olr-cli asset expose content/assets/mine-cart lamp_x lamp.x --default 0 --min 0 --max 167
      olr-cli asset expose content/assets/mine-cart lamp_b_x lamp-b.x --default 0 --min 0 --max 167
      olr-cli asset sound add content/assets/mine-cart tools/levels/factory/sounds/cart.wav
      olr-cli asset state set content/assets/mine-cart Right --loop-sound cart.wav
      olr-cli asset state set content/assets/mine-cart Left --loop-sound cart.wav
      olr-cli asset script content/assets/mine-cart --set tools/levels/factory/cart.rhai

- `forklift/`: **Forklift** (the plant works), with a crate of the
  warehouse's stack on its forks (`forklift`, Rock, its roof beacon in
  group `forkbeacon`, `Pulse`) and a work light on top of its mast
  (`fork-light`, beam 160°, group `forklight`). States `Idle` (start),
  `Driving` (`engine.wav`) and `Reversing` (`beep.wav`); `fork_x` and
  `light_x` (0-14) move it back. Its default script
  `tools/levels/factory/forklift.rhai` backs away from the stack and
  drives up to it again.

      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark forkbeacon --colour ffa020 --intensity 350 --reach 30 --animation "Pulse(period_ms: 800, low: 0, high: 100, shape: Square)"
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark forklight --colour fff0c8 --intensity 700 --reach 80
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material rock --name Forklift
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark forklift --mask tools/levels/factory/art/forklift-mask.png --tile tools/levels/factory/art/forklift-tile.png
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark forklift --mask tools/levels/factory/art/forklift-cab-mask.png --tile tools/levels/factory/art/forklift-cab-tile.png
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark forklift --mask tools/levels/factory/art/forklift-beacon-mask.png --tile tools/levels/factory/art/forklift-beacon-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark forklift --glow 100 --glow-group forkbeacon
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark forklift --mask tools/levels/factory/art/forklift-beacon-glow.png --strength 100
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Fork light"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark fork-light --mask tools/levels/factory/art/forklift-light-mask.png --tile tools/levels/factory/art/forklift-light-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark fork-light --glow 100 --glow-group forklight --glow-beam 160,24
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 536,374,52,30 --layers forklift,fork-light --name Forklift --out content/assets/forklift
      olr-cli asset state set content/assets/forklift Normal --name Idle
      olr-cli asset state add content/assets/forklift Driving
      olr-cli asset state add content/assets/forklift Reversing
      olr-cli asset expose content/assets/forklift fork_x forklift.x --default 0 --min 0 --max 14
      olr-cli asset expose content/assets/forklift light_x fork-light.x --default 0 --min 0 --max 14
      olr-cli asset sound add content/assets/forklift tools/levels/factory/sounds/engine.wav
      olr-cli asset sound add content/assets/forklift tools/levels/factory/sounds/beep.wav
      olr-cli asset state set content/assets/forklift Driving --loop-sound engine.wav
      olr-cli asset state set content/assets/forklift Reversing --loop-sound beep.wav
      olr-cli asset script content/assets/forklift --set tools/levels/factory/forklift.rhai

- `generator/`: **Generator** (the plant works), the upper floor's gauge's
  mounting as the plant's generator (`housing`, Rock, cut from After Dark
  with the Gauge's rectangle left out: the Gauge is a copy of its own),
  its status lamps (`leds`, group `status`, `Flicker`) and a
  vent (`vent`, group `vent`, `Pulse`). States `Running` (start, looping
  `idle.wav`) and `Wrecked` (sooted, lamps and vent dark, entered with
  `pop.wav`), health 40 (`hp`), `Wrecked` at 0. Its default script
  `tools/levels/factory/generator.rhai` repairs it after 30 s. Liero
  Factory Reloaded's level script blacks the plant out while it is
  wrecked.

      olr-cli level element erase content/reloaded/olr-level-liero-factory-after-dark rock --rect 1059,144,48,27
      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock --rect 1046,146,72,25 --name Housing
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark status --colour own --intensity 160 --reach 14 --animation "Flicker(speed: 2, depth: 20)"
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark vent --colour ff8030 --intensity 260 --reach 30 --animation "Pulse(period_ms: 3000, low: 40, high: 100)"
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Leds"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark leds --mask tools/levels/factory/art/gen-lamps-mask.png --tile tools/levels/factory/art/gen-lamps-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark leds --glow 100 --glow-group status
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name Vent
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark vent --mask tools/levels/factory/art/gen-vent-mask.png --tile tools/levels/factory/art/gen-vent-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark vent --glow 100 --glow-group vent
      olr-cli level element glow content/reloaded/olr-level-liero-factory-after-dark vent --mask tools/levels/factory/art/gen-vent-glow.png --strength 100
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 1046,146,72,25 --layers housing,leds,vent --name Generator --out content/assets/generator
      olr-cli asset state set content/assets/generator Normal --name Running
      olr-cli asset state add content/assets/generator Wrecked
      olr-cli asset state picture content/assets/generator Wrecked housing --art tools/levels/factory/art/gen-housing-wrecked.png
      olr-cli asset state picture content/assets/generator Wrecked leds --art tools/levels/factory/art/gen-lamps-wrecked.png
      olr-cli asset state picture content/assets/generator Wrecked vent --art tools/levels/factory/art/gen-vent-wrecked.png
      olr-cli asset state value content/assets/generator Wrecked leds.glow 0
      olr-cli asset state value content/assets/generator Wrecked vent.glow 0
      olr-cli asset sound add content/assets/generator tools/levels/factory/sounds/idle.wav
      olr-cli asset sound add content/assets/generator tools/levels/factory/sounds/pop.wav
      olr-cli asset state set content/assets/generator Running --loop-sound idle.wav
      olr-cli asset state set content/assets/generator Wrecked --enter-sound pop.wav
      olr-cli asset health content/assets/generator --points 40 --at-zero Wrecked
      olr-cli asset expose content/assets/generator hp health --min 0 --max 40
      olr-cli asset script content/assets/generator --set tools/levels/factory/generator.rhai

- `siren/`: **Siren** (the plant works), the roof's two speaker rings
  (the Warning beacon's and the one right of it, `backdrop` and
  `beacons`) and four lenses, two at each ring's middle (`lens-a` …
  `lens-d`, beams 180° and 0°, group `siren`, `#ff2810`, intensity 0).
  States `Normal` (start: dark) and `Alarm` (lit, looping `klaxon.wav`);
  exposed each lens's `spread_<a-d>`, `reach` and `light`. Its default
  script `tools/levels/factory/siren.rhai` turns the beams while it is in
  Alarm, as the Crane's beacon turns.

      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark siren --colour ff2810 --intensity 0 --reach 150
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Lens A"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark lens-a --rect 961,55,2,1 --colour ff7850
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark lens-a --glow 100 --glow-group siren --glow-beam 180,14
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Lens B"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark lens-b --rect 964,55,2,1 --colour ff7850
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark lens-b --glow 100 --glow-group siren --glow-beam 0,14
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Lens C"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark lens-c --rect 997,55,2,1 --colour ff7850
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark lens-c --glow 100 --glow-group siren --glow-beam 180,14
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name "Lens D"
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark lens-d --rect 1000,55,2,1 --colour ff7850
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark lens-d --glow 100 --glow-group siren --glow-beam 0,14
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 951,45,62,28 --layers backdrop,beacons,lens-a,lens-b,lens-c,lens-d --name Siren --out content/assets/siren
      olr-cli asset state value content/assets/siren Normal lens-a.glow 0
      olr-cli asset state value content/assets/siren Normal lens-b.glow 0
      olr-cli asset state value content/assets/siren Normal lens-c.glow 0
      olr-cli asset state value content/assets/siren Normal lens-d.glow 0
      olr-cli asset state add content/assets/siren Alarm
      olr-cli asset state value content/assets/siren Alarm lens-a.glow 100
      olr-cli asset state value content/assets/siren Alarm lens-b.glow 100
      olr-cli asset state value content/assets/siren Alarm lens-c.glow 100
      olr-cli asset state value content/assets/siren Alarm lens-d.glow 100
      olr-cli asset state value content/assets/siren Alarm lens-a.light 1000
      olr-cli asset expose content/assets/siren spread_a lens-a.beam-spread --default 14 --min 1 --max 90
      olr-cli asset expose content/assets/siren spread_b lens-b.beam-spread --default 14 --min 1 --max 90
      olr-cli asset expose content/assets/siren spread_c lens-c.beam-spread --default 14 --min 1 --max 90
      olr-cli asset expose content/assets/siren spread_d lens-d.beam-spread --default 14 --min 1 --max 90
      olr-cli asset expose content/assets/siren reach lens-a.reach --default 150 --min 1 --max 360
      olr-cli asset expose content/assets/siren light lens-a.light --default 0 --min 0 --max 1000
      olr-cli asset sound add content/assets/siren tools/levels/factory/sounds/klaxon.wav
      olr-cli asset state set content/assets/siren Alarm --loop-sound klaxon.wav
      olr-cli asset script content/assets/siren --set tools/levels/factory/siren.rhai

- `lab-flask/`: **Lab flask** (the plant works), the lab's still: its
  flask's liquid (`liquid`, green, group `flask`, a slow `Pulse`; After
  Dark's was olive dirt) and a burner's flame (`burner`, blue, group
  `burner`, `Flicker`), looping `bubble.wav`.

      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark flask --colour 70ff90 --intensity 220 --reach 26 --animation "Pulse(period_ms: 2600, low: 50, high: 100)"
      olr-cli level group add content/reloaded/olr-level-liero-factory-after-dark burner --colour 5080ff --intensity 260 --reach 20 --animation "Flicker(speed: 8, depth: 40)"
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name Liquid
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark liquid --mask tools/levels/factory/art/lab-liquid-mask.png --tile tools/levels/factory/art/lab-liquid-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark liquid --glow 80 --glow-group flask
      olr-cli level element add-layer content/reloaded/olr-level-liero-factory-after-dark --material background --name Burner
      olr-cli level element draw content/reloaded/olr-level-liero-factory-after-dark burner --mask tools/levels/factory/art/lab-burner-mask.png --tile tools/levels/factory/art/lab-burner-tile.png
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark burner --glow 100 --glow-group burner
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 370,205,22,17 --layers liquid,burner --name "Lab flask" --out content/assets/lab-flask
      olr-cli asset sound add content/assets/lab-flask tools/levels/factory/sounds/bubble.wav
      olr-cli asset state set content/assets/lab-flask Normal --loop-sound bubble.wav

- `window-pane/`: **Window pane** (the plant works), one of the upper
  hall's window panes (`glass`); states `Intact` (start) and `Broken`
  (a jagged rim left, `tools/levels/factory/art/pane-broken.png`,
  entered with `smash.wav`), health 5, `Broken` at 0.

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 719,151,19,22 --layers glass --name "Window pane" --out content/assets/window-pane
      olr-cli asset state set content/assets/window-pane Normal --name Intact
      olr-cli asset state add content/assets/window-pane Broken
      olr-cli asset state picture content/assets/window-pane Broken glass --art tools/levels/factory/art/pane-broken.png
      olr-cli asset sound add content/assets/window-pane tools/levels/factory/sounds/smash.wav
      olr-cli asset state set content/assets/window-pane Broken --enter-sound smash.wav
      olr-cli asset health content/assets/window-pane --points 5 --at-zero Broken

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
