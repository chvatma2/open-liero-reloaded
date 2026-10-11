# SDD ledger — plan: docs/superpowers/plans/2026-10-11-plan-7-menus-hud-v0-1.md

Ruling: Work directly on main — user's standing choice — cost if wrong: commits need moving to a branch.
Ruling: Plan specifies interfaces, references, named tests and acceptance (as plans 4–6) — user asked to finish without stopping — cost if wrong: more implementer judgement, caught by reviews.
Ruling: Execution via subagent-driven-development — cost if wrong: token cost.

## Pre-flight scan
| Tasks | Produces vs consumes | Finding |
|---|---|---|
| T1→T2..T9 | UiState/AppCommand, Settings/Profiles/Store | consistent |
| T2 vs T6 | T2 shows level thumbnails; T6 imports them | CONFLICT → Ruling Q1 |
| T5 vs T6 | T5 builds the Modern HUD; T6 the Classic style + fonts | consistent (T6 extends) |
| T3 self | skin preview needs a GPU texture path (rl-render/rl-present) while rl-ui holds no GPU handles | → Ruling Q2 |
| T7 self | import logic lives in rl-cli (L6); rl-ui/rl-game cannot use rl-cli | → Ruling Q3 |
| T8 self | autoplay needs the app's states from T2–T7 | consistent (last implementation task) |
| T9 self | local git tag v0.1.0 (never pushed) | consistent |

Ruling Q1: Task 2 shows a placeholder (the level name and size) when a level pack has no thumb.png; Task 6 adds thumbnails, and the grid then shows them — cost if wrong: none.
Ruling Q2: rl-ui keeps view models only; rl-game owns the egui texture upload for the skin preview (via rl-present/rl-render helpers); rl-ui's widget takes an egui TextureId — cost if wrong: a small API move.
Ruling Q3: the import pipeline (rl-cli's import command logic) moves to rl-content (L4) or a small L4 crate so rl-cli and rl-game share it, mirroring R5 in plan 6 — cost if wrong: a function move.
Task 1: dispatched (BASE 4059a44, implementer opus)
User (2026-10-11): add an engine stress benchmark (32 players, settable loading time → many entities) for optimization and regression testing. Added to plan 7 Task 8 (commit above).
Task 1: implementer DONE (commits 0571fa4, ca690bb, c2a68b5, 5d6fe44; range from 4059a44 includes plan commits 6c6da85/e802f3f); review dispatched (opus). Notes: Deathmatch::ended_at excluded from the checksum (derived; keeps replays/goldens); settings mirror types in rl-ui; Bindings lack serde(default) (Task 4).
Task 1: review Needs fixes — 1 Important (sanitized-but-valid user files moved to .bak and never rewritten → settings/profiles silently reset on next launch). Fix round 1 dispatched (resumed implementer) incl. minors 2 (MatchOver unpause command), 5 (backup limit blocks saves), 8 (opt-in prune skipped without a tag).
Task 1: minor (deferred → carried into Task 4): ApplyVideo carries the whole VideoSettings — only request a paced resize when the window preset/fullscreen actually changed; bindings never sanitized (unreachable action) and Bindings lacks serde(default).
Task 1: minor (deferred → carried into Task 2): Replays main-menu entry needs a Screen/command (G1 list omits it); MatchPrefs lacks loading time, bot personality, per-bot difficulty.
Task 1: minor (deferred): no directory fsync after rename and fixed temp name (two instances race); BotConfig::validate no upper bound; long state test scenario; tidy.rs long doc line.
Task 1: fix round 1 implementer DONE (commit da8f992); scoped re-review dispatched (sonnet)
Task 1: fix round 1/5 (1 Important + 3 Minor addressed, 0 open; commits 5d6fe44..da8f992)
Task 1: minor (deferred): backup rotation deletes the oldest before the copy/rename (only at 100 backups; harmless).
Task 1: complete (commits 4059a44..da8f992 (0571fa4, ca690bb, c2a68b5, 5d6fe44, da8f992), review clean after 1 fix round)
Task 2: dispatched (BASE da8f992, implementer opus)
Task 2: implementer DONE (commits 2384abf, 97d1139); review dispatched (sonnet).
Task 2: Ruling Q4: presentation-only files (level thumb.png, and any later preview art) are excluded from content_hash so adding thumbnails never invalidates replays — carried into Task 6 — cost if wrong: replays don't detect a changed thumbnail (irrelevant to gameplay).
Task 2: note for Task 4: saved window size/fullscreen not applied at startup yet.
Task 2: review Approved, no Critical/Important. ⚠️ resolved: controller ran debug rl-game with no flags, --mode deathmatch and sandbox flags (--autoquit-after) → no panic (GAME_FLAGS ids consistent with clap).
Task 2: minor (deferred → carried into Task 5): Esc/F5 via egui key_pressed may auto-repeat (pause flicker) — ignore repeats / edge-detect.
Task 2: minor (deferred → carried into Task 3): repair a missing saved level selection (select the first listed); replay list shows header errors up front.
Task 2: minor (deferred): symlinked/non-UTF-8 replay names dropped; stale preset for a mod without presets; synchronous unbounded thumbnail decoding and stretched aspect (Task 6 thumbnails); error message overflow; CLI error text lost the flag names; auto-save numbering wrap; coverage gaps (replay playback via the screen, loose missing-content assertion, results Start unreachable from widgets).
Task 2: complete (commits da8f992..97d1139, review clean)
Task 3: dispatched (BASE 97d1139, implementer opus)
Task 3: implementer DONE (commit d78ddd0); review dispatched (sonnet). Note: replays draw every worm with the default skin (skins not recorded); bots wear OLX's default skin.
Task 3: review Approved, no Critical/Important (bots on the default skin is faithful to OLX).
Task 3: minor (deferred → carried into Task 5): bot colours — port OLX's 7 cpuColours (ProfileSystem.cpp:50-60) for bots and skip colours close to the human's; today a 4-colour cycle repeats from worm 4 and bot 2's green sits next to the human default.
Task 3: minor (deferred → post-v0.1): replays draw every worm with the default skin — record cosmetic metadata (human skin/colour) in the replay header outside the checksummed sim state; track in STATUS.
Task 3: minor (deferred): 42 preview textures re-uploaded per colour change (an atlas would be 1); preview relies on the game's continuous repaint; skin search case handling; untested missing skin_id / narrow sheet.
Task 3: complete (commits 97d1139..d78ddd0, review clean)
Task 4: dispatched (BASE d78ddd0, implementer opus)
Task 4: implementer DONE (commit 5ccbc0c); review dispatched (sonnet)
Task 4: review Approved (sonnet), no Critical/Important. ⚠️ verified by controller: Requests::opened at app.rs:269 after window creation; GAME_FLAGS excludes video flags so --fullscreen etc. keep the menu.
Task 4: minor (carried → Task 5): GAME_KEYS (Esc/F5/F2/Tab/H) only rejected in capture UI — strip them in sanitize; capture must swallow the captured input.
Task 4: minor (deferred): repair may restore a default key already used elsewhere silently; pacing test re-implements wiring (apply_video/set_uncapped/reconfigure untested); Desktop preset uses primary monitor on multi-monitor; GPU info present-mode text goes stale; reverting sends a same-size resize; a drifting stick can be captured.
Task 4: complete (commits d78ddd0..5ccbc0c, review clean)
Task 5: dispatched (BASE 5ccbc0c, implementer opus; carried: Esc/F5 edge-detect, OLX cpuColours, GAME_KEYS sanitize + capture swallow)
Task 5: implementer DONE (commits e00de06, 55c9a3f); proposed rulings: weapon select after load + Session::rebuild; Rematch→reselect, F5 keeps weapons; Profile.weapons by name (serde default); bot colours OLX 7 skip dist<120; Esc/F5 repeat:false; capture swallows at app + screens; HUD scale cap; results columns OLX+score/deaths/suicides
Controller: 9d958c2 xtask tidy prunes stale deps executables (keep 3 newest per stem); target 26.5 → 15.4 GB. Note: Task 5 review range is 5ccbc0c..55c9a3f (9d958c2 is outside it).
Task 5: review Approved (opus), no Critical/Important; all 8 proposed rulings accepted (→ Task 5: Ruling R5.1–R5.8 as listed in the implementer line above).
Task 5: minors sent back to implementer for a fix round: arm() restores player on rebuild error; ScrollArea for results/weapon select at UI scale 3 on 720p; headless hud::show test across sizes/scales (Review Focus 5); SaveReplay idempotent; rematch loading label; misleading test comment; capture swallows F2/H/Tab in App::on_key; OLX citation line drift; snapshot re-record logged like bless-render.
Task 5: minor (deferred): e00de06 alone does not build (bisect only); snapshot stability on other adapters unverified.
Task 5: fix round 40f720e (all 9 minors; tests a_refused_choice_leaves_the_match_playable, buttons_stay_on_a_small_screen, hud_paints_at_every_scale, save_replay_once_per_match, rematch_shows_loading, snapshot_by_products_are_no_goldens); controller verified Capture is fed before App::on_key (handler.rs) so the early return keeps capture working. HUD snapshots now re-recorded via bless-render.
Task 5: complete (commits 5ccbc0c..40f720e, review clean; 9d958c2 controller tidy in range)
Task 6: dispatched (BASE 40f720e, implementer opus; carried: thumbnail letterbox + bounded/lazy decode, HUD style option, About from main menu)
Task 6: USER direction (2026-10-11): Classic bitmap HUD/fonts may be dropped if making low-res art work at 4K costs more than it's worth — relayed to implementer; thumbnails + About stay in scope.
Task 6: implementer DONE (49a223d, 84f9dd9, 31b59eb, aa46fe5); proposed rulings T6.1 drop Classic HUD + bitmap import (user's call; §21 G5/G6 narrowed); T6.2 format 4 covers thumb.png only; T6.3 thumbnail uploads via ctx.load_texture in rl-ui (Q2 is for skin preview); T6.4 MAX_THUMB_BYTES 1 MiB
Task 6: review Approved (opus), no Critical/Important; T6.1–T6.4 accepted (→ rulings).
Task 6: minors sent to implementer fix round: read_level stops reading thumb (unbounded read OOM) + bounded level reads; bounded read in ContentSource + regular-file check (FIFO hang); stale options doc; About "bundled" → imported; MAX_THUMB_BYTES doc.
Task 6: minor (carried → Task 7): thumbnail cache keyed by level id, cleared only by set_catalog — in-app import must call it.
Task 6: minor (deferred): thumbnails drawn LINEAR at UI scale 3 unverified by eye; Liero date / full OLX LGPL coverage unverified.
Task 6: fix round 2aab9a5 (ContentSource read_max/read_prefix/file_len, DirFs regular files only, all rl-pack reads bounded MAX_FILE_BYTES 64 MiB / MAX_RON_BYTES 4 MiB, read_level skips thumb) + f9f3ec7 (thumbs via ContentSource, Catalog::sources origins in About, "imported" wording). check passed (test 37.4 s — watch the budget).
Task 6: complete (commits 40f720e..f9f3ec7, review clean)
Task 7: dispatched (BASE f9f3ec7, implementer opus; carried: set_catalog refresh after import)
Task 7: implementer DONE (2a318e9, 4a04a50); proposed rulings T7.1 no prune, replace in place; T7.2 core only if all 18 files; T7.3 packs/ over --content by id; T7.4 no game start during import; T7.5 revision "local"; T7.6 no file dialog; T7.7 atomic vs process exit only (no fsync)
Task 7: review Changes requested (opus): Important — image decode unbounded (PNG decompression bomb: 560 KB → 570 MB RSS; 64 MiB → ~17 GB abort) in rl_pack::image_io::decode_rgba/decode_indexed, reached by in-game import and mod gfx at match start. T7.1–T7.7 accepted (→ rulings).
Task 7: fix round sent: IHDR side limit before allocation + bomb test; minors: restore .replaced- on crash window, remove_dir(old) failure → warning, cancellable discover, follow symlinked dirs, path ~/quotes/trim, show_rows, replay-survives-reimport test, verify discover citations.
Task 7: minor (deferred): two game instances importing at once delete each other's temp dirs; check_replay_packs could fall back to a lower-layer pack with matching hash.
USER (2026-10-11): add fast-forward to Task 8 — `--speed` + fast-forward keys for bot-only matches/spectating and replays (sim untouched; never for human play/net). Replay seeking optional. Tournament command → bot-skill milestone.
USER bug report: game said "AMD Radeon" but PC has RTX 5070 Ti. Root cause: WSL relaunch sets GALLIUM_DRIVER=d3d12 without an adapter, Mesa picks the first DXGI adapter = iGPU. Controller fix (uncommitted, waiting for Task 7 fix round to build): wsl::discrete_adapter (libcuda.so.1/nvidia-smi in /usr/lib/wsl/lib) → MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA unless user set it; tests wsl_prefers_the_discrete_gpu_unless_the_user_chose, nvidia_is_found_from_wsl_driver_files (RED seen). glxinfo confirms selection.
Controller: 19b152e rl-game WSL NVIDIA adapter (tests 3/3, check passed; release rl-game reports "GPU D3D12 (NVIDIA GeForce RTX 5070 Ti)").
Task 7: fix round bb28b64 (image decode bounds: IMAGES side ≤16384 & ≤16 Mi px; LEVEL_ART side ≤32768 & ≤64 Mi px; bomb tests RED→GREEN) + f4ea760 (8 minors). check passed (test 23.2 s warm).
Task 7: complete (commits f9f3ec7..f4ea760, review Important fixed with RED→GREEN tests)
Task 8: dispatched (BASE 19b152e, implementer opus; + fast-forward --speed for spectating/replays, optional replay seek). Task 7 scoped re-review of bb28b64/f4ea760 dispatched (sonnet) in parallel.
Task 7: scoped re-review (sonnet): Fixed — every PNG decode bounded via image_io; no Important.
Task 7: minor (deferred → final fix wave candidates): restore_leftovers picks oldest of .replaced-<id> / ~k (should be newest); stale .replaced- can resurrect a deleted pack; rl-import-lxl Vec::with_capacity(need+1) up to 117 MiB from header; decode_indexed_within could reject non-indexed before allocating; no .lxl hi-res bomb test; >8 Mpx hi-res .lxl silently falls back to art scale 1.
Task 8: implementer DONE (aab4134, 37e2d0d, 8f16be4, dc71661, 2b6a7de, 02aac95); check ~28 s, full 103 s warm; proposed rulings T8.1–T8.9 (FF only unplayed games; sound policy; PgUp/PgDn in GAME_KEYS; screenshot sanity 16 colours/99.5%; e2e software GL; full-tier stress 20 s×4×3, gate p50s only; content benches in rl-cli; machine-local baselines; seek + menu spectate deferred)
Task 8: review Changes requested (opus): Important — --fake-clock without --autoplay speeds up a played game (2 ticks/frame). T8.1–T8.9 accepted (→ rulings). full 106 s passed for reviewer.
Task 8: fix round sent (Important + minors 2–8: script clock at 1× in menus, bench missing measurement fails, bless merges, stress peak_mib gated +25%, MAX_BUDGET for 2×–16×, Session::reset recomputes human, host fingerprint → notice on mismatch).
Task 8: deferred (user-optional): replay seeking; spectating from the menus (--spectate covers it).
Task 8: fix round 4633568 + 9c7bc0c (fake clock requires autoplay — controller verified the release binary refuses; 1× keeps owed ticks; FF 2×–16× keep MAX_BUDGET with reached-speed HUD; Session::human private, recomputed; bench missing measurement fails unless skipped; bless keeps unmeasured; peak_mib gated +25%; host fingerprint → notice). check 27 s, full 111 s. Proposed: budget-cut FF frame plays no sound; mismatched host skips divan benches (stress still runs).
Task 8: complete (commits 19b152e..9c7bc0c, review Important fixed with tests)
Task 9: Ruling: the v0.1.0 tag is created by the controller after the final whole-branch review fix wave, not in Task 9 — the tag must mark reviewed code — cost if wrong: none (tag is local, moves trivially).
Task 9: dispatched (BASE 9c7bc0c, implementer opus): gates, scripted runs 4 levels × 2 mods (real GPU if available), README/STATUS/spec §1.3/CHANGELOG, commit (no tag).
Task 9: implementer DONE (ac95408, 8408eb7, c578b33, 20c03dd, 916e2ea, aa26eca). full 115.8 s, check ~30 s. 8 scripted runs on RTX 5070 Ti real window: ~60 FPS p50, p1 ≥51; tick p99 ≤0.54 ms. Open: HUD shows placeholder loadout during weapon select; minimap/bottom-left panel cover playfield corners (worm hidden at clamped edges); worm names draw over minimap/dialogs and clip at screen edge; autoplay lost clicks in real window (3/14; not on Xvfb); spec M1 importer fuzz targets never existed.
Task 9: complete (commits 9c7bc0c..aa26eca; tag deferred to after final review per ruling)
Final review: dispatched (opus) over 4059a44..aa26eca (39 commits)
Final review (opus): ready with fixes; no Critical. Important 1: minimap (alpha 128) + bottom-left panel cover ~8% of the playfield, worms (incl. the human) hidden at clamped edges. check + full passed for the reviewer (full 1:43).
Final: re-grade: Important 1 stands (affects play). Minors 1 (HUD placeholder loadout), 2 (names clip at screen edge), 3 (typed replay path read unbounded / FIFO hang), 4 (newer-version settings moved aside, stderr-only notice), 5 (stale docs: 103/106 → 109/112, spec HUD style line, game key list, M1 fuzz amendment) enter the fix wave under the user's standing "address the minors"; also Task 7 re-review minor: restore_leftovers should pick the newest .replaced-.
Final: Ruling: Declined-to-judge items stay deferred (two instances racing, .replaced- resurrection, lxl 117 MiB capacity, gamepad-only menus, Esc in weapon select leaves the match per accepted ruling, multi-monitor presets, duplicate profile names after truncation, snapshot stability on other adapters) — none affects a v0.1 single-user playtest — cost if wrong: a later small fix.
Final: minor (deferred): autoplay loses clicks in a real window (3/14), tooling only.
Final: fix wave dispatched (opus).
Final: fixed Important 1 HUD covers playfield — hud_fades_over_a_worm RED→GREEN (fade.rs, MAX_MINIMAP 2.5→1.5; HUD snapshots re-blessed with reason)
Final: fixed Minor HUD placeholder loadout — hud_hides_the_weapons_during_the_selection RED→GREEN
Final: fixed Minor names clip/draw through dialogs — names_stay_inside_the_view RED→GREEN
Final: fixed Minor unbounded replay read — replay_files_are_read_bounded RED→GREEN (MAX_REPLAY_BYTES ≈63 MB, regular files only)
Final: fixed Minor newer settings moved aside — a_newer_file_is_never_overwritten, main_menu_shows_the_store_notices, a_newer_settings_file_survives_a_flush RED→GREEN (kept in place, session saves nothing to it, banner)
Final: fixed Minor restore oldest .replaced- — the_newest_replaced_pack_is_restored RED→GREEN
Final: fixed stale docs (112/109, spec HUD style line, 7 game keys, M1 fuzz amendment)
Final: fix wave commits 25244a9..716fe8b; full 138.8 s passed; controller check passed on final tree.
