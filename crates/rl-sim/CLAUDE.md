# rl-sim

The deterministic simulation (spec §5), a port of OLX's LX56 physics at
commit `d1ad953f`. The oracle traces in `fixtures/oracle` are its acceptance
tests, and they must match bit for bit (`tests/oracle.rs`).

- **Layer 3, `#![no_std]` + `alloc`.** Apart from setup (`spawn_worm`,
  `set_loadout`, `set_green_mask`, `align_physics_clock`), `World::tick(&[Keys])` is the only
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
  counts) are `effects.rs`, and worm damage is `damage.rs`.
  Projectile `pos`/`vel` assignments that OLX makes through `Attr::operator=`
  use `attr_set` (an equal value, such as `+0` over `-0`, is not written).
- **Not here yet (M4):** `damage::injure_worm` only changes health: its
  blood draws, death and the kill are Task 7's. Beams' map trace, bonuses.

Test: `cargo nextest run -p rl-sim`. The oracle test needs imported content;
`cargo xtask check` sets `RL_CONTENT_DIR`.
