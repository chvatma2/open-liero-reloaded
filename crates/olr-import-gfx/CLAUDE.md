# olr-import-gfx

Imports OLX worm skins, level themes and the core pack, and makes level
thumbnails.

- **Layer 2.**
- **Skins** follow `CGameSkin::Colorize_Execute` (OLX `src/client/CGameSkin.cpp:720-790`):
  - the top 18 rows are the image and the next 18 rows the mask;
  - a black mask pixel keeps the pixel, magenta hides it, any other color tints it;
  - magenta or alpha-0 image pixels are always transparent;
  - only each frame's 20-pixel worm window (from `frame_spacing`) is kept: the rest of the
    32-pixel pitch is hidden, so `read_skin(write_skin(s)) == s`;
  - sheets larger than 672×36 are cropped with a warning.
- **Themes** follow `CMap::LoadTheme` (OLX `src/common/CMap.cpp:308-370`):
  5 holes, plus `NumStones` and `NumMisc` from `theme.txt`, plus the tiles,
  plus the shared green-dirt mask `data/gfx/greenball.png` (a warning if missing).
  Stamp colors are kept raw, because their colors carry meaning when carving.
- **Core pack** `olx-core` (spec §19 E9, `import_core`): the files that
  `content/olx-manifest.ron` lists under `core`. Each `.png` becomes a sprite
  named by its lowercase stem and colour-keyed once here (`olx_sprite_to_alpha`: OLX's
  magenta, or `(254,0,254)` by the corner rule, becomes transparent; nothing keys again
  at load, Review Focus 4: `a_254_keyed_sprite_keeps_its_real_magenta`), each `.ogg` a game
  sound kept as bytes, and `colours.cfg` the `[Colours]` values (`StrToCol`'s
  `#rrggbb` and `r,g,b` forms). A missing or broken file is a warning.

- **Image files** (`lib.rs` `decode`, `bmp.rs`): read by their bytes, as
  OLX's `IMG_Load`: a `.png` that is really a Windows bitmap (OLX 0.58's
  release ships two such skins) is decoded as one (uncompressed 1/4/8/24/32
  bits; compressed is an error), within `olr_pack::image_io::IMAGES`.

- **Level thumbnails** (`thumb.rs`, plan 7 G6): `level_thumbnail(&Level)`
  is `olr_format::levels::thumbnail` of the level's front (moved there in
  plan 13d, so the level operations make them too; `letterbox` is
  re-exported). It box-filters the `front` art (at most 8 × 8 samples a pixel) into
  `olr_pack::THUMB_SIZE` (160 × 96), `letterbox`ed with transparent bars so
  it keeps its aspect. OLX's menu preview is the minimap stretched into
  128 × 96 (`minimapForLevel`, `src/game/Level.cpp:33-44`): a deviation in
  `docs/porting/STATUS.md`. `olr-cli import` sets it on every level pack.

Test: `cargo nextest run -p olr-import-gfx` (all 409 skins when `OLR_OLX_GAMEDIR` is set;
`thumbnails_are_imported_for_every_level` when `OLR_CONTENT_DIR` is).
