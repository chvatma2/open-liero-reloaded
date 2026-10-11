# SDD ledger — plan: docs/superpowers/plans/2026-10-10-plan-6-deathmatch-and-bots.md

Ruling: Work directly on main — user's standing choice — cost if wrong: commits need moving to a branch.
Ruling: Plan specifies interfaces, OLX references, named tests and acceptance (as plans 4–5) — user asked to finish without stopping — cost if wrong: more implementer judgement, caught by reviews.
Ruling: Execution via subagent-driven-development — cost if wrong: token cost.

## Pre-flight scan
| Tasks | Produces vs consumes | Finding |
|---|---|---|
| T1→T2,T5,T6,T7,T8 | MatchSetup, GameMode, ModeCtx commands, Message/GameOver | consistent |
| T3 vs T4 | T3 writes crates/rl-bots/src/loadout.rs; T4 creates the rl-bots crate | CONFLICT → Ruling P1 |
| T6 self | Replay::play(&Content): rl-replay is L3, rl-content is L4 | CONFLICT → Ruling P2 |
| T8 vs plan 5 | T8 predates rl-present (the shared Presentation bundle) | → Ruling P3 |
| T4→T5 | Bot, Motor, AreaCache, budget hook | consistent |
| T7 self | goldens + bless-regression; blessings.log shared with bless-render | consistent |
| T2 self | optional oracle scenario dm-kill-limit | consistent (implementer reports if unobservable) |
| T1 self | NinjaRope::throws now in checksum (plan 5) — any golden checksum streams are new in T7 | consistent |

Ruling P1: Task 3 creates the rl-bots crate skeleton (L3, no_std, layers.ron, CLAUDE.md) with loadout.rs; Task 4 extends it — cost if wrong: none.
Ruling P2: rl-replay (L3) holds the format, Recorder and a verifying player that takes a ready World (or a World factory closure) — content resolution (pack ids → hashes → MatchSetup) lives in rl-content (L4) as `Content::world_for_replay(&ReplaySetup)`; plan text "Replay::play(&Content)" is satisfied by that composition — cost if wrong: one function move.
Ruling P3: Task 8 builds the Deathmatch and replay sessions on rl-present's Presentation bundle (add_worm for mid-match spawns, drift check) — cost if wrong: none.
Task 1: dispatched (BASE 417b7a0, implementer opus)
User playtest (2026-10-10): sandbox good; drag-resize/maximize broken and drag-resize crashed. User decision: window non-resizable, no maximize; size changes via the future menu. Hotfix task queued after plan 6 Task 1 (rl-game conflicts): non-resizable window + root-cause the resize crash (fullscreen/DPI share the path).
Playtest crash output (user): GPU D3D12 GL Fifo Bgra8Unorm, then "Io error: Broken pipe (os error 32)" ×2, "error: the event loop: Exit Failure: 1" — WSLg Wayland compositor dropped the connection during drag-resize (not a Rust panic; same class as the plan-5 probe's "Connection reset by peer"). Hotfix scope: (1) non-resizable window, maximize disabled (user decision); (2) reproduce with scripted rapid resizes under Wayland vs X11 (libxkbcommon-x11-0 installed) and pick the robust backend on WSL for fullscreen/DPI changes; (3) a lost display connection exits with a clear message.
Task 1: implementer DONE_WITH_CONCERNS (commit b3cb807); review dispatched (opus)
Hotfix (window): dispatched (BASE b3cb807, implementer opus) — runs while Task 1 is under (read-only) review.
Task 1: review Needs fixes — 1 Important (check_game_over swaps in a Vacant mode so is_over sees a placeholder world; is_over path untested). Fix round 1 queued until the window hotfix finishes (avoid concurrent commits); it also carries Minor 2 (mode_state as_any for Task 8) and optionally Minor 3 (no Spawn after game over, OLX SpawnWorm returns when gameOver).
Task 1: note for Task 2: OLX CheckGameOver runs only when recheckGame is set (CServer_Game.cpp:170-176); rl-sim calls is_over every frame — Deathmatch models any one-frame delay.
Task 1: note for Task 6: MatchSetup/WormSetup/Loadout need serde for replays; post-new setup calls (set_health, place_bonus, align_physics_clock) must be in the replay if used.
Task 1: minor (deferred): on_damage fires for healing (negative dealt); no tests of on_damage/on_pickup/scores args; harm() test helper leaks a Box; bonus.rs 512 lines.
Hotfix (window): implementer DONE (commit 38dd8a1). Root cause: WSLg's Weston 9 segfaults (client-side decorations' shm pool growth during resizes 20–50 ms apart); fixed-size window + game-initiated size changes coalesced and ≥250 ms apart → 0/7 crashes; X11 never crashes but no vsync → opt-in (--window-system x11 / RL_WINDOW_SYSTEM). Review dispatched (sonnet). Task 1 fix round 1 dispatched (resumed implementer).
Hotfix (window): review Needs fixes — Important: SurfaceState::current drops every Suboptimal frame (configured reset to None), possibly a window that never presents; the stale-frame filter is dead defence in this single-threaded loop → remove or fix + Suboptimal test. Fix round queued after Task 1's fix round (avoid concurrent commits); also carries Minor 3 (seed the 250 ms interval at window open).
Hotfix (window): minor (deferred): pending size changes wait while hidden (document); ExitFailure-without-game-error message asserts the Weston cause (soften); attribute test checks fields not behaviour; compositor-initiated resizes (snap) and scale-factor changes rely on min=max + pacing only.
Task 1: fix round 1 implementer DONE (commit ea2c71f); scoped re-review dispatched (sonnet).
Hotfix (window): fix round 1 dispatched (resumed implementer).
Task 1: fix round 1/5 (1 Important + 2 carried addressed, 0 open; commits 38dd8a1..ea2c71f)
Task 1: minor (deferred): long spec line; no_spawn_after_the_end test bypasses tick.
Task 1: complete (commits 417b7a0..ea2c71f (incl. b3cb807, ea2c71f; 38dd8a1 is the window hotfix), review clean after 1 fix round)
Hotfix (window): fix round 1 implementer DONE (commit 2e40d88); scoped re-review dispatched (sonnet).
Task 2: dispatched (BASE 2e40d88, implementer opus)
Hotfix (window): fix round 1/5 (2 Important + 2 Minor addressed, 0 open; commits ea2c71f..2e40d88)
Hotfix (window): complete (commits 38dd8a1, 2e40d88; review clean after 1 fix round)
Task 2: implementer DONE (commits 702544a, 914dcb6; dm-kill-limit oracle scenario matches OLX bit for bit, 421 frames; also fixed Task 1's game-over freeze timing (ms*0.001f > 3)); review dispatched (opus)
Task 2: note: kill messages carry weapon -1 (rl-sim does not track the injuring weapon) — HUD kill feed in Task 8 shows no weapon unless rl-sim adds it.
Task 2: review Approved, no Critical/Important. ⚠️ resolved: controller ran cargo xtask check at 914dcb6.
Task 2: Ruling R4: chaos loadout rotation (F10) is a Deathmatch setting fixed at setup (e.g. `respawn_loadout: RespawnLoadout { Keep | Cycle { per-worm rotation over the mod's weapons } }`), applied by Deathmatch::on_spawn via give_weapon — bots only press keys, so replays stay keys-only; Task 5 consumes it — cost if wrong: a small API move.
Task 2: minor (deferred → carried into Task 8): time_left_ms keeps counting after the end; freeze it at the end frame for the HUD.
Task 2: minor (deferred): two sources of rules (Deathmatch settings vs SimSettings) — read lives/kill limit from world settings or debug_assert agreement; on_tick recomputes the end with its own frame counter (expose server_frame on ModeCtx); misplaced test comment; end texts lost first if the 128-command queue fills.
Task 2: complete (commits 2e40d88..914dcb6, review clean)
Task 3: dispatched (BASE 914dcb6, implementer opus)
Task 3: implementer DONE (commit af2d48e; bonus-natural-default oracle scenario matches; pack format v3); review dispatched (sonnet). R4 note: chaos rotation goes in rl-modes (Task 5).
Task 3: review Approved, no Critical/Important. Controller: cargo xtask check 22 s at af2d48e.
Task 3: minor (deferred): loadout tests re-derive picks with the same formula (pin a glibc value); rl-scenario re-reads the whole mod pack per scenario for weapon_rules (gate still 22 s); O(weapons×list) restriction updates; choose_loadout may return -1 slots (document for Task 5/8); fixtures.rs 511 lines.
Task 3: complete (commits 914dcb6..af2d48e, review clean)
Task 4: dispatched (BASE af2d48e, implementer opus)
Task 4: implementer DONE_WITH_CONCERNS (commit ff20fc4); review dispatched (opus). Notes: shared AreaCache requires fixed think order and one cache per match; budget raised to a level minimum on big maps.
Task 4: review Approved, no Critical/Important.
Task 4: minor (deferred → carried into Task 5): search restarts on any goal move (port AI_CreatePath conditions: 5 s / worm-line tests / path extension / 0.5 s re-search; fix the citation); jump check starved by the lookahead's budget reservation (reserve or run it first; no go-back when the forward scan was cut short); with_cache shares caches between same-sized levels (store a level identity); aim needs parent velocity + speed variance and OLX's gravity (×5) / speed (×20) noise.
Task 4: minor (deferred → Task 7 p99 watch): linear AreaCache::find under-charged and fill_checks uncharged; min_budget uncapped on huge levels; duplicate areas when budget runs out; CacheFull leaves regions unreachable (document).
Task 4: minor (deferred): not test-first (RED by stubs).
Task 4: complete (commits af2d48e..ff20fc4, review clean)
Task 5: dispatched (BASE ff20fc4, implementer opus)
Task 5: implementer DONE_WITH_CONCERNS (commit b7102a3; chaos fires 38/38 + 50/50; Xtreme median kill 25.7 s on Dirt Level); review dispatched (opus)
Task 5: review Approved (no Critical/Important as graded). Controller re-graded Minor 2 (spot search starves the jump check → bots stuck under ledges) and Minor 3 (dig-on pushes into rock → bots flail at rock walls) to Important by player effect. Fix round 1 dispatched (resumed implementer) incl. minors 1 (SIGN -1/0/1), 6 (relax kill-time max), 9 (ChaosCycle accessors for Task 6) and the STATUS left-fallback wording.
Task 5: minor (deferred): game type re-derived from slot names at Bot::new (record or take from config; frozen at construction); fly_clear treats unaffordable/long lobs as "cannot hit"; chaos test excludes specials; stuck handling unreached by tests; bonus seeking / rifle / mortar tactics untested; path extension under a budget cut falls through to a new search.
Task 5: fix round 1 implementer DONE (commit a7460cf; Xtreme kill 10–62 s, median 28.4 s vs 30 s test median — deterministic, re-measure on brain changes); scoped re-review dispatched (sonnet)
Task 5: fix round 1/5 (2 Important + 3 Minor addressed, 0 open; commits b7102a3..a7460cf)
Task 5: minor (deferred): pos_target one tick stale on the no-node path; jump-check test guards ordering by construction.
Task 5: complete (commits ff20fc4..a7460cf, review clean after 1 fix round)
Task 6: dispatched (BASE a7460cf, implementer opus)
Session restart (2026-10-11): Task 6 implementer was mid-work (uncommitted rl-replay crate + rl-content replay.rs); resumed the same agent to finish.
Disk (2026-10-11): previous session died — Windows C: 98% full (24 GB free) backing WSL's ext4.vhdx; inside WSL target/ 45 GB (deps 29 GB, incremental 12 GB) + Docker ~11 GB. Controller removed target/scratch, 6 superseded oracle images and the Docker build cache. Queued after Task 6: dev profile debug = line-tables-only (deps debug = false) + one cargo clean; oracle-record prunes superseded rl-olx-oracle images. User action suggested: sparse/compacted vhdx.
Task 6: implementer DONE_WITH_CONCERNS (commit e2e6f95; 8 bots × 1 min ≈ 12.6 KB); review dispatched (sonnet). The disk-full crash had zeroed target/content (re-imported by the implementer); controller verified the repo: clean tree, git fsck OK, no empty files outside target/.
Disk tooling: dispatched (BASE e2e6f95, implementer sonnet).
Task 6: review — 2 Important (no tick-0 checksum: setup/content divergence reported only at tick 100; play_replay/world_for_replay stringify typed errors so Task 7 cannot downcast Mismatch.tick). Fix round 1 queued until the disk tooling agent's cargo clean/rebuild finishes; also carries Minor 3 (finish checks frame - base == ticks; base from the world) and Minor 6 (NaN settings proptest / validation in decode).
Task 6: minor (deferred): replay_assets duplicates match_setup's assembly (share a helper); TOCTOU between hashing and loading packs; Recorder hard stop at 2 h (caller must handle); Replay fields pub; two miniz_oxide versions.
User (2026-10-11): no need to reclaim WSL space; goal is keeping the project folder from bloating to avoid mid-work failures. Disk agent extended: cargo xtask tidy (incremental dirs, stale artifacts, superseded oracle images, bounded build cache; never target/content or target/olx) + a size warning in cargo xtask check.
User (2026-10-11): keep debug info that is useful. Ruling: debug = line-tables-only for workspace AND deps (backtraces keep file:line everywhere; only debugger variable/type info dropped; symbols not stripped) — sent to the disk agent.
Disk tooling: DONE (commit 5387d1c): line-tables-only for workspace + deps; target 47 GB → 5.7 GB (after full rebuild + tidy); check after a one-file touch 26.7 s → 28.8 s; cargo xtask tidy; check warns > 25 GB; oracle-record prunes superseded images (pure tag choice unit-tested; live Docker pruning not exercised). Controller verified profile + size. (Tooling change outside the plan; not separately reviewed — small, verified by the gates and measurement.)
Task 6: fix round 1 dispatched (resumed implementer).
Task 6: fix round 1 implementer DONE (commit 4c13149); scoped re-review dispatched (sonnet). API for Task 7: Recorder::start before push; finish -> Result; checksums include tick 0.
Task 6: fix round 1/5 (2 Important + 2 Minor addressed, 0 open; commits 5387d1c..4c13149)
Task 6: complete (commits a7460cf..4c13149 (e2e6f95, 4c13149), review clean after 1 fix round)
Task 7: dispatched (BASE 4c13149, implementer opus)
Task 7: implementer DONE_WITH_CONCERNS (commits 322ed55, a76a1b6, c2179af; grid 38/38 + 50/50 weapons, 6/6 action kinds; p99 ≤ 0.29 ms release; check 23–29 s, full ~60 s); review dispatched (opus).
Task 7: Ruling R5: the match builder (bots + Deathmatch + MatchSetup assembly) moves to rl-content (L4) so rl-game (L6) and rl-cli (L6) share it — same-layer apps cannot depend on each other — carried into Task 8 — cost if wrong: a function move.
Task 7: note for Task 8/playtest: Deathmatch bots score few kills per minute — measure in Task 8's scripted runs.
Task 7: review Approved, no Critical/Important (health > 100 verified as faithful OLX: heals uncapped when shieldFactor != 0; MW has Injure damage -3).
Task 7: minor (deferred): replay hint tick for invariant failures; Budget failure returns before rec.push; Record-failure path can panic if Full (unreachable today); long-match test doesn't assert growth bounds; first_divergence untested on failing cases; bonus coverage keyed by string; kills by selected weapon (approximate); file errors lack path context; bless-regression --from weak check; root CLAUDE.md lacks bless-regression/fixtures/matches (Task 9); OLX file:line citations for injure in invariants.rs/STATUS.
Task 7: complete (commits 4c13149..c2179af, review clean)
Note: the scratchpad OLX checkout was wiped by the session restart (tmpfs). OLX source re-cloned durably at target/olx-src/d1ad953f (gitignored; tidy does not touch it). Future dispatches cite this path.
Task 8: dispatched (BASE c2179af, implementer opus)
Task 8: implementer DONE_WITH_CONCERNS (commits ab74f58 MatchBuilder in rl-content, 5eb4a59 rl-game Deathmatch + --replay); review dispatched (opus).
Task 8: concern: bots 1–7 kills/min windowed (mean 3.75), 2.3–6.7 headless; Xtreme ≈ Medium; Classic bots fire mostly the Chaingun (5-weapon 100lt loadout expected). Ruling: after the Task 8 review, a bot-strength investigation (systematic debugging) measures an OLX baseline (OLX dedicated server with bots in Docker, same levels/mods, kills/min) before any tuning — cost if wrong: a few hours.
Task 8: review Approved, no Critical/Important (MatchBuilder equivalence checked line by line; game over/freeze/results/--record on quit/rematch verified).
Task 8: minor (deferred → carried into Task 9): rematch keeps the previous tick's events (clear self.events in restart); Tab scoreboard sticks on focus loss (clear on Focused(false)).
Task 8: minor (deferred): exit-summary think stats cover only the last match; frame spikes unproven (likely screenshot readbacks); F5 fatal if a mode adds worms mid-match (latent); level decoded twice on --replay; replay's last frame freezes popups; flags silently ignored per mode.
Task 8: complete (commits c2179af..5eb4a59, review clean)
Task 9: waits for the bot baseline report (bot strength may need a fix task first).
User (2026-10-11): playtest of Deathmatch vs bots worked well, no issues. Bot difficulty direction: ≥3 levels; top difficulty from strategy, quick reactions, predictive reloading (teamwork later with team modes), NOT perfect aim (current Xtreme aim noise 0 → must change). Feeds the bot-strength follow-up after the OLX baseline.
Bot baseline: our bots match OLX kill rates within noise (Classic/MW × Dirt/Factory/Castle, Medium; OLX Xtreme ≈ Medium on Classic); difference is movement/firing cadence (motor deviations), not kills. Report copied to docs/porting/bot-baseline-2026-10-11.md (committed in Task 9).
Ruling R6: no bot strength fix in plan 6. Per the user's direction, Task 9 adds an aim-error floor so no difficulty aims perfectly (Xtreme > 0; a recorded deviation from OLX) and re-blesses goldens with a reason if needed; strategy/reaction/predictive-reload difficulty scaling and teamwork become a post-v0.1 bot-skill milestone measured with the baseline harness — cost if wrong: Xtreme slightly easier than OLX's.
Task 9: dispatched (BASE 5eb4a59, implementer opus).
Task 9: implementer DONE_WITH_CONCERNS (commits 99e2223 aim floor 1/6 + re-blessed goldens, 1b30869 rematch/Tab, 1916476 invariant for OLX-wedged worms, 99886fe docs); review dispatched (sonnet)
Task 9: review Approved, no Critical/Important. Controller ran cargo xtask full at 99886fe → passed (54 s; bot-grid 13.8 s); target 14 GB.
Task 9: minor (deferred): buried_in_rock lets shallow tunnelling (≤3 px into walls, ≤5 px floors/ceilings) pass — tighten to OLX's exact wedge state or document; xtreme kill-time median bound 65 s loose — assert Xtreme median < Hard's; hud.rs test import order; rl-game CLAUDE.md wrapping.
Task 9: complete (commits 5eb4a59..99886fe, review clean)
Final review: dispatched (opus), range 417b7a0..HEAD
Final review: no Critical/Important; ready for M7. Carried into plan 7 Task 1: single public Deathmatch constructor (prepare/build_match path); freeze time_left_ms at game over; scope `docker builder prune` to this project or make it opt-in (it pruned the machine-wide cache); serde/mirrors + validate for BotConfig/DeathmatchSettings; doc line: pending size changes wait while the window is hidden.
Final: minor (deferred → networking brainstorm): replay sim-version stamp; private replay fields; Bot clone shares Arc<AreaCache> (matters for prediction/rollback).
Final: minor (deferred): setup command queue (128) drops chaos rotations with ≥26 chaos bots and kill messages with ≥26 deaths in a tick; long-match growth assertions loose; per-bot rock_hash scan at match start; one human seat hard-coded as worm 0 (local multiplayer later); all other can-wait items per the triage.
Plan 6: COMPLETE (417b7a0..99886fe).
