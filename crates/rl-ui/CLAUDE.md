# rl-ui

Menu and HUD logic (spec §8.3, §21 G1-G2): the screen state machine, the
settings and profiles with their files, the menus' egui widgets and the
theme. The window, the GPU and the game live in `rl-game`, which draws
these widgets and carries out their commands (`rl-game/src/shell.rs`).

- **Layer 5, uses `std`.** Depends on `rl-content` (`MatchOptions`,
  `Player`, `Catalog`), `rl-data` (images), `rl-modes`, `rl-bots`,
  `rl-sim`, `rl-input` (same layer: `Bindings`), egui 0.36.2, serde, ron,
  `directories` 6.0.0 and anyhow; `egui_kittest` 0.36.2 for tests.
  **No GPU handle and no window:** everything is tested headless, the
  widgets by label with `egui_kittest` (`tests/flows.rs`).
- **State** (`state.rs`): `UiState::new(settings, profiles, seed)`, then
  `update(UiAction) -> Vec<AppCommand>` (pure). `Screen` is G1's list
  plus `Replays`; `Screen::MENUS` are what Main opens, Back returns to
  Main. The app reports `MatchStarted` / `MatchFailed(why)` / `MatchOver`
  after a `StartMatch(MatchOptions)`, `StartSandbox` or
  `PlayReplay(path)`; until it answers, Start, Back and the menu buttons
  do nothing (no double matches); a failure returns to the screen it came
  from (local play, Main, Replays) with the reason. `game()` says what
  plays (`Game::{Match, Sandbox, Replay}`). In game, Back toggles the pause
  menu (`SetPaused`; a match ending while paused emits `SetPaused(false)`),
  `LeaveMatch` ends the match (`EndMatch`); `Restart` (F5, in game when
  not paused or on the results) plays it again in place (`Restart`);
  a match's results' Start is a rematch (`EndMatch` then `StartMatch`,
  the next seed). The
  `Set*` actions sanitize, then apply (`ApplyVideo`, `ApplyAudio`,
  `ApplyBindings`) and save (`SaveSettings`, `SaveProfiles`) only what
  changed. `bindings()` is the selected profile's, else the options'.
- **Settings** (`settings.rs`): video (`WindowPreset`, fullscreen,
  `FrameRate`, `UpscaleFilter`, interpolation, `ui_scale` in
  `UI_SCALE_RANGE` 0.5-3), audio volumes 0-1, `Bindings`, gameplay
  (`HudStyle`, `MatchPrefs`). `sanitize()` fixes out-of-range values with
  one warning each.
- **Match preferences** (`match_prefs.rs`): mod, level, `bots` (1 to
  `MAX_BOTS`), one `difficulty` or `per_bot` levels, `personality`
  (`BotPersonality`), `loadout`, `rules` (`RulePrefs`), `bonuses`,
  `restrictions`, `loading_time` (OLX's menu ranges `LOADING_TIME_RANGE`,
  `LIVES_RANGE`, ... from `FeatureList.cpp`). `options(seed)` is
  `rl_content::build_match`'s input (the human is worm 0); `set_bots`,
  `set_per_bot`, `set_mod` (keeps a preset the mod has, else
  `DEFAULT_PRESET`, else its first). They **mirror** `DeathmatchSettings`
  (`RulePrefs`) and the bot choices (`BotLevel`, `BotLoadout`,
  `BotPersonality`): the file holds only what the menus edit, under names
  this crate owns; conversions name every field, so a field added to
  `DeathmatchSettings` does not compile until it is mirrored.
- **Screens** (`screens/`): `show(ui, &UiState, &mut MenuData) ->
  Vec<UiAction>` draws the current screen; the caller feeds the actions to
  `update`. `MenuData` holds the `rl_content::Catalog`, the replays
  directory and its files (read when the screen opens) and the level
  thumbnails as egui textures. Main (`main.rs`), local play
  (`local_play.rs`: mod, level grid with thumbnails or a name-and-size
  placeholder, Deathmatch settings, bots, the player's profile, Start with
  "Loading…" and the failure message), replays (`replays.rs`), a pause
  menu over the game and a Leave button on the results; Profiles, Options,
  Import and About are placeholders until their tasks. Esc is `Back` and
  F5 `Restart` on every screen.
- **Replays** (`replays.rs`): `list(dir)` (the `.rlrp` files, automatic
  saves newest first), `auto_save(dir, bytes)` (`match-NNNNNN.rlrp`, the
  last `MAX_AUTO_REPLAYS` kept; a player's own files are never removed).
- **Profiles** (`profiles.rs`): `Profile { name, skin_id, colour, bindings }`
  (OLX's "OpenLieroXor" by default; names at most 20 characters),
  `Profiles { list, selected }`; `sanitize()` keeps at least one.
- **Store** (`store.rs`, Review Focus 1): `Store::platform()` (the
  `ProjectDirs` data directory, else `in_memory` and a warning),
  `Store::open(dir)`, `load_*` (`Loaded { value, warnings }`) and
  `save_*`. Versioned RON (`SETTINGS_VERSION`, `PROFILES_VERSION`), every
  struct `#[serde(default)]`, atomic writes (temporary file, sync,
  rename). An undecodable file gives defaults and a warning and is moved
  to `<name>.bak`, `<name>.1.bak`, ... (at most `MAX_BACKUPS`; then the
  oldest goes and the rest move down); one that decodes but needs values
  fixed is copied there and the fixed values are saved in its place (its
  other values survive). A file that cannot be backed up is never
  overwritten. Files over 1 MiB are bad. Warnings are
  returned, never printed: the caller logs each once.
- **Theme** (`theme.rs`): `apply(ctx, ui_scale)`: dark, larger text, the
  scale as egui's zoom factor (on top of the DPI scale; it takes effect at
  the end of the pass).

Test: `cargo nextest run -p rl-ui` (`tests/store.rs`, `tests/state.rs`,
`tests/flows.rs`: kittest flows by label and the local play's match).
