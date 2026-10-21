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
the mod). The game
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
  the rest stay dark, and the engine's explosion light is unchanged:
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

- `olr-level-liero-factory-reloaded/`: **Liero Factory Reloaded** (issue
  #7). OLX's Liero Factory (`origin`: `Liero Factory.lxl` at `d1ad953f`)
  at night, its lights on; its gameplay and art unchanged: the imported
  level split into layers (`olr-cli level element split`), its
  `material.png`, `front.png` and `back.png` byte for byte the imported
  level's (tested). Above them, one drawn layer per glow group and
  material holds only that group's pixels, found by colour in the
  imported `front.png`: `sky` (the blue gradient, a pale moonlight tint
  of unlimited reach), `lamps` (the white ceiling strips of every floor,
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
