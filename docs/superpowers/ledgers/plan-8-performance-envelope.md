# SDD ledger — plan: docs/superpowers/plans/2026-10-11-plan-8-performance-envelope.md
Spec: docs/superpowers/specs/2026-10-11-performance-envelope-design.md (approved by the user 2026-10-11; players max 32, mingw installed, PC idle, Windows build kept).
Plan commit c677f3d. Content dir is content/packs (migration 74f3538/2e088ef, before the plan).
Ruling: the pre-flight interface scan of the 5200-line plan is delegated to a reviewer subagent writing preflight.md here; I rule on its rows — keeps the controller context for coordination — cost if wrong: a missed conflict surfaces in a task review instead.
Pre-flight: table in preflight.md (no interface mismatches between tasks; 8 findings).
Ruling PF1: trailer "Claude Opus 5.5" stands — the session's attribution reminder names Opus 5.5; the scanner was wrong — cost if wrong: none.
Ruling PF2: xtask Deserialize-only structs must pass clippy -D warnings by reading or removing unread fields (no blanket allow(dead_code)); Plan.quick must be used or removed — cost: small edits in Tasks 4/6/7.
Ruling PF3: Task 2's check-tier test uses a small generated level (e.g. gen-256x256-a2-s1); large/art-8 levels run only in the envelope — a 5 GiB test in check is unacceptable — cost: less coverage of big levels in check.
Ruling PF4: Task 3 resolves only the timestamp slots written this frame (probe slots only when the probe ran) — avoids a WAIT-flag hang on Vulkan — cost: slightly more code.
Ruling PF5: Task 7's prerequisites_are_notices must pin skip-with-notice-and-continue for each prerequisite class (interop, rustup target, Docker, display, software-only adapter) through an injectable prerequisite check — Review Focus 2 demands it — cost: a little test plumbing.
Ruling PF6: Task 9 appends to CHANGELOG's existing `### Added`/`### Changed`/`### Fixed` under `## Unreleased` — cost: none.
Ruling PF7: GenLevel::parse refuses side > 4096, art scale > 8, and side × scale > 16384 (the texture limit) with an error, never a panic or a huge allocation beyond 2048²×8 — replays are untrusted (rule 6) — cost: none for the sweeps (their largest is 4096² a1 and 2048² a8).
Ruling PF8: every percentile in this plan uses nearest rank, one definition (rl-game's Pct::of and xtask's Spread::of agree, documented) — cost: none.
Task 1: dispatched (BASE c677f3d, implementer abc5e4a360b9bf186, opus)
Task 1: implementer DONE bc68be8; reviewer a8c42501cc73cb51e dispatched.
Ruling T1-A: chaos bots never use the AreaCache (implementer's finding), so the map and art sweeps would not measure route-finding cost (spec §4 names it). The stress runner gains a bot personality option (Chaos default; Deathmatch bots for the map-size and art-scale axes) — added in Task 1's fix round; Task 7 uses Deathmatch bots for those two axes — cost if wrong: those axes fire fewer projectiles than chaos.
Ruling T1-B: fx_capacity 0 from a library caller: Config clamps to ≥1 at construction so report and bound check agree — folded into Task 1's fix round — cost: none.
Task 1: review approved (spec ✅, quality approved); minors: NaN/negative --memory-ceiling-mib accepted (operator input) — Task 1: minor (deferred): --memory-ceiling-mib accepts NaN/negative. Fix round 1 sent (T1-A personality, T1-B capacity, minors 2-4).
Task 1: fix round 1/5 sent; re-review ae9db6dc2aed302ad (bc68be8..2b25e81)
Task 1: fix round 1/5 (3 addressed, 0 open; commits bc68be8..2b25e81)
Task 1: complete (commits c677f3d..2b25e81, review clean)
Task 2: dispatched (BASE 2b25e81)
Task 2: implementer DONE baaa4fc (PF3: test on gen-256x256-a2-s1; PF7 bounds; rl-cli replay cmd not routed through LevelAssets — not in brief); reviewer a99f305df3727ed11
Task 2: complete (commits 2b25e81..baaa4fc, review clean)
Task 2: minor (deferred): assets.rs parses GenLevel twice.
Task 2: minor (carried to Task 4): replay_world checks a pack level's replay against the content, not self.packs — Task 4 (GPU scenes use replay_world) makes it `ensure!(recorded == self.packs)` for both kinds.
Task 2: minor (carried to Task 7): art-8 stress with verify holds ~7 GiB — envelope runs use --no-verify (P9) for every point.
Task 2: minor (deferred): `rl-cli replay` cannot play a generated-level replay (not routed through LevelAssets).
Task 2: minor (deferred): docs name gpu-bench before Task 4 lands (plan-mandated).
Task 3: dispatched (BASE baaa4fc)
Task 3: implementer DONE 713a1d2 (check + full passed; goldens unchanged). Note: on the real GPU (d3d12 GL) GPU tests SIGSEGV at thread exit after the body finishes — the known WSL d3d12 thread-exit bug (pre-existing tests crash the same way); carry to Task 4: gpu-bench does all device work on the process's main thread, and the envelope treats only the measured output as the result.
Task 3: complete (commits baaa4fc..713a1d2, review clean)
Task 3: minor (carried to Task 4): probe_ms >= 0.0 asserts nothing — Task 4's gpu-bench test asserts probe GPU time > 0 when timestamps exist.
Task 3: minor (deferred): gpu_timing test skips without the file's notice; Timer::read leaves a pending map if device.poll fails (device loss only).
Task 3: minor (carried to Task 4): the probe covers the picture area — gpu-bench draws the view over the whole target (P11), so area = target there.
Task 4: dispatched (BASE 713a1d2)
Task 4: implementer DONE_WITH_CONCERNS 841b513 (fixes Task 3's probe ≥3 passes panic) + 0df4b81; check+full pass. Concerns: GPU time 0.03 ms at 1440p on d3d12 GL unconfirmed — Task 5 cross-checks DX12/Vulkan; --uncapped under WSL X11/GL still Fifo.
Task 4: review approved; GPU-time brackets verified correct (0.03 ms credible; probe scales linearly ~0.046 ms/pass at 1440p).
Ruling T4-A: reviewer's Minor "invalid spans recorded as 0 ms" re-graded Important — the numbers are the deliverable, silent zeros would bias them — fix round 1 — cost if wrong: a few lines.
Task 4: minor (carried to Task 7): frame_log's skip assumes 100 ticks/s (envelope runs at 1×).
Task 4: minor (deferred): PF8 percentile test had no RED run (it does discriminate).
Task 4: fix round 1/5 sent (a59fe05); re-review ae0fa17c444081203
Task 4: fix round 1/5 (2 addressed, 0 open; commits 0df4b81..a59fe05)
Task 4: complete (commits 713a1d2..a59fe05, review clean)
Task 5: dispatched (BASE a59fe05)
Task 5: implementer DONE bd6b350. Native: DX12/Vulkan list the RTX 5070 Ti with timestamps; 1440p GPU 0.02 ms native vs 0.03 WSL; probe pass ~0.058 ms@1440p, 0.105-0.13 ms@4K.
Ruling T5-A (for Task 7, PF5): "software-only adapter" means no non-CPU adapter (DX12 always lists Microsoft Basic Render Driver); Task 7 wraps winbuild::missing() and the display/adapter checks behind its injectable prerequisite check — cost: none.
Task 5: review needs fixes (2 Important, both plan-mandated).
Ruling T5-B: run() gets a timeout with a Windows-side kill scoped to the stage's image path (never by image name), and command()/callers check the exe's exit status — spec §8 (unattended, nothing left on Windows) outranks the plan text — cost: a little code.
Ruling T5-C: reviewer Minor 3 (content stamp by count+bytes misses same-size edits) re-graded Important — stale content would silently measure the wrong packs — stamp includes the newest mtime or a content hash — cost: a few lines.
Task 5: minor (deferred): interop notice names one of two paths; remove_dir_all error swallowed; objdump status unchecked; cmd.exe without /d; copy_dir follows symlinks; stage/run untested (need Windows).
Task 5: fix round 1/5 sent.
Task 5: fix round 1 done (39b2b1a; run(timeout) RUN_TIMEOUT 10 min, kill by stage path); re-review a0aa41cc1f28c130b
Task 5: fix round 1/5 (2 addressed, 1 open — stop_script Unicode-quote injection; commits bd6b350..39b2b1a). Fix round 2 sent (path via env var).
Task 5: fix round 2/5 (2 addressed, 0 open; commits 39b2b1a..91b198a)
Task 5: complete (commits a59fe05..91b198a, review clean)
Task 5: minor (deferred): stop script exit masks a non-terminating Stop-Process failure; a user WSLENV RL_STAGE/p would mangle the path (fails to stop nothing, safe); quoted-path ignored test weaker than reported; one long line in xtask/CLAUDE.md.
Task 6: dispatched (BASE 91b198a)
Task 6: implementer DONE_WITH_CONCERNS b3660aa; oracle-record --check passed (112 traces); rl-sim and OLX equal projectile counts and rand() calls per frame. heavy-napalm peaks at 829 projectiles (spikeballs reaches 3000).
Ruling T6-A: OLX's optimisation is -O3: CMakeOlxCommon.cmake:43-47 sets CMAKE_BUILD_TYPE Release as a normal (non-cache) variable when DEBUG=No and :338/:369 add -O3; build_info's empty cache value is not the truth — Task 7/9's report states "Release, -O3 (CMakeOlxCommon.cmake)" — cost: none.
Ruling T6-B: Task 6's #[expect(dead_code)] on mod olx_timing is removed by Task 7 when it wires it (carried) — cost: none.
Task 6: review approved; 1 Important (plan-mandated): heavy-napalm claims a 3000 fill but peaks at 829.
Ruling T6-C: keep heavy-napalm as a mid-load point and reword its description/docs honestly (spikeballs is the cap-filling one) — a mid-load point is useful and the inputs stay as recorded — cost if wrong: one fewer cap-level OLX scenario.
Task 6: minor (deferred): rl-sim span includes bonus placement (none in timed scenarios); OracleHooks leaves g_rec open if the timing file can't open; docker checks live in olx_timing (Task 7 reuses).
Task 6: fix round 1/5 sent.
Task 6: fix round 1/5 (2 addressed, 0 open; commits b3660aa..085e0bd)
Task 6: complete (commits 91b198a..085e0bd, review clean)
Task 6: minor (deferred): plan line 24/spec §6 and the plan's napalm description still say heavy scenarios fill 3000 — Task 7/8/9 must not repeat it.
Task 7: dispatched (BASE 085e0bd)
Task 7: implementer DONE_WITH_CONCERNS e967e91; --quick ran in 2 min (all sweeps, wsl-gl/dx12/vulkan with timestamps, OLX same work on 3 scenarios). Notes for Tasks 8/9: WSL windowed runs stay Fifo (34 fps at 4K vs 2.75 ms p99 offscreen) — report as WSL present limit; art a8 peaks 5.1 GiB even with --no-verify; passes_144/60 are GPU-side headroom — label so.
Task 7: review approved (Focus 1-4 pinned; budgets, determinism verified).
Ruling T7-A: reviewer Minors 1 (empty cause on signal kill), 3 (quick/full compare+bless mixing) and 5 (one OLX failure drops the table; rand_calls length) re-graded Important — Task 9's unattended run and the blessed baseline depend on them — fix round 1 — cost: small.
Task 7: minor (deferred): windowed runs not checked for a software adapter (rl-game picks its own); unparseable committed results file fails after measuring; other-host branch of run untested; local stress/scenario runs have no timeout.
Task 7: fix round 1/5 sent.
Task 7: fix round 1/5 (3 addressed, 0 open; commits e967e91..c71a852)
Task 7: complete (commits 085e0bd..c71a852, review clean)
Task 7: minor (deferred): incomparable computed twice; OLX notes only printed at skip time (Task 8's report should show them).
Task 8: dispatched (BASE c71a852)
Task 8: implementer DONE dc175f6 (added gpu_dropped to results; --report refuses --bless/--quick)
Task 8: complete (commits c71a852..dc175f6, review clean)
Task 8: minor (deferred): a summary containing the marker text would truncate; WSLg Fifo text keyed on path name not measured mode; hard-coded "10 ms"/"none in these scenarios"; markerless report loses summary silently.
Task 9: dispatched (BASE dc175f6)
Task 9: implementer DONE_WITH_CONCERNS a637316 (real duration 7 min, not 45) + 632f205 (blessed, report, summary). No part skipped; only art a8 breaks (5151 MiB, content cap). Pool sweep never exceeds 4770 live projectiles (listed as a cheap fix). check + full passed.
Task 9: review approved; every summary number traced to the tables/results file.
Ruling T9-A: the networking re-simulation headroom was stated against the 10 ms tick, not the per-frame room (~4 ms at 144 Hz) — re-graded Important because it would mislead the user's networking decisions — fix round 1 with wording minors folded in — cost: none.
Ruling T9-B: accept the measured "about 7 minutes" (brief said 45) and commit a637316 touching mod.rs/main.rs messages — the measurement is the truth — cost: none.
Task 9: fix round 1/5 sent.
Task 9: fix round 1/5 (5 addressed, 0 open; commits 632f205..101af12)
Task 9: complete (commits dc175f6..101af12, review clean)
Final review: dispatched ac8022880fa6582dd over c677f3d..101af12.
Final review: with fixes — Important: (1) software adapters (windowed rows; WARP/Basic Render Driver/softpipe as Other) can be reported as the GPU; (2) *Out structs serde(default) turn a renamed field into a measured 0; (3) map-axis frame check excludes bots' thinking (gpu-bench replays inputs) while called "tick + frame"; (4) WSL windowed numbers mis-explained as a Fifo cap — the WSL present path costs ~2.4 ms/Mpx; WSL windowed misses 144 Hz at all sizes and 60 Hz from 1440p; (5) probe blurs a black texture (compression-friendly), may be optimistic. Minor fix-now: markerless report loses summary; unparseable committed results fails after run; missing noise → use a floor; "No sweep has a breaking point" wording.
Ruling F-A: all five Importants and those four Minors go into ONE fix wave; the probe fix requires re-measuring and re-blessing the GPU part (`--only gpu --bless`), then --report and summary corrections — cost: ~15 min of runs.
Final: fix wave dispatched.
Final: fix wave DONE 1ffdbba..8dac0b3 (GPU re-blessed; probe on drawn frame unchanged within 1%)
Final: fix wave re-review — all findings addressed (commits 101af12..8dac0b3).
Final: minor (deferred): ProbeState::output() doc wrong for 0 passes; summary's "a tick and a frame" for the heavy replay lacks the "no bots' thinking" qualifier (chaos bots ~0.01 ms).
Final: complete. check passed on 8dac0b3.
