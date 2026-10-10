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
  the worms' lives; `Deathmatch::new` skips that. Kills are credited by
  `rl_sim::olx_kill_credit`, exactly as `OracleMode` does (the oracle relies
  on it). `is_over` is `CheckGameOver`, every frame (OLX's `recheckGame`
  stays set during a match): kill limit, then time limit
  (`serverFrame · 10 > TimeLimit`), then fewer than two worms not out.
- **Rules** (`rules.rs`) are pure: `compare` (`CompareWormsScore`, with
  `olx_round`), `highest`, `winner`, and the results order (a stable
  insertion sort, so ties keep the id order).
- **Messages** (`text.rs`): `TextId`s and OLX's default texts. Kill texts
  are queued in `on_kill`; the end's in the `on_tick` of the frame that
  ended (`is_over` cannot queue; `on_tick` checks again with the same
  worms and its own frame count, which equals `World::server_frame`).
- **Callbacks must not allocate** (`World::tick` runs them): the
  scoreboard has a capacity of `MAX_WORMS`, kept by the manual `Clone`.
  `checksum` hashes every field, the settings included.

Test: `cargo nextest run -p rl-modes` (unit tests on hand-made worms, and
`tests/deathmatch.rs` in a small arena with killing beams and a bomb).
