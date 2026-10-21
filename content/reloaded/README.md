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
--content content/packs` and plays a bot match on each level). The game
refuses to edit them in place: copy one from the Editor menu.

Remade OLX content is named "<Old name> Reloaded".

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
