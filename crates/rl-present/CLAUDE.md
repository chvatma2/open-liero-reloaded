# rl-present

One viewport's presentation of a match (spec §8.2, §19 E1-E8): `rl-scene`'s
`Scene`, `rl-fx`'s entities, the colour terrain and every atlas image,
wired in the frame loop's order **once**, so each mode that draws a world
(the sandbox, matches, replays, `rl-cli render`) uses the same wiring.

- **Layer 5.** Depends on `rl-scene`, `rl-fx`, `rl-render` (same layer,
  listed in `xtask/layers.ron`), `rl-content`, `rl-sim`, `rl-terrain`,
  `rl-data`, `rl-netcode` (its tick records, for online play). It holds
  no GPU or OS handle: only `upload` takes the `rl_render::Renderer`, so
  everything else is tested headless.
- **Use** (`lib.rs`):
  - `Presentation::new(&PresentSetup, &world)` registers the core and mod
    images and starts the scene from the world; then `add_worm(skin,
    colour, local, human)` per worm in `World::worms` order (skin and gibs
    in one call, so the scene's and the entities' lists stay parallel).
    `default_colour(i)` is OLX's red, blue, green, yellow.
  - `set_fx_capacity(n)` before the first tick: a bench-only entity pool
    (plan 8).
  - `on_tick(&world, &events)` after every `World::tick`: `Fx::step_tick`
    (the only place the tick's terrain edits reach the colour map; never
    also call `rl_content::apply_edits`), then `Scene::after_tick`.
  - `build(&world, alpha, &mut list)` per drawn frame: scene, then entities.
  - `upload(&mut renderer)` right before the frame is drawn: the atlas when
    a worm was added (also mid-match), the whole colour map when it is new
    (`new`, `restart`, `invalidate_gpu` for a new renderer), else the dirty
    rectangles of the ticks since the last frame (the map merges them).
  - `step_fx(&FxTick)`: `on_tick`'s entities and colour part for a tick
    described by hand (online play).
  - `restart(&world, &level, &theme)` for the same worms on a new world;
    `state_loaded(&world)` for a loaded state in online play (scene
    restarts, entities go; the colour map is repainted by the load's
    restored area).
  - `follow(worm)` / `followed()`: the worm the camera follows (worm 0
    when new; the game's local slot, or a spectator's choice).
  - `terrain_drift(&world)`: the colour map's material is not the world's.
    It compares the whole grid; `rl-game`'s session checks it every
    `DRIFT_CHECK_TICKS` in debug builds.
- **Online play** (`net.rs`, spec §3.4, decision N20): the newest
  predicted tick is drawn and corrections re-simulate shown ticks. Per
  advance: inside it, for each simulated tick (rl-online's `on_tick`
  callback, the world as of that tick) `Scene::after_tick` for a new tick,
  `Scene::after_tick_resimulated` for a re-simulated one, and
  `TickRecorder::simulated`; after it `NetPresenter::apply(present, world,
  restored, ticked, &mut sounds)`: the restored areas are rebuilt from the
  final terrain first, then each tick: a tick never shown runs as in local
  play (all spawns, edits, entity passes, sounds); a shown one runs no entity
  pass, all its terrain edits, and only the cosmetics, sounds and camera
  shakes whose `EventKey` is new. Debug builds check `terrain_drift`
  after it: a drift is counted (`drifts()`) and logged once, not a panic
  (tests assert `drifts() == 0`). `confirmed(tick)` prunes keys below `Session::confirmed`;
  `reset(tick)` (with `Presentation::state_loaded`) on
  `Event::StateLoaded { tick }`, before or after that poll's `apply`: it
  keeps the keys after the state, so ticks run again after it play no sound
  twice. `spawned()` counts the cosmetics handed to the entities. A sound's
  key has no source: a correction swapping which worm fires leaves the
  sound where it first played. A death sound is keyed by the worm whose
  gibs follow it, not by which of the three it drew: a correction that
  re-rolls it plays no second one, one that kills another worm instead
  plays that death's.
  `event_keys` keys an event by tick, sub-step, a hash of its kind and
  integer identity fields (never positions or amounts, which a correction
  moves) and an ordinal among equal ones. An erased event is not undone.
- **The simulation never sees presentation:** it only reads the world.

Test: `cargo nextest run -p rl-present` (`tests/present.rs` plays oracle
scenarios: the bundle draws what the hand wiring drew, the atlas bookkeeping,
drift; `tests/net.rs` rolls a Dirt Level match back by hand: sounds and
spawns once per key, no drift after any advance, the confirmed records the
final timeline's, a loaded state). It needs `RL_CONTENT_DIR` and skips
without it.
