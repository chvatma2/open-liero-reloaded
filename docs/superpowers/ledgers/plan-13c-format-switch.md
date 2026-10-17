# SDD ledger — plan: docs/superpowers/plans/2026-10-17-plan-13c-format-switch.md

Spec: docs/superpowers/specs/2026-10-16-formats-design.md. Research: docs/porting/olx-notes/format-switch.md. Branch: plan-13c (from main 62e9728).

## Pre-flight scan

| Pair / task | Produced vs consumed | Found |
|---|---|---|
| T1 → T6–T8 | the runtime digest snapshot every switch task must keep | match |
| T1 self | needs olr-fx `colorize_gibs` | may need a `#[doc(hidden)] pub` re-export (in the brief) |
| T2 ↔ T3 | both edit olr-game shell/setup | sequential, no conflict |
| T3 → T4 | T3 removes olr-ui's use of `olr_content::import` before T4 moves it | ordered correctly |
| T4 self | `olr_pack::install` prefixes, `forbidden` rule in layers.rs | match |
| T5 → T6–T8 | `read_level_header`, `read_level_files_without_thumb`, tolerant thumb/core reads, structural `lift_core`, `PackError::Format` | match |
| T5 self | structural `lift_core` changes olr-format's core equivalence test | the brief says to key old sprites before lifting |
| T6 self | version 5 bump re-imports every pack; skins/mods/core keep format-4 files with format 5 manifests until their task | the brief's ruling covers it |
| T8 self | keying removal + importer keying + render goldens in one change | GPU tier available (lavapipe) |
| T9 self | validate over new readers; importer tests `validate == []` | match |
| T10 self | records, check, full | — |

No conflicts needing a ruling.

## Progress
Task 1: dispatched (base 62e9728, implementer a9c396721d95983e6, sonnet)
Task 1: implemented aafe8b0 (DONE; 726 lines; gib test passes; check passed; digest keys every mod image — harmless, same rule before and after)
Task 1: review: Needs fixes (Important: level name/art_scale/theme/spawns unhashed; every mod image keyed although the runtime keys only projectile images)
Ruling: the digest hashes the images the runtime uses — each projectile `gfx` image (keyed once, as olr-scene does) — and leaves unreferenced mod files out; core sprites stay keyed (every core sprite is keyed by olr-scene or olr-fx; the stain's raw path is covered by the gib test); levels add name, art_scale, theme, spawns; themes add the name; skins add the def — the digest must prove equality of what the game receives, and must be complete before any switch task — if wrong: a later digest change would invalidate the baseline.
Task 1: fix round 1 dispatched
Follow-up approved by the user (2026-10-17): after 13c merges, implement replay looks (ReplaySetup.looks: name, colour, skin id + hash per worm; FORMAT_VERSION 3→4; names sanitised ≤ 20 chars; recorders pass looks: local profile + bot names, online lobby looks, olr-cli bot names; playback shows names/colours like a live game; skins by 13c's skin_matching rule) — bounded change, own branch, design approved in chat.
Task 1: fix round 1/5 (3 addressed, 0 open; commits aafe8b0..b922efd; snapshot 730 lines)
Task 1: complete (commits 62e9728..b922efd, review clean)
Task 2: dispatched (base b922efd, implementer a808b31a4e14588c4, sonnet)
Task 2: implemented b922efd..861a8d2 (7 commits; check passed; olr-content has own NO_SKIN_HASH/DEFAULT_SKIN constants tested equal; server still opens its download cache)
Task 2: review: ✅ Approved (opus)
Task 2: minor (deferred, final wave): fetch.rs:2 module doc still says "skins after"; the NO_SKIN_HASH/DEFAULT_SKIN guard asserts sit after the OLR_CONTENT_DIR skip (make unconditional); online CLAUDE.md says N31 "removed" (replaced)
Task 2: minor (deferred): load_online decodes skins per slot (fine at <= 32); the "warning once" assertion cannot fail; the server still opens its download cache (cleanup candidate)
Task 2: complete (commits b922efd..861a8d2, review clean)
Task 3: dispatched (base 861a8d2, implementer ab0a03e999515e45d, sonnet)
Task 3: implemented bd92a0d, 7fd1a9a (DONE; 278 tests; check passed; shell owns packs_dir)
Task 3: review: ✅ Approved
Task 3: minor (deferred): olr-cli/src/import.rs:1-2 doc still names the Import screen (carried into Task 4's dispatch); user_packs test draws Local Play without asserting the drawing; a shell.rs comment typo; a store.rs doc wrap
Task 3: complete (commits 861a8d2..7fd1a9a, review clean)
Task 4: dispatched (base 7fd1a9a)
Task 4: implementer agent a06863f28e09204be (sonnet)
Task 4: implemented df4823c..e425587 (ruling: moved tests take higher-layer crates as dev-deps only)
Task 4: review: ✅ Approved
Task 4: minor (deferred, final wave): olr-import/CLAUDE.md:10-11 wrap; tests/import.rs header rewrap; olr-pack install.rs OLD_PREFIX doc should name olr_content::Content::open_layers; optional xtask/CLAUDE.md line on forbidden
Task 4: complete (commits df4823c..e425587, review clean)
Ruling: Task 5 runs targeted nextest + clippy/fmt on olr-format and olr-pack only; the online check runs once at the end of Task 6, covering both — the user wants fewer verification rounds — a Task 5 regression surfaces one task later
Task 5: dispatched (base e425587), implementer ab58192ba415603b8 (sonnet)
Task 5: implemented af4f6d8 (concern: no MAX_THUMB_SIDE in olr-format)
Task 5: review: ✅ Approved
Task 5: minor (deferred, final wave): read_thumb fully decodes (use a header-only size check: png_size, MAX_THUMB_SIDE); sound loop duplicated strict/tolerant; no error-order test, no wrong-size-giblets test; olr-format CLAUDE.md wrap; olr-pack error.rs FormatError import used inconsistently
Task 5: complete (commit af4f6d8, review clean)
Task 6: dispatched (base af4f6d8), implementer ac0669de88b5046d7 (sonnet); its online check also covers Task 5
Task 6: implemented 622a1b8 (code), 117a06a (content); digest unchanged; check green
Task 6: review: ✅ Approved
Task 6: minor (deferred, final wave): STATUS.md:67,:832 thumbnail rows say format 4; cache.rs open_drops_a_format_4_entry leftover `skin_pack`/`let _ = fs`; level_pack.rs:132-135 test imports grouping; theme_pack.rs pack_with_green inline; olr-pack CLAUDE.md:15 wrap
Task 6: complete (commits 622a1b8, 117a06a, review clean)
Ruling: Task 7 re-imports and runs targeted tests plus the digest, but not the online check; Task 8's online check covers both (batching per user feedback) — a Task 7 failure surfaces one task later
Task 7: dispatched (base 117a06a), implementer ac462f47f7ba433be (sonnet)
Task 7: implemented c307f0c (code), dbd0d6e (content); digest fails for 401 skins, all from spacing columns that format 5 drops (13b research §2: never drawn; skin_source and skin_sprite cut 20-px windows at f*32+4)
Ruling: the digest hashes what is drawn. skin_digest hashes the colorized sheet's drawn windows (skin_sprite for every frame, both faces), not the whole sheet. Proof: the digest change is committed on HEAD with its snapshot, then cherry-picked onto 117a06a in a temporary worktree, where the digest test must pass against the same snapshot, so the format switch stays lossless for drawn pixels. No bless log entry (an insta snapshot, not a golden) — if wrong, the hidden columns differed somewhere we draw, which the render goldens in `full` would catch
Task 7: fix commits 6ee7990 (digest hashes drawn frames; proven on 117a06a with the same snapshot), 46cbdb8 (dead load_skin branch removed)
Task 7: review: ✅ Approved (renderer samples by textureLoad within the source rect, so the spacing columns are never drawn: digest ruling confirmed)
Task 7: minor (deferred, final wave): asset-formats.md:209-210,:248 label "(pre-13c, format 4)"; digest.rs ~125 comment add "(plan 13c: format 5 drops the spacing columns)"
Task 7: complete (commits c307f0c, dbd0d6e, 6ee7990, 46cbdb8, review clean)
Task 8: dispatched (base 46cbdb8), implementer aa7bdaac0dca4ffd2 (sonnet); digest drops its keyed() helper in the same change (snapshot must stay identical); the gib keying-order test is deleted; the online check also covers Task 7
Task 8: implemented e44a483 (code, includes a Task 7 skin importer fix: spacing pixels hidden), 5bc1ab8 (content); digest identical; check green; GPU goldens 14/14 + scenario 15/15 on llvmpipe
Task 8: review: Changes requested (Important: mod gfx keying untested; minors: dead validate checks, validate.rs:282 doc, image_io.rs:227 stale, STATUS.md:42 → Task 10)
Task 8: fix round 1 dispatched (resume implementer): mod gfx keying tests + image_io.rs doc; validate.rs minors carried to Task 9 (dead check_mod projectile/cycle checks, :282 doc); STATUS.md:42 carried to Task 10
Task 8: fix round 1: 5be7456 — controller re-review (48-line test-only diff): both tests assert corners transparent, magenta kept, non-PNG bytes kept with a warning ✅
Task 8: complete (commits e44a483, 5bc1ab8, 5be7456)
Task 9: dispatched (base 5be7456); carries Task 8's validate.rs minors
Task 9: implementer a1f9b877a56b0c634 (sonnet)
Task 9: implemented 31353a4; check green; corpus snapshot unchanged
Task 9: review: ✅ Approved (compile_mod's check_cycles is iterative, MAX_INLINE_DEPTH 32, MAX_SHARED bound)
Task 9: minor (final wave): add a long shared-chain test in olr-format compile.rs (replaces the deleted 200k find_cycle test's coverage); reports_cycles could assert file starts projectiles/
Task 9: complete (commit 31353a4, review clean)
Ruling: Task 10 is docs only (no check, as docs-only commits need none); the single online check + `cargo xtask full` run once after the final review's fix wave, so 'full once at plan end' covers the fixes too — cost if wrong: a full failure surfaces after the final review rather than before it
Task 10: dispatched (base 31353a4), implementer a1c80c73650309950 (sonnet), docs only + STATUS.md:42
Task 10: implemented 2d981d6 (docs; also amended spec §9: pack check/fmt/new, level ops, schema docs → 13d, matching the plan)
Ruling: Task 10's docs-only diff is reviewed inside the final whole-branch review (opus) instead of a separate task review — it is all records, and the final reviewer reads every doc against the code anyway — cost if wrong: none beyond a finding arriving one stage later
Task 10: complete (commit 2d981d6)
Final review: dispatched (main..2d981d6)
Final review: ready after fixes; I1 Important (olr-cli import always prunes: deletes other packs in --out; README's command imports nothing and wipes <data>/packs), M1–M8 Minor, plus ledger minors (report: final-review.md)
Ruling (I1): pruning becomes opt-in (`--prune`, passed by xtask fetch); `--all` imports everything `discover` finds (what the removed Import screen did); README, STATUS D6 and CHANGELOG fixed; test: two imports into one --out keep both packs — the user layer is now filled only by the CLI, so it must be additive
Ruling (M8): skip — at most 32 slots, informational
Ruling (set aside): many large images in one downloaded mod/theme can use GiBs — predates 13c, recorded as a follow-up for the user, not fixed here
Final fix wave: dispatched (base 2d981d6)
Final fix wave: 38d78a4..5264f48 (7 commits); online check green; M7 not fixed (server's cache pinned by N30 tests)
Ruling (M7): parked — the server holding <data>/downloads is plan 11's N30 behaviour with its own tests; dropping it is a behaviour change outside 13c — cost if wrong: one idle lock file on servers
Final: re-review dispatched; `cargo xtask full` started
Final re-review: clean (nit: olr-content maps.rs ~94 doc wrap — done with the archive commit)
Final full: fmt/clippy/layers/wasm/olx-content/determinism/bot-grid/gpu/audio/e2e/net-nat/net-feel/determinism-windows/arm64 ok; test FAIL (olr-net blackout timing, concurrent load; passes 3/3 alone); bench + stress FAIL (bots_8_tick +31 %, tick_32_worms +24 %, frame_1080p +19 %, MW stress tick +10-17 %), repeated on a quiet re-run; 13c does not touch olr-sim → comparing with main in a worktree
Bench investigation stopped at the user's request (PC in heavy use; no benchmarks without explicit agreement). The main worktree has been removed.
Final: re-review clean; full green except load-driven failures (olr-net blackout passes alone; bench/stress not re-verified — PC in use, no benchmarks without the user's agreement). Follow-ups: decode memory for many large images in one downloaded mod/theme (predates 13c); M7 server cache (parked); bench re-run on an idle machine when the user agrees.
