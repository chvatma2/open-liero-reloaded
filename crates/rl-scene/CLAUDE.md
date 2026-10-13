# rl-scene

Turns a `World` into a `DrawList` (spec §8.2, §19 E1-E4): OLX's camera,
interpolation between the last two ticks, and OLX's sprite math for worms,
ropes, laser sights, beams, projectiles and bonuses. The porting reference
is `docs/porting/olx-notes/presentation.md` §1, §3-§6 and §11.

- **Layer 5, pure CPU.** Depends on `rl-sim`, `rl-content`, `rl-data`,
  `rl-math`. No GPU or OS handles; it only reads the world, and its
  randomness (shake, laser dots) comes from its own `GlibcRand`s.
- **Use** (`lib.rs`): a `Scene` per viewport (`rl-present` bundles it
  with the entities). `start(&world)` for a new or restarted world, then
  `after_tick(&world, &events)` after every tick:
  the old state becomes the previous `Snapshot`, explosions shake the
  camera, and the tick's `Marks` (muzzle flashes, green crosshairs, laser
  sights, beams) replace the last. Online play calls
  `after_tick_resimulated` for a tick a rollback simulates again: the
  same without the camera shake (`Camera::shake_for(&[Event], now)` shakes
  for an explosion first seen in it). `build(&world, alpha, &assets, &mut list)`
  per drawn frame: `Camera::process` (hard lock, shake, clamp), then the
  layers. The game's view is `Viewport::GAME`, 427 × 192 world pixels for
  every window (spec §19 E4 as amended after the v0.1 playtest);
  `Viewport::from_aspect` (240 high) stays for tools and the goldens. The caller appends `rl-fx`'s entities to `Layer::Entities`.
- **`DrawList`** (`draw_list.rs`): the view rectangle and five layers in
  OLX's order (`Layer::ALL`): entities, beams (laser sights, then worm
  beams, as `DrawEntities` draws them), projectiles, bonuses, worms (rope,
  hook, health bar, crosshair, skin, muzzle per worm). Items are
  `Sprite { atlas, src, pos, size, flip, tint }` in world pixels (OLX's
  sprites are 2× art, so positions fall on half pixels) and
  `Line { a, b, colour }`, a 1-pixel `perform_line`. A solid block is the
  `white` image tinted. `describe` is the compact text the snapshot tests
  commit. Layers keep `RESERVED` items of room (the entities `ENTITIES_RESERVED`,
  rl-fx's play pool), so frames do not allocate
  (`tests/no_alloc.rs`).
- **`SceneAssets`** (`assets.rs`): core sprites and mod projectile images
  (OLX's magenta keyed to transparent at load), and per worm a colorized
  skin (`add_worm`: `rl_content::LoadedSkin::colorized`; `skin_frame` cuts
  with `rl_content::skin_source`, as the profiles' preview does). Unknown ids and missing images never panic.
- **Interpolation** (`snapshot.rs`, Review Focus 1): `Interpolation::Linear`
  blends positions at sub-pixel precision, never across a projectile serial
  change (slot reuse), a death or a respawn (`worm_blends`: alive flag and
  `time_of_death`), nor a hook thrown since the snapshot (the rope's
  `NinjaRope::throws` count, so a rope thrown again while out never
  slides). Bonuses blend within one bonus (slot and `spawn_time`). Laser
  sights and beams start at their worm as drawn, and a hook on a worm
  sits on that worm as drawn (`tests/anchors.rs`), so nothing trails the
  blended worm.
  `Interpolation::Latest` is OLX: the latest state at whole pixels.
- **Exact OLX math** (`worm.rs`, `projectile.rs`, `marks.rs`, `camera.rs`):
  each function cites its OLX source. Offsets are OLX's screen pixels
  (half world pixels) from the drawn position. Deviations are listed in
  `docs/porting/STATUS.md` (presentation rows).

Test: `cargo nextest run -p rl-scene`. The scenario tests (interpolation,
order, marks, anchors, no-alloc, snapshots) need `RL_CONTENT_DIR` and skip without
it. Snapshots (`tests/snapshots/*.snap`) are `insta`'s: an intended change
is accepted with `INSTA_UPDATE=always` after reading the `.snap.new` diff.
