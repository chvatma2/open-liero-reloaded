# Plan 4: Weapons, Damage and Bonuses, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port OLX's LX56 weapons to `rl-sim`: firing, the shot round trip, projectiles and all actions used by Classic and MW 1.0, explosions, carving and dirt, beams, damage, death, respawn and bonuses. Every weapon of both mods must match OLX's recorded traces bit for bit (milestone M4).

**Architecture:**
- **Oracle v2 first.** OLX delivers shots to projectiles through its network layer, and three projectile paths read the wall clock. Recording projectiles is therefore impossible until the hooks make delivery and time deterministic (Task 1).
- **Simulation.** Everything is ported into `rl-sim` (layer 3, `no_std`), driven by a `SimMod` compiled from `rl_data::ModDef`. Randomness (glibc `rand()`, the `RandomNumbers` table, boost `mt19937`) goes into `rl-math`.
- **Acceptance.** `rl-scenario` replays every scenario. A field mask (`rl_oracle::Fields`) widens task by task until the whole trace is compared.

**Tech stack:** Rust 1.99, `libm`, the existing `rl-*` crates. Docker for Tasks 1 and 3 (recording).

**Spec:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`. Read §5, §6, §10 (M4) and the amendments §15–§17. This plan adds §18 (D1–D9) in Task 1.

**Porting references (read the sections a task names):** `docs/porting/olx-notes/`
- `weapons-firing.md`: fire key, slot logic, shot list encoding, `Proj_SpawnInfo::apply`, beams, specials, hooks;
- `projectiles.md`: the container, clocks, collision, events, actions, explosions, carving, dirt;
- `damage-death-bonuses.md`: injury, death, respawn, `FindSpot`, bonuses, Deathmatch;
- `randomness.md`: every `rand()` consumer, glibc `rand()`, float conversions, bots.

These notes were written from the source at commit `d1ad953f` and cite line ranges. When a note and the source disagree, the source wins; fix the note in the same commit.

## How this plan was made, and how it differs from plans 1–3

Plans 1–3 contained verbatim code, verified by replaying them in a scratch clone. This plan does not:
- **Why.** M4 ports about 4000 lines of OLX logic, and the oracle traces are the verification. Pre-writing and replaying that code would double the work without adding evidence.
- **What each task gives instead:**
  - the OLX functions to port, with file:line;
  - the Rust module and its interface;
  - named unit tests with their exact assertions;
  - the oracle scenarios that must match, and the field mask they are compared with.
- **The implementer** writes the port test-first: unit tests RED, then GREEN, then scenario comparisons that go from diverging to matching.

## Global Constraints

- **Toolchain and license.** Rust 1.99, edition 2024, GPL-3.0-or-later. The crate prefix is `rl-`, and every crate has a `CLAUDE.md` and is registered in `xtask/layers.ron`.
- **`rl-math`, `rl-terrain` and `rl-sim` are `#![no_std]` + `alloc`, and build for `wasm32-unknown-unknown`.** No files, threads or clock in them.
- **Bit-exact means OLX's operation order, in `f32`** (and `f64` where OLX uses `double`):
  - no reordering, no `mul_add`, no constant folding that changes rounding;
  - `sqrt` via `libm`, trig via `rl_math::{sinf, cosf}`, never `std`;
  - integer truncation exactly where C++ truncates.
- **Cite OLX for everything ported** (`file:line` at commit `d1ad953f4f20610f2648cce26c9739e7c088409d`). Keep OLX quirks, and record them in `docs/porting/STATUS.md`.
- **`World::tick` is the only state change after setup, and it does not allocate.** `tests/no_alloc.rs` covers the projectile pool and the event buffer too.
- **Never panic on content.** Mods are untrusted: malformed projectile references, recursion cycles and absurd amounts are `SimModError`s at compile time, never panics at tick time.
- **Deterministic.** No `HashMap` iteration in logic. Every random draw comes from the world's generators in OLX's order.

## Decisions (spec amendment §18, written in Task 1)

| # | Decision | Reason |
|---|---|---|
| D1 | **Shot delivery, Model R.** Shots added during frames F and F+1 form one list, which is encoded, decoded and processed before frame F+2 (after `simulationTime` advanced past F+1), exactly as in real play at one frame per main-loop iteration (`weapons-firing.md` §1.1). | It reproduces the latency and the lossy multi-shot encoding a player gets. |
| D2 | **Wall clock pinned:** `tLX->currentTime = simulationTime + 10 ms` (K = 10) for the whole frame and for delivery. | Real play has `currentTime` in (T, T+10]. The hit-test position recorder (`get(0)` quirk), explosion damage, spawn times, respawn and bonus timers all read it. |
| D3 | **Client animation semantics.** The oracle loads projectile images in dedicated mode, so `ANI_ONCE` deletes projectiles when their animation ends and action-less animated projectiles live until then. `SimMod` carries each image projectile's frame count (`width / height`). | That is what a player's client does; Classic's Flamer and many MW projectiles use `ANI_ONCE`. |
| D4 | **Cosmetic `rand()` draws are emulated** (dig, rope hook, explosion dirt, blood, bleeding, death, gibs, colour), including the dedicated server's 1024-entry entity pool, which is never freed (a saturating counter reset at match start). | They advance the same generator as gameplay draws. Emulating the dedicated server's count is the only verifiable choice; a real client frees entities and simulates them with `rand()`, which no port can reproduce. Recorded as a deviation. |
| D5 | **Cosmetic effects get their own generator** in `rl-fx` (M5), never the simulation's. | Spec §5.5. |
| D6 | **`FindSpot`'s boost `mt19937` instances are reseeded** by `simSetSeed` in the oracle; the sim seeds its copy from the match seed. | Spawns and bonuses need a reproducible position; OLX seeds 5489 once per process. |
| D7 | **The `VecTimeRecorder::get(0)` quirk is kept.** | It is real-play behaviour; worms are hit at stale positions in OLX too. |
| D8 | **All worms in a scenario use scripted input** (no bot AI during recording). | Bot AI draws `rand()` and reads the wall clock. |
| D9 | **Exact-zero health:** the Gusanos `think()` death path is ported (the worm dies next frame, credited to nobody), but its Gusanos particles' `rnd()` use is not; scenarios avoid exact-zero health. | Rare; the Gusanos particle system is out of scope. Recorded as a deviation. |

## Review Focus

Conditions the spec implies that the weapon scenarios may not reach. Each is pinned by a named test in the task that owns the code.

1. **A full projectile pool (3000).** Spawning past capacity must drop projectiles exactly as OLX does, never panic or allocate. Pinned by `pool_full_drops_new_projectiles_like_olx` (Task 5).
2. **Malformed or hostile mods.** A projectile that spawns itself (directly or through a cycle) with `amount` 1000, or a missing projectile reference, must be a `SimModError` or bounded by the pool, never a hang. Pinned by `compiling_rejects_missing_and_bounds_cycles` (Task 4).
3. **A worm that dies while its shots are in flight, then respawns.** The shots are dropped at delivery as in OLX, and its old projectiles keep their owner id. Pinned by the `die-before-delivery` scenario (Task 3) and `shots_of_a_dead_worm_are_dropped` (Task 7).
4. **Explosions and carving at the map edge and in rock.** No out-of-bounds access; OLX's clipping quirks are kept. Pinned by `place_dirt_at_the_edges` and `green_dirt_at_the_edges` (Task 6).
5. **Hours-long matches.** Time and frame counters must not overflow or lose precision differently from OLX (`serverFrame` float round trip, `AbsTime` wrap). Pinned by `spawn_time_round_trip_matches_olx_at_large_frames` (Task 4).

## File structure after this plan

```text
crates/rl-math/src/rand.rs        GlibcRand, RANDOM_NUMBERS, get_random_num/pos_num/int, Mt19937
crates/rl-terrain/src/            + place_dirt, place_green_dirt (material grid), carve sizes
crates/rl-sim/src/mod_rt.rs       SimMod: ModDef compiled to indices (weapons, projectiles, frame counts)
crates/rl-sim/src/weapons.rs      slots, simulateWormWeapon, WormShoot, PlayerShoot, laser sight, specials
crates/rl-sim/src/shots.rs        shoot list, encode/decode round trip, Model R delivery, ProcessShot
crates/rl-sim/src/projectile.rs   CProjectile, pool (FastVector), spawn, Proj_SpawnInfo::apply
crates/rl-sim/src/proj_sim.rs     clocks, checkCollAndMove, worm collision, timers, trails
crates/rl-sim/src/actions.rs      events, Proj_Action::applyTo, execute, explosions, dirt
crates/rl-sim/src/beam.rs         ProcessShot_Beam, DrawBeam
crates/rl-sim/src/damage.rs       InjureWorm, kill, bleeding, cosmetic draws, entity counter
crates/rl-sim/src/respawn.rs      server respawn loop, FindSpot, Spawn
crates/rl-sim/src/bonus.rs        server and client bonuses
crates/rl-sim/src/events.rs       TickEvents
tools/olx-oracle/                 oracle v2 hooks
fixtures/oracle/                  + weapon, damage, bonus scenarios and traces
```

## After this plan

**Plan 5** covers presentation (M5): `rl-content`, `rl-scene`, `rl-render`, `rl-fx`, `rl-audio`, `rl-input` and a sandbox `rl-game`. It consumes the `TickEvents` and read-only accessors produced here.

---

### Task 1: Oracle v2 (deterministic shots, pinned clock, weapons, richer traces)

**Files:**
- Modify: `tools/olx-oracle/oracle-hooks.patch`, `tools/olx-oracle/oracle_control.py`, `tools/olx-oracle/CLAUDE.md`
- Modify: `crates/rl-oracle/src/{scenario.rs,trace.rs,compare.rs,lib.rs}`, `crates/rl-oracle/tests/fixtures.rs`
- Modify: `xtask/src/oracle.rs` (if the scenario JSON handed to Python changes)
- Modify: `fixtures/oracle/*.ron`, `*.jsonl.gz` (re-recorded), `crates/rl-scenario/src/lib.rs` (field mask)
- Modify: the spec (append §18 with D1–D9 from this plan), `docs/porting/STATUS.md`

**Interfaces:**
- Produces, in `rl-oracle`:
  - `Scenario` gains `#[serde(default)]` fields:
    - `weapons: Vec<WormWeapons>` (`worm: usize, names: Vec<String>, current: usize`);
    - `health: Vec<(usize, f32)>`;
    - `bonuses: Vec<BonusPlacement>` (`frame, x, y, kind: "health"|"weapon", weapon: Option<String>`);
    - `settings` is unchanged.
  - Trace additions, all optional on parse:
    - `Header`: `weapons: Vec<String>` (by OLX weapon id), `projectiles: Vec<String>` (by OLX projectile index, the name `rl-import-lgs` gives it), `options: BTreeMap<String,String>` (`Game.Blood`, `Game.Particles`, `DoProjectileSimulationInDedicated`, `K`), `seed`;
    - `Frame`: `sf` (serverFrame), `st` and `ct` (`simulationTime` and `tLX->currentTime`, ms since recording start), `rc` (cumulative `rand()` calls since the second `simSetSeed`), `mt` (cumulative `rnd()` draws), `ents` (entity pool size), `s: Vec<ShotRec>` (shots processed by the delivery hook, decoded: `w, wpn, x, y, vx, vy, ang, rnd, t, spawn`);
    - `WormRec`: `suicides`, `tod` (time of death, ms), `respawn` (bRespawnRequested, can-respawn flags), `blood` (fLastBlood, ms);
    - `ProjRec`: `own`, `rnd`, `ls`, `sp`, `ig`, `ws`, `tr`, `ox`, `oy`, `fr` (animation frame; it decides `ANI_ONCE` deletion), `col`;
    - `BonusRec`: client bonus `x, y, vx, vy, life, kind, wpn, used`, plus a `sb` list of server bonuses (`used, spawn, kind, wpn`).
  - `pub struct Fields(u32)` with named bits (`WORM_MOTION`, `SLOTS`, `ROPE`, `MATERIAL`, `RAND`, `SHOTS`, `PROJECTILES`, `PROJ_HIDDEN`, `HEALTH`, `SCORE`, `RESPAWN`, `BONUSES`, `CLOCKS`, `ALL`) and `compare_fields(expected, actual, tolerances, fields) -> Option<Divergence>`. `compare` stays and means `Fields::ALL`.
- Produces, in `rl-scenario`: `pub const CHECKED: Fields` (what the simulation reproduces so far). The acceptance test compares every scenario with `CHECKED`, and lists the scenarios it expects to diverge (empty at the end of the plan).

**OLX patch (`weapons-firing.md` §13, `projectiles.md` "What the oracle hooks need", `damage-death-bonuses.md` hooks section, `randomness.md` recommendations):**
- **Pinned clock (D2).** While a recording is armed, set `tLX->currentTime = simulationTime + 10ms` before `cClient->Frame()` and for delivery.
- **Delivery (D1).** Add `OracleHooks::betweenFrames()` after `simulationTime += frameDt`, implementing Model R with `writePacket`/`readSingle`/`readMulti`/`ProcessServerShotList`. While the hooks are active, `GameServer::SendUpdate` must not send shot lists. Record each processed shot in the next frame line's `s`.
- **Images (D3).** Load projectile images (`proj_t::bmpImage`) in dedicated mode, or at least their width and height for the frame count.
- **No bots (D8).** `simLoadInputs` installs the human handler on every worm. Update `fall.ron` with an explicit empty input.
- **MT reseeding (D6).** `simSetSeed` also reseeds `rnd`, `rndgen` and `midrnd` with the seed.
- **Counters.** Interpose `rand`/`srand` (count, then delegate to `random`/`srandom`, as glibc does) and count `rnd()` draws. Export the entity pool size; clear the pool and the server's last-bonus time when recording starts.
- **New commands:**
  - `simSetWeapons <id> "<n0>,<n1>,...,cur=<k>"` by weapon name (`FindWeaponId`), with `Charge 1`, `Reloading false`, `LastFire 0`;
  - `simSetHealth <id> <hp>`;
  - `simPlaceBonus <x> <y> <health|weapon> [name]`, spawning a bonus through the server path (`SpawnBonus` with a given spot and kind), so client and server both see it;
  - `oracle_control.py` issues them at the scenario's frames (weapons and health right after the spawns; bonuses through the input schedule's frame mechanism).
- **More recorded fields:** everything listed under Interfaces.

**Steps:**
- [ ] **Step 1: RED, the format.** In `rl-oracle`, add parsing tests for every new field (`parses_v2_fields`), a test that old traces still parse (`parses_v1_traces`), and `compare_fields_ignores_unchecked_fields` and `compare_fields_reports_first_checked_divergence`. Run `cargo nextest run -p rl-oracle`: FAIL (compile errors).
- [ ] **Step 2: GREEN, the format.** Implement the format; the tests pass.
- [ ] **Step 3: Patch.** Implement the patch in an OLX checkout at `d1ad953f` (the scratch `olx-dev` copy has the current patch applied), and regenerate `oracle-hooks.patch` as `tools/olx-oracle/CLAUDE.md` describes.
- [ ] **Step 4: Re-record.** Run `cargo xtask oracle-record --check`. Expected: the seven M3 traces differ only in added fields. If any existing field changes, investigate: the pinned clock must not change M3 movement. Then run `cargo xtask oracle-record` to rewrite them.
- [ ] **Step 5: Field mask.** Set `rl_scenario::CHECKED = WORM_MOTION | SLOTS | ROPE | MATERIAL | HEALTH | SCORE`. `cargo xtask check` must pass with the re-recorded traces.
- [ ] **Step 6: Proof scenario.** Add `fixtures/oracle/fire-minigun.ron`: Classic, Liero Factory, one worm with `simSetWeapons` Minigun, holding fire for 100 frames, 200 frames in total. Record it twice; the two traces must be byte-identical, and `p` must be non-empty from frame 3 or 4.
- [ ] **Step 7: Docs and commit.** Append §18 to the spec, update `tools/olx-oracle/CLAUDE.md` and STATUS, run `cargo xtask check`, commit (`olx-oracle: deterministic shots, pinned clock, weapon hooks`).

### Task 2: Randomness in `rl-math` and the world

**Files:**
- Create: `crates/rl-math/src/rand.rs`; Modify: `crates/rl-math/src/lib.rs`, `crates/rl-math/CLAUDE.md`
- Modify: `crates/rl-sim/src/{world.rs,physics.rs,rope.rs}` (cosmetic draws of M3 paths)
- Modify: `crates/rl-scenario/src/lib.rs` (`CHECKED |= RAND`, seeding)

**Interfaces:**
- Produces:
  - `rl_math::GlibcRand`: `new(seed: u32)`, `next() -> i32` (0..=RAND_MAX), `calls() -> u64`, `Clone + PartialEq + Debug`, no allocation (a `[i32; 34]` ring).
  - `rl_math::RANDOM_NUMBERS: [f32; 256]` and `get_fixed_random_num(i: u8) -> f32` (OLX `RandomNumberList.h`, `Misc.cpp:84`).
  - `GlibcRand` helpers `random_num() -> f32` (GetRandomNum), `random_pos_num() -> f32`, `random_int(max: i32) -> i32`, and `random_vec()`, with OLX's exact expressions (`randomness.md` §5).
  - `rl_math::Mt19937`: boost `mt19937`, `new(seed: u32)`, `next_u32()`, `calls() -> u64`.
  - `World` owns `rand: GlibcRand` and `mt: Mt19937`, seeded by `World::new(.., seed: u32)` (the scenario seed). `World::rand_calls()` and `mt_calls()` are accessors for traces.
- Consumes: `randomness.md` §1, §4, §5 (expected values).

**Steps:**
- [ ] **Step 1: RED.** Write the tests:
  - `glibc_rand_matches_glibc_2_35` (the first 10 outputs after `srand(1)` and `srand(12345)`, the 1000th and 1,000,000th, from `randomness.md` §4);
  - `float_helpers_match_olx` (the sample bits from §5, including `random_pos_num` returning exactly 1.0 at the top);
  - `mt19937_matches_boost` (seed 5489: the 10000th output is 4123659995, the standard value);
  - `fixed_table_first_and_last`.

  Run `cargo nextest run -p rl-math`: FAIL.
- [ ] **Step 2: GREEN.** Implement.
- [ ] **Step 3: Thread the generators through `World`.** Port the M3 paths' cosmetic draws (`randomness.md` §3.3):
  - digging: 6 draws if dirt was found;
  - the rope hook's first hit in dirt: 10;
  - bleeding is Task 7.

  Then `CHECKED |= RAND`. The seven M3 scenarios must still match (`cargo nextest run -p rl-scenario`).
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-math: glibc rand, fixed table, mt19937`).

### Task 3: Weapon, damage and bonus scenarios

**Files:**
- Create: `fixtures/oracle/weapons/<mod>/<weapon-slug>.ron` and `.jsonl.gz`; `fixtures/oracle/{die-before-delivery,beam-burst,multi-shot,two-worms-fire,explode-kill-respawn,bleed,bonus-health,bonus-weapon,reload-switch}.ron` and traces
- Create: `xtask/src/oracle_gen.rs` (`cargo xtask oracle-gen-weapons`: writes one scenario per weapon from the imported mods)
- Modify: `xtask/src/oracle.rs` (subdirectories), `crates/rl-scenario` (`fixture_names` walks subdirectories), `crates/rl-oracle/tests/fixtures.rs` (behaviour checks)

**Interfaces:**
- Produces:
  - **Per weapon of both mods,** one scenario with a firing worm and a dummy worm 80 px to its right on flat ground (level: `Dirt Level.lxl`; spawn spots chosen by the implementer and checked by eye in the first frame's material hash). The firer:
    1. aims straight up and fires for 0.5 s (open air);
    2. aims down into the dirt and fires for 0.5 s (terrain);
    3. aims level at the dummy and fires until the dummy dies or 2 s pass (the dummy worm);
    4. waits 1 s.

    About 450 frames. Charge-up weapons and beams get the same phases.
  - The named special scenarios cover:
    - the multi-shot quirk;
    - shots dropped when the firer dies before delivery;
    - the beam's `LastFire = 0.3` burst;
    - explosion → kill → automatic respawn (`RespawnTime`; frames pinned by D2);
    - bleeding below 15 hp;
    - health and weapon bonus pickup;
    - reloading, then switching weapon.
  - **Size budget:** all traces together under 40 MB. If over, shorten the phases, never drop a weapon.
- Consumes: Task 1's hooks and format.

**Steps:**
- [ ] **Step 1: Behaviour checks first.** In `crates/rl-oracle/tests/fixtures.rs`, add tests that the traces show what they claim:
  - every weapon scenario has projectiles or beam carving within 20 frames of the first fire;
  - the dummy loses health in at least 70% of the weapon scenarios;
  - `explode-kill-respawn` shows deaths +1 and a respawn;
  - the bonus scenarios show a pickup.

  These fail without traces.
- [ ] **Step 2: Generate and record.** Write the generator and the special scenarios, then record everything with `cargo xtask oracle-record`. Run the recording twice for 5 random weapons to confirm byte-identical output.
- [ ] **Step 3: Adjust.** Where a behaviour check fails because the scenario is badly placed (for example the dummy is out of reach), adjust the scenario, not the check.
- [ ] **Step 4: Expected divergences.** The acceptance test now lists every new scenario as expected to diverge (they do; there is no weapon code yet).
- [ ] **Step 5: Commit.** Run `cargo xtask check`, then commit (`fixtures: weapon, damage and bonus oracle scenarios`).

### Task 4: `SimMod`, slots and firing, the shot round trip, spawning

**Files:**
- Create: `crates/rl-sim/src/{mod_rt.rs,weapons.rs,shots.rs,projectile.rs}`
- Modify: `crates/rl-sim/src/{world.rs,physics.rs,worm.rs,lib.rs}`, `crates/rl-sim/Cargo.toml` (`rl-data` features as needed)
- Modify: `crates/rl-scenario/src/lib.rs` (build `SimMod` from the mod pack, name→id maps from the trace header, set weapons via `set_loadout`)

**Interfaces:**
- Produces:
  - `SimMod::compile(def: &ModDef, frame_counts: &BTreeMap<String, u32>) -> Result<SimMod, SimModError>`. The frame counts are each image's `width / height`, computed by the caller from the pack's images.
    - Weapons and projectiles are indexed; their order is given by `ModDef` (weapon list order = OLX weapon id; projectile order from the trace header via `SimMod::projectile_index(name)`).
    - Recursion is allowed (OLX spawns lazily), but a missing reference is an error.
  - `World::new(settings, sim_mod: Arc<SimMod>, terrain, holes, start, seed)`. `Arc` (alloc) keeps cloning cheap. Weapons in `Loadout` are weapon indices.
  - In `weapons.rs`, these run in each sub-step for each worm, in OLX's order:
    - `simulate_worm_weapon` (`PhysicsLX56.cpp:471-498`);
    - `worm_shoot` (`CServer_Game.cpp:366-466`);
    - `player_shoot` (`CClient_Game.cpp:496-544`, without the beam: Task 8).
  - In `shots.rs`:
    - `ShootList` (fixed capacity, no allocation per tick);
    - `encode_decode(&[Shot]) -> impl Iterator<Item = Shot>` (the lossy round trip, `weapons-firing.md` §4.4);
    - Model R delivery at the end of `tick`, after the frame clock advanced (D1);
    - `process_shot` (`CClient_Game.cpp:8xx`), with recoil.
  - In `projectile.rs`:
    - `Projectile` (all `CProjectile` state, `projectiles.md` §1.1);
    - `ProjectilePool` (capacity 3000 by default, lowest free slot, `FastVector` semantics, `projectiles.md` §2);
    - `spawn_info_apply` (`PhysicsLX56_Projectiles.cpp:562-640`) and `CProjectile::Spawn`.
  - Accessors: `World::projectiles() -> impl Iterator<Item = (usize, &Projectile)>` in slot order, and `World::last_shots() -> &[Shot]`.
- Consumes: Task 2's generators, Task 3's traces.

**Steps:**
- [ ] **Step 1: RED, unit tests.**
  - `compiling_rejects_missing_and_bounds_cycles` (Review Focus 2);
  - `slot_charge_drain_and_reload` (one ROF/reload cycle from hand-computed OLX values);
  - `shot_round_trip_truncates_like_olx` (positions, velocity, angle, multi-shot reuse of the first position and seed: `weapons-firing.md` §4.4);
  - `spawn_time_round_trip_matches_olx_at_large_frames` (Review Focus 5: `serverFrame` 819204 vs 819205);
  - `pool_takes_the_lowest_free_slot`;
  - `spawn_info_random_index_chain` (the `nRandom` chain, `weapons-firing.md` §5.2).

  Run `cargo nextest run -p rl-sim`: FAIL.
- [ ] **Step 2: GREEN.** Implement until the unit tests pass.
- [ ] **Step 3: Scenarios.** Set `CHECKED |= SLOTS | SHOTS` and compare all weapon scenarios with `rl-cli scenario`. The first divergence must be in a projectile field (`PROJECTILES` is still unchecked), never in `slots`, `s` or `rc`. Fix every slot, shot or rand divergence.
- [ ] **Step 4: Commit.** Run `cargo xtask check` (the no-alloc test includes firing), then commit (`rl-sim: weapons fire through OLX's shot round trip`).

### Task 5: Projectile motion, collision, timers, trails

**Files:**
- Create: `crates/rl-sim/src/proj_sim.rs`; Modify: `projectile.rs`, `world.rs`, `worm.rs` (the position recorder)

**Interfaces:**
- Produces:
  - **`simulate_projectiles`,** called in each sub-step after all worms (`CClient_Game.cpp:214-215`). For each projectile in slot order, with `lastUsed` read live:
    - catch up its own clock in 11 ms steps (`LX56_simulateProjectile`, `PhysicsLX56_Projectiles.cpp:1376-1410`);
    - `LX56ProjectileHandler_doFrame` (`:1330-1375`);
    - `simulate_low_level` (`:1234-1327`), including animation (D3 frame counts) and trails;
    - `check_coll_and_move` (`:367-491`), including the wallshoot window;
    - worm collision through the worm's `VecTimeRecorder` (100 entries, the `get(0)` quirk, D7; `projectiles.md` §3.6);
    - timer, hit and player-hit event detection (§4.1).
  - **Actions** are dispatched to `actions.rs`. This task implements Bounce (the LX56 order quirk, §3.7), Disappear, Nothing, GoThrough, Disappear2 and the speed and heading actions, and leaves Explode, Carve, Dirt, GreenDirt and the Injure* family as `todo!()`-free stubs that delete the projectile. Those stubs are temporary and replaced in Task 6.
  - `Worm` gains `pos_recorder: VecTimeRecorder`, pushed where OLX pushes it.
- Consumes: Task 4's pool and spawns.

**Steps:**
- [ ] **Step 1: RED.**
  - `pool_full_drops_new_projectiles_like_olx` (Review Focus 1);
  - `vec_time_recorder_get0_returns_the_oldest` (D7);
  - `bounce_left_right_overrides_top_bottom` (§3.7);
  - `wallshoot_window_keeps_old_pos_at_spawn`;
  - `children_in_higher_slots_run_in_the_same_pass` (§3.1/§4.4);
  - `ani_once_deletes_when_the_animation_ends` (D3).

  Run `cargo nextest run -p rl-sim`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Scenarios.** Set `CHECKED |= PROJECTILES | PROJ_HIDDEN`. Every weapon whose projectiles never explode, carve, place dirt or injure must match fully (list them in the commit message). The rest must first diverge at an explosion, carve, dirt or damage frame.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-sim: projectile motion, collision, timers, trails`).

### Task 6: Explosions, carving, dirt, green dirt, injury actions

**Files:**
- Create: `crates/rl-sim/src/actions.rs` (if Task 5 left it thin); Modify: `crates/rl-terrain/src/lib.rs` (+ `place_dirt`, `place_green_dirt`), `crates/rl-terrain/src/stamp.rs`
- Modify: content import if the green-ball image (`data/gfx/greenball.png`, 8×9) is not yet imported. It belongs to the theme pack: add it to `content/olx-manifest.ron` and `rl-import-gfx`'s theme import, with a corpus snapshot update.

**Interfaces:**
- Produces:
  - **Explosions and actions in `actions.rs`:**
    - `explosion(time, pos, damage, owner, weapon)`, `CClient_Game.cpp` §5.1: no falloff, d² ≤ 25 gets the full damage, hole `clamp((int)damage, 0, 4)`, 4 cosmetic draws;
    - Carve, Dirt (`projectile_doMakeDirt`, 3 × `PlaceDirt(5, ...)`), GreenDirt, Injure, InjureProj, InjureWorm and ChangeRadius, with the v7 fall-through quirks (`projectiles.md` §4.2–4.4);
    - `execute` in OLX order: children before the parent frees its slot.
  - **`rl-terrain`:**
    - `Terrain::place_dirt(stamp, x, y) -> u32` (the correct edge offset, unlike `carve_hole`; +2 for an edge-colour pixel over background);
    - `Terrain::place_green_dirt(stamp, x, y, rand: &mut impl FnMut() -> i32) -> u32` (one draw per green pixel placed).
  - Damage from actions calls `damage::injure_worm` (Task 7). Until Task 7 lands, health changes are applied directly.

**Steps:**
- [ ] **Step 1: RED.** Write `place_dirt_at_the_edges` and `green_dirt_at_the_edges` (Review Focus 4: corners and a 1×1 map, no panic, OLX's offsets), `explosion_has_no_falloff`, `explosion_hole_size_clamps` and `timer_explode_uses_playerhit_damage` (§4.4 quirk). Then run `cargo nextest run -p rl-terrain -p rl-sim`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Scenarios.** Set `CHECKED |= MATERIAL` (already set) and confirm `mh` and `dirt` match. Every weapon scenario must now match up to the first worm injury, or fully if the dummy is never hit.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-sim: explosions, carving and dirt`).

### Task 7: Damage, death, bleeding, respawn

**Files:**
- Create: `crates/rl-sim/src/{damage.rs,respawn.rs}`; Modify: `world.rs`, `worm.rs`, `physics.rs` (bleeding), `settings.rs` (`RespawnTime`, damage and shield factors, `AllowNegativeScore`, lives and kill limit from the trace header: `damage-death-bonuses.md` §7)

**Interfaces:**
- Produces:
  - `injure_worm(world, victim, damage, attacker)`, `CClient::InjureWorm` (`damage-death-bonuses.md` §2.1). It includes blood draws (7 × iterations) and score damage. Health below 0 kills; exactly 0 kills next frame without credit (D9).
  - `kill`: `GameServer::killWorm` → `CGameMode::Kill` → `CWorm::Kill`. It covers lives, kills, deaths, suicides (none counted at score 0 without `AllowNegativeScore`), and the death draws (sound 1, blood 350, gibs 7 × (2 or 4) by the entity counter, D4). Nothing else is reset (§2.5).
  - Bleeding below 15 hp (`PhysicsLX56.cpp:317-330`): 80 draws at most every 2 s.
  - **Respawn.** The server respawn loop (§3.1) after `RespawnTime`. `FindSpot` (§3.3) uses the world's `Mt19937`. `CWorm::Spawn` (§3.4) keeps the weapons and sets charge 1, not reloading, `LastFire` 0.
  - `EntityCounter`: saturating at 1024, counting every entity spawn OLX makes in dedicated mode (smoke trails, laser sight, explosions, gibs...).
- Consumes: Task 6 (action damage).

**Steps:**
- [ ] **Step 1: RED.**
  - `exact_zero_health_dies_next_frame_uncredited`;
  - `no_death_count_at_score_zero`;
  - `respawn_keeps_weapons_and_refills_them`;
  - `entity_counter_saturates_and_gates_gib_draws`;
  - `shots_of_a_dead_worm_are_dropped` (Review Focus 3).

  Run `cargo nextest run -p rl-sim`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Scenarios.** Set `CHECKED |= HEALTH | SCORE | RESPAWN | CLOCKS`. All weapon scenarios and `explode-kill-respawn`, `bleed` and `die-before-delivery` match fully.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-sim: damage, death, bleeding and respawn`).

### Task 8: Beams, laser sight, specials

**Files:**
- Create: `crates/rl-sim/src/beam.rs`; Modify: `weapons.rs`, `shots.rs`

**Interfaces:**
- Produces:
  - `process_shot_beam`: at delivery, step 1 px from the truncated position along the integer angle; carve the `Damage` hole (0..4) at the first dirt or rock; injure the first other worm within 5 px; set `LastFire = 0.3`; the weapon switch-back quirk.
  - `draw_beam`, which carves every sub-step while `LastFire > 0`.
  - `laser_sight` (an entity-counter tick only).
  - The jetpack quirks (no thrust if ROF > 0; a double drain otherwise), unit-tested only since no mod in scope has specials. See `weapons-firing.md` §6–7.

**Steps:**
- [ ] **Step 1: RED.** Write `beam_carves_first_solid_pixel`, `beam_sets_lastfire_burst` and `jetpack_quirks`. Run `cargo nextest run -p rl-sim`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Scenarios.** The beam weapons' scenarios (Classic Laser and others) and `beam-burst` match fully.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-sim: beams, laser sight and specials`).

### Task 9: Bonuses

**Files:**
- Create: `crates/rl-sim/src/bonus.rs`; Modify: `world.rs`, `settings.rs` (`Bonuses`, `BonusFrequency`, `BonusLife`, `BonusHealthToWeaponChance`, …)

**Interfaces:**
- Produces:
  - The server bonus array: spawn by frequency, `FindSpot`, lifetime; a slot stays busy for the full `BonusLife`.
  - The client bonus array: physics (`PhysicsLX56.cpp:618-715`), the spawn hole, pickup, the health effect (with its `rand()`), and the weapon effect (the slot replaced; the server re-applies one frame later).
  - Both arrays are at most 32 (`damage-death-bonuses.md` §4).
  - `World::place_bonus(x, y, kind)`, a setup-like command for scenarios and future modes (spec hook H1), applied at the next tick.
  - `World::bonuses()`.
- Consumes: Task 7 (respawn and the `FindSpot` MT).

**Steps:**
- [ ] **Step 1: RED.** Write `bonus_falls_and_rests_on_dirt`, `health_bonus_heals_with_olx_draw` and `server_slot_stays_busy_after_pickup`. Run `cargo nextest run -p rl-sim`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Scenarios.** Set `CHECKED |= BONUSES`. `bonus-health` and `bonus-weapon` match fully.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-sim: bonuses`).

### Task 10: Tick events, checksum, smoke matches

**Files:**
- Create: `crates/rl-sim/src/events.rs`, `crates/rl-sim/tests/smoke.rs`; Modify: `world.rs`, `lib.rs`

**Interfaces:**
- Produces:
  - `World::tick(&mut self, keys: &[Keys]) -> &TickEvents` (the signature change from `()`; callers updated).
  - `TickEvents` is a reused buffer of `Event` (spec §5.7): `Sound { id: SoundId, pos }`, `TerrainEdit { op: Carve|Dirt|GreenDirt|Beam, stamp: StampId, x, y }`, `Explosion { pos, damage }`, `SpawnCosmetic { kind, pos, vel }`, `WeaponFired { worm, weapon }`, `Damage { worm, amount, by }`, `Kill { victim, killer }`, `BonusPickup { worm, kind }`, `Spawned { worm }`.
    - Sound ids index `SimMod`'s sound table; it is built from the weapon, projectile and action `sound` names.
    - Events are emitted where OLX plays sounds or spawns entities, at no extra `rand()` cost (the draws already happen, D4).
  - `World::checksum() -> u64`: FNV-1a over all simulation state in a fixed order, with floats as bits.
  - `tests/smoke.rs`: 4-worm matches on every level × both mods, 3000 ticks each, random-but-seeded keys. Every tick checks:
    - no worm inside rock;
    - health ≤ 100;
    - pools within capacity;
    - no panics;
    - a clone at tick 1500 gives an identical checksum stream.

    It runs in the fast gate (under 10 s total; reduce the ticks if not).
- Consumes: everything above.

**Steps:**
- [ ] **Step 1: RED.** Write `events_are_emitted_once_per_cause` (one explosion → one `Explosion`, one `TerrainEdit`), `checksum_changes_with_state` and the smoke test. Run `cargo nextest run -p rl-sim`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: No allocation.** `tests/no_alloc.rs` adds firing every weapon of Classic for 500 ticks: 0 allocations.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-sim: tick events, checksum, smoke matches`).

### Task 11: Acceptance and documentation

**Files:**
- Modify: `crates/rl-scenario/tests/oracle.rs` (`CHECKED == Fields::ALL`, no expected divergences), `docs/porting/STATUS.md`, `crates/rl-sim/CLAUDE.md`, `docs/porting/olx-notes/*` (close the open questions answered by the port)

**Steps:**
- [ ] **Step 1: Full comparison.** Remove the expected-divergence list and compare with `Fields::ALL`. Run `cargo nextest run -p rl-scenario`. Every scenario must match, or carry a documented `Tolerance` whose `reason` proves bit-exactness impossible (expected: none).
- [ ] **Step 2: STATUS.** Add rows for weapons, shots, projectiles, actions, explosions, beams, damage, respawn and bonuses, with the scenarios that verify each. List D3, D4, D7 and D9 under deviations and kept quirks.
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-sim: every weapon of both mods matches OLX`).
