# rl-game

The game binary (spec §8.6, §19 E10-E12): a winit window, the 100 Hz
fixed-tick loop, interpolated wgpu rendering with cosmetic entities, sound,
input and an egui overlay. For now the M5 sandbox (E11): the player and N
idle targets on a level with a mod's first five weapons.

```
cargo run --release -p rl-game -- --level "CastleStrike" --mod "MW 1.0" --targets 3
```

- **Layer 6.** Uses `rl-content`, `rl-present`, `rl-scene`, `rl-fx`, `rl-render`,
  `rl-audio`, `rl-input`, `rl-sim`, `rl-terrain`, `rl-pack`, plus winit,
  wgpu, gilrs and egui / egui-wgpu / egui-winit 0.36.2 (exact pins). Logs
  are `eprintln!("warning: ...")`, each once.
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
  headless; its presentation is one `rl_present::Presentation`. `restart`
  (F5) builds the world from the assets loaded once (`setup::Assets`,
  `rl_content::new_world_from`); the worms are the same, so the atlas
  stays and only the terrain is uploaded again. Debug builds assert every
  `DRIFT_CHECK_TICKS` (100) that the colour terrain's material is the
  world's (`session_catches_terrain_drift`).
- **Window** (`window.rs`, the user's ruling after the plan 6 playtest):
  not resizable, no maximise button; `--fullscreen` is borderless. Size
  and mode changes come only from the game (`size`, `fullscreen` script
  commands, the M7 options menu) through `window::Requests`: the latest
  wins, applied at the start of a frame before `acquire`, at most once
  every `MIN_CHANGE_INTERVAL` (250 ms, creating the window counts as a
  change: `Requests::opened`); a fixed window's min and max move
  with it (`window::apply`). Why: WSLg's Weston 9 segfaults when winit's
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
  `size <w> <h>`, `fullscreen on|off` (window changes wait out
  `MIN_CHANGE_INTERVAL`, so a `shot` within 250 ms may show the old
  size); `--shot <tick>`, `--autoquit-after <s>`, `--stats` (counters
  each second) and the exit summary let an agent run the game in a real
  window and read the PNGs (plan 5 ruling R3).

- **Soak** (`tests/soak.rs`, `cargo xtask full`): `soak_sandbox_10_minutes_headless`
  plays 60 000 ticks of `Session` with chaos keys, an offscreen render every
  10th tick (main thread only) and a kira mock device for sound, and bounds
  the counters; it needs `RL_GPU_TESTS`, content and an adapter, else skips.

Test: `cargo nextest run -p rl-game` (the pure parts; `setup` and
`session` play the imported levels when `RL_CONTENT_DIR` is set).
