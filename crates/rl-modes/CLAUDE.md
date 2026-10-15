# rl-modes

Game modes for `rl-sim` matches (spec §5.8, §20 F6): OLX's Deathmatch, a
port of the bare `CGameMode` (`src/server/CGameMode.cpp`) at commit
`d1ad953f`. Notes: `docs/porting/olx-notes/bots-and-deathmatch.md` §3.

- **Layer 3, `#![no_std]` + `alloc`, wasm32.** Depends on `rl-sim` only (a
  same-layer pair in `xtask/layers.ron`). Learn the match API from
  `target/api/rl_sim.txt` (`GameMode`, `ModeCtx`, `MatchSetup`).
- **`Deathmatch`** (`deathmatch.rs`): `Deathmatch::prepare(settings, &mut
  MatchSetup)` checks two worms (F5) and copies the rules the simulation
  applies (lives, kill limit, scoring) into the match's `SimSettings` and
  the worms' lives. `Deathmatch::resume(settings, &SimSettings)` is for a
  setup made that way before (a replay, an oracle trace): it refuses
  settings that disagree with the rules (`DeathmatchError::Disagrees`).
  These are the only constructors (one source of rules; `new` is private).
  `time_left_ms` stops at the game-over frame. Kills are credited by
  `rl_sim::olx_kill_credit`, exactly as `OracleMode` does (the oracle relies
  on it). `is_over` is `CheckGameOver`, every frame (OLX's `recheckGame`
  stays set during a match): kill limit, then time limit
  (`serverFrame · 10 > TimeLimit`), then fewer than two worms not out.
- **Leaving players:** `on_drop` is `CGameMode::Drop`
  (`CGameMode.cpp:254-263`): `drop_worm` zeroes the worm's kills and
  deaths in a row, then the board is refreshed. `refresh` takes the gone
  mask (`ModeCtx::gone`) and marks those rows `Score::left`; the checksum
  hashes `left` only when true. `check` takes the mask too: a gone worm
  is `WRM_OUT` (so out of the last-standing count), and `rules::in_game`
  leaves it out of the kill-limit check and `winner`'s `highest` (OLX's
  `HighestScoredWorm` no longer sees a removed worm), so it never wins.
  `on_drop` gives no commands (`CommandError::Closed`).
- **Respawn loadouts** (`respawn.rs`, spec §20 F10):
  `DeathmatchSettings::respawn_loadout` is `Keep` (OLX) or `ChaosCycle`:
  `on_spawn` gives chaos bot `i` of `n` the weapons `(i·5 + r·5·n + j) mod
  W` on its life `r` with `give_weapon` (the mode counts each worm's
  spawns, the first being life 0). `ChaosCycle::worms` and `weapon_count`
  give back what it was made from, for replays.
- **Rules** (`rules.rs`) are pure: `compare` (`CompareWormsScore`, with
  `olx_round`), `highest`, `winner`, and the results order (a stable
  insertion sort, so ties keep the id order).
- **Messages** (`text.rs`): `TextId`s and OLX's default texts. Kill texts
  are queued in `on_kill`; the end's in the `on_tick` of the frame that
  ended (`is_over` cannot queue; `on_tick` checks again with the same
  worms and its own frame count, which equals `World::server_frame`).
- Deathmatch transfers its dynamic fields (`save_state`/`load_state`):
  every field but the settings, postcard; a board longer than `MAX_WORMS`
  or naming a worm outside it is refused.
- **Callbacks must not allocate** (`World::tick` runs them): the
  scoreboard has a capacity of `MAX_WORMS`, kept by the manual `Clone`.
  `checksum` hashes every field, the settings included.

Test: `cargo nextest run -p rl-modes` (unit tests on hand-made worms, and
`tests/deathmatch.rs` and `tests/chaos.rs` in a small arena with killing
beams and a bomb).
