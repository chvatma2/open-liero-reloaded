# SDD ledger — plan: docs/superpowers/plans/2026-10-16-plan-13b-asset-formats.md

Spec: docs/superpowers/specs/2026-10-16-formats-design.md (amended with the plan). Research: docs/porting/olx-notes/asset-formats.md. Branch: plan-13b (from main 3b74625).

## Pre-flight scan

| Pair / task | Produced vs consumed | Found |
|---|---|---|
| T1 → T2, T3, T5, T6 | `image_io` (encode/decode rgba, indexed incl. low-bit, grey; `olx_sprite_to_alpha`; bounds) | match |
| T1 → olr-pack callers | `pub use olr_format::image_io::*` keeps every `olr_pack::image_io::…` path | match; the `wasm` list must gain olr-format (in the brief) |
| T2 self | tests use `crate::levels::tests::tiny` from files.rs | the levels test module must be `pub(crate) mod tests` |
| T3 → tests | `olr_terrain::{Stamp, GreenMask}` PartialEq | unknown; the brief says compare Debug if missing |
| T4 → T5 | the true-mirror runtime that the skin format relies on | ordered correctly |
| T4 self | re-bless render goldens (GPU, lavapipe) | fine |
| T5/T6 self | `olr_pack::{SkinPack, CorePack}` field names | the brief says to read them |
| T6 self | module named `core` at the crate root | **conflict**: a root module `core` makes `core::…` paths ambiguous (E0659) |
| T6 self | CORE_LAYOUT has 12 entries (11 sprites + giblet-stains) | count matches |
| T7 self | records, check, full | — |

Ruling: Task 6's module is `olr_format::core_pack` (files `src/core_pack/mod.rs`, `src/core_pack/files.rs`), not `core` — a root module named `core` shadows the `core` crate and makes `core::` paths ambiguous — if wrong: a rename.
Ruling: the levels unit-test module is `pub(crate) mod tests` so files.rs's tests can reuse `tiny()` — the plan's tests reference it — if wrong: none.

## Progress
Task 1: dispatched (base 3b74625, implementer a1379884418318007, sonnet)
Task 1: implemented 1cc0100 (DONE_WITH_CONCERNS: re-keying a 254-keyed alpha image strips real magenta, so olx_sprite_to_alpha is once-only; olr-format dev-depends on olr-pack, a dev cycle cargo accepts)
Carry to 13c: loaders must stop keying (olr-scene/olr-fx `keyed`) in the same change that switches packs to alpha images, or a 254-keyed sprite with real magenta loses it.
Task 1: review: ✅ Approved
Task 1: minor (deferred): olx_sprite_to_alpha's doc does not state that applying it twice is unsafe (a 254-keyed image loses real magenta); the test does not show the non-idempotent case; decode_indexed does not check indices against the palette length (as before)
Task 1: complete (commits 3b74625..1cc0100, review clean)
Task 2: dispatched (base 1cc0100, implementer ada3b4fd7809a3c20, sonnet)
Task 2: implemented 7acd073 (DONE; MAX_SIDE 4096, 64 MiB read limit, lift checks like compile, material size check; thumb over 64 MiB is an error)
Task 2: review: ✅ Approved
Task 2: minor (deferred): a bad or oversized thumb.png fails the level read (presentation-only; olr-pack skips a bad one and caps it at MAX_THUMB_BYTES 1 MiB) — candidate for the final fix wave
Task 2: minor (deferred): no synthetic art-scale-2 case in tests/levels.rs (unit tests cover tiny(2)); a level.ron size mismatch is reported against material.png with an empty path; an unreachable from_cells None branch
Task 2: complete (commits 1cc0100..7acd073, review clean)
Task 3: dispatched (base 7acd073, implementer a5e42520bba0b57d4, sonnet)
Task 3: implemented bc90370 (DONE; concern: a stone/misc pixel with RGB magenta and alpha != 0 compiles but lifts as transparent; writer zeroes paint outside mask 2; zero-sized stamp is an error)
Task 3: review: Needs fixes (Important, plan-mandated: compile keeps stone/misc pixels that change meaning — opaque magenta reads as transparent; a (254,0,254) corner re-keys the image)
Ruling: compile_theme rejects a stone/misc pixel with alpha != 0 whose RGB is magenta, and a corner pixel with alpha != 0 whose RGB is (254,0,254), naming the file and pixel — the plan's own rule that compile rejects colours that would change meaning; the brief's "every other pixel is kept" was an oversight — if wrong: such art must use a nearby colour.
Task 3: fix round 1 dispatched
Task 3: fix round 1/5 (1 addressed, 0 open; commits bc90370..9d6e84b)
Task 3: minor (deferred): mask 0/1 pixels' paint is silently dropped (canonical); hole paint with alpha 0 and a non-black colour compiles untested; no lift test for an oversize or empty stamp
Task 3: complete (commits 7acd073..9d6e84b, review clean)
Task 4: dispatched (base 9d6e84b, implementer a33e7c3fa7fd5b6e4, sonnet)
Task 4: implemented 7d5b088 (DONE; xtask check passed; re-blessed frame-castle and frame-dig only; controller verified the diffs: castle 91 px in x 314..329 y 231..248, dig 289 px in x 876..902 y 349..373 — one worm-sized region each)
Task 4: review: ✅ Approved
Task 4: minor (deferred): every_bundled_skin_mirrors can pass with zero olx-skin-* packs (assert a count); mirror equality holds by construction (guards only skin_source/skin_sprite regressions); OLX citation line ranges differ (584-600 vs 597-601)
Task 4: complete (commits 9d6e84b..7d5b088, review clean)
Task 5: dispatched (base 7d5b088, implementer ad4a9b537f96b20ae, sonnet)
Task 5: implemented 8c130aa (DONE; bundled skin test 0.94 s over >= 700 skins)
Task 5: review: ✅ Approved
Task 5: minor (deferred): CLAUDE.md skins bullet unwrapped; lib.rs doc still says "Plan 13a: mods"; a partial tint under a fully transparent pixel is rejected (tint is unobservable there)
Task 5: complete (commits 7d5b088..8c130aa, review clean)
Task 6: dispatched (base 8c130aa, implementer ae15a19ff6a8993bb, sonnet; module core_pack per ruling)
Task 6: implemented 2a4d95c (DONE; module core_pack, tests/core_pack.rs; extra bounds MAX_SPRITES 256, MAX_SOUNDS 1024, MAX_COLOURS 1024; name validation)
Task 6: review: ✅ Approved
Task 6: minor (deferred, fix in final wave): read_core_files reads every listed sprite/sound before check() enforces the counts (a hostile core.ron could hold ~1024 × 64 MiB) — check the counts up front
Task 6: minor (deferred): the STAINS check runs after keying (wasted work); custom frames/art_scale on giblets are not carried to the runtime (by design; document); allow(type_complexity) on compile_core's tuple
Task 6: complete (commits 8c130aa..2a4d95c, review clean)
Task 7: dispatched (base 2a4d95c, implementer a8d23addbacd7829e, sonnet; check + full)
Task 7: implemented 8d82d51 (DONE; check passed; full passed, all 17 steps)
Task 7: review: ✅ Approved
Task 7: minor (deferred): "kept for editors only" wording on LevelObject::Spawn; tests paragraph says "skip when content/packs is absent" (it is OLR_CONTENT_DIR)
Task 7: complete (commits 2a4d95c..8d82d51, review clean)
Final review: dispatched (range 3b74625..8d82d51, opus)
Final review: With fixes (Important: read_core_files reads before checking counts; level/skin/core/theme checks do not apply the reader's pixel bound, so a saved file can fail to reopen)
Ruling: every kind's check() applies the same pixel bound its reader uses (LEVEL_ART for level art, IMAGES for skins, core sprites, stones, misc, tiles) — goal 1: what an editor saves must reopen — if wrong: none.
Ruling: mask-2 stamp pixels whose paint has alpha 0 are errors in both stamp kinds ("the paint layer is empty there") — the likely editor mistake, and a transparent rim still places dirt — if wrong: an author must use a visible colour.
Ruling: the png compressor crates stay unpinned; a golden-bytes test of encode_rgba catches drift — check's fresh import also would — if wrong: a lockfile bump re-blesses that test.
Ruling: the oversized-thumbnail behaviour is left for 13c (the thumbnail split between olr-pack and olr-format) — it only fails above 64 MiB — if wrong: a level with a huge thumbnail fails to read until 13c.
Final fix wave: dispatched
Final fix wave: 3541b0e, 19f76a2 (237 tests; check --offline passed — olx-content skipped; controller re-runs online check)
Final review: clean after fix wave (re-review all addressed; online xtask check passed incl. olx-content)
