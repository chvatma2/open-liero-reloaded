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
    plan 15b, which since #53 block no light: a worm takes the light where
    it stands, floored by the worm floor); an unlit match sets none and
    draws as always.
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
    match clock (the world's time plus `alpha` of a 10 ms tick), its
    `step` the presentation's count of world states shown (one more each
    `step_fx`, so each tick presented, `restart` and `state_loaded`): every
    frame of a tick has the same, so the renderer makes the glow light
    once a tick (`every_frame_of_a_tick_has_its_step`), and adds the
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
    gets a fresh heat texture. Issue #41: and each distant layer's
    material value (`LoadedLevel::distant_surface`,
    `Renderer::set_distant_surface`), so `set_distant` heats the layers.
  - **Heat** (issue #40, spec
    `docs/superpowers/specs/2026-10-22-materials-and-heat-design.md`): in
    a level whose materials heat (its look has one that `heats()`, and
    surface maps, or issue #41 a distant layer's material heats; `heats`,
    again at `restart`) each explosion of the
    scene's per-tick records (`Scene::explosions`: a projectile's or a
    bonus blown up; an explosive trail or a beam never makes one) queues
    the engine's heat (`olr_look::engine::explosion_heat` of its damage)
    once, keyed by `BlastKey` (its tick's ms, its place in the tick)
    through the same `Once` memory as the end flashes (`look.rs`): a
    re-simulated tick deposits nothing twice. Never from the live
    `olr-fx` entities, which a frame can miss. The queue (`HEAT_QUEUE`
    2048, reserved at the first) holds each deposit with its tick's time
    until `upload`, which after the terrain hands the renderer each one
    after `advance_heat` up to its tick (issue #41: `deposit_heat_seen`
    with every view's camera as last built, `HeatView::centred`, the frame
    that first shows the explosion: the renderer places the distant
    layers' slices by the nearest), then the time up to the frame
    built (`heat_now`, the world's time, ticks never drawn included;
    `heat_clock` the time handed so far), running `Renderer::run_heat`
    whenever `MAX_HEAT_DEPOSITS` wait or more than `MAX_HEAT_STEPS` are
    due (at most 64 runs): the heat is the same however often frames are
    drawn, except where the terrain changed after heat landed (the
    renderer reads the terrain as uploaded; `olr-cli`'s
    `heat_is_the_same_drawn_every_tick_or_once`). Without heat in the
    renderer the queue is dropped. `restart` starts afresh with the
    terrain; `state_loaded` and `reset_heat(&world)` (a replay seek;
    replays have none yet) clear it, the renderer's with the next upload.
    `blasts_started()` (the last frame's keys) and `heat_waiting()` (the
    queue) for tests and tools.
  - **Mods' heat** (issue #42): an explosion whose projectile
    (`TickExplosion::proj`) has an explosion heat (`ModLook::explosion_heat`:
    its look's, else the mod's; none for -1, a bonus blown up) queues that
    (`ExplosionHeatLook::heat` of its damage: the engine's at its percent,
    over its radius) instead of the engine's. A projectile whose look has
    `heat` heats each tick it lives: `new` (again at `restart`, only when
    the level heats) gives the scene `ShadeRules::projectile_heat`, and
    each of the scene's per-tick `hot_projectiles()` (where it was after
    the tick, at most `olr_scene::MAX_HOT_PER_TICK` 64 a tick) queues
    `ProjectileHeat::tick()` there with its tick's time, once (a `WarmKey`:
    tick ms, slot, serial, through its own `Once`): the spec's `amount ×
    ticks elapsed at its drawn position`, a tick at a time, so it never
    depends on the frame rate. Explosions and heating projectiles are
    queued merged in tick order, the explosions of a tick first, under the
    same `HEAT_QUEUE`.
  - **Beams' heat** (issue #69): each of the scene's `beam_hits()` (a
    beam's part stopped on rock or dirt in a sub-step) queues the weapon's
    beam heat (`ModLook::beam_heat`: its own, else `engine::BEAM_HEAT`)
    split over the sub-step's `parts` (`BeamHeat::part`) there, once (a
    `BeamKey`: tick ms, index, through its own `Once`), after the tick's
    explosions and heating projectiles. A beam on a worm or ending in the
    open heats nothing.
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
  projectile: bonuses, explosive trails) keep the engine's; a beam's
  cast none (issue #69).
  `flashes_started()` lists the end flashes the last frame started (tests
  and tools). Issue #12: last, the skins' lights, one steady disc per
  drawn worm whose skin has a light and whose frame glows
  (`SceneAssets::worm_light`, `engine::skin_light`, identity the worm id),
  at the worm as drawn, in drawn-worm order; only in a level with a look
  (a mod's look alone casts none) and while `engine::SKIN_LIGHT` is on
  (`Presentation::set_skin_light` overrides it for tests and tools).
  Issue #69, after the skins' (past the cap they drop, never another
  light): each of the scene's `drawn_beams()` lights discs every
  `engine::BEAM_DISC_SPACING` from its start to its end (both included,
  at most `MAX_BEAM_DISCS`, spaced wider to fit; `beam_discs`), radius
  `BEAM_DISC_RADIUS`, the weapon's own `beam.light` (by reference: no
  allocation) else `engine::beam_light` of its colour, and one at its end
  when it `stopped`, radius `BEAM_TIP_RADIUS`, its `tip_light` else
  `engine::beam_tip_light`; identity the worm, clock the match's. A
  beam's carve explosions (`olr_sim::BEAM_EXPLOSION`) cast no light (the
  tip light replaces them); a beam weapon's shot starts its muzzle light.
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
`a_loaded_state_restores_the_glow`, issue #40's
`explosions_heat_once_through_rollbacks`; `tests/look.rs`: no look for bundled
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
the same whatever the look); issue #40's
`explosions_heat_once_and_trails_never` (napalm: one deposit an
explosion, at its place and the engine's size, none for the trail's many
puffs), `explosion_heat_does_not_depend_on_the_frame_rate` (a frame
every 1, 4 or 7 ticks queues the same deposits) and
`only_a_level_whose_materials_heat_queues_heat` (issue #41: a distant layer's heating material too);
issue #42's `a_projectiles_heat_warms_its_path_each_tick` (the flamer's
flames: one deposit each a tick at its place, a path of many),
`projectile_heat_does_not_depend_on_the_frame_rate_and_is_capped` (frames
every 1, 4 or 7 ticks queue the same; every projectile at the caps heats
64 a tick at 100 degrees, 16 px) and
`explosion_heat_is_the_projectiles_else_the_mods` (`play_looked` gives the
mod a look); `tests/net.rs`'s event keys leave an
explosion's projectile out; `tests/no_alloc.rs`, its own binary, a warm
lit build with every dynamic light, own and mod-wide explosion lights
included, allocates nothing). They need
`OLR_CONTENT_DIR` and skip without it.
