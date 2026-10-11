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
cargo run --release -p rl-game -- --replay f.rlrp
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
  (`rl_ui::Store::platform`; warnings logged) and the first match seed
  from the clock (each seed is in its replay). With such a flag it plays
  that game at once as before (scripted runs and tests: Esc quits, F5
  restarts, `--record`). `--shot`, `--autoquit-after` and `--stats` work
  in both. `Shell` holds the `UiState`, the menus' data, the store and the
  game playing (`Playing { assets, session }`); `app.rs` draws
  `rl_ui::screens::show` over the overlay, feeds the actions to
  `Shell::act` and does the `Outcome`s (`app/menus.rs`: quit, bindings,
  video through the paced window requests, a started game's upload). A
  start is loaded by `Shell::poll` after the frame that showed "Loading…";
  a failure (a pack missing or broken: `setup::load_match`, `load`,
  `load_replay`) returns to the menu with the reason (Review Focus 3).
  Esc and F5 are the menus' there: Esc pauses (ticks stop; the pause menu
  resumes or leaves), F5 restarts in place. Results show → `MatchOver`
  (`Session::results_shown`). Every Deathmatch's replay is saved in the
  data directory's `replays/` when it ends, restarts or the game quits
  (`rl_ui::replays::auto_save`, none of no tick); the Replays screen plays
  them. Settings are written at most once a second (`Shell::flush`) and
  at exit. The human's profile names and colours worm 0 (`PlayerLook`).
- **Single thread** (spec §8.6): the device is created and used only on
  the main (event-loop) thread; under WSL's d3d12 GL driver a thread that
  used the device crashes when it exits. kira runs its own audio thread.
- **WSL** (`wsl.rs`): on a WSL kernel, unless the user set `WGPU_BACKEND`
  or `GALLIUM_DRIVER`, the game starts itself again with
  `GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl` (Mesa reads the variable when the
  driver loads, and `set_var` is unsafe). WSLg presents through GL at the
  compositor's 60 Hz with FIFO only.
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
  shared area cache, the recorder). Until M7's weapon menu the human gets
  random enabled weapons (`rl_content::Player::Human`). The bots think on
  the main thread inside `Session::tick`, in worm order, before the world
  ticks; `MatchDriver` times each tick's thinking (`think_ns`, the F2 panel
  and the exit summary's p50/p99) and counts thinks over budget (none, by
  design). The recording ends `RECORD_AFTER_END` (3 s) after the game over
  or at `MAX_TICKS`; `--record f` saves it at exit and on each rematch
  (`f`, `f-2`, ... : `drive::record_path`).
- **Match HUD** (`hud.rs` pure, `panels.rs` paints): names ("You", "Bot
  N"; a replay's "Worm N"), the kill feed from `Message` events (OLX's
  texts, the last 6 lines for 3 s; kills show no weapon: `rl-sim` does not
  record the injuring weapon, and OLX drops `" with <weapon>"` then), the
  time-limit clock (red in the last 10 s; it stops at the end, as
  `Deathmatch::time_left_ms` does), Tab's scoreboard
  (`World::scores`, the mode's order; `hud::ScoreKey` hides it when the
  window loses focus, which never delivers Tab's release), and on `GameOver` "Game Over",
  then after OLX's `GAMEOVER_WAIT` (3 s) the results (winner or "noone",
  reason, lives, score, kills, deaths, suicides, damage; F5 rematch, Esc
  quit). `Session::tally` counts kills by bots,
  the player and suicides (Kill events) over the run's match time.
- **Window** (`window.rs`, the user's ruling after the plan 6 playtest):
  not resizable, no maximise button; `--fullscreen` is borderless. Size
  and mode changes come only from the game (`size`, `fullscreen` script
  commands, the M7 options menu) through `window::Requests`: the latest
  wins, applied at the start of a frame before `acquire`, at most once
  every `MIN_CHANGE_INTERVAL` (250 ms, creating the window counts as a
  change: `Requests::opened`); a fixed window's min and max move
  with it (`window::apply`). While the window is hidden (minimised or
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
- **Overlay** (`overlay.rs`): names over worms, summed damage popups
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

- **Soak** (`tests/soak.rs`, `cargo xtask full`): `soak_sandbox_10_minutes_headless`
  plays 60 000 ticks of `Session` with chaos keys, an offscreen render every
  10th tick (main thread only) and a kira mock device for sound, and bounds
  the counters; it needs `RL_GPU_TESTS`, content and an adapter, else skips.

Test: `cargo nextest run -p rl-game` (the pure parts; `setup`, `session`,
`tests/deathmatch.rs` (a Deathmatch to its results, the bots' think
budget, a recording replayed through a replay session) and `tests/menu.rs`
(starts with missing or broken content, rapid menu actions) play the
imported levels when `RL_CONTENT_DIR` is set).
