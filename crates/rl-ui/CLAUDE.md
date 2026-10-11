# rl-ui

Menu and HUD logic (spec §8.3, §21 G1-G2): the screen state machine, the
settings and profiles with their files, the menus' egui widgets and the
theme. The window, the GPU and the game live in `rl-game`, which draws
these widgets and carries out their commands (`rl-game/src/shell.rs`).

- **Layer 5, uses `std`.** Depends on `rl-content` (`MatchOptions`,
  `Player`, `Catalog`, skins), `rl-data` (images), `rl-modes`, `rl-bots`,
  `rl-sim`, `rl-replay` (a replay's header), `rl-input` (same layer:
  `Bindings`), egui 0.36.2, serde, ron, `directories` 6.0.0 and anyhow;
  `egui_kittest` 0.36.2 and `rl-scene` (the game's skin frames) for tests.
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
  changed (`SetBindings` repairs: `Bindings::repair`). `bindings()` is the
  selected profile's, else the options'.
- **Settings** (`settings.rs`): video (`WindowPreset`, fullscreen,
  `FrameRate`, `UpscaleFilter`, interpolation, `ui_scale` in
  `UI_SCALE_RANGE` 0.5-3), audio volumes 0-1, `Bindings`, gameplay
  (`HudStyle`, `MatchPrefs`). `sanitize()` fixes out-of-range values and
  keys that leave an action unreachable (OLX's back), one warning each.
  The master volume defaults to OLX's 70 %; the game plays `master * sfx`.
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
  directory and its files (read when the screen opens), the level
  thumbnails as egui textures and the skin preview (`preview_wanted`, set
  by the profiles screen each frame; `preview`, the app's textures). Main
  (`main.rs`), local play (`local_play.rs`: mod, level grid with
  thumbnails or a name-and-size placeholder, Deathmatch settings, bots, the
  player's profile, Start with "Loading…" and the failure message; a saved
  mod or level no longer imported becomes the first listed), profiles
  (`profiles.rs`: list, New, Delete confirmed, "Play as this", name with
  its error, colour sliders, searchable skins, the preview; it opens on
  the profile playing), replays (`replays.rs`, each file's header problem
  shown), options (`options.rs`: Video, Audio, Keys, Gameplay tabs; the UI
  scale applies when its slider is released; Gameplay reuses local play's
  rules and bots widgets for the default match; `keys.rs`: per action its
  keys and gamepad inputs, each removable but the last of its device, "+"
  opens a capture (`MenuData::capture`, an `rl_input::Capture` the app
  feeds), a key other actions have opens a dialog (move, refused when it
  would leave one without; share; cancel), `GAME_KEYS` refused, browser
  shortcuts pointed out, Reset to OLX's keys; a profile with its own keys
  is rebound there), a pause menu over the game and a Leave button on the
  results; Import and About are placeholders until their tasks. Esc is
  `Back` and F5 `Restart` on every screen; in the options Esc first closes
  an open dialog.
- **Replays** (`replays.rs`): `list(dir)` (the `.rlrp` files, automatic
  saves newest first, each with `header_problem`: 6 bytes read),
  `auto_save(dir, bytes)` (`match-NNNNNN.rlrp`, the last
  `MAX_AUTO_REPLAYS` kept; a player's own files are never removed).
- **Profiles** (`profiles.rs`): `Profile { name, skin_id, colour, bindings }`
  (OLX's "OpenLieroXor" by default), `Profiles { list, selected }`;
  `check_name(name, except)` (trimmed, not empty, at most 20 characters,
  no control character, no other profile's name ignoring case:
  `NameError`), `rename`, `add` ("Player N", `NEW_COLOURS` in turn),
  `remove` (never the last; the one playing stays selected);
  `sanitize()` keeps at least one.
- **Skin preview** (`skin_preview.rs`, plan 7 ruling Q2): `frames(skin,
  colour)` cuts every frame, faced right and left, from
  `LoadedSkin::colorized` with `rl_content::skin_sprite`: the game's own
  colorization and frame cut (`skin_preview_uses_colorization_like_the_game`
  compares them with `rl_scene`'s). `PreviewAnim` is OLX's menu animation.
  The app uploads the frames (`PreviewTextures`): no GPU handle here.
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
`tests/flows.rs`: kittest flows by label and the local play's match,
`tests/profiles.rs`: profiles through the widgets, names, the preview; its
last part compares with `rl_scene::SceneAssets` when `RL_CONTENT_DIR` is
set; `tests/options.rs`: rebinding never leaves an action unreachable, the
rebinding flow and the other sections through the widgets).
