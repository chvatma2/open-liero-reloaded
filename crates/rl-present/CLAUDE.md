# rl-present

One viewport's presentation of a match (spec §8.2, §19 E1-E8): `rl-scene`'s
`Scene`, `rl-fx`'s entities, the colour terrain and every atlas image,
wired in the frame loop's order **once**, so each mode that draws a world
(the sandbox, matches, replays, `rl-cli render`) uses the same wiring.

- **Layer 5.** Depends on `rl-scene`, `rl-fx`, `rl-render` (same layer,
  listed in `xtask/layers.ron`), `rl-content`, `rl-sim`, `rl-terrain`,
  `rl-data`. It holds no GPU or OS handle: only `upload` takes the
  `rl_render::Renderer`, so everything else is tested headless.
- **Use** (`lib.rs`):
  - `Presentation::new(&PresentSetup, &world)` registers the core and mod
    images and starts the scene from the world; then `add_worm(skin,
    colour, local, human)` per worm in `World::worms` order (skin and gibs
    in one call, so the scene's and the entities' lists stay parallel).
    `default_colour(i)` is OLX's red, blue, green, yellow.
  - `on_tick(&world, &events)` after every `World::tick`: `Fx::step_tick`
    (the only place the tick's terrain edits reach the colour map; never
    also call `rl_content::apply_edits`), then `Scene::after_tick`.
  - `build(&world, alpha, &mut list)` per drawn frame: scene, then entities.
  - `upload(&mut renderer)` right before the frame is drawn: the atlas when
    a worm was added (also mid-match), the whole colour map when it is new
    (`new`, `restart`, `invalidate_gpu` for a new renderer), else the dirty
    rectangles of the ticks since the last frame (the map merges them).
  - `restart(&world, &level, &theme)` for the same worms on a new world.
  - `terrain_drift(&world)`: the colour map's material is not the world's.
    It compares the whole grid; `rl-game`'s session checks it every
    `DRIFT_CHECK_TICKS` in debug builds.
- **The simulation never sees presentation:** it only reads the world.

Test: `cargo nextest run -p rl-present` (`tests/present.rs` plays oracle
scenarios: the bundle draws what the hand wiring drew, the atlas bookkeeping,
drift). It needs `RL_CONTENT_DIR` and skips without it.
