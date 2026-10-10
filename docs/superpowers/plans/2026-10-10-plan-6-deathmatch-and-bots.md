# Plan 6: Deathmatch, Bots, Replays, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Local Deathmatch against bots, end to end. This means:
- a game mode with OLX's scoring and game over;
- bots that move, rope, aim and fight like OLX's at four difficulties;
- OLX's weapon restrictions and loadouts;
- replays, plus the headless match runner with determinism, replay round-trip, coverage and golden checksums;
- `rl-game` playing a Deathmatch against N bots (milestone M6).

**Architecture:**
- **`rl-sim` gets the spec's match API.** `World::new(MatchSetup)` replaces the positional constructor. A `GameMode` trait gets callbacks and a command interface (spec §5.1, §5.8, hook H1). The world emits `Message`/`GameOver` events.
- **`rl-modes` (L3)** implements Deathmatch.
- **`rl-bots` (L3, `no_std`)** turns a read-only `&World` into `Keys` per tick, with its own RNG and work-unit budgets.
- **`rl-replay` (L3)** stores setup and per-tick keys.
- **`rl-cli match/replay`** runs headless matches and reports failures and coverage.
- **`rl-game`** gets a Deathmatch mode with bots and a minimal scoreboard and results overlay. Full menus are M7.

**Tech stack:** Rust 1.99, the existing crates. No new heavy dependencies; `rl-replay` uses `serde` and `ron` (or a compact binary through `postcard` if size demands, pinned exactly).

**Spec:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`. Read §5.1, §5.8, §5.9, §7.1–§7.4, §10 (M6), §12.1 and the amendments §15–§19. This plan adds §20 (F1–F10) in Task 1.

**Porting reference:** `docs/porting/olx-notes/bots-and-deathmatch.md`:
- the bot (§1);
- weapon selection and `.wps` (§2);
- Deathmatch rules (§3);
- the recommendations for `rl-bots` (§4), which this plan adopts;
- open questions (§5).

Also see `damage-death-bonuses.md` (kill, score helpers, respawn).

**How this plan was made:** like plans 4 and 5. Each task gives OLX references, interfaces, named tests and acceptance. Implementers work test-first.

## Global Constraints

- **Toolchain and license.** Rust 1.99, edition 2024, GPL-3.0-or-later. The crate prefix is `rl-`, and every crate has a `CLAUDE.md` and is registered in `xtask/layers.ron`.
- **L3 crates.** `rl-modes`, `rl-bots` and `rl-replay` are `#![no_std]` + `alloc`, build for `wasm32-unknown-unknown`, and depend on `rl-sim` as same-layer pairs listed in `layers.ron` (spec §4).
- **The simulation stays bit-exact.** All 106 oracle scenarios and the 3 D9 pins stay as they are. The API change must not change behaviour. The oracle uses a mode equivalent to today's hard-coded Deathmatch crediting.
- **`tick` stays allocation-free,** with modes and their callbacks included. Bots may allocate only at setup. Their per-tick work is bounded by a work-unit budget, not by time.
- **Determinism.**
  - Bots use their own seeded PRNG and never the world's generators.
  - No wall clock and no `HashMap` iteration.
  - Two bot matches with the same seed give identical key and checksum streams.
- **Never panic on content.** Hostile mods, levels, replays and settings give errors or bounded behaviour.
- **`cargo xtask check`** stays within about 60 s. Short bot matches go in `check`; the full grid goes in `full`.

## Decisions (spec amendment §20, written in Task 1)

| # | Decision | Reason |
|---|---|---|
| F1 | **Bot loadouts:** OLX's presets by default (with Classic at `LoadingTime` 100 and "Standard 100lt", the fixed 100lt set and tactics: notes §2.2). A setting gives random weapons through the ported `GetRandomWeapons` instead. | Faithful default, with variety on demand. |
| F2 | **Bot aim:** a gravity-aware ballistic solver by default (the low root, correct sign, rejected when impossible), with OLX's per-difficulty noise and tolerance. `AimModel::OlxDirect` is kept for comparison. | OLX computes the ballistic angle and then discards it, so its bots lob grenades badly. |
| F3 | **Bots press keys like humans** (the 8 `Keys`), through a motor that turns intents into keys under the sim's input rules. They get no mouse aim. | The same physics and constraints as a player; replays store keys only. |
| F4 | **Weapon restriction files** (`.wps`) are imported into mod packs: the raw `name → state` lists and the preset names. Random weapons, bot presets and natural bonuses (`World::set_bonus_weapons`) use them. | Needed for OLX's bot loadouts and bonus weapons; it closes the plan-4 deviation. |
| F5 | **Deathmatch needs ≥ 2 worms.** The sandbox stays for single-worm play. | OLX ends a one-worm local game at once. |
| F6 | **`World::new(MatchSetup)` and the `GameMode` trait live in `rl-sim`.** The callbacks are spawn, kill, damage, pickup, per-tick, is-over and scoring. The command interface: spawn a worm or bot with a loadout, give a weapon, apply a terrain stamp, emit a message, end the match. Deathmatch is in `rl-modes`. The world emits `Message` and `GameOver` events. | Spec §5.1, §5.8, H1; the final review of plan 4. |
| F7 | **Bots run once per 100 Hz tick.** Each owns a PRNG seeded from (match seed, worm, personality). Work is budgeted in units (terrain probes and traces), resumable across ticks. Free rectangles of the static rock are cached per level and shared. | Deterministic and bounded (notes §4.3). |
| F8 | **Replays store setup, pack ids, content hashes, settings, seed and every worm's keys per tick** (bots included), plus a checksum every N ticks. A format version is refused when unknown. | Bot code changes cannot break replays; mismatches are detected. |
| F9 | **Matches start at `T0 = 100 000 ms`,** as the oracle does. | Bleeding and bonus timers depend on absolute time (final review of plan 4). |
| F10 | **The chaos personality** changes loadout on respawn through the mode's "give weapon" command, ignores restrictions, and tracks fired weapons for coverage. | Spec §5.9 and §7.1 coverage. |

## Review Focus

1. **A bot stuck forever** (in a pit, under a rock overhang, with an empty or banned loadout) must not hang the match or blow its budget. The match still ends by limit. Pinned by `stuck_bots_stay_within_budget_and_match_ends` (Task 5).
2. **Score edge cases:** suicides at score 0 without `AllowNegativeScore`, simultaneous kills in one tick, the kill limit reached by two worms in one tick, a time-limit tie. These must match OLX's game-over order and winner rule (notes §3.3). Pinned by `deathmatch_game_over_and_winner_like_olx` (Task 2).
3. **A replay of a match against a different content hash or format version** must be refused with a clear error, never replayed into a silent divergence. Pinned by `replay_refuses_mismatched_content` (Task 6).
4. **A bot on a mod without weapons it can use** (all banned, beams only, specials only) must still move and not panic. Pinned by `bots_survive_odd_loadouts` (Task 5).
5. **Long matches:** a 30-minute bot match must not grow any per-tick cost, bot cache, replay buffer or event backlog beyond bounds. Pinned by `long_bot_match_bounded` (Task 7, `xtask full`).

## File structure after this plan

```text
crates/rl-sim/        MatchSetup, GameMode trait, mode commands, Message/GameOver events, settings (lives, limits)
crates/rl-modes/      L3  deathmatch.rs (scoring, game over, winner, results)
crates/rl-bots/       L3  nav/ (areas cache, resumable search, path follow), motor.rs, aim.rs, brain.rs, weapons.rs, chaos.rs, loadout.rs
crates/rl-replay/     L3  format, record, play, checksum stream
crates/rl-import-lgs/ + .wps restriction import (or rl-import-gfx, whichever owns mod directories)
crates/rl-cli/        + match, replay; failure capture; coverage report
crates/rl-game/       + Deathmatch vs bots, scoreboard and results overlay
xtask/                + bless-regression --reason; full grid
fixtures/matches/     golden checksum streams
```

## After this plan

**Plan 7** covers menus, the HUD, profiles, results, `--autoplay` and GUI/E2E tests: v0.1, a playable local Deathmatch against bots with menus.

---

### Task 1: Match API in `rl-sim` (`MatchSetup`, `GameMode`, commands, events)

**Files:**
- Modify: `crates/rl-sim/src/{world.rs,damage.rs,respawn.rs,settings.rs,events.rs,checksum.rs,...}`, `crates/rl-sim/src/mode.rs` (new), and every caller (`rl-scenario`, `rl-content`, `rl-cli`, `rl-game`, tests)
- Modify: the spec (append §20 with F1–F10), `docs/porting/STATUS.md`

**Interfaces:**
- Produces:
  - **`MatchSetup`:** settings, `Arc<SimMod>`, terrain, holes, start (default T0), seed, worms (`Vec<WormSetup { team, loadout, spawn: Option<(i32, i32)> }>`), `pool_capacity` (default 3000, spec §5.3) and `bonus_weapons`.
  - **Construction and validation:** `World::new(MatchSetup, mode: Box<dyn GameMode>) -> Result<World, SetupError>`, which validates the settings (final review of plan 4).
  - **`trait GameMode: Send`** (`no_std`; `Clone` via `box_clone` for `World: Clone`; `checksum(&self, &mut Fnv)`):
    - `on_spawn`;
    - `on_kill(victim, killer, ctx) -> KillCredit`;
    - `on_damage`;
    - `on_pickup`;
    - `on_tick(ctx: &mut ModeCtx)`;
    - `is_over(&World) -> Option<MatchResult>`;
    - `scores(&World) -> &[Score]`.
  - **`ModeCtx` commands (H1):** `spawn_worm(loadout, at)`, `give_weapon(worm, slot, weapon)`, `stamp(op, x, y)`, `message(text_id)`, `end(result)`. They are applied at defined points inside `tick`, in OLX order where OLX has an equivalent.
  - **`World::mode_state()`** for reading.
  - **Events:** `EventKind::Message`, `EventKind::GameOver(MatchResult)`.
  - **`OracleMode`** (in `rl-sim` or `rl-scenario`): reproduces today's hard-coded crediting exactly, so the oracle stays green.
  - **Other final-review items:**
    - canonical NaN in the checksum;
    - `place_bonus` returns a result and is pre-sized;
    - the `bWeaponsReady` note.
  - **Event data for rendering** (from the final review of plan 5): `CosmeticKind::LaserSight` gains `worm` and `CosmeticKind::Beam` gains its colour, so `rl-scene` stops guessing the owner (`marks.rs` `owner`/`beam_colour`). No draws change.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `oracle_mode_matches_the_hard_coded_credit` (the oracle acceptance test itself, unchanged);
  - `world_new_rejects_invalid_settings`;
  - `mode_commands_apply_inside_tick_deterministically`;
  - `give_weapon_mid_match_does_not_allocate` (no-alloc);
  - `checksum_canonicalises_nan`;
  - `pool_capacity_is_a_setting`.

  Run `cargo nextest run -p rl-sim`: FAIL.
- [ ] **Step 2: GREEN.** Implement, and update all callers. The oracle and smoke tests must pass unchanged.
- [ ] **Step 3: Commit.** Append spec §20, run `cargo xtask check`, then commit (`rl-sim: match setup, game modes and mode commands`).

### Task 2: `rl-modes` Deathmatch

**Files:**
- Create: `crates/rl-modes/` (L3)

**Interfaces:**
- Produces `Deathmatch::new(DeathmatchSettings { lives, kill_limit, time_limit, ... })`, implementing `GameMode`. It follows OLX exactly (notes §3):
  - scoring, including suicides and teamkills under `AllowNegativeScore`;
  - game over when a score reaches `KillLimit` (default 15), at `TimeLimit` (default 6 min), or when fewer than 2 worms have lives left;
  - the winner order: last not out, then lives (if limited), then not-out, then score, then damage, ties to the lower id;
  - results data: kills, deaths, suicides and damage per worm.
- It also emits OLX's kill and spree messages as `Message` text ids.
- Consumes: Task 1.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `deathmatch_game_over_and_winner_like_olx` (Review Focus 2, every case listed there);
  - `lives_out_removes_worm_from_respawn`;
  - `spree_messages`.

  If practical, record one OLX oracle scenario `dm-kill-limit` (`KillLimit` 2, two worms) and check `GameOver` on OLX's frame. If the oracle cannot observe game over, explain why in the report.

  Run `cargo nextest run -p rl-modes`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-modes: Deathmatch like OLX`).

### Task 3: Weapon restrictions and loadouts

**Files:**
- Modify: the mod importer (where mod directories are read: `rl-import-lgs` or a sibling), `rl-data` mod pack types (`restrictions: BTreeMap<String /*preset*/, BTreeMap<String /*weapon*/, WeaponState>>`), `rl-pack` (pack format bump if the layout changes, as the policy says), the corpus snapshot
- Create: `crates/rl-bots/src/loadout.rs` (or `rl-modes`, whichever the layers favour), containing `get_random_weapons` (OLX `CWorm::GetRandomWeapons`, notes §2.3) and the bot presets (notes §2.2)

**Interfaces:**
- Produces:
  - `ModDef.restrictions`, the parsed `.wps` files per mod (notes §2.4–2.5).
  - `fn enabled_weapons(def, preset) -> Vec<WeaponId>`.
  - `fn get_random_weapons(enabled, rng) -> [WeaponId; 5]`.
  - `fn choose_loadout(cfg: &BotConfig, setup) -> Loadout` (F1).
  - Natural bonuses use the preset's enabled set (`World::set_bonus_weapons`), which closes the deviation in STATUS.
  - A test re-records `bonus-natural` with OLX's default restriction file, or adds a new scenario `bonus-natural-default`, and it must match.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `wps_presets_enable_what_olx_enables` (the tables in notes §2.5 for Classic and MW 1.0);
  - `bot_presets_match_olx` (100lt, rifle, mortar);
  - `random_weapons_follow_restrictions`;
  - `natural_bonuses_use_default_restrictions` (oracle).

  Run `cargo nextest run -p rl-import-lgs -p rl-bots`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`Weapon restrictions and OLX loadouts`).

### Task 4: `rl-bots` core (navigation, motor, aim)

**Files:**
- Create: `crates/rl-bots/` (L3, `no_std`): `nav/areas.rs`, `nav/search.rs`, `nav/follow.rs`, `motor.rs`, `aim.rs`, `rng.rs`, `lib.rs`

**Interfaces:**
- Produces:
  - `BotConfig`, `Difficulty`, `DifficultyParams` (notes §4.5) and `AimModel`.
  - `Bot::new(worm, cfg, seed, &World)` and `Bot::think(&mut self, &World, &TickEvents) -> Keys`. In this task the brain is a simple "go to a target point" placeholder that Task 5 replaces.
  - `AreaCache`: maximal free rectangles of rock, built lazily under the budget and shared through `Arc` (F7; notes §1.4, §4.3).
  - A resumable A*-like search over areas, with OLX's 7 px passage rule, the 5 px checklist, costs, shortcutting and 50 px splitting.
  - `Motor`: intents to keys under `rl-sim`'s rules (notes §4.4: the aim controller, the face tap, the jump edge, CHANGE+arrow weapon switching, the carve combo, rope shoot and release).
  - `aim::ballistic(...)` and `aim::olx_direct(...)` (F2).
  - Work-unit accounting: a test hook reports the units used per `think`.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `areas_grow_like_olx` (synthetic terrains: corridors of width 6 vs 7, a U-shape, dirt plugs);
  - `search_finds_paths_and_resumes_across_ticks`;
  - `motor_reaches_target_angle_like_olx_turning` (about 100°/s);
  - `motor_switches_weapons_in_two_ticks_per_slot`;
  - `ballistic_hits_static_target` (simulate with `rl-sim`);
  - `think_stays_within_budget`;
  - `bots_are_deterministic` (same seed → same keys).

  Run `cargo nextest run -p rl-bots`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-bots: navigation, motor and aim`).

### Task 5: `rl-bots` brain, weapons, rope, chaos

**Files:**
- Create: `crates/rl-bots/src/{brain.rs,shoot.rs,weapons.rs,rope.rs,chaos.rs}`

**Interfaces:**
- Produces the OLX state machine and behaviours (notes §1.2–1.9, §4.2):
  - THINK ↔ MOVINGTOTARGET, re-thinking every 3 s, 0.5 s while a path is pending, at once on target death, and idle 0.4 s after spawn;
  - target choice and bonus seeking;
  - the fire decision;
  - weapon choice buckets, the change delay and reload behaviour;
  - the 100lt/rifle/mortar tactics;
  - rope spots and timers;
  - stuck handling.
- `Personality::Chaos` (F10, notes §4.6), using mode commands for loadout changes.
- Behaviour tests use real content (they skip with a notice without it).
- Consumes: Tasks 1–4.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `xtreme_bot_kills_static_target_quickly` and `easy_bot_hits_less` (Dirt Level);
  - `bots_use_the_rope_to_climb` (a scripted level spot);
  - `stuck_bots_stay_within_budget_and_match_ends` (Review Focus 1);
  - `bots_survive_odd_loadouts` (Review Focus 4);
  - `chaos_fires_every_weapon_over_a_match_grid`.

  Run `cargo nextest run -p rl-bots`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-bots: OLX's brain, weapons, rope and chaos`).

### Task 6: `rl-replay`

**Files:**
- Create: `crates/rl-replay/` (L3)

**Interfaces:**
- Produces:
  - `Replay { version, setup: ReplaySetup (pack ids + content hashes, settings, seed, worms), mode: ModeSpec, ticks: Vec<[Keys; N]> (compact), checksums: Vec<(u32 tick, u64)> }`.
  - `Recorder::new(&MatchSetup, &ModeSpec)`, `Recorder::push(&[Keys], &World)` (a checksum every 100 ticks) and `Recorder::finish() -> Replay`.
  - `Replay::play(&Content) -> Result<ReplayRun>`, which verifies checksums as it goes and reports the first mismatch tick.
  - `encode`/`decode` in a compact, versioned format.
- Consumes: Task 1.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `record_then_replay_gives_identical_checksums`;
  - `replay_refuses_mismatched_content` (Review Focus 3);
  - `decode_rejects_corrupt_input` (proptest: never panics);
  - `replay_size_is_reasonable` (one minute of 8 worms under a stated bound).

  Run `cargo nextest run -p rl-replay`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-replay: record and replay matches`).

### Task 7: `rl-cli match/replay`, failure capture, coverage, goldens

**Files:**
- Modify: `crates/rl-cli` (`match`, `replay` subcommands), `xtask` (`bless-regression --reason`, the grid in `full`)
- Create: `fixtures/matches/*.ron` (golden checksum streams), `fixtures/blessings.log` (shared with render)

**Interfaces:**
- Produces:
  - `rl-cli match --mod <id> --level <id> --mode deathmatch --bots N [--difficulty d] [--personality chaos|dm] --seed S --ticks T [--record f] [--coverage]`. It runs headless and checks invariants every tick (as the plan-4 smoke test does). It reports the end reason and the p99 tick time, and with `--coverage` lists weapons fired, action kinds triggered, bonuses, rope attachments and kills by weapon.
  - On failure it writes the replay and seed to `target/failures/` (spec §7.3).
  - `rl-cli replay <file> [--until N] [--dump]`.
  - In `check`: 2 short matches (8 bots, 3000 ticks, Classic and MW on different levels), with determinism and replay round-trip.
  - In `full`: the grid (both mods × 4 levels × 3 seeds, chaos and Deathmatch bots). Coverage must include every weapon of both mods and every action kind they use.
  - `long_bot_match_bounded` (Review Focus 5).
  - Golden checksum streams for 3 fixed-seed matches, with `cargo xtask bless-regression --reason` (spec §7.2).
- Consumes: Tasks 1–6.

**Steps:**
- [ ] **Step 1: RED.** Write the `check` matches, `failure_writes_replay`, `coverage_report_lists_every_weapon` (the chaos grid, in `full`) and `golden_match_checksums`. Run `cargo nextest run -p rl-cli`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check` and `cargo xtask full`, then commit (`rl-cli: headless bot matches, replays, coverage`).

### Task 8: Deathmatch vs bots in `rl-game`

**Files:**
- Modify: `crates/rl-game` (a new mode next to the sandbox)

**Interfaces:**
- Produces:
  - `rl-game --mode deathmatch --bots N [--difficulty easy|medium|hard|xtreme] [--lives L] [--kill-limit K] [--time-limit M] [--level] [--mod] [--seed] [--record <file>]`.
    - The human is worm 0, and bots think each tick on the main thread within budget.
    - A minimal egui scoreboard (Tab) and a kill feed from `Message`/`Kill` events.
    - On `GameOver`, a results overlay (winner, kills, deaths, suicides). F5 starts a rematch and Esc quits.
  - `rl-game --replay <file>` plays a replay with the normal renderer.
- Consumes: Tasks 1–7.

**Steps:**
- [ ] **Step 1: RED.** Write the pure-part tests: `deathmatch_session_ends_with_results`, `bots_think_within_frame_budget` and `replay_session_matches_recorded_checksums`. Run `cargo nextest run -p rl-game`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Scripted runs.** Using the R3 approach from plan 5:
  - 4 levels × 2 mods, Deathmatch against 4 Medium bots for 60 s each;
  - inspect screenshots;
  - report frame and tick times, bot think times, and kills by bots.
- [ ] **Step 4: Commit.** Run `cargo xtask check`, then commit (`rl-game: Deathmatch against bots`).

### Task 9: Acceptance and docs

**Steps:**
- [ ] **Step 1: Gates.** `cargo xtask check` and `cargo xtask full` (with the grid) pass, and coverage includes every weapon of both mods.
- [ ] **Step 2: Docs.**
  - **STATUS:** rows for modes, bots, replays and the match runner, and F-decisions as deviations.
  - **README:** how to play Deathmatch against bots, replays and `rl-cli match`.
  - The crate `CLAUDE.md`s.
- [ ] **Step 3: Commit** (`Deathmatch against bots: M6`).
