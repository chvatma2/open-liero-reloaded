# Plan 14c: The Level Editor in the Game Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Players open the level editor from the game:
- **Editor** in the main menu, and `olr --edit <dir>`;
- the canvas drawn by the real renderer, each edit uploading only its dirty rectangle;
- Play in a sandbox or bot match, Stop or Esc returning to the editor exactly as it was;
- a file browser for Open and Import image;
- a recovery copy every 2 minutes;
- an end-to-end autoplay script;
- then a playtest release.

**Architecture:**
- **Hosting.** `olr-game` hosts `olr-editor` (14b): `olr-game` is layer 6 and may depend on it. `olr-ui` does not depend on `olr-editor`:
  - `olr-ui` gains only the **Editor** list screen (level packs, New level, New from a copy, recoveries) and the `AppCommand`s that ask the game to open the editor;
  - a new `olr-game` module, `editor/`, owns an `EditorHost`: the `EditorState`, its `EditorData`, the command execution (files, browser, recovery, play) and the canvas;
  - while the host is open, `App::frame` draws `olr_editor::ui::show` instead of `olr_ui::screens::show`.
- **The canvas** skips `olr-present`, which needs a `World`. It keeps an `olr_terrain::ColorMap` of the bake (or of the material palette) and draws it with `olr_render::Renderer::render_in` into the canvas's rectangle. Uploads go through `ColorMap::replace_rect` and `Renderer::update_terrain`. egui draws the panels and overlays on top in the same frame.
- **Play** builds a sandbox (0 bots) or a Deathmatch (1-8 bots) from the edited `LevelSource` itself, no pack needed. The human worm's `WormSetup::spawn` is the level's first spawn when it is inside the level. The host keeps the editor alive during play, and Stop or Esc returns to it.

**Tech Stack:** Rust 1.99, egui `=0.36.2`, egui_kittest `=0.36.2`, wgpu (through `olr-render`), winit.

**Spec:** `docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md` §4 (files and safety, canvas), §5 (Play), §6, §8 (end to end), §9 (14c). Plan 14b (`docs/superpowers/plans/2026-10-17-plan-14b-level-editor.md`), the "Level editor decisions" in `docs/porting/STATUS.md` and `crates/olr-editor/CLAUDE.md` hold the editor this plan hosts.

## Global Constraints

- Never panic on content (repo rule 6). Packs, images, file names, directory listings and recovery files are untrusted.
- Keep output deterministic (repo rule 7). The recovery timer is the host's: no wall clock enters `olr-editor`, `olr-ui` state or the simulation.
- **Layers.**
  - `olr-game` (layer 6) gains `olr-editor`.
  - `olr-ui` (layer 5) gains no editor dependency: no `same_layer` pair is added.
  - `olr-terrain` (layer 1) stays `no_std` and wasm-clean.
- **These stay byte-identical:**
  - the oracle traces;
  - the match goldens: `WormSetup::spawn` already exists, and nothing outside editor play sets it;
  - the render goldens and `fixtures/ui/`, unless a task below changes a menu (then `cargo xtask bless-render --reason`);
  - `content/packs`;
  - `fixtures/edit/composed-level`.
- **Bundled content is never written.** `content/packs` (the `--content` root) is opened only to copy from. Editing happens in the data directory's `packs/` (`Store::packs_dir`).
- **Limits:**
  - recovery every `RECOVERY_INTERVAL` = 120 s while unsaved;
  - an imported file at most `MAX_IMPORT_BYTES` = 64 MiB, checked from its metadata before reading;
  - Play bots 0..=8 (`olr_editor::MAX_BOTS`);
  - the canvas's world view at most `olr_render`'s `MAX_VIEW` (4096) per side.
- Verify once per batch: targeted `cargo nextest run -p <crate>` while working, then the online `cargo xtask check` at each batch's end.
  - Batch A is Tasks 1-3.
  - Batch B is Tasks 4-5.
  - Batch C is Tasks 6-7.
  - Batch D is Task 8 (docs and the release).
- **`cargo xtask full`** runs once at plan end, and only after asking the user: it shares their PC. GPU tests run with `OLR_GPU_TESTS=1` locally, targeted.
- Commits are small, with an imperative subject (`olr-game: …`), and end with the session's trailer lines.

## Review Focus

1. **Quitting with unsaved changes.** Closing the window (or the main menu's Quit) while the editor holds unsaved changes asks first (Save, Don't save, Cancel), as Close does. It never quits silently. Test: Task 6 (`closing_the_window_asks_when_unsaved`).
2. **Bundled levels are never written.** Edit on a bundled level only offers a copy. Saving after editing the copy leaves `content/packs` byte-identical. Test: Task 5 (`a_bundled_level_is_edited_as_a_copy`).
3. **Recovery whose original moved or vanished.** The copy is still offered. Recovering it opens with the original directory when it exists, else a new `packs/<id>` (Save then creates it). A malformed recovery is listed as damaged and can be discarded, never a panic. Tests: Task 5 (`recovery_survives_a_missing_original`, `a_damaged_recovery_is_discarded_not_fatal`).
4. **Importing a huge or non-image file** (a 2 GB file, a text file named `.png`) is refused with a message before it is read whole. Test: Task 5 (`imports_are_bounded_before_reading`).
5. **Play on a level without spawns, or whose first spawn is inside rock or outside the level.** The worm takes a free spot, as in any match. Test: Task 2 (`the_first_spawn_is_used_only_when_inside_the_level`).

## Rulings made while writing this plan

- **R1 The canvas is `olr-render`'s, not `olr-present`'s.** `Presentation` needs a `World`. The renderer alone draws a terrain-only `DrawList` (as `olr-render/tests/gpu.rs` does). The spec's "olr-present's terrain path" is honoured by the same `Renderer::update_terrain` dirty uploads.
- **R2 The material view** is a second `ColorMap` built from `olr_format::levels::MATERIAL_PALETTE` at art scale 1. Toggling it re-uploads the whole map, and an edit updates the active one.
- **R3 Play.**
  - 0 bots is the sandbox: idle targets, as the main menu's Sandbox.
  - 1-8 bots is a Deathmatch: the human and that many bots, of the Local Play screen's saved bot level and personality.
  - Neither records a replay.
  - Esc ends the play at once (no pause menu), and so does a **Stop** button over the playfield.
- **R4 The first spawn.** The human worm's `WormSetup::spawn` is the level's first spawn when it lies inside the level, else `None` (a free spot). Whatever `spawn` does on later respawns is kept as the simulation does it.
- **R5 Where levels are edited.** The Editor screen lists every level pack:
  - **Edit** for packs in the data directory's `packs/`;
  - **Edit a copy** for the others, which makes `packs/<new id>` from it (`olr_edit::new::new_pack`, `Template::From`) and opens that.
  - New ids are slugs of the name, unique against the catalog and `packs/`.
- **R6 `--edit <dir>`** opens that directory, unless it lies inside the `--content` root. Then it prints "bundled content is not edited in place: use the Editor menu's Edit a copy" and exits with status 2.
- **R7 Recovery** lives in the data directory's `recovery/<id>/`: the pack's files (`olr_edit::pack::files`) plus `recovery.ron` (`RecoveryInfo { origin: PathBuf, name: String }`). It is written atomically (a temporary sibling, then a rename) every `RECOVERY_INTERVAL` while unsaved. It is removed on a successful save and on Close.
- **R8 Editor settings** (`EditorSettings`: Play's mod and bots, the browser's last directory) are their own file, `editor.ron`, written through new public `Store::load_file` and `save_file`. `Settings` stays as it is.
- **R9 The file browser lives in `olr-editor`**, as state and panels. The host lists directories and reads files: `EditorCommand::List`, `EditorCommand::ReadImage`. A path field accepts any typed path (a Windows drive, a WSL path).

---

### Task 1: Rectangle replacement in a colour map (`olr-terrain`)

**Files:**
- Modify: `crates/olr-terrain/src/color.rs`.
- Test: the unit tests in `color.rs`.

**Interfaces:**
- Produces:

  ```rust
  impl ColorMap {
      /// Copies art rectangle `r` of `front` and `back` (images of this map's art size) into
      /// the map's own front, back and composited pixels (the composite takes `front`), and the
      /// world cells under `r` from `material` (the level's size), then marks `r` dirty. Images or
      /// a material of another size change nothing. `r` is clipped to the map.
      pub fn replace_rect(&mut self, r: Rect, front: &RgbaImage, back: &RgbaImage, material: &Terrain);
  }
  ```

- [ ] **Step 1: Write the failing tests.**
  - `replace_rect_copies_and_marks_dirty`: build a 4 × 3 map at art scale 2.
    - Replace `Rect { x: 2, y: 2, w: 4, h: 2 }` from red front and blue back images.
    - The map's pixels equal red there and stay as before elsewhere.
    - `take_dirty()` yields a rectangle holding `r`.
    - `color_at` reports red inside.
  - `replace_rect_of_another_size_changes_nothing`: images of the wrong size, or a material of the wrong size, leave `pixels()` equal and no dirty rectangle.
  - `replace_rect_clips`: `Rect { x: 6, y: 4, w: 100, h: 100 }` changes only the in-map part and never panics.
- [ ] **Step 2:** Run `cargo nextest run -p olr-terrain color`. Expected: FAIL (no method).
- [ ] **Step 3:** Implement `replace_rect` with the existing private `mark`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-terrain` and `cargo build -p olr-terrain --target wasm32-unknown-unknown`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-terrain: replace a rectangle of a colour map`.

### Task 2: Playing an edited level (`olr-content`, `olr-game`'s `setup`)

**Files:**
- Modify:
  - `crates/olr-content/src/maps.rs`;
  - `crates/olr-game/src/setup.rs`;
  - `crates/olr-game/src/shell.rs` (a launch variant, no replay for it).
- Test: `crates/olr-content/tests/` (the crate's integration binary); the unit tests in `setup.rs`.

**Interfaces:**
- Produces, in `olr_content`:

  ```rust
  impl LoadedLevel {
      /// An edited level (`compile_level`, the art bound `load_level` applies) as a match loads it.
      pub fn from_source(src: &LevelSource) -> anyhow::Result<LoadedLevel>;
      /// The first spawn point when it lies inside the level (`0..size`), else `None`.
      pub fn first_spawn(&self) -> Option<(i32, i32)>;
  }
  ```

- Produces, in `olr-game` (`setup.rs`):

  ```rust
  /// What the editor's Play asks for (R3).
  pub struct EditedPlay { pub level: LevelSource, pub mod_id: String, pub bots: u8 }
  /// The sandbox (0 bots) or Deathmatch (1-8) of `play` on the edited level, the human
  /// worm's `WormSetup::spawn` = `first_spawn()` (R4); content by id from `roots`
  /// (mod, theme, core, skin `skin`), the bots' level and personality as `prefs` (the
  /// Local Play screen's saved `MatchPrefs`).
  pub fn load_edited(roots: &[&Path], play: &EditedPlay, skin: &str, prefs: &olr_ui::MatchPrefs) -> anyhow::Result<Game>;
  ```

  `shell.rs`'s private `Launch` gains `Edited(Box<EditedPlay>)`. Its session saves no replay, and its end returns to the editor (Task 6).

- [ ] **Step 1: Write the failing tests.**
  - In `olr-content`, `from_source_equals_load_level`: for `olx-level-dirt-level`, `LoadedLevel::from_source(&read_level_files(dir))` equals `content.load_level(id)` field by field.
  - `the_first_spawn_is_used_only_when_inside_the_level`:
    - spawns `[(-1, 5), (10, 10)]` give `None`: only the first one counts;
    - `[(10, 10)]` gives `Some((10, 10))`;
    - none gives `None`;
    - `(size.0, 0)` gives `None`.
  - In `setup.rs`, `edited_play_builds_a_sandbox_or_a_match`, with `content/packs` (skip without it):
    - `bots: 0` gives `Game::Sandbox` whose human `WormSetup::spawn` is the level's first spawn;
    - `bots: 3` gives `Game::Deathmatch` with 4 players, the human's spawn the same;
    - a level without spawns gives `spawn: None`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-content first_spawn from_source` and `cargo nextest run -p olr-game edited_play`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - `from_source` is `load_level`'s body on a compiled source.
  - `load_edited` reuses `load` and `load_match`'s pieces with the level given, not loaded by id.
  - The spawn is set on the human worm's `WormSetup` before the `World` is made. If `build_match` makes it inside, add an `olr_content` variant taking the first spawn, and leave `build_match` as it is.
- [ ] **Step 4:** Run `cargo nextest run -p olr-content -p olr-game` and `cargo nextest run -p olr-cli --test golden`. Expected: PASS, the goldens unchanged.
- [ ] **Step 5:** Commit: `olr-content, olr-game: play an edited level, its first spawn for the human worm`.

### Task 3: The Editor screen and its commands (`olr-ui`)

**Files:**
- Create: `crates/olr-ui/src/screens/editor.rs`; `crates/olr-ui/tests/editor.rs` (a `mod` of `tests/integration.rs`).
- Modify:
  - `crates/olr-ui/src/state.rs`: `Screen::Editor`, `MENUS` gains it;
  - `crates/olr-ui/src/screens/{mod.rs, main.rs}`;
  - `crates/olr-ui/src/store.rs`;
  - `crates/olr-game/src/autoplay.rs`: `SCREENS` gains `("Editor", Screen::Editor)`.

**Interfaces:**
- Produces:

  ```rust
  // state.rs
  pub enum Screen { …, Editor }
  pub enum AppCommand {
      …,
      /// Open the editor on level pack `id` (one of `MenuData::editable`).
      EditLevel(String),
      /// Copy level pack `from` into the data directory's packs as `name` (R5), then edit it.
      CopyLevel { from: String, name: String },
      /// Make a new level pack (`olr_edit::new::Template::Level`), then edit it.
      NewLevel { name: String, size: (u32, u32), theme: String },
      /// Reopen recovery `id` (its unsaved changes), or remove it.
      Recover(String),
      DiscardRecovery(String),
  }
  // screens/mod.rs: MenuData
  impl MenuData {
      /// The level packs in the data directory's packs (editable in place) and the recoveries
      /// (`(id, level name, damaged)`).
      pub fn set_editor_packs(&mut self, editable: BTreeSet<String>, recoveries: Vec<(String, String, bool)>);
  }
  // store.rs
  impl Store {
      /// A RON file of the data directory (`name`, such as `editor.ron`): `None` when absent or
      /// unreadable (with a warning).
      pub fn load_file<T: serde::de::DeserializeOwned>(&self, name: &str) -> Option<T>;
      /// Written as `save_settings` writes (atomic, versioned like the others).
      pub fn save_file<T: serde::Serialize>(&mut self, name: &str, value: &T) -> anyhow::Result<()>;
      /// The data directory's `recovery/` (R7).
      pub fn recovery_dir(&self) -> Option<PathBuf>;
  }
  ```

- **Layout** (centred, as the other menus):
  - a heading "Editor";
  - the recoveries first: "Recover <name>" and "Discard" per entry, and a damaged one shows only "Discard";
  - a list of the catalog's levels, each with "Edit" (editable) or "Edit a copy" (others; it opens a small form with "Name", default "<Name> copy", and "Copy");
  - "New level", which opens a form: "Name", "Width" (default 504) and "Height" (default 350), each 1..=4096, "Theme" (a combo of the catalog's themes), and "Create";
  - Back.
- The main menu gains **Editor** between "Sandbox" and "Profiles".

- [ ] **Step 1: Write the failing tests** (`tests/editor.rs`, a harness as `tests/flows.rs`'s):
  - `main_menu_opens_the_editor_screen`: "Editor" on Main leads to `Screen::Editor`; Back returns to Main.
  - `bundled_levels_offer_a_copy_editable_ones_edit`: with `editable = {"my-level"}`:
    - the row of `olx-level-dirt-level` has "Edit a copy", and copying with the default name emits `CopyLevel { from: "olx-level-dirt-level", name: "Dirt Level copy" }`;
    - the row of `my-level` emits `EditLevel("my-level")`.
  - `new_level_form_emits_new_level`: typing "Office", 600 and 400, then Create, emits `NewLevel { name: "Office", size: (600, 400), theme: "olx-theme-dirt" }`. An empty name keeps Create disabled.
  - `recoveries_are_offered_first`: a recovery `("my-level", "My Level", false)` shows "Recover My Level", which emits `Recover("my-level")`. A damaged one shows only Discard.
  - In `store.rs`'s tests, `files_round_trip`: `save_file("editor.ron", &x)` then `load_file` gives `x`; a garbage file gives `None`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-ui editor files_round_trip`. Expected: FAIL.
- [ ] **Step 3:** Implement. `layout.rs`'s centring test covers the new screen once it is listed.
- [ ] **Step 4:** Run `cargo nextest run -p olr-ui`. If the main menu's snapshot changed, run `cargo xtask bless-render --reason "plan 14c: the main menu's Editor entry"`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-ui: the Editor screen and its commands`.
- [ ] **Step 6 (batch A):** Run the online `cargo xtask check`. Expected: every step `ok`.

### Task 4: The file browser, recovery and the transparent canvas (`olr-editor`)

**Files:**
- Create: `crates/olr-editor/src/browser.rs`; `crates/olr-editor/src/ui/browser.rs`; `crates/olr-editor/tests/browser.rs`.
- Modify:
  - `state.rs` (the actions and commands below);
  - `catalog.rs` (`EditorSettings::browse_dir`);
  - `ui/mod.rs` (a transparent central panel, `labels`);
  - `ui/toolbar.rs` (Open…, Close);
  - `ui/elements.rs` (Import image… enabled);
  - `ui/canvas.rs` (the canvas's AccessKit label "Canvas").

**Interfaces:**
- Produces:

  ```rust
  // browser.rs
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Purpose { ImportImage, OpenPack }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct DirEntry { pub name: String, pub is_dir: bool, pub is_pack: bool }
  /// The browser's state: its purpose, directory, sorted entries (folders first, then by name,
  /// case aside), the selection and the typed path.
  pub struct Browser { /* private */ }
  impl Browser {
      pub fn purpose(&self) -> Purpose;
      pub fn dir(&self) -> &Path;
      /// The entries shown: folders, and for ImportImage the `.png`, `.jpg`, `.jpeg` files
      /// (case aside); hidden (`.`) entries left out.
      pub fn shown(&self) -> Vec<&DirEntry>;
  }
  // catalog.rs
  pub struct EditorSettings { pub play_mod: String, pub play_bots: u8, pub browse_dir: Option<PathBuf> }
  // state.rs, new actions
  EditorAction::Browse(Purpose)                 // open the browser at browse_dir, else the home directory
  EditorAction::Listed { dir: PathBuf, entries: Result<Vec<DirEntry>, String> }
  EditorAction::BrowseTo(PathBuf)               // a folder clicked, "..", or the typed path
  EditorAction::Pick(String)                    // an entry chosen (a file, or a pack folder for OpenPack)
  EditorAction::CloseBrowser
  EditorAction::ImportFailed(String)
  EditorAction::Recovered(Box<Document>)        // as Opened, but unsaved (dirty)
  // state.rs, new commands
  EditorCommand::List(PathBuf)                  // answer Listed
  EditorCommand::ReadImage(PathBuf)             // answer ImportImage { file_name, bytes } or ImportFailed
  EditorCommand::RecoveryDone                   // the document is saved or closed: drop its recovery copy
  // state.rs, accessors
  pub fn browser(&self) -> Option<&Browser>;
  // ui::labels
  pub const OPEN: &str = "Open…";   pub const IMPORT_IMAGE: &str = "Import image…";
  pub const CLOSE: &str = "Close";  pub const CANVAS: &str = "Canvas";
  pub const BROWSER_PATH: &str = "Path";  pub const CHOOSE: &str = "Choose";
  ```

  Rules:
  - `Pick` of a file (ImportImage) emits `ReadImage(dir/name)` and remembers `browse_dir = dir` (`SaveSettings`).
  - `Pick` of a pack folder (OpenPack) closes the browser and acts as `Open(dir/name)`: the unsaved-changes dialog applies.
  - `Listed(Err)` keeps the old listing and sets the browser's message.
  - `Saved(Ok)` and Close (after any dialog) also emit `RecoveryDone`.
  - The central panel has no fill (`egui::Frame::NONE`), so the host's world render shows through. The canvas reports itself with `WidgetInfo::labeled(WidgetType::Other, true, labels::CANVAS)`.

- [ ] **Step 1: Write the failing tests** (`tests/browser.rs`, state-level and `egui_kittest` flows with a test host listing a `tempfile` tree):
  - `browsing_lists_filters_and_sorts`: a tree with `b/`, `A/`, `x.PNG`, `y.txt` and `.hidden`.
    - ImportImage shows `A/`, `b/`, `x.PNG` in that order.
    - OpenPack shows only folders, and a folder holding `pack.ron` is `is_pack`.
  - `picking_an_image_reads_it_and_remembers_the_folder`: Pick `x.PNG` emits `ReadImage(dir/x.PNG)` and `SaveSettings` with `browse_dir == Some(dir)`.
  - `a_listing_error_keeps_the_old_listing`: `Listed { entries: Err("denied") }` keeps the entries and shows "denied".
  - `opening_a_pack_through_the_browser_asks_when_unsaved`: an edit, then Pick of a pack folder, gives `Dialog::Unsaved { then: Pending::Open(..) }`.
  - `recovered_documents_are_unsaved`: `Recovered(doc)` gives `dirty() == true` and `Upload(doc.all())`.
  - `saving_or_closing_drops_the_recovery`: `Saved(Ok)` emits `RecoveryDone`; Close then Discard emits `RecoveryDone` and `Close`.
  - `the_browser_by_label`: click `labels::IMPORT_IMAGE`; the browser shows; type a path into `labels::BROWSER_PATH` and press Enter, which gives `BrowseTo`; click an entry, then `labels::CHOOSE`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-editor browser`. Expected: FAIL.
- [ ] **Step 3:** Implement `browser.rs`, the actions, and the panels (a modal window, as `dialogs.rs`).
- [ ] **Step 4:** Run `cargo nextest run -p olr-editor` and `cargo clippy -p olr-editor --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-editor: a file browser, recovered documents and a transparent canvas`.

### Task 5: The editor host (`olr-game`, no window)

**Files:**
- Create:
  - `crates/olr-game/src/editor/mod.rs`: `EditorHost`;
  - `crates/olr-game/src/editor/files.rs`: packs, ids, listing, reading;
  - `crates/olr-game/src/editor/recovery.rs`;
  - `crates/olr-game/tests/editor_host.rs`, added to the crate's integration binary.
- Modify:
  - `crates/olr-game/Cargo.toml`: `olr-editor`, `olr-edit`;
  - `crates/olr-edit/src/new.rs`: `pack_id`.

**Interfaces:**
- Consumes: Task 3's `AppCommand`s, `Store::{load_file, save_file, recovery_dir, packs_dir}`; Task 4's commands.
- Produces:

  ```rust
  // olr_edit::new
  /// A pack id from `name`: a slug as `element::new_element_id` makes ids, its leading and
  /// trailing `-` trimmed, with `-2`, `-3`, … appended until `taken` is false; "level" when the
  /// name gives nothing.
  pub fn pack_id(name: &str, taken: impl Fn(&str) -> bool) -> String;
  // olr_game::editor
  pub const RECOVERY_INTERVAL: Duration = Duration::from_secs(120);
  pub const MAX_IMPORT_BYTES: u64 = 64 << 20;
  pub const EDITOR_FILE: &str = "editor.ron";
  /// `recovery/<id>/recovery.ron`.
  #[derive(Serialize, Deserialize)] pub struct RecoveryInfo { pub origin: PathBuf, pub name: String }
  pub struct EditorHost { /* state, data, packs_dir, recovery_dir, content roots, last_recovery: Instant, play: Option<…> */ }
  /// What the host asks of the app around it.
  pub enum HostOut { Upload(Rect), FullUpload, Play(Box<EditedPlay>), Closed, Message(String) }
  impl EditorHost {
      /// The editor's catalog (themes with their tiles, mods) from `roots`; the settings from `store`.
      pub fn new(store: &Store, roots: &[&Path]) -> anyhow::Result<EditorHost>;
      /// Carries out `AppCommand::{EditLevel, CopyLevel, NewLevel, Recover}` (`DiscardRecovery`
      /// is `discard_recovery`): resolve or make the pack in `packs_dir`, open it.
      pub fn open_from_menu(&mut self, c: &AppCommand) -> Vec<HostOut>;
      /// `olr --edit <dir>` (R6 checked by the caller).
      pub fn open_dir(&mut self, dir: &Path) -> Vec<HostOut>;
      /// One frame's actions from `olr_editor::ui::show`, and the commands they lead to.
      pub fn act(&mut self, actions: Vec<EditorAction>) -> Vec<HostOut>;
      /// The recovery timer: writes the copy when `now - last >= RECOVERY_INTERVAL` and unsaved.
      pub fn tick(&mut self, now: Instant) -> Vec<HostOut>;
      pub fn state(&self) -> &EditorState;
      pub fn data(&mut self) -> &mut EditorData;
  }
  /// The recoveries in `dir`: (id, level name, damaged), sorted by id.
  pub fn list_recoveries(dir: &Path) -> Vec<(String, String, bool)>;
  pub fn discard_recovery(dir: &Path, id: &str) -> anyhow::Result<()>;
  /// The level pack ids in `packs_dir` (by manifest), for `MenuData::set_editor_packs`.
  pub fn editable_levels(packs_dir: &Path) -> BTreeSet<String>;
  ```

  Command execution:
  - `Open`, `Create` and `Save` are as in 14b's test host. A save's warnings come back in `Saved`.
  - `List` reads one directory with `std::fs::read_dir`. A pack is a folder holding `pack.ron`.
  - `ReadImage` checks the metadata length against `MAX_IMPORT_BYTES` first, then the header with `image_io::image_size`.
  - `SaveSettings` writes `EDITOR_FILE`.
  - `Upload` becomes `HostOut::Upload`. `Opened` and `Recovered` give `HostOut::FullUpload`.
  - `Play` becomes `HostOut::Play`, and `Close` becomes `HostOut::Closed`.
  - `RecoveryDone` removes `recovery/<id>`.

- [ ] **Step 1: Write the failing tests** (`tests/editor_host.rs`, a `tempfile` data directory with the bundled `content/packs` as the content root; skip without it):
  - `a_new_level_is_made_in_packs_and_opened`: `NewLevel { name: "Office", .. }` makes `packs/office/` and opens it; a second one gives `office-2`.
  - `a_bundled_level_is_edited_as_a_copy`:
    - `CopyLevel { from: "olx-level-dirt-level", name: "Dirt copy" }` opens `packs/dirt-copy`;
    - an `AddLayer` and a `Save` write there;
    - `content/packs` is byte-identical afterwards (compare a digest of its tree before and after).
  - `recovery_is_written_while_unsaved_and_dropped_on_save`:
    - after an edit, `tick(start + 119 s)` writes nothing and `tick(start + 120 s)` writes `recovery/<id>/` with `recovery.ron` naming the origin;
    - a clean document never writes;
    - `Save` removes the copy.
  - `recovery_survives_a_missing_original`: write a recovery, delete the original pack, then `Recover(id)` opens it (dirty) with dir `packs/<id>`, and Save recreates the pack.
  - `a_damaged_recovery_is_discarded_not_fatal`: a `recovery/x/` with garbage `recovery.ron` is listed damaged, `Recover("x")` gives `HostOut::Message`, and `discard_recovery` removes it.
  - `imports_are_bounded_before_reading`:
    - a sparse 2 GiB file (`File::set_len`) named `big.png` gives `ImportFailed` mentioning "64 MiB" without reading it;
    - a text file named `.png` gives `ImportFailed` ("not a PNG or JPEG").
  - `settings_persist`: `PlayBots(3)` then a new `EditorHost` from the same store has `play_bots == 3`.
  - In `olr_edit::new`'s tests: `pack_id("My Office!", |_| false) == "my-office"`, with collisions `-2`; `pack_id("", ..) == "level"`.
- [ ] **Step 2:** Run `cargo nextest run -p olr-game editor_host` and `cargo nextest run -p olr-edit pack_id`. Expected: FAIL.
- [ ] **Step 3:** Implement the host. The pack directory name is the id. `editable_levels` reads each child's manifest (`olr_pack::read_manifest`) and skips the unreadable ones.
- [ ] **Step 4:** Run `cargo nextest run -p olr-game -p olr-edit` and `cargo xtask layers`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-game: the editor host: packs, browser, recovery and settings`.
- [ ] **Step 6 (batch B):** Run the online `cargo xtask check`. Expected: every step `ok`.

### Task 6: The editor in the window (`olr-game`)

**Files:**
- Create: `crates/olr-game/src/editor/canvas.rs`.
- Modify:
  - `crates/olr-game/src/{app.rs, app/menus.rs, gpu.rs, shell.rs, main.rs, overlay.rs}`;
  - `crates/olr-game/CLAUDE.md`.
- Test: the unit tests in `canvas.rs`; `crates/olr-game/tests/` (a GPU test gated by `OLR_GPU_TESTS`); `main.rs`'s argument tests.

**Interfaces:**
- Consumes: Tasks 1-5.
- Produces:

  ```rust
  // editor/canvas.rs
  /// The canvas's colour maps (the bake, and the material view's, R2) and its draw list.
  pub struct Canvas { /* bake: Option<ColorMap>, material: Option<ColorMap>, showing_material: bool */ }
  impl Canvas {
      pub fn new() -> Canvas;
      /// Rebuilds the active map from `doc` (a level resize or a new document changes its size).
      pub fn rebuild(&mut self, doc: &Document, theme: &olr_content::LoadedTheme, material_view: bool);
      /// `doc`'s world rectangle `r` into the active map (`replace_rect`, art pixels = r × art scale).
      pub fn upload(&mut self, doc: &Document, r: Rect);
      /// The terrain-only draw list and the physical-pixel area for `view` over `canvas`
      /// (egui points) at `pixels_per_point`: the visible world rectangle clipped to the level
      /// (at most `MAX_VIEW` per side), and the screen sub-rectangle it fills.
      pub fn draw(&self, view: &olr_editor::view::View, canvas: egui::Rect, pixels_per_point: f32)
          -> Option<(DrawList, [u32; 4])>;
      pub fn map_mut(&mut self) -> Option<&mut ColorMap>; // for Renderer::update_terrain
  }
  // gpu.rs: the world's area is the caller's
  pub fn present(&mut self, frame: Option<Acquired>, world: Option<(&DrawList, [u32; 4])>,
      overlay: &OverlayPaint, textures: &mut egui::TexturesDelta) -> Option<PresentTiming>;
  // main.rs
  #[arg(long, value_name = "DIR")] edit: Option<PathBuf>,  // R6; not a GAME_FLAG: the menus run
  ```

  Behaviour:
  - **Drawing.** While `shell.editor` is `Some` and not playing, `App::frame` draws `olr_editor::ui::show` (instead of `screens::show`), feeds its actions to `EditorHost::act`, and carries out the `HostOut`s:
    - `Upload` becomes `canvas.upload`, then `Renderer::update_terrain` on the next draw;
    - `FullUpload` becomes `rebuild` plus `upload_terrain`;
    - `Play` launches `Launch::Edited` (Task 2);
    - `Closed` drops the host, refreshes the catalog (`Shell::refresh_content`, made public from `list_content`) and the editor packs, and shows `Screen::Editor`.
  - **The world.** `Gpu::present` gets `canvas.draw(...)`'s list and area. Games pass `app::playfield(size)` as before.
  - **Play.** While an edited play runs, the HUD draws as in any game, plus a "Stop" button top-right. Esc or Stop ends it (`Shell::end`, no replay), sends `PlayEnded`, and redraws the canvas from the unchanged document.
  - **Closing the window** while the editor is open sends the editor's Close. The window closes only once the editor answers `Closed`, so the unsaved-changes dialog shows first.
  - **The timer.** `EditorHost::tick(Instant::now())` runs once a frame.

- [ ] **Step 1: Write the failing tests.**
  - In `canvas.rs`, `draw_maps_the_view_to_the_canvas`: art scale 2, view zoom 2.0 (`ZOOMS[3]`), offset (10, 20), canvas `(100, 50)..(500, 350)` at ppp 1.5. The list's view starts at world (−2.5, −5) clipped to 0, and the area is the canvas's physical rectangle minus the clipped part. A view wider than `MAX_VIEW` is clipped to the level.
  - `upload_replaces_only_its_rectangle`: after a paint in the document, `upload(r)` changes the map's pixels only in `r × art scale`, and the material view's map shows `MATERIAL_PALETTE` colours.
  - In `main.rs`'s tests, `edit_takes_a_directory_and_keeps_the_menus`: `olr --edit p` parses, and `plays_directly` is false.
  - `edit_refuses_the_bundled_content`: the R6 check (a pure `fn edit_target(edit: &Path, content: &Path) -> Result<PathBuf, String>`) refuses `content/packs/olx-level-dirt-level` and accepts a temporary directory.
  - `closing_the_window_asks_when_unsaved`: the pure `fn on_close_request(host: Option<&mut EditorHost>) -> CloseDecision { Quit, Wait }` gives `Wait` with an unsaved document (the dialog opens) and `Quit` without a host.
  - The GPU test `the_canvas_draws_the_bake` (`OLR_GPU_TESTS=1`): a `Canvas` over the fixture `fixtures/edit/composed-level` rendered offscreen through `Renderer::render_in`. After an edit and `upload`, the image equals a fresh full upload's.
- [ ] **Step 2:** Run `cargo nextest run -p olr-game canvas edit_ closing_` and, with `OLR_GPU_TESTS=1`, `cargo nextest run -p olr-game the_canvas_draws`. Expected: FAIL.
- [ ] **Step 3:** Implement. Main menu → Editor → a level opens the host through `AppCommand::{EditLevel, …}` in `Shell::carry`. `--edit` opens it before the first frame.
- [ ] **Step 4:** Run `cargo nextest run -p olr-game`, the GPU test, and `cargo run -p olr-game -- --edit <a copied pack>` by hand: the canvas shows the level, a paint shows at once, Play and Esc return. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-game: the editor in the window: canvas, play, --edit`.

### Task 7: The end-to-end script (`olr-game`'s autoplay, `xtask e2e`)

**Files:**
- Create: `fixtures/autoplay/editor.ron`.
- Modify: `crates/olr-game/src/autoplay.rs`; `xtask/src/e2e.rs`; `xtask/CLAUDE.md`.

**Interfaces:**
- Produces, in `autoplay::Do`:

  ```rust
  /// Presses at `from` and releases at `to` (fractions 0..=1 of the labelled widget's rect),
  /// moving in 8 steps, one per frame.
  Drag(String, [f32; 2], [f32; 2]),
  /// A click at a fraction of the labelled widget's rect.
  ClickAt(String, [f32; 2]),
  ```

  `KEYS` gains `"Delete"`, `"B"` and `"P"`. `parse` validates the new steps.
- **`fixtures/autoplay/editor.ron`:**
  1. Main: click "Editor", then "New level"; type the name "E2E Level"; click "Create".
  2. Wait for `labels::CANVAS`; click "Add layer", then "Brush".
  3. `Drag("Canvas", [0.3, 0.5], [0.6, 0.5])`; click "Spawn", then `ClickAt("Canvas", [0.5, 0.4])`; assert the label `"Spawn 1 at"` (by `Check::Label`, contains).
  4. Click "Play"; assert `Playing(true)` within 300 frames; `Key("Escape")`; assert `Playing(false)`.
  5. Click "Save"; screenshot `editor`; click "Close"; wait for `Screen::Editor`; Quit.
- **`xtask e2e`** runs it after the menus script, in its own data directory (`target/e2e/editor-data`). Besides the usual checks it reads the saved pack `packs/e2e-level/`:
  - `editor/composition.ron` names a drawn layer;
  - `level.ron` holds one spawn;
  - `olr-cli pack check` exits 0.

- [ ] **Step 1: Write the failing tests:**
  - `autoplay.rs`'s `parse` test gains `Drag`, `ClickAt` and the new keys;
  - `xtask`'s `e2e` unit test `editor_pack_checks_name_what_is_missing` (the pure check over a pack directory listing).
- [ ] **Step 2:** Run `cargo nextest run -p olr-game autoplay` and `cargo test -p xtask e2e`. Expected: FAIL.
- [ ] **Step 3:** Implement the steps (synthesised `PointerButton`/`PointerMoved` events at the widget's rect fractions, as `Click` does) and the e2e run.
- [ ] **Step 4:** Run `cargo xtask e2e`. Expected: `ok` for the menus, the online run and the editor run.
- [ ] **Step 5:** Commit: `olr-game, xtask: the editor's end-to-end script`.
- [ ] **Step 6 (batch C):** Run the online `cargo xtask check`. Expected: every step `ok`.

### Task 8: Docs, records and the playtest release

**Files:**
- Modify:
  - `crates/olr-game/CLAUDE.md` (the host, the canvas, Play, `--edit`, recovery);
  - `crates/olr-editor/CLAUDE.md` (browser, `Recovered`, what the host does);
  - `crates/olr-ui/CLAUDE.md` (the Editor screen, the commands, `Store::{load_file, save_file}`);
  - `crates/olr-terrain/CLAUDE.md` (`replace_rect`);
  - `README.md` (a short "Editing levels" section: the menu, `--edit`, where packs live, recovery);
  - `CHANGELOG.md` `Unreleased`, Added: "The level editor in the game: Editor in the main menu (and `olr --edit <dir>`), painting, images, spawns, Play and recovery.";
  - `docs/porting/STATUS.md`: the editor-core row becomes "done (plans 14b-14c)", plus "Level editor decisions (plan 14c)" with rulings R1-R9;
  - the spec's §9 (14c done).
- [ ] **Step 1:** Write the records above. Run `cargo xtask format-docs --check` and `cargo xtask check`. Expected: PASS.
- [ ] **Step 2:** Commit: `docs: the level editor in the game: notes and plan 14c's records`.
- [ ] **Step 3:** Ask the user before running `cargo xtask full` (memory: it shares their PC). If they agree, run it and report each failure without rerunning.
- [ ] **Step 4:** Run `cargo xtask dist`. Tell the user:
  - the Flatpak and the Windows zip;
  - the Windows path `\\wsl.localhost\Ubuntu-26.04\home\martin\rust-liero\target\release-zips`;
  - what to try: make a level from a photo, paint, place spawns, Play with bots, recovery after killing the game.
