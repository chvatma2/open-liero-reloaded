# rl-bots

OLX's bots for `rl-sim` matches (spec §5.9, §20 F1-F3, F7, F10), a port of
`CWormBotInputHandler` (`src/common/CWormBot.cpp`) at commit `d1ad953f`.
Notes: `docs/porting/olx-notes/bots-and-deathmatch.md` (§1 the bot, §2
loadouts, §4 the recommendations this crate follows).

- **Layer 3, `#![no_std]` + `alloc`, wasm32.** Depends on `rl-sim` (a
  same-layer pair in `xtask/layers.ron`), `rl-data`, `rl-terrain`,
  `rl-math` and `libm`. Learn their APIs from `target/api/rl_sim.txt`,
  `rl_terrain.txt` and `rl_data.txt`.
- **Determinism:** a bot draws only from its own `BotRng` (`rng.rs`, a
  glibc `rand()` stream seeded from the match seed, the worm and the
  personality), never from the world's generators; OLX's `GetRandomInt`,
  `GetRandomLong` and `randomChoiceFrom` are ported on top of it.
- **Configuration** (`config.rs`): `BotConfig { personality, difficulty,
  loadout, params, aim, budget }`; `Difficulty::params` gives OLX's numbers
  (`DifficultyParams`, notes §1.9), except that the aim errors scale with at
  least `AIM_ERROR_FLOOR` (1/6): no difficulty aims perfectly, Xtreme
  included (a deviation, `docs/porting/STATUS.md`);
  `AimModel::{Ballistic, OlxDirect}` (F2). `BotConfig::validate` refuses
  params that are NaN, infinite or negative (`BotConfigError` names the
  field); `rl_content::build_match` checks every bot with it. No serde here:
  `rl-ui` persists its own mirror of the menu's choices (plan 7 task 1).
- **A bot** (`bot.rs`): `Bot::new(worm, cfg, seed, &World)` (or
  `with_cache` to share an `AreaCache`), then `think(&World, &TickEvents)
  -> Keys` once per tick, before `World::tick`. Clocks are `World::time()`
  (every OLX `currentTime`/`GetTime()`); timers count 10 ms a tick. Methods
  of `Bot` live in the module of their OLX function, on a `Ctx` (the world
  and the worm before the tick) and an `Act` (OLX's `worm_state_t`, aim and
  rope), which `think` turns into an `Intent` for the motor.
- **The brain** (`brain.rs`, notes §1.2-1.3): `getInput` (THINK ↔
  MOVINGTOTARGET, re-thinks, spawn pause, carve), `AI_Think` (targets,
  bonuses, high spots), `AI_CreatePath`, `AI_SetAim`; `set_goal` races to
  a point (no shooting). `shoot.rs`: `AI_Shoot` and `weaponCanHit` (the
  real arc for the ballistic aim, OLX's parabola for `OlxDirect`).
  `weapons.rs`: what the bot knows of each weapon (`Arsenal`), the game
  type from the loadout, `AI_GetBestWeapon` (buckets, change delay, 100lt,
  rifle and mortar tactics), reloading, clearing weapons. `moving.rs`:
  `AI_MoveToTarget` (the shooting spot is searched after the node advance
  and its jump check), `AI_SimpleMove`, `AI_Jump`, stuck handling, and the
  dig-on deviation (through dirt only). `rope.rs`: rope timers and `AI_GetBestRopeSpot`;
  `spot.rs`: `AI_FindShootingSpot` (both spread over ticks under the
  budget). `probe.rs`: the budgeted terrain probes (`traceLine`,
  `traceWeaponLine`, `AI_GetRockBetween`, area checks).
- **Chaos** (`chaos.rs`, F10, notes §4.6): fires whatever is ready,
  prefers weapons it has not fired, moves at random, records what it fired
  (`Bot::fired_weapons`); its loadout rotates by the mode
  (`rl_modes::RespawnLoadout::ChaosCycle`).
- **Work budget** (`budget.rs`, F7): one unit per terrain pixel read. Every
  primitive checks its worst case first (`Budget::afford`), so `think`
  never spends more than `Bot::budget()` (`units_used()` is the test hook);
  a line longer than a whole budget counts as blocked; `min_budget` raises
  the budget to what the level's longest line needs. Shooting gets at
  most a quarter of a `think`, moving the rest of half (the node advance
  keeps room for the jump check), the search the rest.
- **Navigation** (`nav/`, notes §1.4-1.5): `areas.rs` grows OLX's rock-free
  rectangles (`Grower`, resumable per pixel) into an `AreaCache` (atomics,
  `Arc`, append-only, fixed capacity, keyed by the level's `rock_hash`;
  rock never changes in LX56);
  `search.rs` is `searchpath_base` as a resumable state machine (checklist,
  7-trace passage rule, costs, worm-line goal test, `simplifyPath`);
  `path.rs` the 50 px splitting; `follow.rs` the node advance (8 nodes
  ahead) and the jump check; `trace.rs` `simpleTraceLine`,
  `fastTraceLine`, `traceWormLine`. Nothing allocates after `new`.
- **Motor** (`motor.rs`, F3, notes §4.4): `Intent` to the 8 keys under
  `rl-sim`'s input rules (aim speed controller ~100°/s, face tap, jump and
  rope press edges, CHANGE+arrow weapon switching, carve combo).
- **Aim** (`aim.rs`, F2): `olx_direct` (`AI_SetAim`'s angle),
  `olx_aiming_angle` (OLX's solver, quirks kept), `ballistic` and
  `ballistic_from` (the low root, from the muzzle, with the velocity a
  `Launch` carries), `AimState::set_aim` (0.3 s face cooldown, the
  difficulty's offset from the bot's own RNG), `ShotNoise`
  (`weaponCanHit`'s gravity, speed and target noise).
- **Loadouts** (`loadout.rs`, F1): `olx_bot_weapons` is `initWeaponSelection`
  (rifles, 100lt and mortars by mod name, `LoadingTime` and restrictions,
  else random; empty slots filled by `getRandomEnabledWpn`),
  `get_random_weapons` is `CWorm::GetRandomWeapons` (one draw per slot),
  `choose_loadout` picks by personality and `LoadoutPolicy`. Restrictions
  come as `rl_data::restrictions::WeaponRules` (OLX's `CWpnRest` after
  `updateList`), built from the mod pack's presets (`rl-content`:
  `LoadedMod::weapon_rules`).
- **Port, don't improve** where OLX's choices define how its bots feel; cite
  the OLX source on each function.
- **Future: bot skill** (the user's direction; `docs/porting/STATUS.md`,
  "Spec §20 decisions"): difficulty should come from strategy, quick
  reactions and predictive reloading (teamwork with team modes), never from
  perfect aim. That redesign is a later milestone, measured against OLX with
  the baseline harness (`docs/porting/bot-baseline-2026-10-11.md`); until
  then keep OLX's brain and do not lower `AIM_ERROR_FLOOR` to zero.

Test: `cargo nextest run -p rl-bots` (`brain.rs`, `rope.rs`, `chaos.rs`
and `matches.rs` play OLX's content from `RL_CONTENT_DIR` and skip without
it). Build for the web:
`cargo build -p rl-bots --target wasm32-unknown-unknown`.
