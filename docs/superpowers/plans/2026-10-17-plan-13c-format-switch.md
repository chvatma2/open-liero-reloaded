# Plan 13c: The Switch to Pack Format 5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every pack on disk becomes the authoring format (pack format 5);
every pack reader and writer goes through `olr-format`. The game reads
only native packs. The game's Import screen, load-time colour keying and
skin downloads are gone. OLX import moves to a new tool crate,
`olr-import`. The runtime data the simulation and renderer receive stays
byte-identical; a digest pinned before the switch proves it.

**Architecture:**
- **One seam carries the switch.** `olr-pack` keeps its runtime-level API
  (`read_mod`/`write_mod`, `read_level`/`write_level`, … and the pack
  structs `ModPack`, `LevelPack`, `SkinPack`, `ThemePack`, `CorePack`).
  Its bodies are rebuilt on `olr-format`:
  - **reading:** manifest, then `read_*_files`, then `compile_*`;
  - **writing:** manifest, then `lift_*`, then `write_*_files`.

  So loaders, the importer's single write point and the synthetic test
  packs barely change.
- **Core sprites and mod gfx become alpha images** (keyed once by the
  importers). In the same commit, `olr-scene` and `olr-fx` stop keying at
  load. Themes, skins and levels already compile back to today's runtime
  meaning.
- **`olr-import` (new, layer 3)** takes `olr-content`'s import pipeline.
  `olr-cli` depends on it; the game, server and UI must not (checked by a
  new `forbidden` rule in `xtask/layers.ron`).
- **Skins are matched by id and content hash.** A missing or different
  skin draws as the default skin (spec §4), decided in `olr-game`'s match
  setup through an `olr-content` helper. The protocol drops skin
  transfer (`PROTOCOL_VERSION` 3).

**Tech Stack:** Rust 1.99 (edition 2024). Existing workspace dependencies
only.

**Spec:** `docs/superpowers/specs/2026-10-16-formats-design.md` §1, §4, §5,
§7 and §9 (13c). The research behind every step is
`docs/porting/olx-notes/format-switch.md` (committed with this plan).
Read its headline findings and §10 (risks) first.

**Scope:** plan 13d (next) does `olr-cli pack check|fmt|new`, the level
operations, generated schema docs and `check`'s `pack fmt --check`. Here
`olr-cli validate` stays as it is, re-implemented over the new readers.

## Rulings (the research's decisions D1–D15)

1. **D1:** keep `olr-pack`'s runtime-level API and pack structs,
   re-implemented over `olr-format`. Authoring-level read and write
   (`*Source` with a manifest) comes with 13d.
2. **D2:** importers keep producing runtime structs, written through
   `olr_pack::write_*` (no new layer pairs).
3. **D3:** the importers key core sprites and mod gfx once
   (`image_io::olx_sprite_to_alpha`). `olr_format::core_pack::lift_core`
   becomes purely structural (no keying), so
   `read_core(write_core(p)) == p`.
4. **D4:** the skin importer writes `Rgba::TRANSPARENT` for every hidden
   pixel, so `read_skin(write_skin(s)) == s` for imported skins.
5. **D5:** the 13b "skin importer deviation" is retracted. OLX hides
   key-coloured pixels too (`SetColorKey_Alpha`,
   `src/client/GfxPrimitives.cpp:88-141`, before `Colorize_Execute`). No
   code changes; the STATUS entry and the OLX note are corrected, and
   the existing importer test's comment cites it. The mask-alpha edge
   cases the research lists are recorded as a note, not changed.
6. **D6:** the import pipeline moves to a new crate `olr-import`, layer 3.
   `discover`/`discover_until` stay for future editors. `TMP_PREFIX`,
   `OLD_PREFIX` and `newest_aside_first` move to `olr_pack::install`.
7. **D7:** the user `<data>/packs` layer over `--content` stays
   (`olr-cli import --out <data>/packs` can fill it). The shell keeps its
   own `packs_dir`.
8. **D8:** `olr-format` reads the thumbnail tolerantly (at most
   `MAX_THUMB_BYTES`, 1 MiB; a bad or larger file is left out, never an
   error). It also gains `read_level_files_without_thumb`, which
   `olr_pack::read_level` uses: matches and replays never read
   thumbnails. One `THUMB_FILE` constant (olr-format's), re-exported by
   olr-pack.
9. **D9: the skin match rule.**
   - The local player's own slot uses its skin by id.
   - Every other slot uses the skin whose pack id is exactly the look's
     `skin` and whose content hash equals `skin_hash`. A look with
     `NO_HASH` (bots) gets the named skin only if it is `DEFAULT_SKIN`.
     Anything else gets the default skin, with a warning.
   - Assets are keyed by slot.
   - `SKIN_LIMITS` and `MAX_SKIN_ARCHIVE` stay (tests use them as the
     small archive).
   - `Item::required` stays, but `valid_content` refuses an optional
     item.
10. **D10:** `pack.ron` keeps `olr-pack`'s RON until 13d.
11. **D11:** the switch is split by kind: levels and themes (with the
    version bump), then skins, then mods and core together with the
    keying removal. Each task re-imports `content/packs` and runs the
    **online** `cargo xtask check`, so `olx-content` compares a fresh
    import. The mods-and-core task also runs the GPU render goldens.
    `full` runs once, at the end.
12. **D12:** a tolerant core read in `olr-format`
    (`read_core_files_tolerant`). A bad sprite or a missing sound is
    left out with its error; `giblets` and `giblet-stains` drop
    together.
13. **D13:** `olr-cli validate` keeps its name until 13d.
14. **D14:** asset files keep OLX's lowercased names (e.g.
    `cluster_bits.png`) in 0.2. Renaming them to kebab-case is deferred,
    and the spec is amended: it changes no behaviour and would churn
    snapshots.
15. **D15: content hashes are not re-blessed.** Nothing committed pins a
    pack content hash, and no replay is committed. Old replays and cached
    packs are refused or dropped (the format version and the protocol
    version both change). STATUS and CHANGELOG say so.

## Global Constraints

- **The runtime data is unchanged.** The digest from Task 1 must stay
  byte-identical after every task. The oracle traces, the match goldens
  (`fixtures/matches/`) and the render goldens (`fixtures/render/`,
  `fixtures/ui/`) must not change. If one does, the switch changed
  behaviour: that is a bug to fix, not a fixture to bless.
- **`content/packs` is regenerated, never hand-edited:**
  `cargo xtask fetch-olx-content`, committed with `git add -A
  content/packs`, so deletions are included.
- **Layers:**
  - `olr-import` is layer 3 (dependencies: olr-data, olr-vfs, olr-pack,
    olr-format, the three importers, anyhow, serde, ron) and not in the
    `wasm` list.
  - `olr-content` no longer depends on `olr-import-*`.
  - `olr-game`, `olr-server` and `olr-ui` must not depend on `olr-import`
    or `olr-import-*`, directly or transitively (the new `forbidden`
    rule).
- **Never panic on content; deterministic output; cite OLX** for any
  ported behaviour (CLAUDE.md rules 4, 6, 7).
- **Verification cadence (the user's rule):**
  - inside a task, run targeted `cargo nextest run -p <crate>`;
  - at the end of every task, run `cargo xtask check` **online** (not
    `--offline`), because `content/packs` changes;
  - run `cargo xtask full` once, in Task 10.
- **Commits:** small and imperative, per crate, with the session's
  attribution lines.

## Review Focus

1. **A 0.1 client against a 0.2 host.** It is refused at the handshake
   with a clear message (`PROTOCOL_VERSION` 3). It never half-joins and
   then fails on a skin list. Task 2 tests it.
2. **A player whose skin is the same id but different content.** Every
   other peer draws them with the default skin, never with the local
   pack of the same name. Two players sharing one id with different
   hashes do not collide. Task 2 tests both.
3. **An old settings file** with an `import: (…)` block, and an old data
   directory with user `packs/` in format 4:
   - the settings load (the field is ignored);
   - format-4 packs are skipped with a "re-import" warning, never a
     crash;
   - old download-cache entries are dropped.

   Tasks 3 and 6 test it.
4. **A 254-keyed core sprite or mod image with real magenta.** After the
   switch it keeps the magenta, because the importer keys once and
   nothing keys at load. Task 8 tests it with a synthetic image.
5. **The release has no legacy code.** `olr` and `olr-server` do not link
   any importer crate, checked by the `forbidden` layer rule and by
   `cargo tree` in Task 4.

---

### Task 1: Pin the runtime digest (before anything changes)

**Files:**
- Create: `crates/olr-cli/tests/digest.rs` (a `mod digest;` of
  `tests/integration.rs`) and its insta snapshot
- Read: `docs/porting/olx-notes/format-switch.md` §10.5

**What it pins.** One line per bundled pack, sorted by pack id: the id,
then a SHA-256 (via the `sha2` dev-dependency already used by olr-cli's
tests, or add it) of what the loaders hand to the simulation and the
renderer **today**. Load through `olr_content::Content` (the real
loaders), not `olr-pack`:

| Pack kind | What is hashed, in this order |
|---|---|
| **Mods** | `Content::sim_mod(id)`'s `Debug` text; the `ModDef` RON (`ron::ser::to_string`); every image `Content::load_mod` returns, **after today's load-time keying** (apply `RgbaImage::key_transparent` exactly as `olr-scene`/`olr-fx` do: once, per image), as width, height and raw bytes, in path order; every sound's bytes in name order |
| **Levels** | `load_level`'s material cells, `front`, `back` |
| **Themes** | `load_theme`'s holes, green ball, `front_tile`, `back_tile` (raw, as `olr-terrain` reads them) |
| **Skins** | `LoadedSkin::colorized(c)` for `c` in `[(255,0,0), (96,128,252), (10,200,30)]` |
| **Core** | every sprite `Content::core()` gives, after keying (as `olr-scene`/`olr-fx` do), plus the game sounds and the colours |

Also add `the_colorized_gibs_do_not_depend_on_when_they_are_keyed`. For
the bundled `giblets` and the same three colours,
`key(colorize_gibs(raw, c)) == colorize_gibs(key(raw), c)` pixel for
pixel, where `colorize_gibs` is `olr-fx`'s. If it is not public, add a
`#[doc(hidden)] pub` re-export with a comment that the test needs it.
The test proves that dropping the keying at load (Task 8) keeps the
gibs and their stains.

- [ ] **Step 1:** Write `tests/digest.rs`. Skip it with a `skip:` notice
  when `OLR_CONTENT_DIR` is not a directory, and assert that it found at
  least 2 mods, 4 levels, 1 theme, 700 skins and the core pack.
- [ ] **Step 2:** Generate the snapshot
  (`INSTA_UPDATE=always cargo nextest run -p olr-cli digest`), read it,
  run again (PASS).
- [ ] **Step 3:** Run the online `cargo xtask check`, then commit
  (`olr-cli: pin the runtime digest of the bundled content`).

---

### Task 2: Skins are matched by id and hash; skin transfer is removed (N31)

**Files** (the research's §5 lists every line):
- `olr-proto` (content items, `PROTOCOL_VERSION` 2 → 3)
- `olr-online` (`src/content/peer_side.rs` and the rest of §5)
- `olr-lobby` (`ip_of` if now unused, docs)
- `olr-server` (docs, config comments, tests)
- `olr-content` (`Content::skin_matching`)
- `olr-game` (`setup::load_online`, `net.rs` download view)
- `olr-ui` (docs and the test string)
- `docs/porting/STATUS.md` (N31 replaced), `CHANGELOG.md`

**Interfaces:**

```rust
// olr-content (maps.rs, beside skin_or_default)
/// The skin a match draws for a member's look (spec §4, plan 13c ruling 9):
/// the pack `id` exactly (no name resolution) when it is a skin pack whose
/// content hash is `hash`; with `hash == NO_HASH` only `DEFAULT_SKIN`;
/// otherwise the default skin, and the warning says why.
pub fn skin_matching(&self, id: &str, hash: &str) -> (LoadedSkin, Option<String>)
```

**Steps:**
- [ ] **Step 1: tests first.**
  - `olr-content`:
    - `skin_matching` with an exact match;
    - the same id with another hash (default skin and a warning);
    - an unknown id;
    - `NO_HASH` with `DEFAULT_SKIN` and with another id.
  - `olr-game`: the online setup gives two slots with the same skin id
    and different hashes their own skins (keyed by slot).
  - `olr-online`: the host ignores a client's skin `List` and lists only
    required items.
  - `olr-proto`: `valid_content` refuses an optional item.
- [ ] **Step 2: remove skin transfer.** Remove the offer, fetch, serve
  and lay-over paths exactly as the research's §5 lists them. Bump
  `PROTOCOL_VERSION` to 3. `Look { name, colour, skin, skin_hash }`
  stays; `skin_hash` becomes the match key.
- [ ] **Step 3: wire the rule in.** `setup::load_online` uses
  `skin_matching` for every slot except the local player's own, logging
  each warning once.
- [ ] **Step 4: records.**
  - STATUS: replace N31 with "skins by id and hash, never transferred
    (plan 13c, spec §4)".
  - CHANGELOG `Unreleased`, under `### Changed`: "Skins are no longer
    downloaded. A player whose skin you don't have, or whose skin
    differs from yours, appears with the default skin."
- [ ] **Step 5:** run the online `cargo xtask check`, then commit (one
  commit per crate group is fine).

---

### Task 3: The game's Import screen is removed

**Files:** the research's §3 lists every file and line in `olr-ui`,
`olr-game`, the docs and the CHANGELOG.

**Steps:**
- [ ] **Step 1: an old settings file still loads.** Add a test in
  `crates/olr-ui/tests/store.rs` (the precedent is at its lines 94-97):
  a settings file with an `import: (path: "/x")` block loads, and the
  field is ignored.
- [ ] **Step 2: remove the screen.**
  - Delete `crates/olr-ui/src/import.rs`,
    `crates/olr-ui/src/screens/import.rs`,
    `crates/olr-ui/tests/import.rs` and
    `crates/olr-game/tests/import.rs`.
  - Remove `Screen::Import`, its actions and commands, the importing
    guards, the main-menu button and `Settings::import`.
  - Keep the user packs layer: give the shell its own `packs_dir`
    (Ruling 7), so `Shell::roots()` still layers `<data>/packs` over
    `--content`.
  - The local-play "No mods" text names `olr-cli import`.
- [ ] **Step 3: replace the lost layering test.** The deleted
  `olr-game/tests/import.rs` was the only shell-level test of user packs
  layered over bundled ones. Replace it with a shell test that writes a
  pack into `<data>/packs` (with `olr_pack::write_level` of a tiny level)
  and checks that the menus list it over the bundled one of the same id.
- [ ] **Step 4: records.** Update the CLAUDE.md files, README, STATUS
  (the Import screen row and the deviation section) and CHANGELOG
  (`### Removed`: "The Import screen; import OLX content with
  `olr-cli import`, and later in the editors.").
- [ ] **Step 5:** run the online `cargo xtask check`, then commit.

---

### Task 4: The import pipeline moves to `olr-import`

**Files:**
- Create: `crates/olr-import/` (`Cargo.toml`, `CLAUDE.md`, `src/lib.rs`)
  with `crates/olr-content/src/import.rs` moved in as it is;
  `crates/olr-import/tests/integration.rs` with
  `crates/olr-content/tests/import.rs` moved in (the fixtures path
  `fixtures/olx-mini` stays; fix the relative path)
- Create: `crates/olr-pack/src/install.rs` (`TMP_PREFIX`, `OLD_PREFIX`,
  `newest_aside_first` moved from `olr-content/src/import.rs`; used by
  `olr-import` and by `olr-content`'s `open_layers`)
- Modify: `crates/olr-content/Cargo.toml` (drop the three importers),
  `src/lib.rs`
- Modify: `crates/olr-cli` (`olr_import::import`, `prune`, `ImportPlan`)
- Modify: the workspace `Cargo.toml`, `xtask/layers.ron` (`"olr-import": 3`)
- Modify: `xtask/src/layers.rs` (the new `forbidden` rule)

**The `forbidden` rule:**

```ron
forbidden: [
    // The game, the server and the UI ship without legacy importers (spec §5).
    ("olr-game", "olr-import"), ("olr-game", "olr-import-lgs"), ("olr-game", "olr-import-lxl"), ("olr-game", "olr-import-gfx"),
    ("olr-server", "olr-import"), ("olr-server", "olr-import-lgs"), ("olr-server", "olr-import-lxl"), ("olr-server", "olr-import-gfx"),
    ("olr-ui", "olr-import"), ("olr-ui", "olr-import-lgs"), ("olr-ui", "olr-import-lxl"), ("olr-ui", "olr-import-gfx"),
],
```

`cargo xtask layers` fails when the first crate depends on the second
transitively, counting normal dependencies only (as the layer check
does). Add the check and a unit test in `xtask/src/layers.rs`.

**Steps:**
- [ ] **Step 1:** Move the prefixes into `olr_pack::install`, with
  `olr-content` using them; tests pass.
- [ ] **Step 2:** Create `olr-import`, move the pipeline and its tests,
  and switch `olr-cli` to it.
  - `olr-content` drops the importer dependencies.
  - The moved CLAUDE.md "Import" bullet goes to `crates/olr-import/CLAUDE.md`.
- [ ] **Step 3:** Add the `forbidden` rule and its unit test, then
  confirm with `cargo tree -p olr-game -e normal | grep olr-import`
  (empty).
- [ ] **Step 4:** Run the online `cargo xtask check`. `content/packs`
  must be unchanged: this is a pure move. Commit.

---

### Task 5: What the loaders need from `olr-format`

**Files:** `crates/olr-format/src/{levels,core_pack}/`, `src/error.rs`;
`crates/olr-pack/src/{error.rs,level_pack.rs}`

**Interfaces:**

```rust
// olr_format::levels
pub const THUMB_FILE: &str = "thumb.png";       // the one constant; olr-pack re-exports it
pub const MAX_THUMB_BYTES: u64 = 1 << 20;       // a thumbnail read; larger or undecodable → left out
pub struct LevelHeader { pub name: String, pub size: (u32, u32), pub art_scale: u32, pub theme: String, pub spawns: Vec<SpawnPoint> }
pub fn read_level_header(src: &dyn ContentSource) -> Result<LevelHeader, FormatError>   // level.ron only
pub fn read_level_files_without_thumb(src: &dyn ContentSource) -> Result<LevelSource, FormatError>
// read_level_files: the thumbnail is read tolerantly (≤ MAX_THUMB_BYTES, else None), never an error
// olr_format::core_pack
pub fn read_core_files_tolerant(src: &dyn ContentSource) -> (Result<CoreSource, FormatError>, Vec<FormatError>)
    // a bad sprite or missing sound left out with its error; giblets and giblet-stains drop together;
    // core.ron itself unreadable → the Err
// lift_core: structural only (no keying); its doc says the importer keys (Ruling 3)
// olr_pack::PackError gains `Format(olr_format::FormatError)`, and `Diagnostic::from_error` maps it
//   (line and column for parse errors, the field path in the message otherwise)
```

**Steps:**
- [ ] **Step 1: tests first.**
  - The header read never touches images.
  - A 2 MiB `thumb.png` and an undecodable one are left out, not errors.
  - `read_level_files_without_thumb` ignores the thumbnail.
  - The tolerant core read leaves out a bad sprite and a missing sound,
    and drops the giblets pair together.
  - `lift_core` no longer keys: a magenta pixel stays.
  - `Diagnostic` from a parse error carries the line and column.
- [ ] **Step 2: implement.** Update olr-format's own tests that assumed
  `lift_core` keys: the bundled-core equivalence test keys the old
  sprites before lifting, as the importer will.
- [ ] **Step 3:** Run the online `cargo xtask check`, then commit.

---

### Task 6: Levels and themes switch (pack format 5)

**Files:** `crates/olr-data/src/manifest.rs` (`PACK_FORMAT_VERSION = 5`),
`crates/olr-pack/src/{level_pack.rs,theme_pack.rs,ron_io.rs}`, their unit
tests; `crates/olr-format/tests/{levels,themes}.rs`; `content/packs/**`.

**Steps:**
- [ ] **Step 1: re-implement the level and theme APIs over olr-format.**

  ```rust
  read_level(src) = read_manifest + read_level_files_without_thumb + compile_level   // thumb: None
  read_level_def(src) = read_manifest + read_level_header                             // as LevelDef
  write_level(sink, pack) = write_manifest + lift_level(level, thumb bytes) + write_level_files
  read_theme(src) = read_manifest + read_theme_files + compile_theme
  write_theme(sink, pack) = write_manifest + lift_theme + write_theme_files
  ```

  - `write_level` encodes `LevelPack.thumb` with `encode_rgba`, keeping
    `check_thumb_size`.
  - `read_level_thumb` and `decode_thumb` are unchanged.
  - Errors map through `PackError::Format`.
- [ ] **Step 2: bump the version.** `PACK_FORMAT_VERSION` 4 → 5, and the
  "re-import" message names 5. Add a test: a format-4 pack in a user
  layer is skipped with a "re-import" warning by `Content::open_layers`
  (Review Focus 3). Add another: `DownloadCache::open` drops a format-4
  entry.
- [ ] **Step 3: rewrite the format-4 tests.**
  - The olr-pack unit tests that pin the format-4 layout (research §6)
    become format 5.
  - `crates/olr-format/tests/{levels,themes}.rs` read `content/packs`
    directly as format 5: `read_*_files`, then write, then the same
    bytes, then `compile ==` (the canonical round trip of spec §6).
- [ ] **Step 4: re-import.** Run `cargo xtask fetch-olx-content` and
  inspect the diff:
  - every pack's `pack.ron` changes (format 5);
  - level packs change `level.ron`, and their PNGs should be identical;
  - the theme gains `.mask.png` files;
  - skin, mod and core packs may change only in `pack.ron`. **Ruling:**
    if writing format 5 for one kind forces the version bump onto every
    pack, the pack ids and files of the other kinds stay format-4-shaped
    until their task, with format 5 in `pack.ron`. Their readers are
    unchanged, and the version only promises "this build's format".
- [ ] **Step 5: verify.** The digest test (Task 1) is unchanged. Run the
  online `cargo xtask check`.
- [ ] **Step 6:** Commit the code (`olr-pack: levels and themes in format
  5`) and the content (`content: re-import levels and themes in format
  5`).

---

### Task 7: Skins switch

**Files:** `crates/olr-pack/src/skin_pack.rs` and its tests;
`crates/olr-import-gfx/src/skin.rs` (normalise; the D5 comment);
`crates/olr-import-gfx/tests/olx_gfx.rs`; `crates/olr-format/tests/skins.rs`;
`docs/porting/STATUS.md`, `docs/porting/olx-notes/asset-formats.md` (the
retraction); `content/packs/olx-skin-*`.

**Steps:**
- [ ] **Step 1: re-implement over olr-format.**
  `read_skin` = manifest + `read_skin_files` + `compile_skin`, and
  `write_skin` = manifest + `lift_skin` + `write_skin_files`.
- [ ] **Step 2: normalise in the importer.**
  - The skin importer writes `Rgba::TRANSPARENT` for every pixel it marks
    `Transparent` (Ruling 4).
  - Its test of a key-coloured pixel under a tint mask keeps asserting
    `Transparent`, with the comment citing `SetColorKey_Alpha`
    (`src/client/GfxPrimitives.cpp:88-141`) and Ruling 5.
- [ ] **Step 3: retract the deviation.** Replace the STATUS "deviation"
  entry, and the paragraph in `asset-formats.md` §3.3, with: "Not a
  deviation: OLX's `SetColorKey_Alpha` gives key-coloured pixels alpha 0
  before `Colorize_Execute`, which keeps alpha, so OLX hides them as the
  importer does (corrected by plan 13c)." Add a note on the mask-alpha
  edge cases from the research's §9, unchanged.
- [ ] **Step 4: rewrite the tests.** `crates/olr-format/tests/skins.rs`
  becomes the format-5 canonical round trip over `content/packs`. The
  importer's round-trip tests hold again with the normalisation.
- [ ] **Step 5: re-import, then verify.** Re-import with `cargo xtask
  fetch-olx-content`. The digest is unchanged (the skins' colorized
  output). Run the online `cargo xtask check`.
- [ ] **Step 6:** Commit the code and the content separately.

---

### Task 8: Mods and the core pack switch; load-time keying ends (one change)

**Files:**
- `crates/olr-pack/src/{mod_pack.rs,core_pack.rs}` and their tests
- `crates/olr-import-lgs/src/convert.rs` (mod gfx: decode,
  `olx_sprite_to_alpha`, `encode_rgba`)
- `crates/olr-import-gfx/src/core_pack.rs` (sprites keyed once)
- `crates/olr-scene/src/assets.rs` and `crates/olr-fx/src/assets.rs`
  (delete `keyed()` and its tests; reword the docs)
- `crates/olr-fx/tests/common/mod.rs` (the synthetic gib sheet's
  column 0 becomes `Rgba::TRANSPARENT`)
- `crates/olr-fx/tests/entities.rs:154,163` (`c.a != 0`)
- `crates/olr-format/tests/{equivalence,core_pack}.rs`
- `content/packs/olx-classic`, `olx-mw-1-0`, `olx-core`

**Steps:**
- [ ] **Step 1: re-implement the mod and core APIs over olr-format.**

  ```rust
  read_mod(src) = read_manifest + read_mod_files + compile_mod + today's asset walk (skipping
                  mod.ron, weapons/, projectiles/, restrictions.ron, pack.ron)
  read_preset_names(src) = read_manifest + from_canonical("restrictions.ron")   // cheap
  write_mod(sink, pack) = write_manifest + lift_mod + write_mod_files + the assets as they are
  read_core(src) = read_manifest + read_core_files + compile_core
  read_core_tolerant(src) = read_manifest + read_core_files_tolerant + compile_core (errors carried)
  write_core(sink, pack) = write_manifest + lift_core + write_core_files
  ```

  Run the synthetic-mod tests first (`olr-content/tests/content.rs`
  `broken_mod`, `olr-replay/tests/content.rs` `mod_pack`). If `lift_mod`
  refuses one, rewrite that fixture as an LX56-expressible mod that keeps
  what the test tests; do not weaken the check.
- [ ] **Step 2: key once, in the importers.**
  - The mod importer writes `gfx/*.png` as alpha images
    (`olx_sprite_to_alpha`, `encode_rgba`).
  - The core importer keys its sprites.
- [ ] **Step 3: stop keying at load, in the same change.** Delete
  `keyed()` in `olr-scene` and `olr-fx` and their keying unit tests.
  Update the olr-fx test fixture and assertions (listed above).
- [ ] **Step 4: Review Focus 4.** Add a test, in `olr-scene` or
  `olr-import-gfx`: a synthetic sprite whose corner is (254,0,254) and
  which contains pure magenta. After import (keyed once) and load (not
  keyed), the magenta pixel is still there.
- [ ] **Step 5: rewrite the format-4 tests.**
  - `crates/olr-format/tests/{equivalence,core_pack}.rs` become format-5
    round trips over `content/packs`. Keep the mod text snapshots: they
    must equal the committed files' bytes.
  - The olr-pack mod and core unit tests become format 5.
- [ ] **Step 6: re-import, then verify.**
  - Re-import with `cargo xtask fetch-olx-content`.
  - The **digest is unchanged**: the loaders' images now equal what keying
    gave.
  - Run the online `cargo xtask check`.
  - Then run the GPU render goldens:
    `OLR_GPU_TESTS=1 cargo nextest run -p olr-cli --test integration render_golden`
    and the olr-scenario render goldens (read `xtask/src/check.rs`'s
    `GPU_TESTS` for the exact list). They must pass **unchanged**.
- [ ] **Step 7:** Commit the code and the content separately.

---

### Task 9: Validation over the new readers

**Files:** `crates/olr-pack/src/validate.rs`

**Steps:**
- [ ] **Step 1: rebuild `validate`.**
  - `read_manifest`, then the slug check, then the kind's reader. Its
    errors are mapped through `PackError::Format` to `Diagnostic`, with
    the line and column for parse errors and the field path in the
    message.
  - For mods, today's `check_mod` runs on the compiled `ModDef` and the
    assets: missing and unused assets, unused shared projectiles.
- [ ] **Step 2: tests.**
  - The importer tests' `validate == []` still hold.
  - The corpus test (`crates/olr-cli/tests/corpus.rs`) passes. Its insta
    snapshot is unchanged, or re-recorded after reading the
    `.snap.new` if only the source-level counts moved; say which.
  - New tests: a format-5 RON error gives a `Diagnostic` with a line;
    an invalid weapon value gives its field path.
- [ ] **Step 3:** Run the online `cargo xtask check`, then commit.

---

### Task 10: Records, and the single `full`

- [ ] **Step 1: docs.**
  - **STATUS:** the 13a and 13b rows become "the packs' format (plan
    13c)".
  - **D15:** content hashes changed and nothing is re-blessed; old
    replays and caches are refused.
  - **CHANGELOG `Unreleased`:** packs need re-importing (`cargo xtask
    fetch-olx-content`, or `olr-cli import`); the format is version 5.
  - **CLAUDE.md files** of olr-pack (layouts and version 5), olr-content,
    olr-format (remove "nothing applies it at load once plan 13c
    switches"), olr-scene, olr-fx, olr-online, olr-ui, olr-game, olr-cli
    and olr-import.
  - **The spec:** amend §5 for Ruling 14 (asset names kept), and §1 for
    D15 (no re-bless needed).
- [ ] **Step 2: verify.** Run the online `cargo xtask check`, then
  `cargo xtask full`. Expected: every step `ok`, and no golden changed.
- [ ] **Step 3:** Commit (`docs: plan 13c's records`).
