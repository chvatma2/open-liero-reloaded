# rl-content

Loads the packs in a content directory (`target/content`, written by
`rl-cli import`) into ready-to-run inputs: `Content::open(root)` lists the
packs and loads the core pack `olx-core` once; then `load_mod` (`SimMod` +
decoded images + sound bytes keyed by `SimMod::sounds()` names, `olx:` game
sounds from the core pack), `load_level`, `load_theme` (holes, green ball,
tiles; `stamps()`, `green_mask()`), `load_skin`/`skin_or_default`, `core()`
(sprites by name, game sounds, `colours.cfg`) and `new_world(&SandboxSetup)`.

- **Layer 4, uses `std`.** Depends on `rl-sim`, `rl-pack`, `rl-terrain`,
  `rl-data`, `rl-vfs`, `anyhow`. `rl-scenario` (same layer) uses it.
- **Never panic on content** (spec §19, Review Focus 3): a missing or odd
  asset gets a placeholder (`PLACEHOLDER` colour; core sprites at their
  `CORE_SPRITES` size, so frame arithmetic still works; a 672 × 18 skin;
  silence for sounds) and a warning. Warnings are returned
  (`Content::warnings`, `LoadedMod::warnings`, ...), never printed: the
  caller logs each once. A missing pack or unreadable definition is an `Err`.
  The core pack is read tolerantly (`rl_pack::read_core_tolerant`): an
  undecodable sprite or a missing sound file costs only that asset.
- **Hostile levels:** `load_level` refuses an art scale above
  `MAX_ART_SCALE` (8) or a colour image above `MAX_COLOR_MAP_BYTES`
  (256 MiB), so a pack cannot make the colour map allocate gigabytes.
- **Two mod loaders.** `sim_mod(id)` is strict, as OLX's client (what the
  oracle replays): an unreadable PNG or an image projectile without its
  frame count is an error (`check_frames`). `load_mod(id)` gives such a
  projectile a one-frame placeholder. On good content both give the same
  `SimMod` (`tests/content.rs`). Frame counts are `width / height` of the
  pack's PNGs (spec §18 D3).
- **Hostile sounds** (`sounds.rs`): a sound above `MAX_SOUND_BYTES` (2 MiB)
  or longer than `MAX_SOUND_SECONDS` (60 s) by its header (Ogg granule over
  rate, WAV data over byte rate) is silence with a warning, from a mod or
  the core pack, so `rl-audio` never decodes it (`check_sound`). A stream
  whose header lies is bounded only by the byte cap.
- **Weapon restrictions:** `LoadedMod` keeps the mod's `name` and
  `restrictions` (its presets); `LoadedMod::weapon_rules(file)` and
  `Content::weapon_rules(id, file)` give OLX's list for a match
  (`rl_data::restrictions::WeaponRules`). `SandboxSetup::restrictions` (OLX's
  `WeaponRestrictionsFile`, `DEFAULT_PRESET` = `Standard 100lt`) decides the
  weapons natural bonuses may hold (`MatchSetup::bonus_weapons`).
- **Worlds.** `new_world` loads the mod, level and theme and calls
  `new_world_from(&mod, &level, &theme, &setup)`, which a game restart
  calls with the assets it loaded once: `match_setup` (the `rl_sim::MatchSetup`,
  for a world of any game mode) played with `rl_sim::OracleMode`. Worms
  spawn in order; a worm without a position draws one from the world's
  `FindSpot` when it spawns, so the same setup gives the same world. Worms
  get the mod's first five weapons unless the spec names them. The clock
  starts at `SANDBOX_START` (`MatchSetup::T0`).
- **Colour terrain** (`color.rs`, plan 5 ruling R1): `LoadedTheme::art()`,
  `LoadedLevel::color_map(&theme)` (before any edit; spawn holes come with
  the first tick), `color_edit(&EventKind)` converts a `TerrainEdit`, and
  `apply_edits(map, events, terrain)` replays a tick's edits and rebuilds a
  `lost_area` (for callers without `rl-fx`, whose `Fx::step_tick` replays
  them itself between its entity passes: never both). `tests/color.rs` plays 3000-tick matches and checks every
  pixel's colour against the material (Review Focus 2).
- Pack ids of OLX imports: `olx_mod_id("MW 1.0")`, `olx_level_id("CastleStrike.lxl")`.

Test: `cargo nextest run -p rl-content` (`loads_every_manifest_item` and the
real-level part of `new_world_spawns_worms_on_free_spots` and `tests/color.rs`'s matches
need `RL_CONTENT_DIR`).
