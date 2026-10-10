# rl-import-gfx

Imports OLX worm skins and level themes.

- **Layer 2.**
- **Skins** follow `CGameSkin::Colorize_Execute` (OLX `src/client/CGameSkin.cpp:720-790`):
  - the top 18 rows are the image and the next 18 rows the mask;
  - a black mask pixel keeps the pixel, magenta hides it, any other color tints it;
  - magenta or alpha-0 image pixels are always transparent;
  - sheets larger than 672×36 are cropped with a warning.
- **Themes** follow `CMap::LoadTheme` (OLX `src/common/CMap.cpp:308-370`):
  5 holes, plus `NumStones` and `NumMisc` from `theme.txt`, plus the tiles.
  Stamp colors are kept raw, because their colors carry meaning when carving.

Test: `cargo nextest run -p rl-import-gfx` (all 409 skins when `RL_OLX_GAMEDIR` is set).
