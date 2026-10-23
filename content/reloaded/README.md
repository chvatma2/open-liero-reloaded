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

- `olr-level-liero-factory-reloaded/`: **Liero Factory Reloaded** (issue
  #7). OLX's Liero Factory (`origin`: `Liero Factory.lxl` at `d1ad953f`)
  at night, its lights on; its gameplay and art unchanged: the imported
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
    hills, walls and towers stay), Liero Factory's `background`; the
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
  - Liero Factory Reloaded (stone, concrete, metal, wood, dirt, glass):
    `rock` stone, `dirt` dirt, `background` concrete, the
    `lamps`, `glass`, `screens` and `displays` layers (and their
    `-in-rock` ones) glass, the `beacons` metal. The rule: in rock, pure
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
