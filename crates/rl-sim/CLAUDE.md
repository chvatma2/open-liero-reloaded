# rl-sim

The deterministic simulation (spec §5), a port of OLX's LX56 physics at
commit `d1ad953f`. The oracle traces in `fixtures/oracle` are its acceptance
tests, and they must match bit for bit (`tests/oracle.rs`).

- **Layer 3, `#![no_std]` + `alloc`.** Apart from setup (`spawn_worm`,
  `set_loadout`, `set_green_mask`, `set_bonus_weapons`, `align_physics_clock`,
  `place_bonus`, applied at the next tick, and `find_spot`), `World::tick(&[Keys],
  &mut TickEvents)` is the only way state changes, and it does not allocate
  (`tests/no_alloc.rs`).
- **Two clocks.** `tick` is one 100 Hz frame. Inside it, LX56 physics runs in
  `1000 / LX56PhysicsFPS` ms sub-steps (11 ms by default) while the physics
  clock is behind the frame clock, so some frames run no sub-step.
- **Input** (`input.rs`): keys are latched between reads exactly like OLX's
  `CInput`. See `docs/porting/STATUS.md`.
- **Port, don't improve.** Keep OLX's operation order and quirks; cite the
  OLX source for each function. Any change must keep `tests/oracle.rs` green.
- **Randomness** (`rng.rs`): the world owns the `rand()` and `FindSpot`
  generators, seeded by `World::new(.., seed)`, plus the dedicated server's
  entity counter. Cosmetic draws (digging, rope hook, blood) are made in
  OLX's order, because they advance the stream gameplay draws from, and only
  reported as events.
- **Weapons** (M4): `World::new` takes an `Arc<SimMod>` (`mod_rt.rs`), the
  mod compiled to OLX's indices: weapon ids are `ModDef` order, projectiles
  are expanded into OLX's per-occurrence load-order tree with path names
  (`bazooka.spawn`). Slots and firing are in `weapons.rs`, the shot list,
  its byte-exact network round trip (`shot_codec.rs`) and Model R delivery in
  `shots.rs` (delivered at the start of `tick`), projectiles and their pool
  in `projectile.rs`, spawning in `spawn.rs`. `tick` must not allocate:
  buffers are sized in `new`. Hostile mods must not stall a tick: a spawn
  `Amount` is at most `MAX_AMOUNT`, and once the pool is full a spawn skips
  its children's work but makes their draws (`skip_children`);
  `Draws::spawn_work` counts the children computed in full, for tests (not
  state: `WorkCounter` is always equal). `Draws::rot_bumped` keeps OLX's
  `RotIncrement` 0→1 mutation per projectile type (`World::rot_bumped`), and
  each projectile gets a spawn serial (`Projectile::serial`), each rope
  throw a count (`NinjaRope::throws`), so the renderer never blends across
  a re-throw.
- **Projectiles** (`proj_sim.rs`): after all worms in each sub-step, every
  projectile catches up its own clock in slot order (children in higher
  slots run in the same pass). Hit tests read the worm's `PosRecorder`
  (`worm.rs`, OLX's `get(0)` quirk kept). What events do is `actions.rs`;
  their map and health effects (explosions, carving, dirt, green dirt, dirt
  counts) are `effects.rs`.
  Projectile `pos`/`vel` assignments that OLX makes through `Attr::operator=`
  use `attr_set` (an equal value, such as `+0` over `-0`, is not written).
- **Damage and death** (`damage.rs`): `injure_worm` is `CClient::InjureWorm`
  with its blood draws; a kill (`kill_worm`) counts kills, deaths, suicides
  and lives at once, as OLX's server does. A worm left at exactly 0 health
  dies at the start of the next frame, uncredited (`die_at_zero_health`).
  Bleeding is in `physics.rs`. After the physics, the server's respawn loop
  (`respawn.rs`) respawns dead worms at a `FindSpot` position drawn from the
  `Mt19937`. Game over is not ported.
- **Beams and specials** (`beam.rs`): `process_shot_beam` at delivery
  (damage, carving, `LastFire = 0.3`), `draw_beam` from `player_shoot` in
  every sub-step the slot counts down (carves, no damage), and the jetpack.
  The trace's width buffer is sized in `World::new`.
- **Bonuses** (`bonus.rs`): the server's and the client's arrays, and the
  reliable messages between them (D10), delivered at the start of `tick`:
  spawns and destroys to the client, then the shots, then grabs to the
  server. The server spawns by `BonusFrequency` (`FindSpot`, two `rand()`)
  and keeps a slot for the whole `BonusLife`; the client's bonuses fall
  after the projectiles, are picked up after each worm's physics, and
  explode in `effects::explosion` and in the beam trace.
- **Events** (`events.rs`, spec §5.7): `tick` refills the caller's
  `TickEvents` (`TickEvents::new` sizes it once), so a world clone copies
  no events. During the tick the buffer is swapped into `Draws::events`,
  where the emitters push; between ticks `Draws::events` holds the few
  setup events (`spawn_worm`'s), which come first in the next tick's. Each
  `Event` is `{ substep, kind: EventKind }`: `substep` counts the sub-steps
  whose entity pass (after the worm loop) ran before it, up to
  `World::substeps_last_tick()`. Emit them where OLX plays a
  sound, spawns an entity (`Draws::entity`, which also takes the pool slot)
  or edits the map (`effects::carve_px`/`carve_at`), never with a draw of
  their own (D4/D5); cosmetics are emitted even when the entity pool is full.
  Cosmetics, terrain edits and other gameplay events have separate quotas
  in the buffer, so none can crowd out another; drops are counted
  (`cosmetic_dropped`, `gameplay_dropped`) and a dropped edit's area is
  reported (`lost_area`), never lost silently (see `TickEvents`' doc for why
  no fixed size bounds every tick).
  `SoundId` indexes `SimMod::sounds()`: OLX's game sounds first, each prefixed
  `olx:` (`GAME_SOUND_PREFIX`, a path in OLX's data directory), then the mod's
  (paths in its pack, never prefixed), so a loader can tell the roots apart.
  The renderer's data (spec §19 E6): `Explosion.shake`, `Gib { worm }`,
  `Dirt { probe }` (the pixel the colour comes from), `Damage.dealt`,
  `LaserSight { len }` (OLX's trace length), `World::physics_time()`.
  Spec §5.7's `Message` and
  `GameOver` events are not emitted yet (plan 6).
- **Checksum** (`checksum.rs`): FNV-1a 64 over all mutable state, through an
  exhaustive destructuring of each type; a new field must be hashed there
  (types with private fields hash themselves in their module). Tick-time
  buffers are `buf::Buf`, whose clone keeps the capacity, so a cloned world
  ticks without allocating too.

Test: `cargo nextest run -p rl-sim`. The oracle test needs imported content;
`cargo xtask check` sets `RL_CONTENT_DIR`.
