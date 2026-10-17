# olr-pack

The native pack format (spec §9.1): `pack.ron` plus kind-specific files, for
mod, level, skin, theme and core packs; PNG helpers (`image_io`, re-exported from `olr-format`; `png_size` reads a
header's size); content hashing; validation.

- **Layer 2.** Depends on `olr-data`, `olr-format` (`image_io` is
  re-exported from it), `olr-vfs`, `ron`, `png`, `sha2`.
- **Install names** (`install.rs`): `TMP_PREFIX`, `OLD_PREFIX` and `newest_aside_first`, shared by `olr-import` and `olr-content`'s `open_layers`.
- **Deterministic output.** Writing the same value twice gives byte-identical
  files. Never introduce `HashMap` iteration or timestamps into output.
- **Layouts:**
  - mod: `mod.ron`, `weapons/<id>.ron`, `projectiles/<name>.ron`, `restrictions.ron`
    (the `.wps` presets, written only when the mod has any), assets (`gfx/`, `sfx/`)
  - level (format 5, `olr-format`'s `levels`): `level.ron`, `material.png` (8-bit palette; index = `Material`), `front.png`, `back.png`,
    optional `thumb.png` (`THUMB_FILE`, `LevelPack::thumb`: the menus' thumbnail, made at
    import at `THUMB_SIZE` 160 × 96; read bounded by `MAX_THUMB_SIDE` and `MAX_THUMB_BYTES`
    from the PNG header before decoding: `read_level_thumb`, `decode_thumb`; `read_level`
    leaves it out; a bad one is a warning of `validate`)
  - skin: `skin.ron`, `skin.png`, `mask.png` (palette: 0 keep, 1 colorize, 2 transparent)
  - theme (format 5, `olr-format`'s `themes`): `theme.ron`, `holes/N.png` + `holes/N.mask.png`, `stones/N.png`,
    `misc/N.png`, `front_tile.png`, `back_tile.png`, optional `green_ball.png` + `green_ball.mask.png`
  - core (OLX's shared game assets, spec §19 E9): `core.ron` (sprite and sound
    lists, `colours.cfg` values), `gfx/<sprite>.png`, `sounds/<file>` (raw Ogg bytes)
- **Bounded reads:** every pack file is read with `read_max`: at most
  `MAX_FILE_BYTES` (64 MiB; OLX's largest level art is under 1 MB encoded,
  and `png` decodes at most 64 MiB), RON at most `MAX_RON_BYTES` (4 MiB),
  `thumb.png` at most `MAX_THUMB_BYTES`. `read_level` does not read the
  thumbnail (`thumb: None`): matches and replays never pay for it.
- **Bounded decoding** (`image_io`): every PNG's size is checked from its
  header before anything is allocated (`ImageBound`): `decode_rgba` /
  `decode_indexed` within `IMAGES` (a side up to `MAX_IMAGE_SIDE` 16384,
  above OLX's 11563-wide `out_font.png`; at most 4096² pixels, 64 MiB),
  a level's art and material (`read_level`, an `.lxl`'s hi-res chunk)
  within `LEVEL_ART` (side 4096 × art scale 8, at most 64 Mi pixels,
  256 MiB, `olr-content`'s `MAX_COLOR_MAP_BYTES`). Decoding expands in
  place: 4 bytes a pixel at most. `declared_png` makes a bomb for tests.
- **Presentation-only files** (`PRESENTATION_FILES`: `thumb.png`) are left out of
  `content_hash` like `pack.ron` (plan 7 ruling Q4): adding or changing a thumbnail
  keeps every replay of the pack valid (`hash::thumbnails_are_not_content`).
- Menus list packs without loading them: `read_level_def` (manifest and
  `level.ron`, no image) and `read_preset_names` (a mod's restriction presets).
- `read_core` is strict (what `validate` reports); `read_core_tolerant`
  leaves out each undecodable sprite or missing sound file and returns its
  error, for loaders that put a placeholder in its place.
- `validate` never panics and reports `file:line:col` for RON errors.
  Errors: unknown references, cycles, missing assets. Warnings: unused items.
- Bump `PACK_FORMAT_VERSION` (in `olr-data`) when a layout changes. Packs of any other
  version are refused ("re-import the pack"; `cargo xtask check` re-imports); there
  are no migrations while all packs are imports in `content/packs` (until packs are
  published). Version 2 added the theme's `green_ball.png`, which a version-1 import
  may lack although OLX has it; version 3 the mod's `restrictions.ron`; version 4 the
  level's `thumb.png` (plan 7 G6); version 5 the level and theme packs' native authoring format
  (plan 13c; one constant, so skin, mod and core packs carry 5 in `pack.ron` with their format-4
  layout until their switch). `read_level`/`write_level`, `read_theme`/`write_theme` are built over
  `olr-format`'s `read_*_files`/`compile_*`/`lift_*`/`write_*_files`.

- **Archive** (`archive`, plan 11 N29): `encode(src)` / `decode(reader, sink, &Limits)`
  of the `.olpk` format (`OLPK1\n`, `u32` count, then per file sorted by path a
  `u16` path length, the path, a `u64` length and the bytes; no compression;
  deterministic). `decode` streams (one file in memory), refuses bad paths
  (backslash, `..`, absolute, a name starting with `.`, over `MAX_ARCHIVE_PATH`,
  a name `olr_vfs::non_portable` refuses, two paths or directories alike but
  for ASCII case, a file where a directory is; `encode` refuses the same),
  duplicates and disorder, more than `MAX_ARCHIVE_FILES`, and bytes after the last
  file; it never panics (proptest). `Limits { bytes, disk, files, depth }`
  (the caller's; `Limits::PACK`: `MAX_ARCHIVE`, `MAX_UNPACKED` 512 MiB,
  `MAX_ARCHIVE_FILES`, `MAX_DEPTH` 8) bounds the archive's bytes, files and
  path depth and the unpacked **disk charge** (review C2): `file_charge(len)`
  (rounded up to 4 KiB blocks, plus a block) per file and `DIR_CHARGE` (a
  block) per directory, charged before each file is written; directories
  are at most the file count and `MAX_DIRS` (512). `decode` returns
  `Unpacked { files, disk }`; `encode` refuses what `Limits::PACK` would.

Test: `cargo nextest run -p olr-pack`.
