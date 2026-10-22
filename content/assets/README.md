# The level editor's built-in assets

Ready-made pieces of a level (issue #9; the format is
`docs/formats/asset.md`) that the level editor's Assets tab offers under
**Built-in**, read-only, beside the user's own (`<data>/assets`). The
programs find this directory beside the content directory
(`olr_content::assets_dir`: `content/assets` beside `content/packs`);
`cargo xtask dist` copies it into the Windows zip and the Flatpak (this
README stays behind). `cargo xtask check` reads every one whole
(`olr-cli asset check`, in `olr-cli`'s `tests/asset.rs`).

Each was cut from OLR's own Reloaded levels (`content/reloaded/`) with
`olr-cli asset save`; to make one again, run its command from the
repository's root. They carry the surface materials their pixels have
there (issue #46), so placed in a level they heat as they did in it.

- `torch/`: **Torch**, a cavern torch of Castle Strike Reloaded, its
  flame glowing in group `flames` (`Flicker`). Cut from the layers under
  the torch only: `background-2` holds no pixels there (the torch shows
  through its hole) and would carry the `sky` group.

      olr-cli asset save content/reloaded/olr-level-castlestrike-reloaded --rect 203,423,14,16 --layers background,dirt,rock --name Torch --out content/assets/torch

- `ceiling-lamp/`: **Ceiling lamp**, a round office lamp of Liero Factory
  Reloaded under three rows of its ceiling (rock), group `lamps`.

      olr-cli asset save content/reloaded/olr-level-liero-factory-reloaded --rect 319,137,13,6 --layers rock,lamps --name "Ceiling lamp" --out content/assets/ceiling-lamp

- `strip-light/`: **Strip light**, one of the factory's white ceiling
  strips, group `lamps`.

      olr-cli asset save content/reloaded/olr-level-liero-factory-reloaded --rect 778,80,29,2 --layers lamps --name "Strip light" --out content/assets/strip-light

- `computer-screen/`: **Computer screen**, the office computer on its
  wall, group `screens`.

      olr-cli asset save content/reloaded/olr-level-liero-factory-reloaded --rect 906,83,14,14 --layers background,screens --name "Computer screen" --out content/assets/computer-screen

- `control-desk/`: **Control desk**, the control room's console: its
  three screens (group `screens`) and lamps (group `displays`).

      olr-cli asset save content/reloaded/olr-level-liero-factory-reloaded --rect 903,119,30,11 --layers background,screens,displays --name "Control desk" --out content/assets/control-desk

- `lit-sign/`: **Lit sign**, the "13-37 -LD-" board set in rock, group
  `displays` (`Flicker`).

      olr-cli asset save content/reloaded/olr-level-liero-factory-reloaded --rect 934,299,31,21 --layers rock,displays-in-rock --name "Lit sign" --out content/assets/lit-sign

- `warning-beacon/`: **Warning beacon**, a roof speaker's red ring on its
  box, group `beacons` (`Pulse`).

      olr-cli asset save content/reloaded/olr-level-liero-factory-reloaded --rect 951,45,25,28 --layers background,beacons --name "Warning beacon" --out content/assets/warning-beacon
