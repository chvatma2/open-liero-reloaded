# rl-fx

OLX's cosmetic entities (`src/client/Entity.cpp`, spec §19 E1/E8): blood,
blood droppers, dirt particles, gibs, explosions, smoke, chemical smoke,
doomsday, sparkles, spawn effects and jetpack spray. The porting reference
is `docs/porting/olx-notes/presentation.md` §7 (and §2.2 for stains).

- **Layer 5, pure CPU.** Depends on `rl-sim`, `rl-terrain`, `rl-content`,
  `rl-scene` (same layer, `xtask/layers.ron`), `rl-data`, `rl-math`. No GPU
  or OS handles. It only reads the world; its randomness (gib spins, drips)
  is its own `GlibcRand` (`Fx::new(seed)`), never the simulation's.
- **Pool** (`pool.rs`): OLX's `FastVector<entity_t, 1024>`: lowest free
  slot first, new entities dropped when full, iteration in slot order, and
  a freed slot keeps its contents. Laser sights take a slot for two passes
  like OLX's, but `rl-scene` draws them (and the beams, which take none).
- **Physics** (`entity.rs`): one `SimulateEntities` pass per sub-step,
  motion with the sub-step `dt` (`LX56PhysicsDT`, 11 ms), animation with
  OLX's fixed 10 ms frame, all in `f32` like OLX (the sums drift; tests pin
  the resulting frame counts). Collisions read the colour map's own
  material, which is in step with the edits replayed so far.
- **Drawing** (`draw.rs`): `DrawEntities` into `Layer::Entities` of the
  scene's `DrawList`, at OLX's half-pixel screen position relative to
  `out.view`'s corner truncated to whole pixels (OLX's `WorldX` is an int,
  so the grid stays the world's under a sub-pixel camera), with OLX's clipping and SDL's source clipping (frames before
  or past an image draw nothing). Entities are drawn at their latest state
  (no interpolation).
- **Assets** (`assets.rs`): `FxAssets` registers the core sprites and each
  worm's colorized gibs (`CWorm::ColorizeImage`, floats) in any `Atlas`
  (`SceneAssets` in the game), sharing the atlas's white pixel
  (`Atlas::white`); `default_dirt_colour` is the theme's `iDefaultColour`.
  Sprites are keyed with OLX's corner rule (`RgbaImage::key_transparent`).
- **Registration before upload.** `FxAssets::add_worm` keeps its own list
  parallel to `SceneAssets::add_worm` (same order: the worm's index in
  `World::worms`). Register every worm in both, then call
  `rl_render::Renderer::upload_atlas`: an image added after the upload
  draws as a magenta square until the atlas is uploaded again.

## The frame-loop contract

`rl_present::Presentation` implements this contract (and the registration
order above); new modes use it rather than wiring `Fx` by hand.

Per tick, after `world.tick(&keys, &mut events)`:

```rust
scene.after_tick(&world, &events);
fx.step_tick(&FxTick::new(&world, &events), &mut color_map, &fx_assets);
```

`step_tick` walks the events in order: it applies each `TerrainEdit` to the
colour map (`rl_content::color_edit`), spawns each cosmetic right before
entity pass `substep` (spec §19), runs the passes left, and rebuilds a
`lost_area`. So a `Dirt` particle samples the colour image after the edits
before it, and stains land between the edits as in OLX. **The caller must not
also call `rl_content::apply_edits` for the same tick.** Per drawn frame,
`fx.draw(&fx_assets, &mut list)` after `scene.build`. On a restart:
`fx.clear()` and a new colour map.

Test: `cargo nextest run -p rl-fx`. `tests/entities.rs` and `tests/events.rs`
use a made-up level; `tests/matches.rs` plays oracle scenarios and needs
`RL_CONTENT_DIR` (it skips without it).
