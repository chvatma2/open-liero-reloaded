# OLR's own packs

Hand-made packs that ship with every release beside the imported OLX
content (`content/packs`, which holds only what the OLX import makes, so
`cargo xtask check` can compare it with a fresh import). The programs lay
this directory over the content wherever it is opened
(`olr_content::bundled_roots`: the user's data `packs/` on top, then
`content/reloaded`, then `content/packs`); `cargo xtask dist` copies it
into the Windows zip and the Flatpak (this README stays behind). A pack
id here must not also be one in `content/packs` (`olr-cli`'s
`tests/reloaded.rs`, which also checks every pack here with `pack check
--content content/packs` and plays a bot match on each level and with
the mod; `tests/skin.rs` runs `olr-cli skin check` on the skin and
finds it in the profiles' skin list). The game
refuses to edit them in place: copy one from the Editor menu.

Remade OLX content is named "<Old name> Reloaded".

- `olr-mod-liero-v1-reloaded/`: **Liero v1.0 Reloaded** (issue #21), the
  mod a fresh profile plays (`olr_ui::DEFAULT_MOD`). OLX's Classic
  (`origin`: `share/gamedir/Classic` at `d1ad953f`), made with `olr-cli
  pack new mod olr-mod-liero-v1-reloaded --from content/packs/olx-classic
  --name "Liero v1.0 Reloaded"`; its gameplay is Classic's: the compiled
  mod but for its name, every image and sound, the presets and loadouts
  the same, and the same bot matches give the same checksums (tested; the
  bots arm for it as for Classic, its name holding "Liero v1.0"). Only
  `look` blocks were added (`docs/formats/mod.md`), lights for the weapons
  where light makes sense; plain bullets, grenades, mines, nukes, dirt and
  the rest stay dark in flight, and their explosions light to suit them
  (issue #15, below):
  - Bazooka: the missile glows (80) and lights the tunnel, warm orange
    `#ffa040` at 70 %, reach 44, flickering.
  - Mini Rockets and Doomsday: the same rocket light, smaller (40 % and
    35 %, reach 28; Doomsday a little redder, `#ff9040`), glow 70.
  - Laser: a red muzzle light (`#ff2828`, 100 %, reach 48) while it fires
    (a beam has no projectile to light).
  - Flamer: each flame glows (90) with a faint warm light (`#ff8030`,
    12 %, reach 20) that dies with it over its 0.6 s; an orange muzzle
    light (`#ff9040`, 70 %, reach 40).
  - Napalm: the canister glows (70) with a flickering light (`#ff9030`,
    30 %, reach 28), bursts with an end flash (`#ffa040`, 200 %, reach 96,
    fading over 0.45 s), and its burning drops glow fully with a small
    flickering light each (`#ff8020`, 15 %, reach 22).
  - Blaster: the ball glows (90) with a steady light (`#ffb040`, 50 %,
    reach 36) and bursts with an end flash (`#ffc060`, 160 %, reach 80,
    0.25 s); its sparks glow (70), unlit.
  - Gauss Gun: red tracers, glowing (100) with a light each (`#ff4030`,
    30 %, reach 28), and a heavy muzzle flash (`#ffd0b0`, 150 %, reach 72,
    80 ms).
  - Crackler and Hellraider: the same red ball glows (80), unlit (they
    come in crowds).
  - Zimm: the lavender ball glows (100) and pulses (`#c0a0ff`, 60 %,
    reach 40, 70-100 % every 0.7 s); its flash lights up (`#e0d0ff`,
    150 %, reach 64, fading over its 0.2 s).
  - Uzi: tracer rounds, glowing (80) with a small light (`#ffb040`, 12 %,
    reach 16).
  - Heavy guns' muzzle flash, stronger than the engine's (80 %, reach 40,
    50 ms): Super Shotgun (`#ffdca0`, 130 %, reach 60, 70 ms), Cannon
    (`#ffd090`, 150 %, reach 64, 80 ms), Rifle and Winchester (`#ffe0b0`,
    120 %, reach 56, 60 ms).
  - Explosion lights (issue #15), each fading over its explosion as the
    engine's does, in place of the engine's (warm, 124 % and reach 88 for
    the commonest explosion, size 8):
    - By default (`mod.ron`'s `look`): a small impact light, `#ffb070`
      at 60 %, a disc of 2, reach 28: bullets, shrapnel, sparks, bits and
      every projectile not named here.
    - Nukes flash the cave: Big Nuke's detonation (`#fff0d8`, 600 %, a
      disc of 16, reach 480) and its 50 bomblets (`#ffd090`, 160 %, 8,
      reach 110); Mini Nuke's (`#fff0d8`, 400 %, 12, reach 260) and its
      bomblets (`#ffd090`, 130 %, 6, reach 80).
    - Big explosives, hot (`#ffd090`): Mine, Booby Trap and Bouncy Mine
      (320 %, 12, reach 200), Bazooka and Float Mine (260 %, 10, reach
      160), Chiquita Bomb and Cluster Bomb (240 %, 10, reach 150; their
      bits the default).
    - Medium blasts (`#ffbc70`): Grenade (200 %, 8, reach 120; its
      fragments the default), Cannon and Explosives (180 %, 7, reach 110),
      Bouncy Larpa (160 %, 6, reach 96), Mini Rockets and Doomsday
      (150 %, 6, reach 90), Larpa (140 %, 6, reach 88; its trail's drops
      the default) and RB Rampage (120 %, 5, reach 72).
    - Napalm's explosive trail (a cosmetic of its flight, no projectile's
      explosion) keeps the engine's light.
  - Glow masks (issue #19, `gfx/<name>.glow.png`; Classic's images
    unchanged, so the frame counts and the play are Classic's, tested):
    the hot pixels glow whatever the darkness, the brightest fully, the
    next at 75 % and the darkest orange at 50 %; grey shells, metal and
    smoke stay unlit. Made with `olr-cli mod gfx glow … --colour`:
    the Flamer's flame (its yellow core fully, `#f8dc28`, `#f4f450`,
    `#f4e83c` and `#f8c024`; `#f8a420` and `#f8961c` at 75 %), the
    Napalm canister's core (`#ecb400` and `#dea000` fully, `#d08c00` and
    `#c27800` at 75 %, `#b46400` at 50 %), the Blaster's ball (the same
    oranges but the darkest), the Mini Nuke's centre (`#ecb400`, and
    `#d08c00` at 75 %) and the Big Nuke's top (as the Napalm's, with
    `#a65000` at 50 %).
  - Heat (issue #46, `docs/formats/mod.md`'s `explosion_heat` and
    `heat`; the engine's heat is `1.2 × damage⁴` degrees of stone at the
    centre, at most 1000, over twice the hole's radius plus half the
    damage): every explosion not named here keeps the engine's. The
    flames heat what they pass, 8 degrees a tick over 5 pixels (a wall
    they play on warms; one they only pass, as a back wall, takes a
    fifth), and Napalm's burning drops 12 over 6 (their landing keeps
    the engine's 750 over 17.5). Bazooka 150 % over 18 pixels (its rim
    glowing orange-yellow; a worm hit's 1000 kept to 18 pixels, not
    28.5). Mine, Bouncy Mine and Float Mine 400 % over 20 (their timed
    blast of damage 3 would barely glow; a worm hit's 1000 not over
    31), Booby Trap the engine's amount over 20. Mini Nuke's detonation
    200 % over 22 (its bomblets the engine's). Big Nuke's 50 bomblets
    80 % over 24 (a wide orange-white field rather than 50 white discs;
    its detonation has no damage and so no heat), their sparks
    (`projectiles/big-nuke.s2.ron`, also the Mini Nuke's) 300 %
    of their faint 97.

- `olr-level-liero-factory-after-dark/`: **Liero Factory After Dark** (issue
  #7). It was Liero Factory Reloaded (`olr-level-liero-factory-reloaded`)
  until 0.3, renamed in issue #70: that name and id now belong to the
  level built from assets (issue #96). OLX's Liero Factory (`origin`:
  `Liero Factory.lxl` at `d1ad953f`) at night, its lights on; its gameplay and art unchanged: the imported
  level split into layers (`olr-cli level element split`), its
  `material.png` byte for byte the imported level's, and its `front.png`
  and `back.png` too but where the sky is see-through (tested; issue
  #63, below). Above them, one drawn layer per glow group and
  material holds only that group's pixels, found by colour in the
  imported `front.png`: `sky` (the blue gradient, a pale moonlight tint
  of unlimited reach; a distant layer since issue #63), `lamps` (the white ceiling strips of every floor,
  warm white, reach 90), `glass` (the office and main hall windows, a
  faint cool glow), `screens` (the office computer, the control room's
  monitor and console, the teal device's screen: their own colour, short
  reach), `displays` (the "13-37" board and its LEDs, the gauge and the
  console's lamps, a faint `Flicker`) and `beacons` (the roof speakers'
  red rings and the laser by the ramp, a slow `Pulse`). Ambient
  `#405880` at 40 %: night, with dark corners, still readable.
- `olr-level-castlestrike-reloaded/`: **Castle Strike Reloaded** (issue
  #7), the day Castle Strike made for plan 15c's showcase (formerly
  `content/playtest/olr-level-castlestrike-day`, unchanged): a low
  blue-grey ambient, the sky glowing warm white at 300 % (group `sky`,
  unlimited reach) so daylight comes in through the hall, the loft and
  the towers, and the cavern torches and the hall's candles flickering
  (group `flames`).
- **Parallax skies** (issue #63): both levels' skies lie on a glowing
  distant layer, scrolling slower than the level, the light as before.
  `tools/levels/reloaded_skies.py` made them through `olr-cli level
  element`, from the levels as they were:
  - **The sky cut out.** The sky region is Castle Strike's sky glow
    mask (`editor/glow/background-2.png`, its non-zero pixels, rows
    0-304) and Liero Factory's `sky` layer (its covered pixels). It is
    written as a PNG (white the sky, black the rest) and erased from
    every back element covering it with `level element erase <id>
    --mask <png>`: Castle Strike's `background` and `background-2` (the
    hills, walls and towers stay), Liero Factory's `backdrop` (then
    `background`; issue #70 shortened the factory's ids); the
    factory's `sky` layer (unlocked first) is removed.
  - **The layer.** A level-sized image at (0, 0) (`add-image --at 0,0`,
    no colour reduction, `editor/images/sky.png`), Background, named
    Sky, set distant with the old sky's glow: Castle Strike's at depth
    85, glow 100 in group `sky` (the old mask was 255 over the whole
    region, so no mask), Liero Factory's at depth 90, glow 35 in `sky`,
    locked as the old layer was. With the camera at the level's centre
    (the layer's reference placement) it shows the old sky's art at the
    old sky's pixels, byte for byte, so the baked glow, the look maps
    and the light grid are as before. The rest (behind the hills and
    buildings) continues the sky's radial gradient: the centre fitted
    to the sky (the point whose half-pixel rings vary least: Castle
    Strike (724, 291), Liero Factory (592, 226)), then each pixel takes
    the commonest colour of the old sky at its distance (Castle
    Strike's crisp bands) or one drawn from those within 4 of the
    median brightness there (the factory's dithered gradient); never
    stretched. Only the layer's middle is ever seen in play (`view +
    (1 − depth)(size − view)`: Castle Strike's (392, 152) to (958,
    398), Liero Factory's (347, 129) to (853, 351)).
  - **Stars** (Liero Factory): single art pixels, 1 in 20 of the two
    brightest 2 × 2, never touching another; four colours from pale
    blue to white (`#93acdf` 45 %, `#bccff5` 30 %, `#e4ecff` 18 %,
    `#ffffff` 7 %); a chance per pixel of 1/140 at the top of the part
    ever seen to 1/420 at its bottom (487 stars there, 1 per 230
    pixels). SplitMix64, seed 63. They are the layer's art, so they glow
    with it and show only where the sky shows; at the reference
    placement 1293 sky pixels became stars (the light grid's hashes did
    not change).
  - **The roof garden** (Liero Factory, 2026-10-25): the old sky the
    flowers on the left roof hid (the cut-out above took the sky layer's
    pixels, which the dirt flowers covered) showed as fixed blue
    flower shapes once the dirt was shot away. Erased from `backdrop`
    with `level element erase --mask` (its 966 pixels within 8 a channel
    of the sky's art, in x 380-639, y 120-175), then `level bake`: they
    show the distant sky now, within 2 a channel of OLX's colour.
  - **The windows are glass** (Liero Factory, 2026-10-25, the background
    glass design): the `glass` element's pixels mixed halfway to
    `(200, 215, 235)`, `glass: (opacity: 35)`, its glow 67 in the group
    `glass` (`Tint("#c8d4f0")`, intensity 220, reach 160: the window's
    moonlight); the `backdrop` pixels under them erased, so the sky
    shows through. The hall window's fifth pane (x 799-817, y 151-172)
    is broken: the jagged hole `(y − 160) > (x − 799) × 0.9 + 2 sin(1.7
    x)` erased from the glass, ringed by the new element `glass-rim`
    (the hole's 3 × 3 neighbours within the glass, `(215, 225, 240)`,
    opacity 82). Since 2026-10-26 the second and sixth panes are broken
    too (the maintainer's ask): OLX's darker shade there (`(100, 165,
    232)` in `glass`, x 739-757 and 819-838) erased, ringed alike (534
    pixels open, 55 of rim). A moon in `editor/images/sky.png` at (645.5, 184): in
    the hall window's middle for a camera at (760, 215). Made by a
    one-off script of the session (numpy), then `level bake`.
  - **Screens, PCs and headlights** (Liero Factory, 2026-10-25, the
    directional-glow design, `docs/superpowers/specs/2026-10-25-directional-glow-design.md`).
    The glass's frost is 1.0 px (`frost: 10`, `glass` and `glass-rim`).
    The control room's lower consoles shine further: `screens` 450,
    reach 48. Its upper screen (the `screens` pixels with y < 110 and x
    880-950) has its own Background layer `ctrl-scr` (glow 90,
    group `control`, `Tint("#d0e0ff")`, 450, reach 48) and lights all
    around, blue-white. The offices' PC screens (the `rock` pixels of the
    four screen colours at x 523-533 × y 209-213, x 344, 498 and 513 × y
    259-263) left `rock-scr` for three Rock layers in group `pcs`
    (own colour, 1000, reach 56):
    - `rock-pcs`: the blue floor's PC, which faces the player, glow 100,
      undirected;
    - `pcs-right`: the PCs at x 344 and 513, glow 15, `--glow-beam
      0,35`;
    - `pcs-left`: the PC at x 498, glow 15, `--glow-beam 180,35`.

    The glow of 15 makes their beams 0.12 of their colour, the strength
    the maintainer picked: `BEAM_SCALE` 0.08 × the group's cast (10 at
    intensity 1000) × glow 0.15 = 0.12. Their screens are duller than at 100, which
    the maintainer accepted. The truck's headlights (`rock` 42-44 × 410-414,
    yellow) are `headlights`, group `headlights` (`Tint("#fff0c8")`,
    1000, reach 120), glow 100, `--glow-beam 166,15`. The look's haze is
    50 (`level look --haze 50`), and there is no haze map: the beams
    show in the air and over the sky. Made with `level element
    add-layer`/`set`, `level group add`/`set` and `level look`; the layer
    pixels were moved by a one-off script of the session (PIL), then
    `level bake`.

  ```sh
  cargo build -p olr-cli
  python3 -I tools/levels/reloaded_skies.py target/debug/olr-cli
  ```

  A level already holding a distant layer is left alone; the
  generated images are in `target/reloaded-skies/`.
- **Surface materials** (issue #46; `docs/formats/material.md`, the
  spec `docs/superpowers/specs/2026-10-22-materials-and-heat-design.md`):
  both levels hold copies of the built-in materials they use, so
  explosions leave them hot; their collision material, `front.png` and
  `back.png` are unchanged (and so every match). Each element has a
  default, then a rule paints its pixels by collision kind and art
  colour (`tools/levels/reloaded_materials.py`, its `factory` and
  `castle`), one exact-colour `olr-cli level element surface --rule`
  per colour unlike the default:
  - Liero Factory After Dark (stone, concrete, metal, wood, dirt, glass):
    `rock` stone, `dirt` dirt, `backdrop` concrete, the
    `lamps`, `glass`, `glass-rim`, `screens`, `ctrl-scr` and `displays`
    layers and the glowing rock ones (`rock-scr`, `rock-pcs`,
    `pcs-right`, `pcs-left`, `headlights`, `rock-disp`) glass, the
    `beacons` and `rock-bcn` metal. The rule: in rock, pure
    grey (r = g = b) is the cave's stone, a grey tinted warm the
    building's concrete, a grey tinted blue or near white steel (metal),
    brown, olive and green wood (desks, crates), any other colour
    painted machinery (metal: the truck, the red machine and pipe, the
    teal device, the orange control room); in dirt, the tunnels'
    saturated orange-brown earth is dirt, the pure browns of the rack
    and the crates, the plants and the sofa wood, greys and gold metal;
    in the background, the cave's dark pure grey stone, the red pipe
    and truck metal, the sky none (erased since issue #63), the rest
    concrete. The distant sky (issue #63) has no material.
  - Castle Strike Reloaded (stone, brick, wood, metal): `rock` stone,
    `dirt` wood (barrels, logs, sacks, food), `background` stone (it
    shows only at the torches), `background-2` none (it holds the sky
    and the hills; the rule paints the rest). The rule: grey is stone,
    the sandstone walls' beige stone, browns wood (floors, beams,
    furniture, the hall's ceiling, barrels), the red roof tiles brick;
    in the dirt, greys are the tankards' metal and the rest wood; in the
    backgrounds, the sky, the water and the hills none.

  Made with (`--preview DIR` writes each ruled element's materials as a
  false-colour PNG instead; the built-in assets cut from these levels,
  `content/assets/`, carry their materials, so their commands were run
  again):

  ```sh
  cargo build -p olr-cli
  python3 -I tools/levels/reloaded_materials.py target/debug/olr-cli
  ```

- `olr-level-liero-factory-reloaded/`: **Liero Factory Reloaded**
  (issue #96), Liero Factory After Dark rebuilt from assets (the design
  `docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`,
  "Liero Factory"): its origin, size, theme, spawns, look and surface
  materials are After Dark's, and so is its bake outside its live
  copies (`material.png`, `front.png`, `back.png`, the distant sky and
  the `look/` files; `olr-cli`'s `tests/reloaded.rs`). Since issue #97
  the crane is live: every couple of seconds the crane's claw crosses
  its rail (25 px a second since issue #100), humming, carrying worms on
  its shoulders and pushing those in its way aside (it crushes nobody),
  and the terminal left of it (issue #100; the console under the deck
  before it) goes red, pulsing, while it moves.
  Since issue #98 three ceiling strips (`strip-2`, `strip-4`,
  `strip-7`) fail like faulty tubes, stuttering on and off in uneven
  bursts, sometimes dark or dim a while, buzzing and ticking; the other
  six stay steady. Since issue #99 the control room's screen is a
  security camera, the 13-37 sign, the control desk's lamps and the
  gauge change now and then, and an orange warning beacon under the
  crane's trolley turns while the crane moves. Laid out as #73
  and #74 lay a level out: `editor/placed.ron`, the ground at `editor/`,
  each other asset at `editor/<key>/`, and the baked files beside, so it
  plays without any library.

  | Key | Asset (origin) | Where (world pixels) | Copy ids |
  |---|---|---|---|
  | `ground` | the level's own: Ground (After Dark's `backdrop`, `dirt` and `rock` and its glowing, glass and beam layers no piece takes) | the whole level | `ground` |
  | `y` | the level's own: Night sky (After Dark's sky image), distant at 90 | the whole level | `sky` |
  | `s` | built-in `strip-light` | the nine two-row strips (29 × 2) at (778, 80), (1004, 80), (1119, 80), (984, 177), (1067, 177), (314, 286), (427, 286), (524, 286), (187, 342) | `strip-1` … `strip-9` |
  | `l` | built-in `ceiling-lamp` | (319, 137), 13 × 6 | `ceil-lamp` |
  | `c` | built-in `computer-screen` | (906, 83), 14 × 14 | `screen` |
  | `d` | built-in `control-desk` | (903, 119), 30 × 11 | `desk` |
  | `w` | built-in `warning-beacon` | the first roof speaker, (951, 45), 25 × 28 | `beacon` |
  | `k` | built-in `crane` | (762, 269), 114 × 50 (x 762-875, y 269-318) | `crane` |
  | `o` | built-in `console` | (933, 269), 33 × 64, the sign's rectangle empty | `console` |
  | `t` | built-in `lit-sign` | (934, 299), 31 × 21, above the console | `sign` |
  | `g` | built-in `gauge` | (1059, 144), 48 × 27 | `gauge` |

  Each copy is 1:1, its aspect kept, and the ground lost exactly its
  pixels (`level element erase --rect` on the layers the piece was cut
  from); the ground's `screens`, `ctrl-scr`, `rock-scr`, `displays` and
  `rock-disp` were left empty and removed. Stays in the ground, in After
  Dark's layers:
  - **the five one-row strips** (x 318, 444 and 537 at y 183; x 373 and
    498 at y 237): they show one row of the 29 × 2 Strip light (the
    second lies under rock), so the Strip light there would add pixels;
  - **the second roof speaker** (about x 987): it is drawn broken (a dark
    hole in its ring), not the Warning beacon's picture;
  - the windows and their broken panes (`glass`, `glass-rim`), the PCs
    (`rock-pcs`, `pcs-right`, `pcs-left`), the truck's `headlights`, the
    laser by the ramp (`rock-bcn`), and the brackets beside the gauge.

  Made with `tools/levels/factory_assets.py` (stdlib only), `olr-cli`
  commands only: the Crane, the Console and the Gauge by their blocks in
  `content/assets/README.md`, then `pack new level … --from <After Dark>
  --keep-origin`, `level notes`, the ground's `level element erase`s,
  the sky cut with `asset save --elements sky` (made near first) and
  placed with `level copy add … --key y --id sky`, `level copy set sky
  --distant 90`, and each built-in asset placed with `level copy add …
  --key <k> --id <id> --at X,Y` (a second strip reusing the first's
  asset). Since issue #97 it also paints the ground's `backdrop` behind
  the crane's claw (the pixels the ground lost in the crane's box, 2
  more around them, each blended along its row from its neighbours), so
  the moving claw leaves no picture of itself behind. Running it again
  makes the same bytes; its scratch files are in
  `target/factory-assets/`.

  Then `tools/levels/factory_live.py` (issue #97, stdlib only, `olr-cli`
  commands only) brings it alive: it paints the Console's two state
  pictures (`tools/levels/factory/art/`) from After Dark's `front.png`,
  makes the Crane and the Console again by their blocks in
  `content/assets/README.md` (states, exposed values, sound, script),
  and then `level library update <pack> k o`, `level copy set … crane
  --live` and `… console --live` (issue #99: `level library update
  <pack> k o s c t d g`, and `screen`, `sign`, `desk` and `gauge` Live
  too), `level script --set
  tools/levels/factory/level.rhai`, `level notes`, `level bake`, `level
  check-scripts` and `pack check`.
  - **The crane** (Live, its asset's default script
    `tools/levels/factory/crane.rhai`): `Idle`, it rests 200 ticks
    (`REST()`), then enters `Moving` and moves `claw_x` at speed 50
    (`SPEED()`, half a pixel a tick: 1.5 s from 0 to 75, 1.7 s across)
    to its maximum and its minimum in turn (`this.max`/`this.min`, never
    numbers); `arrived` sets `Idle` and rests again; `blocked` stays
    `Moving`, waits 50 ticks (`WAIT()`) and heads back. Its direction is
    `mem.up`. Constants are zero-argument functions (Rhai's top-level
    `const` is not seen in functions).
  - **The console** (Live, no script): the level script
    `tools/levels/factory/level.rhai` sets it to `Alarm` while the crane
    is `Moving` and back to `Normal` otherwise, only on a change (one
    tick after the crane, ruling R8).
  - **The hum** (`tools/levels/factory/sounds/hum.wav`, the Crane's
    `Moving` loop): synthesised for OLR by
    `tools/sounds/factory_sounds.py`, CC0; the maintainer may replace it.
  - **The faulty tubes** (issue #98; `factory_live.py` also paints the
    Strip light's `Dim` and `Off` pictures, makes it again by its block
    and runs `level library update <pack> s` and `level copy set …
    strip-2 --live`, `strip-4` and `strip-7`): Live copies of the Strip
    light, whose default script `tools/levels/factory/tube.rhai` (its
    constants at the top, all ticks) stays `On` 300-1200 ticks, then a
    burst of 2-6 flickers (`Off` 3-10 ticks, `On` 2-15); after the burst,
    of 12 equal draws 3 (1 in 4) stay `Off` 100-400 ticks and then a
    recovering burst, 2 (1 in 6) glow `Dim` 200-600, and the rest go back
    to `On`. Its step and counts are in `mem`; every draw is `this.rand`,
    the scripts' one generator drawn in run order, so the three never
    flicker together and the same match seed flickers alike. `On` sets
    nothing (entering it restores `lamps.glow` to 100 and the picture,
    ruling R2), `Dim` the tube at 60 % and `lamps.glow` 35, `Off` the tube
    unlit and `lamps.glow` 0. The six static strips bake in `On` and never
    flicker or sound (a static copy's states, scripts and sounds do
    nothing). The ground's `backdrop` still holds After Dark's lit strip
    under each live one (the flattened art); every state's picture covers
    it whole.
  - **The security screen** (issue #99; `screen`, Live, the Computer
    screen's default script `tools/levels/factory/screen.rhai`): states
    `Truck` (start), `Crane`, `Offices` and `Garden`, 250 ticks each in
    that order, again and again, no randomness. The pictures
    (`tools/levels/factory/art/screen-*.png`, 12 × 11) are After Dark's
    parts drawn full bright (`olr-cli look-preview --view full`: the claw
    under the deck at (800, 300), the lab and the teal office at (500,
    230), the roof garden under the moon at (500, 160)), each box
    downsampled to 12 × 11 (a box's most saturated colour where it
    covers a quarter of it, saturation × 1.6), mapped to the truck
    picture's colours. **Hand touch-up** (in code, `GARDEN_*`): the
    garden's sky cleared over rows 0-7, the moon a white pixel at (10,
    1), four green stalks under gold heads at x 1, 3, 5 and 7.
  - **The displays** (issue #99, each its asset's default script):
    `sign` (Lit sign, `sign.rhai`) stays `Normal` 300-900 ticks, then
    shows `No dash`, `No LD` or `Dying 7` (equal odds) 20-80 ticks; `desk`
    (Control desk, `desk.rhai`) and `gauge` (Gauge, `gauge.rhai`) change
    to one of their two other states (`A`, `B`, `C`) every 100-400 and
    150-500 ticks. Every draw is `this.rand`; no state changes a solid
    pixel. The beacons on the roof stay static (their glow already
    pulses).
  - **The warning beacon** (issue #99, #95's pick): two deep orange
    beams (`#ff6a10`, intensity 400, reach 360, half-angle 10°), level,
    turning once every 3 s, the lamp flaring as they face the viewer;
    under the crane's trolley, astride the cable, lit and turning only
    while the crane is `Moving`. Built into the Crane (its block in
    `content/assets/README.md`); `crane.rhai`'s `tick()` keeps the two
    lens layers at the claw's offset and `turn()` sets their spread,
    reach and light from a table of the half turn (26 entries, every 3
    ticks, interpolated in integers), the halo's glow `(400 − light) /
    4` and light `400 − light`.
  - **The buzz and the tick** (`tools/levels/factory/sounds/buzz.wav`,
    looped while a tube is `On` or `Dim`; `tick.wav`, the starter,
    played on each entry to `On`): synthesised for OLR by
    `tools/sounds/factory_sounds.py`, CC0; the maintainer may replace
    them.
  - **The night shift** (2026-11-03, drawn as real frames with the
    maintainer and kept; look only, `material.png` as before): light
    where After Dark had none and colour by area. `factory_assets.py`
    adds, on the ground while it is alone, each in a group of its own:
    9 mine lamps on the tunnels' back wall (a `#28282e` plate round a
    2 × 2 `#ffd890` bulb, only the bulb glowing, `level element glow
    --mask`; group `mine`, `#ffb060`, 1000, reach 110, a slow flicker;
    every lamp pixel at least 3 px from rock and dirt, none hanging from
    dirt), the lab floor's three one-row strips moved out of `lamps`
    (`level element extract`) into group `cool` (`#dce8ff`, 500, 90),
    two high-bay faces on the deck's lowest rock rows over the lower hall
    (x 895 and 1010, a Rock layer; group `sodium`, `#ffb050`, 520, 95),
    the red duct's elbow (its saturated red rock pixels, extracted)
    breathing at 59 % in group `flue` (`#ff5a20`, 320, 70, a 4.2 s
    pulse), and a red aviation light on the chimney (1082, 36; group
    `aviation`, `#ff2a14`, 400, 22, a 1.5 s blink). Then the skyline
    (`tools/levels/factory_skyline.py`, seeded): the far city (towers,
    masts, water towers, lit floors; its windows and mast tips glowing
    in `city`, `Own`, 120, 8, a slow flicker) and the near industry
    (sheds, chimneys, a cooling tower, pylons and their wires; the red
    tops glowing in `pylon`, `#ff3020`, 200, 10, a 2 s blink), each a
    level-sized image made the level's own asset and placed after the
    sky at depth 75 and 45. `factory_live.py` sets the Strip light's
    exposed `colour` per copy: sodium (`#ffb050`) on `strip-6` to
    `strip-9` (the warehouse and the dock), cool white (`#dce8ff`) on
    `strip-1` to `strip-3` (the upper hall's top); a static copy of
    another colour bakes in a group of its own (`lamps-2`, …). Ruled
    out: beams from the big window, glinting dirt, glow on the bonus-box
    stock, ambient occlusion.
  - **The plant works** (2026-11-04, the maintainer's go on the "Factory
    Pass Mockups" page; `tools/levels/factory_works.py`, the new assets'
    entries in `content/assets/README.md`, a playtest still to tune
    each). The lower hall's **Dirt-ball line** in the Console's place:
    the stamp under the 13-37 sign counts down on its LED column, drops
    its die 18 px onto the belt (crushing a worm under it), thumps and
    rises, and a dirt ball rides the belt, through the drive and up the
    incline into the crate (the balls a Background layer lit at least at
    60 % by a group that casts nothing, as front dirt is by the solid
    floor; After Dark's painted balls on the incline erased); the belt's red stripe is the line's status
    light, red while it stands, white while it runs, the drive's lamp
    green. The **shift** (the level script): 15 s standing, the horn,
    30 s running. A **lift** in a new shaft at the hall's far right (x
    1186-1197, right of the ramp's mouth) between the hall floor and a
    landing in the mine's big chamber; a **mine cart** on new rails along
    the left tunnel, its lamp ahead lit; a **forklift** at the
    warehouse's right end backing away from the crate stack and driving
    up to it again, beeping, its beacon turning and a work light on its
    mast (the doorway's threshold plate behind it made flush with the
    floor, its art on the backdrop, so its wheels roll back); the **generator** (the gauge's mounting: status lamps and a
    vent, health 40): wrecked, it **blacks the plant out** (the lamps'
    groups `lamps`, `cool`, `sodium` and `mine` at 0, every strip Dark,
    the line, the crane, the lift and the cart stopped, the red
    **emergency lamps** lit) until it is repaired 30 s later. The
    **alarm**: a blast of 5 damage or more (Classic's heaviest: its
    bullets do 1), or three kills within 10 s,
    sounds the roof's **siren** (the two speaker rings, two turning
    beams each, a klaxon) for 10 s and tints the lamps red, then rests
    20 s before it may sound again. Every strip is live: health 20,
    **Broken** when shot (a pop), a blast of 2 or more within 40 px of it
    flickers it (the level script sets it Off: one handler for every
    blast, not one a strip), and maintenance mends one every 20 s; the three faulty
    tubes keep their stutter and buzz, the others run `steady.rhai` in
    `Steady` (lit, silent). Three panes of
    the big window **shatter** (health 5, a smash) and show the sky.
    The lab's flask glows green and bubbles over a blue burner; the
    truck's hazard lights blink (a Rock layer: the truck stays solid) and its cab glows; the security screen
    cuts to the camera nearest each kill for 4 s. Sounds
    (`tools/sounds/factory_sounds.py`, CC0): `thump`, `rumble`,
    `horn`, `whine`, `cart`, `engine`, `beep`, `idle`, `bubble`,
    `klaxon`, `smash`, `pop`. Ruled out on the way: the mockups' work
    lamp over the bonus boxes (the strip above them already lights
    them: two lamps side by side), the mockups' lift shaft right of the
    crates (it would have cut through the ramp beside it), the displays
    going dark in a blackout (the control room on its battery), the
    truck's idling (all match long at a parked truck), and the painted
    particles (no particles in the engine yet).

  ```sh
  cargo build -p olr-cli
  python3 -I tools/levels/factory_assets.py target/debug/olr-cli
  python3 -I tools/sounds/factory_sounds.py
  python3 -I tools/levels/factory_live.py target/debug/olr-cli
  ```

- `olr-skin-firefly/`: **Firefly** (issue #13), a double-resolution skin
  (art scale 4, 40 × 36 frames) for judging double resolution and the
  skin light in play. OLX's default skin (`origin`:
  `share/gamedir/skins/default.png` at `d1ad953f`) doubled by Scale2x,
  then refined by `tools/skins/firefly.py` editing its PNGs: the body
  shaded at the finer resolution (lit from the top left, a dark rim
  below, the gun's shadow on it), rings along the body, white eyes whose
  pupil looks where the worm aims, the gun's metal shaded, and the tail
  tip a firefly's lantern, pale yellow-green, not tinted, glowing
  (`glow.png`: 255 at its core, 192 at its edge) with a light of 80 %
  over 24 world pixels (`skin.ron`; the caps are 200 % and 48). Its
  outline is the doubled skin's (tested). Made with:

  ```sh
  cargo build -p olr-cli
  target/debug/olr-cli skin double olx-skin-default --out content/reloaded/olr-skin-firefly --name Firefly --id olr-skin-firefly
  python3 -I tools/skins/firefly.py content/reloaded/olr-skin-firefly content/packs/olx-skin-default
  target/debug/olr-cli pack fmt content/reloaded/olr-skin-firefly
  ```
