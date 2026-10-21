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
  `WeaponRules::from_loadout(mod name, weapons, &MatchLoadout)` (issue #5:
  the loadout's weapons enabled, the rest banned, names stored trimmed)
  and `unique()` (a loadout's unique rule; never for a preset).
  `ShippedLoadout { loading_time, unique }` (`mod.ron`'s `loadouts`, keyed
  by preset name; here so the format docs read it) is
  `ModDef::loadouts`.
- **Distant layers** (`level.rs`, issue #11, not OLX's): `DistantLayer
  { image, at, depth }`, a level's parallax decoration (art resolution,
  binary alpha; `at` the top-left in world pixels with the camera at the
  level's centre; `depth` percent `MIN_DEPTH` 1 to `MAX_DEPTH` 99).
  `position(camera, size)` is `at + depth × (camera − size / 2)`, exact
  (a camera that is not finite counts as the centre); the renderer
  rounds it to its texels. No simulation reads them.
- **Loadouts** (`loadouts.rs`, issue #5, not OLX's): `MatchLoadout { name,
  weapons, loading_time, unique }` (weapons by the mod's names; empty means
  every weapon: `Default`, `DEFAULT_LOADOUT`, `MatchLoadout::default()`,
  100 %, unique off), `shipped` (a preset's enabled weapons, `None` when it
  enables none: `Bonus only`), `mod_loadouts` (Default first, then the
  shipped ones by name), `enabled`, `fit` (to a mod's weapons: unknown
  names dropped, the mod's spelling and order, loading time clamped into
  `LOADING_TIME_RANGE` 0-500, the name `clean_name`d (at most
  `MAX_LOADOUT_NAME` 40 characters); one warning each), `unique_ok(slots,
  enabled)` (no repeat unless every enabled weapon is in a slot).
- **Names** (`text.rs`): `valid_name`, `is_hidden`, `sanitise_name`, the
  one rule for players' names: olr-proto refuses lobby names that break it,
  olr-replay cleans recorded ones with it.

Test: `cargo nextest run -p olr-data`. Build for the web: `cargo build -p olr-data --target wasm32-unknown-unknown`.
