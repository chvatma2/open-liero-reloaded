# olr-game

The game binary (spec §8.6, §19 E10-E12, §21): a winit window, the 100 Hz
fixed-tick loop, interpolated wgpu rendering with cosmetic entities, sound,
input, an egui overlay and the menus (`olr-ui`). It plays the M5 sandbox
(E11: the player and N idle targets on a level with a mod's first five
weapons; the editors' Play and the command line's start, not the main
menu since issue #33), a Deathmatch against bots (M6) and replays.

```
cargo run --release -p olr-game                      # the menus
cargo run --release -p olr-game -- --level "CastleStrike" --mod "MW 1.0" --targets 3
cargo run --release -p olr-game -- --mode deathmatch --bots 4 --difficulty medium \
    [--locals N] [--lives L] [--kill-limit K] [--time-limit M] [--seed S] [--record f.olrp]
cargo run --release -p olr-game -- --replay f.olrp [--speed 1|2|4|8|16|max]
cargo run --release -p olr-game -- --spectate --bots 8 [--speed 16]   # bots alone, watched
cargo run --release -p olr-game -- --host [name] [--port 23400]        # host online, the Lobby opens
cargo run --release -p olr-game -- --join 192.168.1.20[:23400]         # join online
cargo run --release -p olr-game -- --join olr1…                         # join by invite code
cargo run --release -p olr-game -- --net-sim typical                   # feel a network (spec §9.10)
```

- **Layer 6.** Uses `olr-ui`, `olr-content`, `olr-present`, `olr-scene`, `olr-fx`, `olr-render`,
  `olr-audio`, `olr-input`, `olr-sim`, `olr-modes`, `olr-bots`, `olr-replay`,
  `olr-terrain`, `olr-pack`, `olr-editor`, `olr-edit`, `olr-format`,
  `olr-vfs` (the level editor), `olr-skin-editor` (the skin editor),
  `olr-mod-editor` (the mod editor), `olr-material-editor` (the material
  editor), `olr-online`, `olr-netcode`, `olr-proto`,
  `olr-lobby` (identities, host settings), `olr-net` (endpoint settings,
  LAN announcements, `IceConfig`), `olr-discovery` (the hosted lobby's summary,
  `GameSummary::of_lobby`, and `hosting`, shared with olr-server; the
  relay pool, the game list, pings, invites), `olr-log` (the run log),
  `olr-update` (issue #4: versions, releases, signatures, the folder
  swap), plus `log`, rustls and webpki-roots (a bug report's POST, the
  update check's GETs), zbus 5.19.0 (Linux only: the Flatpak portal), winit,
  wgpu, gilrs and egui / egui-wgpu / egui-winit 0.36.2 (exact pins). Logs
  go through `log::{debug, info, warn, error}!` (below), each warning once.
- **Version** (`lib.rs`'s `VERSION`): `OLR_BUILD_VERSION` at compile time
  (`cargo xtask dist` sets it: `0.2.0-dev.192.24b2ac4`, or `0.2.0` for a
  final release), else `<CARGO_PKG_VERSION>-dev (local build)`; `olr
  --version`, the terminal's first line and the run log's (`olr
  <version>`), `MenuData::version`
  (the main menu's corner, About). Shown only: nothing compares it
  (replays, the game list's `FINGERPRINT`, the netcode stay as they are).
  `olr-server` has the same `VERSION` (`--version`, its first line).
- **Run log** (issue #2, `logging.rs`, `olr-log`'s `CLAUDE.md`): `main`
  starts it after the WSL relaunch (the parent writes none) and before
  anything else, in `<data>/logs/` (`--data-dir`, else
  `Store::platform`'s; none: the terminal alone, a warning), at `--log
  <level>`, else `OLR_LOG`, else `info` (`logging::level`); the header is
  `olr <version>` and `logging::System::line` (`os: linux x86_64, <os-release
  PRETTY_NAME (the host's under Flatpak)>, kernel …, WSL, Flatpak, <XDG
  session> session`), then `window system: Wayland|X11|…` once the event
  loop is made, `GPU …` and `window <Shape>, scale factor …; video: VSync|
  uncapped, filter, interpolation, UI scale` (`App::open`). The terminal
  keeps its lines: `olr <version>` first (printed by `main`), `olr: …` on
  stdout for `info`, `warning: …`/`error: …` on stderr (`xtask e2e` fails
  on those: nothing new warns in a clean run). `main` ends it with
  `olr_log::end()` (the clean last line) on every way out but a panic or a
  kill; an error is logged first. At `info`, besides the old prints
  (now `log::info!`/`warn!`; a caught pump panic is `error`): the
  content listed (`content: N mods, N levels, N skins from …`),
  `game start: <Assets::describe>` (kind and seed, mod, level, players
  and bots) for every game (`direct`, `Shell::launch`, `start_online`),
  `restart: …`, `match over`, `game end: …` and `saved the replay
  <path>` (`olr_ui::replays::auto_save`'s file: a log and a replay
  match), `playing the replay <path>`; online: in the lobby, leaving,
  joining steps, the path, `<name> has left`/`timed out`, the end's
  reason, kicks and disconnects (`online: …`), migration, relays and
  reachability, desync bundles (`warn`, with the file); settings saved
  (`logging::settings_changes`: video, audio and gameplay values, the keys
  and the online part only named: TURN entries hold credentials); window
  changes (`window change: Change {…}`, a window made again, the
  surface made again, the present mode, the display's scale factor;
  `Resized` at `debug`). Nothing per frame or tick at `info` but
  `--stats`' once-a-second line (asked for). Addresses and keys in `info`
  lines (joining, paths, reach) are hidden in the file, not the terminal.
  `tests/run_log.rs` runs the binary (`--edit` of a missing directory:
  no window) for the file, a crashed run reported once, an unwritable
  directory; `tests/speed.rs` gives its runs a data directory of their
  own.
- **Updates** (`update.rs`, `update/portal.rs`, issue #4; the screens are
  olr-ui's, the formats `olr-update`'s): a build checks only with
  `OLR_RELEASE_REPO` baked in (`update::BUILD_REPO`: `cargo xtask dist`
  sets it) and a version that parses (`releases_url`; a local build:
  `Updater::of_build` is `None`, `MenuData::update` stays default, no
  notice, no option). `Shell::new` → `set_updater(of_build())` (tests: an
  `Updater::start` on a mock); `OLR_UPDATE_API` at run time replaces
  GitHub's list (`https://api.github.com/repos/<repo>/releases?per_page=30`)
  in such a build. The check (`fetch_releases`, thread `olr-game update`,
  `CHECK_TIMEOUTS` 10 s connect, 15 s per read) never delays the menu;
  its failure is one `info` line (`update check: none (…)`), never shown.
  `Updater::poll` (every `Shell::poll`): the list, then the offer for the
  setting (`olr_update::channel`: under Flatpak the branch's; made again
  when the setting changes, never while installing or once installed),
  the notes of every release it skips (issue #22: `release::since` of
  the list already fetched, then `notes::changes`' headings, texts and
  `left_out` into `UpdateOffer::changes`), its `UpdateMethod` by `Install::detect`
  (`Portable(folder of olr.exe)` on Windows, `Flatpak(/.flatpak-info)`:
  `Portal` on `stable`/`beta`, `Command(switch_command)` on an older
  bundle's `master`; else `Page`). `AppCommand::InstallUpdate` →
  `Updater::install` (thread `olr-game install`, `Event`s into
  `UpdateStatus`): **Windows** `install_portable`: refuses with an
  unfinished swap's journal, `check_writable` (else `NotWritable`, the
  release page), the `<zip>.sig` asset (`manifest::verify` against
  `manifest::release_key()`; another key's, unsigned, another version's
  or file's refused), the zip into `.olr-update/<zip>.part` hashed as it
  comes (`DOWNLOAD_TIMEOUTS` 60 s per read, at most the signed size and
  `MAX_ZIP_BYTES`), `Manifest::check` (version offered and newer, size,
  SHA-256), `install::stage_zip`, `Plan::new`, `install::swap`; every
  failure before the swap leaves the folder as it was. **Flatpak**
  `install_flatpak` over a `Portal` (`portal::Dbus`: `CreateUpdateMonitor`,
  the monitor's `Progress` listened to before `Update("", {})`, then
  `Close`): `Done` is installed, `UpToDate` "no newer build yet",
  new permissions (`needs_permissions`), another failure or no portal →
  `Outside` with `FlatpakInfo::update_command`. `RestartGame` →
  `Updater::restart` (the new `olr.exe` with the same arguments; under
  Flatpak `portal::spawn_latest`, `Spawn` with
  `FLATPAK_SPAWN_FLAGS_LATEST_VERSION`, argv `olr` and the arguments,
  NUL-terminated byte strings), then `Outcome::Quit`; a failure is said
  on the screen. `OpenUrl` → `update::open_url` (`https://` only:
  `explorer`, `open`, `xdg-open`). `main` calls
  `update::finish_interrupted_update()` right after the log starts
  (Windows `dist` builds): `install::recover` finishes a cut-short swap
  (when this process is the old build it starts the new `olr.exe` and
  exits) or removes the last update's leftovers. `http::get`: redirects
  (at most `MAX_REDIRECTS`, never `https` → `http`), `Content-Length`,
  chunked or to the close, streamed to a sink with its progress, at most
  `max` bytes, a non-2xx status an error. Tests: `tests/update.rs` (a
  mock on 127.0.0.1 with `https://github.example` mapped to it: every
  framing and redirect, a silent server bounded; a signed release
  downloaded and swapped in, the player's file kept, an old one removed;
  another key's, a changed zip, an older release's signature, none, a cut
  download refused with the folder unchanged; a read-only folder; the
  fake portal's outcomes; the shell: a local build shows nothing, the
  offer following Include pre-releases (no downgrade) with the notes of
  both builds skipped, newest first, Update off the UI
  thread to Installed; an unreachable GitHub shows nothing), unit tests
  (`releases_url`, each install's method).
- **Bug reports** (`report.rs`, `http.rs`, issue #3; the screen
  is olr-ui's, the relay `tools/bug-relay/`): the relay's address is
  `OLR_REPORT_URL` at run time (a local mock in tests), else the build's
  (`option_env!`, `report::BUILD_URL`: `cargo xtask dist` passes it on);
  neither: `MenuData::report.available` false, Report a bug hidden
  (`Shell::set_report_url`). The screen's `PrepareReport` is
  `Outcome::PrepareReport`: `App::report_environment` (the GPU's
  `Gpu::facts`: adapter, backend, device type, wgpu's `driver` and
  `driver_info`, present mode, format; the window's scale factor and
  size, `current_monitor`'s size and refresh rate; `window::system_name`)
  → `Shell::prepare_report` (`report::facts`: `logging::System::read`,
  Flatpak also by `FLATPAK_ID`, `VERSION`, the settings in effect with
  `video::Overrides` marked, `report::overridden`; `report::logs`: this
  run's file after `olr_log::flush()` and `olr_log::run::previous`'s with
  its end, `run::ended`, each's last 256 KiB, the home directory as `~`;
  no log file is a warning on the screen). Send (`AppCommand::SendReport`)
  starts `report::Sending` on the `olr-game report` thread (one at a time;
  status `Sending`), `Shell::poll` takes its outcome into
  `ReportData::finished`. `report::body`: `multipart/form-data`, part
  `report` the report's JSON, part `replay` the file's bytes unchanged
  (a boundary found in neither). `report::http`: one HTTP/1.1 POST,
  `Connection: close`, over rustls (aws-lc-rs, `webpki-roots`) for
  `https://`, plain for `http://`; the connect `TIMEOUTS` 10 s per
  address, the exchange 45 s in all, the answer at most 64 KiB
  (`Content-Length`, chunked, or to the close). `report::outcome` puts
  the relay's `{"ok":true,"url"}` / `{"ok":false,"error"}` (or the status)
  in words: `rate_limited`/429 is `RATE_LIMITED` ("Too many reports right
  now, please try again later."), `too_large`/413, `invalid`/400 (its
  `detail`), `upstream`/502-504 ("GitHub did not take …"), others "The
  report server answered N …", no connection "Could not reach the report
  server (…)". The log says `bug report: sending (N bytes …)`, `filed as
  <url>` or `not sent: <why>`, never the text. Tests: `tests/report.rs`
  (a mock relay on 127.0.0.1: the multipart request, each failure and a
  silent server bounded, the menus' Send returning at once with a second
  Send ignored, the text kept on a failure, the logs carried),
  `report`'s and `report::http`'s unit tests.
- **Menus first** (`main.rs`, `shell.rs`, plan 7 task 2): without a flag
  that chooses a game (`GAME_FLAGS`: `--mode`, `--replay`, `--level`,
  `--mod`, `--targets`, `--seed`, `--script`, `--demo`, ...) the game opens
  the main menu, with `settings.ron`/`profiles.ron` and the own loadouts
(`loadouts.ron`, `olr_ui::LOADOUTS_FILE` through `Store::load_file`,
`UiState::set_loadouts`; issue #5) from the data directory
  (`olr_ui::Store::platform`; warnings logged; `Store::notices`, a newer
  game's file left untouched, on the main menu) and the first match seed
  from the clock (each seed is in its replay). With such a flag it plays
  that game at once as before (scripted runs and tests: Esc quits, F5
  restarts, `--record`). `--shot`, `--autoquit-after` and `--stats` work
  in both. `Shell` holds the `UiState`, the menus' data, the store and the
  game playing (`Playing { assets, session }`); `app.rs` draws
  `olr_ui::screens::show` over the overlay, feeds the actions to
  `Shell::act` and does the `Outcome`s (`app/menus.rs`: quit, bindings,
  video through `video::Video` and the paced window requests, a started
  game's upload). A
  start is loaded by `Shell::poll` after the frame that showed "Loading…";
  a failure (a pack missing or broken: `setup::load_match`,
  `load_replay`) returns to the menu with the reason (Review Focus 3).
  A replay file (a typed path too) is read by `setup::read_replay_file`:
  a regular file only (a FIFO never blocks), its header checked first, at
  most `MAX_REPLAY_BYTES` (twice `MAX_TICKS` × `MAX_WORMS` key bytes plus
  16 MiB).
  Esc and F5 are the menus' there: Esc pauses (ticks stop; the pause menu
  resumes, opens the options or leaves), F5 restarts in place. A
  Deathmatch starts on the weapon selection (`shell::weapon_menu` gives
  `MenuData::weapons`; skipped when there is no choice): no tick until
  Done, whose `ArmPlayer` puts `Player::HumanWith` in the assets' options
  and rebuilds the world (`Session::rebuild`). `Shell::tick(keys)` ticks
  only when neither the pause nor the selection holds the game (the loop
  calls it; replays record ticks only). The app feeds the game's keys
  held (`MenuData::held`) every frame. Results show → `MatchOver`
  (`Session::results_shown`; `MenuData::results` set); Save replay keeps
  `Session::replay` with `olr_ui::replays::save_kept`. Every Deathmatch's replay is saved in the
  data directory's `replays/` when it ends, restarts or the game quits
  (`olr_ui::replays::auto_save`, none of no tick); the Replays screen plays
  them. Settings (and profiles and own loadouts: `AppCommand::SaveLoadouts`
→ `Store::save_file`) are written at most once a second (`Shell::flush`) and
  at exit. The human's profile names and colours the local slot's worm
  (`PlayerLook`, `Assets::set_player_look`: the first local player's) and
  its skin is that worm's; each local player is a `setup::LocalPlayer`
  (`Assets::locals`, in local order: `look`, `skin`, `skin_id`,
  `skin_hash`; `load_match_for(roots, options, skins)` gives player `k`
  `skins[k]`, else the first, `load_match` one skin for all); several
  players without a profile are "Player N" in `olr_present::default_colour(k)`,
  one alone "You"; every
  other worm wears OLX's default skin (`Assets::bot_skin`, as OLX's bot
  profiles) in OLX's bot colours away from every player's
  (`olr_ui::bot_colours_away`). A Deathmatch's recorder gets those same names
  and colours, each player's skin pack and hash and the default skin for
  the bots (`session::replay_looks`); a replay plays with its recorded
  names, colours and skins (`load_replay` fills `Assets::slot_skins` with
  `Content::skin_matching(id, hash)` for every worm, there being no own
  slot: a skin this install lacks, or has with other content, is the
  default skin, with a warning). Weapon selection's Done arms the first local
  slot; with several local players (issue #32) `ArmPlayers` arms each
  local slot with its player's choice in one rebuild (`Shell::arm`; a
  player without an enabled weapon keeps the random draw), and a match
  without a choice skips every player's selection.
  **The join step** (issue #32; the screens, the lineup and the seats'
  rules are olr-ui's `join.rs`): a match started from the join step
  (`UiState::lineup`) loads with `load_match_for`, local player `k`
  wearing its profile's skin and look (none: "Player N" in the default
  ones); `Shell::local_seats()` (the lineup's seats while that match
  plays, one joined player's too; none for a match started without the
  join step, the sandbox, replays, online, the editors' Play) is what the app seats the players on
  (`LocalInputs::fit_to`). Every frame the app gives the menus each
  seat's input (`LocalInputs::menu_seats` → `MenuData::set_seats`), each
  player's keys (`MenuData::held_by`, the weapon selection) and the
  joined pads (`PadNav::set_players`: only their Start is the pause's);
  `about_to_wait` tells the lineup of a pad plugged in again
  (`take_reseated` → `UiAction::Reseat`, logged `Gamepad N plays from
  Gamepad M now`). Test: `tests/join.rs`.
  The menus list and play `--content` with the data directory's `packs/`
  over it (`Content::open_layers`, spec §9.4), and OLR's own packs
  between them (`olr_content::bundled_roots`, issue #7: `content/reloaded`
  beside `content/packs`, the Reloaded levels; the command line's games
  use those two bundled layers alone; `--edit` refuses a pack in either, `edit_target`);
  without the flag `olr_content::default_content_dir`:
  `content/packs` beside the executable, else in the working directory). The shell keeps the layer's directory itself (`packs_dir`, `Store::packs_dir()`): `Shell::roots()`, no
  import screen (`olr-cli import` writes the packs; `tests/user_packs.rs`).
  `Shell::update_preview(ctx)`, after the menus are drawn,
  uploads the profiles screen's skin preview (`MenuData::preview_wanted`,
  `olr_ui::skin_preview::frames`) as egui textures (`MenuData::preview`,
  from the next frame), loading the skin once while it stays the same.
- **Options** (`video.rs`, plan 7 task 4): `Video::change` asks the
  window requests for a size or mode only when the preset or fullscreen
  changed (fullscreen keeps its size for later) and returns the rest to
  apply at once (`Live`: UI scale; VSync or uncapped, `Gpu::set_uncapped`
  re-configuring the surface when the present mode changes; the filter,
  `renderer.settings`); interpolation is the shell's
  (`Shell::interpolation`, new games and the one playing), and so is the
  blood amount (issue #6: `Presentation::set_blood` for every game the
  shell starts, local or online, and at once for the one playing; the
  command line's games keep 100 %; `video.rs` asserts the slider's end is
  `olr_fx::BLOOD_MAX`). The window opens
  at the saved preset and mode (`Video::opening`, physical pixels;
  fullscreen keeps the preset for when it ends), so no resize follows its
  creation. `--fullscreen`, `--uncapped`, `--filter`, `--no-interp`,
  `--hud-layout` given
  on the command line win for the run (`video::Overrides`, applied before
  the `UiState` is made, so the options show them); `Shell::set_overrides`
  keeps the file's values for them until the player changes them in the
  options. Volumes: `master * sfx` to the menus' or the game's
  `Audio::set_volume` at once (`Shell::volume`). While the options capture
  an input, the app feeds `MenuData::capture` every window and gilrs event,
  and egui hears no key, nor while it is a mouse input (issue #25) a
  mouse button or the wheel (`app::egui_hears`). **Menus by pad** (issue
  #31, olr-ui's `pad.rs`): every gilrs event also goes to
  `MenuData::pad` (`handler.rs`), and a frame that draws the menus adds
  `MenuData::pad_events` to egui's input before the pass (`App::frame`);
  one that draws an editor, its Play or the skin or mod editor calls
  `pad.forget()` instead.
- **Mouse cursor** (the user's note, 2026-10-19): hidden while a match
  plays with nothing open (`app::cursor_hidden`: the game has the screen,
  not the editor's Play with its Stop button, not the in-game chat), by
  handing egui-winit `CursorIcon::None`, which shows it again with egui's
  next icon; play uses no mouse but mouse controls' (`tests/watch.rs`,
  `the_cursor_hides_only_in_play`).
- **Local players' inputs** (`players.rs`, issue #29): `LocalInputs`
  holds one `olr_input::InputState` per local player, fitted to
  `Session::locals` each tick (`fit_to(n, Shell::local_seats())`: the
  join step's seats, issue #32, else `fit(n)`; made afresh when they
  change). One player alone is `InputState::new` (keyboard 1, the mouse,
  every pad, as before seats); among several, each on its lineup seat
  (keyboard 1 first: the mouse's player is local player 1, its view the
  first, so mouse controls' pointer stays in it; a lineup without
  keyboard 1 has no mouse controls), or for the command line's
  `--locals N` player `k` on `SEATS[k]` (keyboard 1 with the mouse,
  keyboard 2, pad 0, pad 1). Every window, key and gilrs event
  reaches every seat and each hears only its own devices (OLX
  `CClient.cpp:1527-1551`), so one player's keys never reach another's
  worm; any seat's pad Start pauses (`take_pause`). The first is
  `first()` (mouse controls, the online player, the selection);
  `held()` gives the overlay every player's CHANGE and the weapon
  selection each player's keys. It also holds the menus' seats (an
  `olr_input::Seats`: keyboard 1, keyboard 2, every pad heard;
  `menu_seats()`, the join step's). **Unplugged** (issue #32): a joined
  pad's `Disconnected` releases its player's keys (its worm stands
  still) and the player waits for it (`unplugged()`); the same gilrs id
  coming back plays on; another id takes the seat back by the gilrs
  UUID it connects with (`pad_seen`, the app's for every gilrs event,
  from `connected_gamepad`), else the first pad no player has that
  presses a button (that press only claims it); `take_reseated()`.
- **Mouse controls** (issue #24, experimental; the controller is
  `olr-input`'s `mouse.rs`): Options → Keys' switch (`olr_ui::MouseSettings`,
  `Outcome::MouseControls` → `InputState::set_mouse_controls`, also at
  start from the settings). `pointer.rs`'s `Pointer`: the game has the
  pointer while `pointer::wanted` (mouse controls on, `cursor_hidden`, no
  weapon selection, focused); `Pointer::update` (each frame, after the
  cursor icon) grabs it `Locked` (the virtual cursor moved by
  `DeviceEvent::MouseMotion`, `handler.rs`'s `device_event`), else
  `Confined` (the window's `CursorMoved`), else none (`Refused`, warned
  once: the free cursor), always clamped to the drawn view (`dest`); it
  is freed when not wanted, at focus loss (`on_focus`) and with a window
  made again (`window_replaced`), and grabbed again once wanted; the
  mouse buttons go up when it is freed (`release_mouse`). While it is
  active the window's mouse events go to `InputState::on_mouse`. Each
  drawn frame gives it the view and the local worm as drawn
  (`hud::worms_on_screen`, physical pixels and pixels per game pixel);
  `Pointer::aim` (cursor minus drawn worm, in game pixels) is the
  controller's cursor for the frame's ticks: a local tick's
  `latch_tick_for(LocalWorm::of(world, local, 0))` (`App::local_worm`),
  an online tick's from the `KeysFor` the peer gives (its world, slot and
  input delay). The overlay draws the target marker at
  `Pointer::marker` (`overlay::target_marker`). The autoplay sends no
  game mouse: the manual playtest is in `docs/deferred-verification.md`.
- **Single thread** (spec §8.6): the device is created and used only on
  the main (event-loop) thread; under WSL's d3d12 GL driver a thread that
  used the device crashes when it exits. kira runs its own audio thread;
  each online peer's network pump on its own (`olr-game net`, below),
  which touches no device either.
- **WSL** (`wsl.rs`): on a WSL kernel, unless the user set `WGPU_BACKEND`
  or `GALLIUM_DRIVER`, the game starts itself again with
  `GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl` (Mesa reads the variable when the
  driver loads, and `set_var` is unsafe). With NVIDIA's WSL driver present
  (`/usr/lib/wsl/lib/libcuda.so.1`) it also sets
  `MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA`, unless the user set it: Mesa
  otherwise takes the first adapter Windows lists, often the CPU's
  integrated GPU. WSLg presents through GL at the
  compositor's 60 Hz with FIFO only.
- **Playfield and scale** (`app.rs`, the user's rulings after the v0.1
  playtest): the game is designed for 16:9. `playfield(size)` is the
  window's centred 16:9 frame above the HUD bar (`olr_ui::hud::playfield`,
  whole pixels); `Gpu::draw` renders the world there
  (`Renderer::render_lit`, which is `render_in` without a look; black
  around it) and the egui overlay paints
  the bar (and the side layout's columns) over the rest; the names'
  `ScreenMap` uses the same rectangle (`Renderer::dest_rect_in`, issue
  #12: whole multiples of the level's own scale, whatever the skins). The
  view is `olr_scene::Viewport::GAME` (427 × 192 world pixels) for every
  window and HUD layout (spec §19 E4 amended): resizes do not touch it.
  **Split screen** (issue #30): with 2 to 4 views (`Presentation::views`,
  a draw list each, `App::lists`, `Session::build_views`),
  `playfields(size, n)` gives each view's area from
  `olr_ui::hud::SplitLayout` (the playfield's shape, smaller: the same
  world area; equal whole pixels, rounded down so none leaves the
  window; one view is `playfield`), `Gpu::draw` renders them all
  (`Renderer::render_views`, black around and between them) and the
  overlay paints a `ScreenMap` per view (`Overlay::maps`; `map()` the
  first's: mouse controls, the top lines, F3): each view's worm names,
  weapon names, reload bars and damage popups on its own picture
  (`overlay::view_labels`), the HUD's compact panels and shared minimap,
  clock and log (`olr_ui::hud::show`). Test
  `split_views_are_equal_whole_pixel_areas`.
  The UI's scale: every frame `olr_ui::theme::set_scale` with the
  display's scale factor, `scale_height(size, in_game)` (the frame's
  height in game, the window's in the menus) and the options' UI scale
  (`App::ui_scale`; 1 for the command line's games).
  `olr-render`'s goldens do not change.
- **Loop** (`app.rs`): each frame first acquires the surface texture
  (`Gpu::acquire`), so a VSync wait comes before the input is read, not
  between reading and drawing it. Then `TickClock` (`clock.rs`) runs the
  ticks owed, at most `MAX_CATCHUP_TICKS` (10) per frame, dropping the
  rest. egui hears each frame's real time (`app::egui_frame_time`, its
  `predicted_dt`: the game never asks egui for a repaint, so egui's
  `stable_dt`, which menu animations and the skin preview step by, would
  otherwise be 1/60 s at any frame rate). Per tick: script steps, the
  local players' keys (`players::LocalInputs`, below: the first latched
  with mouse controls' aim, then `latch`), `Shell::tick_locals` →
  `Session::tick_locals` (`world.tick` →
  its sounds, without others' rope throws, `audio.play_heard` at the views' centres (`Presentation::listeners`: a view per local player) → `Presentation::on_tick`, see
  `olr-present` → damage popups). Per frame: `Session::build` (scene,
  entities), the overlay, `Presentation::upload` (atlas if a worm was
  added, terrain dirty rectangles), `Gpu::present`, then a screenshot if
  one is due (one that fails is a warning, never the end of the game).
  **Lit matches** (plan 15a Tasks 13, 19): `Gpu::present` and
  `Gpu::screenshot` take a `gpu::World`, `(&[ViewDraw], Option<&LookFrame>)`
  (each view's list and area; one but in split screen, issue #30); a match passes `Presentation::look()` (`Some` when the level or
  the mod has a look, `Presentation::has_look`), and
  `Presentation::upload` turns the renderer's lighting on or off to match,
  uploading the terrain whole when it changes. The game builds no look of
  its own: `olr-content` loads the level's and mod's looks, `olr-present`
  builds the frame.
- **Fast-forward** (`speed.rs`, plan 7 task 8, the user's request): a
  game nobody plays (`Session::human()` false, worked out again at each
  restart: a replay, or `--spectate`'s
  bots alone, `Assets::human_plays`) runs at `Speed` 1×, 2×, 4×, 8×,
  16× or max: `--speed`, PageUp/PageDown (`App::change_speed`; both in
  `olr_ui::GAME_KEYS`), "Fast-forward N×" as a HUD notice, with the speed
  reached when short of it ("16× (≈9×)"; always at max: `speed::Rate`
  over the last `RATE_FRAMES`, `speed::shown`). A game a person
  plays runs at 1× (`Speed::for_game`; `--speed` refused at start, the
  keys ignored). `speed::run_frame` (the app's loop and `tests/speed.rs`'s)
  runs exactly the `due` ticks at 1× (ticking or not: the script's clock
  and the input latch keep real time in the menus and the pause); faster,
  the `due` ticks times the factor, stopping at a tick that does not
  happen and, once the real-time ticks ran, when the frame's `MAX_BUDGET`
  (12 ms) is spent, so the window stays responsive; at max until that
  budget (`MAX_TICKS_PER_FRAME`). It draws only the last;
  `Speed::sound` thins the sounds (`Session::sound`: every tick up to
  2×, the frame's last from 4×, none at max). The world never sees the
  speed: `tests/speed.rs` plays bots alone at 1×, 4×, 16× and max to the
  same world and replay bytes and replays them at max. A menu start
  resets the speed to `--speed`. Spectating from the menus and replay
  seeking are not there yet.
- **Latency** (`latency.rs`): the exit summary's `input to present` is a
  key press to the present call of the first frame drawing a tick that
  read it; `--latency-probe <ms>` adds probes on a wall-clock grid (they
  press nothing) for scripted runs. `acquire` and `present` are the swap
  chain's waits. One frame in flight (`desired_maximum_frame_latency` 1;
  wgpu's GL backend ignores it). Under WSLg the 60 Hz pacing is Wayland's
  frame callback, before a frame starts: acquire takes ~0.01 ms.
- **Session** (`session.rs`): the match without a window, so it is tested
  headless; its presentation is one `olr_present::Presentation` (a worm a
  mode spawns mid-match gets `add_worm`, which marks the atlas for upload).
  What it plays is `setup::Game` (`Sandbox`, `Deathmatch`, `Replay`,
  `Online`), loaded once into `setup::Assets`; who presses the keys is
  `session::Driver` (`drive.rs`): the player alone, the player and
  `MatchDriver`'s bots, `ReplayDriver` (an `olr_replay::Cursor` checking
  every checksum; a divergence is shown and logged once, the replay stops
  at its end), or `Online` (`online.rs`).
  **The local slots** (issue #29, split screen headless:
  `Session::locals`, `Assets::local_slots`: the sandbox's 0, a
  Deathmatch's players that are not bots in worm order (`--locals N`, 1
  to `MAX_LOCALS` 4, `DeathmatchOptions::locals`: the humans first, then
  the bots; a Deathmatch only, never with `--spectate`), an online
  match's own slot; none for a replay, bots alone and an online match
  watched; `Session::local`/`Assets::local_slot` the first) each get
  their own player's keys (`Session::tick_locals(&[Keys])`, player `k`'s
  to slot `k`, `Keys(0)` for a player missing; `tick(keys)` the first's
  alone; OLX's `CClient.cpp:1527-1551`, each local worm its own input),
  name, look, the crosshair (the scene's
  `local`; with no local slot a local game's worm 0 keeps it, as OLX's
  local worms, and an online match watched has none), a view each
  (`Presentation::set_views`, `followed_view(k)`: every player sees the
  same 427 × 192, the fair-view rule, a deviation in `docs/porting/STATUS.md`;
  the app draws them all in split screen's layout, issue #30; a replay a view per
  recorded viewer), the big minimap dot, its tally (`Tally::player_kills`,
  `player_deaths`, `player_spawns`: one per local player, `[MAX_LOCALS]`,
  OLX's per-worm counts, `CClient.cpp:1653-1744`). The first's is the
  camera (`Presentation::follow`), the HUD and the highlighted scoreboard
  and results row (`Row::followed`); the exit summary gives every
  player's (`app::per_player`). Without one
  the camera and the HUD follow the worm chosen (`follow`, `follow_next`,
  worm 0 at first; `follow_next` skips worms that left, `World::gone`, and `after_tick` moves a watcher on from a followed worm that
  left, N89: `watch::the_watched_worm_moves_on_when_it_leaves` through a recorded replay with `Recorder::withdraw`, and
  `online::an_online_watcher_moves_on_when_the_followed_worm_leaves`, a
  spectator in an online match); the left and right arrows choose it in any game nobody
  plays (`Session::spectating`, `app::watch_key`: not in a menu, never a
  played game's; "Watching <name>" on the HUD; `app::game_has_screen`: a
  command-line `--spectate` or `--replay` always has the screen, though
  its menus' state stays on the main menu, final review B-I4). **Per-worm
  looks** (`session::WormLook { name, colour, skin }`, one per worm,
  registered in worm order): local play's as above, an online match's the
  lobby's (`setup::load_online` loads every slot's skin by slot,
  `Assets::slot_skins`: the local player's by its id, every other slot's
  by pack id and content hash, `Content::skin_matching`; one this install
  lacks or has with other content is the default skin, with a warning
  logged once: skins are never downloaded, plan 13c).
  `restart` (F5) builds the world from the assets loaded once: the
  sandbox again, a Deathmatch's rematch (seed + rematch number; the
  previous match's replay is handed back to save), a replay from the
  start; the worms are the same, so the atlas stays and only the terrain is
  uploaded again. It clears the last tick's events (the rematch's bots
  think first, on the new world). Debug builds assert every `DRIFT_CHECK_TICKS` (100) that
  the colour terrain's material is the world's (`session_catches_terrain_drift`).
  A tick's bookkeeping (`after_tick`: clock, tally, log, end and results,
  popups) reads a `TickView`: a local tick's world and events, or an
  online match's `ConfirmedTick`.
  **Whose rope throws are heard** (`throw_sound.rs`, the user's playtest
  report 2026-10-16): only the players' own, as OLX plays `smpNinja` in
  the human input handler alone (a bot's or a remote worm's throw is
  silent): every local slot's, a replay's viewers', nobody's for bots
  alone or an online match watched. The simulation's THROW sounds carry
  no owner, so `ThrowSounds::retain` keeps as many as those worms'
  `NinjaRope::throws` grew since the last tick or poll, summed (on the newest
  predicted world online, inside `OnlineDriver::present`; a rollback that
  takes throws back plays none); every other sound plays as before.
  `Session::sounds` is what the last tick or poll played (or would have,
  with `sound` off).
- **Online** (`online.rs`, plan 10 task 10; spec §3.4, decision N20):
  `Session::online(assets, OnlineDriver::new(handle, &start), ..)` after the
  peer's `MatchStarting` (`handle`: the peer's `net::PeerHandle`, below;
  the events after it in that poll go to `Session::present_events`, or
  `present_polled` with that poll's `pumped`); then
  `Session::frame(now, local_keys)` once a frame instead of
  `Session::tick`: one `PeerHandle::poll` (`OnlinePeer::poll` with the
  pump's queued events first; the local keys
  from `local_keys`, which the netcode calls once per new tick: the input
  latch; the scene reads every simulated tick inside it,
  `after_tick_resimulated` for a re-simulated one), then the poll's events
  in a fixed order whatever their order in the poll (`online::present_poll`,
  pinned by `a_loaded_state_is_presented_before_the_polls_ticks`): a
  loaded state (`OnlineEvent::StateLoaded`: `Presentation::state_loaded`,
  `NetPresenter::reset`; also when the pump ran ticks, `OnlineDriver::
  present`: a `StateLoaded` at the pumped tick and the whole map restored
  come first, and ticks the pump ran after the poll, `PeerHandle::
  with_pumped` under the same lock, drop the poll's own ticks), the
  restored areas and ticks
  (`NetPresenter::apply`, its sounds at the views' listeners; debug builds check the
  colour terrain after every advance, about 1 ms a frame at worst in
  `an_online_match_runs_headless`), `NetPresenter::confirmed`, and the
  confirmed ticks through `after_tick` (log, tally, results, clock,
  popups: never a predicted tick, so a damage popup comes late, at the
  confirmed tick's positions). The HUD's scoreboard and the followed
  worm's lives, kills and deaths are the last confirmed tick's
  (`Session::scores`); the newest predicted world is drawn
  (`Session::with_world(f)`: `f` reads it under the peer's lock, so it
  must not use the peer or `with_world` again: a nested lock panics
  instead of deadlocking; one frame's reads may straddle a pump poll, which
  the scene and HUD bear: they look worms up by index with `get`, and the
  next poll starts the presentation again). At `MatchEnded` the peer stops presenting while its
  session goes on to confirm and drain: `present_events` takes the world
  presented last once (`OnlinePeer::take_ended_world`) into
  `Session::world` and freezes there (`frozen`; no clone per frame),
  pinned by `an_online_match_runs_headless` (when the pump ran ticks, the
  frozen world's colour terrain is rebuilt from it); so too at a late
  rejoiner's `Migrated { me: false }` (plan 12c, N72: its peer dropped
  the match it played and kept its world, `take_ended_world`), until the
  new host's start replaces the view (it watches; never its start world:
  `a_late_rejoiner_watches_until_the_next_match`). The other events (lobby,
  chat, end, connection) are handed back for the shell. On
  `Disconnected`, `Kicked` or `Refused` the peer has dropped its match,
  so the session would draw its start world: the shell must leave the
  match view before the next build; `OnlineDriver::into_handle`
  gives the peer back (its pump still running). An online match is never fast-forwarded (`human()`).
  A `MatchStarting` among a poll's events and everything after it are
  handed back unpresented (`present_events`): they are the next match's.
- **Online from the menus** (`shell/online.rs`, `net.rs`; plan
  10 task 12): `HostGame`/`JoinGame` make the `OnlinePeer` (the identity
  from `Store::load_identity`, a fresh key with a warning when the file's
  is no valid key; the selected profile's look, its skin by pack id; a
  `Content` of the shell's roots; the host's setup from the saved match,
  `net::setup_of`: its loadout in full, `Default` when too large to send,
  logged (`olr_online::matches::loadout_wire`); `net::prefs_of` shows a
  lobby's loadout (`loadout_of`); bots "Bot N" in OLX's bot colours, never through
  `MatchPrefs::sanitize`: a lobby may have no bot). The host is in its
  lobby at once (`LobbyJoined`); a client when its first lobby names it.
  A typed address goes through `shell::resolve`: a name's first IPv4
  address (`choose_address`; an IPv4-mapped one counts); an IPv6 literal
  or an IPv6-only name is refused with `IPV6_UNSUPPORTED` (the host binds
  `0.0.0.0`, the client's UDP socket is IPv4).
  **The network pump** (`net/pump.rs`, plan 12c Task 5, rulings PF12c-2,
  PU-1, PU-2):
  every online peer (a host's, a client's, `--net-sim`'s host) lives
  behind a `PeerHandle` (`Net::peer`, then the match's `OnlineDriver`,
  `Shell::sim_host`) from its start (`PeerHandle::pumped`) to its release
  (`into_peer`: the pump stopped and joined within one of its polls, then
  `release`'s leave, replay and NT-2 shutdown as before). Its thread
  `olr-game net` wakes every `PUMP_EVERY` (10 ms) and polls the peer
  whenever the UI thread has not for `PUMP_AFTER` (100 ms; the UI's last
  poll an atomic): a dragged window (Windows' modal loop blocks winit) or
  a long match load no longer silences the peer, while a slow frame (any
  above 10 fps) is no stall and plays exactly as before (PU-1:
  `frames_every_30_ms_play_as_before`, `..._45_ms_...`). Meanwhile the
  local player presses the keys the UI last latched and still held then
  (`PeerHandle::poll` keeps the last its `local_keys` gave, masked by
  `PeerHandle::set_held`: `App::net_step` gives `InputState::repeatable`
  (`held`, plus mouse controls' own Up, Down and Strafe, so a stall never
  turns a strafing worm) through
  `Shell::set_held_keys` before each network frame, so a tap is not
  repeated, nor are Dig's taps: `held` has no gesture keys) for `PUMP_HOLD` (250 ms) of the
  stall, then `Keys(0)` (a long drag does not keep firing); the input
  latch is untouched, nothing is drawn, the host's bots think inside the
  peer; its events wait for the UI's next poll, except the presentation's
  (`Ticked`, `Restored`, `StateLoaded`), which are dropped: that poll's
  `PolledEvents::pumped` (the newest pumped tick) starts the presentation
  again as after a loaded state (`OnlineDriver::resyncs` counts it; never
  once the session froze its ended world). At most `PUMP_QUEUE` (4 096)
  events wait: beyond it the oldest two neighbouring `Confirmed` with no
  control event between them (never across a match's end or start; the
  newest never thinned) are thinned into one (only the ticks with a kill,
  spawn, message or the end, without
  damage or positions; scores only with the end), so the HUD's log, the
  tally's kills and the results survive a stall of any length (bound:
  the exit summary's match time misses the thinned ticks, and their damage
  popups are gone); then the oldest chat-like event goes (`Chat`,
  `Notice`, `Left`, `TimedOut`, `Downloads`, `Reachability`, `RelayJoin`,
  `ListenRelay`); control events (all others: a match's start and end,
  the lobby, lagging, a lost connection, a replay) never go (PU-2), so
  only they can pass the bound; each loss counted, the first logged. A
  pump poll that panics stops the pump at once: a console warning, a
  lobby/HUD notice at the UI's next poll, `is_pumped` false; the frames
  poll on. The F2 meter counts only the ticks a frame's own poll ran
  (`Session::polled_ticks`). One
  `Mutex`, held for one poll (the UI's or the pump's) or one
  `PeerHandle::with` (lobby actions, stats, the world read), never across
  rendering and never while waiting on anything else: the UI waits at most
  one pump poll. A peer locked again on a thread that holds it (directly
  or through another peer's lock) panics instead of deadlocking.
  `PeerHandle::with_pumped` hands the pending pumped tick to the
  presentation and takes it, so a frame starts again once. The pump's time goes on from the UI's last `now` (a
  test's clock too), and no poll is given a time before the last one
  given. `Shell::with_peer(f)` is the peer's lock (tests:
  `test_prompt_limit`); `Shell::pumped()` whether it has a pump,
  `Shell::pump_polls()` its polls; `App::press(action, down)` presses a
  bound key through the input path (autoplay's `Press`, tests),
  `App::ticks_measured()` the meter's ticks.
  `start_online` takes the content from the peer (`with`: an `Arc`) and
  loads without the lock, so the pump keeps the match's network alive
  meanwhile (`Shell::test_slow_load(d)`, `test-hooks`: each load `d`
  longer). The ticks the pump runs are the netcode's like any other
  (`olr-online`'s `OnlinePeer: Send`, pinned there): the peer cannot tell
  which thread polled it. The hidden window (N21) is unchanged: while
  hidden and online it still runs `hidden_frame` every `HIDDEN_FRAME`, so
  the UI keeps polling and the pump stays idle; a hidden frame does more
  than the peer's poll (the LAN announcer, a Public game's notes, the
  replays, `--net-sim`'s host's start, the lobby and match flow), and a
  slower wake would make every hidden frame rebuild the colour terrain
  after pumped ticks.
  `Shell::net_frame(now, keys)` (every frame, before the ticks; while an
  online match plays its peer runs the ticks and the local clock rests)
  polls `--net-sim`'s host, the LAN `Listener` on `LAN_PORT` while the
  Multiplayer screen shows (games of another `FINGERPRINT` hidden), then
  the peer once: through `Session::frame` while a match plays (the
  input latch; `Keys(0)` while the match's menu, the chat or another
  screen has the keys), else `PeerHandle::poll`. Events: `Lobby` →
  `MenuData::lobby` (`net::lobby_view`: `PHASE_LOBBY` while gathering or
  showing results, `SEAT_SPECTATOR`, the host's `can_start` reason, a
  client's "Waiting for the host"; the peer sends its `Have` itself), the
  weapon screen's menu for the lobby's mod and loadout (its weapons and
  unique rule: `net::weapon_menu`, decision
  N19, from the peer's content, made again when it changes: a downloaded
  mod counts, `net::WeaponsKey`), the host's LAN
  announcement (`Announcer`, for `Visibility::Public` and `Lan`; `GameSummary::of_lobby(..).lan()`,
  the level's name from the peer's content); `Chat` ("name: text",
  `LineKind::Chat`), `Notice`, `Left` ("name has left") and `TimedOut` ("name has timed out"; OLX HasLeft/HasTimedOut) → the lobby's chat
  (`net::CHAT_KEEP` lines) and the HUD log; `MatchStarting` → the last
  match's view ends, `setup::load_online` (from `OnlinePeer::content`,
  the content the match was built from), `Session::try_online` (a
  failure hands the peer back to say goodbye) with the
  events after it, `MatchStarted` (InGame), `Outcome::Started`;
  (the delivering poll's `pumped` with them: `present_polled`);
  `MatchEnded` → the results if they did not show yet; `Results`' Back →
  `EndMatch`: the peer back to `Net` (`end`), the Lobby; `Lagging` → the
  warning; `Kicked`/`Refused`/`Disconnected` → the match view goes at
  once (`lost`), then `OnlineFailed` (Multiplayer with the reason);
  **host migration** (plan 12c Task 9, spec §3.7): `Migrating(step)` →
  `Net::migrating`, the step's text (olr-online's `MigrationStep`: "The
  host is gone: moving the game to Anna…") as the playfield's top line
  (`Shell::migration_line`, in place of "Connection problem"; amber,
  wrapped to the picture) and `olr: migration: …` on
  the console; a successor join's `Joining` steps meanwhile only on the
  console (the menus wait for no join); `Migrated { host, me }` → the line
  goes, "<host> is now hosting" or "You are now hosting" in the HUD and
  lobby log and on the console; `me`: `Net::host`, `Net::migrated` (no
  announcer, no `Hosted`, no connection line: the migrated game is closed,
  N72; its lobby view, made again at once, the host's with `listing`
  `MIGRATED_LISTING`); a late rejoiner (the peer without its match) also
  logs `late_rejoin(host, playing)` ("You reached Anna after the match
  resumed: you watch until the next match"; its lobby past the match:
  "…after the match ended: you play the next one"): while the lobby still
  plays, `Net::late` and its view stays frozen until the new host's start
  lets it watch (`MatchStarting` clears it); when the lobby is past the
  match, at once or with a later `Lobby` state (Results, Gathering),
  `leave_late_view`: the view goes, `UiAction::MatchLeft`, the Lobby (a
  host whose match ended sends no start: review Important 1); the
  `HOST_BACK` notice (N69, called off) clears the
  line; each `MigrationLog` (ruling FR12c-4: an attempt to reach a
  successor, its name, address and outcome) only as `olr: migration:
  …` on the console; `HostLost(why)` (`LostWhy`, its text on the screen:
  FR12c-4) → `host_lost`: the match view goes at once, the
  peer is let go (`release`: its replay to `Shell::replays`, saved by
  `auto_save`), `MenuData::host_lost` (why), `host_lost_hosting` (this
  player hosted: its own step-down, or `CUT_OFF`, ruling SB-2: it heard
  none of its players), `host_lost_replay` set once the
  replay is saved (`Shell::host_lost_pending`, that replay's number in
  `Shell::replays`, so an earlier match's replay finishing first does not
  say it: `save_online_replay`; none
  for a mid-match watcher or without a data directory), then
  `UiAction::HostLost` (the Host lost screen), never `OnlineFailed`.
  The playfield's top line is `overlay::top_line(migrating, lagging)`.
  Every client the shell starts takes part (`joining::migration_config`,
  N81): `MigrationConfig { standby: a port-0 endpoint (the settings'
  STUN, no port mapping, `allow_local` when every relay is local, the
  `--net-sim` impairment, `Shell::ice`), relays: the settings' (none:
  `None`), ice: STUN and the player's TURN (`Shell::ice`), region,
  turn_from_nostr, allow_local, enabled }`; test hooks (`test-hooks`):
  `test_migration(on)` (off: `MigrationConfig::off()`, D7),
  `test_no_standby()` (N79: it follows, never hosts), `test_stop_pump()`
  (`PeerHandle::stop_pump`: with no frames, as silent as a crashed game);
  `ReplayReady` → `replays::auto_save` (a peer let go, `release`, its pump
  stopped first, hands
  its replay being made to `Shell::replays`, polled every frame and
  waited for at quitting, at most `REPLAY_STOP_WAIT` 2 s: leaving right
  after a match keeps its replay); `DesyncBundle` → `<data>/desyncs/
  desync-<unix s>-<n>.oldg` (`net::save_bundle`, olr-online's bounded
  `bundle::save`: 4 MiB a bundle, 64 MiB in all; olr-online's `GameBundle`:
  the start, the confirmed input rows from tick 0, both checksums, the
  build fingerprint, the platform; a `net-repro` for game bundles is
  still to come). The lobby's commands (`lobby_command`): Start and Force
  start take `UiState::take_seed`; a refusal shows as
  `MenuData::online_status`. Leaving (`LeaveOnline`, the match menu's
  Leave the game, quitting) says goodbye (`OnlinePeer::leave`).
  **Downloads** (plan 11): `Shell::new` opens one `DownloadCache` in
  `Store::cache_dir()` (`net::open_download_cache`: while another game
  holds it, `InstallError::Locked`, `downloads-2` to `-4`; `Err(why)`
  without one, logged once: downloads off, the peer's notice in the lobby
  says why) and every peer it starts gets it (`PeerContent { cache,
  upload_rate: HOST_RATE }`, `--net-sim`'s host and client the same one,
  ruling PF11-4). `Downloads` → `Net::downloads` (`net::download_view`:
  `queued`, `downloading`, `checking`, `done`, `failed: <why>`, `not
  available`) and `LobbyView::downloads`; the prompt is
  `OnlinePeer::download_prompt` (`DownloadPrompt` refreshes it), its
  Download `approve_downloads`, its Leave leaves. A replay's packs
  (`setup::load_replay`, the Replays screen's and `--replay`'s) come from
  the roots, else from any of the cache directories (`net::cache_dirs`:
  `downloads`, `-2` .. `-4`) by content hash, read without opening the
  cache (`DownloadCache::lookup`: no lock, nothing written; N36), laid
  over (`with_cached_packs`, `Content::with_packs`), then
  `check_replay_packs` as before: a replay plays while a game holds the
  cache, and `--replay` creates, locks and writes none. The
  options' download cache row (`MenuData::download_cache`, kept current
  by `Shell::poll` while the options show: packs and partials, read at
  most every `CACHE_READ_EVERY` (1 s; at once on opening the options),
  whether Clear may run every frame; its lock tried, never waited for;
  `Shell::download_cache()` is the shared cache) and its Clear (`clear(&[])`: the menus keep
  nothing), refused while an online game is on (`net` or `--net-sim`'s
  host: "leave the online game first").
  **The network's gate** (Task 9 review Critical 1): one predicate,
  `Shell::network_live` (an online game, `online_active`; the Multiplayer
  or Host screen; the LAN listener or the relays not yet let go; a typed
  name being looked up). `App::network_frame` (every drawn frame, and the
  hidden window's) runs the network frame while it holds, and
  `Shell::net_frame` itself returns at once otherwise, so the tests'
  frames go through the same gate (before, only an online game opened
  it: the Internet tab and the LAN list stayed empty, a typed name hung
  on "Looking up …"). **Hidden window** (decision N21): while
  `Shell::network_live`, `about_to_wait` runs `App::hidden_frame` (the
  network and the match's ticks, no drawing) every `Shell::hidden_wake`
  (`WaitUntil`: not a spinning `Poll`): `net::HIDDEN_FRAME` (4 ms) while
  an online game is live, `net::HIDDEN_IDLE_FRAME` (60 ms) for the lists,
  lingering relays, a lookup or a replay being made alone
  (`a_hidden_window_wakes_by_what_is_live`). **In-game chat**: the options' `online.chat_key` (T)
  opens olr-ui's field over the playfield (`Shell::open_chat`, from
  `App::key`); while it is open no key reaches the game (`App::game_key`,
  for the window's keys and `App::press` alike; `App::key`) and those
  held are released, so digits typed there select no weapon, not even
  once it closes (issue #48, `online::digits_typed_in_chat_select_no_weapon`). **Network
  overlay** (F3, spec §7.3, `overlay::network`): ping, input delay,
  rollback depth last and p99 over 5 s (`net::NetMeter` on the session's
  depth histogram), the last second's deepest, loss, corrections per
  second, the frontier lag (`current_tick - frontier`); "Connection
  problem" at the playfield's top while `Lagging` (a migration's line
  instead while one is under way: `Overlay::migrating`). Online frames are
  interpolated by the pace's own tick phase (`Session::online_alpha` →
  `OnlineDriver::alpha` → `OnlinePeer::tick_phase`; the local clock's
  phase is not the netcode's), 1 (the newest tick as it is) while the
  match stalls or catches up, while its pace is held (a migration's
  resume hold, a jump back: `tick_phase` `None`, plan 12c Task 8b), for a spectator, on the frame after a
  loaded state or a rollback (`Presented::jumped`) and once it ended. The
  host's setup packs are hashed once per mod and level (`Net::packs`);
  `--net-sim` readies again whenever the lobby gathers. **Flags**: `--host [name]` and `--join <addr>` act through
  the menus (`host_from_flag`, `join_from_flag`; nothing they change is
  remembered: `UiState::restore_online`, so no later save writes it),
  `--port <n>` hosts there and joins an address without a port
  there (`Shell::set_port`; tests host on 0), `--net-sim <profile>`
  (`start_net_sim`: a host in this process with `NET_SIM_BOTS` (3) bots
  on the saved match, watching; this player joins it over a loopback
  link impaired both ways, `EndpointConfig::impair`; the client readies
  itself and the host starts whenever every player is ready).
  **Finding games** (`finding.rs`, plan 12a Task 10, N39-N43): the shell
  owns one `finding::Internet` while the Multiplayer or Host screen shows
  or a hosted game is Public (`poll_internet`; otherwise it lingers
  `RELAY_LINGER` (30 s, unsubscribed, still drained: a visit within it
  reuses the pool, `relay_pools_opened`, `relays_open`), then closes and
  clears `selected_game`, its pool
  dropped on a thread of its own, `relays-closing`, so the UI never waits;
  at quitting, `close_internet` waits so a closing note leaves): a `Pool`
  on the settings' relays (`relays_of`: else `DEFAULT_RELAYS`; none: no
  pool, "No relay reachable"; relays edited in Options close the open
  pool, lingering or in use, and open one on the new list, final review
  B-I2), a `GameList` (the settings' blocked keys, kept in step with
  Options every frame, `Internet::set_blocked`: a block hides at once, an
  unblocked host's game shows with its next note;
  `allow_local` when every relay is local, `local_relays`, olr-discovery's
  `hosting`: one's own or the tests' relay) subscribed (`GAMES_SUB`, `filter_since`) while the
  screens show, the selected game pinged (`Pinger`, a TCP connect on a
  thread, only games the list accepted with an address), the view
  (`internet_entry`: the mod's name from the catalog, the region
  converted naming every variant, the seated players and the bots apart
  as the note gives them; the LAN list's `net::lan_entry` alike) made again only when something changed
  (a note the list refused changes nothing: `GameList::offer`'s result).
  The list's ages and the subscription's `since` use the wall clock read
  at each poll (`unix_now`; never the frames' monotonic clock, which
  stands still while the machine sleeps).
  `InternetStatus`: `NoRelay` without relays or when none is connected
  and each failed since the start, `Searching` until the list's first
  `EOSE`, else `Listed`. `BlockHost` drops the key's games at once.
  **Joining** (plan 12b, N46): `JoinGame { addr: Option<String>,
  password, expected_host }` → `start_client`. Text that `is_invite` is
  decoded (`Invite::decode`: "this invite code is damaged", …) and joined
  with `Join { direct: invite.addr, nostr }`, its key expected, the
  relays `join_relays(invite.relay_urls(), settings')` (hints first, at
  most 8); a typed address or a LAN game is `Join::direct`; a listed
  game's Join expects its key (N37: another is `WRONG_HOST`), its address
  directly first then through the settings' relays, and without an
  address through the relays alone (`listed_join`, `nostr_join`:
  `NostrJoin { host, pool, region: the settings', turn_from_nostr: true,
  allow_local: local_relays }`, `None` without relays; neither address nor
  relays: `NO_WAY_TO_JOIN`). A join through Nostr's endpoint asks the
  settings' STUN servers and their own TURN servers (`turn_of`:
  `TurnServerSpec`s of `user:pass@host:port`, olr-online uses the first 2);
  a direct-only join asks nothing. A typed host name is looked up on a
  thread (`shell/joining.rs`, with the joins' building and `Unmapping`:
  `Resolving`, `olr-game resolve`; `literal` takes an IP address
  at once), polled by `net_frame` (`poll_resolving`; "Looking up …" the
  waiting text; Back gives it up): no DNS on the UI thread. Every
  `OnlineEvent::Joining(step)` is `MenuData::joining` (the Multiplayer
  screen's waiting text) and a log line; `Connected` and
  `OnlinePeer::connection` (following ICE, `poll_connection`) are the
  client's `LobbyView::connection` (`path_text`); every member's
  `Member::path` its `MemberView::path` (`path_tag`, `net::member_view`).
  **Hosting** (`finding::Hosted`, in `Net::hosted`): the LAN announcer for
  Public and Lan; for Public and Invite the endpoint asks the router to
  open its port (`IceConfig { stun: the settings', port_mapping: the
  settings', allow_local: local_relays }`) and listens on the settings'
  relays (`HostListen { pool, public_address: the setting }`, N54; Lan
  neither); relays edited in Options are listened on at once
  (`OnlinePeer::listen_on`) and hinted by the invite. Its
  `OnlineEvent::Reachability` is kept (`Hosted::set_reach`, each new line
  logged "reach: …"): its address is `Reachability::direct()` (N47: the
  router's usable mapping, else the public address setting; never STUN's)
  on the invite (`LobbyView::invite`, the host's only) and, for Public, in
  the notes (`Hosted::announced`: IPv4, and not a private one on public
  relays, final review B-M2; with its external port); its lines are
  `LobbyView::reach` under `listing`. Each `OnlineEvent::RelayJoin`
  (ruling FR12b-3: what became of a join through the relays) is a line
  in the host's lobby log (`chat_line`) and `olr: relays: …` on the
  console; each `OnlineEvent::ListenRelay` (B-M4) is `olr: relay
  <url>: connected|unreachable: …` on the console; each member's path
  (`net::path_news`, final review C-M1) is `olr: join: <name> via
  <tag>` when it first shows, `olr: path: <name> now via <tag>` when it
  changes. A Public game's `Publisher` signs
  with the install's identity: notes once the reachability settled, at
  most `SETTLE_WAIT` (5 s) after hosting (`created_at` the wall clock; the
  first note dated 10 s after this game's last closing note,
  `Shell::closed_note`, A-M6), address or not (N46: joined through the
  relays); none with an unreachable public address setting on public
  relays (`Hosted::publishes`); the closing note when the lobby is left
  or lost (`close_hosted`). `LobbyView::listing`: `FINDING_ADDRESS`
  ("Checking how players can reach you…") until settled, `UNREACHABLE_ADDRESS`, "Not listed: no relays
  …", "Not listed: no relay reachable" (checked before "Listed": all
  relays dropped), "Announcing on the Internet…", "Listed on the Internet"
  (a relay's `OK` for one of this game's notes: `Internet::forget_ours` at
  each hosting, B-M1). An Invite game's line is about its code (B-I3):
  `FINDING_ADDRESS`, then none with an address, else "This
  code joins through the relays" (`INVITE_THROUGH_RELAYS`), or without
  relays `INVITE_JOINS_NOTHING`. A Public game's pool opened on edited
  relays gets the last note again at once (`Hosted::last_note`, `pool`).
  **Leaving** (ruling NT-2): a peer let go (`release`: leave, its replay
  taken, `OnlinePeer::shutdown`; `--net-sim`'s host too) never blocks;
  its `Shutdown` (the router mapping being deleted, olr-net's confirming
  `PortMapper::close` on a thread) is kept while the router is asked
  (`prune_unmapping` at each push and network frame, `Shutdown::poll`:
  finished ones go, one the router did not confirm warned about then;
  `unmapping_pending`), `Shell::take_unmapping` hands them over as an
  `Unmapping`, and `main` waits on it (`UNMAP_WAIT`, 1 s in all) after
  `App` (the window) is dropped, warning when the router did not confirm.
- **Level editor** (`editor/`, `shell/editing.rs`, plan 14c; decisions in
  `docs/porting/STATUS.md`, "Level editor decisions (plan 14c)"):
  `EditorHost` holds `olr-editor`'s state and carries out its commands:
  the Editor screen's `AppCommand`s (`open_from_menu`: Edit a pack of the
  data directory's `packs/` (`level_dirs`, by manifest id), Copy a level
  (`new_pack` `Template::From`), New level, Recover; ids `pack_id`
  slugs unique against the catalog and `packs/`; `DeleteLevel` is the
  shell's, `files::delete_level`: only a pack `level_dirs` lists, its
  folder removed (a link only the link), then the content listed again;
  a bundled id is refused in `editor_message`, `tests/editor_host.rs`'
  `delete_removes_only_a_level_of_yours`), `open_dir` (`olr --edit`),
  `act` (a frame's actions, the answers fed back in order: `Open`,
  `Create`, `Save` (a missing directory made first), `List`
  (`files::list`, `list_dir`: every name read, sorted as the browser shows
  them, the first `MAX_ENTRIES` and one more kept (the browser says it cut); kinds from the listing, a `stat`
  only for a symbolic link and a folder's `pack.ron`),
  `ReadPreview` (`files::read_preview`: read as an import is, decoded
  within `IMAGES`, made at most `PREVIEW_SIDE`, 192, on a side, each pixel
  its block's mean), `ReadImage` (`files::read_image`:
  metadata above `MAX_IMPORT_BYTES`, 64 MiB, or no regular file refused
  before opening, a non-PNG/JPEG signature before reading the rest),
  `SaveSettings` (`EDITOR_FILE` through `Store::save_file`), `RecoveryDone`)
  and `tick(now)` (the recovery copy, `recovery.rs`: `recovery/<id>/`, the
  pack's files and `recovery.ron`, `replace_dir`-atomic (an interrupted
  write's hidden siblings of that id removed first, its warnings said),
  when unsaved changes have waited `RECOVERY_INTERVAL`, 120 s, since the
  first tick that saw them, made from `Document::snapshot` and written on
  the `olr-game recovery` thread (250 ms for a 1350 × 550 level on the UI
  thread before), waited for (`wait_recovery`) before a save or Close drops
  it, before another document opens, at `flush_recovery` and on drop; each session's id unique against `recovery/`, so a crash's copy
  survives (a recovered document keeps its copy's id; it reopens in its
  original folder only when that still holds its pack, else in a new folder,
  under a new id when another pack took its own); `list_recoveries`
  (damaged ones listed; a copy a kill left hidden between `replace_dir`'s
  renames is made `<id>` again first), `discard_recovery` (ids are plain names)).
  Bundled roots (every root but `packs/`) and `recovery/` are refused for
  Open, Create and Save (`BUNDLED`, `RECOVERY_IN_PLACE`).
  Plan 14e: the catalog's themes carry their holes (`StampMask::from_hole`
  of `LoadedTheme::holes`, the crater brushes); the user's texture library
  is `Store::textures_dir()`, listed at `new` (`files::list_textures`:
  `<name>.png` in name order, at most `MAX_USER_TEXTURES`, each read as an
  import and within `MAX_TEXTURE_SIDE`, an unreadable one a warning);
  `AddTexture` (`files::add_texture`: bounded read, side checked from the
  header, the cap counted by name, a unique portable name, written through
  a hidden `.part`) and `RemoveTexture` (`remove_texture`, plain names
  only) answer `TexturesChanged` with the library re-listed, and `Failed`
  when the change failed. Issue #43: `Catalog::materials` is
  `olr_edit::material::list_materials` of `olr_content::materials_dir` of
  the last bundled root and `Store::materials_dir()`, listed once at
  `new` (a file that does not read a warning); an edited Play carries the
  bake's surface maps (`edited_play_builds_a_sandbox_or_a_match`), so it
  heats. Issue #9, the asset library: the built-in
  assets are `olr_content::assets_dir` of the last bundled root
  (`content/assets` beside `content/packs`, read-only, listed once at
  `new`), the user's `Store::assets_dir()` (`assets/`), both listed by
  `files::list_assets` (`olr_edit::asset::list_library`: each folder's
  summary, the user's capped at `MAX_USER_ASSETS`, a folder that does not
  read a warning) into `Catalog::assets`, built-in first. `ReadAsset`
  answers `AssetRead` (`load_dir`, a valid folder id only); `SaveAsset`
  (`add_to_library`), `ImportAsset` (`import_into_library`) and
  `RemoveAsset` (`remove_from_library`) answer `AssetsChanged` and, for a
  folder written, `AssetSaved`, or `Failed` (`EditorHost::assets`). The
  built-in folder is never written (`tests/editor_host.rs`'s
  `assets_are_listed_saved_placed_and_removed`). Issue #77, the asset
  tab: `ReadAssetToEdit` answers `AssetToEdit` (`read_asset`, as
  `ReadAsset`); the canvas draws the shown tab's document
  (`EditorState::active_document`), and `act` answers a change of
  `shown_generation` (a tab switch, an asset opened) with `FullUpload`;
  recovery stays the level's (`document()`, `dirty()`). A Ctrl+V egui-winit swallowed (an empty system
  clipboard: it sends no key and no paste) is given to egui as an empty
  `Event::Paste` (`app::paste_swallowed`, the window's modifiers kept in
  `App::modifiers`), so the editor's own clipboard pastes.
  `HostOut::{Upload, FullUpload, Distant, Play, Closed,
  Message}` is what the app does. The shell owns the host (`Shell::editor`):
  while it is open `App::frame` draws `olr_editor::ui::show` instead of the
  menus, `Shell::editor_frame` runs it (Play becomes `Launch::Edited`:
  `setup::load_edited` of an `EditedPlay` with `PlayLevel::Edited` and
  `PlayMod::Installed`, no replay, `Assets::edited`; Closed brings the
  Editor screen back with the content listed again), and the app's
  `editor::canvas::Canvas` (the bake's `ColorMap`, the collision view's
  at art scale 1, or issue #43's Materials view's: the art at its scale,
  each world pixel's surface material's tint (`olr_editor::surface_tint`)
  half and half over it, grey without one; `CanvasView::of` the editor's
  state) gets its uploads (`replace_rect`, then `update_terrain`;
  whole after a new document, a resize, a view toggled or a Play,
  whose session had the renderer's terrain; dropped while the editor is
  closed or its Play runs) and draws a terrain-only list of the world under
  the whole canvas (black off the level: panning never changes its size, so
  the renderer's world target stays; cut to one world target, it follows
  the visible level) at egui's own scale of the pass into
  its part of the window (`Gpu::present`'s world is `(list, look, area)`; games
  pass `playfield(size)`), with SharpBilinear for that frame; the
  renderer's warnings are logged once and said in the editor (`Note`).
  **The lit canvas** (plan 15a Task 19, `editor/canvas.rs`): the map
  carries the bake's glow and clear solids (`ColorMap::set_glow`,
  `set_clear`; a dab's rectangle also `replace_glow_rect`, and the clear
  flags when they changed; a glow that came or went rebuilds the map,
  `fits`; the glow beam and haze maps when they changed, and any upload
  over a level with beams is a new `beams_generation`, so a directional
  element's strength, group or art re-derives the beams:
  `a_beam_strength_edit_derives_the_beams_again`). The look is the
  document's `canvas_look()` (issue #79: an asset's shown state may set
  its glow groups' colour or intensity). `Canvas::lit(doc)` is true for a look, glow or
  clear solids, never in the material view; the app sets the renderer's
  lighting to it, uploading the canvas whole on a change.
  **Distant layers** (issue #11): a see-through bake makes the map
  `ColorMap::new_see_through` (`fits` builds it again when that changes;
  never in the material view); `HostOut::Distant` hands the layers to the
  canvas (`Canvas::set_distant`) and the app gives them to
  `Renderer::set_distant` when they changed and with every whole upload
  (`take_distant`; none in the material view); the draw list's
  `parallax` is the editor's camera, `Canvas::draw`'s argument from
  `EditorState::parallax_camera()` (issue #66: the level's centre, so a
  layer shows where placed; with View ▾ Preview parallax the canvas's
  centre), never the cut view's, so the canvas draws them where the
  editor hit-tests them (`distant_layers_reach_the_canvas`).
  `Canvas::look_frame(doc, clock_ms, view_mode)` compiles the document's
  look (`olr_format::levels::compile_look`, again only when it changed; a
  look that does not check draws as none) and begins the frame at the
  editor's clock and view mode; the canvas's world is `(list, look, area)`. `EditorHost` advances the
  editor's clock (`EditorState::advance_clock`) by the time between its
  frames, at most 100 ms a frame (a Play's length never jumps it). Play
  needed no change: `setup::load_edited`'s `LoadedLevel::from_source`
  carries the look, glow and clear (its test asserts `has_look`).
  During an edited Play the HUD draws with a Stop button at the
  playfield's bottom left (`overlay::stop_button`; `hud::Frame::edited`: its keys
  say "Esc stop"), Esc or Stop ends it (`Shell::stop_edited`), F5 plays it
  again (`Shell::restart_edited`), and a match over ends it when its
  results would show (`Session::results_shown`), the editor saying so. Closing the window asks
  the editor first (`editor::on_close_request`, `CloseDecision::Wait`: the
  window closes when the editor does; a Cancel lets it stay); a quit that
  does not ask (autoplay's, `--autoquit-after`, an error) writes the
  recovery copy first (`Shell::finish`, `EditorHost::flush_recovery`). A
  Play that does not start is an error dialog in the editor
  (`EditorAction::Failed`), the host's messages its status bar (`Note`).
  `--edit` conflicts with every `GAME_FLAGS` flag and with `--host`,
  `--join` and `--net-sim`.
  `olr --edit <dir>` (`main.rs`'s `edit_target`, R6) exits with status 2
  for a directory inside `--content`, a missing path or a file. Tests:
  `tests/editor_host.rs`, `editor::canvas`'s unit tests,
  `tests/canvas_gpu.rs` (a binary of its own in the GPU tier, `OLR_GPU_TESTS`:
  a rectangle upload draws as a full one; `the_canvas_draws_lit`: a dark
  ambient darkens the canvas, Full bright is the art byte for byte),
  `editor::canvas`'s `the_canvas_frame_follows_the_document` and
  `a_glow_edit_uploads_its_rectangle`, `the_materials_view_tints_by_surface`, `main.rs`'s argument tests,
  `fixtures/autoplay/editor.ron` in `cargo xtask e2e`.
- **Skin editor** (`editor/skin.rs`, `shell/editing.rs`, issue #14;
  decisions in `docs/porting/STATUS.md`, "Skin editor (issue #14)"):
  `SkinHost` holds `olr-skin-editor`'s state and does its disk work. The
  Skin tab's `AppCommand::EditSkin(id)` opens one of the user's skin packs
  (`files::skin_dirs` of `packs/`, by manifest id; never a bundled root)
  in place; `NewSkin(None)` a blank double-resolution skin ("New skin"),
  `NewSkin(Some(id))` a copy of any installed skin (`Content::pack_dir`,
  named "<name> copy", origin dropped, its other files and authors kept);
  each checked first (`olr_edit::skin::check_dir`, then
  `olr_edit::pack::load` and `SkinDoc::new`): a bad pack or a layout the
  editor does not take is refused with why on the Editor screen
  (`editor_message`), never a crash. `act` carries out `Save` (in place
  through `olr_edit::pack::save`, or the first save a new pack in
  `packs/` with an id `pack_id(name)` free among the installed packs and
  the folders there, "skin" for a name without a letter or digit; a failed
  write removes the new folder) and answers `Saved(id)` or `Failed(why)`;
  `tick(now)` steps the preview (at most 0.25 s). The shell owns it
  (`Shell::skin_editor`): while open `App::frame` draws
  `olr_skin_editor::ui::show` instead of the menus and
  `Shell::skin_frame` runs it; every save lists the content again
  (`list_content`, `refresh_editor_packs`: `MenuData::set_editable_skins`
  with `files::editable_skins`), so the profiles' skin picker offers the
  skin at once; Close brings the Editor screen back. The window's close
  asks it first (`Shell::request_close`: `Wait` while its Unsaved dialog
  is up). No recovery copy (a deviation from the level editor's, in
  STATUS.md). Tests: `tests/skin_editor.rs` (a copy of Firefly saved to
  `packs/` with Firefly untouched, in the catalog's skins and the Skin
  tab's yours, then edited in place; OLX's default made double and
  saved; a blank one closing unasked; a bad pack and a built-in Edit
  refused with a message; the window's close asking).
- **Mod editor** (`editor/mods.rs`, `shell/editing.rs`, issue #17;
  decisions in `docs/porting/STATUS.md`, "Mod editor (issue #17)"):
  `ModHost` holds `olr-mod-editor`'s state and does its disk work. The
  Mod tab's `AppCommand::EditMod(id)` opens one of the user's mod packs
  (`files::mod_dirs` of `packs/`, by manifest id; never a bundled root)
  in place; `NewMod { from, name }` first makes a new pack in `packs/`
  (`new_pack`, `Template::From` of `Content::pack_dir(from)`, its id
  `pack_id(name)` free among the installed packs and the folders there,
  "mod" for a name without a letter or digit) with `editor/mod.ron`
  naming `from`, lists the content again and opens it; `RecoverMod(id)`
  a recovery copy (`Store::mod_recovery_dir`, `recovery::load`; its
  origin must be in `packs/`), unsaved. The base mod named by
  `editor/mod.ron` is loaded from the content when installed (the
  Original values). `act` carries out `Save` (`olr_edit::pack::save` in
  place, which refuses errors; the recovery copy removed), `LoadMod` (Add
  weapon's other mods, by the catalog's pack folders) and `Close` (the
  copy removed). `tick(now)`: the recovery copy (`recovery::write`,
  `mod-recovery/<pack id>/`) when unsaved changes have waited
  `RECOVERY_INTERVAL`; `flush_recovery` at a quit that does not ask
  (`Shell::finish`). The shell owns it (`Shell::mod_editor`): while open
  `App::frame` draws `olr_mod_editor::ui::show` instead of the menus and
  `Shell::mod_frame` runs it; a save lists the content again; Close
  brings the Editor screen back; the window's close asks it first
  (`Shell::request_close`). `refresh_editor_packs` also fills
  `MenuData::set_editor_mods` (`files::editable_mods`, the mod
  recoveries). **Play** (issue #18; STATUS.md "Mod editor Play (issue
  #18)"): `ModHost::set_play` gives the editor the catalog's levels and
  the settings of `MOD_PLAY_FILE` (`mod-editor.ron`, `PlaySettings`;
  without an installed level there, Local Play's), `ModOut::SavePlaySettings`
  stores them (`Store::save_file`), and `ModOut::Play` is the shell's
  `play_edited` of an `EditedPlay { level: PlayLevel::Installed, game_mod:
  PlayMod::Edited(pack) }`: `setup::load_edited` checks the pack as a
  save would (`olr_edit::pack::mod_pack`) and loads it with
  `Content::load_mod_pack` (the session plays `Assets::m` alone). It runs
  as the level editor's Play (`editing_play` counts either editor): the
  app draws the match, not the mod editor, with the Stop button; Esc or
  Stop (`stop_edited`, which answers `PlayEnded`), F5, a match over and
  the window's close (the match ends first) as there; a Play that does
  not load is the editor's error dialog ("The mod could not be played").
  **Practice** (issue #20; STATUS.md "Mod editor practice (issue #20)"):
  `ModCommand::Practice` is an `EditedPlay` with `practice:
  Some(olr_mod_editor::Practice)` (`None` for every other Play), run as
  Play is; `setup::practice_setup` makes it a sandbox: the weapon in the
  player's first slot, then the mod's first others, 1 to 4 idle targets
  (pressing nothing, as `Personality::Idle`), `LoadingTime` 100, or
  `FAST_LOADING_TIME` (0) with Fast reload; a weapon the mod lacks is the
  editor's error dialog. `Assets::practice` turns on the targets' health
  bars (`SceneOptions::show_health_bars`, off in every other game) and
  `Session::last_hit` (the last `Damage` event's `Hit { worm, amount,
  dealt }`, cleared by a restart; `None` in other games), which the
  overlay draws at the picture's top centre (`overlay::last_hit_text`).
  `play_settings` fills the practice level with `shell::editing`'s
  `PRACTICE_LEVEL` (Castle Strike) until one installed is chosen.
  **Art and sounds** (issue #19; STATUS.md "Mod editor art and sounds
  (issue #19)"): the import browser opens in the home folder
  (`set_browse_dir`); `ModCommand::List` is `files::list` (as the level
  editor's), `ReadFile` `files::read_file` (a regular file only, at most
  `olr_edit::mods::art::MAX_ASSET_BYTES` by its metadata and as read),
  `PlaySound` `ModOut::PlaySound(bytes)`: the shell's
  `Audio::preview` (the menus' device; a sound that does not decode is
  a warning and the editor's status). Play needs nothing new: the
  edited pack's sounds, images, glow masks and looks load with it.
  Tests: `tests/mod_editor.rs` (a copy of Classic made,
  edited, saved and opened again with Classic's originals, Classic
  untouched; a built-in Edit refused; a recovery copy written, offered
  by the next run, recovered, removed on Save and Close; the window's
  close asking; issue #18's
  `play_runs_the_unsaved_mod_and_escape_comes_back` and
  `closing_the_window_during_play_asks_first`; issue #20's
  `practice_runs_the_weapon_shown_and_escape_comes_back`; issue #19's
  `an_imported_sound_plays_in_play`: imported through the host's browser,
  ▶, set as the bazooka's fire sound, in the match's mod, decoded by a
  mock device and heard when fired; and
  `a_look_light_set_in_the_editor_lights_the_level_in_play`: Classic's
  copy unlit, the rocket's light makes the match lit and is an emitter
  of its colour), `files`' `an_import_reads_a_bounded_regular_file`, `setup.rs`'s
  `edited_play_runs_the_unsaved_mod_on_an_installed_level` and
  `practice_holds_the_weapon_against_idle_targets`, and `session.rs`'s
  `practice_keeps_the_last_hit` (equal to the last `Damage` event; none
  outside a practice).
- **Material editor** (`editor/material.rs`, `shell/editing.rs`, issue
  #44, its previews issue #45 below; decisions in `docs/porting/STATUS.md`, "Material editor (issue
  #44)"): `MaterialHost` holds `olr-material-editor`'s state and does its
  disk work. The Material tab's `AppCommand::OpenMaterialEditor` lists
  the built-in materials (`olr_content::materials_dir` of the last bundled
  root, read-only) and the user's (`Store::materials_dir`) with
  `olr_edit::material::list_materials` (a file that does not read a
  warning); `RecoverMaterial(id)` opens a recovery copy, unsaved;
  `DiscardMaterialRecovery(id)` removes one. The user's folder inside the
  built-in one is refused. `act` carries out `Create` (never over a file
  there), `Save` (`material_text`: checked), `Rename` (the new file, then
  the old one removed) and `Delete` in the user's folder only, each
  written through a hidden `.<file>.tmp` and a rename. Recovery: one
  file a material, `Store::material_recovery_dir()`'s `<id>.ron`
  (`write_material_recovery`, canonical RON, unchecked), written by
  `tick(now)` when unsaved changes have waited `RECOVERY_INTERVAL` and by
  `flush_recovery` (`Shell::finish`), removed once they are saved or
  thrown away, another material's copy is written, or the editor closes;
  `list_material_recoveries` (damaged ones listed by id: unreadable, or
  holding another id), `load_material_recovery` (bounded,
  `MAX_RON_BYTES`), `discard_material_recovery` (ids only). The shell owns
  it (`Shell::material_editor`): while open `App::frame` draws
  `olr_material_editor::ui::show` instead of the menus,
  `Shell::material_frame` runs it, Close brings the Editor screen back,
  the window's close asks it first (`Shell::request_close`), and
  `refresh_editor_packs` fills `MenuData::set_material_recoveries`. The
  level editor lists the library when it opens, so a saved material is
  offered by its Add from library. Tests: `tests/material_editor.rs`
  (the host on a temporary built-in folder: listed, Edit a copy and New
  written and saved, a file there never written over, the built-in
  folder byte for byte unchanged; Rename and Delete; the recovery copy
  written after the interval, offered, recovered, removed on Save, Don't
  save and Close; damaged copies listed and refused; through the shell
  over the bundled content: the tab opens it on the built-in set, the
  window's close asks, a crash's copy is offered and discarded).
  **Live previews** (issue #45; STATUS.md "Material editor's live
  previews (issue #45)"): `editor/heat_preview.rs`'s `HeatPreview` is a
  second `olr-render` `Renderer` (lit, IntegerNearest, `OUTPUT_FORMAT`)
  on the window's device, drawing `olr_material_editor::preview`'s scene
  (`SCENE` 136 × 72: two identical patches of air over dirt with a rock
  boulder and bedrock, a material on their front and back wall, value 1
  the material open and 2 the one compared, an 8-pixel rock wall without
  one between them) under a dark `LookFrame` (ambient 0.1) into a
  `PICTURE` texture (`PICTURE_SCALE` 4 texels a world pixel).
  `command(PreviewCommand)`: `Blast { at, size }` carves the theme's hole
  `clamp(damage, 0, 4)` at that pixel of both patches
  (`ColorEdit::Carve`), uploads it (`update_terrain`, issue #67: before
  the deposit, as play does, so the crater's back keeps its slice) and
  deposits `olr_look::engine::explosion_heat` there; `SetCompare(m)`, `Restart` (the scene again: `upload_terrain`
  clears the heat), `TimeScale(f)` (clamped to `MAX_TIME_SCALE` 16).
  `set_open(m)` each frame: a changed material changes the table
  (`Renderer::set_surface_materials`, the heat kept; `set_surface` when
  there is no heat to keep; a material that does not check keeps the last
  values). `frame(elapsed_ms)`: `advance_heat(min(ms, MAX_FRAME_MS 100) ×
  scale)`, `update_terrain`, `render_lit`; after the heat went idle
  `SETTLE_FRAMES` (30) more, then none until a change (returns whether
  it drew; `drawn()`). `round_holes()` stands in for a theme's holes.
  `MaterialHost` queues the editor's `MaterialCommand::Preview`s
  (`take_preview`, at most `MAX_PREVIEW_COMMANDS` 64 kept), holds the
  holes (`set_holes`: `Shell::material_command` loads `PREVIEW_THEME`,
  `olx-theme-dirt`, from the content) and hands the panel its picture
  (`set_preview(PreviewImage)`). The app (`app/preview.rs`, the field
  `App::material_preview`): `sync` before the UI makes it as the editor
  opens (from the editor's state: material, compare, time scale),
  registers its view (`Gpu::register_texture`, egui-wgpu's native
  texture, linear; `free_texture` on the first frame after the editor
  closed); `frame` after `Shell::material_frame` carries out the
  commands, gives the material open and, only when the editor was drawn
  that frame, steps and draws it (a window made again keeps it: the
  device stays). Closed or hidden it costs nothing. Tests:
  `tests/preview_gpu.rs` (a binary of its own in the GPU tier and `check
  --render`, `OLR_GPU_TESTS`: `a_blast_heats_the_patch_and_it_glows`,
  `the_heat_cools_and_an_idle_preview_draws_nothing` (4× steps four
  times as fast), `side_by_side_draws_two_materials_differently`,
  `a_field_edit_changes_the_next_frame_and_keeps_the_heat`, issue #67's
  `a_second_blast_on_a_hot_patch_heats_its_crater`),
  `tests/material_editor.rs`' `the_previews_commands_wait_for_the_app`
  and the dirt theme's holes, `heat_preview`'s unit tests (the scene's
  patches identical, the wall without a material; the discs).
- **Deathmatch** (plan 6 ruling R5): `setup::match_options` (the humans
  are worms 0 to `locals - 1`, then the bots at one difficulty, OLX's LX56 settings, the
  `Default` loadout: every weapon at 100 %, issue #5; there is no
  `--bonuses`: bonuses are gone from play, `SandboxOptions::bonuses` is
  the soak test's) feeds `olr_content::build_match`, the same assembly as
  `olr-cli match` (loadouts, the Deathmatch and its respawn loadout, the
  shared area cache, the recorder). The command line's human gets random
  enabled weapons (`olr_content::Player::Human`). The bots think on
  the main thread inside `Session::tick`, in worm order, before the world
  ticks; `MatchDriver` times each tick's thinking (`think_ns`, the F2 panel
  and the exit summary's p50/p99) and counts thinks over budget (none, by
  design). The recording ends `RECORD_AFTER_END` (3 s) after the game over
  or at `MAX_TICKS`; `--record f` saves it at exit and on each rematch
  (`f`, `f-2`, ... : `drive::record_path`).
- **Match HUD** (`olr_ui::hud`, an opaque bar along the bottom, or the
  side layout: `Frame::arrangement`, the options' `hud_layout`;
  `hud::view(session, &Frame)` builds its `HudView` from
  the session and the frame; with 2 or more views (split screen, issue
  #30) also `HudView::views`, each view's followed worm (`followed_view`)
  and colour for its compact HUD, without the keys (`tests/locals.rs`:
  four players' names and colours in view order, a split replay's
  viewers, one player alone the solo HUD), and every view's worm's
  minimap dot big in a replay; no weapon rows
  while the selection is open, `Shell::selecting`, the world's being a
  placeholder until Done): names ("You", "Bot N"; a replay's recorded names,
  over the worms on screen: `worms_on_screen`), the event log
  (`Session::log`, an `olr_ui::hud::Log`) from `Message` events (OLX's
  texts, the last 64 lines; kills show
  no weapon: `olr-sim` does not record the injuring weapon, and OLX drops
  `" with <weapon>"` then), the time-limit clock (red in the last 10 s; it
  stops at the end, as `Deathmatch::time_left_ms` does) and kill limit,
  the minimap (`hud::MinimapTexture`: the colour terrain as an egui
  texture, made again every `MINIMAP_TICKS`), Tab's scoreboard
  (`World::scores`, the mode's order; `hud::ScoreKey` hides it when the
  window loses focus, which never delivers Tab's release), and on
  `GameOver` "Game Over", then after OLX's `GAMEOVER_WAIT` (3 s) the
  results (the menus' screen; the command line's:
  `olr_ui::screens::results::panel` with F5 rematch, Esc quit). `Session::tally` counts kills by bots,
  each local player and suicides (Kill events) over the run's match time.
- **Window** (`window.rs`, the user's ruling after the plan 6 playtest):
  not resizable, no maximise button; `--fullscreen` is borderless. It opens
  hidden and shows with its first presented frame (`app::reveal_now`, at
  the latest after `REVEAL_AFTER_FRAMES` or `REVEAL_AFTER`, 1 s; autoplay
  plays no step before, `Seen::shown`; the user's report of a white
  rectangle while the GPU was set up); while hidden its frames are drawn
  from `about_to_wait` (Windows sends no redraw to an invisible window).
  Wayland has no hidden windows. On
  Linux its Wayland app ID and X11 class are `APP_ID`, the Flatpak's
  application ID (`tools/flatpak/`). Size
  and mode changes come only from the game (`size`, `fullscreen` script
  commands, the options menu: `video.rs`) through `window::Requests`: the latest
  wins, applied at the start of a frame before `acquire`, at most once
  every `MIN_CHANGE_INTERVAL` (250 ms, creating the window counts as a
  change: `Requests::opened`). How a change reaches the window is
  `window::plan`: on Wayland the window is **made again** (`App::recreate`:
  a new winit window at the new `window::Shape`, its surface on the same
  device by `Gpu::rebind`, which keeps the format and every GPU resource,
  egui's window state on the same context; keys held released; events of
  the closed window ignored, `App::current`; if the new window cannot be
  made, the old one takes the change in place), because WSLg left pieces of
  a changed window's client-side frame on the screen and a window made at
  its size never did (`window.rs` module docs: placement, the first frame
  callback's wait, why no window has the desktop's size). On X11 and elsewhere it
  changes in place, and a fixed window's min and max move with it
  (`window::apply`). While the window is hidden (minimised or
  occluded: no frames) pending changes wait; they apply, still paced, when
  frames resume. Why: WSLg's Weston 9 segfaults when winit's
  client-side decorations are resized quickly (their `wl_shm` pool grows
  while the compositor still reads it): resizes 50 ms apart crashed it
  every run, 250 ms apart never. `--window-system auto|wayland|x11` (or
  `OLR_WINDOW_SYSTEM`) picks the window system; auto stays Wayland on WSL
  (paced at 60 Hz; WSLg's X11 path is robust but draws uncapped). When
  winit ends its loop with an exit failure the game did not ask for, the
  display server probably dropped the connection: `window::lost_connection`
  says so and names the other window system.
- **Surface** (`surface.rs`, Review Focus 4): a state machine over a
  `Surface` trait: no frame while minimised or occluded (`drawable`),
  configure on resize (once a frame, at the latest size), reconfigure and
  retry on `Outdated` (Wayland's first frame), recreate on `Lost`, bounded
  retries; every frame it gives is presented (a suboptimal one too; the
  surface is configured again before the next). While not drawable the
  loop stops redrawing and waits for window events (`ControlFlow::Wait`):
  the sandbox pauses, and the hidden time is not caught up.
  `gpu.rs` clamps every surface and screenshot size to the device's
  `max_texture_dimension_2d` (`clamp_size`), picks 8-bit RGBA (gamma
  space first: egui wants it, the readback reads 4 bytes a pixel) and
  Mailbox when there is one, else FIFO; `--uncapped` prefers Immediate.
- **egui textures** (`gpu::with_textures`): each frame's `TexturesDelta`
  is applied (set before drawing, free after) and drained whether or not
  a frame is drawn; egui hands each change out once (a lost font-atlas
  delta makes egui-wgpu panic on the next patch) and debug builds assert
  every delta was handled.
- **Overlay** (`overlay.rs`): names over worms (inside the picture at
  its edges: `name_rect`; only in game and not paused, `Shell::names_shown`,
  never through the selection, the pause menu or the results); each
  local player's worm's label is its current weapon's name, in white, while
  its change key is held now (`Overlay::changing[k]`, its input's) or for
  `WEAPON_NAME_TICKS` (OLX's 0.75 s) after a change made with it, with
  Previous/Next weapon or a Weapon 1-5 key (a change within
  `WEAPON_KEY_TICKS` of one held: a tap is read a tick or the input
  delay later), and after every tick a Weapon 1-5 key selects with
  (issue #48: held or pressed for the current slot, changed or not; not
  while firing without `WeaponCombos`, nor beyond the slots in play)
  (`Session::weapon_name_of(k, held)`, `weapon_name` the first's,
  `worm_label`; each change noted by `Session::tick_locals` and
  `Session::frame` from that player's keys, OLX's per-worm
  `bForceWeapon_Name`, `tests/weapon_name.rs`, `tests/locals.rs`); else no
  name over a local player's own worm (OLX
  draws none over a local human worm; plan 15a Task 8: `worm_label(i,
  locals, ..)` is `None`; a game with no local slot names every worm);
  under the place of each local player's worm's name, while
  its current weapon reloads, a reload bar filled to the slot's charge
  (`reload_fraction`, `reload_bar`: `RELOAD_WIDTH` 14 world pixels, OLX's
  reloading red brightened, outlined; the user's request 2026-10-18: one's
  own only, the others' reload stays hidden as in OLX), summed damage popups
  (`Damage.dealt`), the HUD (OLX's kills; the board's deaths and
  suicides, every one, not the worm's score-gated counters), and F2's counters: FPS, frame and *busy* ms
  (the frame without the VSync wait), tick ms, entities, projectiles,
  voices. egui allocates per frame; the world path does not.
- **Scripts** (`script.rs`): `--script <file>` or `--demo` press keys
  through the input path at given ticks, plus `shot`, `restart`, `quit`,
  `scores on|off` (Tab), `size <w> <h>`, `fullscreen on|off` (window changes wait out
  `MIN_CHANGE_INTERVAL`, so a `shot` within 250 ms may show the old
  size); `--shot <tick>`, `--autoquit-after <s>`, `--stats` (counters
  each second) and the exit summary let an agent run the game in a real
  window and read the PNGs (plan 5 ruling R3). `--frame-log F
  [--frame-log-skip S]` (plan 8; S 3 s by default) writes `stats::FrameLog`
  as RON at exit, for `cargo xtask envelope`: the frames after the first S
  seconds (frame-to-frame and busy ms, `Pct`: p50, p99, max), the ticks
  after S × 100, the frames per second, the surface's size at the last
  frame (`App::drawn_size`: a `size` script command that did not take
  shows here) and the GPU line (adapter, backend, present mode); the
  test `frame_log_sample` keeps `fixtures/bench/tool-output/frame-log.ron`
  equal to what it writes (xtask reads it strictly; rewrite with
  `OLR_BLESS_TOOL_OUTPUT=1`). Every
  percentile is nearest rank (`stats::percentile`: rank `ceil(n × p)`,
  plan 8 ruling PF8; the exit summary's too).

- **Autoplay** (`autoplay.rs`, spec §21 G7, plan 7 task 8): `--autoplay
  script.ron` drives the real app from a RON script of steps (`at_frame`,
  `ui`, `within`): `Click(label)` (the widget found by label in egui's
  AccessKit tree, enabled for the run: `widgets`, `click_point`; a pointer
  press and release injected into the next two frames' `RawInput`),
  `ClickUntil(label, shows)` (clicks `label` until an enabled widget
  `shows` appears, again every `RECLICK_FRAMES` (30) frames while it has
  not: the effect, not the sending, ends the step; the e2e's Internet
  tab row, whose Join once never showed after one click on a loaded
  machine, `click_until_clicks_again_until_its_effect_shows`),
  `Key(name)` (egui and `App::key`, as the window delivers a key),
  `Type(text)` (egui's text event to the focused field: click it first),
  `Press`/`Release(action)` (the input path, every seat hearing it),
  `Press2`/`Release2(action)` (keyboard 2's key for it: a second local
  player, issue #32; pads cannot be faked), `WaitFor(screen)`,
  `Wait(frames)`, `Drag(label, from, to)` and `ClickAt(label, at)`
  (fractions of the widget's rect, written `(0.3, 0.5)`; a drag moves in
  `DRAG_STEPS`, 8, one a frame: the editor's canvas), `Screenshot(name)`
  (`<shot dir>/<name>.png`, checked not blank: `sanity`), `Assert(Screen |
  Label | LabelContains(text) | WindowSize | Speed | Playing | TicksAtLeast
  | Lobby(members: n))` (the online lobby's members), `Quit`,
  `WaitForExit` (the menus' Quit). A step that
  waits fails the run after `within` frames (`DEFAULT_WITHIN`); the game
  then exits with the error. Each step is a JSON line in `<shot
  dir>/autoplay.jsonl`, the last `pass` or `fail`. `--fake-clock` (only
  with `--autoplay`: clap requires it, and the app ignores it without
  one, since a fixed tick count a frame would change a played game's
  speed; `tests/speed.rs`): every
  frame is `FAKE_FRAME` (20 ms) and runs `FAKE_TICKS_PER_FRAME` (2) ticks
  (max speed `FAKE_MAX_TICKS`), egui's time and the window pacing follow
  it (`App::now`), the first menu seed is 1: a run gives the same frames,
  replay and screenshots every time. `--data-dir` (a data directory
  instead of the platform's) and `--no-sound` (no device) keep a run
  away from the user's files and speakers. `fixtures/autoplay/menus.ron`
  (with `settings.ron`) is `cargo xtask e2e`'s script, on Xvfb in `full`;
  `fixtures/autoplay/online.ron` its online run against `olr-server` (real
  clock).
- **Soak** (`tests/soak.rs`, `cargo xtask full`): `soak_sandbox_10_minutes_headless`
  plays 60 000 ticks of `Session` with chaos keys, an offscreen render every
  10th tick (main thread only) and a kira mock device for sound, and bounds
  the counters; it needs `OLR_GPU_TESTS`, content and an adapter, else skips.

Test: `cargo nextest run -p olr-game` (the pure parts; `setup`, `session`,
`tests/finding.rs` (plans 12a, 12b; olr-discovery's in-process
`TestRelay`, the server in-process (`olr-server` a dev-dependency with its
`test-hooks`: `test_gateway` pins its base to 127.0.0.1; `allow_local`,
since its joiners' candidates are loopback ones, final review A-M4), no STUN, every
endpoint's candidates on 127.0.0.1 (`Shell::test_ice`, this crate's
`test-hooks` feature, enabled by its own dev-dependency, ruling NT-1):
nothing leaves the machine, and `online::menus` sets no relay, no STUN
server and no port mapping for every other test, as
`fixtures/autoplay/settings.ron` does, and turns host migration off,
`test_migration(false)`, D7):
`a_listed_server_is_joined_from_the_internet_tab` (listed with its name,
map, region and address, pinged, an impostor key refused, the listed
key's lobby, the server's invite in upper case joins, blocked),
`a_public_game_without_an_address_is_joined_through_the_relays` (the
lobby's listing and reach lines, the invite without an address, listed
without one, its invite joining through Nostr with the steps as the
waiting text, the client's "connected directly to 127.0.0.1:…" and the
host's "direct" tag, the listed game joined through the relays, a
damaged code, a direct join, the closing note takes it off the list),
`a_listed_server_without_an_address_is_joined_through_nostr` (Task 9's
end to end), `the_internet_tab_without_relays` (a closed port: "No relay
reachable", no frame over 250 ms, hosting and a direct join unaffected;
none set: at once), `the_relays_linger_after_the_multiplayer_screen` (a
fake clock: reused within 30 s, closed after, the selection cleared),
`relay_edits_apply_to_the_open_pool`, `block_edits_apply_to_the_open_list`,
`a_public_host_follows_relay_edits` (announced and listened on the new
relay: its invite joins through it alone),
the app's own frame path (`App::network_frame` on `online::app_of`, an
`App` without a GPU backend; RED against the old gate):
`the_apps_internet_tab_lists_games_without_an_online_game`,
`the_apps_lan_list_fills_without_an_online_game` (`Shell::test_lan_port(0)`:
the announcement sent to 127.0.0.1, never broadcast),
`the_app_joins_a_typed_host_name` ("Looking up localhost:…", then the
lobby), `finished_shutdowns_are_forgotten`,
`a_typed_name_is_looked_up_off_the_frame` (`localhost`: no peer at the
Join, the lobby frames later; Back gives the lookup up),
`an_invite_only_game_without_an_address_says_so` (final review B-I2,
B-I3; through the relays, or joining nothing without them),
`quitting_deletes_the_router_mapping` (NT-2: a loopback `FakeGateway`,
`LateUpnp`, external 127.0.0.2: the reach line and the invite's mapped
address, leaving returns at once with the mapping still there, the
`Unmapping` wait sees it deleted),
`hosting_again_on_the_same_port_keeps_its_mapping` (ruling RH-1: a
`LateDelete` router, leave and host again at once on the same port: the
new game's TCP and UDP mapping still there after the old deletion took
effect; the port is one below the ephemeral range, `unclaimable_port`, so
no other test's port-0 bind takes it between the leave and the new host),
`tests/deathmatch.rs` (a Deathmatch to its results, the bots' think
budget, a recording replayed through a replay session),
`tests/join.rs` (issue #32: 2, 3 and 4 seats, keyboards and fake pads,
join local play through the menus' join step, each steps its own
profile, readies, loads with a worm each in its profile's look and
skin, chooses its weapons with its own keys, the last Done arming all;
in play only its own keys reach its worm; a joined pad's Start pauses
and the pause menu hears it alone, an unjoined pad's does not; pad 4
unplugged stands still, back as pad 9 its first button claims the seat,
the lineup follows and F5's rematch keeps it), `tests/locals.rs` (issue #29: four local players on slots 0, 2, 3 and 5
between two bots, each moved by its own scripted keys and no other's,
their names, colours, skins and views, the throws heard each tick theirs
alone, the tally, deaths and spawns each player's, the replay's rows the
scripts' and its viewers the slots, played back with four views to the
same checksums; each player's weapon name its own; issue #30: the
HUD's compact panel per view, live and played back), `players.rs`' unit
tests (each seat hears its own keys; any seat's Start pauses; issue
#32: the join step's seats, the menus' seats, a pad back by UUID or by
a button),
`tests/throw_sound.rs` (only the player's rope throws are heard against
seven bots, none for bots alone, a replay's viewers'; online, in
`an_online_match_runs_headless`: the client's own only), `tests/menu.rs`
(starts with missing or broken content, rapid menu actions) and
`tests/profiles.rs` (the preview's textures; a menu match wears the
profile's look), `tests/online.rs` (a local match whose player is slot 2,
a watcher following any worm, a `Driver::Online` session against a host
on 127.0.0.1: 300 frames on a fake 10 ms clock, the HUD's scores the
host's confirmed ones, the end from a confirmed tick; two `Shell`s, one
hosting on port 0 and one joining, to the lobby, a bot, ready, start,
equal confirmed scores, the results, the replay saved, back to the
lobby and leaving; chat in game reaching the other's HUD log; the
hidden window's `App::hidden_frame` ticking the match; the network pump
through the apps' frames (`App::hidden_frame`, real clock, a 0.2-minute
match with a bot): `a_host_whose_frames_stop_keeps_hosting` (3 s without
the host's frames: the client confirms 250 ticks more, the host's first
frame after counts fewer than 50 ticks on the meter, then both reach the
same results, no colour drift), `a_client_whose_frames_stop_stays_joined`
(3 s without the client's: the host fills fewer than 50 of its inputs, no
"Connection problem" on any frame, still in), `a_match_load_does_not_silence_the_host`
(`test_slow_load` 2.5 s: the client confirmed 150 ticks during it), each
RED without the pump; `frames_every_30_ms_play_as_before` and `_45_ms_`
(both players hold Right: every confirmed final input of both slots
holds it, no pump poll, no resync; RED at the old 20 ms); `net/pump.rs`'
unit tests on a fake peer (a `Poll`): the pump stands back while the UI
polls every 5, 30 and 45 ms and polls about every 10 ms once it stops,
each at least `PUMP_AFTER` after the UI's; it presses the last latched
keys for `PUMP_HOLD`, then none, and only those still held; the queue's
bound (confirmed ticks thinned first, every kill and chat kept, never
across a control event); control events never let go;
`a_tap_before_a_stall_is_not_repeated` (app path: a Fire tapped before
the host's frames stop shows on one tick, the held Right on all);
plan 12c Task 9's host migration through the apps' frames, against an
in-process `olr-server` (port 0, no relay or STUN, no port mapping, its
base 127.0.0.1, one bot; `MigrationRig`: each player welcomed before the
next joins, so the first is the first successor):
`a_frozen_server_hands_the_match_to_the_first_player` (the server no
longer polled 3 s in: Anna's HUD line "The host is gone: you are taking
over the game…", then "You are now hosting", Bert's "Anna is now
hosting" within 8 s; both to the results with equal confirmed scores and
results, both replays saved and the same match (`same_match`: but for
the viewer), Anna's lobby a host's with `MIGRATED_LISTING`; about 12 s),
`the_host_lost_screen_shows_and_the_replay_is_saved` (the server and
Anna frozen, her pump stopped; Bert without a standby gets the Host lost
screen with `HOST_LOST` within 13 s and his replay saved; about 16 s),
`a_late_rejoiner_watches_until_the_next_match` (a fourth player, Dan,
makes Anna's quorum while Cid is away, ruling FR12d-Q, in this and the
next; Cid's rejoin held 6 s:
`late_rejoin` in his log, his view kept as last seen until the new
host's start, then he watches; RED without the freeze or olr-online's
kept world; about 11 s), `a_late_rejoiner_after_the_match_goes_to_the_lobby`
(held 7 s while Anna ends the match early, `end_match`: Cid's Lobby
shows, he is seated at the gathering and his Ready reaches Anna; RED: the
frozen view stayed; about 15 s), `a_host_heard_again_calls_the_migration_off`
(Bert's datagrams dropped 3.5 s, `test_outage`: his migration line, then
`HOST_BACK` in his log and HUD, the line gone, the match on, nobody took
over; about 9 s), `a_host_cut_off_from_its_players_shows_why` (ruling
SB-2: the hosting game with two players, Anna and Bert (FR12c-1: with
one it plays on), its network blacked out 6 s: its Host lost screen,
`CUT_OFF`, `host_lost_hosting`, its replay saved, while Anna takes over
and plays on; RED without olr-online's `cut_off_check`; about 5 s), and
the pump's two tests run again with
migration on, `a_host_whose_frames_stop_keeps_hosting_with_migration`
and, in the full tier only (`OLR_FULL_TESTS`: its migration checks pass
without the pump, a poll receives before it judges the host silent),
`a_client_whose_frames_stop_stays_joined_with_migration` (ruling
PF12c-2: no migration line, handover, call-off or Host lost on either
during or after the 3 s, the match to its results; the host's RED
without the pump: the client's "finding who takes over…" line);
`overlay`'s `the_top_line_is_the_migrations_else_lagging`;
`with_pumped` takes the mark; a panicking pump poll said at once; the
UI's poll, called while a pump poll holds the peer (gated by the test,
so every round is contended), begins after it and before any other pump
poll (order, not milliseconds: review Important 2; the latched keys
judged on the time the pump gave each poll, the waits bounded by counts
with generous deadlines); stopping joins (1 s bound) and gives the peer
back, nothing polling it after; a nested lock panics, through another
peer's too; `--net-sim
typical`'s setup playing and starting the next match; leaving right
after the end keeps both replays; the flags not remembered; LAN off:
`Shell::set_lan(false)`; each under 20 s), `tests/downloads.rs` (a
joiner whose content lacks Dirt Level downloads it in the lobby into its
`Store::open` cache and plays the match from it; over the prompt
(`OnlinePeer::test_prompt_limit` through `Shell::with_peer`: olr-online's
`test-hooks` in the dev-dependencies) it waits, queued, until Download;
a replay of that level,
the level only in the cache (`downloads`, or `downloads-2` while the
shell holds `downloads`), plays back from the Replays screen and is
refused, naming it, without; a command-line replay finds it while
another holder locks the cache, and makes, locks and writes no cache; the options' Clear, refused while a match
is online, empties the cache), `tests/ingame.rs` (the weapon selection, the pause and a
valid replay, the results against the world's scores), `tests/options.rs`
(video through the paced requests,
a window made again keeps the game ticking, volumes live,
`the_blood_amount_applies_at_once_and_never_to_the_world`, the saved window with the flags on top) play the imported
levels when `OLR_CONTENT_DIR` is set, as does `tests/speed.rs` (fast-forward
never changes the simulation; a played game refuses it); `tests/user_packs.rs`: a user pack
in `<data>/packs` over the bundled one of its id, a new one added).
