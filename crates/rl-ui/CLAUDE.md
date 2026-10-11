# rl-ui

Menu and HUD logic (spec §8.3, §21 G1-G2): the screen state machine, the
settings and profiles with their files, and the egui theme. The widgets
and the window live in `rl-game`, which drives this crate.

- **Layer 5, uses `std`.** Depends on `rl-content` (`MatchOptions`,
  `Player`), `rl-modes`, `rl-bots`, `rl-sim`, `rl-input` (same layer:
  `Bindings`), egui 0.36.2, serde, ron, `directories` 6.0.0 and anyhow.
  **No GPU handle and no window:** everything is tested headless (plain
  tests now; `egui_kittest` drives widget flows by label in later tasks).
- **State** (`state.rs`): `UiState::new(settings, profiles, seed)`, then
  `update(UiAction) -> Vec<AppCommand>` (pure). `Screen` is G1's list;
  `Screen::MENUS` are what Main opens, Back returns to Main. The app
  reports `MatchStarted` / `MatchFailed(why)` / `MatchOver` after a
  `StartMatch(MatchOptions)` or `StartSandbox`; until it answers, Start
  and Back do nothing (no double matches). In game, Back toggles the pause
  menu (`SetPaused`), `LeaveMatch` ends the match (`EndMatch`); results'
  Start is a rematch (`EndMatch` then `StartMatch`, the next seed). The
  `Set*` actions sanitize, then apply (`ApplyVideo`, `ApplyAudio`,
  `ApplyBindings`) and save (`SaveSettings`, `SaveProfiles`) only what
  changed. `bindings()` is the selected profile's, else the options'.
- **Settings** (`settings.rs`): video (`WindowPreset`, fullscreen,
  `FrameRate`, `UpscaleFilter`, interpolation, `ui_scale` in
  `UI_SCALE_RANGE` 0.5-3), audio volumes 0-1, `Bindings`, gameplay
  (`HudStyle`, `MatchPrefs`). `MatchPrefs::options(seed)` is
  `rl_content::build_match`'s input (the human is worm 0). The match
  preferences **mirror** `DeathmatchSettings` (`RulePrefs`) and the bot
  choices (`BotLevel`, `BotLoadout`): the file holds only what the menus
  edit, under names this crate owns; conversions name every field, so a
  field added to `DeathmatchSettings` does not compile until it is
  mirrored. `sanitize()` fixes out-of-range values with one warning each.
- **Profiles** (`profiles.rs`): `Profile { name, skin_id, colour, bindings }`
  (OLX's "OpenLieroXor" by default; names at most 20 characters),
  `Profiles { list, selected }`; `sanitize()` keeps at least one.
- **Store** (`store.rs`, Review Focus 1): `Store::platform()` (the
  `ProjectDirs` data directory, else `in_memory` and a warning),
  `Store::open(dir)`, `load_*` (`Loaded { value, warnings }`) and
  `save_*`. Versioned RON (`SETTINGS_VERSION`, `PROFILES_VERSION`), every
  struct `#[serde(default)]`, atomic writes (temporary file, sync,
  rename). A bad file gives defaults (or the values fixed) and a warning
  and is moved to `<name>.bak`, `<name>.1.bak`, ...; one that cannot be
  moved is never overwritten. Files over 1 MiB are bad. Warnings are
  returned, never printed: the caller logs each once.
- **Theme** (`theme.rs`): `apply(ctx, ui_scale)`: dark, larger text, the
  scale as egui's zoom factor (on top of the DPI scale; it takes effect at
  the end of the pass).

Test: `cargo nextest run -p rl-ui` (`tests/store.rs` and `tests/state.rs`).
