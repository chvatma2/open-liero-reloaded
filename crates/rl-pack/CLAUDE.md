# rl-pack

The native pack format (spec §9.1): `pack.ron` plus kind-specific files, for
mod, level, skin, theme and core packs; PNG helpers (`image_io`, `png_size` reads a
header's size); content hashing; validation.

- **Layer 2.** Depends on `rl-data`, `rl-vfs`, `ron`, `png`, `sha2`.
- **Deterministic output.** Writing the same value twice gives byte-identical
  files. Never introduce `HashMap` iteration or timestamps into output.
- **Layouts:**
  - mod: `mod.ron`, `weapons/<id>.ron`, `projectiles/<name>.ron`, `restrictions.ron`
    (the `.wps` presets, written only when the mod has any), assets (`gfx/`, `sfx/`)
  - level: `level.ron`, `material.png` (8-bit palette; index = `Material`), `front.png`, `back.png`,
    optional `thumb.png` (`THUMB_FILE`, `LevelPack::thumb`: the menus' thumbnail, made at
    import at `THUMB_SIZE` 160 × 96; read bounded by `MAX_THUMB_SIDE` and `MAX_THUMB_BYTES`
    from the PNG header before decoding: `read_level_thumb`, `decode_thumb`; one that cannot
    be read is left out by `read_level` and a warning of `validate`)
  - skin: `skin.ron`, `skin.png`, `mask.png` (palette: 0 keep, 1 colorize, 2 transparent)
  - theme: `theme.ron`, `holes/N.png`, `stones/N.png`, `misc/N.png`, `front_tile.png`, `back_tile.png`,
    optional `green_ball.png`
  - core (OLX's shared game assets, spec §19 E9): `core.ron` (sprite and sound
    lists, `colours.cfg` values), `gfx/<sprite>.png`, `sounds/<file>` (raw Ogg bytes)
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
- Bump `PACK_FORMAT_VERSION` (in `rl-data`) when a layout changes. Packs of any other
  version are refused ("re-import the pack"; `cargo xtask check` re-imports); there
  are no migrations while all packs are imports in `target/content` (until packs are
  published). Version 2 added the theme's `green_ball.png`, which a version-1 import
  may lack although OLX has it; version 3 the mod's `restrictions.ron`; version 4 the
  level's `thumb.png` and the core pack's fonts and HUD images (plan 7 G6).

Test: `cargo nextest run -p rl-pack`.
