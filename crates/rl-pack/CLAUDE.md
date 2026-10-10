# rl-pack

The native pack format (spec §9.1): `pack.ron` plus kind-specific files, for
mod, level, skin and theme packs; PNG helpers; content hashing; validation.

- **Layer 2.** Depends on `rl-data`, `rl-vfs`, `ron`, `png`, `sha2`.
- **Deterministic output.** Writing the same value twice gives byte-identical
  files. Never introduce `HashMap` iteration or timestamps into output.
- **Layouts:**
  - mod: `mod.ron`, `weapons/<id>.ron`, `projectiles/<name>.ron`, assets (`gfx/`, `sfx/`)
  - level: `level.ron`, `material.png` (8-bit palette; index = `Material`), `front.png`, `back.png`
  - skin: `skin.ron`, `skin.png`, `mask.png` (palette: 0 keep, 1 colorize, 2 transparent)
  - theme: `theme.ron`, `holes/N.png`, `stones/N.png`, `misc/N.png`, `front_tile.png`, `back_tile.png`
- `validate` never panics and reports `file:line:col` for RON errors.
  Errors: unknown references, cycles, missing assets. Warnings: unused items.
- Bump `PACK_FORMAT_VERSION` (in `rl-data`) when a layout changes, and add a migration here.

Test: `cargo nextest run -p rl-pack`.
