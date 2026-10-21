# olr-ui

Menu and HUD logic (spec §8.3, §21 G1-G2): the screen state machine, the
settings and profiles with their files, the menus' egui widgets and the
theme. The window, the GPU and the game live in `olr-game`, which draws
these widgets and carries out their commands (`olr-game/src/shell.rs`).

- **Layer 5, uses `std`.** Depends on `olr-content` (`MatchOptions`,
  `Player`, `Catalog`, skins), `olr-data` (images), `olr-format` (the
  level size limit), `olr-modes`, `olr-bots`,
  `olr-sim`, `olr-replay` (a replay's header), `olr-input` (same layer:
  `Bindings`), egui 0.36.2, serde, ron, serde_json (a bug report's payload),
  `directories` 6.0.0 and anyhow;
  `egui_kittest` 0.36.2 (with `wgpu` and `snapshot` for the HUD's image
  snapshots) and `olr-scene` (the game's skin frames) for tests.
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
- **Settings** (`settings.rs`): video (`WindowPreset`: 1280 × 720,
  1600 × 900, 1920 × 1080; fullscreen, borderless at the monitor's size,
  offered in the same "Display" list; v0.1.0's `window: Desktop` loads as
  fullscreen through `VideoFile`;
  `FrameRate`, `UpscaleFilter`, interpolation, `ui_scale` in
  `UI_SCALE_RANGE` 0.5-3 (on top of the height's scale), `hud_layout`:
  `HudArrangement::{Auto, Bar, Sides}`, Auto when an old file lacks it;
  `blood`, issue #6: this player's blood amount in percent, `BLOOD_RANGE`
  0-500, 100 by default and for an older file, the slider's
  `BLOOD_STEP` 10; the app hands it to `olr_present::Presentation::set_blood`,
  nothing sends it), audio volumes 0-1, `Bindings`, gameplay
  (`MatchPrefs`: local play's last setup; the HUD has one style: an old file's `hud:` is ignored),
  an old file's `import:` block (the removed Import screen's) is ignored.
  `sanitize()` fixes out-of-range values and
  keys (`sanitize_bindings`: the `GAME_KEYS` Esc, F5, F2, F3 (the network
  panel), Tab, H and
  PageUp/PageDown (a watched game's fast-forward) are dropped, then an
  action left unreachable gets OLX's back), one warning each.
  The master volume defaults to OLX's 70 %; the game plays `master * sfx`.
- **Match preferences** (`match_prefs.rs`): mod, level, `bots` (1 to
  `MAX_BOTS`), one `difficulty` or `per_bot` levels, `personality`
  (`BotPersonality`: Olx, Chaos, Idle, plan 15c: "Idle (does nothing)"), `loadout` (`BotLoadout`: the
  bots' weapon choice), `rules` (`RulePrefs`; OLX's menu ranges
  `LIVES_RANGE`, ... from `FeatureList.cpp`), `weapons` (the match's
  `MatchLoadout` as last chosen: its name, which the menus look up again,
  and what it holds, which plays; an older file's `bonuses`,
  `restrictions` and `loading_time` are ignored: it plays `Default`).
  `options(seed)` is `olr_content::build_match`'s input (the human is
  worm 0; OLX's LX56 settings, no bonuses, `WeaponChoice::Loadout`);
  `set_bots`, `set_per_bot`, `set_mod(m, own)` (the loadout of the same
  name when the new mod has one, else `Default`: `loadouts::resolve`);
  `sanitize` cleans the loadout's name and clamps its loading time
  (`LOADING_TIME_RANGE`, re-exported from `olr_data::loadouts`). They **mirror** `DeathmatchSettings`
  (`RulePrefs`) and the bot choices (`BotLevel`, `BotLoadout`,
  `BotPersonality`): the file holds only what the menus edit, under names
  this crate owns; conversions name every field, so a field added to
  `DeathmatchSettings` does not compile until it is mirrored.
- **Loadouts** (`loadouts.rs`, issue #5; not OLX's): `OwnLoadouts {
  mods: BTreeMap<mod id, Vec<MatchLoadout>> }`, the player's own, in the
  data directory's `LOADOUTS_FILE` (`loadouts.ron`, through
  `Store::load_file`/`save_file`; `UiState::set_loadouts` at start,
  `UiAction::SetLoadouts` → `AppCommand::SaveLoadouts` when changed);
  `sanitize` never drops a loadout (a name cleaned, `Default` or a
  repeat ignoring case numbered, the loading time clamped; one warning
  each). Weapons are kept by name as saved: a weapon a mod update removed
  stays in the file and is left out only when played. `choices(m, own)`:
  `Default` and the mod's shipped ones (`ModInfo::loadouts`, read-only),
  then the own ones (one under a shipped name, ignoring case, is not
  listed); `resolve(name, m, own)`: the loadout to play and what was wrong
  (an own one fitted to the mod's weapons; `Default` when the name is gone
  or an own one keeps no weapon); `check_name` (unique per mod ignoring
  case, shipped names and `Default` refused), `free_name` ("Mine 2"),
  `explicit` (the weapons written out: `Default`'s empty list is every
  weapon), `MAX_OWN_LOADOUTS` (64 a mod, the editor's cap).
- **Screens** (`screens/`): `show(ui, &UiState, &mut MenuData) ->
  Vec<UiAction>` draws the current screen; the caller feeds the actions to
  `update`. `MenuData` holds the `olr_content::Catalog`, the replays
  directory and its files (read when the screen opens), the level
  thumbnails as egui textures (decoded only when their card is in view, at
  most `THUMB_DECODES_PER_FRAME` a frame, each once, shown letterboxed;
  an unreadable one keeps its placeholder and is reported once through
  `take_warnings`, which the app logs) and the skin preview (`preview_wanted`, set
  by the profiles screen each frame; `preview`, the app's textures).
  **The page frame** (`mod.rs`, every screen the main menu opens):
  `page(ui, title, width, out)` returns a column at most `width` points
  wide and `PAGE_HEIGHT` (900) high centred in the window (the window less `PAGE_MARGIN` each side
  when narrower), the title centred on top, Back at its left edge;
  `footer` puts the primary action (`primary`: a large button; Start) and its status centred at the column's bottom and returns the
  rest; `columns` splits that rest (egui's own `columns` would reach
  under the footer); `tab_row(ui, size, n)` sizes and centres a screen's
  tab row (Options, Multiplayer), each tab an equal share of a column too
  narrow for `size` (ruling UI-1: 427 points wide at UI scale 3 in a
  1280 × 720 window); `section`/`tall_section` group widgets in
  `theme::section_frame` panels under a title (the title's id may label a
  picker); `form_row`/`form_check_row` put a name right-aligned in a
  `FORM_LABEL`-wide column so a form's controls start on one line. A new
  screen uses them: `tests/layout.rs` checks every sub-screen (the
  options' four tabs each) is centred at 1280 × 720 and 3440 × 1440. Main
  (`main.rs`, centred on its own), local play (`local_play.rs`, 1100
  wide: the Map panel (mod, level grid with
  thumbnails or a name-and-size placeholder), the Match panel
  ("Loadout": `Default` first, then the mod's, then the player's own,
  and "Edit loadouts…"; then lives, kill limit, time limit), the
  Players panel (the player's profile, Bots, Difficulty, "Advanced…"),
  Start with "Loading…" and the failure message (issue #5 made it
  simpler: no loading time, preset, bonuses or reset); a saved
  mod or level no longer imported becomes the first listed; the
  loadout is looked up again by name every frame, `screens::loadouts::
  refresh`, `Default` when gone, each problem logged once through
  `take_warnings`). **The loadout editor** (`screens/loadouts.rs`, a modal
  window, `MenuData`'s): the mod's loadouts (the read-only ones marked),
  New (a copy of `Default`), Copy, Rename ("New name", OK; refused names
  said in red) and Delete (confirmed; own ones only); the selected one's
  weapon checkboxes, All on, All off, "Loading time", "Unique weapons"
  (`UNIQUE_HINT`), greyed for a read-only one (`READ_ONLY`); every valid
  change is saved at once (`SetLoadouts`), one with no weapon on is shown
  with `NO_WEAPON` and not saved; Done, Esc or a click outside closes it
  and selects the loadout it showed. **"Advanced…"** opens the bots
  window: "Each bot its own difficulty" (then "Bot N" pickers),
  Personality, "Bots' weapons" (OLX's choice, Random), Close. Esc closes
  an open window instead of going back (`MenuData::window_open`), profiles
  (`profiles.rs`: Players, Look and Skin panels; the list, New, Delete confirmed, "Play as this", name with
  its error, colour sliders, searchable skins, the preview; it opens on
  the profile playing), replays (`replays.rs`, each file's header problem
  shown), options (`options.rs`, 720 wide: Video (with Blood, a 0-500 %
  slider applied at once, and the HUD layout),
  Audio, Keys and Online tabs,
  centred; Online (scrolling): "Finding games" (Region, Public address,
  plan 12b's "Open the game's port on my router (UPnP/NAT-PMP/PCP)", a
  checkbox of its own line so its label wraps),
  Relays and STUN servers (each row's Remove, "New relay"/"Add relay"
  with why it cannot add, in red beside it and on Add's hover: "needs
  wss:// or ws://", "needs host:port", "already listed", "the list is
  full",
  "Reset relays to defaults"; the same for STUN), "TURN relays" (the
  same, no Reset: "needs user:pass@host:port", at most `MAX_TURN`, each
  row without its password, `turn_shown`; the new one masked as it is
  typed, a `PasswordInput`, unless "Show password" is ticked: final
  review C-M5), Blocked hosts (Unblock),
  each change one `SetFinding` (relay edits reconnect the relays in use,
  the rest applies to the next game hosted or joined: the app's); then "Download cache: N MB" and Clear from
  `MenuData::download_cache` (`DownloadCacheView`: the app's bytes and
  whether Clear may run, else why, the disabled button's hover; `Err`:
  none, and why) → `UiAction::ClearDownloadCache`, which only the options
  turn into `AppCommand::ClearDownloadCache`; the match is set up in local play only, which remembers it in
  `settings.gameplay`; the UI
  scale applies when its slider is released; `keys.rs`: per action its
  keys and gamepad inputs, each removable but the last of its device, "+"
  opens a capture (`MenuData::capture`, an `olr_input::Capture` the app
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
  About (`about.rs`, §21 G9: `CREDITS`, `LICENSES`, every imported
  pack's recorded origin, `sources`: skins grouped by directory,
  `origin_text`). Esc is `Back` and
  F5 `Restart` on every screen, a held key's repeats ignored
  (`pressed_once`); in the options Esc first closes an open dialog.
  `MenuData::version` (the app's build version) shows in the main menu's
  bottom right corner (an area of its own, outside the menu's layout) and
  at About's top (the crate's version when empty).
- **Editor screen** (`screens/editor.rs`, plan 14c): Main's **Editor**
  (between Sandbox and Profiles; `Screen::Editor`, in `MENUS`): the
  recoveries first ("Recover <name>" and Discard; a damaged one only
  Discard), New level (a form: Name, Width and Height 1..=4096, default
  504 × 350, Theme from `Catalog::sources`' themes, `olx-theme-dirt` by
  default; Create disabled until valid; a form closes once sent), and every catalog level with Edit
  (`MenuData::editable`, the data directory's packs) or Edit a copy (a
  form: Name, "<Name> copy", Copy). They emit `UiAction::{EditLevel,
  CopyLevel, NewLevel, Recover, DiscardRecovery}`, which `update` turns
  into the `AppCommand`s of the same names only on the Editor screen with
  nothing playing. The app fills `MenuData::set_editor_packs(editable,
  recoveries)` and `editor_message` (a failure, in red); it opens the
  editor itself (this crate has no `olr-editor`). Tests: `tests/editor.rs`.
- **Online screens** (`online.rs`, `screens/{multiplayer,host,lobby}.rs`,
  plan 10 task 11): plain views, no networking (this crate stays free of
  `olr-net`/`olr-lobby`/`olr-discovery`; `olr-game` converts): `LobbyView`/`MemberView`
  (`seat` is `SEAT_SPECTATOR` or a player's; `phase` is `PHASE_LOBBY` or a
  match under way), `LanEntry`, `HostPrefs` (`Visibility::{Public, Lan,
  Invite}`, N43, "Anyone (Internet)", "This network", "Invite only";
  plan 10's `DirectOnly` reads as `Invite`; 2..=32 players, port;
  `settings.online` remembers the last address and the host's choices),
  `LobbyCommand`. The state machine:
  Main > Multiplayer > Host; `Host(prefs)` / `Join` / `JoinLan` emit
  `HostGame` / `JoinGame` and wait (`is_connecting`) for the app's
  `LobbyJoined` (opens Lobby) or `OnlineFailed(why)` (back, red message);
  Back while waiting gives up (`LeaveOnline`); in Lobby the `Lobby*`
  actions become `AppCommand::Lobby(LobbyCommand)`, Back / `LeaveLobby`
  emit `LeaveOnline` and return to Multiplayer. **Host lost** (plan 12c
  Task 9): the app's `HostLost(why)` (the host gone and nobody could take
  the match over, this host stalled and stepped down, or this host heard
  none of its players, ruling SB-2's `CUT_OFF`) while online ends
  the match (`EndMatch`) and opens `Screen::HostLost` (`host_lost.rs`, a
  page: `HOST_LOST_TITLE` "Host lost", or `STEPPED_DOWN_TITLE` "Match
  lost" when `MenuData::host_lost_hosting` (this player's own hosted
  match left: its stalled game stepped down, or its network cut it off), `MenuData::host_lost` (the app's reason), then
  `HOST_LOST_REPLAY` ("The match's replay is saved in Replays.") once
  `MenuData::host_lost_replay` (the app sets it when the file is written),
  and Replays, which opens the Replays screen; Back or Esc returns to
  Multiplayer); ignored outside an online game. `MatchLeft` (the app: a
  late rejoiner's match is over before it could watch) ends the online
  match (`EndMatch`) and shows the Lobby, the game still on. A lobby
  whose player took the match over is an ordinary host's (Start, Force
  start, kick, the setup), its `listing` `MIGRATED_LISTING` ("Taken over
  from the host: closed to new players"). The
  host's Start and Force start stand one above the other in a column too
  narrow for both, and a member's Kick, Ban and move buttons wrap (UI
  scale 3 on a 4:3 desktop). The lobby reuses Local
  Play's panels (`local_play`'s `rules`, `level_grid`, `mod_picker`, the
  loadout picker: the host edits and has "Edit loadouts…", others see
  them greyed with the host's loadout's name), bots 0..=MAX_BOTS
  (Add/Remove bot, Difficulty and the same "Advanced…" window),
  "Your weapons" (Random, or the weapon screen: `Open(WeaponSelect)` from
  the lobby, whose Done answers `Loadout`, Back returns). Start (host,
  `can_start`'s reason its tooltip) and Force start (two worms and a
  member: `LobbyForceStart`), else "Waiting for the host"; a Back while
  `phase != PHASE_LOBBY` first asks "Leave the match?" in the widget
  (`lobby::intercept_back`). A locked LAN entry copies its address to the
  Direct tab instead of joining. Tests: `tests/online.rs`; a freshly
  opened page needs `settle` (two extra frames) before a pointer click.
  **Finding games** (plan 12a Task 10): the Multiplayer screen (900 wide)
  has three tabs: **Internet** (`MenuData::internet`, an `InternetView`
  the app keeps current: `InternetStatus::{NoRelay, Searching, Listed}`,
  "No relay reachable", "Searching…", "No games found"; an aligned
  grid with headers of `InternetEntry` when its columns (each its widest
  text, up to a cap, truncated with a hover) fit `ui.available_width()`
  (`table_widths`; the page is 1100 wide), else each game on two lines so
  it fits 320 points: the
  name (truncated, its hover has all; a lock before it), then map, mod,
  players, ping and region wrapping; the tab scrolls in a window too
  short for it; the name (selects: `MenuData::selected_game`, which the app
  pings), map, mod, players (`players_text()`: the seats taken of the
  seats, the bots apart, "1/8 +3 bots", "1/8 +1 bot", "1/8" without; the
  user's report of 2026-10-15: bots take no seat), ping (`PingView`), region, a lock; another
  version greyed with "needs version …"; plan 12b: one without an address
  is joinable (through the relays, the app's), its ping column
  `VIA_RELAYS` ("via relays"); the selected game's "Game password", Join
  (`JoinListed { addr: Option<SocketAddr>, key, password }` → `JoinGame {
  addr: Option<String>, expected_host: Some(key) }`, not remembered;
  disabled with its reason on hover: another version, full: `is_full()`,
  the seats taken, bots never) and "Block this host" (`BlockHost`
  → `AppCommand::BlockHost`, the hex key saved in
  `online.finding.blocked`)), **LAN** (today's list; `LanEntry`'s
  `players_text()` and `is_full()` as the Internet tab's) and **Direct**
  ("Address or invite code": `Join` as before, `is_invite` text, `olr1`
  any case or at least 60 letters and digits (a typo in the prefix: the
  app then says "damaged", never looks it up as a name), said to be an
  invite; the app decodes it). While a join waits, the footer says its
  step (`MenuData::joining`, the app's `JoinStep` in words), else
  "Connecting…". The Internet tab's password is cleared
  when the selection changes (`View::password_for`: one game's password
  is never offered to another, final review B-M7). `FindingSettings`
  (`online.finding`): `region` (`Region`, olr-discovery's mirrored), `relays`
  and `stun` (`None`: the app's defaults, `MenuData::default_relays`/
  `default_stun`; `valid_relay`, `valid_stun`, at most `MAX_SERVERS`),
  `public_address` (empty: none; plan 12b's N47: a manual forward's
  address), `blocked` (64 lowercase hex, `key_hex`/`parse_key_hex`, at
  most `MAX_BLOCKED`), `port_mapping` (default true, an older file's too:
  a manual `Default`), `turn` (the player's TURN servers, `user:pass@host:port`,
  `valid_turn`/`turn_parts`: the user before the first `:`, the password
  up to the last `@`; at most `MAX_TURN` 8; none by default, N52);
  `sanitize` drops bad entries, one warning each (a TURN entry is never
  quoted in it); `Debug` is written by hand: TURN entries as `turn_shown`
  (`user:•••@host:port`, "•••" for one that is not an entry), so no
  printed setting, action or command holds a password; `SetFinding` saves. The lobby shows
  `LobbyView::invite` with Copy (`ctx.copy_text`), `listing` (a Public
  game's listing line, an Invite game's "This code joins through the
  relays" …) and `reach` (plan 12b: the host's reachability lines, under
  `listing`, wrapping in the column) in an "Invite" section over the
  setup, the host's only; a client's `connection` ("connected directly to
  …") in a "Connection" section; each member's `path` tag ("direct",
  "open port", "punched", "relayed": how the host reaches it) beside its
  ping.
  **Online matches** (plan 10 task 12): while online the app's
  `MatchStarted` (from any screen) is `Game::Online` in game, no weapon
  selection (the lobby's); Esc opens the match's menu ("Menu", the match
  goes on; its "Leave the game" emits `EndMatch`, `LeaveOnline` and goes
  to Multiplayer); `Restart`, Rematch and Save replay do nothing (the app
  saves every match's replay); `MatchOver` → Results, whose "Back to
  lobby" emits `EndMatch` and returns to the Lobby; `OnlineFailed` from
  any screen ends the match (`EndMatch`) and goes to Multiplayer;
  `LobbyChat` works in game and on the results too; `take_seed` gives the
  lobby's start its seed; `restore_online` puts the online settings back
  without saving (the app's command line flags). In game the chat field
  over the playfield's bottom (`MenuData::ingame_chat`, `ingame.rs`; the
  app opens it with `settings.online.chat_key`, `CHAT_KEY` T by default,
  never one of `GAME_KEYS`: sanitized): Enter sends (`LobbyChat`) and
  closes, Esc or a click elsewhere closes, and Esc there is not Back
  (`show`'s typing check covers InGame); the match's menu closes it.
  The Keys tab rebinds it (`UiAction::SetChatKey`, refusing `GAME_KEYS`).
  A read-only lobby (a client) shows the time limit as text, not a slider.
  **Downloads** (plan 11): `LobbyView::downloads` (`DownloadView`: name,
  kind, `have`/`size`, the state in words) is a "Downloads" list under the
  members (a progress bar a row); a member row shows `downloading N%`
  (`MemberView::download`) instead of its missing content; Ready is
  disabled ("get the missing content first") while this player's
  `missing` is not empty; `LobbyView::prompt` (bytes) asks "This game
  needs N MB of content from the host." with Download
  (`LobbyDownload(true)` → `LobbyCommand::ApproveDownloads`) and Leave
  (`LobbyDownload(false)`: leaves as Back does). `megabytes(bytes)`:
  "73.4 MB", "312 MB", in MiB shown as "MB" (the prompt's 50 MiB
  threshold reads "50.0 MB").
- **Bug reports** (`report.rs`, `screens/report.rs`, issue #3): Main's
  **Report a bug** (after About; only when `MenuData::report.available`,
  the app's: the build has a relay) opens `Screen::Report` (in `MENUS`;
  `Open(Report)` emits `PrepareReport`: the app gathers the automatic
  section and the logs into `ReportData::context`). The page (900 wide):
  "What happened" (required) and "What you expected" (`MAX_TEXT_CHARS`
  each), "Attach a replay" (off by default) with a picker of
  `replay_choices(dir, catalog)` (newest first by the file's date, at
  most `MAX_REPLAY_CHOICES`, each read and decoded when the screen opens:
  date in UTC (`utc_minute`), mod and level names, the worms' names, the
  bytes kept, so what is sent is what was listed; one over
  `MAX_REPLAY_BYTES` (384 KiB) or undecodable listed disabled with why;
  the newest attachable preselected), the preview (`Report::preview`:
  the texts, every `Section`'s facts, the replay with its worms' names,
  the logs' summaries; each log's text under a collapsed header), Send
  (`UiAction::SendReport(report)` → `AppCommand::SendReport`, only on the
  Report screen and with "What happened" filled) and its status
  (`SendStatus::{Idle, Sending, Sent(url), Failed(why)}`, set by the app;
  `ReportData::finished`: a success clears the texts and shows "Thanks,
  report filed:", the address selectable and Copy; a failure keeps
  everything). The one `Report` made at the frame's start is both shown
  and sent. `Report::json` is the payload (`REPORT_FORMAT` 1:
  `happened`, `expected`, `sections`, `logs`, `replay`), the replay's
  bytes beside it (`replay_bytes`). `sections(&SystemFacts)`: Game
  (version), System (OS, distribution, kernel, window system, desktop
  session, WSL, Flatpak), GPU (adapter, backend, device type, driver,
  driver info, present mode, surface format), Display (scale factor,
  window, monitor, refresh rate), Video settings (each one a command-line
  flag set marked "(set on the command line)": `VideoOverrides`), Audio
  settings, Gameplay settings (the match as RON); never bindings, the
  online settings, profiles or the identity. `LogAttachment::new` keeps
  a log's last `LOG_TAIL_BYTES` (256 KiB) from a line's start, the home
  directory as `~`. Esc in its text fields only leaves the field. Tests:
  `tests/report.rs` (the section for fixed facts, every value sent
  previewed, the entry only with a relay, the flow with a replay recorded
  from the committed content, the logs collapsed, Send's gate).
- **Updates** (`update.rs`, `screens/update.rs`, issue #4): plain views
  the app fills (`MenuData::update`, an `UpdateData`): `enabled` (the
  build checks for updates: `cargo xtask dist` made it; else no notice and
  no option), `build_prerelease`, `flatpak` (`FlatpakChannel`: the branch
  is `beta`, the command switching branch), `offer` (`UpdateOffer`:
  version, title, notes as plain text, the release page, `UpdateMethod::
  {Portable, Portal, Command(cmd), Page}`) and `status` (`UpdateStatus::
  {Idle, Working { step, progress }, Installed, Failed(why),
  NotWritable(why), Outside { why, command }}`). Main shows "Update
  available: <version>" (`screens::update::NOTICE`) over its buttons, a
  primary-coloured button, only with an offer in an enabled build; it
  opens `Screen::Update` (in `MENUS`): the release's title, "Version X
  (you have Y)", the page as a link, "Release notes" (selectable,
  scrolling), and in the footer Update (`UiAction::InstallUpdate` →
  `AppCommand::InstallUpdate`, only on that screen; disabled while
  `Working`, with the step and a progress bar or a spinner below; a
  failure in red, Update retries), `INSTALLED` and "Restart now"
  (`RestartGame` → `AppCommand::RestartGame`), "Open the release page"
  (`OpenUrl(url)` → `AppCommand::OpenUrl`, `https://` only, the Update
  screen only) for `NotWritable` and `Page`, and a selectable command with
  Copy (`command_line`) for `Outside` and `Command`. Back returns to Main:
  nothing is forced. The options' Online tab ends with "Updates" (enabled
  builds only): "Include pre-releases" (`UpdateData::includes_prereleases`:
  `settings.updates.include_prereleases`, `UpdateSettings`, `None` until
  changed = the build's own channel) → `SetIncludePrereleases(on)`
  (saved); under Flatpak the checkbox shows the branch's channel and
  toggling it shows the switch command instead (nothing saved). Tests:
  `tests/update.rs`; `tests/layout.rs` checks the screen and its primary
  action.
- **Replays** (`replays.rs`): `list(dir)` (the `.olrp` files, automatic
  saves newest first, each with `header_problem`: 6 bytes read),
  `auto_save(dir, bytes)` (`match-NNNNNN.olrp`, the last
  `MAX_AUTO_REPLAYS` kept; a player's own files are never removed),
  `save_kept(dir, bytes)` (the results' Save replay: `replay-NNN.olrp`).
- **HUD** (`hud/`, §21 G5; one style, the Classic one was dropped): `model.rs` (names, OLX's
  message texts, `Log` of `LogLine { text, kind: LineKind::{Event, Chat} }`
  (the last `LOG_KEEP` 64; `Chat` is for a multiplayer chat later),
  `clock`, scoreboard `rows(scores, names, followed)` with the followed
  worm's `Row::followed` (yellow in `table`) and without the rows of
  worms that left; `Results::of`/`new` keep them (plan 12d, N85: `Row::left`,
  lives "left", kills and deaths so far, grey in `table`; ranked as the
  mode orders them, an out worm's place); test
  `a_left_row_is_off_the_scoreboard_and_left_in_the_results`), `view.rs`
  (`HudView`, the frame's data; `WormHud::of(world, i, names, ..)`: health,
  lives, kills, slots with OLX's bar states), `layout.rs`
  (`HudLayout::new(screen, arrangement)`: the game is designed for 16:9:
  `frame(screen)` is the largest 16:9 rectangle centred in the window;
  its bottom fifth (`BAR_SHARE`, OLX 0.58's 98 of 480) the opaque `bar`
  (health and weapons left, the minimap centred and as tall as the bar
  at OLX's 119 × 94 proportions, the `log` right), the rest the
  `playfield` (`playfield(screen)`, 16 : 7.2, where `olr-game` draws
  `olr_scene::Viewport::GAME`), the clock top centre and the keys top
  right over it; every size is given for a frame `REFERENCE_HEIGHT`
  (720) high and scaled by `unit`, so the HUD looks the same on every
  window and at every UI scale. The side layout (`sides`: `Auto` or
  `Sides` and a quarter of the frame's height free on each side,
  `SIDE_SHARE`; 21:9 and wider): the health and weapons, larger, in the
  column left of the frame, the log, taller, in the one right of it; the
  frame, bar, playfield and minimap unchanged. The user's rulings after
  the v0.1 playtest: Review Focus 5, `hud_layout_holds_at_all_scales`),
  `minimap.rs` (`image`: the colour terrain box-filtered to
  `TEXTURE_SIZE`; OLX's dot rectangles and blink, no view rectangle (the user's ruling);
  `minimap_scale`),
  `paint.rs` (`show(ui, &view)`: the bar (and the side columns, opaque),
  then its parts, the log's newest line at the bottom, a long one
  wrapped; nothing fades; a worm without slots shows no weapon
  rows; the scoreboard centred over the playfield; `table`). The weapon
  selection, the pause menu and the results centre over the playfield
  (`screens::playfield`). Image snapshots at 1080p, 1440p and the side
  layout at 3440 × 1440 (`tests/hud_snapshots.rs` against `fixtures/ui/`, in the GPU tier
  with `OLR_GPU_TESTS`; `cargo xtask bless-render --reason` re-records them, logged).
- **Weapon selection** (`weapon_select.rs`, §21 G4): `WeaponMenu` (names,
  enabled, `findEnabledWeapon`'s fallback, seed, `unique`: the loadout's
  rule; `needs_choice`),
  `WeaponSelect::new(menu, remembered)` (the profile's names; under the
  unique rule a repeat becomes the first enabled weapon no slot holds),
  `change` (OLX's loop over the enabled weapons; under the unique rule
  over those no other slot holds, unless every enabled weapon is in one), `random` (`get_random_weapons`),
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
  `LoadedSkin::colorized` with `olr_content::skin_sprite`: the game's own
  colorization and frame cut (`skin_preview_uses_colorization_like_the_game`
  compares them with `olr_scene`'s). `PreviewAnim` is OLX's menu animation.
  The app uploads the frames (`PreviewTextures`): no GPU handle here.
- **Store** (`store.rs`, Review Focus 1): `Store::platform()` (the
  `ProjectDirs` data directory, else `in_memory` and a warning;
  `replays_dir()`, `packs_dir()`: `packs/`, the user packs layer (written by `olr-cli import`); `cache_dir()`:
  `ProjectDirs::cache_dir()/downloads`, a `Store::open(dir)`'s
  `<dir>/cache/downloads`, none in memory: plan 11's download cache),
  `Store::open(dir)`, `load_*` (`Loaded { value, warnings }`) and
  `save_*`; `load_file::<T>(name)` / `save_file(name, &value)` for any other
  RON file of the data directory (the editor's `editor.ron`: `(version: 1,
  value: …)`, the same atomic write, backups and newer-file rules; `None`
  when absent; a name that is not a plain `.ron` file of its own, or one of
  the store's own files, is refused); `recovery_dir()` (`recovery/`, the
  editor's copies); `textures_dir()` (`textures/`, the editor's texture
  library, plan 14e); `assets_dir()` (`assets/`, the editor's asset
  library of the user's, issue #9).
  Versioned RON (`SETTINGS_VERSION`, `PROFILES_VERSION`), every
  struct `#[serde(default)]`, atomic writes (temporary file, sync,
  rename). An undecodable file gives defaults and a warning and is moved
  to `<name>.bak`, `<name>.1.bak`, ... (at most `MAX_BACKUPS`; then the
  oldest goes and the rest move down); one that decodes but needs values
  fixed is copied there and the fixed values are saved in its place (its
  other values survive). A file that cannot be backed up is never
  overwritten. A newer game's file (version above ours) stays in place,
  untouched: defaults, and the session saves nothing to it while it is
  there; `notices()` says so, and the main menu shows it
  (`MenuData::notices`, set by the app). Files over 1 MiB are bad. Warnings are
  returned, never printed: the caller logs each once.
- **Theme** (`theme.rs`): `apply(ctx)`: dark, larger text. The scale
  follows the height, not the display's DPI (the user's ruling after the
  v0.1 playtest): `pixels_per_point(height, ui_scale)` is `height / 720 ×
  ui_scale` (the window's height in the menus, its 16:9 frame's in game:
  the app's choice), `set_scale(ctx, native, height, ui_scale)` sets
  egui's zoom to it on a display of `native` pixels per point (every
  frame; it takes effect at the end of the pass); `BACKGROUND`, `ACCENT` (section titles),
  `PRIMARY` (the primary action's fill), `section_frame()`.

Test: `cargo nextest run -p olr-ui` (`tests/store.rs`, `tests/state.rs`,
`tests/flows.rs`: kittest flows by label and the local play's match
(`local_play_is_the_simple_setup`), `tests/loadouts.rs`: the editor
through its widgets, a chosen loadout gone playing `Default`, own
loadouts saved when changed,
`tests/layout.rs`: every sub-screen a centred column, the primary action
centred below,
`thumbnails_decode_lazily_and_bounded`, `about_credits_the_authors_and_the_licenses`
(each thumbnail file in a directory of its own, in a temporary directory
per test thread: parallel tests never read a file another writes),
`tests/profiles.rs`: profiles through the widgets, names, the preview; its
last part compares with `olr_scene::SceneAssets` when `OLR_CONTENT_DIR` is
set; `tests/options.rs`: rebinding never leaves an action unreachable, the
rebinding flow and the other sections through the widgets, game keys never
bound, the capture swallowing its key, the download cache's Clear (disabled
with its reason; none), `options_edit_relays_and_unblock`, the finding
settings sanitized (TURN entries, `port_mapping` on for an older file),
`options_edit_turn_relays_and_port_mapping`,
`the_blood_slider_sets_the_players_blood_amount`; `tests/online.rs` also `tabs_and_direct_invite`,
`internet_games_join_by_key_and_block`, `the_internet_tab_says_no_relay_reachable`,
`the_lobby_shows_the_invite_and_copies_it`, `the_host_chooses_who_can_join`,
`an_addressless_internet_game_is_joinable` (Join enabled, `addr: None`,
the join's step as the waiting text), `the_lobby_shows_reach_lines_and_path_tags`,
`listed_games_show_bots_apart_and_only_seats_fill`;
`the_host_lost_screen_says_why_and_goes_back` (the reason, the replay
line only with a replay, the stepped-down title, Replays, Back and Esc,
ignored offline), `a_migrated_lobby_shows_host_controls`,
`a_match_left_shows_the_lobby` (Ready works there);
`tests/layout.rs`'s `the_host_lost_screen_and_a_migrated_lobby_fit` (4K,
3440 × 1440, 1280 × 720 at 3, and borderless 16:10 and 4:3 at UI scale
3: the Host lost screen a centred column with every widget in the
window; the migrated lobby's listing and host controls across the
window; the lobby's setup panels still overflow a 320- or 384-point
column), `the_lobbys_long_lines_wrap_in_a_narrow_window`
(1280 × 720 at 1, 2 and 3 pixels a point, 3440 × 1440 at 2),
`the_longest_join_texts_and_turn_options_fit` (the same sizes, and
borderless fullscreen on 16:10 and 4:3 desktops at UI scale 3, 384 and
320 points wide: olr-online's `no_path` as the Multiplayer screen's
failure, the Multiplayer and Options tab rows, the Online tab reached by
a click and every widget of it in the window: `form_name` takes at most
40 % of a narrow column, the Online tab's fields `fit` it, its server
rows wrap), `the_internet_table_fits_and_a_game_can_be_joined` (the
same sizes and 4K, headers on wide screens, two lines on narrow: long names, "8/8 +24 bots (playing)", every region;
every widget in the window, a game selected and joined);
`tests/ingame.rs`: the HUD layout,
the weapon selection, the pause menu, Esc/F5 repeats, the results;
`tests/hud_snapshots.rs`
with `OLR_GPU_TESTS`).
- **Identity file** (`identity.ron`, plan 10): `Store::load_identity(fresh)`
  returns the install's 32-byte secret key, calling `fresh` (the caller's
  randomness) and writing `(version: 1, secret: "<64 hex>")` atomically
  when there is no file; a damaged file is moved to `.bak` and replaced by
  a new identity with a warning (the fingerprint changes). The store does
  not check the scalar: `olr-lobby`'s `Identity::from_secret` does, and the
  caller then asks for a fresh secret.
