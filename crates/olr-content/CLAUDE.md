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
tiles; `stamps()`, `green_mask()`), `load_skin`/`skin_or_default`, `skin_matching(id, hash)` (plan 13c: the pack id exactly and its content hash, `NO_SKIN_HASH` only for `DEFAULT_SKIN`, else the default skin and a warning), `core()`
(sprites by name, game sounds, `colours.cfg`) and `new_world(&SandboxSetup)`.

- **Layer 4, uses `std`.** Depends on `olr-sim`, `olr-modes`, `olr-bots`,
  `olr-replay`, `olr-pack`, `sha2`, `hex`, `olr-terrain`, `olr-look`, `olr-data`, `olr-vfs`,
  `anyhow`, `serde` and `ron`.
  `olr-scenario` (same layer) uses it.
- **Never panic on content** (spec §19, Review Focus 3): a missing or odd
  asset gets a placeholder (`PLACEHOLDER` colour; core sprites at their
  `CORE_SPRITES` size, so frame arithmetic still works; a 672 × 18 skin
  (`sheet_size` of OLX's layout);
  silence for sounds) and a warning. Warnings are returned
  (`Content::warnings`, `LoadedMod::warnings`, ...), never printed: the
  caller logs each once. A missing pack or unreadable definition is an `Err`.
  The core pack is read tolerantly (`olr_pack::read_core_tolerant`): an
  undecodable sprite or a missing sound file costs only that asset.
- **Edited levels** (plan 14c): `LoadedLevel::from_source(&LevelSource)`
  is `load_level` on the editor's level (compiled, the same art bound);
  `first_spawn()` is the first spawn point when it is a good spawn point
  (OLX's `CMap::IsGoodSpawnPoint`, as `FindSpot` checks: no rock in the
  6 × 6 square from `(x - 3, y - 3)`, outside the level counting as rock),
  else `None`; `build_match_spawning(assets, options,
  human_spawn)` is `build_match` with the first human worm's
  `WormSetup::spawn` set (`build_match` passes `None`: unchanged).
- **Distant layers** (issue #11): `LoadedLevel::distant` (from
  `olr_pack::read_level`, or `from_source`'s level; dropped with a warning
  when they do not check); with any, `color_map` makes a see-through map
  (`ColorMap::new_see_through`), else the map is as before
  (`distant_layers_load_and_make_the_map_see_through`). No simulation,
  bot or material reads them.
- **Looks** (`maps.rs`, `mods.rs`, plan 15a Tasks 6 and 14b): `LoadedLevel`
  carries the level's `look` (`olr_look::LevelLook`), `glow`
  (`olr_terrain::GlowLayers`), `clear` (clear flags per world pixel,
  `look/clear.png`: solids light passes while worms and projectiles still
  collide) and `warnings`. Playing reads a level leniently (`olr_pack::read_level`
  through `olr_format::levels::read_level_files_lenient`): a look, glow
  image or clear image that does not check is dropped with a warning
  naming the pack (a dropped look drops its glow too), while
  `olr_pack::validate` stays strict. `has_look()` is a look, a glow or
  clear solids: the renderer's look passes draw it. `from_source` compiles
  the editor's look as well (`olr_format::levels::compile_look`), and
  `color_map` hands the glow and clear flags to the `ColorMap`
  (`set_glow`, `set_clear`). `LoadedMod::look` (`olr_look::ModLook`) is
  indexed like `SimMod::projectiles` and `SimMod::weapons`, each
  projectile's look found by its `ModDef` key (`SimProjectile::key`), so
  shared and named projectiles and their children keep their looks (batch
  A review, Critical 1). The imported packs have no look (`tests/look.rs`;
  `content/reloaded`'s Liero v1.0 Reloaded has one, issue #21).
- **Hostile levels:** `load_level` refuses an art scale above
  `MAX_ART_SCALE` (8) or a colour image above `MAX_COLOR_MAP_BYTES`
  (256 MiB), so a pack cannot make the colour map allocate gigabytes.
- **Two mod loaders.** `sim_mod(id)` is strict, as OLX's client (what the
  oracle replays): an unreadable PNG or an image projectile without its
  frame count is an error (`check_frames`). `load_mod(id)` gives such a
  projectile a one-frame placeholder. On good content both give the same
  `SimMod` (`tests/content.rs`). Frame counts are `width / height` of the
  pack's PNGs (spec §18 D3). `load_mod_pack(id, ModPack)` is `load_mod` on
  a pack read elsewhere (issue #18: the mod editor's unsaved mod, from
  `olr_edit::pack::mod_pack`), its game sounds from this content's core
  pack (`a_mod_pack_given_loads_as_an_installed_one`). Issue #19:
  `LoadedMod::glow` holds each image's glow mask (`gfx/<name>.glow.png`,
  `olr_format::mods::decode_glow_mask`: of the image's size, else left
  out with a warning), and `glow_overlays(file)` its glowing pixels per
  quarter, as a skin's (`a_mod_image_loads_with_its_glow_mask`); the
  simulation never sees a mask.
- **Hostile sounds** (`olr_pack::sounds`, re-exported; moved there in
  issue #19 so the editors' imports check a sound as the game does): a sound above `MAX_SOUND_BYTES` (2 MiB)
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
  **Loadouts** (issue #5): `WeaponChoice::{Preset(file), Loadout(MatchLoadout)}`
  is a match's weapons (`MatchOptions::weapons`): a preset for the
  developer tools (`olr-cli`'s matches, the oracle: `deathmatch()` gives
  `DEFAULT_PRESET`), a loadout for every player-facing game
  (`WeaponChoice::default_loadout()`: `Default`, every weapon at 100 %).
  `LoadedMod::rules(&WeaponChoice)` / `Content::rules(id, ..)` give its
  `WeaponRules` (a loadout's by `WeaponRules::from_loadout`). A loadout's
  loading time (clamped into `LOADING_TIME_RANGE`) replaces the settings',
  its weapons are the natural bonuses' (`bonus_weapons`) and its unique
  rule refuses a `HumanWith` repeat while other weapons are free
  (`unique_ok`); the bots keep it (`olr_bots::choose_loadout`). A
  match whose weapons enable none of the mod's is refused. The
  replay records the result (loading time, bonus weapons, loadouts).
  `DEFAULT_LOADOUT` and `MatchLoadout` are re-exported.
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
  world as `match_setup` does (the mod from `load_mod`). `build_match`'s
  recorder starts with `default_looks(players)` ("Bot N" in OLX's bot
  colours, "Player N" grey, the default skin: what `olr-cli match` and
  stress record); a caller that knows better calls
  `BuiltMatch::recorder.set_looks` (`olr-game`).
- **Matches** (`matches.rs`, plan 6 ruling R5): `build_match(MatchAssets,
  &MatchOptions) -> BuiltMatch` (or `Content::build_match`) is the one
  assembly of a Deathmatch, used by `olr-cli match` and `olr --mode
  deathmatch`: `players[i]` is `Player::Human` (random enabled weapons:
  the command line's games), `Player::HumanWith(weapons)` (the weapon
  selection's choice, refused unless the rules enable each, or under a
  loadout's unique rule it repeats one while others are free; the
  recording's setup holds it) or `Player::Bot(BotConfig)` (`choose_loadout`
  from `BotRng::new(seed, i, personality)`); `MatchOptions::pool_capacity`
  sets the projectile pool (`MatchSetup::DEFAULT_POOL_CAPACITY`, OLX's
  3000, everywhere but `olr-cli stress`; the replay records it); chaos bots get the
  `ChaosCycle` respawn loadout; a bot whose `BotConfig::validate` fails is
  refused; every human's worm, in worm order, is the replay's local slots
  (`Recorder::set_viewers`, issue #29); `Deathmatch::prepare`, `World::new`, a
  started `Recorder` and the bots on one shared `AreaCache` (think in
  `bots` order). `olr-cli`'s golden checksum streams pin it.
  `MatchOptions::deathmatch(mod, level, seed, players)` gives the defaults
  (LX56 settings, default rules, `WeaponChoice::Preset(DEFAULT_PRESET)`,
  OLX's pool), which
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
- **Skins** (`skins.rs`, plan 7 task 3): a skin's `FrameLayout`
  (`LoadedSkin::layout()`, `art_scale()`; OLX's or issue #12's double
  resolution), `SHEET_FRAMES`, `skin_source(layout, f, face)` (a frame's
  rectangle, saturating; facing left is the exact mirror of the
  right-facing frame: OLX's one-column offset quirk was dropped
  2026-10-16), `LoadedSkin::colorized(colour)` and `skin_sprite(sheet,
  layout, f, face)`: the one source the game (`olr-scene`) and the
  profiles' preview (`olr-ui`) draw skins from. Issue #12:
  `glow_overlays(colour)` (per glow quarter, `GLOW_LEVELS` 4 and
  `glow_level`, `olr-data`'s re-exported (issue #13), the colorized sheet's pixels of that strength, transparent
  elsewhere; none without glow) and `frame_lights(colour)` (`SkinLights`:
  the light's reach and per frame a `SkinFrameLight` (sRGB colour of the
  frame's glowing pixels weighted by glow; intensity the light's scaled by
  the frame's glow over `FULL_LIGHT_GLOW`, 3 world pixels of full glow),
  `None` for a frame without glow; `None` without a light). Tests:
  `double_frames_are_cut_at_their_layout`, `glow_rounds_to_quarters`,
  `tests/content.rs`'s `a_double_skin_loads_with_its_glow_and_light`. `Content::skins()` and `Catalog::skins` list
  them by manifest name; `resolve_id(given, prefix, ids)` (an id, or an
  OLX name's import id such as `default.png` → `olx-skin-default`),
  `Content::skin_id`, `Catalog::skin`.
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
  relative to the working directory. `bundled_roots(content)` (issue #7):
  OLR's own packs, `RELOADED_DIR` beside it (`content/reloaded`, the
  hand-made Reloaded levels and Liero v1.0 Reloaded, shipped too), then `content`;
  `Content::open_bundled(content)` opens those layers (an unreadable
  `content` an error, as `open`; a missing `reloaded` skipped), what the
  game, the server and `olr-cli`'s commands open by default. The two roots
  share no pack id (`olr-cli`'s `tests/reloaded.rs`), so neither hides
  the other's; `tests/content.rs`'s
  `the_reloaded_packs_lie_over_the_bundled_content`. `assets_dir(content)`
  (issue #9): the level editor's built-in assets, `ASSETS_DIR` beside it
  (`content/assets`; not packs, so no root: the editor's host lists it).
- Pack ids of OLX imports: `olx_mod_id("MW 1.0")`, `olx_level_id("CastleStrike.lxl")`.

Test: `cargo nextest run -p olr-content` (`loads_every_manifest_item` and the
real-level part of `new_world_spawns_worms_on_free_spots`, `tests/color.rs`'s
matches, `tests/matches.rs`, the imported part of `tests/catalog.rs` and
`tests/look.rs`'s colour-map and bundled-content checks need
`OLR_CONTENT_DIR`; `olr-import`'s tests cover the layers).
