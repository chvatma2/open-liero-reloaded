# rl-content

Loads the packs in a content directory (`target/content`, written by
`rl-cli import`) into ready-to-run inputs: `Content::open(root)` lists the
packs (a directory named `.…` is skipped: an import's pack being written)
and loads the core pack `olx-core` once (`Content::open_layers(&[user
packs, bundled])`: layered, spec §9.4: an id in a higher layer hides the
same id below it, the core pack too; an unreadable higher pack hides
nothing; a missing root is skipped); then `load_mod` (`SimMod` +
decoded images + sound bytes keyed by `SimMod::sounds()` names, `olx:` game
sounds from the core pack), `load_level`, `load_theme` (holes, green ball,
tiles; `stamps()`, `green_mask()`), `load_skin`/`skin_or_default`, `core()`
(sprites by name, game sounds, `colours.cfg`) and `new_world(&SandboxSetup)`.

- **Layer 4, uses `std`.** Depends on `rl-sim`, `rl-modes`, `rl-bots`,
  `rl-replay`, `rl-pack`, `rl-terrain`, `rl-data`, `rl-vfs`, the
  importers (`rl-import-lgs`, `rl-import-lxl`, `rl-import-gfx`), `anyhow`,
  `serde` and `ron`.
  `rl-scenario` (same layer) uses it.
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
- **Replays** (`replay.rs`, spec §20 F8): `replay_packs(mod, level)` names
  a match's packs (the level's theme too) with their `rl_pack::content_hash`;
  `replay_assets`, `world_for_replay` and `play_replay` refuse a replay whose
  packs are missing or hash differently (Review Focus 3), then rebuild its
  world as `match_setup` does (the mod from `load_mod`).
- **Matches** (`matches.rs`, plan 6 ruling R5): `build_match(MatchAssets,
  &MatchOptions) -> BuiltMatch` (or `Content::build_match`) is the one
  assembly of a Deathmatch, used by `rl-cli match` and `rl-game --mode
  deathmatch`: `players[i]` is `Player::Human` (random enabled weapons:
  the command line's games), `Player::HumanWith(weapons)` (the weapon
  selection's choice, refused unless the restrictions enable each; the
  recording's setup holds it) or `Player::Bot(BotConfig)` (`choose_loadout`
  from `BotRng::new(seed, i, personality)`); chaos bots get the
  `ChaosCycle` respawn loadout; a bot whose `BotConfig::validate` fails is
  refused; `Deathmatch::prepare`, `World::new`, a
  started `Recorder` and the bots on one shared `AreaCache` (think in
  `bots` order). `rl-cli`'s golden checksum streams pin it.
- **Replays' content:** `check_replay_packs` (ids and content hashes) and
  `replay_assets_from(m, level, theme)` let a game that draws a replay load
  its packs once.
- **Catalog** (`catalog.rs`, plan 7 G1/G6): `Content::catalog()` lists every
  mod (`ModInfo`: name, restriction presets) and level (`LevelInfo`: name,
  size, `thumb.png` as a `ThumbRef`) without loading them, for the menus,
  and every pack's recorded origin (`Catalog::sources`, `PackSource`; the
  About screen's credits); `mod_info`/`level_info` for one. A thumbnail is not decoded here:
  `ThumbRef::probe` reads its length and PNG header (24 bytes) through the
  pack's `DirFs` (regular files only: a FIFO is never opened), and one
  above `MAX_THUMB_SIDE` (512) or `MAX_THUMB_BYTES` (1 MiB), or not a PNG,
  is left out with a warning (`Catalog::warnings`), like an unreadable
  pack; `ThumbRef::load` decodes it when a menu shows it, refusing a file
  that changed since (bounded read, header checked again before decoding).
- **Skins** (`skins.rs`, plan 7 task 3): OLX's frame layout
  (`FRAME_WIDTH`, `SKIN_WIDTH`, ..., `SHEET_FRAMES`), `skin_source(f,
  face)` (a frame's rectangle; facing left OLX shows the columns one to the
  right, mirrored), `LoadedSkin::colorized(colour)` and `skin_sprite(sheet,
  f, face)`: the one source the game (`rl-scene`) and the profiles' preview
  (`rl-ui`) draw skins from. `Content::skins()` and `Catalog::skins` list
  them by manifest name; `resolve_id(given, prefix, ids)` (an id, or an
  OLX name's import id such as `default.png` → `olx-skin-default`),
  `Content::skin_id`, `Catalog::skin`.
- **Import** (`import.rs`, plan 7 ruling Q3): the one OLX import
  pipeline, `rl-cli import`'s and the game's Import screen's.
  `import(olx, &ImportPlan, out, &ImportOptions, progress)` imports the
  plan's mods, levels (with their thumbnail), themes, core files and
  skins one by one (`Progress::Item`/`Done`; the callback returning
  `false` cancels between items), writes each pack hidden
  (`.import-<id>`) and renames it into place (the old one renamed aside
  to `.replaced-<id>`, then removed: a failure is that item's warning; a
  `.replaced-<id>` without its `<id>`, from an import killed between the
  renames, is listed by `open_layers` and put back by the next import;
  other leftovers removed, or `ImportReport::notes`), symlinked mod,
  level, skin, data and theme directories followed one level, ids
  colliding in a run get `-2`, ..., and writes `import-report.ron`
  (`ImportEntry`: source, pack, error, warnings; byte-identical to the
  CLI's before the move). `prune` (the CLI's) removes packs of earlier
  runs. Every file is read at most `MAX_SOURCE_BYTES`, every image
  decoded within `rl_pack::image_io`'s bounds. A failed item is in
  the report; a failed write (permission, full disk) is the import's
  error. `discover(path)` lists what an OLX game directory (or its
  `share/gamedir`) holds where OLX's menus look (directories with a
  `script.lgs`, `levels/*.lxl`, `data/themes/*`, `skins/`; the core pack
  only when all of `OLX_CORE_FILES` are there), case-insensitively, with
  `notes` for what it skips; not an OLX directory is an error;
  `discover_until(path, stop)` stops with `Cancelled`.
  `tests/import.rs` runs on `fixtures/olx-mini` (made by
  `fixtures/olx-mini.py`).
- `Content::pack_dir(id)`, `pack_kind(id)`, `pack_origin(id)`.
- Pack ids of OLX imports: `olx_mod_id("MW 1.0")`, `olx_level_id("CastleStrike.lxl")`.

Test: `cargo nextest run -p rl-content` (`loads_every_manifest_item` and the
real-level part of `new_world_spawns_worms_on_free_spots`, `tests/color.rs`'s
matches, `tests/matches.rs` and the imported part of `tests/catalog.rs` need
`RL_CONTENT_DIR`; `tests/import.rs` uses `fixtures/olx-mini` and
`imported_packs_override_bundled` the layers).
