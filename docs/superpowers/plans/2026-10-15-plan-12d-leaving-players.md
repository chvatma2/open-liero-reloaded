# Plan 12d: Leaving Players Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In an online match, a player who leaves (or times out, or is kicked) is removed at once on every peer: the worm leaves play, comes off the live scoreboard, keeps a "left" row in the final results, and the log says "<name> has left" ("<name> has timed out"), as in OpenLieroX. Replays reproduce it, and host migration keeps working through it.

**The playtest report this answers:** "When a player left the game, they remained in game and did not disappear immediately." Today the host fills a leaver's slot with `Keys(0)` until the match ends (decision N6), so the worm stands still, can be shot for kills, and stays on the scoreboard.

**Architecture:**
- **`rl-sim`** gets `World::withdraw(worm)`. It takes the worm out of play for good: dead, out of lives, rope cleared, never respawned. A gone mask (`World::gone()`, kept in the referee) records the worms withdrawn.
  - The mode is told through `GameMode::on_drop` (OLX's `CGameMode::Drop`).
  - A gone worm's later kills credit nobody.
  - The mask is hashed only when it is set, so no local match changes.
- **`rl-modes`**: Deathmatch resets the leaver's streaks and marks its scoreboard row `Score::left`. OLX's "fewer than two worms" game over then follows on its own, because a gone worm is out of lives.
- **`rl-replay`** records each withdrawal (`Replay::withdrawals`) and replays it before its tick (format version 3).
- **`rl-proto` + `rl-netcode`**: the host decides the tick. When it abandons a player's slot, the slot's first unknown tick T becomes its withdrawal tick, stored in the input log with the slot's value at T.
  - **How the mark travels (ruling PF12d-4).** Every message that carries final inputs carries, once, a compact `(slot, tick)` list of the marks whose tick lies within its frames:
    - the host's datagrams (`HostInputs::gone`);
    - a migration's report (`InputsReport::gone`);
    - its resumes (`Control::Resume`, `HostResume`).

    A peer stores the value at T together with the mark, so it cannot confirm tick T without it. `Frame` and the frame codec do not change.
  - **Withdrawing:** the session withdraws the worm before simulating T, and rolls back when it learns of a withdrawal it predicted past.
  - **The harness** checks every peer withdrew alike, and its offline replay withdraws too.
- **`rl-online`**:
  - The match record and its replay carry the withdrawals.
  - A timeout is told apart from a leave (`Notice::TimedOut`, `OnlineEvent::TimedOut`).
- **`rl-lobby`**: `LobbyHost::timed_out`.
- **`rl-ui` / `rl-game`**:
  - Texts: "<name> has left" and "<name> has timed out".
  - The live scoreboard leaves gone worms out; the results keep a "left" row.
  - A watcher's camera moves on from a worm that left.
- **`rl-cli`**:
  - The net-sim desync bundle carries the withdrawals, and `net-repro` withdraws.
  - The stall verdict excuses a client crash's 10 s of host fills (ruling PF12d-5).
  - net-soak cases include client departures.

**Tech Stack:** Rust 1.99 (edition 2024). Existing crates only (serde, postcard, proptest, egui_kittest). **No new dependency** and no new system package.

**Spec:** `docs/superpowers/specs/2026-10-12-networking-design.md`:
- §3.2: what the host forwards for (player, tick) is final.
- §3.3: rollback.
- §3.7: host migration.
- §7.2: "Kill messages, joins and leaves appear in the HUD log as in OLX."
- §9.1: every peer's confirmed checksums equal an offline replay of the final inputs.

This plan settles what decision N6 left open: "what happens to the worm (removal, a bot taking over) is decided with the lobby in plan 10" (`docs/porting/STATUS.md`). Plan 10 did not decide it. The user decided it on 2026-10-15:
1. Remove the worm; no bot takes it over.
2. Take it off the live scoreboard, but keep a "left" row in the final results with its kills and deaths so far. Record this as a deviation from OLX.
3. If fewer than two worms remain, the match ends at once (OLX's rule).
4. Keep the 10 s crash timeout: the worm repeats its last keys until then, and is removed when the timeout fires.
5. Slots abandoned by a migration are removed at the resume tick too.

**OLX reference (commit `d1ad953f`):**
- `GameServer::DropClient` (`src/server/CServer.cpp:1352-1434`) says `sHasLeft` ("<player> has left") for a quit and `sHasTimedOut` ("<player> has timed out") for a timeout. The texts are in `src/client/Options.cpp:769-770`. It then removes the client's worms.
- `GameServer::RemoveClientWorms` (`CServer.cpp:1440-1482`) calls `CGameMode::Drop` (`src/server/CGameMode.cpp:254-263`: resets the worm's kills and deaths in a row), then `Game::removeWorm` (`src/game/Game.cpp:1152-1160`, `1359-1366`), then `RecheckGame`.
- The game-over check after `RecheckGame` is `CGameMode::CheckGameOver` (`CGameMode.cpp:276-358`): fewer than two worms not out ends the match (`:316-336`).
- Clients remove the worm on `S2C_WORMSOUT` (`src/client/CClient_Parse.cpp:1650-1680`).
- A kill by a worm already removed credits nobody: `GameServer::killWorm` looks the killer up with `game.wormById(killer, false)`, which is null for a removed worm (`src/server/CServer_Game.cpp:126-127`), and `CGameMode::Kill` credits only a non-null killer (`CGameMode.cpp:69-109`).

## Global Constraints

- **Determinism (spec §9.1):**
  - Every peer withdraws a worm at the same tick, and a rollback replays the withdrawal identically.
  - The gone mask lives on `World` (in its referee), never on `Worm`: `Worm`'s 992-byte size assertion (`crates/rl-sim/src/worm.rs`) stays as it is.
  - The mask is hashed into `World::checksum` only when it is not 0. `Score::left` is hashed into the Deathmatch's checksum only when it is true.
  - `BTreeMap`/`BTreeSet` only in anything that orders output. No wall clock in logic.
- **Local play, match goldens, oracle and render goldens do not change.**
  - No local path calls `World::withdraw`: `cargo xtask check`'s match goldens (`fixtures/matches/*.ron`), the oracle (`fixtures/oracle`) and the render and UI goldens pass unchanged.
  - No `bless-regression` or `bless-render` in this plan.
  - Every task's check runs them.
- **Compatibility is broken on purpose** (user rule, 2026-10-15: proof-of-concept stage). No shim, and no old format kept readable:
  - **`rl_proto::FINGERPRINT`** changes. It hashes the sources of rl-math, rl-data, rl-terrain, rl-sim, rl-modes, rl-netcode and rl-proto (`crates/rl-proto/build.rs`), so released builds refuse this one at the offer.
  - **The wire:** `HostInputs`, `HostResume`, `InputsReport` and `Control::Resume` gain a `gone` list.
  - **`rl_replay::FORMAT_VERSION`** goes 2 → 3.
  - **rl-online's `GAME_BUNDLE_VERSION`** goes 1 → 2.
  - **rl-cli's `BUNDLE_VERSION`** goes 2 → 3.
  - **`PROTOCOL_VERSION` stays 2:** the fingerprint check at the same offer already refuses another build.
  - No committed fixture holds a replay or a bundle. `fixtures/net/cases/*.ron` are specs that run again, so nothing is re-recorded.
- **Nothing in `cargo xtask check` or any test touches the public internet or the machine's real router.** Ruling NT-1: tests send only to 127.0.0.0/8.
  - rl-online's tests use `tests/common/net.rs` (port 0 on 127.0.0.1, `MigrationConfig::direct()` with `base_override` 127.0.0.1).
  - rl-game's tests use `Shell::test_ice` and no relay, STUN or port mapping, as its `tests/online.rs` does.
- **Layers (`xtask/layers.ron`) unchanged:** no new crate and no new pair.
  - `rl-proto`, `rl-netcode`, `rl-lobby`, `rl-sim`, `rl-modes` and `rl-replay` stay `#![no_std]` + `alloc` and build for `wasm32-unknown-unknown`.
  - One integration-test binary per crate: a new test file is a `mod` of `tests/integration.rs`.
- **Never panic on network or file input:**
  - A `gone` list is refused by the decoders (`ProtoError::Invalid`) when it has:
    - more than `MAX_SLOTS` entries;
    - a slot twice;
    - a slot past the match's;
    - a tick outside its message's frames.
  - A second withdrawal mark for a slot is ignored.
  - A mark for a tick already final without it is a conflict:
    - at a client (`take_frames`), `Event::Inconsistent`;
    - at a new host's merge, counted in `Stats::report_conflicts`.
  - A world state whose gone mask names no worm, or a living worm, is refused (`StateError::Worms`).
  - A replay's withdrawals are bounded and checked on decode.
  - The fuzzers in `crates/rl-proto/tests/fuzz.rs` cover the new fields.
- **Cite OLX** for ported behaviour, in doc comments, at `d1ad953f` (the references above). Faithfulness beats elegance. The one deliberate deviation (the results' "left" row) goes in `docs/porting/STATUS.md`.
- **Tests must be able to fail, and drive the real path:**
  - Each task's first step writes a test that fails before its change (the expected failure is named).
  - Online tests go through `OnlinePeer::leave`, the session and the real `WorldSim`, and through the shell's `UiAction`s, never a helper that sets the world directly. The sim and mode unit tests are the exception: there, `World::withdraw` *is* the path.
  - Online tests bound ticks by state read from the peers (an input log's `known_upto`, a record's marks), never by a tick read off the real clock.
- **Commits:**
  - Run `cargo xtask check` before every commit (`--offline` without network), chained with `&&`. Redirect its output to a file and read the tail. Never put `| tail` before the commit.
  - Stage files explicitly (`git add <paths>`, never `-A`). Commit on `main`, never push.
  - **`cargo xtask full` runs once, in Task 8, never mid-plan** (user rule, 2026-10-15). Run nothing else from `full` mid-plan either: no `net-soak`, `e2e`, `net-feel` or `net-nat` before Task 8.
  - Trailers:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```
- **Start from HEAD.** Plan 12c is finished (HEAD `6a3b273` when this plan was checked). Read the current code, not line numbers: line numbers below are hints only.

## Decisions this plan makes (recorded as STATUS.md rulings in Task 8)

The user's five decisions are above. These settle the rest. PF12d-1 to PF12d-5 are the controller's pre-flight rulings (2026-10-15).

- **N82 – withdrawal, not deletion.**
  - `World::withdraw(i)` keeps worm `i` in `World::worms()`: slot `i` stays worm `i`, which the netcode, replays and the presentation index by. The worm is made dead, `lives = WRM_OUT`, its rope cleared, its latches reset and its state cleared.
  - Every gameplay path already ignores a dead `WRM_OUT` worm: physics, hits, beams, rope attachment, bonuses, respawns, bots, drawing, the minimap and the names.
  - OLX deletes the worm (`Game::removeWorm`).
  - **Cost if wrong:** a future reader of `World::worms()` must skip gone worms (`World::gone()`), as it already skips dead ones.
- **N83 – the withdrawal tick and how it travels** (ruling PF12d-4).
  - **Which tick:** when the host abandons a player's slot, the slot's first unknown tick T is its withdrawal tick. Abandoning means `Session::peer_lost`: a `Bye`, a timeout, a kick or a failed repair; and, at a migration's resume, absent players. T is the same tick at which N6's `Keys(0)` began.
  - **Final, like the input:** the log stores the mark with the slot's value at T (`InputLog::set_final(T, s, 0, true)`).
  - **On the wire:** the mark travels once per message, as an entry of a compact `(slot, tick)` list (`gone`) beside the frames. A message lists the marks whose tick lies within its frames, so the datagram that carries T's value carries the mark too. After T the slot's value stays `Keys(0)`, which the world ignores.
  - **Who withdraws, and when:** every peer (host, clients, spectators, the replay) withdraws before simulating T. A peer that simulated T or later without the mark rolls back to T.
  - **N6 is amended**, not dropped: the slot is still filled with `Keys(0)`, and its worm is gone from T.
  - **Cost if wrong:** a slightly more complex encoder; one byte more per datagram (an empty list).
- **N84 – the mode's side, as OLX.**
  - `GameMode::on_drop` is `CGameMode::Drop`: Deathmatch zeroes the worm's kills and deaths in a row.
  - The game over below two worms is Deathmatch's existing `CheckGameOver` port: a gone worm is out of lives, so it is no longer counted and cannot be the winner.
  - A kill credited to a gone worm (its projectile in flight, its damage leaving a worm at 0 health) credits nobody (`wormById` is null, `CServer_Game.cpp:126-127`).
  - **Cost if wrong:** none known.
- **N85 – the scoreboard (deviation, user decision 2).**
  - `Score::left` marks a gone worm's row. The mode keeps the row, ranked as an out worm by OLX's `CompareWormsScore`.
  - The live scoreboard (Tab, `rl_ui::hud::rows`) leaves it out. The results (`rl_ui::hud::Results`) keep it, its lives reading "left", greyed.
  - OLX's results lose the worm, because it is deleted.
  - `MatchEndWire` is unchanged: its rows still list every row, and the dedicated server's log names the first row, never a left one unless every worm left.
  - **Cost if wrong:** one table row.
- **N86 – the texts** (ruling PF12d-3 for the resume).
  - OLX's `NetworkTexts` defaults (`Options.cpp:769-770`):
    - "<name> has left" for a `Bye` or a link that closed;
    - "<name> has timed out" for the session's `PeerTimedOut` or the link's `Why::Timeout`.
  - Today's "<name> left" becomes "<name> has left".
  - Kicks and bans keep their notices.
  - **At a migration's resume:**
    - The old host is withdrawn silently. The migration's own lines ("The host is gone…", "<name> is now hosting") say it, and the migrated lobby never keeps the old host's member (`LobbyHost::migrated`), so nothing announces it.
    - Each player who did not come back gets "<name> has left": `MigratedLobby::drop_away` drops them with `Gone::Left`.
  - **Cost if wrong:** one log line.
- **N87 – the crash timeout stays 10 s** (user decision 4): `SILENCE_TIMEOUT_US` in the session, `SILENCE_TIMEOUT` in rl-net. Until it fires, the lagging rule repeats the slot's last final keys; at it, the worm is withdrawn.
  - Those 10 s of fills stall players whose link cannot bear them (`fills_stall`, true on `bad`). net-sim's stall verdict excuses them in a window from the crash tick to the timeout plus the recovery (ruling PF12d-5).
  - **Cost if wrong:** a stall bug inside that window hides.
- **N88 – migrations** (user decision 5):
  - **Abandoned at the resume:** the slots the resume abandons (the old host's player, players not back) are withdrawn at their first unknown tick, by the same fill.
  - **Marks are final:** the merge takes every withdrawal mark a report carries, so a survivor's mark is never lost (spec §3.2).
  - **A leave before the resume:** a peer that rejoins and then leaves is withdrawn at the resume's fill.
  - **A side effect, intended (user decision 3):** a 2-worm match whose host leaves with migration on resumes with one worm and ends `LastStanding`.
  - **Cost if wrong:** none known.
- **N89 – a watcher's camera** (a spectator, a replay or bots alone) skips gone worms. When the worm it follows is withdrawn, it moves to the next worm not gone. **Cost if wrong:** a watcher presses an arrow key once.

## Review Focus

1. **A player leaves while the others have predicted past its withdrawal tick** (`bad` and `awful` links, a 100+ ms trip).
   - **Expected:** every peer rolls back to T, withdraws there, and their checksums agree with an offline replay. No desync, no repair.
   - **Pinned by:** Task 4's `a_leaving_client_is_withdrawn_at_one_tick_everywhere` (every profile × 4 seeds) and its session unit test `a_withdrawal_learned_late_rolls_back_to_it` (it fails without `on_gone`).
2. **A player leaves during a migration, or just before the host crashes** (the mark may have reached some survivors and not others).
   - **Expected:** every survivor withdraws the worm at the same tick: the mark one survivor held, or the resume's fill.
   - **Pinned by:**
     - Task 4's `a_leave_during_a_migration_is_withdrawn_alike`, `a_leave_just_before_the_host_crashes_is_kept` and `a_leave_after_a_migration_is_withdrawn_by_the_new_host`;
     - Task 5's `a_player_leaving_during_a_migration_is_removed_at_the_resume` (rl-online).
3. **The last opponent leaves** (a 1-vs-1, or only one worm left with lives, bots included).
   - **Expected:** the match ends at once (LastStanding), and the remaining worm wins.
   - **Pinned by:** Task 1's `a_withdrawn_worm_leaves_the_board_and_the_last_one_wins` and Task 5's `the_last_opponent_leaving_ends_the_match`.
4. **A hostile or broken host's marks:**
   - a `gone` list too long, with a slot twice, a slot past the match's or a tick outside its frames;
   - a second mark for one slot;
   - a mark for a tick already final without it;
   - a world state whose gone mask names no worm or a living one;
   - a replay whose withdrawals are out of order, repeated or past its end.
   - **Expected:** no panic; refused, ignored or a conflict, as the Global Constraints say.
   - **Pinned by:** Task 3's `withdrawal_marks_are_bounded` and `a_withdrawal_mark_is_final`, Task 1's `a_state_naming_no_worm_gone_is_refused`, Task 2's `withdrawals_are_checked_on_decode`, and the fuzzers.
5. **What players see of a leaver:**
   - its rocket still in flight;
   - a watcher following it;
   - its row in the results.
   - **Expected:**
     - The rocket explodes but credits nobody.
     - The watcher's camera moves on.
     - The results keep its row, marked "left", with its kills and deaths so far.
   - **Pinned by:** Task 1's `a_projectile_of_a_worm_that_left_credits_nobody` and `a_gone_worms_kill_credits_nobody`, Task 6's `the_watched_worm_moves_on_when_it_leaves` and `a_player_who_leaves_is_off_the_scoreboard_and_left_in_the_results`.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/rl-sim/src/{world.rs, referee.rs, mode.rs, damage.rs, respawn.rs, setup.rs, state.rs, world_tests.rs, state_tests.rs}` | `World::withdraw`, `World::gone`, the referee's mask, `ModeCtx::gone`, `GameMode::on_drop`, `Score::left`, gone killers credit nobody, the state transfer (Task 1). |
| `crates/rl-modes/src/{deathmatch.rs, rules.rs, tests.rs}`, `tests/deathmatch.rs` | Deathmatch's `on_drop` and its board's `left` rows (Task 1). |
| `crates/rl-replay/src/{lib.rs, record.rs, play.rs, wire.rs, error.rs}`, `tests/replay.rs` | `Replay::withdrawals`, `Recorder::withdraw`, the cursor, format 3 (Task 2). |
| `crates/rl-proto/src/lib.rs`, `tests/{fuzz.rs, migration.rs}` | The `gone` lists, `valid_marks`, the decoders' checks (Task 3). |
| `crates/rl-netcode/src/{inputs.rs, session.rs, session/migrate.rs, migration.rs}`, `tests/{play.rs, sync.rs, migration.rs}` | `InputLog::set_final`/`gone_at`/`gone_marks`/`marks_within`; every message that carries final inputs carries the marks (`fit_report`, `fit_resume`, `resume_datagram` included) (Task 3). |
| `crates/rl-netcode/src/{sim.rs, world_sim.rs, session.rs, session/migrate.rs, harness.rs}`, `tests/{play.rs, migration.rs, sync.rs}`, `crates/rl-cli/src/netsim.rs` (`empty_report`) | The host withdraws, the session simulates and rolls back withdrawals, `Simulation::withdraw`, the harness's report and verdict (Task 4). |
| `crates/rl-online/src/{replay.rs, peer.rs, bundle.rs}`, `tests/online.rs` | `MatchRecord::gone`, replays and bundles with withdrawals (Task 4). |
| `crates/rl-proto/src/lobby.rs`, `crates/rl-lobby/src/{host.rs, model.rs, harness.rs}`, `tests/lobby.rs`, `crates/rl-online/src/{peer.rs, events.rs, peer/migrate.rs}`, `tests/{online.rs, migration.rs}`, `crates/rl-game/src/{shell/online.rs, net/pump.rs}` | `Notice::TimedOut`, `LobbyHost::timed_out`, `OnlineEvent::TimedOut`, the texts; rl-online's leave, timeout, last-opponent and migration tests (Task 5). |
| `crates/rl-ui/src/hud/{model.rs, paint.rs}`, `crates/rl-game/src/session.rs`, `tests/{online.rs, watch.rs}` | Live scoreboard without left rows, results with them, the watcher's camera (Task 6). |
| `crates/rl-cli/src/{bundle.rs, netsim.rs, soak.rs}`, `tests/{net_sim.rs, net_soak.rs, net_repro.rs}` | The net-sim bundle's withdrawals, `net-repro`, the crash window, client departures in net-soak (Task 7). |
| `docs/porting/STATUS.md`, `CHANGELOG.md`, `docs/online.md`, the crates' `CLAUDE.md` | Records (each task keeps its crates' `CLAUDE.md` current; Task 8 the rest) and the single `full` run (Task 8). |

---

### Task 1: `rl-sim` and `rl-modes`: withdrawing a worm

**Files:**
- Modify: `crates/rl-sim/src/referee.rs` (`Referee::gone`, its hash, `PartialEq`, the `ModeCtx` it builds), `crates/rl-sim/src/mode.rs` (`Score::left`, `GameMode::on_drop`, `ModeCtx::gone`), `crates/rl-sim/src/world.rs` (`World::gone`, `World::withdraw`), `crates/rl-sim/src/damage.rs` (`kill_worm`: a gone killer is nobody; `Harm::mode`'s `ModeCtx`), `crates/rl-sim/src/respawn.rs` and `crates/rl-sim/src/setup.rs` (their `ModeCtx`), `crates/rl-sim/src/state.rs` (`WorldState::gone`, its check), `crates/rl-sim/src/world_tests.rs`, `crates/rl-sim/src/state_tests.rs`, `crates/rl-sim/src/mode_tests.rs` (its `Score` literals), `crates/rl-sim/CLAUDE.md`
- Modify: `crates/rl-modes/src/deathmatch.rs` (`on_drop`, `refresh` with the mask, the checksum), `crates/rl-modes/src/rules.rs` (`row` sets `left: false`), `crates/rl-modes/src/tests.rs`, `crates/rl-modes/tests/{deathmatch.rs, state.rs}`, `crates/rl-modes/CLAUDE.md`

`Score { .. }` literals exist only in rl-sim and rl-modes. Every one gains `left: false`, and the compiler names them.

**Interfaces:**
- Consumes: nothing new.
- Produces (Tasks 2, 4, 6):
  ```rust
  // rl_sim (mode.rs)
  pub struct Score {
      // …the existing fields, unchanged and in order, then:
      /// The worm left the match (`World::withdraw`): off the live
      /// scoreboard, a "left" row in the results (decision N85).
      pub left: bool,
  }
  pub trait GameMode {
      // …existing methods, then:
      /// Worm `worm` left the match for good ([`World::withdraw`]), in place
      /// of OLX's `CGameMode::Drop` (`src/server/CGameMode.cpp:254-263`).
      /// It is dead and out of lives already.
      fn on_drop(&mut self, worm: i32, ctx: &mut ModeCtx) {
          let _ = (worm, ctx);
      }
  }
  impl ModeCtx<'_> {
      /// The worms withdrawn from the match, bit `i` for worm `i` ([`World::gone`]).
      pub fn gone(&self) -> u32;
  }
  // rl_sim (world.rs)
  impl World {
      /// The worms withdrawn from the match ([`World::withdraw`]), bit `i` for worm `i`.
      pub fn gone(&self) -> u32;
      /// Takes worm `i` out of the match for good (decision N82) …
      pub fn withdraw(&mut self, i: usize);
  }
  ```

Context:
- **The mask lives in `Referee`** (`pub gone: u32`). Every place that builds a `ModeCtx` already holds the referee:
  - `damage.rs`'s `Harm::mode`;
  - `respawn.rs`'s `simulate_respawns`;
  - `setup.rs`'s `add_worm`;
  - `referee.rs`'s `mode_tick` and its test `ctx`.

  Add `pub(crate) gone: u32` to `ModeCtx` and fill it from `referee.gone` at each.
- **`Referee::hash`:** after `game_over_sent`, `if *gone != 0 { h.u32(*gone); }`. A world with no withdrawal hashes exactly as before: the match goldens and the oracle are the proof. `PartialEq` compares `gone`.
- **`WorldState`:**
  - Append `gone: u32` after `game_over_sent` (the wire layout's end).
  - `state()` takes it from the referee and `load_state` puts it back.
  - `validate` refuses (`StateError::Worms`) a mask with a bit at or above `worms.len()`, and a gone worm that is alive or not `WRM_OUT`. A stranger cannot mark a living worm gone.
- **`World::withdraw(i)`.** Doc comment:

  ```rust
  /// Takes worm `i` out of the match for good (decision N82), as OLX's
  /// `GameServer::RemoveClientWorms` (`src/server/CServer.cpp:1440-1482`)
  /// removes a client's worms: the mode is told (`CGameMode::Drop`, here
  /// [`GameMode::on_drop`]) and the next game-over check counts it out
  /// (`RecheckGame`). Unlike OLX's `Game::removeWorm` the worm stays in
  /// [`World::worms`], so worm `i` is still slot `i`: dead, out of lives
  /// (`WRM_OUT`), its rope, input state and key latches cleared; it never
  /// respawns, and every path that skips dead worms skips it. Its kills
  /// from now on credit nobody (`GameServer::killWorm` finds no killer,
  /// `src/server/CServer_Game.cpp:126-127`). An input between two ticks,
  /// as the netcode and replays give it: before the tick it belongs to.
  /// A worm that does not exist, or is gone already, changes nothing.
  ```

  Body:

  ```rust
  pub fn withdraw(&mut self, i: usize) {
      if i >= self.worms.len() || self.referee.gone & (1 << i) != 0 {
          return;
      }
      self.referee.gone |= 1 << i;
      let w = &mut self.worms[i];
      w.alive = false;
      w.lives = crate::damage::WRM_OUT;
      w.can_respawn_now = false;
      w.respawn_requested = false;
      w.state = Default::default();
      w.rope.clear();
      self.latches[i] = Latches::default();
      let (id, now) = (w.id, self.time);
      let r = &mut self.referee;
      let mut ctx =
          ModeCtx { worms: &self.worms, settings: &self.settings, now, gone: r.gone, commands: &mut r.commands };
      r.mode.on_drop(id, &mut ctx);
  }
  ```

  `i < MAX_WORMS` (32) holds because `worms` never exceeds it, so `1 << i` cannot overflow. A command the mode queues there is applied at the next tick's `apply_commands`, as setup's are.
- **`kill_worm`** (damage.rs): at its start, after the game-over return, map a gone killer to nobody:

  ```rust
  // A worm withdrawn is no killer (decision N84): OLX's `game.wormById(killer, false)`
  // is null for a removed worm (`src/server/CServer_Game.cpp:126-127`).
  let killer = match index(h.worms, killer) {
      Some(k) if h.referee.gone & (1 << k) != 0 => -1,
      _ => killer,
  };
  ```

  - `die_at_zero_health` goes through `kill_worm`, so a `last_damager` that left is covered too.
  - A projectile's explosion goes through `kill_worm` too.
  - A beam shot whose worm is dead at delivery is dropped already (`shots.rs`: "The shot of a worm that is dead at delivery is dropped").
- **Deathmatch:**
  - `refresh(&mut self, worms, s, gone: u32)` sets `left` on each row (`r.left = gone & (1 << i) != 0` for worm index `i`). Its callers pass `ctx.gone()`.
  - `on_drop` is OLX's `Drop`:

    ```rust
    /// `CGameMode::Drop` (`src/server/CGameMode.cpp:254-263`): the worm's
    /// kills and deaths in a row start over; its row is now a left one.
    fn on_drop(&mut self, worm: i32, ctx: &mut ModeCtx) {
        self.drop_worm(worm);
        self.refresh(ctx.worms(), ctx.settings(), ctx.gone());
    }
    ```

    with `pub(crate) fn drop_worm(&mut self, worm: i32)`, which zeroes `kills_in_row` and `deaths_in_row` at `worm` when `0 <= worm < MAX_WORMS` (OLX's bounds check logs and returns).
  - The checksum's board loop destructures `left` and hashes it only when true: `if left { h.bool(true); }`.
  - `check` and `rules::winner` need no change: a gone worm's `lives` is `WRM_OUT`.

- [ ] **Step 1: Write the failing tests.** Add to `crates/rl-sim/src/world_tests.rs`:

```rust
/// N82: a withdrawn worm is out of play for good, its slot still its own,
/// and the mask itself is hashed: a twin made dead and `WRM_OUT` by hand,
/// without the mask, differs only by it.
#[test]
fn a_withdrawn_worm_is_out_for_good() {
    let mut w = world(Material::Background);
    w.spawn_worm(60, 20);
    w.spawn_worm(140, 20);
    for _ in 0..20 {
        w.tick_new(&[Keys(0), Keys(0)]);
    }
    let mut by_hand = w.clone();
    {
        let g = &mut by_hand.worms[1];
        g.alive = false;
        g.lives = crate::damage::WRM_OUT;
        g.can_respawn_now = false;
        g.respawn_requested = false;
        g.state = Default::default();
        g.rope.clear();
        by_hand.latches[1] = Latches::default();
    }
    w.withdraw(1);
    assert_eq!(w.gone(), 0b10);
    assert_eq!((by_hand.gone(), &by_hand.worms()[1]), (0, &w.worms()[1]), "the same worm");
    assert_ne!(w.checksum(), by_hand.checksum(), "the mask is hashed once set");
    assert_eq!(w.worms().len(), 2, "worm 1 is still slot 1");
    w.withdraw(1);
    w.withdraw(7);
    assert_eq!(w.gone(), 0b10, "again, or no such worm: nothing");
    // Pressing jump never respawns it.
    for _ in 0..2_000 {
        w.tick_new(&[Keys(0), Keys(Keys::JUMP)]);
    }
    assert!(!w.worms()[1].alive);
}

/// N84: a kill credited to a worm that left credits nobody.
#[test]
fn a_gone_worms_kill_credits_nobody() {
    let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, Material::Background));
    let s = SimSettings { olx_quirks: false, ..SimSettings::lx56() };
    let mut w = World::test_new(s, test_mod(), terrain, holes(), AbsTime(0), 1);
    w.spawn_worm(60, 20);
    w.spawn_worm(140, 20);
    w.tick_new(&[Keys(0), Keys(0)]);
    // Worm 0's damage left worm 1 at exactly 0 health: it dies at the next
    // frame's start, credited to its last damager (play, spec §18 D9).
    w.worms[1].health = 0.0;
    w.worms[1].last_damager = 0;
    w.withdraw(0);
    w.tick_new(&[Keys(0), Keys(0)]);
    assert!(!w.worms()[1].alive, "worm 1 died");
    assert_eq!(w.worms()[0].kills, 0, "nobody is credited");
}
```

Add to `crates/rl-sim/src/state_tests.rs`:

```rust
/// A transferred state keeps the mask; one naming no worm, or a living
/// worm, is refused.
#[test]
fn a_state_naming_no_worm_gone_is_refused() {
    let mut a = two_worm_world();
    a.withdraw(1);
    let mut b = two_worm_world();
    b.load_state(a.state()).unwrap();
    assert_eq!((b.gone(), b.checksum()), (a.gone(), a.checksum()));
    let mut bad = a.state();
    bad.set_gone_for_test(0b100);
    assert_eq!(two_worm_world().load_state(bad), Err(StateError::Worms));
    let mut alive = two_worm_world().state();
    alive.set_gone_for_test(0b01);
    assert_eq!(two_worm_world().load_state(alive), Err(StateError::Worms), "a living worm is not gone");
}
```

`two_worm_world()` is a helper in `state_tests.rs`: build as the file's other tests do, with two worms spawned. `WorldState::set_gone_for_test` is `#[cfg(test)] pub(crate)`.

Add to `crates/rl-modes/tests/deathmatch.rs`:

```rust
/// N84, N85, user decision 3: a withdrawn worm's row is a left one; with
/// fewer than two worms not out the match ends at once and the last wins
/// (`CGameMode::CheckGameOver`, `src/server/CGameMode.cpp:316-336`).
#[test]
fn a_withdrawn_worm_leaves_the_board_and_the_last_one_wins() {
    let mut r = settled(dm(), &[(50, LASER), (200, LASER), (300, LASER)]);
    r.w.withdraw(1);
    r.idle(1);
    let rows: Vec<(i32, bool)> = r.w.scores().iter().map(|s| (s.worm, s.left)).collect();
    assert_eq!(rows.iter().filter(|r| r.1).collect::<Vec<_>>(), [&(1, true)]);
    assert!(r.w.result().is_none(), "two worms play on");
    r.w.withdraw(2);
    r.idle(1);
    assert_eq!(r.w.result(), Some(MatchResult { winner: 0, reason: EndReason::LastStanding }));
}

/// N84 (ruling PF12d-2): a projectile in flight when its worm leaves still
/// explodes and kills, crediting nobody. A beam cannot show it: its shot is
/// dropped when its worm is dead at delivery.
#[test]
fn a_projectile_of_a_worm_that_left_credits_nobody() {
    // Worm 0 throws a bomb at its feet (the suicide layout of
    // `deathmatch_game_over_and_winner_like_olx`); worm 1 stands in the
    // blast; worm 2 far away keeps two worms in the match.
    let mut r = settled(dm(), &[(50, BOMB), (58, LASER), (300, LASER)]);
    r.fire(&[0], 0);
    r.idle_until(20, |w| w.projectiles().next().is_some()); // delivered: in flight
    r.w.withdraw(0);
    r.idle(200);
    let kills: Vec<(i32, i32)> = r.kills().iter().map(|k| (k.1, k.2)).collect();
    assert!(kills.contains(&(1, -1)), "the bomb killed worm 1, credited to nobody: {kills:?}");
    assert_eq!(r.w.worms()[0].kills, 0);
}
```

`Run::idle_until(max, pred)` is a small helper added to `tests/common`: tick with no keys until `pred(&r.w)` holds, at most `max` ticks, and panic if it never does. If the blast does not reach worm 1 at x 58, move worm 1 to where the suicide bomb of `deathmatch_game_over_and_winner_like_olx` lands. The test needs the bomb to kill worm 1, not these numbers.

Add to `crates/rl-modes/src/tests.rs`:

```rust
/// `CGameMode::Drop`: the worm's runs start over.
#[test]
fn drop_resets_the_runs_like_olx() {
    let s = SimSettings::lx56();
    let w = worms(3);
    let mut dm = Deathmatch::new(DeathmatchSettings::default());
    dm.kill(&w, &s, 1, 0);
    dm.kill(&w, &s, 2, 0);
    dm.kill(&w, &s, 0, 2);
    assert_eq!((dm.kills_in_row[0], dm.deaths_in_row[0]), (0, 1));
    assert_eq!(dm.kills_in_row[2], 1);
    dm.drop_worm(2);
    dm.drop_worm(0);
    assert_eq!((dm.kills_in_row[2], dm.deaths_in_row[0]), (0, 0));
    dm.drop_worm(-1);
    dm.drop_worm(99); // out of range: nothing
}
```

- [ ] **Step 2: Run them; they fail to compile** (`no method named withdraw`, `no field left`):

  ```bash
  cargo nextest run -p rl-sim -p rl-modes
  ```

- [ ] **Step 3: Implement** as the Context says: `Referee::gone`, `ModeCtx::gone`, `Score::left`, `GameMode::on_drop`, `World::gone`/`withdraw`, `kill_worm`'s mapping, the state's mask, Deathmatch's `on_drop`/`drop_worm`/`refresh`/checksum. Then add `left: false` to every `Score { .. }` literal the compiler names.

- [ ] **Step 4: Run them; they pass.** Then run the whole check, which also replays the match goldens and the oracle:

  ```bash
  cargo nextest run -p rl-sim -p rl-modes && cargo xtask check > /tmp/check.log 2>&1; tail -30 /tmp/check.log
  ```

  Expected: every test passes, the goldens and oracle unchanged. A golden failing means the mask or `left` is hashed when unset: fix that, never bless.

- [ ] **Step 5: Keep the records current.** `crates/rl-sim/CLAUDE.md` gets:
  - `World::withdraw` and `World::gone` (N82);
  - the mask in the referee, hashed only when set;
  - `WorldState::gone` and its check;
  - `ModeCtx::gone`, `GameMode::on_drop`, `Score::left`;
  - gone killers.

  `crates/rl-modes/CLAUDE.md` gets `on_drop` and the left rows.

- [ ] **Step 6: Commit.**

```bash
cargo xtask check > /tmp/check.log 2>&1 && git add crates/rl-sim crates/rl-modes && git commit -m "rl-sim, rl-modes: a worm can be withdrawn from the match

World::withdraw takes a worm out for good (dead, out of lives, never
respawned) and tells the mode (CGameMode::Drop); its later kills credit
nobody. The gone mask is hashed only when set: local play unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV"
```

---

### Task 2: `rl-replay`: withdrawals in replays (format 3)

**Files:**
- Modify: `crates/rl-replay/src/lib.rs` (`FORMAT_VERSION = 3`, `Replay::withdrawals`), `crates/rl-replay/src/record.rs` (`Recorder::withdraw`, carried through `finish` and `finish_failed`), `crates/rl-replay/src/play.rs` (`Cursor::step` withdraws first), `crates/rl-replay/src/wire.rs` (the field on the wire, checked on decode), `crates/rl-replay/src/error.rs` (`RecordError::NoWorm`), `crates/rl-replay/tests/replay.rs`, `crates/rl-replay/CLAUDE.md`

**Interfaces:**
- Consumes: `World::withdraw`, `World::gone` (Task 1).
- Produces (Tasks 4, 6):
  ```rust
  pub const FORMAT_VERSION: u16 = 3;
  pub struct Replay {
      // …
      /// `(t, worm)`: `worm` withdrawn ([`rl_sim::World::withdraw`]) right
      /// before tick `t` (0-based) is played; in order of `t`, each worm at
      /// most once, `t` below [`Replay::ticks`].
      pub withdrawals: Vec<(u32, u8)>,
  }
  impl Recorder {
      /// Withdraws `worm` from `world` before the next tick and records it.
      /// `RecordError::NotStarted` before `start`; `RecordError::NoWorm` for
      /// a worm `world` does not have or has withdrawn already.
      pub fn withdraw(&mut self, world: &mut World, worm: u8) -> Result<(), RecordError>;
  }
  pub enum RecordError { /* …, then: */ NoWorm(u8) }
  ```

Context:
- **`Recorder::withdraw`** calls `world.withdraw(worm as usize)` and pushes `(self.ticks, worm)`: the next tick to be pushed.
- **`Cursor::step`**: before `world.tick(replay.row(self.tick), out)`, it withdraws every `(t, w)` with `t == self.tick`, in order.
- **On decode** (`from_wire`), `DecodeError::Invalid` when:
  - more than `MAX_WORMS` entries;
  - a `t` at or past `ticks`;
  - `t` out of order;
  - a worm repeated;
  - a worm at or above `MAX_WORMS`.
- `finish_failed` keeps the withdrawals.
- `Replay`'s `PartialEq` includes the new field. `rl_online::replay::same_match` compares it with no change.

- [ ] **Step 1: Write the failing tests** in `crates/rl-replay/tests/replay.rs`. Base them on the file's existing recorder helper, which records a match of N ticks on its test world:

```rust
/// A withdrawal is recorded at its tick and replayed there.
#[test]
fn a_withdrawal_replays_at_its_tick() {
    let (mut world, mut rec) = recorder_and_world(); // the file's helper; two worms at least
    rec.start(&world).unwrap();
    let mut out = rl_sim::TickEvents::new();
    let keys = vec![rl_sim::Keys(0); world.worms().len()];
    for t in 0..250u32 {
        if t == 120 {
            rec.withdraw(&mut world, 1).unwrap();
        }
        world.tick(&keys, &mut out);
        rec.push(&keys, &world).unwrap();
    }
    assert_eq!(rec.withdraw(&mut world, 1), Err(RecordError::NoWorm(1)), "gone already");
    assert_eq!(rec.withdraw(&mut world, 40), Err(RecordError::NoWorm(40)));
    let replay = rec.finish(&world).unwrap();
    assert_eq!(replay.withdrawals, [(120, 1)]);
    let back = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(back, replay);
    let run = back.play(fresh_world()).unwrap(); // the file's helper: the same start world
    assert_eq!(run.world.gone(), 0b10);
    // Without the withdrawal the checksums no longer match.
    let mut cut = back.clone();
    cut.withdrawals.clear();
    assert!(cut.play(fresh_world()).is_err());
}

#[test]
fn withdrawals_are_checked_on_decode() {
    let base = short_replay(); // the file's helper: a decodable replay of a few ticks
    for bad in [
        vec![(base.ticks(), 0)],
        vec![(2, 0), (1, 1)],
        vec![(1, 0), (2, 0)],
        vec![(1, 40)],
        (0..33).map(|i| (0, i as u8)).collect(),
    ] {
        let mut r = base.clone();
        r.withdrawals = bad.clone();
        assert!(
            matches!(Replay::decode(&r.encode()), Err(DecodeError::Invalid(_))),
            "{bad:?}"
        );
    }
}
```

The test names the helpers it needs. If the file's existing ones are named otherwise, use them and keep the test's shape. Update the file's existing version test: it reads `FORMAT_VERSION`, so it follows the bump with no change.

- [ ] **Step 2: Run them; they fail to compile** (`no method named withdraw`, `no field withdrawals`): `cargo nextest run -p rl-replay`.

- [ ] **Step 3: Implement** as the Context says. Bump `FORMAT_VERSION` to 3. Update its doc comment: "3: withdrawals (plan 12d)".

- [ ] **Step 4: Run `cargo nextest run -p rl-replay`: they pass.** `rl-game`'s replay tests and `rl-cli`'s match round trips still pass in check: a local replay has no withdrawals.

- [ ] **Step 5: Records.** `crates/rl-replay/CLAUDE.md`: what a replay holds (withdrawals), `Recorder::withdraw`, the decode bounds, format 3.

- [ ] **Step 6: Commit** (`rl-replay: replays record withdrawals (format 3)`, the trailers) after `cargo xtask check > /tmp/check.log 2>&1 &&`.

---

### Task 3: `rl-proto` and `rl-netcode`: withdrawal marks on the wire and in the log

Nothing sets a mark yet. This task carries the marks through every message that carries final inputs, so Task 4 only has to set and use them. `Frame` and the frame codec (`encode_frames`, `decode_frames`) are unchanged (ruling PF12d-4).

**Files:**
- Modify: `crates/rl-proto/src/lib.rs`:
  - `HostInputs::gone`, `HostResume::gone`, `InputsReport::gone`, `Control::Resume::gone`;
  - `valid_marks`;
  - their checks in `validate_datagram` and `decode_control`.
- Modify: `crates/rl-proto/tests/{fuzz.rs, migration.rs}`, `crates/rl-proto/CLAUDE.md`.
- Modify: `crates/rl-netcode/src/inputs.rs`: `set_final`, `gone_at`, `gone_marks`, `marks_within`.
- Modify: `crates/rl-netcode/src/session.rs`:
  - the host's `HostInputs` loop;
  - `resume_datagram`, the source of `Datagram::Resume`;
  - `take_frames` and its callers;
  - the scratch list;
  - the test helper `host_inputs`.
- Modify: `crates/rl-netcode/src/migration.rs`: `fit_report` and `fit_resume` take the marks.
- Modify: `crates/rl-netcode/src/session/migrate.rs`: the report sent, the merge, `Control::Resume`.
- Modify: every struct literal of those messages the compiler names: `tests/{play.rs, sync.rs, migration.rs}` and the session's unit tests.
- Modify: `crates/rl-netcode/CLAUDE.md`.

**Interfaces:**
- Produces (Task 4):
  ```rust
  // rl_proto
  pub struct HostInputs {
      // …the existing fields, then:
      /// Withdrawals (decision N83) at ticks within `frames`, as `(slot,
      /// tick)`: at most `MAX_SLOTS`, each slot once.
      pub gone: Vec<(u8, u32)>,
  }
  // HostResume, InputsReport and Control::Resume gain the same `gone`
  // field, after their `frames`.
  /// Whether `marks` fit a message whose frames are ticks `first..first +
  /// len`: at most `MAX_SLOTS` entries, each slot below `slots` and listed
  /// once, each tick within the frames.
  pub fn valid_marks(first: u32, len: usize, marks: &[(u8, u32)], slots: usize) -> bool;
  // rl_netcode::InputLog
  /// Records `v` as `slot`'s final value at `tick`, and with `gone` that its
  /// worm is withdrawn there (decision N83). As `set`: only the slot's next
  /// unknown tick is stored. A mark for a slot withdrawn already is
  /// ignored (the value is still stored). For a known tick: `Same` when
  /// both the value and the mark agree, else `Conflict`.
  pub fn set_final(&mut self, tick: u32, slot: u8, v: u8, gone: bool) -> SetResult;
  /// The tick `slot`'s worm is withdrawn at, if it is.
  pub fn gone_at(&self, slot: u8) -> Option<u32>;
  /// Every withdrawal at a tick below `below`, as `(tick, slot)`, by tick then slot
  /// (records and replays' order).
  pub fn gone_marks(&self, below: u32) -> Vec<(u32, u8)>;
  /// The withdrawals at ticks `first..first + n`, as `(slot, tick)` (the wire's order).
  pub fn marks_within(&self, first: u32, n: u32) -> Vec<(u8, u32)>;
  // rl_netcode::migration
  pub fn fit_report(/* …as before…, */ marks: &dyn Fn(u32, u32) -> Vec<(u8, u32)>) -> InputsReport;
  pub fn fit_resume(/* …as before…, */ marks: &dyn Fn(u32, u32) -> Vec<(u8, u32)>) -> Control;
  ```
  `InputLog::set(t, s, v)` stays and is `set_final(t, s, v, false)`.

Context:
- **Checks:**
  - `validate_datagram` checks `valid_marks(first, frames.len(), &gone, slots)` for `Host` and `Resume`.
  - `decode_control` checks `Report` and `Resume` with `MAX_SLOTS` as the slot bound. The receiver's log ignores a slot beyond its match (`Gap`).
- **The log:** `gone: [u32; 32]`, with `u32::MAX` for none. `start_at` leaves it as it is (none).
- **Every encoder lists the marks within the frames it sends.** Where the span is cut to fit, it is computed again for the span that fits:
  - the host's datagrams: inside the halving loop, `gone: log.marks_within(first, n)`;
  - `resume_datagram`: the same;
  - `fit_report` and `fit_resume`: `marks(first, n)` inside `fit_frames`' closure. Their callers in `migrate.rs` pass `&|f, n| log.marks_within(f, n)`.
- **Every decoder takes them with the values:**
  - `take_frames(from, first, frames, gone)` is the client's. Its scratch list becomes `Vec<(u32, u8, u8, bool)>`, with `g = gone.contains(&(s, t))`. Then `set_final(t, s, v, g)`; `New` → `on_final(t, s, v)` and nothing more yet; `Conflict` → `Event::Inconsistent`.
  - The report merge in `migrate.rs` works the same way. Its `Conflict` is counted in `Stats::report_conflicts`.
- **Task 4 adds the reaction** (`on_gone`). Here a mark is only stored.

- [ ] **Step 1: Write the failing tests.** In `crates/rl-proto/tests/migration.rs` (it holds the migration wire's round trips and bounds):

```rust
/// N83: the marks travel once per message, within its frames, each slot once.
#[test]
fn withdrawal_marks_are_bounded() {
    assert!(valid_marks(10, 5, &[(0, 10), (3, 14)], 4));
    for bad in [
        vec![(0, 9)],          // before the frames
        vec![(0, 15)],         // past them
        vec![(4, 12)],         // a slot past the match's
        vec![(1, 11), (1, 12)], // a slot twice
    ] {
        assert!(!valid_marks(10, 5, &bad, 4), "{bad:?}");
    }
    assert!(!valid_marks(0, 100, &(0..33).map(|s| (s as u8 % 32, s)).collect::<Vec<_>>(), 32));
    let h = HostInputs {
        stamp: Stamp::default(),
        tick: 20,
        your_ack: 0,
        margin: 0,
        first: 10,
        frames: encode_frames(10, 5, 4, &|_, _| Some(0), &|_| Some(0)),
        gone: vec![(2, 12)],
    };
    let d = Datagram::Host(h.clone());
    assert_eq!(decode_datagram(&encode_datagram(&d), 4), Ok(d));
    let bad = Datagram::Host(HostInputs { gone: vec![(2, 15)], ..h });
    assert_eq!(decode_datagram(&encode_datagram(&bad), 4), Err(ProtoError::Invalid));
}
```

In `crates/rl-netcode/src/inputs.rs`'s tests:

```rust
#[test]
fn a_withdrawal_mark_is_final() {
    let mut log = InputLog::new(2);
    assert_eq!(log.set_final(0, 1, 4, false), SetResult::New);
    assert_eq!(log.set_final(1, 1, 0, true), SetResult::New);
    assert_eq!(log.gone_at(1), Some(1));
    assert_eq!(log.set_final(1, 1, 0, true), SetResult::Same);
    assert_eq!(log.set_final(1, 1, 0, false), SetResult::Conflict, "the mark is part of the value");
    assert_eq!(log.set_final(0, 1, 4, true), SetResult::Conflict, "no mark for a tick final without one");
    assert_eq!(log.set_final(2, 1, 0, true), SetResult::New);
    assert_eq!(log.gone_at(1), Some(1), "a second mark is ignored");
    assert_eq!(log.gone_at(0), None);
    log.set(0, 0, 9);
    log.set(1, 0, 9);
    assert_eq!(log.gone_marks(2), [(1, 1)]);
    assert_eq!(log.gone_marks(1), []);
    assert_eq!(log.marks_within(0, 2), [(1, 1)]);
    assert_eq!(log.marks_within(2, 5), []);
}
```

In `crates/rl-netcode/src/session.rs`'s tests, a client takes a mark from a hand-made host datagram. The file's `client()` has 3 slots, slot 2 its own. Its helper `host_inputs(tick, margin, first, frames)` becomes a call of a new `host_inputs_marked(tick, margin, first, frames, gone)` with `vec![]`:

```rust
/// N83: a client keeps the mark that comes with the value at its tick.
#[test]
fn a_client_keeps_the_hosts_withdrawal_mark() {
    let mut c = client();
    let frames = rl_proto::encode_frames(0, 20, 3, &|_, s| (s < 2).then_some(0), &|_| Some(0));
    c.receive(0, Channel::Unreliable, &host_inputs_marked(20, 0, 0, frames, vec![(0, 12)]), 1_000);
    assert_eq!(c.input_log().gone_at(0), Some(12));
    assert_eq!(c.input_log().gone_at(1), None);
}
```

Extend the `HostInputs`, `HostResume` and `Control` strategies of `crates/rl-proto/tests/fuzz.rs` with arbitrary `gone` lists. The decoders must never panic.

- [ ] **Step 2: Run them; they fail to compile** (`no field gone`, `no function valid_marks`, `no method set_final`): `cargo nextest run -p rl-proto -p rl-netcode`.

- [ ] **Step 3: Implement** as the Context says. Update every struct literal and caller the compiler names; a test that builds a message by hand passes `gone: vec![]`.

- [ ] **Step 4: Run `cargo nextest run -p rl-proto -p rl-netcode`: they pass**, and every existing session, sync, migration and timing test still passes (nothing sets a mark yet).

- [ ] **Step 5: Records.**
  - `crates/rl-proto/CLAUDE.md`: the `gone` lists and `valid_marks`.
  - `crates/rl-netcode/CLAUDE.md`:
    - `InputLog`'s marks;
    - "every message that carries final inputs lists, once, the marks within its frames: the host's datagrams, reports, `Control::Resume`, `Datagram::Resume`".

- [ ] **Step 6: Commit** (`rl-proto, rl-netcode: withdrawal marks travel with final inputs`, the trailers) after `cargo xtask check > /tmp/check.log 2>&1 &&`.

---

### Task 4: the session withdraws; the harness and rl-online records follow

The core of the plan: the host decides the withdrawal tick, every peer withdraws there, rollbacks replay it, the harness checks it, and rl-online's records and replays reproduce it.

rl-online's record must change in this same commit. Once survivors withdraw, a replay made from the rows alone would miss the confirmed checksums, and rl-online's migration tests assert there is no `DesyncBundle`. `rl-cli/src/netsim.rs`'s `empty_report()` builds a `harness::Report` field by field, so it gains the new fields here too.

**Files:**
- Modify: `crates/rl-netcode/src/sim.rs` (`Simulation::withdraw`; `ToySim`'s gone slots), `crates/rl-netcode/src/world_sim.rs` (`WorldSim::withdraw`), `crates/rl-netcode/src/session.rs` (`fill` marks, `simulate` withdraws, `on_gone`, the unit test `a_withdrawal_learned_late_rolls_back_to_it`), `crates/rl-netcode/src/session/migrate.rs` (the merge's `New` marks call `on_gone`), `crates/rl-netcode/src/harness.rs` (`Report::{host_gone, gone, exited_us}`, the verdict, its offline replay), the test simulations `Faulty` (`tests/sync.rs`) and `Huge` (`session.rs` tests), `crates/rl-netcode/tests/migration.rs`, `crates/rl-netcode/CLAUDE.md`
- Modify: `crates/rl-cli/src/netsim.rs`: `empty_report()` fills `host_gone`, `gone` and `exited_us` (empty).
- Modify: `crates/rl-online/src/replay.rs` (`MatchRecord::gone`, `make` withdraws), `crates/rl-online/src/peer.rs` (`record_of`), `crates/rl-online/src/bundle.rs` (`GameBundle::gone`, `GAME_BUNDLE_VERSION = 2`), `crates/rl-online/tests/{online.rs, replay.rs, migration.rs}`, `crates/rl-online/CLAUDE.md`

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces (Tasks 5-7):
  ```rust
  // rl_netcode
  pub trait Simulation {
      // …
      /// Takes `slot`'s worm out of the match for good before the tick the
      /// session simulates next (decision N83). Called again for that tick
      /// after a rollback: it must change nothing the second time.
      fn withdraw(&mut self, slot: u8);
  }
  // rl_netcode::harness::Report gains
  /// The last host's withdrawals, `(tick, slot)` (its `InputLog::gone_marks` below its frontier).
  pub host_gone: Vec<(u32, u8)>,
  /// Every peer's withdrawals below its frontier.
  pub gone: BTreeMap<PeerId, Vec<(u32, u8)>>,
  /// When each exited peer went (virtual µs).
  pub exited_us: BTreeMap<PeerId, u64>,
  // rl_online::replay
  pub struct MatchRecord {
      pub start: MatchStartWire,
      pub rows: Vec<[u8; 32]>,
      pub checksums: Vec<(u32, u64)>,
      /// The withdrawals at ticks below `rows.len()`, `(tick, slot)` (decision N83).
      pub gone: Vec<(u32, u8)>,
  }
  // rl_online::bundle::GameBundle gains `pub gone: Vec<(u32, u8)>`; GAME_BUNDLE_VERSION = 2.
  ```

Context:
- **The host decides** (`Session::fill`). For a lost player's slot and for an `abandoned` slot alike:

  ```rust
  while self.log.known_upto(s) <= t {
      let k = self.log.known_upto(s);
      // N83: its worm is withdrawn at its first unknown tick, with the
      // `Keys(0)` N6 fills from there.
      let gone = self.log.gone_at(s).is_none();
      self.log.set_final(k, s, 0, gone);
      scratch.push((k, s, 0, gone));
  }
  ```

  The scratch handler calls `self.on_final(k, s, v)`, then `if gone { self.on_gone(k) }`.
- **`on_gone(tick)`**: a withdrawal learned for a tick already simulated (it was predicted without it) rolls back there. It fires even when the value at T equals the prediction, where `on_final` alone would not:

  ```rust
  /// A withdrawal at `tick` became known (decision N83). A tick already
  /// simulated was simulated without it: roll back to it.
  fn on_gone(&mut self, tick: u32) {
      if tick < self.current && tick >= self.base {
          self.rollback_from = Some(self.rollback_from.map_or(tick, |r| r.min(tick)));
      }
  }
  ```

  `take_frames` and the report merge call it for every `New` mark (`g == true`).
- **`simulate`**: after the snapshot is pushed and before `self.sim.tick`, withdraw every slot marked at `t`:

  ```rust
  // N83: a withdrawal is part of tick `t`'s input: after the snapshot
  // (a rollback restores the worm and withdraws it again), before the tick.
  for s in 0..slots {
      if self.log.gone_at(s) == Some(t) {
          self.sim.withdraw(s);
      }
  }
  ```

  A world state loaded past T already holds the mask.
- **`WorldSim::withdraw`** is `self.world.withdraw(usize::from(slot))`.
- **`ToySim`:**
  - Gains `gone: u32`, set by `withdraw` (idempotent).
  - In `tick`, a gone slot does not move, and the hash takes `0x1_0000 | i` in place of its keys.
  - `checksum` folds `gone` in.
  - The state encoding adds `gone` after `hash`: 20 + 8·n bytes. `decode_state` refuses other lengths.
  - `Faulty` and `Huge` delegate or do nothing.
- **The harness:**
  - **Report:** it fills `host_gone`/`gone` beside `host_log`/`logs` (`log.gone_marks(log.frontier())`), and `exited_us` when it carries out an exit.
  - **Verdict, gone marks of kept peers:** a kept peer's marks with a tick in its compared range (`from_of(peer).0..ticks`) equal the last host's marks in that range.
  - **Verdict, peers that went:** a `left_from` peer's marks agree with the host's for every slot `s` and tick below `upto[s]`. That is the same bound its rows have; when that bound is `usize::MAX`, the peer's whole log.
  - **Verdict, offline replay:** before `sim.tick(&keys)` at `t`, it calls `sim.withdraw(s)` for each `(t, s)` in `host_gone`.
- **rl-online:**
  - `record_of` fills `gone: log.gone_marks(n)` for `n` rows.
  - `make` calls `recorder.withdraw(&mut world, s)` for each `(t, s)` of `record.gone` at `t` before `world.tick`. A `RecordError` is an error, like `push`'s.
  - `GameBundle::of` copies `gone`.
  - Every `MatchRecord { .. }` literal gains `gone`. The compiler names the four: `peer.rs`'s fallback record and test, `bundle.rs`, `replay.rs`, and `tests/replay.rs`.

- [ ] **Step 1: Write the failing tests first.**

`crates/rl-netcode/src/session.rs`'s unit tests: `on_gone` alone.

```rust
/// N83, Review Focus 1: a client that predicted a slot as `Keys(0)` past T
/// learns, with that same value at T, that the slot is withdrawn there: it
/// rolls back to T. The values agree, so only the mark can roll it back
/// (RED without `on_gone`).
#[test]
fn a_withdrawal_learned_late_rolls_back_to_it() {
    let (mut c, mut twin) = (client(), client());
    let mut d = driver();
    let zeros = |first, n| rl_proto::encode_frames(first, n, 3, &|_, s| (s < 2).then_some(0), &|_| Some(0));
    for s in [&mut c, &mut twin] {
        s.receive(0, Channel::Unreliable, &host_inputs(10, 0, 0, zeros(0, 10)), 1_000);
    }
    let mut now = 1_000;
    while c.current_tick() < 20 {
        now += 10_000;
        c.advance(now, d.as_mut());
        twin.advance(now, d.as_mut());
    }
    c.drain_events();
    c.receive(0, Channel::Unreliable, &host_inputs_marked(20, 0, 10, zeros(10, 10), vec![(0, 12)]), now);
    twin.receive(0, Channel::Unreliable, &host_inputs(20, 0, 10, zeros(10, 10)), now);
    now += 10_000;
    c.advance(now, d.as_mut());
    twin.advance(now, d.as_mut());
    assert!(c.drain_events().iter().any(|e| matches!(*e, Event::Rollback { from: 12, .. })));
    assert_ne!(c.sim().checksum(), twin.sim().checksum(), "the toy world withdrew slot 0 at 12");
}
```

`crates/rl-netcode/tests/migration.rs` (it holds the harness's toy scenarios and `mscenario`). Add a no-migration twin of `mscenario` and the scenarios:

```rust
/// A host (slot 0 its player, slot 1 its bot), `clients` players and one spectator, no migration.
fn scenario(seed: u64, profile: Profile, clients: u8) -> Scenario {
    Scenario { migration: None, ..mscenario(seed, profile, clients) }
}

/// The withdrawal of `slot` the last host confirmed, and that every
/// survivor confirmed it alike (the verdict checks it too: this names it).
fn withdrawn(r: &Report, slot: u8) -> u32 {
    let t = r.host_gone.iter().find(|g| g.1 == slot).map(|g| g.0).expect("the slot is withdrawn");
    for (peer, marks) in &r.gone {
        if let Some(m) = marks.iter().find(|g| g.1 == slot) {
            assert_eq!(m.0, t, "peer {peer} withdrew slot {slot} at another tick");
        }
    }
    t
}

/// N83, user decision 1: a client that leaves is withdrawn at one tick on
/// every peer, after its `Bye` reached the host; the offline replay withdraws
/// there too (the verdict). RED: no peer withdraws.
#[test]
fn a_leaving_client_is_withdrawn_at_one_tick_everywhere() {
    for profile in Profile::ALL {
        for seed in 1..=4 {
            let mut sc = scenario(seed, profile, 3);
            sc.exits.push(Exit { peer: 2, tick: 400, after_us: 0, how: ExitHow::Leave });
            let r = run_checked(&sc);
            let t = withdrawn(&r, 3);
            // Its first unknown tick: about the host's tick when the `Bye` came, less
            // at most the fill window on a lagging link.
            assert!((380..520).contains(&t), "seed {seed} {}: {t}", profile.name);
            assert!(r.host_log[t as usize..].iter().all(|row| row[3] == 0));
        }
    }
}

/// N87, user decision 4: a client that crashes is withdrawn when the host's
/// timeout fires, not before: until then its slot repeats its last keys.
#[test]
fn a_crashed_client_is_withdrawn_at_its_timeout() {
    let mut sc = scenario(7, Profile::TYPICAL, 2);
    sc.ticks = 2_000;
    sc.exits.push(Exit { peer: 1, tick: 300, after_us: 0, how: ExitHow::Crash });
    let r = run_checked(&sc);
    let t = withdrawn(&r, 2);
    let timeout = (rl_netcode::SILENCE_TIMEOUT_US / rl_netcode::TICK_US) as u32;
    assert!(t >= 300 + timeout - 20 && t <= 300 + timeout + 60, "{t}");
}

/// N88, Review Focus 2: a client leaving while the match moves to a new
/// host is withdrawn alike on every survivor (the mark one held, or the
/// resume's fill), at least once during the migration over the offsets.
#[test]
fn a_leave_during_a_migration_is_withdrawn_alike() {
    let mut during = 0;
    for after_us in [0, 500_000, 1_500_000, 2_050_000, 2_200_000, 2_600_000, 3_200_000, 4_500_000] {
        let mut sc = mscenario(11, Profile::GOOD, 3);
        sc.exits.push(Exit { peer: HOST, tick: 400, after_us: 0, how: ExitHow::Crash });
        sc.exits.push(Exit { peer: 3, tick: 400, after_us, how: ExitHow::Leave });
        let r = run_checked(&sc);
        withdrawn(&r, 4);
        withdrawn(&r, 0); // the old host's player, abandoned at the resume (user decision 5)
        let m = &r.migrations[0];
        let left = r.exited_us[&3];
        during += usize::from(m.silent_us < left && left < m.resumed_us);
    }
    assert!(during > 0, "no leave fell inside a migration: change the offsets");
}

/// N88: a mark the old host made just before it crashed, which a survivor
/// held when the match moved, is kept by the merge. Otherwise the new host
/// would withdraw the slot later, and the holder's mark would differ (the
/// verdict). The crash tick is swept so the client's `Bye` (a trip) lands
/// before it in some runs.
#[test]
fn a_leave_just_before_the_host_crashes_is_kept() {
    let mut held = 0;
    for crash in 404..=420 {
        let mut sc = mscenario(1, Profile::TYPICAL, 3);
        sc.exits.push(Exit { peer: 2, tick: 400, after_us: 0, how: ExitHow::Leave });
        sc.exits.push(Exit { peer: HOST, tick: crash, after_us: 0, how: ExitHow::Crash });
        let r = run_checked(&sc);
        let t = withdrawn(&r, 3);
        // A survivor held slot 3's value at `t` when the match moved: it held the mark.
        held += usize::from(r.held.iter().any(|h| {
            h.peer != HOST && h.slots.get(3).is_some_and(|v| h.origin as usize + v.len() > t as usize)
        }));
    }
    assert!(held > 0, "no survivor held the old host's mark: move the crash ticks");
}

#[test]
fn a_leave_after_a_migration_is_withdrawn_by_the_new_host() {
    let mut sc = mscenario(5, Profile::TYPICAL, 3);
    sc.exits.push(Exit { peer: HOST, tick: 300, after_us: 0, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 3, tick: 700, after_us: 0, how: ExitHow::Leave });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
    assert!(withdrawn(&r, 4) >= 700);
}
```

`SILENCE_TIMEOUT_US` and `TICK_US` are re-exported at `rl_netcode`'s root already.

`crates/rl-online/tests/online.rs`: the playtest's reproduction, as a test. Its ticks are bounded by state:
- the lower bound is the host's `known_upto` of Bert's slot just before he leaves;
- the upper bound is that value at the poll that first shows `Left("Bert")`.

```rust
/// The playtest (plan 12d): a player who leaves mid-match is removed at
/// once, at one tick, on every peer; the match goes on; the records and
/// replays carry the withdrawal and agree. RED: Bert's worm stays, alive.
#[test]
fn a_leaving_player_is_removed_from_every_world() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.15);
    let clients = net.join_all(&["Anna", "Bert"]);
    let (a, b) = (clients[0], clients[1]);
    net.ready(&clients);
    net.host().peer().lobby().start(9).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() >= 200);
    let sb = net.peers[b].started()[0].unwrap();
    let known = |n: &mut Net| n.host().peer().match_session().unwrap().input_log().known_upto(sb);
    let before = known(&mut net);
    net.peers[b].peer().leave();
    net.run_until(Duration::from_secs(3), "the host hears Bert leave", |n| {
        n.peers[0].has(|e| matches!(e, OnlineEvent::Left(x) if x == "Bert"))
    });
    let heard = known(&mut net);
    net.run_until(Duration::from_secs(3), "both withdraw Bert", |n| {
        [0, a].iter().all(|&i| {
            n.peers[i].p.as_ref().and_then(|p| p.match_session()).is_some_and(|s| {
                s.input_log().gone_at(sb).is_some_and(|t| t < s.confirmed())
            })
        })
    });
    let ticks: Vec<u32> = [0, a]
        .iter()
        .map(|&i| net.peers[i].peer().match_session().unwrap().input_log().gone_at(sb).unwrap())
        .collect();
    assert_eq!(ticks[0], ticks[1], "one tick on every peer");
    assert!((before..=heard).contains(&ticks[0]), "at once: {} in {before}..={heard}", ticks[0]);
    for i in [0, a] {
        let s = net.peers[i].peer().match_session().unwrap();
        let w = s.sim().world();
        assert_eq!(w.gone() & (1 << sb), 1 << sb);
        assert!(!w.worms()[usize::from(sb)].alive);
        assert!(w.scores().iter().any(|r| r.worm == i32::from(sb) && r.left));
    }
    net.run_until(Duration::from_secs(30), "the others finish", |n| {
        [0, a].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let host = net.peers[0].peer().last_record().cloned().unwrap();
    let anna = net.peers[a].peer().last_record().cloned().unwrap();
    assert_eq!(host, anna, "rows, checksums and withdrawals");
    assert_eq!(host.gone, [(ticks[0], sb)]);
    for i in [0, a] {
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))), "the replay meets every checksum");
    }
    let replay = rl_replay::Replay::decode(net.peers[0].replays()[0]).unwrap();
    assert_eq!(replay.withdrawals, [(ticks[0], sb)]);
    net.content().play_replay(&replay).unwrap();
}
```

`Net::ready` and `Net::host_tick` exist in `tests/common/net.rs`.

`a_client_vanishes_mid_match` (same file) gains a check that the vanished slot is withdrawn, at one tick on the host and on Anna. Its bounds come the same way: `before`, the host's `known_upto(sb)` read just before `net.peers[b].p = None`; `heard`, the same at the poll that first shows `Left("Bert")`:

```rust
    let t = host.gone.iter().find(|g| usize::from(g.1) == sb).expect("withdrawn").0;
    assert!((before..=heard).contains(&t), "{t} in {before}..={heard}");
    assert_eq!(host.gone, anna.gone);
```

`crates/rl-online/tests/migration.rs`, `a_frozen_host_hands_the_match_to_the_first_player`: add that `ra.gone` holds slot 0 (the old host's player) at a tick at or past the largest `held`.

- [ ] **Step 2: Run them; they fail.**
  - The session test fails on the missing `Rollback` from 12. It compiles once `Simulation::withdraw` exists.
  - The harness tests fail to compile (`no field host_gone`). Give `Report` the fields first and they fail on "the slot is withdrawn".
  - The rl-online test fails on `gone_at(sb)` staying `None`.

  ```bash
  cargo nextest run -p rl-netcode ; RL_CONTENT_DIR=$PWD/content/packs cargo nextest run -p rl-online --test integration online::a_leaving_player
  ```

- [ ] **Step 3: Implement** as the Context says:
  - `Simulation::withdraw`;
  - `WorldSim`, `ToySim`, `Faulty`, `Huge`;
  - `fill`, `on_gone` and `simulate`;
  - the merge's `on_gone`;
  - the harness and `netsim.rs`'s `empty_report`;
  - rl-online's record, replay and bundle.

- [ ] **Step 4: Run them; they pass.** Then the crates' whole suites. Every existing migration and play test must pass:
  - a leaving or crashing host's player is now withdrawn at the resume;
  - the toy checksums move with it;
  - the verdict replays it.

  ```bash
  cargo nextest run -p rl-netcode && RL_CONTENT_DIR=$PWD/content/packs cargo nextest run -p rl-online -p rl-cli
  ```

  An rl-online or rl-cli test that asserted the old behaviour ("the old host's player stands still", "the vanished slot presses nothing") still holds: the slot's value stays `Keys(0)`.

- [ ] **Step 5: Records.**
  - `crates/rl-netcode/CLAUDE.md`:
    - the lagging rule's paragraph: "peer_lost abandons a peer: its slot is withdrawn at its first unknown tick (N83) and `Keys(0)` from there";
    - the resume's abandoned slots (N88);
    - `Simulation::withdraw`;
    - `simulate`'s order;
    - `on_gone`;
    - the harness's report fields and verdict.
  - `crates/rl-online/CLAUDE.md`: `MatchRecord::gone`, `make`, `GameBundle` version 2.

- [ ] **Step 6: Commit** (`rl-netcode, rl-online: a player who leaves is withdrawn at one tick everywhere`, a body naming N83, N87 and N88, the trailers) after `cargo xtask check > /tmp/check.log 2>&1 &&`. Stage `crates/rl-cli/src/netsim.rs` with the rest.

---

### Task 5: "has left", "has timed out"; the last opponent; a leave during a migration (rl-online)

**Files:**
- Modify: `crates/rl-proto/src/lobby.rs` (`Notice::TimedOut(u8)`, appended), `crates/rl-proto/tests/fuzz_lobby.rs` (the strategy gains it)
- Modify: `crates/rl-lobby/src/host.rs` (`LobbyHost::timed_out`, `Gone::TimedOut`), `crates/rl-lobby/src/model.rs` (`HostEvent::TimedOut(u8)`), `crates/rl-lobby/src/harness.rs` (`Net::time_out(c)`), `crates/rl-lobby/tests/lobby.rs`, `crates/rl-lobby/CLAUDE.md`
- Modify: `crates/rl-online/src/events.rs` (`OnlineEvent::TimedOut(String)`), `crates/rl-online/src/peer.rs` (`forget_peer_as`; the host's timeouts go through `timed_out`; the client maps `Notice::TimedOut`), `crates/rl-online/src/peer/migrate.rs` (its `NetEvent::Disconnected` arms alike), `crates/rl-online/tests/{online.rs, migration.rs}`, `crates/rl-online/CLAUDE.md`
- Modify: `crates/rl-game/src/shell/online.rs` (the two lines), `crates/rl-game/src/net/pump.rs` (`TimedOut` is evictable, as `Left` is), `crates/rl-game/tests/online.rs` (`"Anna left"` becomes `"Anna has left"`), `crates/rl-game/CLAUDE.md`

**Interfaces:**
- Consumes: Task 4.
- Produces:
  ```rust
  // rl_proto::Notice gains (appended) TimedOut(u8): member `id` timed out.
  // rl_lobby
  impl LobbyHost {
      /// `peer`'s link timed out (the session's `PeerTimedOut`, or the
      /// transport's timeout): as `disconnected`, announced as
      /// `Notice::TimedOut` and `HostEvent::TimedOut` (decision N86).
      pub fn timed_out(&mut self, peer: PeerId);
  }
  pub enum HostEvent { /* …, then: */ TimedOut(u8) }
  // rl_online
  pub enum OnlineEvent { /* …, then: */ /// A member timed out (OLX's "<player> has timed out").
      TimedOut(String) }
  ```

Context:
- **rl-lobby:** `drop_member`'s `Gone::TimedOut` broadcasts `ToClient::Notice(Notice::TimedOut(id))` and pushes `HostEvent::TimedOut(id)`. Everything else is as `Gone::Left`. rl-proto validates no `Notice` id, so `TimedOut(u8)` needs only the fuzz sample.
- **rl-online, host side:**
  - `forget_peer` becomes `forget_peer_as(peer, timed_out: bool)`. It calls `lobby.timed_out(peer)` or `lobby.disconnected(peer)`, then does the rest as now: `forget`, the content side, `Session::peer_lost`. `forget_peer(peer)` stays as `forget_peer_as(peer, false)`.
  - `Event::PeerTimedOut { peer }` gets its own arm: close the endpoint (as `drop_peer` does), then `forget_peer_as(peer, true)`.
  - `NetEvent::Disconnected { peer, why: Why::Timeout }` calls `forget_peer_as(peer, true)` only. The link is gone already.
  - Every other `Disconnected`, and `RepairFailed`/`JoinRefused`, keep `forget_peer`/`drop_peer`.
  - Both paths reach `Session::peer_lost`, so the worm is withdrawn (N87).
- **rl-online, client side:** `Notice::TimedOut(id)` → `OnlineEvent::TimedOut(name)`. `HostEvent::TimedOut(id)` → the same on the host.
- **At a resume (N86, ruling PF12d-3):** nothing changes in the lobby.
  - `LobbyHost::migrated` never keeps the old host's member, so the old host goes silently.
  - `MigratedLobby::drop_away` drops each player not back with `Gone::Left`, so they get "<name> has left".
  - The migration test below pins both.
- **rl-game:**
  - `OnlineEvent::Left(name)` → `"{name} has left"`.
  - `OnlineEvent::TimedOut(name)` → `"{name} has timed out"` (both `LogLine::event`).
  - Doc comment: `/// OLX's NetworkTexts HasLeft and HasTimedOut (src/client/Options.cpp:769-770).`
  - The pump's evictable list (`net/pump.rs`) gains `TimedOut`.

- [ ] **Step 1: Write the failing tests.**

`crates/rl-lobby/tests/lobby.rs` (after the leaving tests; start the lobby with the file's own `lobby()` helper, as its neighbours do):

```rust
/// N86: a member whose link timed out is announced as timed out, not left.
#[test]
fn a_timed_out_member_is_announced_as_such() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let w = join(&mut net, 2, "Wat");
    let id = id_of(&net, a);
    net.time_out(a);
    net.settle();
    assert!(got(&net, w, &ClientEvent::Notice(Notice::TimedOut(id))));
    assert!(net.host_events.contains(&HostEvent::TimedOut(id)));
    assert!(!net.host_events.contains(&HostEvent::Left(id)));
    assert!(net.host.state().members.iter().all(|m| m.id != id));
    consistent(&net);
}
```

`crates/rl-online/tests/online.rs`:

```rust
/// User decision 3: the last opponent leaving ends the match at once
/// (OLX's CheckGameOver below two worms); the one left wins. RED before
/// Task 4 (Anna's worm stayed); here it pins the end and the text.
#[test]
fn the_last_opponent_leaving_ends_the_match() {
    let Some(content) = content() else { return };
    let setup = MatchSetupWire { bots: vec![], ..setup(&content, 1.0) };
    let cfg = HostConfig { name: "Duel".into(), password: None, max_players: 8, max_spectators: 4, admins: vec![] };
    let mut net = Net::hosted_with(content, setup, PeerRole::Host { lobby: cfg, listen: HostListen::off() });
    let a = net.join_all(&["Anna"])[0];
    net.ready(&[a]);
    net.host().peer().lobby().start(4).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() >= 200);
    net.peers[a].peer().leave();
    net.run_until(Duration::from_secs(8), "the host's match ends", |n| n.peers[0].has(ended));
    let rec = net.peers[0].peer().last_record().cloned().unwrap();
    let t = rec.gone.iter().find(|g| g.1 == 1).expect("Anna is withdrawn").0;
    // The end comes in the tick of the withdrawal; the record runs RECORD_AFTER_END
    // (300) more ticks, and a few for the end to be confirmed.
    assert!(rec.rows.len() <= (t + 1 + rl_online::RECORD_AFTER_END + 20) as usize, "ended at once, not at the limit");
    assert!(net.peers[0].has(|e| matches!(e, OnlineEvent::Left(n) if n == "Anna")));
    let end = net.peers[0].events.iter().find_map(|e| match e {
        OnlineEvent::MatchEnded(end) => Some(end.clone()),
        _ => None,
    });
    assert_eq!(end.unwrap().reason, "last worm standing");
}

/// N86: a member whose link times out is "timed out" to the others.
#[test]
fn a_member_that_times_out_is_said_to() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.5);
    let clients = net.join_all(&["Anna", "Bert"]);
    net.ready(&clients);
    net.host().peer().lobby().start(9).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() >= 100);
    net.peers[clients[1]].frozen = true; // alive, silent: no Bye, no close
    net.run_until(Duration::from_secs(14), "the host times Bert out", |n| {
        n.peers[0].has(|e| matches!(e, OnlineEvent::TimedOut(x) if x == "Bert"))
            && n.peers[clients[0]].has(|e| matches!(e, OnlineEvent::TimedOut(x) if x == "Bert"))
    });
    assert!(!net.peers[0].has(|e| matches!(e, OnlineEvent::Left(x) if x == "Bert")));
}
```

`RECORD_AFTER_END` is `pub` in `rl-online/src/peer.rs`; re-export it at the crate root if it is not already.

`crates/rl-online/tests/migration.rs`: a leave during a migration, over real connections:

```rust
/// N86, N88, Review Focus 2: Cid leaves while the match moves from a frozen
/// host to Anna; Anna and Bert remove Cid's worm and the old host's, their
/// records and replays agree, and only Cid is said to have left (the old
/// host goes silently, ruling PF12d-3).
#[test]
fn a_player_leaving_during_a_migration_is_removed_at_the_resume() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.15);
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", direct());
    let c = net.join_with_migration("Cid", direct());
    net.ready(&[a, b, c]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 300);
    let sc = net.peers[c].started()[0].unwrap();
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(5), "Bert finds the host gone", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrating(_)))
    });
    net.peers[c].peer().leave();
    net.peers[c].frozen = true;
    net.run_until(Duration::from_secs(12), "both resume under Anna", |n| {
        n.peers[a].has(migrated) && n.peers[b].has(migrated)
    });
    net.run_until(Duration::from_secs(25), "the match ends and both make replays", |n| {
        [a, b].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let ra = net.peers[a].peer().last_record().cloned().unwrap();
    let rb = net.peers[b].peer().last_record().cloned().unwrap();
    assert_eq!(ra, rb);
    let slots: Vec<u8> = ra.gone.iter().map(|g| g.1).collect();
    assert!(slots.contains(&0) && slots.contains(&sc), "the old host and Cid: {:?}", ra.gone);
    for i in [a, b] {
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))));
        assert!(net.peers[i].has(|e| matches!(e, OnlineEvent::Left(x) if x == "Cid")));
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::Left(x) if x == "Host")), "the old host goes silently");
    }
}
```

`crates/rl-game/tests/online.rs`: `hosting_and_joining_from_the_shell`'s two `"Anna left"` asserts become `"Anna has left"`.

- [ ] **Step 2: Run them; they fail:**
  - the lobby test fails to compile (`no method time_out`, `no variant TimedOut`);
  - `a_member_that_times_out_is_said_to` fails to compile too;
  - the rl-game text assert fails;
  - the last-opponent and migration tests may already pass after Task 4. They pin it: run each once with Task 4's `fill` change reverted locally, to see them fail, then restore.

- [ ] **Step 3: Implement** as the Context says.

- [ ] **Step 4: Run them; they pass:**

  ```bash
  cargo nextest run -p rl-proto -p rl-lobby && RL_CONTENT_DIR=$PWD/content/packs cargo nextest run -p rl-online -p rl-game
  ```

- [ ] **Step 5: Records.**
  - `crates/rl-lobby/CLAUDE.md`, Leaving: "`Bye`, `disconnected`: `Notice(Left)`; `timed_out`: `Notice(TimedOut)`; during a match the slot's worm is withdrawn (rl-netcode)". This replaces "during a match the slot stays in the match".
  - `crates/rl-online/CLAUDE.md`: `OnlineEvent::TimedOut` and which events time out; `forget_peer_as`; the new tests in its Test paragraph.
  - `crates/rl-game/CLAUDE.md`: "`Left` ("name has left"), `TimedOut` ("name has timed out")".

- [ ] **Step 6: Commit** (`rl-lobby, rl-online, rl-game: say who left and who timed out`, the trailers) after `cargo xtask check > /tmp/check.log 2>&1 &&`.

---

### Task 6: the scoreboard, the results and the watcher's camera

**Files:**
- Modify: `crates/rl-ui/src/hud/model.rs` (`Row::left`, `rows` leaves left rows out, `Results::new` keeps them, "left" as their lives), `crates/rl-ui/src/hud/paint.rs` (`table` greys a left row), `crates/rl-ui/CLAUDE.md`
- Modify: `crates/rl-game/src/session.rs` (`follow_next` skips gone worms; a followed worm that goes is left for the next), `crates/rl-game/tests/{online.rs, watch.rs}`, `crates/rl-game/CLAUDE.md`

**Interfaces:**
- Consumes: `Score::left`, `World::gone` (Task 1); `Recorder::withdraw` and `Cursor::step` (Task 2); Task 5's shell.
- Produces:
  ```rust
  // rl_ui::hud
  pub struct Row { /* …, then: */ /// The worm left the match: a results row only, its lives "left" (N85).
      pub left: bool }
  /// The live scoreboard: the mode's rows but those of worms that left.
  pub fn rows(scores: &[Score], names: &Names, followed: Option<i32>) -> Vec<Row>;
  ```

Context:
- `rows` filters `!s.left`.
- `Results::new` builds every row with a private `row(s, ..)` helper, setting `left: s.left` and `lives: "left"` for one.
- `table` paints a left row in `Color32::from_gray(140)`.
- `Session::follow_next(step)` walks from the followed worm in `step`'s direction to the first worm whose bit in `world.gone()` is clear. If every worm is gone it stays.
- After each tick's bookkeeping, a session with no local slot (a watcher) whose followed worm is gone calls `follow_next(1)`. Do it in `after_tick`, so a replay and an online match both get it.

- [ ] **Step 1: Write the failing tests.**

`crates/rl-ui/src/hud/model.rs`'s tests, with a local `Score` maker:

```rust
fn score(worm: i32, kills: i32) -> Score {
    Score { worm, score: kills, lives: -2, kills, deaths: 1, suicides: 0, damage: 0.0, left: false }
}

#[test]
fn a_left_row_is_off_the_scoreboard_and_left_in_the_results() {
    let names = Names(vec!["A".into(), "B".into()]); // as the file's other tests make them
    let mut scores = [score(0, 3), score(1, 1)];
    scores[1].left = true;
    scores[1].lives = -1;
    let live = rows(&scores, &names, Some(0));
    assert_eq!(live.iter().map(|r| r.worm).collect::<Vec<_>>(), [0]);
    let r = Results::new(MatchResult { winner: 0, reason: EndReason::TimeLimit }, &scores, &names, None, Some(0));
    let left: Vec<(i32, &str, bool)> = r.rows.iter().map(|r| (r.worm, r.lives.as_str(), r.left)).collect();
    assert_eq!(left[1], (1, "left", true));
    assert_eq!((r.rows[1].kills, r.rows[1].deaths), (1, 1), "its counts so far");
}
```

`crates/rl-game/tests/watch.rs`: a recorded replay with a withdrawal, watched through the real replay path (`setup::load_replay`, the session's `Cursor::step`):

```rust
/// A replay of three bots (Classic, Dirt Level, seed 5) in which worm 1 is
/// withdrawn before tick `k` (as an online match's record makes one, with
/// `Recorder::withdraw`), loaded as the Replays screen loads one.
fn replay_withdrawing(dir: &Path, k: u32, ticks: u32) -> Assets {
    let content = rl_content::Content::open(dir).unwrap();
    let o = DeathmatchOptions { bots: 3, spectate: true, seed: 5, ..DeathmatchOptions::default() };
    let built = content.build_match(&setup::match_options(&o, "olx-classic", "olx-level-dirt-level")).unwrap();
    let (mut world, mut rec) = (built.world, built.recorder);
    let (keys, mut out) = (vec![Keys(0); world.worms().len()], rl_sim::TickEvents::new());
    for t in 0..ticks {
        if t == k {
            rec.withdraw(&mut world, 1).unwrap();
        }
        world.tick(&keys, &mut out);
        rec.push(&keys, &world).unwrap();
    }
    let replay = rec.finish(&world).unwrap();
    let file = std::env::temp_dir().join(format!("rl-game-watch-gone-{}.rlrp", std::process::id()));
    std::fs::write(&file, replay.encode()).unwrap();
    let r = setup::load_replay(&[dir], &file, "default", &[]);
    std::fs::remove_file(&file).unwrap();
    r.unwrap()
}

/// N89: the watched worm leaving moves the camera on; following skips it.
#[test]
fn the_watched_worm_moves_on_when_it_leaves() {
    let Some(dir) = content_dir() else { return };
    let mut s = session(&replay_withdrawing(&dir, 100, 200));
    s.follow_next(1);
    assert_eq!(s.followed(), 1);
    while s.ticks() < 101 {
        s.tick(Keys(0));
    }
    assert_eq!(s.followed(), 2, "moved on");
    s.follow_next(-1);
    assert_eq!(s.followed(), 0, "worm 1 is skipped");
}
```

If `content.build_match` already started the recorder (rl-online's `make` pushes without `start`), keep it so. Otherwise call `rec.start(&world)` first.

`crates/rl-game/tests/online.rs`, through the shells (the real app path):

```rust
/// N85, user decision 2: Anna leaves mid-match; the host's log says so,
/// its live scoreboard has no Anna row, and its results keep one, "left",
/// with her counts. RED: Anna's row stays on the scoreboard.
#[test]
fn a_player_who_leaves_is_off_the_scoreboard_and_left_in_the_results() {
    let Some(dir) = content_dir() else { return };
    let (mut h, mut c) = host_and_client(&dir, 0.1, None);
    start_match(&mut h, &mut c);
    frames_until("the match runs", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        confirmed(s[0]).is_some_and(|(t, _)| t > 150)
    });
    c.act(UiAction::Back);
    c.act(UiAction::LeaveMatch);
    frames_until("the host says so", Duration::from_secs(5), &mut [&mut h], |s| {
        s[0].session().is_some_and(|x| x.log.texts().any(|l| l == "Anna has left"))
    });
    frames_until("Anna's worm is gone", Duration::from_secs(3), &mut [&mut h], |s| {
        s[0].session().is_some_and(|x| x.with_world(|w| w.gone()) & 0b10 != 0)
            && confirmed(s[0]).is_some_and(|(_, sc)| sc.iter().any(|r| r.worm == 1 && r.left))
    });
    let view = hud::view(h.session().unwrap(), &Frame { scores: true, ..Frame::default() });
    let names: Vec<String> = view.scoreboard.unwrap().iter().map(|r| r.name.clone()).collect();
    assert!(!names.contains(&"Anna".to_string()), "{names:?}");
    frames_until("the results", Duration::from_secs(15), &mut [&mut h], |s| s[0].ui.screen() == Screen::Results);
    let r = h.session().and_then(|s| s.results.as_ref()).unwrap();
    let anna = r.rows.iter().find(|r| r.name == "Anna").expect("Anna's row");
    assert!(anna.left && anna.lives == "left");
}
```

- [ ] **Step 2: Run them; they fail** (`no field left` on `Row`; the watch test's `followed` stays 1): `cargo nextest run -p rl-ui && RL_CONTENT_DIR=$PWD/content/packs cargo nextest run -p rl-game`.

- [ ] **Step 3: Implement** as the Context says.

- [ ] **Step 4: Run them; they pass.** The UI image snapshots (`fixtures/ui/*.png`) are unchanged: no snapshot has a left row. If one changes, a filter is wrong. Never bless.

- [ ] **Step 5: Records.**
  - `crates/rl-ui/CLAUDE.md`: the live scoreboard without left rows; the results with them.
  - `crates/rl-game/CLAUDE.md`: the watcher's camera (N89); the test names.

- [ ] **Step 6: Commit** (`rl-ui, rl-game: a player who left is off the scoreboard and "left" in the results`, the trailers) after `cargo xtask check > /tmp/check.log 2>&1 &&`.

---

### Task 7: `rl-cli`: net-sim bundles, `net-repro`, the crash window, and client departures in net-soak

**Files:**
- Modify: `crates/rl-cli/src/bundle.rs` (`DesyncBundle::gone`, `BUNDLE_VERSION = 3`, `reproduce` withdraws)
- Modify: `crates/rl-cli/src/netsim.rs`:
  - `BundleRecorder::on_event` fills the bundle's `gone` with `session.input_log().gone_marks(log.frontier())`, where it builds the `DesyncBundle` and again where `StateLoaded` refreshes `inputs`;
  - `excused_windows` gets the client-crash windows;
  - `judge` takes the windows path when the spec has a client crash;
  - its unit tests.
- Modify: `crates/rl-cli/src/soak.rs` (a client departure in a quarter of the cases), `crates/rl-cli/tests/{net_sim.rs, net_soak.rs, net_repro.rs}`, `crates/rl-cli/CLAUDE.md`

**Interfaces:**
- Consumes: Tasks 3-4 (`InputLog::gone_marks`, `Simulation::withdraw`, the harness).
- Produces: `DesyncBundle { …, gone: Vec<(u32, u8)> }`.

Context:
- **`reproduce`** withdraws `(t, s)` before `sim.tick` at `t`, as the harness's offline replay does.
- **The crash window** (N87, ruling PF12d-5). For each exit `(peer, tick, "crash")` of a client (`peer` in `1..=clients`), `excused_windows` adds:

  ```rust
  // N87: the host fills a crashed client's slot with its last keys until
  // its timeout, then withdraws it; players whose link cannot bear the
  // fills stall meanwhile (`fills_stall`), and recover after.
  let timeout = (SILENCE_TIMEOUT_US / TICK_US) as u32;
  let recovery = slowest_profile(spec, peer).outage_recovery_ticks(timeout, CLIENT_FRAME_US).max(MAX_PREDICTION);
  windows.push(Window { ticks: tick..=tick.saturating_add(timeout).saturating_add(recovery), peer: Some(peer) });
  ```

  `peer: Some(crashed)` excuses the crashed peer and everyone whose slowest link `fills_stall`, as an outage's window does. An exit's tick is a host tick already: no `host_tick_at`. `judge`'s guard gains `|| spec.exits.iter().any(|e| e.0 != 0 && e.2 == "crash")`.
- **`soak::generate`**: after the host's draw, `if clients >= 2 && r.below(4) == 0`, push `(1 + r.below(u64::from(clients)) as u32, tick, how)`:
  - the tick is drawn as the host's is;
  - `how` is `"leave"` or `"crash"` (one draw).

  It comes after every existing draw, so each seed's earlier fields are unchanged. The kept cases in `fixtures/net/cases/` are full specs, not seeds, and are unaffected.
- **The rest of the verdict** is the harness's (Task 4). A departed client's last stall episode is already not judged (it went). Client exits turn `n66` off for those cases (its `clean` needs host exits only). That is accepted: the cases without one keep it.

- [ ] **Step 1: Write the failing tests.**

`crates/rl-cli/src/netsim.rs`'s unit tests, beside `only_stalls_within_an_outage_are_excused`, with its twin:

```rust
/// N87, ruling PF12d-5: a client crash excuses, from its tick to the host's
/// timeout plus the recovery, the stalls of every player whose link cannot
/// bear the host's fills for its slot (`bad`); the same stall with no crash
/// in the spec, or past the window, fails.
#[test]
fn a_client_crash_excuses_the_fills_stalls_until_its_timeout() {
    let crash = NetSpec { profile: "bad".into(), exits: vec![(1, 300, "crash".into())], ..spec() };
    let judge = |s: &NetSpec, stalls: &[(PeerId, u32, Option<u32>)]| {
        let mut r = empty_report();
        r.exited.insert(1);
        let e: Vec<StallEpisode> =
            stalls.iter().map(|&(peer, tick, ended)| StallEpisode { peer, tick, ended }).collect();
        stalls_within_windows(s, &r, &e)
    };
    let timeout = (SILENCE_TIMEOUT_US / TICK_US) as u32;
    assert!(judge(&crash, &[(2, 400, Some(460)), (HOST, 900, Some(950))]).is_ok());
    let none = NetSpec { exits: vec![], ..crash.clone() };
    assert!(judge(&none, &[(2, 400, Some(460))]).is_err(), "the twin: no crash, no window");
    let leave = NetSpec { exits: vec![(1, 300, "leave".into())], ..crash.clone() };
    assert!(judge(&leave, &[(2, 400, Some(460))]).is_err(), "a leave is withdrawn at once: no fills");
    assert!(judge(&crash, &[(2, 300 + timeout + 400, Some(300 + timeout + 410))]).is_err(), "past it");
    assert!(judge(&crash, &[(2, 299, Some(310))]).is_err(), "before it");
}
```

`crates/rl-cli/tests/net_sim.rs` (the file's `spec(profile, seed)` and `pass`):

```rust
/// Plan 12d on the real simulation: a client leaving mid-match is withdrawn
/// at one tick on every peer, the verdict's offline replay (the real world)
/// agrees, and the world after it holds the worm out. Coverage: green since
/// Task 4.
#[test]
fn a_client_leave_withdraws_its_worm_in_a_real_match() {
    let Some(c) = content() else { return };
    let s = NetSpec { ticks: 800, exits: vec![(2, 300, "leave".into())], ..spec("typical", 4) };
    let r = pass(&c, &s);
    let t = r.report.host_gone.iter().find(|g| g.1 == 1).expect("client 2 is slot 1").0;
    let mut f = rl_cli::netsim::WorldFactory::new(&c, &s).unwrap();
    let mut sim = rl_netcode::harness::Factory::sim(&mut f, 0);
    for (tick, row) in r.report.host_log.iter().enumerate().take(t as usize + 1) {
        if tick as u32 == t {
            rl_netcode::Simulation::withdraw(&mut sim, 1);
        }
        let keys: Vec<rl_sim::Keys> = row[..usize::from(s.slots)].iter().map(|&k| rl_sim::Keys(k)).collect();
        rl_netcode::Simulation::tick(&mut sim, &keys);
    }
    let w = sim.world();
    assert!(!w.worms()[1].alive && w.gone() == 0b10);
}

/// N87, ruling PF12d-5: a client that crashes on `bad` stalls the other
/// players for its timeout's fills; the verdict excuses exactly that.
/// RED before the window: "player peer 2 stalled".
#[test]
fn a_client_crash_on_bad_passes() {
    let Some(c) = content() else { return };
    pass(&c, &NetSpec { ticks: 1_600, exits: vec![(1, 300, "crash".into())], ..spec("bad", 5) });
}

/// A net-sim desync bundle carries the withdrawals, and `net-repro`
/// withdraws: its replay meets the host's state, and the planted fault's
/// divergence comes at the fault, not at the withdrawal near 200. RED: the
/// bundle has no `gone`, and the replay diverges at the withdrawal.
#[test]
fn a_bundle_with_a_withdrawal_reproduces_to_the_fault() {
    let Some(c) = content() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = NetSpec { ticks: 800, exits: vec![(2, 200, "leave".into())], faults: vec![(1, 500, 0, 1.0)], ..spec("typical", 6) };
    let r = rl_cli::netsim::run_with(&c, &s, Some(dir.path())).unwrap();
    let path = r.bundles.first().expect("the planted desync wrote a bundle");
    let b = rl_cli::bundle::DesyncBundle::decode(&std::fs::read(path).unwrap()).unwrap();
    assert!(b.gone.iter().any(|g| g.1 == 1));
    let repro = rl_cli::bundle::reproduce(&c, &b).unwrap();
    assert!(repro.first_local_divergence.is_some_and(|t| t >= 500), "{:?}", repro.first_local_divergence);
    assert!(repro.host_diff.is_none(), "{:?}", repro.host_diff);
}
```

`crates/rl-cli/tests/net_repro.rs`: its two `DesyncBundle` literals gain `gone: vec![]`.

`crates/rl-cli/tests/net_soak.rs`, `cases_are_deterministic_and_within_bounds`:
- Match on the host's exits only, so that a client exit no longer panics the match: `let host_exits: Vec<_> = spec.exits.iter().filter(|e| e.0 == 0).cloned().collect();` and `match (host_exits.as_slice(), spec.host_freeze)`.
- Count the client departures apart, each within the middle 60 % and naming a client:

```rust
        for e in spec.exits.iter().filter(|e| e.0 != 0) {
            assert!((1..=u32::from(spec.clients)).contains(&e.0) && middle.contains(&e.1), "{e:?}");
            departures += 1;
        }
    // after the loop:
    assert!((25..=75).contains(&departures), "about a quarter of the cases: {departures}");
```

- [ ] **Step 2: Run them; they fail:**
  - `no field gone` on `DesyncBundle`;
  - no crash window: the unit test and `a_client_crash_on_bad_passes` fail;
  - no departures in soak cases.

  ```bash
  RL_CONTENT_DIR=$PWD/content/packs cargo nextest run -p rl-cli --lib --test net_sim --test integration
  ```

- [ ] **Step 3: Implement** as the Context says.

- [ ] **Step 4: Run them; they pass.** The kept net cases (`tests/net_cases.rs`) pass too.

- [ ] **Step 5: Records.** `crates/rl-cli/CLAUDE.md`:
  - `BUNDLE_VERSION` 3 ("3: withdrawals, plan 12d");
  - the crash window beside the outage windows (N87, PF12d-5), with its negative twin;
  - net-soak's client departures;
  - the new tests.

- [ ] **Step 6: Commit** (`rl-cli: net-sim knows departing clients: bundles, the crash window, net-soak`, the trailers) after `cargo xtask check > /tmp/check.log 2>&1 &&`.

---

### Task 8: records, and the single `full` run

**Files:**
- Modify: `docs/porting/STATUS.md`, `CHANGELOG.md`, `docs/online.md`, `README.md` (only if it describes leaving), and every crate `CLAUDE.md` a task missed

- [ ] **Step 1: `docs/porting/STATUS.md`.**
  - **Amend N6** in "Networking decisions (plan 9)". Keep its text and add: "Amended by plan 12d (N83): the slot is still filled with `Keys(0)`, and from its first unknown tick its worm is withdrawn from the match (`World::withdraw`); the user decided removal over a bot (2026-10-15)."
  - **Amend N72** (host migration decisions, "Its slot stays `Keys(0)` for the rest of the match"): a late rejoiner's slot is withdrawn at the resume (N88), and its worm leaves the match there.
  - **Add a section** "Leaving players decisions (plan 12d)" with:
    - the user's five decisions (dated 2026-10-15);
    - the rulings PF12d-1 to PF12d-5;
    - N82-N89 as this plan states them, each with its OLX citation and its cost if wrong;
    - the format bumps (replay 3, game bundle 2, net-sim bundle 3, the fingerprint; protocol 2).
  - **Under "Deliberate deviations"**, add: "Results keep a 'left' row (N85, user decision 2026-10-15): OLX deletes a leaving client's worm (`Game::removeWorm`, `src/game/Game.cpp:1359-1366`), so its results lose it; ours keep its row, its lives 'left', its kills and deaths as it left. The live scoreboard leaves it out, as OLX."
  - **Add a porting-table row** for "Players leaving mid-match":
    - OLX: `CServer.cpp:1352-1482`, `CGameMode.cpp:254-263`, `CGameMode.cpp:316-336`, `CClient_Parse.cpp:1650-1680`;
    - ours: `rl-sim::World::withdraw`, `rl-netcode`'s marks, `rl-online`, `rl-game`;
    - status: done (plan 12d);
    - tests: the tests of Tasks 1-7 by name.

- [ ] **Step 2: `CHANGELOG.md`**, under "Unreleased", a "Fixed: players leaving a match" entry:
  - A player who leaves, times out or is kicked is removed from the match at once on every screen, as in OpenLieroX: no frozen worm left to shoot.
  - The log says "<name> has left" or "<name> has timed out".
  - If only one worm is left, the match ends.
  - The final results keep a "left" row with the player's kills and deaths.
  - Replays show it.
  - A crashed player's worm keeps its last move for up to 10 s, then goes.
  - Under "Changed": replays and desync bundles from earlier builds no longer open, and earlier builds cannot join (proof-of-concept stage).

- [ ] **Step 3: `docs/online.md`.**
  - **Add a short "When a player leaves" section** before "When the host leaves", in the file's voice. It covers:
    - what players see (the line, the worm gone, the scoreboard, the results' "left" row, the match ending when one worm is left);
    - the 10 s for a player whose game crashed or lost its network;
    - that a player leaving while the host is being replaced is removed when the match resumes.
  - **Amend the late rejoiner's text** in "When the host leaves" ("its worm stays idle for the rest of this one"): its worm left the match at the resume, and it watches until the next one.

- [ ] **Step 4: Check every touched crate's `CLAUDE.md`** against its code once more: rl-sim, rl-modes, rl-replay, rl-proto, rl-netcode, rl-lobby, rl-online, rl-ui, rl-game, rl-cli. These texts become false and must be fixed if a task has not already:
  - `crates/rl-online/CLAUDE.md`'s late rejoiner: "Its old slot stays `Keys(0)` (abandoned at the resume)" becomes "its old slot's worm is withdrawn at the resume (N88)".
  - `crates/rl-lobby/CLAUDE.md`'s Leaving: "during a match the slot stays in the match".

- [ ] **Step 5: The single `full` run** (user rule: once per plan, here):

  ```bash
  cargo xtask full > /tmp/full.log 2>&1; tail -60 /tmp/full.log
  ```

  It runs:
  - check;
  - the timed bot grid (alone, before the online tests);
  - the full-tier tests (`FULL_TESTS`: `net_sim`'s and `transport`'s grids, rl-net's link tests, rl-game's integration tests);
  - the device tier;
  - `e2e` on Xvfb (`fixtures/autoplay/online.ron` plays an online match through the real app);
  - `net-nat`;
  - the `net-feel` gate;
  - the determinism windows;
  - the benches and the short stress run.

  What to expect and do:
  - **Expected:** all pass.
  - **Feel drift:** the `gone` list adds one byte to each datagram. If the feel gate's bytes per second move past its tolerance on that alone, re-bless it with `cargo xtask net-feel --bless --reason "plan 12d: a withdrawal list in each datagram, one byte when empty"` (ruling PF12d-4), and report the reason. If it does not move, bless nothing.
  - **Anything else** failing is a bug of this plan: fix it, and run `full` again only after that fix.

- [ ] **Step 6: Commit** (`docs: players leaving a match (plan 12d)`, the trailers) after `cargo xtask check > /tmp/check.log 2>&1 &&`.
