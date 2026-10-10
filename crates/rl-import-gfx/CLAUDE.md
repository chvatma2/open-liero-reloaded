# rl-import-gfx

Imports OLX worm skins, level themes and the core pack.

- **Layer 2.**
- **Skins** follow `CGameSkin::Colorize_Execute` (OLX `src/client/CGameSkin.cpp:720-790`):
  - the top 18 rows are the image and the next 18 rows the mask;
  - a black mask pixel keeps the pixel, magenta hides it, any other color tints it;
  - magenta or alpha-0 image pixels are always transparent;
  - sheets larger than 672×36 are cropped with a warning.
- **Themes** follow `CMap::LoadTheme` (OLX `src/common/CMap.cpp:308-370`):
  5 holes, plus `NumStones` and `NumMisc` from `theme.txt`, plus the tiles,
  plus the shared green-dirt mask `data/gfx/greenball.png` (a warning if missing).
  Stamp colors are kept raw, because their colors carry meaning when carving.
- **Core pack** `olx-core` (spec §19 E9, `import_core`): the files that
  `content/olx-manifest.ron` lists under `core`. Each `.png` becomes a sprite
  named by its lowercase stem (raw colors, like stamps), each `.ogg` a game
  sound kept as bytes, and `colours.cfg` the `[Colours]` values (`StrToCol`'s
  `#rrggbb` and `r,g,b` forms). A missing or broken file is a warning.

Test: `cargo nextest run -p rl-import-gfx` (all 409 skins when `RL_OLX_GAMEDIR` is set).
