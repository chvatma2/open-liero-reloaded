# olr-fx

OLX's cosmetic entities (`src/client/Entity.cpp`, spec §19 E1/E8): blood,
blood droppers, dirt particles, gibs, explosions, smoke, chemical smoke,
doomsday, sparkles, spawn effects and jetpack spray. The porting reference
is `docs/porting/olx-notes/presentation.md` §7 (and §2.2 for stains).

- **Layer 5, pure CPU.** Depends on `olr-sim`, `olr-terrain`, `olr-content`,
  `olr-scene` (same layer, `xtask/layers.ron`), `olr-data`, `olr-math`. No GPU
  or OS handles. It only reads the world; its randomness (gib spins, drips)
  is its own `GlibcRand` (`Fx::new(seed)`), never the simulation's.
- **Pool** (`pool.rs`): OLX's `FastVector<entity_t, 1024>` of any size:
  lowest free slot first, new entities dropped when full, iteration in
  slot order, a freed slot keeps its contents, and `len` is a count, not a
  scan. Laser sights take a slot for two passes like OLX's, but
  `olr-scene` draws them (and the beams, which take none). Its size is the
  match's (`Fx::for_match`, spec §19 E1 amended 2026-10-13): in play
  `PLAY_ENTITIES` (16384, allocated once) of which the last
  `DEATH_RESERVE` (1024) only a death's gibs and blood take
  (`Event::death`; other spawns and drips stop short of it, so every
  death shows; `death_spawns`/`death_drops` count them); with
  `SimSettings::olx_quirks` (the oracle's scenarios, render goldens,
  `Fx::new`) OLX's `MAX_ENTITIES` and no reserve. A landed gib stamps
  only rock and dirt in play, open air too with `olx_quirks`
  (`stain_image`'s `on_air`; STATUS.md, "A gib stamps only rock and dirt"). `Fx::with_capacity` is
  the performance envelope's knob (plan 8; a sixteenth reserved), never
  used by a game.
- **Blood amount** (`blood.rs`, issue #6; a deliberate deviation in
  STATUS.md): `Fx::set_blood(percent)` (0-`BLOOD_MAX` 500, `BLOOD_DEFAULT`
  100, the player's Options → Video setting through
  `olr_present::Presentation::set_blood`) scales what `step_tick` spawns,
  never the simulation's draws. With `s` the amount: `Blood` events `s`
  times as many, `BloodDropper` events `min(s, 1)`, a dropper's drops
  `max(s, 1)` (none at 0 %). Fewer are spread by accumulators that start
  at a half (50 %: the first of each two); more are copies with
  velocities jittered by the amount's own `GlibcRand` (`seed ^
  0xb100d000`), so at 100 % nothing differs from before (render goldens,
  oracle scenarios) and other effects never shift with the amount. A
  copy spawns only below `Fx::extra_blood_limit` (play:
  `ordinary - EXTRA_BLOOD_HEADROOM`, an eighth of the pool; 0 with
  `olx_quirks`, OLX's pool having no reserve), so extra blood is refused
  before ordinary effects and never takes the death reserve; a death's
  own blood (the first of each event) still may. `Fx::spawn` is one
  entity whatever the amount. Tests: `tests/blood.rs` (0 %, 50 %, 130 %,
  500 %, the caps, determinism); `tests/matches.rs` runs
  `fx_never_touches_sim_randomness_at_500_percent_blood` and the
  allocation test at 500 % too.
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
- **Shades** (`draw.rs`, plan 15a Task 15, R13): `Fx::draw`'s `lit` (a match
  with a look) gives explosions, sparkles and spawn effects
  `olr_scene::Shade::GLOW`, and blood, blood droppers and gibs
  `Shade::FLOORED`, so they never show below the level's solid floor, as
  rock does (the user's playtest, 2026-10-19); dirt particles, smoke,
  chemical smoke, doomsday and jetpack spray stay `Shade::LIT`. Unlit, every
  entity is `Shade::LIT`. `olr-present` lights explosions from the entities
  (`Fx::entities`).
- **Assets** (`assets.rs`): `FxAssets` registers the core sprites and each
  worm's colorized gibs (`CWorm::ColorizeImage`, floats) in any `Atlas`
  (`SceneAssets` in the game), sharing the atlas's white pixel
  (`Atlas::white`); `default_dirt_colour` is the theme's `iDefaultColour`.
  Sprites arrive keyed: the importers apply OLX's corner rule once
  (`image_io::olx_sprite_to_alpha`), nothing keys at load (plan 13c).
- **Registration before upload.** `FxAssets::add_worm` keeps its own list
  parallel to `SceneAssets::add_worm` (same order: the worm's index in
  `World::worms`). Register every worm in both, then call
  `olr_render::Renderer::upload_atlas`: an image added after the upload
  draws as a magenta square until the atlas is uploaded again.

## The frame-loop contract

`olr_present::Presentation` implements this contract (and the registration
order above); new modes use it rather than wiring `Fx` by hand.

Per tick, after `world.tick(&keys, &mut events)`:

```rust
scene.after_tick(&world, &events);
fx.step_tick(&FxTick::new(&world, &events), &mut color_map, &fx_assets);
```

`step_tick` walks the events in order: it applies each `TerrainEdit` to the
colour map (`olr_content::color_edit`), spawns each cosmetic right before
entity pass `substep` (spec §19), runs the passes left, and rebuilds a
`lost_area`. So a `Dirt` particle samples the colour image after the edits
before it, and stains land between the edits as in OLX. **The caller must not
also call `olr_content::apply_edits` for the same tick.** Per drawn frame,
`fx.draw(&fx_assets, lit, &mut list)` (`lit`: a match with a look, see
Shades above) after `scene.build`. On a restart:
`fx.clear()` and a new colour map.

Test: `cargo nextest run -p olr-fx`. `tests/entities.rs` and `tests/events.rs`
use a made-up level (`blood_and_gibs_take_the_solid_floor_when_lit` pins
the shades); `tests/matches.rs` plays oracle scenarios and needs
`OLR_CONTENT_DIR` (it skips without it).
