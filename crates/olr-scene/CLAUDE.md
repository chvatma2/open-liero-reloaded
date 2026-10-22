# olr-scene

Turns a `World` into a `DrawList` (spec §8.2, §19 E1-E4): OLX's camera,
interpolation between the last two ticks, and OLX's sprite math for worms,
ropes, laser sights, beams, projectiles and bonuses. The porting reference
is `docs/porting/olx-notes/presentation.md` §1, §3-§6 and §11.

- **Layer 5, pure CPU.** Depends on `olr-sim`, `olr-content`, `olr-data`,
  `olr-math`. No GPU or OS handles; it only reads the world, and its
  randomness (shake, laser dots) comes from its own `GlibcRand`s.
- **Use** (`lib.rs`): a `Scene` per match (`olr-present` bundles it
  with the entities), with a camera per view. `start(&world)` for a new or restarted world, then
  `after_tick(&world, &events)` after every tick:
  the old state becomes the previous `Snapshot`, explosions shake every
  camera whose view holds them, and the tick's `Marks` (muzzle flashes, green crosshairs, laser
  sights, beams) replace the last. Online play calls
  `after_tick_resimulated` for a tick a rollback simulates again: the
  same without the camera shake (`Camera::shake_for(&[Event], now)` shakes
  for an explosion first seen in it). `build(&world, alpha, &assets, &mut list)`
  per drawn frame: `Camera::process` (hard lock, shake, clamp), then the
  layers. The game's view is `Viewport::GAME`, 427 × 192 world pixels for
  every window (spec §19 E4 as amended after the v0.1 playtest);
  `Viewport::from_aspect` (240 high) stays for tools and the goldens. The caller appends `olr-fx`'s entities to `Layer::Entities`.
- **Views** (issue #27, split screen): `Scene::new` makes one view
  (`camera()`, `camera_mut()`); `add_view(Camera)` adds one (at most
  `MAX_VIEWS`, 4: OLX's fork's local players), `truncate_views(n)` keeps
  the first (at least one), `cameras()`/`cameras_mut()`, `views()`.
  Each camera follows its own worm with the solo rules (hard lock,
  clamp, shake for an explosion in its own view, a dead worm's view
  staying, the jump at the respawn); `Scene::shake_for` shakes them all
  (online play's first-seen explosions). `listeners()` is each view's
  `Camera::listener` (a `Listeners`, a slice of up to four). Everything
  else is shared: the snapshots, the marks, the shade rules and the look
  records. `build_views(&world, alpha, &assets, &mut [DrawList])` draws
  view `v` into list `v` (`build` is it for one list: the first view):
  one blend, each camera's `process`, the layers per view; the laser
  sights' dots are rewound for each view after the first
  (`Marks::dots`, `rewind_dots`), so every view draws the frame's same
  dots and the generator moves on once a frame; every list holds every
  occluder; `drawn_projectiles()` and `drawn_worms()` come from the first
  view's pass. View `v`'s list equals a one-view scene's with `v`'s
  camera (`tests/views.rs`), and one view draws exactly as before (the
  snapshots).
- **`DrawList`** (`draw_list.rs`): the view rectangle and five layers in
  OLX's order (`Layer::ALL`): entities, beams (laser sights, then worm
  beams, as `DrawEntities` draws them), projectiles, bonuses, worms (rope,
  hook, health bar, crosshair, skin, muzzle per worm). Items are
  `Sprite { atlas, src, pos, size, flip, tint, shade }` in world pixels (OLX's
  sprites are 2× art, so positions fall on half pixels) and
  `Line { a, b, colour, shade }`, a 1-pixel `perform_line`. A solid block is the
  `white` image tinted (`pixel`, or `pixel_shaded` with a shade).
  `describe` is the compact text the snapshot tests commit; it names a
  shade only when it is not `Shade::LIT` (`glow N`, `marker`, `floored`,
  `worm floor`),
  so the snapshots of unlit matches stay as they were. Layers keep `RESERVED` items of room (the entities `ENTITIES_RESERVED`,
  olr-fx's play pool), so frames do not allocate
  (`tests/no_alloc.rs`).
- **Parallax camera** (issue #11): `DrawList::parallax`, `None` but in
  the level editor (its canvas's centre: the view there is cut to the
  device's target), and `camera()`, it or the view's centre: where the
  renderer places a level's distant layers.
- **Shades** (`draw_list.rs`, `lib.rs`, plan 15a Task 15, R5 and R13):
  every item carries a `Shade { self_light, marker, floor }`, which
  only the renderer's look passes read: `Shade::LIT` (lit by the world, the
  default), `GLOW` (100 % self-light), `MARKER` (drawn as its art, unlit
  and ungraded), `FLOORED` (`Floor::Solid`: lit, never below the level's
  solid floor, as rock: blood and gibs, the user's playtest, 2026-10-19)
  and `WORM` (`Floor::Worm`: never below the worm floor; worms, their rope
  and hook, plan 15b).
  `Scene::set_shades(Option<ShadeRules>)` gives a lit match its rules
  (`olr-present` sets them; `None`, the default, draws every item lit, as
  always): `ShadeRules::floor` is the least self-light of projectiles and
  bonuses (worms take `Shade::WORM`), and `projectile_glow` (by `SimMod`
  index) raises a projectile's to its look's glow; `projectile_heat`
  (issue #42, by `SimMod` index) marks the projectiles whose look heats, for the look records. With rules the muzzle
  flash and the beams glow, and the crosshair, health bars and laser-sight
  dots are markers (`Marks::draw`'s `lit`, `WormFrame::lit`).
- **Occluders** (plan 15b): in a lit match `build` lists each living
  worm's body (`worm::body`: its skin frame's atlas, source, position, size
  and flip, `worm` = index + 1) in `DrawList::occluders`, whatever the
  view; the renderer draws them into its occluder map, which stops no
  light (#53: worms take no part in the light; #54's halo is to use it as
  their silhouette). `clear` empties
  them (256 of room, no per-frame allocation).
- **Look records** (`lib.rs`, plan 15a Task 15 and the batch C review),
  kept only with shade rules: each `build` records `drawn_projectiles()`
  (`DrawnProjectile`: slot, serial, `SimMod` index, drawn position, spawn
  time, view or not) and `drawn_worms()` (`DrawnWorm`: id, index, drawn
  position, occluder id, skin frame: issue #12's skin lights);
  each
  `after_tick` and `after_tick_resimulated` records `ended_projectiles()`
  (`EndedProjectile`: a projectile in the previous snapshot whose slot is
  empty or holds another serial, at its last position; `ProjSnap` now
  keeps `proj` and `spawn_time`) and `shots()` (`Shot`: a projectile
  weapon's `WeaponFired`) and, issue #40, `explosions()` (`TickExplosion`:
  each `EventKind::Explosion`, a projectile's or a bonus blown up, never
  an explosive trail's or a beam's cosmetic: its position, damage, tick
  and `index` among the tick's; issue #42: `proj`, the exploded
  projectile, -1 for none) and, issue #42, `hot_projectiles()`
  (`HotProjectile`: slot, serial, `SimMod` index, position and tick of
  each projectile in the new snapshot that `ShadeRules::projectile_heat`
  marks, slot order, at most `MAX_HOT_PER_TICK` (64) a tick; nothing when
  the list is empty), per tick so end flashes, muzzle lights
  and heat never depend on the frame rate. `clear_look_events` forgets them (the
  presentation takes them each frame); at most `LOOK_EVENTS` (8192) of
  each are kept between two clears. The first tick re-simulated after a
  rollback reads no ends (its previous snapshot is the old timeline's, a
  later time); a re-simulated tick reports its ends again, and
  `olr-present` starts each once.
- **`SceneAssets`** (`assets.rs`): core sprites and mod projectile images
  (already alpha: the importers key OLX's magenta once, nothing keys at load), and per worm a colorized
  skin (`add_worm`: `olr_content::LoadedSkin::colorized`, at the skin's art
  scale as `px_per_world`; `skin_frame(angle, face, walk, layout, frames)`
  cuts with `olr_content::skin_source`, as the profiles' preview does).
  Issue #12: a `WormLook` carries the skin's `layout` and `glow` (an atlas
  image per glow quarter, `LoadedSkin::glow_overlays`), drawn over the
  body in a lit match only (`Shade { self_light: 25-100, ..Shade::WORM }`);
  `worm_light(i, frame)` is the frame's `SkinFrameLight` and the skin
  light's reach (`LoadedSkin::frame_lights`, made once in `add_worm`).
  The body sits `BODY_OFFSET` (5, 4.5) world pixels up-left of the worm
  whatever the layout. Issue #19: a mod image with a glow mask
  (`LoadedMod::glow_overlays`) gets an atlas image per glow quarter
  (`mod:<file>:glow<k>`, `projectile_glow(proj)`), which `draw_projectile`
  draws over the sprite (same frame, place and size) in a lit match only
  (`lit`), at `max(k × 25, the sprite's self-light)`; a sprite without a
  mask draws as before. Unknown ids and missing images never panic.
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

Test: `cargo nextest run -p olr-scene` (issue #12: `frames.rs`'s
`skin_frame_picks_double_frames`; issue #27's `views.rs`:
`two_cameras_follow_two_worms_through_shake_death_and_respawn`, unlit and
lit, each view's list the single view's frame for frame,
`views_draw_the_laser_sights_dots_alike`, `a_scene_holds_one_to_four_views`). The scenario tests (interpolation,
order, marks, anchors, no-alloc, snapshots) need `OLR_CONTENT_DIR` and skip without
it. Snapshots (`tests/snapshots/*.snap`) are `insta`'s: an intended change
is accepted with `INSTA_UPDATE=always` after reading the `.snap.new` diff.
