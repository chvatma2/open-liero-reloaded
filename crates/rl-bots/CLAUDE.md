# rl-bots

OLX's bots for `rl-sim` matches (spec §5.9, §20 F1-F3, F7, F10), a port of
`CWormBotInputHandler` (`src/common/CWormBot.cpp`) at commit `d1ad953f`.
Notes: `docs/porting/olx-notes/bots-and-deathmatch.md` (§1 the bot, §2
loadouts, §4 the recommendations this crate follows).

- **Layer 3, `#![no_std]` + `alloc`, wasm32.** Depends on `rl-sim` (a
  same-layer pair in `xtask/layers.ron`), `rl-data` and `rl-math`. Learn
  their APIs from `target/api/rl_sim.txt` and `rl_data.txt`.
- **Determinism:** a bot draws only from its own `BotRng` (`rng.rs`, a
  glibc `rand()` stream seeded from the match seed, the worm and the
  personality), never from the world's generators; OLX's `GetRandomInt`,
  `GetRandomLong` and `randomChoiceFrom` are ported on top of it.
- **Configuration** (`config.rs`): `BotConfig { personality, difficulty,
  loadout }`; plan 6 task 4 adds the difficulty's numbers, the aim model and
  the work budget.
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
