# rl-sim

The deterministic simulation (spec §5), a port of OLX's LX56 physics at
commit `d1ad953f`. The oracle traces in `fixtures/oracle` are its acceptance
tests, and they must match bit for bit (`tests/oracle.rs`).

- **Layer 3, `#![no_std]` + `alloc`.** Apart from setup (`spawn_worm`,
  `set_loadout`, `align_physics_clock`), `World::tick(&[Keys])` is the only
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
  in `projectile.rs`. `tick` must not allocate: buffers are sized in `new`.
- **Not here yet (M4):** projectile motion, explosions, beams' map trace,
  bonuses, damage.

Test: `cargo nextest run -p rl-sim`. The oracle test needs imported content;
`cargo xtask check` sets `RL_CONTENT_DIR`.
