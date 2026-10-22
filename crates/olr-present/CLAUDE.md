# olr-present

The presentation of a match (spec §8.2, §19 E1-E8): `olr-scene`'s
`Scene` (a camera per view), `olr-fx`'s entities, the colour terrain and every atlas image,
wired in the frame loop's order **once**, so each mode that draws a world
(the sandbox, matches, replays, `olr-cli render`) uses the same wiring.

- **Layer 5.** Depends on `olr-scene`, `olr-fx`, `olr-render` (same layer,
  listed in `xtask/layers.ron`), `olr-content`, `olr-sim`, `olr-terrain`,
  `olr-data`, `olr-look` (the look frame), `olr-netcode` (its tick records,
  for online play). It holds
  no GPU or OS handle: only `upload` takes the `olr_render::Renderer`, so
  everything else is tested headless.
- **Use** (`lib.rs`):
  - `Presentation::new(&PresentSetup, &world)` registers the core and mod
    images and starts the scene from the world; then `add_worm(skin,
    colour, local, human)` per worm in `World::worms` order (skin and gibs
    in one call, so the scene's and the entities' lists stay parallel).
    `default_colour(i)` is OLX's red, blue, green, yellow.
    The entities' pool is the match's (`olr_fx::Fx::for_match`): play's
    16384 with its death reserve, OLX's 1024 with
    `SimSettings::olx_quirks` (scenarios, render goldens).
  - **Lit matches** (plan 15a Tasks 13 and 15, R7, R13): `has_look()` is the
    level's `LoadedLevel::has_look` or a mod look that is not empty. `new`
    then gives the scene its `olr_scene::ShadeRules` (`floor` =
    `olr_look::engine::FLOOR`, 50 %, for projectiles and bonuses; each
    projectile's look glow; worms take `Shade::WORM` and are occluders,
    plan 15b); an unlit match sets none and draws as always.
  - `set_fx_capacity(n)` before the first tick: a bench-only entity pool
    (plan 8); the blood amount stays.
  - `set_blood(percent)` / `blood()` (issue #6): the player's blood amount
    (Options → Video, 0-500 %, 100 when new; `olr_fx::Fx::set_blood`),
    applied from the next tick, kept through restarts and loaded states.
    Presentation only: the world, replays and the netcode never carry it,
    and online it scales the cosmetics `NetPresenter` hands the entities.
  - `on_tick(&world, &events)` after every `World::tick`: `Fx::step_tick`
    (the only place the tick's terrain edits reach the colour map; never
    also call `olr_content::apply_edits`), then `Scene::after_tick`.
  - `build(&world, alpha, &mut list)` per drawn frame: scene, then entities
    (`Fx::draw` with the match's lit flag), then `build_look(&world,
    alpha)`: the frame's `olr_look::LookFrame`, which `look()` returns
    (`None` unlit) for the renderer. It begins with the level's look at the
    match clock (the world's time plus `alpha` of a 10 ms tick) and adds the
    dynamic lights (`look.rs`) as glowing discs, never culled to the view
    (plan 15c: bounced light may reach it from past a disc's reach). It allocates
    nothing once warm (`tests/no_alloc.rs`).
  - `upload(&mut renderer)` right before the frame is drawn: the atlas when
    a worm was added (also mid-match), the whole colour map when it is new
    (`new`, `restart`, `invalidate_gpu` for a new renderer), else the dirty
    rectangles of the ticks since the last frame (the map merges them).
    It first turns the renderer's lighting on or off to match `has_look()`
    (`Renderer::lighting`, `set_lighting`), uploading the terrain whole on a
    change (the look textures go with the map).
  - Issue #11: the level's distant layers (`LoadedLevel::distant`, again
    at `restart`) go to `Renderer::set_distant` with every whole terrain
    upload; the colour map is see-through when there are any
    (`LoadedLevel::color_map`).
  - Issue #39: every whole terrain upload first hands the renderer the
    level's surface materials and maps (`LoadedLevel::surface`, again at
    `restart`; `Renderer::set_surface`), so a level whose materials heat
    gets a fresh heat texture. Nothing deposits heat or advances it yet
    (issue 4/10).
  - `step_fx(&FxTick)`: `on_tick`'s entities and colour part for a tick
    described by hand (online play).
  - `restart(&world, &level, &theme)` for the same worms on a new world;
    `state_loaded(&world)` for a loaded state in online play (scene
    restarts, entities go; the colour map is repainted by the load's
    restored area). Both forget the end flashes and muzzle lights.
  - `follow(worm)` / `followed()`: the worm the first view's camera
    follows (worm 0 when new; the game's local slot, or a spectator's
    choice).
  - **Views** (issue #27, split screen): `set_views(&[worm id])` makes a
    view per worm (1 to `olr_scene::MAX_VIEWS`; a view kept keeps its
    camera, a new one is seeded from `seed` by its index), `views()`,
    `follow_view(v, worm)`, `followed_view(v)`; `set_viewport` gives
    every view a new camera of the shape. `build_views(&world, alpha,
    &mut [DrawList])` draws view `v` into list `v`
    (`Scene::build_views`), the same entities into each, then the
    frame's one look (`build` is it for one list). One `Fx`, one colour
    map, one look and one set of dynamic lights serve every view: view
    `v`'s list is a one-view presentation's with that camera
    (`tests/views.rs`). `listeners()` is each view's sound listener
    (`olr_audio::Audio::play_heard` hears each sound from the nearest).
  - `terrain_drift(&world)`: the colour map's material is not the world's.
    It compares the whole grid; `olr-game`'s session checks it every
    `DRIFT_CHECK_TICKS` in debug builds.
- **Dynamic lights** (`look.rs`, plan 15a Task 15 and the batch C and D
  reviews; plan 15c: `LookFrame::emitters`, glowing discs of
  `engine::disc_radius` of the reach, an explosion's `explosion_radius` of
  its size; the engine's first so the cap never drops them: muzzle
  lights, explosion lights, then mods' explosion lights (issue #15),
  projectile lights (the
  scene's `drawn_projectiles`, slot order, the clock from the spawn time,
  identity the serial) and end flashes (start order); muzzle lights in
  start order, explosion lights in the entities' slot order. End flashes and
  muzzle lights start from the scene's per-tick records
  (`Scene::ended_projectiles`, `Scene::shots`, taken with
  `clear_look_events` each frame), so they never depend on the frame rate,
  and run their keys from the tick they started. An end flash is the
  projectile look's `end_flash`, keyed by `FlashKey` (slot, serial,
  `SimMod` index, spawn time ms), started once: a set for membership
  (never iterated) and a time-ordered deque remember keys 2 s (`MEMORY_MS`,
  past the rollback window) and at most 8192, so a re-simulated tick or a
  mispredicted end a correction erased never flashes twice (the flash stays
  where it first showed). At most `olr_look::MAX_EMITTERS` flashes show;
  a flash or muzzle light ends with its animation or after 2 s (`LIFE_MS`), and a
  clock that went back forgets what came after. A muzzle light is the
  weapon look's `muzzle_light`, else `olr_look::engine::muzzle_light()`:
  one a worm (a new shot restarts it, batch D review, Important 3),
  following the worm as drawn, none for a dead worm (its disc's cells
  never count as its worm, plan 15c). An explosion's light
  is `engine::explosion_light(size)`, made once per size, its clock the
  explosion's age over its life (size × 25 ms) scaled to
  `engine::EXPLOSION_MS` (batch C review, Minor 2), identity its slot.
  Issue #15: an explosion entity whose projectile (`olr_fx::Kind::
  Explosion { proj }`) has an explosion light (`ModLook::explosion`: its
  look's, else the mod's) shows that instead, in the mods' part (first,
  before the projectile lights, slot order): its disc `radius()`, its reach
  capped (`reach()`), its clock fitted as the engine's when `fitted`, else
  the explosion's age in ms; it ends with the explosion. The rest (no
  projectile: bonuses, beams, explosive trails) keep the engine's.
  `flashes_started()` lists the end flashes the last frame started (tests
  and tools). Issue #12: last, the skins' lights, one steady disc per
  drawn worm whose skin has a light and whose frame glows
  (`SceneAssets::worm_light`, `engine::skin_light`, identity the worm id),
  at the worm as drawn, in drawn-worm order; only in a level with a look
  (a mod's look alone casts none) and while `engine::SKIN_LIGHT` is on
  (`Presentation::set_skin_light` overrides it for tests and tools).
- **Online play** (`net.rs`, spec §3.4, decision N20): the newest
  predicted tick is drawn and corrections re-simulate shown ticks. Per
  advance: inside it, for each simulated tick (olr-online's `on_tick`
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
  sound where it first played. A death's events (`Event::death`: its
  sound, gibs and blood) show once per worm and life (`death_parts`; the
  life counts the worm's spawns as last simulated), whatever tick within
  `olr_netcode::MAX_PREDICTION` a correction moves the death to and
  whichever of the three sounds it draws; a correction that kills another
  worm instead shows that death. A death seen farther from the last one of
  that life is a new death (a correction erased the first), and `reset`
  forgets the deaths up to a loaded state, so a real death is never
  hidden by an erased one.
  `event_keys` keys an event by tick, sub-step, a hash of its kind and
  integer identity fields (never positions or amounts, which a correction
  moves) and an ordinal among equal ones. An erased event is not undone.
- **The simulation never sees presentation:** it only reads the world.

Test: `cargo nextest run -p olr-present` (`tests/views.rs`, issue #27:
`two_views_draw_what_single_views_draw_over_shared_entities`, unlit and
lit; `tests/present.rs` plays oracle
scenarios: the bundle draws what the hand wiring drew, the atlas bookkeeping,
drift, `the_blood_amount_reaches_the_entities`; `tests/net.rs` rolls a Dirt Level match back by hand: sounds and
spawns once per key, no drift after any advance, the confirmed records the
final timeline's, a loaded state,
`end_flashes_once_per_projectile_through_rollbacks`,
`a_loaded_state_restores_the_glow`; `tests/look.rs`: no look for bundled
content, the level part, projectile and end-flash lights, engine lights and
floors only in lit matches, `engine_lights_are_emitters`, `living_worms_are_occluders`, `emitters_are_deterministic`, frame-rate independence, muzzle
lights' curve and no stacking; issue #12's
`a_skin_light_follows_its_worm_in_its_colour`,
`skin_lights_need_a_lit_level_and_the_switch` (glow overlays lit only),
`skin_light_caps_hold_with_many_worms` (32 worms asking past the caps);
issue #19's `a_projectiles_glowing_pixels_draw_bright_in_a_lit_match`
(a glow mask's overlay over the missile at full self-light, lit only);
issue #15's `a_projectile_explosion_light_replaces_the_engines` (a
hand-made mod's bounce, timer and hit explosions in their projectile's
light or the mod's, the engine's without either, the world's checksums
the same whatever the look); `tests/net.rs`'s event keys leave an
explosion's projectile out; `tests/no_alloc.rs`, its own binary, a warm
lit build with every dynamic light, own and mod-wide explosion lights
included, allocates nothing). They need
`OLR_CONTENT_DIR` and skip without it.
