# olr-sim

The deterministic simulation (spec §5), a port of OLX's LX56 physics at
commit `d1ad953f`. The oracle traces in `fixtures/oracle` are its acceptance
tests, and they must match bit for bit (`tests/oracle.rs`).

- **Layer 3, `#![no_std]` + `alloc`.** A match is `World::new(MatchSetup,
  Box<dyn GameMode>) -> Result<World, SetupError>` (`setup.rs`: settings
  checked, worms spawned in order, `MatchSetup::T0` start, pool size).
  Apart from setup (`set_health`, `align_physics_clock`, `place_bonus`,
  applied at the next tick, and `find_spot`), `World::tick(&[Keys], &mut
  TickEvents)` is the only way state changes, and it does not allocate,
  mode callbacks and commands included (`tests/no_alloc.rs`). In-crate
  tests build worlds with `World::test_new` (the old positional form,
  settings unchecked) and `spawn_worm`/`set_loadout`.
- **Game modes** (`mode.rs`, `referee.rs`; spec §20 F6): the world calls
  the mode where OLX calls its `CGameMode` (`on_kill` inside `killWorm`,
  returning a `KillCredit`; `on_damage`, `on_pickup`, `on_spawn`;
  `is_over` at the start of `SimulateGame`, `on_tick` at its end). Its
  commands (`ModeCtx`: spawn, give weapon, stamp, message, end) are checked
  when queued and applied in order right after `on_tick`; a spawn's slots
  come from vectors made at setup (`setup::Spare`). Game over
  (`World::game_over`, OLX's `GameServer::GameOver`) gates kills, respawns,
  bonus spawns, input and shooting, clears the client's projectiles at the
  next delivery and stops the physics after 3 s. `OracleMode` is OLX's bare
  crediting (`olx_kill_credit`) with no end: the oracle and the sandbox
  play it. A mode's state is hashed by `GameMode::checksum`; `World::clone`
  uses `box_clone`; `is_over(&World)` sees the whole world, the mode
  included (`World::scores`); `World::mode_state().as_any()` downcasts to
  the mode's own type. `Score::left` marks a row whose worm left.
  `Score::deaths`/`suicides` are the mode's counts (Deathmatch: every
  one). `Worm::deaths`/`suicides` are OLX's score-gated counters, which
  the oracle checks.
- **Leaving players** (decision N82): `World::withdraw(i)` takes worm `i`
  out for good, between ticks (OLX `RemoveClientWorms`): dead, `WRM_OUT`,
  rope, input state and latches cleared, never respawned, still slot `i`;
  the mode is told by `GameMode::on_drop` (`CGameMode::Drop`), which gives
  no commands (`Commands::closed`, `CommandError::Closed`: the queue stays
  empty between ticks, and a state transfer carries none), and the next
  `is_over` counts it out. `World::gone()` is the mask (bit `i` for
  worm `i`), kept in the `Referee` (never on `Worm`: its 992-byte size
  stays) and hashed **only when not 0**, so a match nobody left hashes as
  before (match goldens, oracle). Every `ModeCtx` carries it
  (`ModeCtx::gone`). A gone killer is nobody: `kill_worm` maps it to -1
  (decision N84), for the mode and the `Kill` event alike; a gone owner of
  damage is none in `injure_worm` (no damage factor, no damage score, no
  `last_damager`: final review A-M2).
- **Rollback** (`snapshot.rs`, networking spec §3.3): `enable_rollback` +
  `save`/`restore`/`forget_before`; `sync_checksum` is `checksum` with the
  terrain's cell hash. Snapshots carry no events; the caller re-runs ticks
  for them. `take_restored_areas` drains the map areas a restore put back
  (the undone edits' rectangles; the whole map for a restore without
  rollback or a `load_state`), for the colour map's `rebuild_area`.
- **State transfer** (`state.rs`, networking spec §3.5): `World::state`/`load_state`;
  a new state field must be added to `WorldState` (the destructuring forces it)
  and validated if it indexes anything. `WorldState` holds what `checksum`
  hashes (postcard, then deflate; `MAX_INFLATED`), the mode as
  `GameMode::save_state` bytes. The sender may be a stranger: `validate`
  refuses (`StateError`, nothing changed) anything that could index out of
  bounds, outgrow a `Buf`, overflow a clock or counter, or stall a tick
  (projectile clocks, rope speed, aim), and every float of a worm (rope,
  slots, position record), projectile in use, bonus in use or shot that is
  NaN or infinite (`worm_finite` and its siblings, exhaustive
  destructurings: a new float field must be listed); the proptest in
  `tests/state.rs` fuzzes it, `state_tests.rs` refuses each non-finite
  field, `olr-cli`'s `tests/state_transfer.rs` loads chaos matches' states.
  The receiving world must come from the same setup (`setup_worms`, map
  size, pool size, bonus weapons are checked). `WorldState::gone` (the
  layout's last field) is refused (`StateError::Worms`) when it names no
  worm, or a worm alive or not `WRM_OUT` (`gone_ok`).
- **Two clocks.** `tick` is one 100 Hz frame. Inside it, LX56 physics runs in
  `1000 / LX56PhysicsFPS` ms sub-steps (11 ms by default) while the physics
  clock is behind the frame clock, so some frames run no sub-step.
  `substeps_ahead(k)` foretells the sub-steps of the tick `k` after the
  next and `latches(i)` shows a worm's pending presses, both read-only,
  for `olr-input`'s mouse controller (issue #24).
- **Input** (`input.rs`): keys are latched between reads exactly like OLX's
  `CInput`. See `docs/porting/STATUS.md`. `Keys` is a `u16` of 16 bits: the
  classic 8, then `STRAFE`, `PREV_WEAPON`, `NEXT_WEAPON` (issue #23;
  strafing is always allowed, a deviation), then `WEAPON1`..`WEAPON5`
  (issue #48, `Keys::WEAPONS`, `Latches::weapons`: held, each selects its
  slot, `physics::select_slot`; beyond the worm's slots nothing in play,
  OLX's wrap with `olx_quirks`). The checksum hashes issue #23's three
  latches only when one is not at rest, and the five weapon latches only
  when one of them is not at rest, so worlds that never press them hash
  as before (match goldens, oracle).
- **Port, don't improve.** Keep OLX's operation order and quirks; cite the
  OLX source for each function. Any change must keep `tests/oracle.rs` green.
- **Randomness** (`rng.rs`): the world owns the `rand()` and `FindSpot`
  generators, seeded by `MatchSetup::seed`, plus the dedicated server's
  entity counter. Cosmetic draws (digging, rope hook, blood) are made in
  OLX's order, because they advance the stream gameplay draws from, and only
  reported as events.
- **Weapons** (M4): `MatchSetup` holds an `Arc<SimMod>` (`mod_rt.rs`), the
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
  state: `WorkCounter` is always equal); `Draws::actions` (`tally.rs`,
  `World::actions_triggered`) counts the projectile actions applied by
  kind for coverage reports, not state either (`projectile_actions(def)`
  lists what a projectile can apply). `Draws::rot_bumped` keeps OLX's
  `RotIncrement` 0→1 mutation per projectile type (`World::rot_bumped`), and
  each projectile gets a spawn serial (`Projectile::serial`), each rope
  throw a count (`NinjaRope::throws`), so the renderer never blends across
  a re-throw.
- **Projectiles** (`proj_sim.rs`): after all worms in each sub-step, every
  projectile catches up its own clock in slot order (children in higher
  slots run in the same pass). Hit tests read the worm's `PosRecorder`
  (`worm.rs`, `hit_position`: the newest entry; OLX's `get(0)` quirk only
  with `SimSettings::olx_quirks`, which an oracle trace's settings turn
  on; spec §18 D7 as amended). `World::hit_audit` (`tally.rs`, not state)
  counts the worm hits from outside the struck worm's current hit
  geometry, for `olr-cli`'s `tests/hits.rs`. What events do is `actions.rs`;
  their map and health effects (explosions, carving, dirt, green dirt, dirt
  counts) are `effects.rs`.
  Projectile `pos`/`vel` assignments that OLX makes through `Attr::operator=`
  use `attr_set` (an equal value, such as `+0` over `-0`, is not written).
- **Damage and death** (`damage.rs`): `injure_worm` is `CClient::InjureWorm`
  with its blood draws; a kill (`kill_worm`) counts kills, deaths, suicides
  (as the mode's `KillCredit` says) and lives at once, as OLX's server
  does. A worm left at exactly 0 health dies at the start of the next
  frame (`die_at_zero_health`): in play credited to its `last_damager`
  (the owner of the damage that left 0, cleared on respawn) with the
  lethal hit's death sound, gibs and blood (`death_effects`; after the
  game over too, unless a lethal hit that killed nobody already played
  them, `Worm::death_shown`); with
  `SimSettings::olx_quirks`, as OLX, uncredited and without them (spec
  §18 D9 as amended). A death's sound, gibs and blood are pushed as the
  death's where they are emitted (`TickEvents::death`,
  `Draws::death_entity`; `Event::death` marks them for `olr-fx`'s death
  reserve) and count against the gameplay quota, so no tick's other
  cosmetics crowd them out.
  Bleeding is in `physics.rs`. After the physics, the server's respawn loop
  (`respawn.rs`) respawns dead worms at a `FindSpot` position drawn from the
  `Mt19937`.
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
  `CosmeticKind::Explosion { size, proj }` (issue #15: the exploded
  projectile's `SimMod` index from its effects, timer or `Bounce`
  explosion, which `effects::explosion` takes only for it; -1 for a bonus
  blown up, a beam and an explosive trail; in no checksum; issue #42:
  `Explosion.proj`, the same, for the projectile look's explosion heat),
  `Dirt { probe }` (the pixel the colour comes from), `Damage.dealt`,
  `LaserSight { worm, len }` (OLX's trace length), `Beam { worm, colour }`,
  `World::physics_time()`. `Message` comes from a mode's command,
  `GameOver` once when the match ends.
- **Checksum** (`checksum.rs`): FNV-1a 64 over all mutable state, through an
  exhaustive destructuring of each type; a new field must be hashed there
  (types with private fields hash themselves in their module, the mode
  through `GameMode::checksum` with the public `Fnv`). Every NaN hashes as
  one canonical NaN. Tick-time buffers are `buf::Buf`, whose clone keeps
  the capacity, so a cloned world ticks without allocating too.

Bench: `benches/tick.rs` (divan, `cargo xtask bench`): a tick with OLX's
pool full (3000 bouncing pellets of a mod made there) and a tick of 32
walking worms; no content needed. `tick_32_worms` is sensitive to code
layout (`Worm`'s size, asserted at 992 bytes, and inlining: a constructor
the tick never calls once cost it 7 %, hence `#[inline(never)]` on
`Worm::spawned`): A/B a change against its parent in interleaved runs
before blaming it.

Test: `cargo nextest run -p olr-sim`. The oracle test needs imported content;
`cargo xtask check` sets `OLR_CONTENT_DIR`.
