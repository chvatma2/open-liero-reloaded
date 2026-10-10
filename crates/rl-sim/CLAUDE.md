# rl-sim

The deterministic simulation (spec §5), a port of OLX's LX56 physics at
commit `d1ad953f`. The oracle traces in `fixtures/oracle` are its acceptance
tests, and they must match bit for bit (`tests/oracle.rs`).

- **Layer 3, `#![no_std]` + `alloc`.** Apart from setup (`spawn_worm`,
  `set_loadout`, `set_green_mask`, `set_bonus_weapons`, `align_physics_clock`,
  and `place_bonus`, applied at the next tick), `World::tick(&[Keys])` is the only
  way state changes, and it does not allocate (`tests/no_alloc.rs`).
- **Two clocks.** `tick` is one 100 Hz frame. Inside it, LX56 physics runs in
  `1000 / LX56PhysicsFPS` ms sub-steps (11 ms by default) while the physics
  clock is behind the frame clock, so some frames run no sub-step.
- **Input** (`input.rs`): keys are latched between reads exactly like OLX's
  `CInput`. See `docs/porting/STATUS.md`.
- **Port, don't improve.** Keep OLX's operation order and quirks; cite the
  OLX source for each function. Any change must keep `tests/oracle.rs` green.
- **Randomness** (`rng.rs`): the world owns the `rand()` and `FindSpot`
  generators, seeded by `World::new(.., seed)`, plus the dedicated server's
  entity counter. Cosmetic draws (digging, rope hook) are made and discarded,
  in OLX's order, because they advance the stream gameplay draws from.
- **Weapons** (M4): `World::new` takes an `Arc<SimMod>` (`mod_rt.rs`), the
  mod compiled to OLX's indices: weapon ids are `ModDef` order, projectiles
  are expanded into OLX's per-occurrence load-order tree with path names
  (`bazooka.spawn`). Slots and firing are in `weapons.rs`, the shot list,
  its byte-exact network round trip (`shot_codec.rs`) and Model R delivery in
  `shots.rs` (delivered at the start of `tick`), projectiles and their pool
  in `projectile.rs`, spawning in `spawn.rs`. `tick` must not allocate:
  buffers are sized in `new`.
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

Test: `cargo nextest run -p rl-sim`. The oracle test needs imported content;
`cargo xtask check` sets `RL_CONTENT_DIR`.
