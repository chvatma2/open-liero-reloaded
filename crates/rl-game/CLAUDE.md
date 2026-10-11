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
```

- **Layer 6.** Uses `rl-ui`, `rl-content`, `rl-present`, `rl-scene`, `rl-fx`, `rl-render`,
  `rl-audio`, `rl-input`, `rl-sim`, `rl-modes`, `rl-bots`, `rl-replay`,
  `rl-terrain`, `rl-pack`, plus winit,
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
  at exit. The human's profile names and colours worm 0 (`PlayerLook`)
  and its skin is worm 0's (`Assets::skin`); every other worm, and every
  worm of a replay, wears OLX's default skin (`Assets::bot_skin`, as OLX's
  bot profiles) in OLX's bot colours away from the player's
  (`rl_ui::bot_colours`; a replay's worms in the default colours).
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
  What it plays is `setup::Game` (`Sandbox`, `Deathmatch`, `Replay`), loaded
  once into `setup::Assets`; who presses the keys is `session::Driver`
  (`drive.rs`): the player alone, the player and `MatchDriver`'s bots, or
  `ReplayDriver` (a `rl_replay::Cursor` checking every checksum; a
  divergence is shown and logged once, the replay stops at its end).
  `restart` (F5) builds the world from the assets loaded once: the
  sandbox again, a Deathmatch's rematch (seed + rematch number; the
  previous match's replay is handed back to save), a replay from the
  start; the worms are the same, so the atlas stays and only the terrain is
  uploaded again. It clears the last tick's events (the rematch's bots
  think first, on the new world). Debug builds assert every `DRIFT_CHECK_TICKS` (100) that
  the colour terrain's material is the world's (`session_catches_terrain_drift`).
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
  window and read the PNGs (plan 5 ruling R3).

- **Autoplay** (`autoplay.rs`, spec §21 G7, plan 7 task 8): `--autoplay
  script.ron` drives the real app from a RON script of steps (`at_frame`,
  `ui`, `within`): `Click(label)` (the widget found by label in egui's
  AccessKit tree, enabled for the run: `widgets`, `click_point`; a pointer
  press and release injected into the next two frames' `RawInput`),
  `Key(name)` (egui and `App::key`, as the window delivers a key),
  `Press`/`Release(action)` (the input path), `WaitFor(screen)`,
  `Wait(frames)`, `Screenshot(name)` (`<shot dir>/<name>.png`, checked not
  blank: `sanity`), `Assert(Screen | Label | WindowSize | Speed | Playing
  | TicksAtLeast)`, `Quit`, `WaitForExit` (the menus' Quit). A step that
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
  (with `settings.ron`) is `cargo xtask e2e`'s script, on Xvfb in `full`.
- **Soak** (`tests/soak.rs`, `cargo xtask full`): `soak_sandbox_10_minutes_headless`
  plays 60 000 ticks of `Session` with chaos keys, an offscreen render every
  10th tick (main thread only) and a kira mock device for sound, and bounds
  the counters; it needs `RL_GPU_TESTS`, content and an adapter, else skips.

Test: `cargo nextest run -p rl-game` (the pure parts; `setup`, `session`,
`tests/deathmatch.rs` (a Deathmatch to its results, the bots' think
budget, a recording replayed through a replay session), `tests/menu.rs`
(starts with missing or broken content, rapid menu actions) and
`tests/profiles.rs` (the preview's textures; a menu match wears the
profile's look), `tests/ingame.rs` (the weapon selection, the pause and a
valid replay, the results against the world's scores), `tests/options.rs`
(video through the paced requests,
a window made again keeps the game ticking, volumes live, the saved window with the flags on top) play the imported
levels when `RL_CONTENT_DIR` is set, as does `tests/speed.rs` (fast-forward
never changes the simulation; a played game refuses it); `tests/import.rs` imports
`fixtures/olx-mini` through the shell: the menus refreshed, the imported
pack over the bundled one, a quit mid-import).
