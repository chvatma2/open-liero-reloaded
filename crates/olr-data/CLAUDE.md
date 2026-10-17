# olr-data

Content definitions shared by importers, packs and (later) the simulation:
mods, weapons, projectiles and their actions, levels with material grids,
skins with colorization masks, themes, and pack manifests.

- **Layer 1, `#![no_std]` + `alloc`.** No I/O, no `std`. Only dependency: `serde`.
- **Values mirror OLX exactly** as OLX holds them in memory after loading
  (for example `WeaponDef::rof` is already divided by 1000). Every type cites
  the OLX source it mirrors; keep those references when editing.
- **Floats must round-trip bit-exactly** through RON; a property test enforces it.
- Enums keep unknown OLX codes as `Other(i32)` instead of failing.
- **Weapon restrictions** (`restrictions.rs`): `ModDef::restrictions` holds a
  mod's `.wps` presets (name to `WeaponState`, as OLX's `CWpnRest` map holds
  them); `WeaponRules` is OLX's list for a match (`loadList`'s file-name rule,
  `updateList`, `findWeapon`'s trimmed case-insensitive lookup), with the
  enabled ids (bots), the not-banned ids (natural bonuses) and
  `find_enabled_weapon` (the weapon selection's fallback).
- **Names** (`text.rs`): `valid_name`, `is_hidden`, `sanitise_name`, the
  one rule for players' names: olr-proto refuses lobby names that break it,
  olr-replay cleans recorded ones with it.

Test: `cargo nextest run -p olr-data`. Build for the web: `cargo build -p olr-data --target wasm32-unknown-unknown`.
