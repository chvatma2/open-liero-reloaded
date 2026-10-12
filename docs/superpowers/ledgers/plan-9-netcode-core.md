# SDD ledger — plan: docs/superpowers/plans/2026-10-12-plan-9-netcode-core.md

Spec: docs/superpowers/specs/2026-10-12-networking-design.md (approved by the user 2026-10-12: "Reviewed, write the plan and implement"). Execution: subagent-driven, on main (user's standing rule: commit only on main, never push).

## Pre-flight scan

| Pair / task | Produced vs consumed | Finding |
|---|---|---|
| T1→T2 | Terrain::{start_journal, journal_mark, undo_to, forget_before, cell_hash, journal_bytes}, Default | consistent |
| T1→T3 | T3 adds Terrain::{cells, load_cells} using set_cell + forget_before | consistent (T3 touches rl-terrain; its commit adds crates/rl-terrain) |
| T2→T8 | World::{enable_rollback, save, restore→Result, forget_before, sync_checksum} | consistent; WorldSim::restore expects Ok |
| T3→T8, T3→T10 | WorldState::{encode, decode}, World::{state, load_state}; Serialize for JSON diff | consistent |
| T4→T6/T7/T8 | Datagram/ClientInputs/HostInputs/Frame, Control::{Late, Checksum, Desync, StateBegin, StateChunk}, encode/decode_frames, limits | consistent |
| T5→T6 | Simulation, ToySim, Network (received/sent LinkStats), Profile, PeerId, Channel, Rng | consistent |
| T6→T7 | PeerPlan gains `freeze` (T7 updates tests/session.rs), Event variants, Pace replaces T6 pacing | consistent (note carried) |
| T7→T8 | Scenario gains `corrupt` (T8 updates earlier literals) | consistent (note carried) |
| T8→T9/T10 | Factory corrupt hook: T8 text says `corrupt(&mut S)`, T10 says `corrupt(sim, index)` | CONFLICT → ruling PF1 |
| T9→T10 | NetSpec gains `faults` (T10 updates net_sim.rs); harness observer added in T10 (touches rl-netcode harness) | consistent |
| T10→T11 | observer for positions/rollbacks; measure_one | consistent |
| T8/T9→T12 | WorldSim, genlevel ids | consistent |
| T1 self | tests vs code: journal tests use `journal = None` from inline tests (private field, same module) | consistent |
| T2 self | `world()` helper may lack dirt: plan instructs a variant if needed | consistent |
| T3 self | test order: terrain checked before worms (same size → Worms) | consistent |
| T4 self | MAX_CONTROL 40 KiB > chunk 32 KiB + header | consistent |
| T5 self | latency test steps 1 µs-resolution loop (20 M iterations) | acceptable |
| T6 self | toy verdict without fill rule: host may stall under awful; expect_no_stalls false | consistent |
| T7 self | `a_frozen_client_catches_up`: freeze 600..900 = 300 ticks, fills start at ~612 → < 300 consecutive fills, PeerLate cannot fire | CONFLICT → ruling PF2 |
| T8 self | desync detected 500..530 | consistent |
| T9-T14 self | — | consistent |

Ruling PF1: The Factory hook is `fn corrupt(&mut self, _sim: &mut S, _index: usize) {}` (default no-op), defined in T8; `Scenario.corrupt: Vec<(PeerId, u32)>` and the index passed is the entry's position in that list; ToyFactory adds 1 to slot 0's position whatever the index; T10's WorldFactory applies `spec.faults[index]`. — T8 and T10 then agree without T10 changing the trait. — Cost if wrong: one signature change in T10.
Ruling PF2: `a_frozen_client_catches_up` freezes peer 1 for host ticks 600..1000 (4 s) instead of 600..900, so 3 s of consecutive fills happen and PeerLate fires; the rest of the test unchanged. — The behaviour rule (3 s) is the spec's; the test window was too short. — Cost if wrong: none (a test window).

## Tasks
Task 1: dispatched (BASE c9f1bf1, implementer sonnet)
Task 1: complete (commits c9f1bf1..37dda73, review: approved, no Critical/Important; tests per report: rl-terrain 21/21, rl-sim no_alloc, rl-cli golden, xtask check --offline)
Task 1: minor (deferred): ColorMap::rebuild_area (color.rs:269) writes its Terrain clone's cells directly, and ColorMap::new clones a journaled terrain with its journal — inert now; plan 10 must not clone a journaled terrain into ColorMap (or clear the journal there).
Task 1: minor (deferred): journal_mark / JournalError lack doc comments.
Task 2: dispatched (BASE 37dda73, implementer sonnet)
Task 2: review round 0 → Needs fixes (Important: stale snapshot marks accepted silently after an older restore). Fix round 1 dispatched to the same implementer (HEAD before fix d8f2f80).
Ruling U-ARM (user decision 2026-10-12): Task 13 gains an arm64 Linux leg (cross-built, run under qemu-aarch64, in full). Plan text amended.
Task 2: complete (commits 37dda73..5f7234d; review: fix round 1 addressed all findings; tests per report: rl-terrain+rl-sim 128, golden, xtask check)
Task 3: dispatched (BASE 2762904, implementer opus)
Task 3: implementer deviations accepted by review: StateError::Clocks added (Tasks 8/10 must handle it), setup-only `World::setup_worms` field (not hashed), `load_cells` seals the journal (older marks refused), aimed `hostile_states_never_panic` proptest added.
Task 3: review round 0 → Needs fixes (Important: fixed 1000 ms projectile-lag bound refuses legitimate states at LX56PhysicsFPS=1 or GameSpeed=0.1). Fix round 1 dispatched (HEAD before fix 7c6aa1c).
Task 3: note for Tasks 8/10: consumers outside the sim (HUD, bots) must treat a received state's MatchResult/Score values as untrusted.
Task 3: complete (commits 2762904..44ff0e5; fix round 1 addressed all; tests per report: rl-sim+rl-modes 131, golden, xtask check)
Task 4: dispatched (BASE 44ff0e5, implementer sonnet)
Task 4: review approved (no Critical/Important); re-graded minor 'fingerprint ignores frames.rs' as worth fixing (a codec change would not change the fingerprint → incompatible peers would join). Follow-up round 1 dispatched: hash all rl-proto/src + panic on missing source, strict trailing-byte decoding, two frame tests.
Task 4: complete (commits 44ff0e5..1e3fab3; follow-up verified by controller from the diff: all of rl-proto/src hashed, panics on unreadable sources, strict decoding; tests per report 15/15, xtask check)
Task 5: dispatched (BASE 1e3fab3, implementer sonnet)
Task 5: review round 0 → Needs fixes (Important: unreliable datagrams arriving inside a scheduled outage; re-graded: burst 1 not independent loss; set_profile keeps GE state). Fix round 1 dispatched (HEAD before fix 4b5b6e3).
Task 5: complete (commits 1e3fab3..2e42b27; fix round 1 verified by controller from the diff: arrival-time outage drop, independent loss at burst<=1, GE reset; tests per report 14/14, xtask check)
Task 6: dispatched (BASE 2e42b27, implementer opus)
Task 6: complete (commits 2e42b27..68e7d70; review approved, no Critical/Important; deviations accepted: host sends while stalled (deadlock otherwise), Report.logs, MAX_AHEAD=6000 against hostile peers, HOST_FRAME_US, RTT in eighths, host first=ack.min(frontier); tests per report 22/22, xtask check)
Task 6: minors carried into Task 7: harness end waits for every peer (spectators) to reach `ticks` within limit; clamp stamp sent_ms >= 1 (0 means no echo); the host sending every frame while stalled must not survive Task 7's pacing (one datagram per due tick); own-slot override test via the fill rule.
Task 6: minor (deferred): InputLog/own grow ~36 B/tick for the whole match (13 MB/h) — bound it in plan 10 (keep what replays need, e.g. stream to the recorder).
Task 6: minor (deferred): `known` mask in InputLog written but never read; host_log duplicates logs[&HOST]; awkward block in harness.rs:355-360; little unit-level testing of Session alone (clock_ppm, MAX_AHEAD, Malformed counter).
Task 7: dispatched (BASE 68e7d70, implementer opus)
Task 7: Ruling T7-A: MAX_PREDICTION = 28 (N1 amended from 20; plan text updated) — measured stall events in `bad` (10 seeds, 4 clients) by limit 20→310, 22→68, 24→14, 26→3, 28→0; a client's frontier also waits for the slowest client's inputs (up to the fill window); a stall freezes the player's game, worse than a deep correction; spec's rule "zero stalls up to bad" kept — cost if wrong: snapshot ring 29 instead of 21 (memory), deeper worst-case rollbacks on bad networks (measured in Task 12).
Task 7: Ruling T7-B: implementer deviations accepted: integral term in the speed adjustment (proportional-only left ±0.5 % drift unresolved); margins ignored after a jump until catch-up ends then 16 fresh samples; a client behind its own pace is never sped up; host samples a datagram's newest input when newer, client always sends its newest; filled set pruned below the client's highest `first`; a freeze stops the peer's clock; run ends once each peer re-simulated after confirming `ticks`; snapshot trimming keeps a pending rollback — each was forced by a brief test or a probe — cost if wrong: tuning in Task 11's feel numbers.
Task 7: note: `awful` → host fills ~94 % of a client's inputs (the spec's lagging-player outcome); verdicts hold.
Task 7: complete (commits 68e7d70..eea9237; review approved, no Critical/Important; tests per report 31/31 incl. bad with zero stalls; xtask check)
Task 7: minors carried into Task 8 (it extends session.rs): clamp a jump's size (hostile margin), ignore stale/out-of-range HostInputs.tick in sync (pinned sync_tick disables the integral term), purge queued datagrams in peer_lost, keep the newest input in ClientInputs beyond MAX_KEYS (send the newest MAX_KEYS? or ensure margin sampling sees newest).
Task 7: minor (deferred): exact-once assertions for PeerTimedOut/Late, Keys(0) rows after peer_lost, sync unit tests against hostile margin/tick.
Task 8: dispatched (BASE 626d284, implementer opus)
Task 8: Ruling T8-A: deviations accepted — encode_confirmed_state returns Option (no state before the first confirmed tick or while joining); the host keeps all its own checksums (needed for reports/bundles; ~290 KB per 30 min); state sending and floor re-simulation happen at the next advance; InputLog gains an origin for joined spectators; time-sync tick bound = current+100 (last-seen+100 sticks after >1 s outages); newest MAX_KEYS inputs sent; sim_mut moves the state floor (a test fault must not be rolled back away). — cost if wrong: small interface changes in Task 10.
Task 8: review round 0 → Needs fixes (Important: Repair::Sent never re-arms — a refused state or a re-diverging peer stays silently desynced). Fix round 1 dispatched (HEAD before fix 89010cc): grace period + re-arm, cap 3 → Event::RepairFailed; minors 1, 2, 4, 5.
Task 8: minor (deferred): a hostile host can grow a client's `own` via repeated valid states moving current by up to MAX_AHEAD each (commented in code).
Task 8: complete (commits 626d284..a8da103; fix round 1 addressed all; tests per report rl-netcode 54/54, golden, xtask check)
Task 8: note for plan 10: repair cap is 3 per peer per match (never reset after a successful repair); RepairFailed / JoinRefused events must be handled by the game's driver.
Task 9: dispatched (BASE a8da103, implementer sonnet)
Task 9: review round 0 → Needs fixes (Important: NetSpec::stem not unique per spec → failure files/cases collide; kept net case not logged in blessings.log, --reason dropped). Fix round 1 dispatched (HEAD before fix e8d809f). Deviation accepted: factory keeps build_match's own bots (own AreaCache per peer).
Task 9: note for Task 11: measured ~18-25 KB/s received per client at only 8 slots (budget 24 KB/s at 32 players incl. overhead) — expect the bandwidth gate to need session changes.
Task 9: complete (commits a8da103..8030b7f; fix round 1 verified by controller from the diff stat + report: hashed stems with a unit test, kept net cases logged; tests per report net_sim/net_cases/xtask units, xtask check)
Task 10: dispatched (BASE 8030b7f, implementer sonnet)
Task 10: deviations accepted: observer as a trait (on_event, on_frame, run_observed); new Session::change_confirmed (test/harness-only) to plant faults after the last confirmed tick; faults wait until the peer confirmed tick t; reproduce refuses uncovered state ticks (to be softened in fix round).
Task 10: review round 0 → Needs fixes (Important: test-shaped "worms first" diff order → report every differing top-level field in WorldState declaration order; missing proptest for DesyncBundle::decode; no direct test of change_confirmed). Fix round 1 dispatched (HEAD before fix e07e1ac).
Task 10: complete (commits 8030b7f..89f9bdc; fix round 1 addressed all; tests per report net_repro, net_sim, rl-netcode, xtask check)
Task 10: minor (deferred): Repro::uncovered is stringly typed and its non-empty path untested; FIELDS key-set test needs content (runs in check via RL_CONTENT_DIR).
Task 11: dispatched (BASE 89f9bdc, implementer opus; bandwidth fixes authorised within rules)
Task 11: Ruling T11-A (N8): on links with RTT <= 100 ms each input change is repeated in 4 datagrams and resent from the ack when the echo shows a gap, instead of every datagram repeating every unacked input (spec §3.1's wording); longer links keep full redundancy. — Measured: bandwidth32 (typical) 28 693 → 16 687 B/s (budget 24 576), feel numbers identical in every profile; plain timeout-resend or 50 Hz coalescing made `bad` stall. — Cost if wrong: a loss pattern the 4 copies miss costs one resend round trip (an extra rollback), visible in the feel numbers.
Task 11: Ruling T11-B (N9): the 24 KB/s budget (spec §3.1) is held at the `typical` profile; on `bad`/`awful` links a 32-worm client receives ~45 KB/s or more because full redundancy keeps them stall-free (≈360 kbit/s, within any broadband). — Cost if wrong: very slow uplinks with 32 players; revisit in plan 10 with real transport numbers.
Task 11: review approved (no Critical/Important). Baselines: rollback p50/p99 lan 3/4, good 5/6, typical 8/10, bad 14/18, awful 27/28; correction p99 2.2/5.1/11.0/19.8/31.2 px; stalls 0 lan..bad (awful 29 880, ungated: the lagging player pays); down B/s 9240/12346/12993/32362/55346; bandwidth32 16 687.
Task 11: follow-up dispatched: hypot → IEEE sqrt (host independence).
Task 11: minor (deferred): no tests for changes above the host frontier lost in all 4 copies, RTT crossing 100 ms mid-match / unmeasured at start, truncated datagrams in short mode; measure_one errors (instead of a gated number) if a client never frames after the match.
Task 11: complete (commits 89f9bdc..89e3358; hypot follow-up verified by report: net-feel output byte-identical, no re-bless)
Task 12: dispatched (BASE 89e3358, implementer sonnet)
Task 12: complete (commits 89e3358..8edcc17; review approved, no Critical/Important). Rollback p99: Castle Strike 16 worms d12 1.21 ms (budget 2), d28 2.95 ms, 4096² d12 1.19 ms (budget 4); restore alone 41-63 µs.
Task 12: minor (deferred): divan rollback bench lacks a one-time checksum assert; snapshot_bytes is a proxy; bench window calmer than the command's samples.
Task 13: dispatched (BASE 8edcc17, implementer sonnet; node and qemu-aarch64 not installed at dispatch, aarch64 gcc present)
Task 13: deviations accepted: compare takes a platform name; one leg per flag (none = WASM); skips detected before recording; golden spec parsed by a local RON struct in xtask; Windows build also builds rl-detcheck. Windows leg ran: 3 replays, 1274 lines equal native.
Task 13: review approved with one Important (golden-spec field drift: no deny_unknown_fields, missing mode/loading_time/pool_capacity). Fix round 1 dispatched (HEAD before fix a14725d). WASM and arm64 run legs pending the user's `sudo apt install nodejs qemu-user`.
Task 13: complete (commits 8edcc17..56c8d50; fix round 1 verified from report: strict golden parsing with per-golden flag tests, node >= 18, empty-stream guard; Windows leg equal; WASM/arm64 run legs pending apt install)
Task 14: dispatched (BASE 56c8d50, implementer sonnet)
Task 14: complete (commit 0e1524e; docs only; xtask check passed; controller checked the trailer is the required Opus 5.5 one)
Final review: dispatched (range c9f1bf1..0e1524e, reviewer opus)
Final review: With fixes. Important: (1) non-finite floats pass World::load_state (Review Focus 1 explicitly requires refusing NaN positions); (2) rollback cannot report undone terrain areas to the presentation (ColorMap would keep mispredicted holes) — plan-10 API gap, cheap now; (3) WASM/arm64 determinism legs never ran (needs the user's apt install). Re-graded to the fix pass: fingerprint omits rl-netcode (two builds with different session semantics would join each other).
Final: fix pass dispatched (one implementer, opus): items 1, 2, fingerprint.
Final: plan-10 carry-overs (from final review minors): hide sim_mut (#[doc(hidden)] or test-hooks); coalesce Late per datagram and give the lagging client its own 3 s warning; count stall episodes not advances; repair re-arm window should start at the client's load/ack or scale with state size (multi-MB states on real links); handle Control::Bye / closed channel → peer_lost; public confirmed tick (max(frontier, floor)) for the confirmed-only HUD; spurious Malformed race at a spectator joined before any confirmed tick; host-freeze harness scenario; committed structure-aware datagram proptest; profile encode_confirmed_state on 4096² (host hitch on join/repair); InputLog growth; repair cap per match.
Final: fix pass done (76ae63b non-finite refused, af682cd restored terrain areas, 35a7d16 fingerprint covers rl-netcode). Ruling F-A: a mod whose numbers make honest states non-finite gets its states refused (repair fails, late spectators cannot join; the match plays on) — documented in state.rs; no shipped mod needs an exemption — cost if wrong: such a mod cannot be repaired or spectated mid-match. Ruling F-B: restored areas reached via Session<WorldSim>::take_restored_areas (driver has only &S); rl-netcode depends on rl-terrain — cost if wrong: an API move in plan 10.
Final: note: Task 8's Castle Strike restore test never changed a cell (Castle Strike has no dirt); the fix pass's terrain tests use Dirt Level. Plan-10 carry-over: rebuild_area is a fallback (blood stains and original dirt art inside restored areas are lost) — an exact ColorMap undo needs its own journal.
Final: re-review of fix pass: all addressed, no new Critical/Important. STATUS.md gains N13 (F-A) and N14 (F-B). Pending outside the plan: WASM and arm64 determinism run legs (need the user's sudo apt install nodejs qemu-user). Final review clean → ledger archived, workspace deleted.
