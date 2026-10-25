# olr-pack

The native pack format (spec §9.1): `pack.ron` plus kind-specific files, for
mod, level, skin, theme and core packs; PNG helpers (`image_io`, re-exported from `olr-format`; `png_size` reads a
header's size); content hashing; validation.

- **Layer 2.** Depends on `olr-data`, `olr-format` (`image_io` is
  re-exported from it), `olr-vfs`, `ron`, `png`, `sha2`.
- **Install names** (`install.rs`): `TMP_PREFIX`, `OLD_PREFIX` and `newest_aside_first`, shared by `olr-import` and `olr-content`'s `open_layers`.
  `replace_dir(dir, write)` (plan 13d, moved from `olr-import`) is the
  whole-directory swap the importer and `olr_edit::pack::save` use:
  `write` fills a fresh `<parent>/.import-<name>`, the old directory moves
  to `.replaced-<name>` (`~k` when taken, at most 100), the new one is
  renamed into place and the old one removed (a failure to remove it is a
  warning: "the next save or import of this pack tries again"). A failed write or rename removes the hidden copy and leaves
  `dir` the old directory (renamed back).
- **Deterministic output.** Writing the same value twice gives byte-identical
  files. Never introduce `HashMap` iteration or timestamps into output.
- **Layouts:**
  - level (format 5, `olr-format`'s `levels`): `level.ron`, `material.png`
    (8-bit palette; index = `Material`), `front.png`, `back.png`, optional
    `thumb.png` (`THUMB_FILE`, `LevelPack::thumb`: the menus' thumbnail, made
    at import at `THUMB_SIZE` 160 × 96; read bounded by `MAX_THUMB_SIDE` and
    `MAX_THUMB_BYTES` (defined in `olr-format`, re-exported here) from the
    PNG header before decoding: `read_level_thumb`, `decode_thumb`;
    `read_level` leaves it out; a bad one is a warning of `validate`)
  - skin (format 5, `olr-format`'s `skins`): `skin.ron`, `skin.png` (a
    140x54 grid, or 280x108 for a double-resolution skin, issue #12),
    `tint.png` (0 keep, 255 colour), optional `glow.png`; compiled to the
    672x18 OLX sheet or its 1344x36 double
  - theme (format 5, `olr-format`'s `themes`): `theme.ron`, `holes/N.png` +
    `holes/N.mask.png`, `stones/N.png`, `misc/N.png`, `front_tile.png`, `back_tile.png`, optional `green_ball.png` + `green_ball.mask.png`
  - mod (format 5, `olr-format`'s `mods`): `mod.ron`, `weapons/<id>.ron`, `projectiles/<id>.ron`
    (shared projectiles), `restrictions.ron`, assets (`gfx/` alpha PNGs, keyed once at import; `sfx/`)
  - core (format 5, `olr-format`'s `core_pack`; OLX's shared game assets, spec §19 E9): `core.ron`
    (per sprite frames and art scale, sound list, `colours.cfg` values), `gfx/<sprite>.png`
    (alpha; `giblets` and `giblet-stains` are two files that `read_core` composes), `sounds/<file>`
- **Distant layers** (issue #11): `LevelPack::distant` (`read_level`
  fills it through the lenient reader, `write_level` writes
  `distant/<i>.png` and `level.ron`'s list); they are content: in
  `content_hash`, so a replay or download carries them
  (`distant_layers_roundtrip_and_are_content`).
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
- **Authoring files** (`is_authoring`, plan 14a ruling R2): a path whose
  first component is `editor` in any case (a level's composition). `content_hash` and
  `archive::encode` leave them out, `archive::decode` refuses one
  (`BadPath`), so players download only the bake. `validate` on a level
  reads its composition (`read_composition_files`, `check_composition`):
  an error is a diagnostic; the files `composition.ron` names count as
  read (`read_composition`, so a missing one is one error), any other
  `editor/` file is "not read by the format". A mod's `editor/mod.ron`
  (issue #17, the mod editor's file) counts as read; one that does not
  parse as `ModEditorFile` is a warning.
- **Surface materials** (issue #37): a level's `look/surface.png` and
  `look/surface-back.png` are format files (`file_paths`) and content
  (hashed), checked by the strict reader (size, values within the look's
  materials); `validate` checks a composition's surfaces and painted maps
  (`check_surfaces`, `check_surface_maps`; `surface_maps_are_validated`).
  Issue #39: `read_level` keeps the maps in `LevelPack::surface` for the
  renderer's heat; `write_level` writes none (the format's writer does).
  Issue #41: `LevelPack::distant_surface` is each distant layer's material
  as a value in the look's (`olr_format::levels::distant_surface_values`),
  for the renderer's heat on distant layers; not written either.
  The directional-glow design: `LevelPack::glow_beams` and `haze` keep the
  lenient read's beam and haze maps for the renderer; not written either.
- **Presentation-only files** (`PRESENTATION_FILES`: `thumb.png`) are left out of
  `content_hash` like `pack.ron` (plan 7 ruling Q4): adding or changing a thumbnail
  keeps every replay of the pack valid (`hash::thumbnails_are_not_content`).
- Menus list packs without loading them: `read_level_def` (manifest and
  `level.ron`, no image) and `read_mod_def` (a mod's definitions: weapons,
  presets, shipped loadouts; no asset read).
- `read_core` is strict (what `validate` reports); `read_core_tolerant`
  leaves out each undecodable sprite or missing sound file and returns its
  error, for loaders that put a placeholder in its place.
- **Glow masks** (issue #19, `olr_format::mods::image_of_mask`): `validate`
  checks a mod's `gfx/<name>.glow.png` against its image: none, or a mask
  of another size (`png_size`) or that does not decode as greyscale
  (`decode_glow_mask`), is an error on the mask; a mask whose image is
  used is not "asset is never used" (`glow_masks_go_with_their_images`).
- **Sounds** (`sounds`, moved from `olr-content` in issue #19):
  `check_sound` (at most `MAX_SOUND_BYTES` 2 MiB and `MAX_SOUND_SECONDS`
  60 s by the header) and `is_sound` (`OggS` or `RIFF`…`WAVE`), for the
  game's loads and the editors' imports alike.
- `validate` never panics and reports `file:line:col` for RON errors.
  Errors: unknown references, cycles, missing assets. Warnings: unused items;
  `<path>: not read by the format` for each file outside the kind's
  format files (`olr-format`'s `file_paths` of the source read, plus
  `pack.ron`, a level's `thumb.png` and a mod's `restrictions.ron`; for a mod, files outside `gfx/` and `sfx/` that
  nothing names, plan 13d); `level.ron: objects[i]: spawn at (x, y) is
  outside the level (W×H)`.
- Bump `PACK_FORMAT_VERSION` (in `olr-data`) when a layout changes. Packs of any other
  version are refused ("re-import the pack"; `cargo xtask check` re-imports); there
  are no migrations while all packs are imports in `content/packs` (until packs are
  published). Version 2 added the theme's `green_ball.png`, which a version-1 import
  may lack although OLX has it; version 3 the mod's `restrictions.ron`; version 4 the
  level's `thumb.png` (plan 7 G6); version 5 every pack kind's native authoring format
  (plan 13c). `read_*`/`write_*` are built over `olr-format`'s `read_*_files`/`compile_*`/`lift_*`/
  `write_*_files`. Colour keying happens once, in the importers: mod gfx and core sprites are alpha
  images, and no loader keys them (a second keying would clear real magenta of a `(254,0,254)`-keyed
  sprite).

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
