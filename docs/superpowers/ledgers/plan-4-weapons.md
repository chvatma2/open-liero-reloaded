# SDD ledger — plan: docs/superpowers/plans/2026-10-10-plan-4-weapons.md

Ruling: Work directly on main — the user's standing choice for this repo (plans 1–3) — cost if wrong: commits would need moving to a branch.
Ruling: Plans 4+ specify OLX sources, interfaces, named tests and oracle acceptance instead of verbatim pre-verified code — the oracle traces are the verification and pre-writing ~4000 lines of port would double the work; user asked to finish the implementation without stopping — cost if wrong: implementers make more design calls, caught by task reviews and the oracle.
Ruling: Execution via subagent-driven-development (not native) — plan is long and later tasks would run on compacted controller context — cost if wrong: higher token cost.

## Pre-flight scan
| Tasks | Produces vs consumes | Finding |
|---|---|---|
| T1→T2..T11 | Fields mask, rl_scenario::CHECKED, trace v2 fields | consistent |
| T2→T4 | World::new gains `seed` (T2), then `sim_mod` (T4) | consistent: T4 extends T2's signature |
| T4→T5 | ProjectilePool, Projectile, spawn_info_apply, projectiles() | consistent |
| T5→T6 | action stubs (Explode/Carve/Dirt/GreenDirt/Injure*) replaced | consistent |
| T6→T7 | action damage applies health directly until injure_worm exists | consistent |
| T5/T8→T7 | EntityCounter defined in T7, but smoke/explosive trails (T5) and laser sight (T8) spawn entities that must be counted before gib draws (T7) are checked | CONFLICT → Ruling P1 |
| T7 vs T8 | T7 Step 3 says all weapon scenarios match fully, but beam weapons need T8 | CONFLICT → Ruling P2 |
| T9→T7 | FindSpot Mt19937 from T7 | consistent |
| T10 | tick signature `-> &TickEvents`; rl-scenario, rl-cli, tests updated | consistent |
| T11 | CHECKED == ALL, no expected divergences | consistent |
| T1 self | CHECKED includes HEALTH|SCORE at T1; M3 traces have no damage | consistent |
| T3 self | weapon scenarios under subdirectories; fixture_names must walk them; acceptance lists them as expected divergences | consistent |
| T6 self | greenball image import touches content manifest + corpus snapshot | consistent (snapshot update allowed) |

Ruling P1: EntityCounter (saturating 1024) and its increments for trails, explosions and the laser sight move to Task 5 (laser sight tick included there); Task 7 consumes it for gib draws — the count must be complete before any gib draw is compared — cost if wrong: one small move between tasks.
Ruling P2: Task 7's full-match acceptance excludes beam weapons (Task 8 owns them); they stay in the expected-divergence list until Task 8 — cost if wrong: none.
Task 1: dispatched (BASE e807c26, implementer opus)
Task 1: implementer DONE_WITH_CONCERNS (commit 3d9ab9e); review dispatched (opus)
Task 1: Ruling: T0 pinned to 100000 ms by simLoadInputs (header options.T0) accepted — needed for reproducible absolute-time behaviour (bleeding) — cost if wrong: none, the sim adopts T0.
Task 1: Ruling: server→client packets other than shots (bonus spawn/destroy, respawn) still go over the network; Task 3 extends the delivery hook to process them synchronously between frames (like shots) before recording respawn/bonus scenarios; Task 3 dispatch carries this — cost if wrong: respawn/bonus scenarios unrecordable until done.
Task 1: review Approved, no Critical/Important. ⚠️ items resolved by controller: `cargo xtask oracle-record --check` on the final image → all 8 traces match (M3 traces unchanged except added fields, fire-minigun reproducible).
Task 1: minor (deferred → carried into Task 3): fire-minigun.ron description claims multi-shot and reload coverage it lacks; STATUS multi-shot line cites it. Task 3's multi-shot and reload-switch scenarios must cover these.
Task 1: minor (deferred): `mt` counts only CMap::FindSpot draws, not other rnd()/rndgen/midrnd users — document in Fields::RAND or widen.
Task 1: minor (deferred → carried into Task 3): scenario validation — reject `"` in weapon names, check 5-slot count, bound bonus x/y to i16, reject hp == 0.
Task 1: minor (deferred → carried into Task 3): fixtures.rs header assertion weak; assert every projectile name non-empty.
Task 1: minor (deferred): `Fields(pub u32)` exposes raw bits; make private.
Task 1: minor (deferred): p.len/b.len no longer compared for M3 scenarios until PROJECTILES/BONUSES join CHECKED (Tasks 5, 9).
Task 1: minor (deferred → carried into Task 3): D3 not shown in a committed trace; add a behaviour check that an ANI_ONCE projectile (Classic Flamer) disappears before its timer.
Task 1: complete (commits e807c26..3d9ab9e, review clean)
Task 2: dispatched (BASE 3d9ab9e, implementer sonnet)
Task 2: implementer DONE_WITH_CONCERNS (commit 9d315d9); review dispatched (sonnet)
Task 2: Ruling: the minimal saturating entity counter (Draws::spawn_entity) introduced in Task 2 (RAND covers `ents`) is the EntityCounter of Ruling P1; Tasks 5/7/8 extend it rather than add another — cost if wrong: none.
Task 2: review Approved, no Critical/Important. ⚠️ resolved: controller ran `cargo xtask check` at 9d315d9 → passed (oracle acceptance included).
Task 2: minor (deferred): STATUS.md lacks the ents-1024 approximation, T0/header-seed handling, GlibcRand layout note.
Task 2: minor (deferred): fixed random table only spot-checked (4 entries); add a checksum test.
Task 2: minor (deferred): air-jump sparkle entity spawns (3) unasserted.
Task 2: minor (deferred): rope::simulate has 8 params (clippy allow); bundle into a context struct.
Task 2: minor (deferred): Mt19937::calls/next_u32 docs; FindSpot scaling not provided yet (Task 7 builds it).
Task 2: minor (deferred): unparsable T0 in trace header silently becomes 0; should be an error.
Task 2: minor (deferred → carried into Task 3): rope scenario never hooks dirt, so the rope's 10 draws are not oracle-pinned; add a rope-into-dirt scenario.
Task 2: complete (commits 3d9ab9e..9d315d9, review clean)
Task 3: dispatched (BASE 9d315d9, implementer opus)
Task 3: implementer DONE_WITH_CONCERNS (commit 138a651); review dispatched (sonnet)
Task 3: Ruling: weapon scenarios on FossilFacility (Dirt Level has no open air), dummy at 60 px, phases level/up/down 1/0.5/0.5 s + 0.3 s wait, firer 10000 hp — brief's layout was infeasible or over the 40 MB budget; coverage kept (every weapon, 76 % dummy hits) — cost if wrong: re-record with another layout.
Task 3: Ruling: D10 delivery also hands the client's messages to the server between frames (bonus grabs, damage reports) — needed for the server to see pickups within a trace; real local play delivers them the next iteration too — cost if wrong: re-record bonus scenarios.
Task 3: note for Task 4: multi-shot packets split more often than weapons-firing.md §4.2's run-break rule predicts; the port must follow the source, and fix the note.
Task 3: review Approved, no Critical/Important. ⚠️ resolved: controller ran oracle-record --check on the 8 older traces + bonus-weapon + weapons/mw-1-0/bazooka → all match (D10 hook leaves old traces unchanged, recordings reproducible).
Task 3: minor (deferred): flame ANI_ONCE check pins lifetimes 55..=62 frames without asserting the flamer has no timer.
Task 3: minor (deferred): fixtures.rs:865 p.sp.unwrap() in test; use expect.
Task 3: minor (deferred): oracle_gen silently overwrites two weapons with the same id; bail on duplicate.
Task 3: minor (deferred): oracle_gen description hard-codes "all 5 slots".
Task 3: minor (deferred): fixture directory walk duplicated in xtask, rl-scenario, fixtures.rs.
Task 3: minor (deferred): "dummy hurt" counts any phase, looser than the level-phase hit.
Task 3: complete (commits 9d315d9..138a651, review clean)
Task 4: dispatched (BASE 138a651, implementer opus)
Task 4: Ruling: SimMod expands shared pack projectiles back into OLX's per-occurrence load-order tree so indices equal OLX's and names equal header path names — avoids ambiguous name mapping — cost if wrong: an index remap layer later.
Task 4: implementer DONE_WITH_CONCERNS (commit 065aadb); review dispatched (opus)
Task 4: review Approved, no Critical/Important.
Task 4: Ruling: first-divergence exceptions accepted — 5 trail weapons diverge first at rc@6 (trail child's colour draw, Task 5 scope) and the 2 lasers at ents@2 (DrawBeam, Task 8); MATCHING_WEAPONS (7 mine-like weapons now matching CHECKED) accepted as a stricter ratchet; Task 5 removes them when PROJECTILES is checked — cost if wrong: none, later tasks must make them all match anyway.
Task 4: minor (deferred → carried into Task 5): no test enforces "first divergence is a projectile field".
Task 4: minor (deferred → carried into Task 7): st/ct assume 10 ms frames; use the world's frame step (GameSpeed).
Task 4: minor (deferred → carried into Task 5): image frame counts silently None on an asset key mismatch; load_mod must error (Task 5 depends on them for ANI_ONCE).
Task 4: minor (deferred): check_ids passes vacuously on empty header lists.
Task 4: minor (deferred): projectile.rs comment on RotIncrement wrong (CProjectile::Draw reads it; renderer must reproduce 0→1).
Task 4: minor (deferred): ShootList::take_due relies on caller clear().
Task 4: minor (deferred): no_alloc test duplicates the mod builder; two too_many_arguments allows.
Task 4: minor (deferred): no tests for rl-cli --fields parsing.
Task 4: complete (commits 138a651..065aadb, review clean)
Task 5: dispatched (BASE 065aadb, implementer opus)
Task 5: implementer DONE_WITH_CONCERNS (commit 9c72174); review dispatched (opus)
Task 5: review Approved, no Critical/Important. ⚠️ resolved: controller ran rl-scenario acceptance at 9c72174 → 4/4 pass (pinned divergences real).
Task 5: minor (deferred → carried into Task 6): (delay*1000) as u64 saturates negative/NaN; OLX wraps via cvttss2si — emulate (c_i64 as u64) or record.
Task 5: minor (deferred → carried into Task 6, optional): divergence guard can hide a same-frame projectile regression sorting after the pinned field; add a second pass without effect fields.
Task 5: minor (deferred): proj_sim.rs:436 doc wording on radius.
Task 5: minor (deferred → carried into Task 7): respawn must reuse the worm's existing pos recorder (clear_to), not a fresh one (D7 stale contents).
Task 5: minor (deferred → carried into Task 6): unit test pinning attr_set's keep-sign-of-zero.
Task 5: complete (commits 065aadb..9c72174, review clean)
Task 6: dispatched (BASE 9c72174, implementer opus)
Task 6: implementer DONE_WITH_CONCERNS (commit c01647a); review dispatched (opus)
Task 6: review Approved, no Critical/Important. ⚠️ resolved: controller ran `cargo xtask check` at c01647a → passed.
Task 6: minor (deferred → carried into Task 7): WITHOUT_DRAWS hand-duplicates CHECKED minus RAND; add Fields::difference.
Task 6: minor (deferred): flags_of duplicates Terrain::flags mapping.
Task 6: minor (deferred → carried into Task 7): theme pack layout changed (greenball) without PACK_FORMAT_VERSION bump; stale imports silently place no green dirt — bump or document.
Task 6: minor (deferred): remaining saturating float→u64 casts (shot_codec, shots ping, spawn, physics air-jump) → c_u64 for consistency.
Task 6: minor (deferred → carried into Task 7): pass-2 owner labels for kill/death rows imprecise.
Task 6: minor (deferred): theme_pack match-on-bool style.
Task 6: minor (deferred): rl-terrain RED by mutation, not test-first.
Task 6: complete (commits 9c72174..c01647a, review clean)
Task 7: dispatched (BASE c01647a, implementer opus)
Task 7: implementer DONE_WITH_CONCERNS (commit dafefa0); review dispatched (opus)
Task 7: Ruling: D9 stands although exact-zero health is common with MW damage values — the Gusanos death particles' rnd() use (hundreds of draws, particle simulation) is out of scope; the observable effect is only which random spawn spot FindSpot picks next. assassin-s-shot, flak, plasma-launcher (MW) stay as documented permanent expected divergences (reason: D9); Task 11's "no expected divergences" becomes "only D9-documented ones" — cost if wrong: respawn positions after exact-zero deaths differ from OLX's sequence (not the feel).
Task 7: review Approved, no Critical/Important. ⚠️ resolved: controller ran `cargo xtask check` at dafefa0.
Task 7: minor (deferred → carried into Task 8): Worm::spawned hard-codes lives -2; apply SimSettings::lives (FT_Lives <0 → unlimited) — matters for local play.
Task 7: minor (deferred → carried into Task 8): shots_of_a_dead_worm_are_dropped asserts weakly (count <= flying, owner always 0); strengthen.
Task 7: minor (deferred): rl-data manifest.rs doc still says "add a migration".
Task 7: minor (deferred): Lives range widened to -2 without comment.
Task 7: minor (deferred): respawn loop doc omits bWeaponsReady gate.
Task 7: minor (deferred): citation/doc slips (CMap.h:458, rl-scenario CLAUDE.md Game.Particles, damage notes question 2 splice).
Task 7: complete (commits c01647a..dafefa0, review clean)
Task 8: dispatched (BASE dafefa0, implementer opus)
Task 8: implementer DONE (commit b6de4ba); review dispatched (sonnet). Note for Task 9: ProcessShot_Beam's bonus explosion check (3 px) belongs in beam::process_shot_beam before the pixel test.
Task 8: review Approved, no Critical/Important. ⚠️ resolved: controller ran `cargo xtask check` at b6de4ba.
Task 8: minor (deferred): wide-beam path (init_width>1, width_increase, DistributeDamageOverWidth, negative width) has no value-asserting unit test.
Task 8: minor (deferred): non-jetpack Special kinds drain without effect (OLX SPC_NONE); add a note.
Task 8: minor (deferred): dead-worm test's last assert lacks a message; identifies projectiles by (slot, spawn_time).
Task 8: complete (commits dafefa0..b6de4ba, review clean)
Task 9: dispatched (BASE b6de4ba, implementer opus)
Task 9: implementer DONE (commit 24c5d50); review dispatched (sonnet). Note for plan 6: import OLX weapon restriction files (.wps, e.g. Classic 'Standard 100lt') for random weapons and bonuses; World::set_bonus_weapons is the hook.
Task 9: review Approved, no Critical/Important. ⚠️ resolved: controller ran `cargo xtask check` at 24c5d50.
Task 9: minor (deferred): natural-spawn test re-implements random_long/random_pos_num; hard-code a known value.
Task 9: minor (deferred → plan 6): weapon restriction files not imported; set_bonus_weapons unused.
Task 9: minor (deferred): dropped place_bonus not reported to callers.
Task 9: minor (deferred): place_bonus may allocate if used during play (documented as setup-like).
Task 9: complete (commits b6de4ba..24c5d50, review clean)
Task 10: dispatched (BASE 24c5d50, implementer opus)
Task 10: Ruling: the smoke test lives in rl-sim only if the layer check allows rl-sim dev-deps on pack loading; else crates/rl-scenario/tests/smoke.rs — rl-sim cannot load packs at layer 3 — cost if wrong: none (test location).
Task 10: implementer DONE_WITH_CONCERNS (commit 120f62e); review dispatched (opus)
Task 10: review Approved, no Critical/Important. ⚠️ resolved: controller ran `cargo xtask check` at 120f62e.
Task 10: minor (deferred → carried into Task 11): smoke test should assert events.dropped()==0 (TerrainEdit drops would desync the renderer).
Task 10: minor (deferred → carried into Task 11): smoke.rs content_dir() skips silently when unset; doc claims a target/content default.
Task 10: minor (deferred → carried into Task 11): Message/GameOver omission not in STATUS deviations.
Task 10: minor (deferred → carried into Task 11): game sounds (OLX data paths) and mod sounds share one name table; prefix game sounds (e.g. "olx:") or document.
Task 10: minor (deferred → carried into Task 11): carry the laser-sight length in its event (no draws involved).
Task 10: minor (deferred): TickEvents lives inside Draws (World PartialEq compares output too).
Task 10: minor (deferred → plan 5 note): tick returns &TickEvents borrowing &mut self; M5 consumers should use world.events().
Task 10: minor (deferred): proj_sim.rs 510 lines.
Task 10: complete (commits 24c5d50..120f62e, review clean)
Task 11: dispatched (BASE 120f62e, implementer sonnet)
Task 11: Ruling: keep exactly the 3 D9 pins with written reasons instead of removing the expected-divergence tables; everything else must match Fields::ALL — consistent with the Task 7 D9 ruling — cost if wrong: none.
Task 11: implementer DONE (commit c9a287e); review dispatched (sonnet)
Task 11: review Approved, no Critical/Important.
Task 11: minor (deferred): weapons.rs laser_sight doc stale (says bot targets only / aim direction).
Task 11: minor (deferred): olx-notes "Answered" text prepended on the question line; STATUS tick-events bullet nested parentheses; redundant other.alive=true in a test; damage notes Q8 still open.
Task 11: complete (commits 120f62e..c9a287e, review clean)
Final review: dispatched (opus), range e807c26..HEAD
Final review: no Critical; 3 Important (hostile-mod tick stall via amount×full pool; gameplay/terrain events droppable from the shared event cap; oracle gaps: suicides, natural bonus spawns, respawn on request). Fix wave dispatched (opus) with these + fix-now minors (spawn.rs:95 RotIncrement comment/deviation note, weapons.rs:145 laser_sight doc, rl-pack CLAUDE.md migration line, respawn bWeaponsReady doc).
Final: minor (deferred → plan 5): physics_time() accessor and per-projectile spawn serial for interpolation; move pack→SimMod loader (load_mod/check_frames) out of rl-scenario into rl-content.
Final: minor (deferred → plan 6): pool capacity setting (spec §5.3); World::new validates SimSettings; canonicalise NaN in checksum; game starts at T0=100000 like the oracle; GameMode/MatchSetup API step at the start of M6; place_bonus result + pre-sized placements; weapon restriction files; natural-spawn test hard-coded value.
Final: minor (deferred): Fields(pub u32); remaining can-wait items per the final review triage.
Note (2026-10-10): user installed libasound2-plugins (+ ~/.asoundrc pcm/ctl default pulse) and libxkbcommon-x11-0; plan 5 Task 6/7 must verify real audio playback through kira.
Final fix wave: implementer DONE_WITH_CONCERNS (commits 025fe21, 1dae595, 2897b10, 1b31622); scoped re-review dispatched (sonnet).
Final: Ruling: the 48128-event buffer living inside World (≈1.15 MB per clone) is moved out of World state in plan 5 Task 1 (caller-owned or clone-excluded TickEvents) — clones are used for rollback/replay/bots — cost if wrong: an API change later.
Final: Ruling: terrain edits beyond the 32768/tick quota (hostile content only) are reported via gameplay_dropped()/lost_area(); M5's renderer rebuilds lost areas from the material grid — no finite bound exists for hostile mods — cost if wrong: approximate colours after an absurd tick.
Final fix wave: re-review → all 7 findings ADDRESSED; no new Critical/Important.
Final: minor (deferred): skip_children still makes OLX's per-child rotation draw (pool×amount next() for rotating self-trailing projectiles, ms-scale); doc wording overstates the bound.
Final: minor (deferred → plan 5 Task 1): spawn_work participates in World PartialEq (exclude with the event buffer move).
Final: minor (deferred): spawn test nit (projectile-parent case reuses weapon spawn info); amount==0 hoisted index (validated, safe).
Final: minor (deferred): Shot.random arithmetic non-wrapping in int_from / slow path (pre-existing; decoded values are bounded).
Final: minor (deferred → plan 5 Task 2/5): renderer must rebuild lost_area() from material when gameplay_dropped() > 0.
Plan 4: COMPLETE (e807c26..1b31622).
