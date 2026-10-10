# rl-import-lgs

Imports OLX `script.lgs` gamescript **version 7** mods into `ModPack`s.

- **Layer 2.** Ports `CGameScript::Load` / `LoadProjectile`
  (OLX `src/common/CGameScript.cpp:582-1152`) at commit `d1ad953f`.
- `parse.rs` follows the file byte by byte; `convert.rs` names projectiles,
  shares identical ones, and copies assets.
- **Asset lookup is OLX's:** `<mod>/gfx/` then `data/gfx/`; `<mod>/sfx/` then
  `data/sounds/`, case-insensitive, and sounds also try `.ogg`. A missing sound
  is dropped with a warning (OLX does the same); a missing image is an error.
- **Never panic on input.** Property tests feed corrupted files.
- Version 8 files are rejected (`UnsupportedVersion`); no mod in scope uses v8.

Test: `cargo nextest run -p rl-import-lgs`. Real mods are tested when
`RL_OLX_GAMEDIR` is set; `cargo xtask check` sets it.
