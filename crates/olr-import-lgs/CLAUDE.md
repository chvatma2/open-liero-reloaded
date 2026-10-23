# olr-import-lgs

Imports OLX `script.lgs` gamescript **version 7** mods into `ModPack`s.

- **Layer 2.** Ports `CGameScript::Load` / `LoadProjectile`
  (OLX `src/common/CGameScript.cpp:582-1152`) at commit `d1ad953f`.
- `parse.rs` follows the file byte by byte; `convert.rs` names projectiles,
  shares identical ones, and copies assets.
- **Asset lookup is OLX's:** `<mod>/gfx/` then `data/gfx/`; `<mod>/sfx/` then
  `data/sounds/`, case-insensitive, and sounds also try `.ogg`. A missing sound
  is dropped with a warning (OLX does the same); a missing image is an error.
- **Mod images are keyed once, here** (`keyed_image`): a `gfx/` file is decoded, OLX's
  colour key (`olx_sprite_to_alpha`) made transparent and written as an alpha PNG; nothing
  keys at load. A file that does not decode is kept as it is, with a warning.
- **Nested projectile names** (issue #64, `intern`): a weapon's
  projectile is named after the weapon id, its children by
  `olr_data::mod_def::child_key` (`rocket.s`, `rocket.st`,
  `medical-camp.s3ts9t`), the format's derived keys, so lifted files
  carry no `id`; an identical definition reuses the first name, and a
  name taken by another definition gets `-2`.
- **Portable pack names** (plan 11 review C1): a weapon id that is a
  Windows device name gets `-2` (`con-2`), and an asset whose OLX file name
  is not portable (`olr_vfs::non_portable`) is not taken (a warning; a
  missing image is still the item's error).
- **Weapon restriction presets** (`wps.rs`, `CWpnRest::loadList`,
  `src/common/CWpnRest.cpp:183-213`): `import_mod` adds every
  `cfg/presets/<mod dir>/*.wps`, then every `cfg/presets/*.wps` of another
  name, as `ModDef::restrictions`. `parse_wps` reads lines like OLX (split at
  `,`, a state is `istringstream >> int` taken `MOD 3`, names kept as written,
  case-insensitive, last line wins).
- **Shipped loadouts** (issue #5, `wps::shipped_loadouts`): every preset
  that enables a weapon becomes a `ModDef::loadouts` entry at its
  `PRESET_LOADING_TIMES` time (`Standard 20lt`, `Rifle only`, `Mortar Only`
  20 %; any other 100 %), unique off; `Bonus only` enables none and is
  left out.
- **Never panic on input.** Property tests feed corrupted files.
- Version 8 files are rejected (`UnsupportedVersion`); no mod in scope uses v8.

Test: `cargo nextest run -p olr-import-lgs`. Real mods are tested when
`OLR_OLX_GAMEDIR` is set; `cargo xtask check` sets it.
