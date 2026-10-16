# SDD ledger — plan: docs/superpowers/plans/2026-10-16-plan-13a-mod-format.md

Spec: docs/superpowers/specs/2026-10-16-formats-design.md (read). Branch: plan-13a (from main d205f95).

## Pre-flight scan

| Pair / task | Produced vs consumed | Found |
|---|---|---|
| T1 → T2 | `defaults` helpers (`d::*` names), `Colour`, `ron_io` | names match; T1 defines `three_tenths` and the `zero()` getter, which nothing uses → dead_code warnings |
| T1 → T5 | `read_text`, `write_text` (pub(crate)) | only T5 uses them → dead code from T1 until T5 |
| T2 → T3 | types and spawn fields, `Projectile::new`, `SpecialKind::Off`, `Trail::*` | match |
| T3 → T4 | `MAX_INLINE_DEPTH` | match |
| T3 → T5 | `MOD_FILE`, `weapon_file`, `shared_file`, `valid_id` (pub(crate)) | match |
| T4 check step | `cargo xtask check` (clippy is strict) | T1's `read_text` and `write_text` are still unused at T4 → clippy dead-code failure |
| T2–T5 → T6 | `olr_format::mods::*`, `Colour`, `Spawn` fields, `ModSource` | match |
| T6 | the MW snapshot picks `rocket-launcher` and `floating-bomber` | may not exist; the plan says to pick existing ones |
| T1 self | tests vs code | consistent; Review Focus 2 and 3 are in its tests |
| T2 self | tests vs code | consistent; the Step 3 fallback covers serde `from`/`into` with generics |
| T3 self | tests vs code | messages match the tests ("nested deeper than 32", "\"gun\"", "projectiles/nope.ron", "not supported by this runtime") |
| T4 self | tests vs code | the stack pop on error paths is left to the implementer (noted in the plan) |
| T5 self | tests vs code | `mutated_files_never_panic` asserts only that nothing panics; this is fuzzing, not a rubric defect |
| T6 self | tests vs code | the `prop_flat_map` closures move owned values; acceptable |
| T7 self | docs only | — |

Ruling: Task 1's `f32_defaults!`/`i32_defaults!` macros emit `#[allow(dead_code)]` on the generated getters and predicates, and the unused `three_tenths` entry is dropped — the getter of a pair is often unused (serde's `default` uses `Default`) — if wrong: a lint nit only.
Ruling: Task 4's `cargo xtask check` moves to the end of Task 5 (after `read_text`/`write_text` get their caller); Task 4 runs only `cargo nextest run -p olr-format` — clippy would fail on dead code consumed one task later — if wrong: one extra lint fix round in Task 5.
Ruling: `mutated_files_never_panic` stays as plan-written (a no-panic fuzz is the assertion) — the spec's §6 "fuzzed authoring files never panic" asks exactly this — if wrong: a weak test, caught in final review.

## Progress
Task 1: dispatched (base d205f95, implementer a67ff7d0c40c656ab, sonnet)
Task 1: implemented 89f39ee (DONE_WITH_CONCERNS: no RED captured; extra allow(dead_code) on FormatError::invalid, is_zero_i32, is_false, is_default); review dispatched (reviewer ad4e804549a2aed8b, sonnet)
Task 1: review: spec ❌ (layers.ron entry missing); fix round 1 dispatched to the implementer
Task 1: minor (deferred): to_canonical maps serialize failures to FormatError::Io (plan-mandated; a depth overflow is not I/O)
Task 1: minor (deferred): no test asserts an uppercase colour is rewritten lowercase by to_canonical
Task 1: minor (deferred): deep-nesting ron test asserts only is_err, not that the error is the recursion limit
Task 1: fix round 1/5 (1 addressed, 0 open; commits 89f39ee..58ef46f)
Task 1: complete (commits d205f95..58ef46f, review clean)
Task 2: dispatched (base 58ef46f, implementer a854cd436918c2b8a, sonnet)
Task 2: implemented f62e085 (DONE_WITH_CONCERNS: docs not checked row by row against mod-fields.md)
Task 2: review dispatched (reviewer aa89e7fe375de7b85, sonnet)
Task 2: review: spec ✅ structure, Needs fixes (Important: enums lack deny_unknown_fields [plan-mandated gap]; Important: derived float == in tests)
Ruling: enum struct variants get deny_unknown_fields (fixed in Task 2) — spec goal 3 (agents) needs typos to be errors; the plan's omission was an oversight — if wrong: stricter parsing, nothing lost.
Ruling: the derived-float-== finding needs no Task 2 change — a_weapon_round_trips_through_canonical_text also compares the re-written text, and Task 6's proptest compares written bytes — if wrong: a lost -0.0 in some field slips past Task 2 but not Task 6.
Ruling: the incomplete field docs (a reviewer Minor) enter the fix round — the brief requires them and plan 13c generates the schema from them; a per-field OLX file:line is not required where the type's doc cites the source, but every field states its meaning and unit — if wrong: 13c's docs need another pass.
Task 2: fix round 1 dispatched to the implementer (enum deny_unknown_fields, docs, tests, attribute order)
Task 2: fix round 1/5 (4 addressed, 0 open; commits f62e085..d6f0682)
Task 2: minor (deferred): Spawn.context is a public PhantomData field (plan-mandated); Worm/Rope tests live in types.rs although the types are in mods/mod.rs
Task 2: complete (commits 58ef46f..d6f0682, review clean)
Task 3: dispatched (base d6f0682, implementer a7ebb2ccc1323066c, sonnet)
Task 3: implemented a867452 (DONE; reworded two messages to match the tests; while-let in check_cycles)
Task 3: review: ✅ Approved
Task 3: minor (deferred): no compile tests for Beam/Special/Worm/Rope mapping, Image size default, non-projectile trail delay, Bounce/Injure fields, MAX_* bounds, the 32/33 depth boundary (Task 6's equivalence covers most mapping)
Task 3: minor (deferred): a NaN or negative timer after_s with no effects compiles to no action without a warning (same as OLX's time > 0 rule)
Task 3: minor (deferred): an invalid shared id reports path `id` with an empty prefix; "no spawn named" points at the projectile, not the effect index
Task 3: complete (commits d6f0682..a867452, review clean)
Task 4: dispatched (base a867452, implementer aa94008af772063d0, sonnet)
Task 4: implemented 0940169 (DONE; projectile_body split for the stack pop)
Task 4: review: Needs fixes (Important: lift depth off by one vs compile); fix round 1 dispatched (+ minors: tests for id/trail/timer/beam/special/anim, dead line, doc)
Task 4: fix round 1/5 (3 addressed, 0 open; commits 0940169..020aee5)
Task 4: minor (deferred): the depth-limit test's compile-side check does not assert the message; a dead `leaf.spawns.clear()`; trail depth at the limit is not exercised (compile's spawn adds one level for trail and event children alike; lift's stack does too)
Task 4: complete (commits a867452..020aee5, review clean)
Task 5: dispatched (base 020aee5, implementer a41bde348edbf399f, sonnet; runs the moved xtask check)
Task 5: implemented 8d60d7c (DONE; xtask check passed; raw-string fix in hostile.rs; MAX_SHARED bound tested with 4097 files)
Task 5: review: ✅ Approved
Task 5: minor (deferred): a `weapons/<listed id>` file without `.ron` is silently ignored; a walk failure reports an empty file name; write_mod_files validates ids after writing mod.ron and does not reject duplicate weapon ids up front; no bound on the listed weapon count while reading (compile bounds it)
Task 5: complete (commits 020aee5..8d60d7c, review clean)
Task 6: dispatched (base 8d60d7c, implementer abe19baf72434b089, sonnet)
Task 6: implemented bfe88fc (DONE; equivalence ran on both mods; no source fixes)
Task 6: review: Needs fixes (Important [plan-mandated]: proptest holds many fields fixed; Important: equivalence's f32 == misses a -0.0 lost in lift/compile)
Ruling: a spawn writes its parameters before `shared`/`projectile` (SpawnWire field order) — the spec §2 example shows that order, and a spawn's numbers are unreadable after a long nested projectile — if wrong: the snapshots change back, nothing else.
Ruling: the proptest strategy is strengthened to vary every authoring field (plan-mandated weakness) — the spec's §6 round trip covers random authoring values — if wrong: a slower test only.
Ruling: the equivalence test also compares the RON text of both ModDefs (olr-data's serde), catching a sign or bit lost in lift/compile — `f32 ==` alone cannot — if wrong: none.
Ruling: spec §2's example is updated in Task 7 to the canonical output (shared spawn, field order) — the spec documents the format; the snapshot is its truth — if wrong: doc churn only.
Task 6: fix round 1 dispatched to the implementer (spawn field order, proptest coverage, RON-text equivalence, shared snapshot)
Task 6: fix round 1/5 (4 addressed, 0 open; commits bfe88fc..c684180)
Task 6: minor (deferred): `recharge: 0.17999999` reads oddly but is OLX's exact f32; `Explode()` with no fields reads oddly; proptest strategies needed .boxed() to fit the test stack
Task 6: complete (commits 8d60d7c..c684180, review clean)
Task 7: dispatched (base c684180, implementer a93559eb1320eae16, sonnet; includes the spec §2 example update ruling and the final check)
Task 7: implemented 89e1924 (DONE; xtask check passed)
Task 7: review: Needs fixes (thrust row says default 0 but is required; CLAUDE.md lists spawn.rs tests that do not exist); fix round 1 dispatched
Task 7: fix round 1/5 (2 addressed, 1 Minor open — a ragged short line at the start of the spec's "The names follow plan" paragraph; commits 89e1924..e359384)
Task 7: minor (deferred): spec §2 "The names follow plan / 13a's final table" paragraph keeps a 21-character first line
Task 7: complete (commits c684180..e359384, review clean)
Final review: dispatched (range d205f95..e359384, opus)
Final review: With fixes (Important: CRLF canonical text on Windows; NaN timer drops effects; write_mod_files writes before validating ids and allows duplicates; compile accepts what SimMod rejects)
Ruling: canonical text is LF on every platform (`.new_line("\n")`) — spec §1 "one text form per value"; Windows zips ship — if wrong: none.
Ruling: a timer whose after_s is not above 0 (NaN included) with effects is an error — the crate docs and spec "never a silent change" — if wrong: one more rejected hostile input.
Ruling: write_mod_files validates every id (valid, unique) before writing anything — editors build on it — if wrong: none.
Ruling: compile_mod mirrors SimMod's MAX_AMOUNT (1000) now with a field-path error; the depth and occurrence limits after expansion are left to plan 13c's `pack check`, which will run SimMod::compile — cheap now vs. needing expansion logic — if wrong: a pack fails at match start with SimMod's message until 13c.
Ruling: Effect::Disappear's name (vs the research's `Halt`) is surfaced to the user, unchanged here — a naming call the spec made — if wrong: a rename in 13c.
Final fix wave: dispatched
Final fix wave: 8273314 (xtask check passed; 66 tests)
Final review: clean after fix wave (re-review: all addressed). Out-of-scope minors: lift does not check amount <= MAX_AMOUNT; compile does not validate amount <= 0; uneven lift_mod doc wrap.
