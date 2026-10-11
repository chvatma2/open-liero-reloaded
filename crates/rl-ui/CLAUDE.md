# rl-ui

Menu and HUD logic (spec §8.3, §21 G1-G2): the screen state machine, the
settings and profiles with their files, the menus' egui widgets and the
theme. The window, the GPU and the game live in `rl-game`, which draws
these widgets and carries out their commands (`rl-game/src/shell.rs`).

- **Layer 5, uses `std`.** Depends on `rl-content` (`MatchOptions`,
  `Player`, `Catalog`, skins), `rl-data` (images), `rl-modes`, `rl-bots`,
  `rl-sim`, `rl-replay` (a replay's header), `rl-input` (same layer:
  `Bindings`), egui 0.36.2, serde, ron, `directories` 6.0.0 and anyhow;
  `egui_kittest` 0.36.2 (with `wgpu` and `snapshot` for the HUD's image
  snapshots) and `rl-scene` (the game's skin frames) for tests.
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
  plays (`Game::{Match, Sandbox, Replay}`). A match starts on
  `WeaponSelect`: `ChooseWeapons { weapons, names }` (Done) emits
  `ArmPlayer(weapons)`, remembers the names in the selected profile
  (`SaveProfiles`) and goes in game. In game, Back toggles the pause
  menu (`SetPaused`; a match ending while paused emits `SetPaused(false)`);
  paused, `Open(Options)` opens the options and their Back returns;
  `LeaveMatch` ends the match (`EndMatch`); `Restart` (F5, in game when
  not paused or on the results) plays it again in place (`Restart`);
  a match's results' Start is a rematch (`EndMatch` then `StartMatch`,
  the next seed, the weapons chosen again); `SaveReplay` asks the app
  (`AppCommand::SaveReplay`, answered by `ReplaySaved`, shown as the
  message). The
  `Set*` actions sanitize, then apply (`ApplyVideo`, `ApplyAudio`,
  `ApplyBindings`) and save (`SaveSettings`, `SaveProfiles`) only what
  changed (`SetBindings`: `sanitize_bindings`). `bindings()` is the
  selected profile's, else the options'.
- **Settings** (`settings.rs`): video (`WindowPreset`, fullscreen,
  `FrameRate`, `UpscaleFilter`, interpolation, `ui_scale` in
  `UI_SCALE_RANGE` 0.5-3), audio volumes 0-1, `Bindings`, gameplay
  (`HudStyle`, `MatchPrefs`). `sanitize()` fixes out-of-range values and
  keys (`sanitize_bindings`: the `GAME_KEYS` Esc, F5, F2, Tab and H are
  dropped, then an action left unreachable gets OLX's back), one warning
  each.
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
  is rebound there; while a capture is open egui hears no key: the
  capture has it alone), the pause menu (`ingame.rs`: Resume, Options,
  Quit to menu), the weapon selection (`weapon_select.rs`, over the level:
  the slots with ◀ ▶, Random, Done; driven by `MenuData::held`, the game's
  keys the app feeds, or the mouse; `MenuData::weapons` is the app's
  `WeaponMenu`) and the results (`results.rs`: `panel(ctx, results,
  footer)` also serves the command line's games; Rematch or, for a
  replay, Watch again; Save replay, once a match (`replay_saved`: then
  "Saved"); Back to menu; `MenuData::results`; "Loading…" while a rematch
  starts; the table and the weapon slots scroll so the buttons stay on a
  small screen);
  Import and About are placeholders until their tasks. Esc is `Back` and
  F5 `Restart` on every screen, a held key's repeats ignored
  (`pressed_once`); in the options Esc first closes an open dialog.
- **Replays** (`replays.rs`): `list(dir)` (the `.rlrp` files, automatic
  saves newest first, each with `header_problem`: 6 bytes read),
  `auto_save(dir, bytes)` (`match-NNNNNN.rlrp`, the last
  `MAX_AUTO_REPLAYS` kept; a player's own files are never removed),
  `save_kept(dir, bytes)` (the results' Save replay: `replay-NNN.rlrp`).
- **HUD** (`hud/`, §21 G5, Modern style): `model.rs` (names, OLX's kill
  feed texts, `clock`, scoreboard `rows`, `Results`), `view.rs`
  (`HudView`, the frame's data; `WormHud::of(world, i, names, ..)`: health,
  lives, kills, slots with OLX's bar states), `layout.rs`
  (`HudLayout::new(screen)`: health and weapons bottom left, clock top
  centre, feed top left, minimap top right at OLX's 119 × 94 proportions,
  keys bottom right; it shrinks to half size and never covers the health
  or weapons: Review Focus 5, `hud_layout_holds_at_all_scales`),
  `minimap.rs` (`image`: the colour terrain box-filtered to
  `TEXTURE_SIZE`; OLX's dot rectangles and blink; the view rectangle),
  `paint.rs` (`show(ui, &view)`, `table`). Image snapshots at 1080p and
  1440p (`tests/hud_snapshots.rs` against `fixtures/ui/`, in the GPU tier
  with `RL_GPU_TESTS`; `cargo xtask bless-render --reason` re-records them, logged).
- **Weapon selection** (`weapon_select.rs`, §21 G4): `WeaponMenu` (names,
  enabled, `findEnabledWeapon`'s fallback, seed; `needs_choice`),
  `WeaponSelect::new(menu, remembered)` (the profile's names), `change`
  (OLX's loop over the enabled weapons), `random` (`get_random_weapons`),
  `on_keys(menu, held, now)` (edges, auto-repeat, × 6 with Select weapon,
  Done on Fire's release).
- **Profiles** (`profiles.rs`): `Profile { name, skin_id, colour, bindings,
  weapons }` (OLX's "OpenLieroXor" by default; `weapons`: the last
  selection by name, OLX's `DEFAULT_WEAPONS` at first), `Profiles { list,
  selected }`; `bot_colours(human, n)` (OLX's `CPU_COLOURS` in turn,
  skipping those near the human's);
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
rebinding flow and the other sections through the widgets, game keys never
bound, the capture swallowing its key; `tests/ingame.rs`: the HUD layout,
the weapon selection, the pause menu, Esc/F5 repeats, the results;
`tests/hud_snapshots.rs` with `RL_GPU_TESTS`).
