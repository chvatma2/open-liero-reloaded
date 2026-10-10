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
  (`DifficultyParams`, notes §1.9); `AimModel::{Ballistic, OlxDirect}` (F2).
- **A bot** (`bot.rs`): `Bot::new(worm, cfg, seed, &World)` (or
  `with_cache` to share an `AreaCache`), then `think(&World, &TickEvents)
  -> Keys` once per tick, before `World::tick`. Clocks are `World::time()`
  (every OLX `currentTime`/`GetTime()`). The brain is a placeholder that
  walks to `set_goal`'s point; plan 6 task 5 replaces it.
- **Work budget** (`budget.rs`, F7): one unit per terrain pixel read. Every
  primitive checks its worst case first (`Budget::afford`), so `think`
  never spends more than `Bot::budget()` (`units_used()` is the test hook);
  a line longer than a whole budget counts as blocked; `min_budget` raises
  the budget to what the level's longest line needs. Following a path gets
  at most half a `think`, the search the rest.
- **Navigation** (`nav/`, notes §1.4-1.5): `areas.rs` grows OLX's rock-free
  rectangles (`Grower`, resumable per pixel) into an `AreaCache` (atomics,
  `Arc`, append-only, fixed capacity; rock never changes in LX56);
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
  `ballistic_from` (the low root, from the muzzle), `AimState::set_aim`
  (0.3 s face cooldown, the difficulty's offset from the bot's own RNG).
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

Test: `cargo nextest run -p rl-bots`. Build for the web:
`cargo build -p rl-bots --target wasm32-unknown-unknown`.
