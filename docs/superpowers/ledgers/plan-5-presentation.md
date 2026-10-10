# SDD ledger — plan: docs/superpowers/plans/2026-10-10-plan-5-presentation.md

Ruling: Work directly on main — user's standing choice — cost if wrong: commits need moving to a branch.
Ruling: Plan specifies interfaces, OLX references, named tests and acceptance rather than verbatim code (as plan 4) — user asked to finish without stopping; presentation correctness is checked by snapshots/goldens — cost if wrong: more implementer judgement, caught by reviews.
Ruling: Execution via subagent-driven-development — cost if wrong: token cost.
Ruling: Plan 5 starts only after plan 4's final fix wave and re-review are committed (both touch rl-sim events) — cost if wrong: none.

## Pre-flight scan
| Tasks | Produces vs consumes | Finding |
|---|---|---|
| T1→T2 | ColorEdit source data / TerrainEdit + theme art via rl-content | T2 defines ColorEdit in rl-terrain (cannot depend on rl-sim); conversion lives in rl-scene or rl-content — consistent if T2 owns the conversion function in rl-scene? rl-scene is T3 → Ruling R1 |
| T1→T3 | E6 data, Projectile::serial, physics_time, substeps | consistent |
| T2/T3→T4 | Fx::step_tick takes &mut ColorMap (T2), draws into DrawList (T3) | consistent |
| T2–T4→T5 | Renderer consumes ColorMap dirty rects, DrawList, SceneAssets (T3) | consistent |
| T1→T6 | Audio consumes LoadedMod/CorePack sounds | consistent |
| T1–T6→T7 | rl-game wires all | consistent |
| T5 vs T8 | xtask full created in T5 Step 3; T8 extends it | consistent |
| T2 self | golden blessing via `cargo xtask bless-render --reason` — no such xtask exists yet | Ruling R2 |
| T3 self | insta snapshots in check | consistent |
| T7 self | Step 3 manual check: implementer cannot see a window | Ruling R3 |

Ruling R1: the TerrainEdit→ColorEdit conversion lives in rl-content (L4, depends on rl-sim and rl-terrain) so Task 2 can test its full-match agreement without rl-scene — cost if wrong: a function move.
Ruling R2: Task 2 creates `cargo xtask bless-render --reason <text>` (records the reason in fixtures/blessings.log, spec §7.2) for its CPU golden; Task 5 reuses it for GPU goldens — cost if wrong: none.
Ruling R3: Task 7's "manual check" means: run rl-game under a real WSLg window for a timed scripted session (e.g. an `--autoquit-after <secs>` flag plus a scripted key sequence), capture frames via the offscreen path and inspect the PNGs with the Read tool, and report FPS/tick timing from the debug counters; the human playtest follows at the end — cost if wrong: visual issues found later by the user.
Task 1: dispatched (BASE e923c51, implementer opus)
Task 1: implementer DONE (commits 4489717, cb564ca, 57ccae9); review dispatched (opus)
Task 1: note for Task 7: rl-content returns warnings, never prints; rl-game logs them once.
Task 1: review Approved, no Critical/Important. ⚠️ resolved: controller ran cargo xtask check at 57ccae9 (oracle + no_alloc included).
Task 1: minor (deferred → carried into Task 2): Dirt probe doc: "colour image after applying the preceding edits in this buffer"; probe is in material px (scale by art_scale).
Task 1: minor (deferred → carried into Task 4): bonus-chain explosion's Dirt events come after its carve events (OLX captures colour before) — rl-fx falls back to the theme default colour when the probe pixel is no longer dirt; record in STATUS. Also add E6 source tests (bounce shake 0, bonus chain 5, beam bonus 5; dig and rope probes; substep == k for worm-loop events) and fix the setup-events-first assertion.
Task 1: minor (deferred → carried into Task 7): new_world drops LoadedMod/LoadedTheme warnings and forces re-loading assets; add new_world_from(&LoadedMod,&LoadedLevel,&LoadedTheme,&SandboxSetup) or a bundle; F5 must not re-decode.
Task 1: minor (deferred → carried into Task 2): core_pack import: one unreadable .ogg/colours.cfg fails the whole pack (should warn); or_insert reads eagerly.
Task 1: minor (deferred): sandbox indexes worms()[i] instead of spawn_worm's index; FindSpot truncation comment; rl-cli core origin path.
Task 1: complete (commits e923c51..57ccae9, review clean)
Task 2: dispatched (BASE 57ccae9, implementer opus)
Task 2: implementer DONE (commits c3d6133, 9f2c734, d8602be); review dispatched (sonnet)
Task 2: note for Task 4: rl-fx applies ColorEdits one at a time while walking events so Dirt probes sample correctly; apply_edits (whole tick) only for callers that never sample probes.
Task 2: review Approved, no Critical/Important. ⚠️ resolved: controller ran cargo xtask check at d8602be (layers + wasm included) → passed.
Task 2: Ruling: render goldens (fixtures/render/*.png) are committed test fixtures even though they derive from OLX art — spec §7 mandates committed goldens and OLX content is acceptable to bundle (spec §11); "converted assets are never committed" applies to imported packs — cost if wrong: remove goldens, regenerate locally.
Task 2: minor (deferred → carried into Task 5): document golden provenance (scenario, level, art scale, produced from imported OLX content) in rl-scenario/rl-render CLAUDE.md and the root CLAUDE.md Content section.
Task 2: minor (deferred → carried into Task 3): cap art_scale (e.g. ≤ 8) or total colour-map bytes when loading levels (hostile packs).
Task 2: minor (deferred): golden/3000-tick tests skip with eprintln only (project convention); stained pixels exempt permanently in class check; edit_colours permissive (covered by unit tests).
Task 2: complete (commits 57ccae9..d8602be, review clean)
Task 3: dispatched (BASE d8602be, implementer opus)
Task 3: implementer DONE_WITH_CONCERNS (commit 9d949c3); review dispatched (opus)
Task 3: note for Task 4: rl-scene already draws LaserSight and Beam cosmetics; rl-fx skips them.
Task 3: review Needs fixes — 1 Important (Scene::reset leaves camera shake running across a restart). Fix round 1 dispatched (resumed implementer).
Task 3: minor (deferred → carried into Task 7): in Linear interpolation bonuses aren't blended, laser/beam start at sub-step position, worm-attached hook uses current position — blend or anchor to drawn worm position.
Task 3: minor (deferred): rope worm end truncated vs sub-pixel skin (≤1 px); add STATUS line.
Task 3: minor (deferred → plan 6 or later): green-crosshair owner guessed / trace recomputed, beam colour from current weapon — add worm id, hit flag, colour to events.
Task 3: minor (deferred): order test permissive (health bars off).
Task 3: minor (deferred): colour-map cap budgets per image (≈768 MiB total).
Task 3: minor (deferred): rl-scene RED by mutation only.
Task 3: fix round 1/5 (1 addressed, 0 open; commits 9d949c3..868781e)
Task 3: complete (commits d8602be..868781e, review clean after 1 fix round)
Task 4: dispatched (BASE 868781e, implementer opus)
Task 4: implementer DONE_WITH_CONCERNS (commits 80e76f0, 16ceadf); review dispatched (opus)
Task 4: contract for Task 7: rl-fx step_tick applies the tick's terrain edits to the ColorMap; rl-game must not also call apply_edits for the same tick.
Task 4: review Needs fixes — 1 Important (entity grid follows the fractional camera → shimmer). Fix round 1 dispatched (resumed implementer).
Task 4: Ruling: entities are drawn at their latest state, not interpolated (deviation from E2's wording) — OLX draws latest; entities are short-lived cosmetics; with a world-aligned grid they sit still against the terrain — cost if wrong: slight entity stepping at very high refresh rates.
Task 4: Ruling: Task 7's plan text "color.apply per TerrainEdit" is superseded by the rl-fx contract (rl-fx applies edits; rl-game never also does) — the Task 7 dispatch carries this — cost if wrong: none.
Task 4: minor (deferred): pool iterator not OLX's literal FastVector::next (a used slot just above lastUsed after a double free is still visited in OLX); STATUS text overstates; add a test.
Task 4: minor (deferred): huge explosion damage → invisible explosion never ends (faithful); record under kept quirks.
Task 4: minor (deferred): dig source test compares the pre-tick terrain (comment).
Task 4: minor (deferred → carried into Task 5): FxAssets duplicates white image; FxAssets::add_worm must mirror SceneAssets::add_worm order; all registration before atlas upload — document; a late-joining worm needs re-upload.
Task 4: minor (deferred → carried into Task 5): colour key only (255,0,255); OLX IsTransparent corner rule (254,0,254) — verify giblets.png / core sprites.
Task 4: minor (deferred): no-alloc coverage of lost-area rebuild.
Task 4: fix round 1/5 (1 addressed, 0 open; commits 16ceadf..352ece1)
Task 4: minor (deferred): trunc vs floor grid origin for negative views (consistent either way); CLAUDE.md line wrap.
Task 4: complete (commits 868781e..352ece1, review clean after 1 fix round)
Task 5: dispatched (BASE 352ece1, implementer opus)
Task 5: implementer DONE (commits a2b9efb, 6c8583e, ddb975e); controller viewed fixtures/render/frame-bazooka.png (Fossil Facility, worms, blood — plausible); review dispatched (opus)
Task 5: note for Task 7: under WSL real GPU (d3d12 GL), a thread that used the device segfaults on exit — rl-game renders on the main thread.
Task 5: review Approved, no Critical/Important. ⚠️ resolved: controller timed cargo xtask check after an rl-sim touch → 23.6 s total (within ~60 s).
Task 5: minor (deferred → carried into Task 8): bless-render can rewrite the CPU golden then bail without a log entry on a GPU-less machine; probe adapter and skip GPU goldens with a notice.
Task 5: minor (deferred → carried into Task 8): golden threshold 0.001 loose; tighten (~0.0001 or a small absolute count).
Task 5: minor (deferred): no atlas uploaded → sprites skipped, not magenta; create a 1×1 default atlas.
Task 5: minor (deferred → plan 7 or later): render() single-viewport (clears whole output); note in rl-render CLAUDE.md.
Task 5: minor (deferred → carried into Task 8): §8.2 terrain deviation (one RGBA texture) not in STATUS/spec §19.
Task 5: minor (deferred): margin/sharp-bilinear seam GPU assertion; atlas warnings duplicate on re-upload; rl-cli render simulates past its frame.
Task 5: complete (commits 352ece1..ddb975e, review clean)
Task 6: dispatched (BASE ddb975e, implementer opus)
Task 6: implementer DONE (commit 775ef0c); real device opened (ALSA→Pulse→WSLg), throw/bump played; review dispatched (sonnet)
Task 6: note for Task 7: latch_tick() + play_events(view centre) once per tick; stop_all() on F5; log take_warnings() after new()/load().
Task 6: review Approved, no Critical/Important.
Task 6: minor (deferred → carried into Task 7): cap sound byte size/duration before decode (hostile packs).
Task 6: minor (deferred): kira boundary mix untested; global sounds drift with listener (confirm vs sound_sample_openal.cpp); per-tick set_volume/panning commands; gamepad trigger/hat axes; Alt-as-Jump web risk; on_gamepad untested.
Task 6: complete (commits ddb975e..775ef0c, review clean)
Task 7: dispatched (BASE 775ef0c, implementer opus)
Task 7: implementer DONE_WITH_CONCERNS (commits d12d799, 59ffc42); controller viewed MW Liero Factory screenshot (bazooka trail, blood, popups, HUD, D3D12 AMD GPU) — plausible; review dispatched (opus)
Task 7: review Needs fixes — 1 Important (egui texture deltas dropped when no surface frame → garbled text or egui-wgpu panic after minimise/occlusion). Controller re-graded Minor 8 (busy-spin while minimised) and Minor 9 (no clamp to max_texture_dimension_2d → validation panic) to Important by player effect. Fix round 1 dispatched (resumed implementer).
Task 7: minor (deferred): pick_format first non-sRGB (prefer 8-bit) [optional in fix]; set_viewport resets camera to 0,0 for one frame [optional in fix]; Config::interpolation unused; overlay per-frame String allocation; F2 panel fixed width overflow; renderer warnings drained once; decoded-frames cap for lying Ogg granules (rl-audio); exclusive fullscreen absent (spec §8.6).
Task 7: fix round 1/5 (3 addressed, 0 open; commits 59ffc42..4dfda89)
Task 7: minor (deferred): --autoquit-after not checked while hidden; meter counters see one odd frame after a long hide.
Task 7: complete (commits 775ef0c..4dfda89, review clean after 1 fix round)
Task 8: dispatched (BASE 4dfda89, implementer sonnet)
Task 8: implementer DONE (commit 8ef61fb); review dispatched (sonnet). Note: base a85acb1 is the plan-6 docs commit (controller) which landed during Task 8.
Task 8: review Approved, no Critical/Important. ⚠️ resolved: controller ran cargo xtask full at 8ef61fb → passed (37 s; gpu on llvmpipe Vulkan; audio step ok).
Task 8: minor (deferred): soak bounds are design caps (only instance buffer growth-checked); one level/mod; "main thread" wording in soak/rl-render docs; long CLAUDE.md line; README "10-minute soak" is simulated time.
Task 8: complete (commits a85acb1..8ef61fb, review clean)
Final review: dispatched (opus), range e923c51..8ef61fb
Final review: no Critical; 2 Important (rope re-throw hook slide; duplicated presentation wiring / implicit ordering — M6 readiness). Controller re-graded Minor 2 (input latency under FIFO unmeasured) to Important by player effect (playtest next). Fix wave dispatched (opus) incl. small items (update_terrain before present, per-sprite core tolerance, screenshot error → warning).
Final: minor (deferred → plan 6 Task 1): LaserSight/Beam events carry worm id and beam colour (marks.rs owner/beam_colour guesses).
Final: minor (deferred): hostile level ~5 copies at 256 MiB cap → lower cap/total budget; giant explosion holds an fx slot forever (faithful; consider capped deviation); decoded-frames cap for lying Ogg granules (must-fix before M7 lets players load arbitrary mods).
Final: minor (deferred): all other can-wait items per the final review triage.
Final fix wave: implementer DONE (commits 20d29db, 31bc8c9, 42c3f53, 201aafb, b000967); scoped re-review dispatched (sonnet)
Final fix wave: re-review → all 6 findings ADDRESSED; no new Critical/Important.
Final: minor (deferred): scripted Command::Size draws one frame at the old surface size; drift check is a debug-only panic (internal invariant); latency=1 untested on native GPU-bound backends.
Plan 5: COMPLETE (e923c51..b000967).
