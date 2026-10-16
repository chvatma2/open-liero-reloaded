# olr-import-lxl

Imports OLX `.lxl` levels (image type) into `LevelPack`s.

- **Layer 2.** Ports `ML_LieroX` (OLX `src/common/MapLoader_LieroX.cpp`).
- **Material:** `Material::from_lx_flags`; rock beats dirt, anything else is background.
- **Front pixels** where `PX_EMPTY` is set are replaced by the back pixel, exactly as OLX does.
- **Art scale:** art is stored at 1×, because OLX's 2× is plain pixel doubling.
  An "OLX hi-res data" chunk gives 2× art instead.
- **Decompression is bounded** by width × height × 7 + 1 bytes.
- Pixmap levels (type 0) are rejected; none are in scope.

Test: `cargo nextest run -p olr-import-lxl` (real levels when `OLR_OLX_GAMEDIR` is set).
