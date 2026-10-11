# Plan 7: Menus, HUD and Profiles (v0.1), Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the playable Deathmatch into a game you start from a menu: main menu, local-play setup (mod, level with thumbnails, Deathmatch settings, bots and difficulty), player profiles with a colorized skin preview, options (video incl. window size, audio, key bindings, gameplay), OLX's pre-match weapon selection, an in-game HUD and minimap, a pause menu and a results screen. Back these with `--autoplay` end-to-end tests, GUI flow tests and benchmarks. This is milestone M7, **v0.1: playable local Deathmatch against bots**.

**Architecture:**
- **Menus and HUD.** A new L5 crate `rl-ui` holds the menu and HUD *logic* (screen state machine, view models, validation) separate from the egui widgets, so `egui_kittest` can drive flows by label (spec §8.3, §7).
- **App wiring.** `rl-game` hosts the screens: the existing sandbox and Deathmatch sessions become states the menu enters and leaves. Settings and profiles persist in the platform data directory (spec §9.4: `profiles.ron`, `settings.ron`).
- **Assets.** Level thumbnails and OLX's bitmap fonts are imported into packs (A5 deferred them to M7).
- **Bot skill.** The bot-difficulty redesign (strategy, reactions, predictive reloading, teamwork) is not in this plan; v0.1 ships OLX's four difficulties with the aim-error floor.

**Tech stack:** Rust 1.99; egui/egui-wgpu/egui-winit 0.36.2 (already in use); `egui_kittest` 0.36.2 for GUI tests; `directories` 6.0.0; `divan` (pin exactly) for benchmarks. Xvfb is installed for headless end-to-end runs.

**Spec:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`. Read §1.3, §7 (GUI flows, end to end, benchmarks), §8.3, §8.5, §8.6, §9.4, §10 (M7), §11 (credits) and §15–§20. This plan adds §21 (G1–G9) in Task 1.

**Porting reference:**
- `docs/porting/olx-notes/presentation.md` §9 (OLX's HUD, as reference only), §2.4 (minimap), §10 (input options);
- `docs/porting/olx-notes/bots-and-deathmatch.md` §2 (weapon selection, restrictions) and §3.4 (results data);
- OLX source at `target/olx-src/d1ad953f` (`src/client/CClient_Draw.cpp` HUD and minimap, `src/client/DeprecatedGUI/Menu_*.cpp` for menu flows and weapon selection, the `src/common/CWormHuman.cpp` weapon-selection screen).

## Global Constraints

- **Toolchain and license.** Rust 1.99, edition 2024, GPL-3.0-or-later. The crate prefix is `rl-`, and every crate has a `CLAUDE.md` and is registered in `xtask/layers.ron`.
- **Layers.** `rl-ui` is L5 and holds no GPU handles. Its logic is testable without a window.
- **The simulation is untouched.** All oracle scenarios, the D9 pins, match goldens and render goldens stay as they are, unless a change is intended and blessed with a reason.
- **User decisions (binding).**
  - The window is not drag-resizable or maximizable. Window size and fullscreen change only from the options menu (presets) and the startup flags, applied through the existing paced size-change path.
  - Bots: at least 3 difficulties are offered (OLX's Easy, Medium, Hard and Xtreme). No difficulty aims perfectly.
- **Never panic on content or on user files.** Corrupt or old `settings.ron`/`profiles.ron` fall back to defaults with a warning, and the bad file is kept as `.bak`.
- **Determinism.** Menus never touch a running simulation except by starting or ending matches. Replays record exactly what is played.
- **Fast gate.** `cargo xtask check` stays within about 60 s. GUI flow tests (kittest snapshots) go in `check` if fast, otherwise in `full`. End-to-end Xvfb runs and benchmarks are `full` only.
- **UI.** egui at native resolution, scaled by DPI and a UI-scale setting (spec §8.2/§8.3). Default key bindings avoid browser-reserved keys (spec §8.5).

## Decisions (spec amendment §21, written in Task 1)

| # | Decision | Reason |
|---|---|---|
| G1 | **`rl-ui` (L5): screen logic and view models separate from egui widgets.** Screens: Main, LocalPlay, Profiles, Options, WeaponSelect, InGame (HUD + pause), Results, Import, About. | Spec §8.3; testable by `egui_kittest` and plain unit tests. |
| G2 | **Persistence** through `directories` (`ProjectDirs` "rust-liero"): `settings.ron`, `profiles.ron`, `replays/` (auto-saved last matches, capped by count). Every format is versioned; an unknown version means defaults plus a warning. | Spec §9.4. |
| G3 | **Window size** comes from options presets (e.g. 1280×720, 1600×900, 1920×1080, the native desktop size) plus a fullscreen toggle, applied at most every 250 ms through the existing window code, and remembered. | User decision; the WSLg compositor crash. |
| G4 | **OLX's weapon selection before the match** (and when OLX allows re-selection): five slots, restricted by the preset, with a "random" button and keyboard/gamepad navigation. Bots keep their loadouts (F1). | OLX feel; replaces the random human loadout. |
| G5 | **HUD:** health, the five weapon slots with charge and reload, a kill feed, a Tab scoreboard, a time and kill-limit readout, and a minimap (OLX's colours and scale from `presentation.md` §2.4). Two styles: **Modern** (egui, the default) and **Classic** (OLX's bitmap fonts and layout). | Spec §8.3. |
| G6 | **Thumbnails and fonts are imported:** a level thumbnail (`thumb.png`) generated from the level art at import, and OLX's `font.png`/`out_font.png` in the core pack. Pack format bump per policy. | A5 deferred both to M7. |
| G7 | **`--autoplay <script.ron> --fake-clock`** drives the real app through menus and a match with scripted input and a fake clock, taking screenshots at checkpoints. It runs on Xvfb in `xtask full` and asserts a clean structured log. | Spec §7. |
| G8 | **Benchmarks** (`divan`): tick time with 3000 projectiles, an 8-bot tick, DrawList build and an offscreen render frame. Baselines are stored in `fixtures/bench/`; a regression over 10% fails `xtask full` (with a noise guard: medians over several runs). | Spec §7. |
| G9 | **About/credits:** Jason Boettcher (Liero Xtreme), the OpenLieroX team, Joosa Riekkinen (Liero), and the licenses. | Spec §11. |

## Review Focus

1. **A corrupt or future-version settings/profiles file**, or a missing data directory, must start the game with defaults and a warning, never panic or lose the file. Pinned by `bad_user_files_fall_back_to_defaults_and_keep_a_backup` (Task 1).
2. **Key rebinding conflicts:** binding a key already used by another action, unbinding every key for an action, gamepad and keyboard on the same action. The game must stay controllable. Pinned by `rebinding_never_leaves_an_action_unreachable` (Task 4).
3. **Starting a match with missing or broken content**, for example a selected level pack deleted between the menu and the start, must return to the menu with a message, never panic. Pinned by `starting_with_missing_content_returns_to_menu` (Task 2).
4. **Rapid menu actions:** double-pressing Start, Esc during loading, changing window size repeatedly in options. No double matches, stale sessions or compositor crashes (pacing). Pinned by `menu_actions_are_idempotent` (Task 2) and the end-to-end script (Task 8).
5. **HUD at extreme UI scales and window sizes:** no overlap that hides health or weapons, and no panics at UI scale 0.5–3.0. Pinned by `hud_layout_holds_at_all_scales` (Task 5).

## File structure after this plan

```text
crates/rl-ui/          L5  screens/ (main, local_play, profiles, options, weapon_select, ingame, results, import, about), view models, persistence (settings, profiles), theme
crates/rl-game/             app states: menu ↔ match/sandbox/replay; --autoplay/--fake-clock
crates/rl-import-gfx/       + level thumbnails, OLX bitmap fonts (core pack)
xtask/                      + e2e (Xvfb autoplay), bench (divan baselines)
fixtures/autoplay/          end-to-end scripts; fixtures/bench/ baselines; fixtures/ui/ kittest snapshots
```

## After this plan

v0.1 is done. The next steps are separate brainstorms: bot skill (strategy-driven difficulty, teamwork with team modes), the network model (no master server, browser clients), and modernization (sub-project 3).

---

### Task 1: `rl-ui` foundation, persistence, screen state machine

**Files:**
- Create: `crates/rl-ui/` (L5): `lib.rs`, `state.rs` (the screen state machine), `settings.rs`, `profiles.rs`, `store.rs` (persistence), `theme.rs`, `CLAUDE.md`
- Modify: `xtask/layers.ron`, the workspace `Cargo.toml` (`directories`, `egui_kittest` exact pins), the spec (append §21 with G1–G9)

**Interfaces:**
- Produces:
  - `Settings` (video: window preset, fullscreen, vsync/uncapped, filter, interpolation, UI scale; audio: master, sfx and music volumes; input: `Bindings`; gameplay: default match settings, HUD style).
  - `Profile { name, skin_id, colour, bindings? }` and `Profiles { list, selected }`.
  - `Store::open(dirs) -> Store` with `load_settings`, `save_settings`, `load_profiles`, `save_profiles`: versioned RON, atomic writes (temp file + rename), a bad file gives defaults plus a `.bak` copy plus a warning.
  - `Screen` enum and `UiState`. `UiState::update(&mut self, action: UiAction) -> Vec<AppCommand>` is pure logic. `AppCommand` is `StartMatch(MatchOptions)`, `StartSandbox`, `ApplyVideo(VideoSettings)`, `Quit`, and so on.
  - `theme::apply(ctx, ui_scale)`.
- Consumes: `rl-input::Bindings`, `rl-content` match options (`build_match` inputs) and the `rl-game` window code (through `AppCommand`s).

**Steps:**
- [ ] **Step 1: RED.** Write `bad_user_files_fall_back_to_defaults_and_keep_a_backup`, `settings_round_trip_and_version` and `state_machine_navigates_and_emits_commands`. Run `cargo nextest run -p rl-ui`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Append spec §21, run `cargo xtask check`, then commit (`rl-ui: screen state, settings and profiles`).

### Task 2: Main menu and Local Play setup

**Files:**
- Create: `crates/rl-ui/src/screens/{main.rs,local_play.rs}`; Modify: `crates/rl-game` (the app enters and leaves match sessions through `AppCommand`s)

**Interfaces:**
- Produces:
  - **Main menu:** Local Play, Sandbox, Profiles, Options, Replays (a list of the auto-saved and recorded files, played with the existing replay session), Import, About, Quit.
  - **Local Play:**
    - mod picker;
    - level grid with thumbnails (G6) and their size;
    - Deathmatch settings (lives, kill limit, time limit, bonuses on/off, weapon restriction preset, loading time);
    - bots (count 1–31, difficulty per bot or for all, personality);
    - the human's profile;
    - Start.
  - Start builds `MatchOptions` and calls `rl_content::build_match`. On error, the screen shows a message and stays.
  - Esc and Back behave consistently. Start is idempotent while loading (Review Focus 4).
- Consumes: Task 1.

**Steps:**
- [ ] **Step 1: RED.** Write `local_play_builds_match_options`, `starting_with_missing_content_returns_to_menu` (Review Focus 3), `menu_actions_are_idempotent`, and kittest flow `main_to_local_play_to_match` (by label). Run `cargo nextest run -p rl-ui -p rl-game`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-ui: main menu and local play`).

### Task 3: Profiles and the skin preview

**Files:**
- Create: `crates/rl-ui/src/screens/profiles.rs`; Modify: `rl-content` (the skin list and colorization for the preview), `rl-render` or `rl-present` (an egui texture of the colorized skin: a live preview of the walk animation frames)

**Interfaces:**
- Produces:
  - Create, edit, delete and select profiles: name (validated), skin picker (all imported skins, searchable), colour picker.
  - A live colorized preview animating OLX's walk frames, faced left and right.
  - Profiles persist (Task 1). The selected profile's skin and colour are used in matches (the human worm).
- Consumes: Tasks 1 and 2.

**Steps:**
- [ ] **Step 1: RED.** Write `profile_edits_persist`, `skin_preview_uses_colorization_like_the_game` (the same pixels as the in-game skin for one frame) and `profile_names_are_validated`. Run `cargo nextest run -p rl-ui`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-ui: profiles with a live skin preview`).

### Task 4: Options

**Files:**
- Create: `crates/rl-ui/src/screens/options.rs`; Modify: `rl-game` (apply video settings), `rl-audio` (volumes), `rl-input` (rebinding capture)

**Interfaces:**
- Produces:
  - **Video** (G3): window size presets, fullscreen toggle, vsync or uncapped, upscale filter, interpolation on/off, UI scale. Applied live through the paced window path, then saved.
  - **Audio:** master and sfx volumes, applied live.
  - **Key bindings:** per action, a list of keys and gamepad buttons; "press a key" capture; reset to OLX defaults; conflicts shown and resolved explicitly (Review Focus 2).
  - **Gameplay:** HUD style (Modern/Classic), default match settings.
- Consumes: Tasks 1–2.

**Steps:**
- [ ] **Step 1: RED.** Write `rebinding_never_leaves_an_action_unreachable`, `video_changes_go_through_the_paced_path`, `volume_changes_apply_live` and a kittest flow `options_rebind_a_key`. Run `cargo nextest run -p rl-ui -p rl-input -p rl-game`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-ui: options for video, audio, bindings and gameplay`).

### Task 5: In-game HUD, minimap, weapon selection, pause, results

**Files:**
- Create: `crates/rl-ui/src/screens/{ingame.rs,weapon_select.rs,results.rs}`, `crates/rl-ui/src/hud/` (the Modern style), and the minimap; Modify: `rl-game` (replace the interim egui HUD from plans 5–6)

**Interfaces:**
- Produces:
  - **The HUD (G5).** Health, five weapon slots with charge and reload bars, the current weapon, the kill feed, the Tab scoreboard, time and kill limit, and the minimap (OLX colours, worm dots, view rectangle, per `presentation.md` §2.4). It is laid out for 16:9 and 21:9 and holds at UI scales 0.5–3.0 (Review Focus 5).
  - **Weapon selection (G4).** It appears before the human's first spawn, with five slots, the preset's allowed weapons, Random and Done, and keyboard/gamepad navigation. The chosen loadout goes into `MatchOptions` (recorded in replays).
  - **Pause (Esc).** Resume, Options (in-game), and Quit to menu. The local match pauses: no ticks run while paused, and replays record ticks only.
  - **Results.** Winner, per-worm kills, deaths, suicides and damage (OLX's columns), with Rematch, Back to menu and Save replay.
- Consumes: Tasks 1–4.

**Steps:**
- [ ] **Step 1: RED.** Write:
  - `hud_layout_holds_at_all_scales` (layout logic: no overlap of the health/weapons rectangles across scales and aspect ratios);
  - `weapon_selection_respects_restrictions`;
  - `pause_stops_ticks_and_replays_stay_valid`;
  - `results_match_world_scores`;
  - kittest snapshots of the HUD at 1080p and 1440p.

  Run `cargo nextest run -p rl-ui -p rl-game`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-ui: HUD, minimap, weapon selection, pause and results`).

### Task 6: Classic style, thumbnails and fonts import, About

**Files:**
- Modify: `crates/rl-import-gfx` (level thumbnails at import; OLX `font.png`, `out_font.png` and the HUD images listed in `presentation.md` "Assets" into the core pack), `rl-pack` (format bump), `content/olx-manifest.ron`, corpus snapshot
- Create: `crates/rl-ui/src/hud/classic.rs` (OLX bitmap font rendering into egui textures), `crates/rl-ui/src/screens/about.rs`

**Interfaces:**
- Produces:
  - `thumb.png` per level pack (a fixed size such as 160×96, letterboxed).
  - The OLX bitmap font as a glyph atlas, with an egui text painter for the Classic HUD.
  - The About/credits screen (G9).
- Consumes: Tasks 1–5.

**Steps:**
- [ ] **Step 1: RED.** Write `thumbnails_are_imported_for_every_level`, `bitmap_font_glyphs_match_olx_metrics` and `classic_hud_draws_the_same_information`. Run `cargo nextest run -p rl-import-gfx -p rl-ui`: FAIL.
- [ ] **Step 2: GREEN.** Run `cargo xtask check` (re-imports content; accept intended snapshot changes).
- [ ] **Step 3: Commit.** Commit (`Classic HUD with OLX fonts, level thumbnails, credits`).

### Task 7: Content import screen

**Files:**
- Create: `crates/rl-ui/src/screens/import.rs`; Modify: `rl-game` (run an import in a background thread with progress, then refresh `Content`)

**Interfaces:**
- Produces:
  - "Import OLX content" from a user-chosen OLX game directory (a typed path; a file dialog is optional and must not add heavy dependencies). It uses the existing importers (`rl-cli`'s import logic, moved to a shared crate if needed: L4/L5) and writes into the user data `packs/` directory, layered over the bundled packs (spec §9.4).
  - It shows progress and the import report (warnings) and never blocks the UI thread. Failures are shown, not panics.
- Consumes: Tasks 1–2.

**Steps:**
- [ ] **Step 1: RED.** Write `import_runs_off_the_ui_thread_and_reports` (with a temp OLX tree from the test fixtures) and `imported_packs_override_bundled`. Run `cargo nextest run -p rl-ui -p rl-content`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Run `cargo xtask check`, then commit (`rl-ui: import OLX content from the menu`).

### Task 8: `--autoplay`, end-to-end tests, benchmarks

**Files:**
- Modify: `crates/rl-game` (`--autoplay <script.ron> --fake-clock`, a structured log), `xtask` (`e2e` step on Xvfb in `full`; `bench` with baselines in `fixtures/bench/`)
- Create: `fixtures/autoplay/*.ron`, `crates/rl-sim/benches/`, `crates/rl-render/benches/` (divan)

**Interfaces:**
- Produces:
  - **Autoplay scripts:** sequences of `{ at_frame, ui: click(label) | key(...) | wait_for(screen) | screenshot(name) | assert(...) }`. The scripted path covers menu → Local Play → weapon select → match (scripted keys) → results → menu → quit, plus options window size changes (pacing). It runs on Xvfb with `--fake-clock`, so it is deterministic, and it asserts a clean log (no warnings or errors) and screenshot sanity.
  - **Benchmarks (G8)** with stored baselines; `xtask full` compares medians and fails over 10%. `cargo xtask bench --bless --reason` updates the baselines and logs to `blessings.log`.
  - **Engine stress test** (user request: future 32-player matches with settable loading time).
    - **Setup:** a headless, seeded match on the largest level (Castle Strike), once for each mod (MW 1.0 is the heavier):
      - `MAX_WORMS` (32) worms, all bots with the chaos personality, firing every weapon continuously;
      - the minimum `LoadingTime`, so reloads are near-instant;
      - bonuses on;
      - several minutes of ticks.
    - **Pool variant:** run once at OLX's projectile pool (3000) and once with a raised `pool_capacity` (e.g. 20 000) to push past OLX's limits.
    - **Measurements:**
      - sim tick time (p50/p99/max), and bot think time and units;
      - projectile pool occupancy (peak and time at cap), cosmetic entities (fx cap 1024), events per tick, dropped events;
      - an offscreen render frame time at 1440p every N ticks (rl-present + rl-render);
      - peak memory.
    - **`cargo xtask stress`** runs it and prints a report for optimization work.
    - **In `xtask full`,** a shorter stress run compares against a stored baseline (`fixtures/bench/stress.ron`, noise-guarded medians, fail over 10%), blessed with `cargo xtask bench --bless --reason`.
    - **Invariants** (no panic, bounded memory, determinism over two runs) are asserted every tick.
- Consumes: Tasks 1–7.

**Steps:**
- [ ] **Step 1: RED.** Write the e2e script, the bench harness and the stress test, and watch them fail before the implementation. Run `cargo xtask full`: FAIL.
- [ ] **Step 2: GREEN.**
- [ ] **Step 3: Commit.** Commit (`End-to-end autoplay on Xvfb, benchmarks`).

### Task 9: Acceptance, v0.1

**Steps:**
- [ ] **Step 1: Gates.** `cargo xtask check` and `cargo xtask full` (e2e, bench, bot grid, GPU, audio) pass.
- [ ] **Step 2: Scripted runs** (plan 5's R3 approach). Starting from the menu, each of the 4 levels × 2 mods for 60 s with 4 Medium bots, using the menu flow. Inspect screenshots and report FPS, tick and think times.
- [ ] **Step 3: Docs.**
  - README "Playing v0.1": install, import or fetch content, run, menus, controls.
  - STATUS rows for the UI and decisions.
  - Spec §1.3 success criteria, checked off with evidence.
  - A `CHANGELOG.md` entry for v0.1.
- [ ] **Step 4: Commit and tag** (`v0.1: playable local Deathmatch against bots`). Create the git tag `v0.1.0` locally; never push.
