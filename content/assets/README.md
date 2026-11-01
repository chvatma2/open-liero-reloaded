# The level editor's built-in assets

Ready-made pieces of a level (issue #9; the format is
`docs/formats/asset.md`) that the level editor's Assets tab offers under
**Built-in**, read-only, beside the user's own (`<data>/assets`). The
programs find this directory beside the content directory
(`olr_content::assets_dir`: `content/assets` beside `content/packs`);
`cargo xtask dist` copies it into the Windows zip and the Flatpak (this
README stays behind). `cargo xtask check` reads every one whole
(`olr-cli asset check`, in `olr-cli`'s `tests/asset.rs`).
They are in the 0.3 asset format (issue #71) and stateless: one state,
nothing exposed, no sounds, health or script. All glow but the Crane.

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
  strips, group `lamps`.

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 778,80,29,2 --layers lamps --name "Strip light" --out content/assets/strip-light

- `computer-screen/`: **Computer screen**, the control room's upper
  screen on its wall, group `control` (blue-white, `Tint("#d0e0ff")`;
  its own layer `ctrl-scr` since 2026-10-25).

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 906,83,14,14 --layers backdrop,ctrl-scr --name "Computer screen" --out content/assets/computer-screen

- `control-desk/`: **Control desk**, the control room's console: its
  three screens (group `screens`) and lamps (group `displays`).

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 903,119,30,11 --layers backdrop,screens,displays --name "Control desk" --out content/assets/control-desk

- `lit-sign/`: **Lit sign**, the "13-37 -LD-" board set in rock, group
  `displays` (`Flicker`).

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 934,299,31,21 --layers rock,rock-disp --name "Lit sign" --out content/assets/lit-sign

- `warning-beacon/`: **Warning beacon**, a roof speaker's red ring on its
  box, group `beacons` (`Pulse`).

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 951,45,25,28 --layers backdrop,beacons --name "Warning beacon" --out content/assets/warning-beacon

- `crane/`: **Crane** (issue #96), the conveyor deck's crane: its rail
  (`rail`, rows 269-271 of After Dark, x 765-872, with the trolley's
  pixels in those rows) and its trolley, cable and claw (`claw`, rows
  272-318, x 772-800), two rock layers that never overlap, cut from
  After Dark's `rock` with `level element extract`. Its box covers the
  claw's whole travel along the rail (issue #97 moves `claw`). It does
  not glow.

      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock --rect 762,272,114,47 --name Claw
      olr-cli level element extract content/reloaded/olr-level-liero-factory-after-dark rock --rect 762,269,114,3 --name Rail
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 762,269,114,50 --layers rail,claw --name Crane --out content/assets/crane

- `console/`: **Console** (issue #96), the box hanging under the conveyor
  deck (`rock`) with its column of yellow LEDs (`rock-disp`, group
  `displays`, x 942). The Lit sign's rectangle is left empty: the sign
  is a copy of its own on top of it in Liero Factory Reloaded.

      olr-cli level element erase content/reloaded/olr-level-liero-factory-after-dark rock --rect 934,299,31,21
      olr-cli level element set content/reloaded/olr-level-liero-factory-after-dark rock-disp --locked false
      olr-cli level element erase content/reloaded/olr-level-liero-factory-after-dark rock-disp --rect 934,299,31,21
      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 933,269,33,64 --layers rock,rock-disp --name Console --out content/assets/console

- `gauge/`: **Gauge** (issue #96), the teal device of the upper floor:
  its yellow gauge and lit buttons (`rock-disp`, group `displays`) and
  its small screen (`rock-scr`, group `screens`); the grey brackets
  beside it stay in the level.

      olr-cli asset save content/reloaded/olr-level-liero-factory-after-dark --rect 1059,144,48,27 --layers rock,rock-scr,rock-disp --name Gauge --out content/assets/gauge

## Editing an asset from the command line

`olr-cli asset state|expose|unexpose|sound|script|health` (issue #72)
edit an asset folder in place: the folder is read whole, changed by one
operation, checked and written anew with its thumbnail, or left as it
was with the reason (`<dir>: <why>`, status 1). `olr-cli asset info`
lists what a folder holds. Work on a copy: these built-in assets stay
stateless (the factory's get states of their own, issues #96-#99). For
a strip light that can be switched off, with a 29 × 2 PNG of its dark
tube:

    cp -r content/assets/strip-light my-strip-light
    olr-cli asset state add my-strip-light Off
    olr-cli asset state picture my-strip-light Off lamps --art dark-tube.png
    olr-cli asset state value my-strip-light Off lamps.glow 0
