# olr-content

Loads the packs in a content directory (`content/packs`, written by
`olr-cli import`) into ready-to-run inputs: `Content::open(root)` lists the
packs (a directory named `.…` is skipped: an import's pack being written)
and loads the core pack `olx-core` once (`Content::open_layers(&[user
packs, bundled])`: layered, spec §9.4: an id in a higher layer hides the
same id below it, the core pack too; an unreadable higher pack hides
nothing; a missing root is skipped); then `load_mod` (`SimMod` +
decoded images + sound bytes keyed by `SimMod::sounds()` names, `olx:` game
sounds from the core pack), `load_level`, `load_theme` (holes, green ball,
tiles; `stamps()`, `green_mask()`), `load_skin`/`skin_or_default`, `core()`
(sprites by name, game sounds, `colours.cfg`) and `new_world(&SandboxSetup)`.

- **Layer 4, uses `std`.** Depends on `olr-sim`, `olr-modes`, `olr-bots`,
  `olr-replay`, `olr-pack`, `sha2`, `hex`, `olr-terrain`, `olr-data`, `olr-vfs`, the
  importers (`olr-import-lgs`, `olr-import-lxl`, `olr-import-gfx`), `anyhow`,
  `serde` and `ron`.
  `olr-scenario` (same layer) uses it.
- **Never panic on content** (spec §19, Review Focus 3): a missing or odd
  asset gets a placeholder (`PLACEHOLDER` colour; core sprites at their
  `CORE_SPRITES` size, so frame arithmetic still works; a 672 × 18 skin;
  silence for sounds) and a warning. Warnings are returned
  (`Content::warnings`, `LoadedMod::warnings`, ...), never printed: the
  caller logs each once. A missing pack or unreadable definition is an `Err`.
  The core pack is read tolerantly (`olr_pack::read_core_tolerant`): an
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
  the core pack, so `olr-audio` never decodes it (`check_sound`). A stream
  whose header lies is bounded only by the byte cap.
- **Weapon restrictions:** `LoadedMod` keeps the mod's `name` and
  `restrictions` (its presets); `LoadedMod::weapon_rules(file)` and
  `Content::weapon_rules(id, file)` give OLX's list for a match
  (`olr_data::restrictions::WeaponRules`). `SandboxSetup::restrictions` (OLX's
  `WeaponRestrictionsFile`, `DEFAULT_PRESET` = `Standard 100lt`) decides the
  weapons natural bonuses may hold (`MatchSetup::bonus_weapons`).
- **Worlds.** `new_world` loads the mod, level and theme and calls
  `new_world_from(&mod, &level, &theme, &setup)`, which a game restart
  calls with the assets it loaded once: `match_setup` (the `olr_sim::MatchSetup`,
  for a world of any game mode) played with `olr_sim::OracleMode`. Worms
  spawn in order; a worm without a position draws one from the world's
  `FindSpot` when it spawns, so the same setup gives the same world. Worms
  get the mod's first five weapons unless the spec names them. The clock
  starts at `SANDBOX_START` (`MatchSetup::T0`). The settings take the
  mod's worm and rope physics (`LoadedMod::worm`/`rope`,
  `SimSettings::with_mod_physics`, OLX's `lx56modSettings` layer): MW 1.0
  plays its own gravity, jump and rope, not Classic's.
- **Colour terrain** (`color.rs`, plan 5 ruling R1): `LoadedTheme::art()`,
  `LoadedLevel::color_map(&theme)` (before any edit; spawn holes come with
  the first tick), `color_edit(&EventKind)` converts a `TerrainEdit`, and
  `apply_edits(map, events, terrain)` replays a tick's edits and rebuilds a
  `lost_area` (for callers without `olr-fx`, whose `Fx::step_tick` replays
  them itself between its entity passes: never both). `tests/color.rs` plays 3000-tick matches and checks every
  pixel's colour against the material (Review Focus 2).
- **Replays** (`replay.rs`, spec §20 F8): `replay_packs(mod, level)` names
  a match's packs (the level's theme too) with their `olr_pack::content_hash`;
  `replay_assets`, `world_for_replay` and `play_replay` refuse a replay whose
  packs are missing or hash differently (Review Focus 3), then rebuild its
  world as `match_setup` does (the mod from `load_mod`).
- **Matches** (`matches.rs`, plan 6 ruling R5): `build_match(MatchAssets,
  &MatchOptions) -> BuiltMatch` (or `Content::build_match`) is the one
  assembly of a Deathmatch, used by `olr-cli match` and `olr --mode
  deathmatch`: `players[i]` is `Player::Human` (random enabled weapons:
  the command line's games), `Player::HumanWith(weapons)` (the weapon
  selection's choice, refused unless the restrictions enable each; the
  recording's setup holds it) or `Player::Bot(BotConfig)` (`choose_loadout`
  from `BotRng::new(seed, i, personality)`); `MatchOptions::pool_capacity`
  sets the projectile pool (`MatchSetup::DEFAULT_POOL_CAPACITY`, OLX's
  3000, everywhere but `olr-cli stress`; the replay records it); chaos bots get the
  `ChaosCycle` respawn loadout; a bot whose `BotConfig::validate` fails is
  refused; `Deathmatch::prepare`, `World::new`, a
  started `Recorder` and the bots on one shared `AreaCache` (think in
  `bots` order). `olr-cli`'s golden checksum streams pin it.
  `MatchOptions::deathmatch(mod, level, seed, players)` gives the defaults
  (LX56 settings, default rules, `DEFAULT_PRESET`, OLX's pool), which
  `olr-cli`'s `MatchSpec::match_options` and `olr-netcode`'s tests start from.
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
  face)` (a frame's rectangle; facing left is the exact mirror of the
  right-facing frame: OLX's one-column offset quirk was dropped
  2026-10-16), `LoadedSkin::colorized(colour)` and `skin_sprite(sheet,
  f, face)`: the one source the game (`olr-scene`) and the profiles' preview
  (`olr-ui`) draw skins from. `Content::skins()` and `Catalog::skins` list
  them by manifest name; `resolve_id(given, prefix, ids)` (an id, or an
  OLX name's import id such as `default.png` → `olx-skin-default`),
  `Content::skin_id`, `Catalog::skin`.
- **Import** (`import.rs`, plan 7 ruling Q3): the one OLX import
  pipeline, `olr-cli import`'s and the game's Import screen's.
  `import(olx, &ImportPlan, out, &ImportOptions, progress)` imports the
  plan's mods, levels (with their thumbnail), themes, core files and
  skins one by one (`Progress::Item`/`Done`; the callback returning
  `false` cancels between items), writes each pack hidden
  (`.import-<id>`) and renames it into place (the old one renamed aside
  to `.replaced-<id>`, then removed: a failure is that item's warning; a
  `.replaced-<id>` without its `<id>`, from an import killed between the
  renames, is listed by `open_layers` and put back by the next import,
  the newest of an id's (`.replaced-<id>~k` too: `newest_aside_first`, by
  the manifest's time, then the highest `k`);
  other leftovers removed, or `ImportReport::notes`), symlinked mod,
  level, skin, data and theme directories followed one level, ids
  colliding in a run get `-2`, ..., and writes `import-report.ron`
  (`ImportEntry`: source, pack, error, warnings; byte-identical to the
  CLI's before the move). `ImportPlan::extra_skins` (`SkinSource`: a
  directory of skin PNGs with its own origin source and revision) adds
  skins from outside the game directory in the same run (the manifest's
  `extra_skins`: OLX 0.58 rc5's release skins). `prune` (the CLI's) removes packs of earlier
  runs. Every file is read at most `MAX_SOURCE_BYTES`, every image
  decoded within `olr_pack::image_io`'s bounds. A failed item is in
  the report; a failed write (permission, full disk) is the import's
  error. `discover(path)` lists what an OLX game directory (or its
  `share/gamedir`) holds where OLX's menus look (directories with a
  `script.lgs`, `levels/*.lxl`, `data/themes/*`, `skins/`; the core pack
  only when all of `OLX_CORE_FILES` are there), case-insensitively, with
  `notes` for what it skips; not an OLX directory is an error;
  `discover_until(path, stop)` stops with `Cancelled`.
  `tests/import.rs` runs on `fixtures/olx-mini` (made by
  `fixtures/olx-mini.py`).
- **Download cache** (`cache.rs`, plan 11 ruling N30): `DownloadCache::open(dir)`
  keeps packs a host sent in `packs/<content hash>/` (`get` touches the
  LRU use counter, saved with the next install, eviction or clear or when
  the cache drops; `evict(keep, limit)` drops the least recently used but
  never `keep`, `CACHE_BYTES` = 1 GiB, and reports `Eviction { evicted,
  stuck }`: a pack it could not remove whole leaves the index too),
  `index.ron` (atomic; rebuilt from `packs/*/pack.ron` when bad) and
  resumable partials `<hash>.part` + `<hash>.part.ron` (archive, size and
  `started`, a use counter; `append` only at `offset == have`; `partial`;
  `partial_bytes`, their disk charge, `file_charge(have)` plus a block for
  the meta file each; `trim_partials(keep, limit)` discards the oldest
  partials not in `keep` while packs plus partials exceed `limit`; at most
  `MAX_PARTIALS` (32): a new one discards the oldest not kept by a
  `Keeper`, and is refused with the reason when all are kept; review R1).
  Partials are an in-memory index built when the cache opens and kept by
  every append, discard and clear: accounting never scans the directory.
  `install(hash, id, kind, limits)` checks the partial's size against
  `limits.bytes` and its archive SHA-256, decodes into `tmp/` within
  `limits` (`archive_limits(kind)`: `SKIN_LIMITS` for skins, 64 KiB,
  16 files, depth 2, 256 KiB on disk; else `Limits::PACK`), then checks
  content hash, id, kind and `validate`; an entry's bytes (and
  `total_bytes`, what eviction counts) are its **disk charge**
  (`olr_pack::archive::file_charge` a file, `DIR_CHARGE` a directory: real
  disk use, review C2), the same when `open` adopts a pack; any
  failure removes `tmp/` and the partial (an incomplete partial stays).
  It is `begin_install` + `PendingInstall::verify` (needs no cache, so a
  worker verifies without holding the cache's mutex) + `finish_install`.
  `open` reconciles the index with `packs/` (adopts unlisted valid packs, drops the rest).
  Adoption reads only `pack.ron`, it does not re-verify the content hash:
  a pack `evict` left partly removed (`stuck`) is adopted again if its
  manifest survived (known, left for later).
  `Keeper::new(&Arc<Mutex<DownloadCache>>)` / `keep(hashes)` registers a
  list's hashes with the shared cache until it drops: `evict` and
  `trim_partials` keep the union of every keeper's hashes besides their
  `keep` (review Minor 2: one process's fetchers, host list and
  `--net-sim` peers share one cache).
  One cache directory belongs to one open cache (ruling PF11-3): `open`
  holds an exclusive `File::try_lock` on `<dir>/lock` for the cache's life
  and fails with `InstallError::Locked` while another holds it (flock on
  Linux, LockFileEx on Windows: per open file, so a second `open` in the
  same process is refused too); one process shares one cache behind a mutex.
  A hash that is not 64 lowercase hex never names a file.
  `DownloadCache::lookup(dir, hash)` reads without opening: a valid hash
  whose `packs/<hash>/` holds a readable manifest; no lock, no index,
  nothing created or written, the pack not verified (replays: their
  `check_replay_packs` hashes again; plan 11 N36).
  `Content::with_packs(&[(id, dir)])` is a copy with those packs laid over
  by id (nothing on disk changes; a pack of another kind than this
  content's of that id is refused); `Content::has(id, hash)`; `pack_hash`
  is memoised per pack directory for the `Content`'s life (an overlay
  copies the memo); `known_hash(id)` reads the memo without hashing and
  `remember_hash(id, dir, hash)` fills it from a hash computed elsewhere
  (olr-online's worker thread; ignored unless `dir` is `id`'s directory).
- `Content::pack_dir(id)`, `pack_kind(id)`, `pack_origin(id)`.
- `default_content_dir(exe)` (`locate.rs`): `olr-game`'s and `olr-server`'s
  `--content` default, `CONTENT_DIR` (`content/packs`) beside the
  executable when it is a directory there (an unpacked release zip), else
  relative to the working directory.
- Pack ids of OLX imports: `olx_mod_id("MW 1.0")`, `olx_level_id("CastleStrike.lxl")`.

Test: `cargo nextest run -p olr-content` (`loads_every_manifest_item` and the
real-level part of `new_world_spawns_worms_on_free_spots`, `tests/color.rs`'s
matches, `tests/matches.rs` and the imported part of `tests/catalog.rs` need
`OLR_CONTENT_DIR`; `tests/import.rs` uses `fixtures/olx-mini` and
`imported_packs_override_bundled` the layers).
