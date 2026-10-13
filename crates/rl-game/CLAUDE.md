# rl-game

The game binary (spec §8.6, §19 E10-E12, §21): a winit window, the 100 Hz
fixed-tick loop, interpolated wgpu rendering with cosmetic entities, sound,
input, an egui overlay and the menus (`rl-ui`). It plays the M5 sandbox
(E11: the player and N idle targets on a level with a mod's first five
weapons), a Deathmatch against bots (M6) and replays.

```
cargo run --release -p rl-game                      # the menus
cargo run --release -p rl-game -- --level "CastleStrike" --mod "MW 1.0" --targets 3
cargo run --release -p rl-game -- --mode deathmatch --bots 4 --difficulty medium \
    [--lives L] [--kill-limit K] [--time-limit M] [--seed S] [--record f.rlrp]
cargo run --release -p rl-game -- --replay f.rlrp [--speed 1|2|4|8|16|max]
cargo run --release -p rl-game -- --spectate --bots 8 [--speed 16]   # bots alone, watched
cargo run --release -p rl-game -- --host [name] [--port 23400]        # host online, the Lobby opens
cargo run --release -p rl-game -- --join 192.168.1.20[:23400]         # join online
cargo run --release -p rl-game -- --net-sim typical                   # feel a network (spec §9.10)
```

- **Layer 6.** Uses `rl-ui`, `rl-content`, `rl-present`, `rl-scene`, `rl-fx`, `rl-render`,
  `rl-audio`, `rl-input`, `rl-sim`, `rl-modes`, `rl-bots`, `rl-replay`,
  `rl-terrain`, `rl-pack`, `rl-online`, `rl-netcode`, `rl-proto`,
  `rl-lobby` (identities, host settings), `rl-net` (endpoint settings,
  LAN announcements), `rl-discovery` (the hosted lobby's summary,
  `GameSummary::of_lobby`, shared with rl-server), plus winit,
  wgpu, gilrs and egui / egui-wgpu / egui-winit 0.36.2 (exact pins). Logs
  are `eprintln!("warning: ...")`, each once.
- **Menus first** (`main.rs`, `shell.rs`, plan 7 task 2): without a flag
  that chooses a game (`GAME_FLAGS`: `--mode`, `--replay`, `--level`,
  `--mod`, `--targets`, `--seed`, `--script`, `--demo`, ...) the game opens
  the main menu, with `settings.ron`/`profiles.ron` from the data directory
  (`rl_ui::Store::platform`; warnings logged; `Store::notices`, a newer
  game's file left untouched, on the main menu) and the first match seed
  from the clock (each seed is in its replay). With such a flag it plays
  that game at once as before (scripted runs and tests: Esc quits, F5
  restarts, `--record`). `--shot`, `--autoquit-after` and `--stats` work
  in both. `Shell` holds the `UiState`, the menus' data, the store and the
  game playing (`Playing { assets, session }`); `app.rs` draws
  `rl_ui::screens::show` over the overlay, feeds the actions to
  `Shell::act` and does the `Outcome`s (`app/menus.rs`: quit, bindings,
  video through `video::Video` and the paced window requests, a started
  game's upload). A
  start is loaded by `Shell::poll` after the frame that showed "Loading…";
  a failure (a pack missing or broken: `setup::load_match`, `load`,
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
  `Session::replay` with `rl_ui::replays::save_kept`. Every Deathmatch's replay is saved in the
  data directory's `replays/` when it ends, restarts or the game quits
  (`rl_ui::replays::auto_save`, none of no tick); the Replays screen plays
  them. Settings are written at most once a second (`Shell::flush`) and
  at exit. The human's profile names and colours the local slot's worm
  (`PlayerLook`) and its skin is that worm's (`Assets::skin`); every
  other worm, and every worm of a replay, wears OLX's default skin
  (`Assets::bot_skin`, as OLX's bot profiles) in OLX's bot colours away
  from the player's (`rl_ui::bot_colours`; a replay's worms in the
  default colours). Weapon selection's Done arms the local slot.
  The menus list and play `--content` with the data directory's `packs/`
  over it (`Content::open_layers`, spec §9.4; the command line's games
  use `--content` alone). **Import** (plan 7 task 7): `StartImport`
  spawns an `rl_ui::import::ImportJob` (one at a time; without a data
  directory the screen says so), `Shell::poll` feeds its reports to
  `MenuData::import` every frame, and when it ends lists the content again
  (`list_content`: the catalog, `MenuData::set_catalog` so re-imported
  thumbnails are made again, the skin preview reloaded) and reports
  `ImportFinished`; no game starts meanwhile (`UiState`); `finish` (quit)
  stops it, waiting at most `IMPORT_STOP_WAIT` (2 s).
  `Shell::update_preview(ctx)`, after the menus are drawn,
  uploads the profiles screen's skin preview (`MenuData::preview_wanted`,
  `rl_ui::skin_preview::frames`) as egui textures (`MenuData::preview`,
  from the next frame), loading the skin once while it stays the same.
- **Options** (`video.rs`, plan 7 task 4): `Video::change` asks the
  window requests for a size or mode only when the preset or fullscreen
  changed (fullscreen keeps its size for later) and returns the rest to
  apply at once (`Live`: UI scale; VSync or uncapped, `Gpu::set_uncapped`
  re-configuring the surface when the present mode changes; the filter,
  `renderer.settings`); interpolation is the shell's
  (`Shell::interpolation`, new games and the one playing). The window opens
  at the saved preset and mode (`Video::opening`, physical pixels;
  fullscreen keeps the preset for when it ends), so no resize follows its
  creation. `--fullscreen`, `--uncapped`, `--filter`, `--no-interp`,
  `--hud-layout` given
  on the command line win for the run (`video::Overrides`, applied before
  the `UiState` is made, so the options show them); `Shell::set_overrides`
  keeps the file's values for them until the player changes them in the
  options. Volumes: `master * sfx` to the menus' or the game's
  `Audio::set_volume` at once (`Shell::volume`). While the options capture
  a key, the app feeds `MenuData::capture` every window and gilrs event,
  and egui hears no key (`app::egui_hears`).
- **Single thread** (spec §8.6): the device is created and used only on
  the main (event-loop) thread; under WSL's d3d12 GL driver a thread that
  used the device crashes when it exits. kira runs its own audio thread;
  an import runs on its own (`olx-import`), which touches no device.
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
  window's centred 16:9 frame above the HUD bar (`rl_ui::hud::playfield`,
  whole pixels); `Gpu::draw` renders the world there
  (`Renderer::render_in`, black around it) and the egui overlay paints
  the bar (and the side layout's columns) over the rest; the names'
  `ScreenMap` uses the same rectangle (`rl_render::dest_rect_in`). The
  view is `rl_scene::Viewport::GAME` (427 × 192 world pixels) for every
  window and HUD layout (spec §19 E4 amended): resizes do not touch it.
  The UI's scale: every frame `rl_ui::theme::set_scale` with the
  display's scale factor, `scale_height(size, in_game)` (the frame's
  height in game, the window's in the menus) and the options' UI scale
  (`App::ui_scale`; 1 for the command line's games).
  `rl-render`'s goldens do not change.
- **Loop** (`app.rs`): each frame first acquires the surface texture
  (`Gpu::acquire`), so a VSync wait comes before the input is read, not
  between reading and drawing it. Then `TickClock` (`clock.rs`) runs the
  ticks owed, at most `MAX_CATCHUP_TICKS` (10) per frame, dropping the
  rest. Per tick: script steps, `input.latch_tick()`, `Session::tick` (`world.tick` →
  `audio.play_events` at the view centre → `Presentation::on_tick`, see
  `rl-present` → damage popups). Per frame: `Session::build` (scene,
  entities), the overlay, `Presentation::upload` (atlas if a worm was
  added, terrain dirty rectangles), `Gpu::present`, then a screenshot if
  one is due (one that fails is a warning, never the end of the game).
- **Fast-forward** (`speed.rs`, plan 7 task 8, the user's request): a
  game nobody plays (`Session::human()` false, worked out again at each
  restart: a replay, or `--spectate`'s
  bots alone, `Assets::human_plays`) runs at `Speed` 1×, 2×, 4×, 8×,
  16× or max: `--speed`, PageUp/PageDown (`App::change_speed`; both in
  `rl_ui::GAME_KEYS`), "Fast-forward N×" as a HUD notice, with the speed
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
  headless; its presentation is one `rl_present::Presentation` (a worm a
  mode spawns mid-match gets `add_worm`, which marks the atlas for upload).
  What it plays is `setup::Game` (`Sandbox`, `Deathmatch`, `Replay`,
  `Online`), loaded once into `setup::Assets`; who presses the keys is
  `session::Driver` (`drive.rs`): the player alone, the player and
  `MatchDriver`'s bots, `ReplayDriver` (a `rl_replay::Cursor` checking
  every checksum; a divergence is shown and logged once, the replay stops
  at its end), or `Online` (`online.rs`).
  **The local slot** (`Session::local`, `Assets::local_slot`: the
  sandbox's 0, a Deathmatch's first player that is not a bot, an online
  match's own slot; `None` for a replay, bots alone and an online match
  watched) gets the player's keys, name, look, the crosshair (the scene's
  `local`; with no local slot a local game's worm 0 keeps it, as OLX's
  local worms, and an online match watched has none), the camera
  (`Presentation::follow`), the HUD, the big minimap dot, the highlighted scoreboard and results row (`Row::followed`), the
  tally's player kills and `player_deaths`/`player_spawns`. Without one
  the camera and the HUD follow the worm chosen (`follow`, `follow_next`,
  worm 0 at first); an online spectator's PageUp/PageDown choose it
  (`Session::spectating`; "Watching <name>" on the HUD). **Per-worm
  looks** (`session::WormLook { name, colour, skin }`, one per worm,
  registered in worm order): local play's as above, an online match's the
  lobby's (`setup::load_online` loads every slot's skin once by id from
  the peer's content, downloads laid over, `Assets::skins`; one still
  missing is the default skin).
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
- **Online** (`online.rs`, plan 10 task 10; spec §3.4, decision N20):
  `Session::online(assets, OnlineDriver::new(peer, &start), ..)` after the
  peer's `MatchStarting` (the events after it in that poll go to
  `Session::present_events`); then `Session::frame(now, local_keys)` once a
  frame instead of `Session::tick`: one `OnlinePeer::poll` (the local keys
  from `local_keys`, which the netcode calls once per new tick: the input
  latch; the scene reads every simulated tick inside it,
  `after_tick_resimulated` for a re-simulated one), then the poll's events
  in a fixed order whatever their order in the poll (`online::present_poll`,
  pinned by `a_loaded_state_is_presented_before_the_polls_ticks`): a
  loaded state (`OnlineEvent::StateLoaded`: `Presentation::state_loaded`,
  `NetPresenter::reset`), the restored areas and ticks
  (`NetPresenter::apply`, its sounds at the camera; debug builds check the
  colour terrain after every advance, about 1 ms a frame at worst in
  `an_online_match_runs_headless`), `NetPresenter::confirmed`, and the
  confirmed ticks through `after_tick` (log, tally, results, clock,
  popups: never a predicted tick, so a damage popup comes late, at the
  confirmed tick's positions). The HUD's scoreboard and the followed
  worm's lives, kills and deaths are the last confirmed tick's
  (`Session::scores`); the newest predicted world is drawn
  (`Session::world`). At `MatchEnded` the peer stops presenting while its
  session goes on to confirm and drain: `present_events` takes the world
  presented last once (`OnlinePeer::take_ended_world`) into
  `Session::world` and freezes there (`frozen`; no clone per frame),
  pinned by `an_online_match_runs_headless`. The other events (lobby,
  chat, end, connection) are handed back for the shell. On
  `Disconnected`, `Kicked` or `Refused` the peer has dropped its match,
  so the session would draw its start world: the shell must leave the
  match view before the next build (Task 12); `OnlineDriver::into_peer`
  gives the peer back. An online match is never fast-forwarded (`human()`).
  A `MatchStarting` among a poll's events and everything after it are
  handed back unpresented (`present_events`): they are the next match's.
- **Online from the menus** (`shell/online.rs`, `net.rs`; plan
  10 task 12): `HostGame`/`JoinGame` make the `OnlinePeer` (the identity
  from `Store::load_identity`, a fresh key with a warning when the file's
  is no valid key; the selected profile's look, its skin by pack id; a
  `Content` of the shell's roots; the host's setup from the saved match,
  `net::setup_of`, bots "Bot N" in OLX's bot colours, never through
  `MatchPrefs::sanitize`: a lobby may have no bot). The host is in its
  lobby at once (`LobbyJoined`); a client when its first lobby names it.
  A typed address goes through `shell::resolve`: a name's first IPv4
  address (`choose_address`; an IPv4-mapped one counts); an IPv6 literal
  or an IPv6-only name is refused with `IPV6_UNSUPPORTED` (the host binds
  `0.0.0.0`, the client's UDP socket is IPv4).
  `Shell::net_frame(now, keys)` (every frame, before the ticks; while an
  online match plays its peer runs the ticks and the local clock rests)
  polls `--net-sim`'s host, the LAN `Listener` on `LAN_PORT` while the
  Multiplayer screen shows (games of another `FINGERPRINT` hidden), then
  the peer once: through `Session::frame` while a match plays (the
  input latch; `Keys(0)` while the match's menu, the chat or another
  screen has the keys), else `OnlinePeer::poll`. Events: `Lobby` →
  `MenuData::lobby` (`net::lobby_view`: `PHASE_LOBBY` while gathering or
  showing results, `SEAT_SPECTATOR`, the host's `can_start` reason, a
  client's "Waiting for the host"; the peer sends its `Have` itself), the
  weapon screen's menu for the lobby's mod (`net::weapon_menu`, decision
  N19, from the peer's content, made again when it changes: a downloaded
  mod counts, `net::WeaponsKey`), the host's LAN
  announcement (`Announcer`, `Visibility::Lan`; `GameSummary::of_lobby(..).lan()`,
  the level's name from the peer's content); `Chat` ("name: text",
  `LineKind::Chat`), `Notice` and `Left` ("name left") → the lobby's chat
  (`net::CHAT_KEEP` lines) and the HUD log; `MatchStarting` → the last
  match's view ends, `setup::load_online` (from `OnlinePeer::content`,
  the content the match was built from), `Session::try_online` (a
  failure hands the peer back to say goodbye) with the
  events after it, `MatchStarted` (InGame), `Outcome::Started`;
  `MatchEnded` → the results if they did not show yet; `Results`' Back →
  `EndMatch`: the peer back to `Net` (`end`), the Lobby; `Lagging` → the
  warning; `Kicked`/`Refused`/`Disconnected` → the match view goes at
  once (`lost`), then `OnlineFailed` (Multiplayer with the reason);
  `ReplayReady` → `replays::auto_save` (a peer let go, `release`, hands
  its replay being made to `Shell::replays`, polled every frame and
  waited for at quitting, at most `REPLAY_STOP_WAIT` 2 s: leaving right
  after a match keeps its replay); `DesyncBundle` → `<data>/desyncs/
  desync-<unix s>-<n>.rldg` (`net::save_bundle`, rl-online's bounded
  `bundle::save`: 4 MiB a bundle, 64 MiB in all; rl-online's `GameBundle`:
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
  **Hidden window** (decision N21): while `Shell::online_active`,
  `about_to_wait` runs `App::hidden_frame` (the network and the match's
  ticks, no drawing) every `net::HIDDEN_FRAME` (4 ms, `WaitUntil`: not a
  spinning `Poll`). **In-game chat**: the options' `online.chat_key` (T)
  opens rl-ui's field over the playfield (`Shell::open_chat`, from
  `App::key`); while it is open no key reaches the game (`handler.rs`,
  `App::key`) and those held are released. **Network
  overlay** (F3, spec §7.3, `overlay::network`): ping, input delay,
  rollback depth last and p99 over 5 s (`net::NetMeter` on the session's
  depth histogram), the last second's deepest, loss, corrections per
  second, the frontier lag (`current_tick - frontier`); "Connection
  problem" at the playfield's top while `Lagging`. Online frames are
  interpolated by the pace's own tick phase (`Session::online_alpha` →
  `OnlineDriver::alpha` → `OnlinePeer::tick_phase`; the local clock's
  phase is not the netcode's), 1 (the newest tick as it is) while the
  match stalls or catches up, for a spectator, on the frame after a
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
- **Deathmatch** (plan 6 ruling R5): `setup::match_options` (the human is
  worm 0, then the bots at one difficulty, OLX's LX56 settings, the default
  restrictions) feeds `rl_content::build_match`, the same assembly as
  `rl-cli match` (loadouts, the Deathmatch and its respawn loadout, the
  shared area cache, the recorder). The command line's human gets random
  enabled weapons (`rl_content::Player::Human`). The bots think on
  the main thread inside `Session::tick`, in worm order, before the world
  ticks; `MatchDriver` times each tick's thinking (`think_ns`, the F2 panel
  and the exit summary's p50/p99) and counts thinks over budget (none, by
  design). The recording ends `RECORD_AFTER_END` (3 s) after the game over
  or at `MAX_TICKS`; `--record f` saves it at exit and on each rematch
  (`f`, `f-2`, ... : `drive::record_path`).
- **Match HUD** (`rl_ui::hud`, an opaque bar along the bottom, or the
  side layout: `Frame::arrangement`, the options' `hud_layout`;
  `hud::view(session, &Frame)` builds its `HudView` from
  the session and the frame; no weapon rows
  while the selection is open, `Shell::selecting`, the world's being a
  placeholder until Done): names ("You", "Bot N"; a replay's "Worm N",
  over the worms on screen: `worms_on_screen`), the event log
  (`Session::log`, an `rl_ui::hud::Log`) from `Message` events (OLX's
  texts, the last 64 lines; kills show
  no weapon: `rl-sim` does not record the injuring weapon, and OLX drops
  `" with <weapon>"` then), the time-limit clock (red in the last 10 s; it
  stops at the end, as `Deathmatch::time_left_ms` does) and kill limit,
  the minimap (`hud::MinimapTexture`: the colour terrain as an egui
  texture, made again every `MINIMAP_TICKS`), Tab's scoreboard
  (`World::scores`, the mode's order; `hud::ScoreKey` hides it when the
  window loses focus, which never delivers Tab's release), and on
  `GameOver` "Game Over", then after OLX's `GAMEOVER_WAIT` (3 s) the
  results (the menus' screen; the command line's:
  `rl_ui::screens::results::panel` with F5 rematch, Esc quit). `Session::tally` counts kills by bots,
  the player and suicides (Kill events) over the run's match time.
- **Window** (`window.rs`, the user's ruling after the plan 6 playtest):
  not resizable, no maximise button; `--fullscreen` is borderless. Size
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
  `RL_WINDOW_SYSTEM`) picks the window system; auto stays Wayland on WSL
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
  never through the selection, the pause menu or the results), summed damage popups
  (`Damage.dealt`), the HUD (OLX's kills, deaths and suicides, which a
  zero score does not count), and F2's counters: FPS, frame and *busy* ms
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
  `RL_BLESS_TOOL_OUTPUT=1`). Every
  percentile is nearest rank (`stats::percentile`: rank `ceil(n × p)`,
  plan 8 ruling PF8; the exit summary's too).

- **Autoplay** (`autoplay.rs`, spec §21 G7, plan 7 task 8): `--autoplay
  script.ron` drives the real app from a RON script of steps (`at_frame`,
  `ui`, `within`): `Click(label)` (the widget found by label in egui's
  AccessKit tree, enabled for the run: `widgets`, `click_point`; a pointer
  press and release injected into the next two frames' `RawInput`),
  `Key(name)` (egui and `App::key`, as the window delivers a key),
  `Type(text)` (egui's text event to the focused field: click it first),
  `Press`/`Release(action)` (the input path), `WaitFor(screen)`,
  `Wait(frames)`, `Screenshot(name)` (`<shot dir>/<name>.png`, checked not
  blank: `sanity`), `Assert(Screen | Label | WindowSize | Speed | Playing
  | TicksAtLeast | Lobby(members: n))` (the online lobby's members), `Quit`, `WaitForExit` (the menus' Quit). A step that
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
  `fixtures/autoplay/online.ron` its online run against `rl-server` (real
  clock).
- **Soak** (`tests/soak.rs`, `cargo xtask full`): `soak_sandbox_10_minutes_headless`
  plays 60 000 ticks of `Session` with chaos keys, an offscreen render every
  10th tick (main thread only) and a kira mock device for sound, and bounds
  the counters; it needs `RL_GPU_TESTS`, content and an adapter, else skips.

Test: `cargo nextest run -p rl-game` (the pure parts; `setup`, `session`,
`tests/deathmatch.rs` (a Deathmatch to its results, the bots' think
budget, a recording replayed through a replay session), `tests/menu.rs`
(starts with missing or broken content, rapid menu actions) and
`tests/profiles.rs` (the preview's textures; a menu match wears the
profile's look), `tests/online.rs` (a local match whose player is slot 2,
a watcher following any worm, a `Driver::Online` session against a host
on 127.0.0.1: 300 frames on a fake 10 ms clock, the HUD's scores the
host's confirmed ones, the end from a confirmed tick; two `Shell`s, one
hosting on port 0 and one joining, to the lobby, a bot, ready, start,
equal confirmed scores, the results, the replay saved, back to the
lobby and leaving; chat in game reaching the other's HUD log; the
hidden window's `App::hidden_frame` ticking the match; `--net-sim
typical`'s setup playing and starting the next match; leaving right
after the end keeps both replays; the flags not remembered; LAN off:
`Shell::set_lan(false)`; each under 20 s), `tests/downloads.rs` (a
joiner whose content lacks Dirt Level downloads it in the lobby into its
`Store::open` cache and plays the match from it; over the prompt
(`OnlinePeer::test_prompt_limit` through `Shell::peer_mut`: rl-online's
`test-hooks` in the dev-dependencies) it waits, queued, until Download;
a replay of that level,
the level only in the cache (`downloads`, or `downloads-2` while the
shell holds `downloads`), plays back from the Replays screen and is
refused, naming it, without; a command-line replay finds it while
another holder locks the cache, and makes, locks and writes no cache; the options' Clear, refused while a match
is online, empties the cache), `tests/ingame.rs` (the weapon selection, the pause and a
valid replay, the results against the world's scores), `tests/options.rs`
(video through the paced requests,
a window made again keeps the game ticking, volumes live, the saved window with the flags on top) play the imported
levels when `RL_CONTENT_DIR` is set, as does `tests/speed.rs` (fast-forward
never changes the simulation; a played game refuses it); `tests/import.rs` imports
`fixtures/olx-mini` through the shell: the menus refreshed, the imported
pack over the bundled one, a quit mid-import).
