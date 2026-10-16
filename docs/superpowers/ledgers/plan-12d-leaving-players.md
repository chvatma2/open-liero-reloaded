# SDD ledger — plan: docs/superpowers/plans/2026-10-15-plan-12d-leaving-players.md
Origin: the user's playtest report (a player who left mid-match stayed in game). User decisions 2026-10-15: remove the worm (OLX parity); keep a 'left' row in the final results; fewer than 2 worms ends the match; 10 s crash timeout stays; controller default: slots abandoned by a migration are removed at the resume tick. User: compatibility breaks are fine (proof of concept).
Plan commit: 5114219 (written by the investigating subagent; decisions N82–N89).
User rules: cargo xtask full once, in the last task; target/ tidy limit 40 GiB.
Ruling PL12d-0: accept N82–N89 without asking — within OLX parity and the user's decisions — cost if wrong: rework of one decision.
Pre-flight scan dispatched (opus, read-only). BASE: 6a3b273
User 2026-10-15: plan 13 (browser) dropped for now; revisit after game improvements. After 12d, report and ask what is next.
## Pre-flight scan (preflight.md, opus, 45 rows): 20 problems; the six user decisions all carried; 12c's final fixes break nothing the plan relies on.
Ruling PF12d-1: accept every proposed fix and have the plan author amend the plan before Task 1 — cost if wrong: plan rework only.
Ruling PF12d-2 (P1): keep N84's test, rewritten with a projectile in flight when its worm is withdrawn — the claim deserves its own test — cost if wrong: none.
Ruling PF12d-3 (P8): at a resume the old host is withdrawn silently (the migration lines already say it), players who did not come back get "<name> has left" — cost if wrong: one log line.
Ruling PF12d-4 (P16): withdrawal marks travel once per datagram as a compact (slot, tick) list, not a bit on every frame, so the feel bandwidth does not move; the feel gate is re-blessed with `cargo xtask net-feel --bless --reason` only if it does — cost if wrong: a slightly more complex encoder.
Ruling PF12d-5 (P14): a client crash's 10 s of host fills gets an excused window (crash tick to timeout + recovery) for the stalls it causes, with a negative twin — the fills are the designed behaviour — cost if wrong: a stall bug inside that window hides.
Plan amendment dispatched to the plan author (resume).
Plan amended 5c0ea3f (P1–P20 with PF12d-1..5). BASE T1: 5c0ea3f
Task 1: implementer dispatched (opus)
Task 1: implementer DONE_WITH_CONCERNS e1d585a (goldens/oracle unchanged; bundle FIELDS gains 'gone'; concern: a command a mode queues in on_drop is not carried by world state transfer — Deathmatch queues none). Review dispatched (opus). BASE 5c0ea3f
Task 1: review (opus) → spec ✅, Needs fixes: I1 (plan-mandated) a gone worm can win (winner/highest scan all worms; OLX HighestScoredWorm sees none removed). 3 minors. Ruling PF12d-6 (Minor 1): on_drop must not queue commands — enforced in withdraw, documented — carrying would put Command on the wire — cost if wrong: a future mode needs another hook. Fix round 1 dispatched (resume).
User playtest 2026-10-15: results show wrong deaths (10 lives; 3 worms show 11 deaths, the out one 7). Investigator dispatched (opus, Phase 1 only, scratch copy: no edits while Task 1's fix round edits rl-modes).
Task 1: fix round 1 done 4b92014 (gone worms never win: a_withdrawn_worm_never_wins RED winner 1 → 0; on_drop refuses commands (CommandError::Closed); clears test mutation-checked; out-of-range drop asserts). Accepted on RED evidence.
Task 1: complete (commits 5c0ea3f..4b92014).
BASE T2: 4b92014
Task 2: implementer dispatched (sonnet)
Deaths report: root cause — the results' Deaths/Suicides columns show OLX's iDeaths, which only grows while score > 0 (rl-sim damage.rs:73-98, 275-277; OLX CWorm.cpp:1430-1435, AllowNegativeScore off); 11 deaths with 10 lives is correct; OLX never displays iDeaths (CClient_Draw.cpp:1851-1950 shows Lives, Kills, Damage).
User decision 2026-10-15: count every death and suicide on the board; OLX's gated counters keep driving score, ranking and winner (oracle unchanged; match goldens re-blessed). Queued after Task 2 (a golden-changing edit must not overlap another task's check): resume the investigator to implement it with its proposed test.
Task 2: implementer DONE_WITH_CONCERNS b13a6f5 → amended abd05a9 (trailer). Concern: a withdrawal after the last pushed tick records t == ticks, refused by decode. Review dispatched (sonnet).
Deaths fix dispatched (resume investigator) in parallel with Task 2's review.
Task 2: review (sonnet) → spec ✅, Needs fixes: Recorder can emit a replay its decoder refuses (withdrawal after the last push). Ruling PF12d-7: decode accepts t <= ticks; Cursor applies trailing withdrawals before the final checksum — such a replay must stay valid (a leave after the final push is reachable) — cost if wrong: none. Fix round 1 dispatched (resume) with the 5 minors. Carried to Tasks 4–7: rl-cli transport.rs:1098 replay_checksums must route withdrawals.
Deaths fix: done 953565a (Deathmatch died/killed_self counted in kill, hashed, saved; board deaths/suicides from them; a left row keeps its deaths; local HUD reads the board; STATUS deviation entry; goldens re-blessed, checksum lines only). RED board_counts_every_death_and_suicide (0 → 2). Two rl-game tests now count Kill events exactly.
Final: minor (deferred): ingame::results_match_world_scores now asserts only board counts ≥ the worm's gated ones (weaker than exact).
Task 2: fix round 1 done 68c0088 (decode t <= ticks; Cursor applies trailing withdrawals before the final checksum; finish replaces a periodic checksum at ticks; mutation REDs per check; stepping test). Accepted on RED evidence.
Task 2: complete (commits 4b92014..68c0088).
BASE T3: 68c0088
Task 3: implementer dispatched (opus)
Task 3: implementer DONE_WITH_CONCERNS 8d51545 (223 tests; 19 mutations caught; net-feel ok, ~1 B per host datagram: typical 12799→12898 B/s down; Ahead keeps marks; a mark with no value in its frames dropped quietly). Carried to Task 4: a joined spectator learns no marks before its origin — the gone mask must come with the world state; nothing reacts to a mark yet. Review dispatched (opus).
Task 3: review (opus) → spec ✅, Approved pending a ruling on I1: a new host's report merge takes marks from any rejoined peer for any slot (N67's trust model), so a forged report could withdraw a live player once Task 4 acts on marks. 3 minors. Mid-plan net-feel run was the controller's request.
Ruling PF12d-8 (I1): reported marks are held until the resume; at the resume they apply only to slots whose player is not present (not rejoined and not the new host's own); a reported mark for a present player is refused and counted in report_conflicts — a genuine third-party mark only matters for absent slots, which the resume withdraws anyway; a forged mark must never remove a live player — cost if wrong: an absent slot's exact withdrawal tick may be the resume tick instead of the old host's — Task 4 implements it with a test.
Task 3: fix round for the 3 minors dispatched (resume).
Task 3: fix round done 5b24870 (stray mark dropped, pinned; reachable host test; one decode_marked). Accepted.
Task 3: complete (commits 68c0088..5b24870).
BASE T4: 5b24870
Task 4: implementer dispatched (resume the Task 3 implementer, opus) with carries PF12d-8, migration-abandoned slots, spectator mask via state, transport replay_checksums, Task 2 replay rule, 12c rules, crash timeout tick
User playtest 2026-10-15: ninja rope throw sounds spam the game (everyone's throws heard map-wide?). Investigator dispatched (opus): compare with OLX at d1ad953f; port OLX's rule if it limits; stop for a user decision if OLX also plays every throw.
Rope sound: fixed e55299b (OLX plays the throw sound only for the local human's own throws: CWormHuman.cpp:340-342, 555-563, 570-574; bots/remote none; ours played every worm's at the listener, 7.5–12/s with 8 bots). Throws heard: local player's; replay: the viewer's; bots alone / watched online: none. Other sounds already match OLX (positional, no distance cut). Open (user's call): online pickup sound plays for every worm on every peer; OLX only for local worms (CClient_Game.cpp:136-142). Process slip: check piped into tail before && commit (check had passed).
Task 4: implementer DONE_WITH_CONCERNS b8f88c8 (425 tests; 10 mutations; soak 20 runs 0 failures; PF12d-8 implemented; spectator via state; replay_checksums withdraws; awful upper bound 640 (lost Bye resent)). Review dispatched (opus). Note: e55299b (rope sound) sits inside this range; the package covers b8f88c8's files.
Task 4: review (opus) → spec ✅, Needs fixes: I1 PF12d-8's refusal of a fresh mark for a present player after the resume (migrate.rs:574-577) has no test (disabling it passes all 167); I2 an honest refused mark (a client timed out by the old host while alive, who then rejoins) costs a desync + one state repair at survivors that held the mark, not only Inconsistent. 5 minors; 640 bound justified (resent Byes in awful bursts).
Ruling PF12d-8a (I2): accept — a rejoined player is present and stays; survivors that withdrew it are repaired by the desync path (one state transfer); amend PF12d-8's cost in STATUS and pin the repair with a test (the survivor ends with the player present and checksums equal) — the case needs the old host to time out a live client and then vanish — cost if wrong: one repair and a desync bundle in that rare case.
Task 4: fix round 1 dispatched (resume): I1 test, I2 per PF12d-8a, 5 minors.
Task 4: fix round 1 done 1ba6e85 (post-resume forged mark refused, pinned; a held mark keeps its later values so a refused honest mark needs exactly one repair at holders; verdict accepts exactly that one repair; rejoiners no longer counted dropped; leave bound from the host's state at PeerLeft; one soak case in four has a client leave). Accepted on mutation evidence (8).
Task 4: complete (commits 5b24870..1ba6e85).
BASE T5: 1ba6e85
Task 5: implementer dispatched (sonnet)
Task 5: implementer DONE_WITH_CONCERNS 3ca9a59 → amended d0b59ee (trailer). Concern: PeerTimedOut arm unpinned alone. Review dispatched (sonnet).
Task 5: review (sonnet) → spec ✅, Needs fixes: I1 PeerTimedOut arm unpinned; I2 'exactly once' never asserted (has, not count == 1). 4 minors. Fix round 1 dispatched (resume).
Task 5: fix round 1 done fb1c0a0 (one test per timeout arm via test_mute_game; count == 1 assertions on every peer; spectator, lobby and kick tests). Accepted on mutation evidence.
Task 5: complete (commits 1ba6e85..fb1c0a0).
BASE T6: fb1c0a0
Task 6: implementer dispatched (sonnet)
Task 6: implementer DONE_WITH_CONCERNS 1dca900 (e2e green; no RED for the rl-ui unit test; online test passed first run; no layout test; grey wins over the followed highlight). Review dispatched (sonnet).
Task 6: review (sonnet) → spec ✅, Approved, 3 minors.
Final: minor (deferred): Task 6 — no replay test reaches the results screen with a left row (holds by construction); follow_next untested for "all but the followed worm left" / "all gone"; the rl-ui unit test's RED was only a compile error.
Task 6: complete (commits fb1c0a0..1dca900).
BASE T7: 1dca900
Task 7: implementer dispatched (opus) with PF12d-5, P12, P13, P15, bundle versions
Task 7: implementer DONE_WITH_CONCERNS 069dcf7 (crash window, bundles v3, reproduce withdraws, soak cases; REDs). Soak run 2: 2 failures kept in the workspace: (1) good-15s2c0v-…-c6b621bc: client 2 crashes, the host leaves (Bye) before its 10 s timeout, client 1 expects client 2 and never commits → host lost — FR12c-1 says a Bye lets the successor commit at once, so this looks like a bug; (2) good-3s2c0v-…-a769a127: pre-existing at 1dca900 — a stall with no excuse window after an outage-caused migration with the old host crashing later.
Both dispatched to a debugging agent (opus) before Task 7's review.
Soak debugging: Failure 2 fixed c7d74d7 (a new host behind the match resumed at its own tick and replayed final rows at real-time pace → peer stalled 3.7 s; R = max(current, log frontier after the merge), the host catches up; RED test; case kept). Failure 1: the literal FR12c-1 fix (8b83a4f) split a match in the next soak → reverted bfb6237. Soaks at HEAD: 20 + 24 runs, 0 failures; net-feel equal (+~100 B/s down as Task 3).
Ruling FR12c-1b (Failure 1, option B): keep D8's no-split protection — a successor commits alone only when it expects nobody; when the old host said Bye but an expected peer never rejoins (e.g. it crashed), the lone survivor ends Host lost, as FR12c-1's recorded cost; the SK-10 verdict accepts it narrowly when every other expected successor had exited; pair exits with migrations by exited_us <= resumed_us — a split match is worse than a lone survivor's Host lost, and no option avoids both — cost if wrong: a rare lone survivor sees Host lost instead of playing on.
Debugging agent resumed for FR12c-1b.
FR12c-1b: done bd621bb (exits paired with migrations by time), ce6b352 (SK-10 counts an exited successor as unreachable; twin), aca289a (N66 counts only the first host and hosts that resumed; a crashed-before-resume successor case kept). Soaks at aca289a: 18 + 20 runs, 0 failures. Kept cases: c6b621bc, lan-4s2c2v-seed723619267 (no-split), typical-14s4c1v-seed1857358438.
Final: minor (deferred): no rl-online test pins "client crashes, then the host's Bye" (lone survivor Host lost).
Task 7 review dispatched (sonnet); the soak fixes go to the final review.
Task 7: review (sonnet) → spec ✅, Approved, 4 minors.
Final: minor (deferred): Task 7 — reproduce filters gone per tick (O(inputs × gone)); no twin for a profile where fills_stall is false; the crash window does not model migrations.
Task 7: complete (commits 1dca900..069dcf7; soak fixes c7d74d7, bd621bb, ce6b352, aca289a for the final review).
BASE T8: aca289a
Task 8 records: dispatched (sonnet); the controller runs full afterwards.
Task 8 records: done 8ff6b51 (STATUS plan 12d section, CHANGELOG, online.md, rl-online CLAUDE.md; disagreements recorded as the code). Controller full run started.
Plan 12d full run: PASSED at 8ff6b51 (recorded in STATUS 4de9a14). Final review dispatched (opus ×2).
Final review A: 0C/1I/4M. I1 after the PF12d-8a repair each survivor that held the refused mark keeps it in its input log: if it later hosts, fill never marks that player's real leave (frozen worm again), and its replay shows a withdrawal the match did not have. Fix: reconcile the log's marks with the loaded world in state_arrived.
Final review B: 0C/0I/9M (docs and tighter tests). Final fix pass dispatched (opus): A-I1 (reconcile log marks with the loaded world in state_arrived) + all 13 minors; gates check, net-feel, net-soak (no second full: the change is narrow and the plan's full passed).
Final fix pass: done 1fd255f (A-I1: state_arrived drops refused marks the state lacks + checksums from that tick, Event::WithdrawalUndone; RED test), 2608e86, 50ba1c8, 99d2200, 3ad072a, b80e1a7 (minors). Rulings FR12d-B5 (no client-side dedupe of leave lines — ids wrap; cost: a duplicated/missing line in a rare crash window), FR12d-B9 (keep PF12d-5's crash window; cost: ~0.8 s more excused stalls on bad). net-feel equal; soaks 23 runs 2 failures (both reproduce on 4de9a14, pre-existing) + 21 runs 0.
Open soak failures: (1) bad-10s1c2v-seed1616063176: host crash with one client and two mid-match spectators under outages → all Host lost, SK-10 does not accept; (2) lan-16s7c0v-seed2762456828: a split — host Bye while peers 2 and 6 are cut off; peer 1 resumes; peer 2 resumes as a second host 11 s later (D8's no-split broken, likely a partition {1,…} | {2,6}).
Ruling FR12d-Q (2): replace FR12c-1's "at least one rejoin" with a majority quorum — a successor commits only when it plus its rejoined peers are more than half of the match members not known gone (players and spectators expected back; the old host counts unless its Bye/Closed arrived) — a partition can then continue on at most one side; it reproduces FR12c-1 (two-peer blip = stall) and FR12c-1b (lone survivor of three = Host lost) — cost if wrong: an even partition continues on neither side (both Host lost).
Debugging/fix agent dispatched (opus) for (1) and FR12d-Q.
Quorum: done 3e78d01 (SK-10 compares a joined spectator from its state; failure 1 legitimate), 6abbefb (FR12d-Q majority quorum in harness, migration::quorum, rl-online; failure 2 was a commit-rule failure, not a partition), 311b93c, 19edbf7 (two more verdict gaps found by soak, both pre-existing). Final code soaks: 20 + 20 + 22 runs, 0 failures; net-feel equal. Concerns: a crashed host counts as a member (host + 3 players: host crash + one more out of reach → Host lost for two); followers do not count; peers cut off during their asks stay Host lost even after their link returns (N73: an unreached successor is never asked again in its window). Flakes seen: rl-discovery garbage_input_is_answered_not_fatal; rl-game hosting_again_on_the_same_port_keeps_its_mapping (port in use). Process slip: 3e78d01 through check | tail while check had failed on that flake.
Ruling FR12d-Q1: followers (spectators expected back) count in both the quorum's numerator and denominator — consistent counting keeps the majority property and keeps host + player + spectators matches alive — cost if wrong: none known.
Ruling FR12d-Q2 (amends N73): an unreached successor is asked again every 1 s within its window (bounded by the window), so a peer whose link returns mid-window still joins; the quorum keeps this split-safe — cost if wrong: a few more join attempts per migration.
Flakes: fix both (state/order, not timing; a port nobody else can take).
Quorum agent resumed for FR12d-Q1, Q2 and the two flakes.
Quorum round 2: done 0495cc3 (FR12d-Q1 followers count, members = the old host's whole succession; FR12d-Q2 REASK_US 1 s; tests RED without), 033aa4e (rl-discovery flake: oversized frame's close during the send → BrokenPipe; 480 runs under load 0 failures), b8729ac (rl-game flake: a freed ephemeral port taken by another test's port-0 bind; hosts below the ephemeral range). Soak case 2 and failure 1 now continue. Soaks 21 + 25 + 18 runs, 0 failures; net-feel identical.
Final: minor (deferred): a member that can never come back (a spectator still loading) counts against the quorum; FR12d-Q2 has no end-to-end rl-online test (the outage hook drops UDP only); SK-10's reach model stays generous.
Next: STATUS update for the final-fix and quorum rulings (docs agent), then the controller re-runs full (migration code changed after the plan's full run).
Docs: c76ffad (STATUS plan 12c/12d amendments, online.md 'Who goes on: a majority', CHANGELOG). Controller re-runs full.
Re-run full: PASSED at c76ffad (recorded in STATUS). Plan 12d: complete.
