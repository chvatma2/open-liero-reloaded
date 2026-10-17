# The switch to pack format 5 (research for plan 13c)

Research for plan 13c ("the switch"). Done read-only at `f81082f` (main, clean).
Every citation is `path:line` at that commit.

Inputs read:
- `docs/superpowers/specs/2026-10-16-formats-design.md` (all)
- `crates/olr-format/CLAUDE.md`
- `docs/superpowers/ledgers/plan-13b-asset-formats.md`
- OLX at `target/olx-src/d1ad953f/` (for item 9)

## Headline findings

1. **One seam carries the whole switch.** If olr-pack's `read_*`/`write_*` keep their runtime-struct
   signatures and are re-implemented on top of olr-format, almost no caller changes:
   - loaders: `olr-content` mods.rs, maps.rs, lib.rs, catalog.rs
   - importer output: `olr-content/src/import.rs:218-229`, the single place where import results
     become pack files
   - synthetic test packs in about 15 test sites

   Only four reads need new olr-format entry points, because olr-format keeps those types private
   or reads all-or-nothing:
   - the menus' cheap reads (`read_level_def`, `read_preset_names`)
   - the thumbnail-free level read
   - the tolerant core read
2. **Core sprites and mod gfx become alpha images; nothing else changes meaning at runtime.**
   - `compile_theme` writes OLX's colour-coded holes, green ball and stones back.
   - `compile_skin` and `compile_level` give today's structs.
   - So load-time keying is only in `olr-scene/src/assets.rs` and `olr-fx/src/assets.rs`. It must go in the
     same commit that makes core sprites and mod gfx alpha (13b carry-forward).
3. **`lift_core` keys the sprites itself** (`crates/olr-format/src/core_pack/mod.rs:222`).
   - So it must not be the path for writing an already-alpha runtime `CorePack`: a 254-keyed sprite would
     lose its real magenta.
   - The plan must move that keying into the importer, or give olr-pack a non-keying path. See D3.
4. **Item 9 (the skin importer deviation) is very likely a non-issue.**
   - OLX loads skins as alpha surfaces, and `SetColorKey` gives key-coloured pixels alpha 0
     (`SetColorKey_Alpha`) before `Colorize_Execute` runs.
   - `Colorize_Execute` keeps the pixel's alpha, so OLX hides those pixels too.
   - The importer already matches OLX. The STATUS entry (`docs/porting/STATUS.md:82-84`) should be
     retracted rather than "fixed". Details in §9.
5. **The Import screen lives in olr-ui, not olr-game** (`crates/olr-ui/src/import.rs`).
   - No autoplay script, e2e run or PNG golden touches it.
   - No Cargo dependency becomes unused.
   - **Trap:** `MenuData::packs_dir` also feeds `Shell::roots()`, which layers the data directory's
     user `packs/` over the content (`crates/olr-game/src/shell.rs:369-370`).
6. **Skin fetching is almost entirely in `crates/olr-online/src/content/peer_side.rs`.**
   - Today the match picks skins by id only, in `crates/olr-game/src/setup.rs:273-280`. `skin_hash` is
     never checked when drawing.
   - The new rule (id + hash → that skin, else the default) belongs there, through an olr-content
     helper.
7. **The import pipeline cannot stay in olr-content** once olr-content drops the importers.
   - A new layer-3 crate (`olr-import`) is the clean home: olr-cli and future editors depend on it.
   - It owns `discover`, `prune`, the report and fixtures/olx-mini. `fixtures/olx-mini.py` stays where
     it is.
   - olr-content keeps only the `.replaced-<id>` recovery in `open_layers`. Its two helpers move down to
     olr-pack.

8. **Nothing committed pins a pack content hash, and no replay is committed** (§6).
   - The spec's "hash re-bless" is a no-op.
   - What must be regenerated: `content/packs` (`cargo xtask fetch-olx-content`), the olr-pack unit tests
     written against the format-4 layout, and possibly two or three insta snapshots.
   - Render goldens, match goldens and the oracle must stay byte-identical. They prove the switch is
     lossless.

---

## 1. Callers of olr-pack's readers, writers and validator

olr-pack's public API is listed in `crates/olr-pack/src/lib.rs:20-34`:
- `read_core`, `read_core_tolerant`, `write_core`, `CorePack`
- `content_hash`, `PRESENTATION_FILES`
- `LevelPack`, `THUMB_*`, `MAX_THUMB_*`, `decode_thumb`, `read_level`, `read_level_def`,
  `read_level_thumb`, `write_level`
- `ModPack`, `RESTRICTIONS_FILE`, `read_mod`, `read_preset_names`, `write_mod`
- `read_manifest`, `write_manifest`, `MAX_FILE_BYTES`, `MAX_RON_BYTES`
- `SkinPack`, `read_skin`, `write_skin`, `ThemePack`, `read_theme`, `write_theme`
- `validate`, `Diagnostic`, `Severity`
- `archive`, `image_io` (re-export of olr-format's)

Runtime pack structs:
- `ModPack {manifest, def: ModDef, assets: BTreeMap<VPath, Vec<u8>>}` (`mod_pack.rs:23-28`)
- `LevelPack {manifest, level: Level, thumb: Option<RgbaImage>}` (`level_pack.rs:42-48`)
- `SkinPack {manifest, skin}` (`skin_pack.rs:19-22`)
- `ThemePack {manifest, theme}` (`theme_pack.rs:18-21`)
- `CorePack {manifest, def, sprites, sounds}` (`core_pack.rs:18-26`)

Dependency kinds:
- Normal dependency: olr-cli, olr-content, olr-game, olr-online, olr-render, xtask, the three importers.
- Dev-dependency only: olr-replay, olr-scenario, olr-ui, olr-format.
- There is a dev cycle: olr-format dev-depends on olr-pack, and olr-pack depends on olr-format
  (`crates/olr-format/Cargo.toml:20`).

### 1.1 Production callers

| Caller | Call | What it needs |
|---|---|---|
| `crates/olr-content/src/mods.rs:79-81` (`mod_pack`) → `sim_mod` :86-97, `weapon_rules` :101-104, `load_mod` :110+ | `read_mod` | `ModDef` (for `SimMod::compile`, `WeaponRules::new`, restrictions, worm, rope) + `assets` (PNG bytes decoded at :66-72 for images and frame counts; sfx bytes) |
| `crates/olr-content/src/maps.rs:104-118` `load_level` | `read_level` | `Level` (material, front, back, def); **no thumbnail** |
| `crates/olr-content/src/maps.rs:124-144` `load_theme` | `read_theme` | `Theme` (holes, green ball, tiles; stones/misc unused at runtime) |
| `crates/olr-content/src/maps.rs:147-162` `load_skin` | `read_skin` | `Skin` (672 × 18 sheet + mask) |
| `crates/olr-content/src/lib.rs:146`, `:170`, `:271` | `read_manifest` | manifest only (listing, layering, `with_packs` kind check) |
| `crates/olr-content/src/lib.rs:187` | `read_core_tolerant` | `CorePack` minus bad sprites/sounds + their errors (placeholders, `core_assets.rs:48-84`) |
| `crates/olr-content/src/core_assets.rs:100` | builds an empty `olr_pack::CorePack` | the type only |
| `crates/olr-content/src/catalog.rs:143-144` `mod_info` | `read_manifest`, `read_preset_names` | name + preset names, **cheap** (no weapons) |
| `crates/olr-content/src/catalog.rs:153` `level_info` | `read_level_def` | name, width, height, **cheap** (no image) |
| `crates/olr-content/src/catalog.rs:73,91,98,104` | `THUMB_FILE`, `decode_thumb`; `image_io::png_size` (:82, :101) | thumbnail probe and load, outside `read_level` |
| `crates/olr-content/src/catalog.rs:18` | re-exports `MAX_THUMB_BYTES`, `MAX_THUMB_SIDE` | constants |
| `crates/olr-content/src/replay.rs:22` `pack_hash` | `content_hash` | hash of the directory |
| `crates/olr-content/src/cache.rs:29-30, 269, 328, 702, 706, 714` | `archive::{decode, Limits, ArchiveError}`, `content_hash`, `read_manifest`, `validate` | install verification and adoption (§7, §8) |
| `crates/olr-content/src/import.rs:49` | `MAX_FILE_BYTES` | `MAX_SOURCE_BYTES` |
| `crates/olr-content/src/import.rs:222-226` (`Imported::write_to`) | `write_mod`, `write_level`, `write_skin`, `write_theme`, `write_core` | **the only production writer**: importer output → pack files |
| `crates/olr-content/src/import.rs:491, 525, 547` | `MANIFEST` | prune / leftover detection |
| `crates/olr-content/src/lib.rs:136-144` | `MANIFEST`, `import::OLD_PREFIX`, `import::newest_aside_first` | `.replaced-<id>` recovery in `open_layers` |
| `crates/olr-cli/src/validate.rs:3-41` (`olr-cli validate`, `main.rs:79-80,107`) | `validate`, `Severity`, `MANIFEST` | diagnostics |
| `crates/olr-online/src/content/worker.rs:14, 168, 175-177` | `archive::Limits`, `content_hash`, `archive::encode` | hashing and archiving packs to serve |
| `crates/olr-online/src/content/mod.rs:55` | doc mention of `MAX_ARCHIVE` | — |
| `crates/olr-game/src/gpu.rs:370`, `crates/olr-render/src/offscreen.rs:173` | `image_io::encode_rgba` | screenshots (unaffected) |
| `xtask/src/oracle_gen.rs:95` | `read_mod` | weapon list of the bundled mods (`ModDef`) |
| `crates/olr-import-lgs/src/convert.rs:11, 49` | constructs `ModPack` | importer output |
| `crates/olr-import-lxl/src/lib.rs:15` | constructs `LevelPack` | importer output |
| `crates/olr-import-gfx/src/{skin.rs:8, theme.rs:9, core_pack.rs:10}`, `thumb.rs:43` | construct `SkinPack`/`ThemePack`/`CorePack`; `THUMB_SIZE` | importer output |
| `crates/olr-import-gfx/src/{bmp.rs:11,94, lib.rs:33-35}`, `crates/olr-import-lxl/src/lib.rs:193-197` | `image_io` | decoding (unaffected) |

### 1.2 Test callers

The writers of synthetic packs are the reason to keep runtime-level writers.

| Test | Calls |
|---|---|
| `crates/olr-cli/tests/corpus.rs:31-122` | for every bundled pack: `validate`, `read_manifest`, then per kind `read_x` → `write_x` → `read_x == p`, plus `read_level_thumb` (:86); insta summary `crates/olr-cli/tests/snapshots/integration__corpus__imported_content.snap` |
| `crates/olr-content/tests/content.rs:45-174, 259, 398-443` | builds `ModPack`/`LevelPack`/`ThemePack`/`CorePack`/`SkinPack` from runtime structs; `write_mod/level/theme/core/skin` |
| `crates/olr-content/tests/catalog.rs:9, 44, 61-107, 136` | `LevelPack`, `write_level`, `write_manifest`, `THUMB_FILE`, `RESTRICTIONS_FILE` (writes raw `{"Standard 100lt": {}, "All": {}}` at :71), `THUMB_SIZE` |
| `crates/olr-content/tests/cache.rs:12, 43, 51, 63, 97, 298, 330-333, 549` | `write_skin` (`SkinPack`), `write_manifest`, `content_hash`, `archive` |
| `crates/olr-content/tests/import.rs:13, 51, 241` | `LevelPack`, `write_level`, `declared_png` |
| `crates/olr-replay/tests/content.rs:45-131, 189-190` | `ModPack`/`LevelPack`/`ThemePack` + writers, `THUMB_SIZE` |
| `crates/olr-online/tests/content.rs:26, 75, 126, 291, 334, 372, 433, 443`; `crates/olr-online/src/content/peer_side.rs:1350-1361` (skin re-id test helper); `crates/olr-online/tests/common/mod.rs:72-77` | `write_level`, `content_hash`, `archive`, skin.png re-encode |
| `crates/olr-game/tests/downloads.rs:57, 334` | `archive::encode`, `file_charge`, `BLOCK` |
| `crates/olr-ui/tests/flows.rs:27-28` | `image_io::encode_rgba`, `THUMB_FILE` |
| `crates/olr-cli/tests/cli.rs:26`, `render_golden.rs:70`, `crates/olr-scenario/tests/render_golden.rs:32-50` | `image_io` only |
| importer tests: `crates/olr-import-lgs/src/convert.rs:302-315`, `crates/olr-import-lgs/tests/olx_mods.rs:25-29`, `crates/olr-import-lxl/tests/olx_levels.rs:29-31`, `crates/olr-import-gfx/tests/olx_gfx.rs:26-60`, `crates/olr-import-gfx/src/core_pack.rs:177-178` | `write_x` then `validate == []` and `read_x == imported` |
| olr-format's own tests: `tests/equivalence.rs:20,56,98`, `tests/levels.rs:35`, `tests/skins.rs:26`, `tests/themes.rs:36`, `tests/core_pack.rs:27` | read the **bundled packs through today's olr-pack readers**: the "old path" of the equivalence tests |
| `crates/olr-pack/tests/archive.rs:10` | a hard-coded `format_version: 4` string (archive bytes only; it never reads the manifest, so it still passes; worth updating for honesty) |

### 1.3 What each caller needs: the API proposal that keeps churn minimal

Keep olr-pack's runtime-level API and change only its bodies. Add an authoring-level API beside it.

**Runtime level (loaders, tests).** Same names and structs.
- `read_mod` = `read_manifest` + `olr_format::mods::read_mod_files` + `compile_mod` + the asset walk.
  - The walk is today's `mod_pack.rs:96-117`. It must skip `mod.ron`, `weapons/`, `projectiles/` and
    `restrictions.ron`, which it already does.
- `read_level` = `read_manifest` + `read_level_files` (thumbnail skipped, see below) + `compile_level`.
- `read_skin` / `read_theme` = manifest + `read_*_files` + `compile_*`.
- `read_core` = manifest + `read_core_files` + `compile_core`. **Its sprites are now alpha.**
- `write_x(&XPack)` = `write_manifest` + `lift_x` + `write_x_files` (+ mod assets passed through).
  - This keeps `import.rs:218-229` and all the synthetic test packs unchanged.
  - Caveat for mods: `lift_mod` may refuse a hand-built `ModDef` that is not LX56-expressible. Check
    `olr-content/tests/content.rs` `broken_mod` (:45-101) and `olr-replay/tests/content.rs` `mod_pack`
    (:45-81).
  - Caveat for core: see D3; `lift_core` keys.
  - Caveat for levels: `LevelPack.thumb` is an `RgbaImage` and `lift_level` takes bytes.
    `write_level` encodes it with `encode_rgba`, keeping `check_thumb_size` (`level_pack.rs:87-96`).

**Cheap reads.** These need new olr-format entry points.
- `read_level_def` (`level_pack.rs:131-138`) parses `level.ron` into `LevelDef` today.
  - Format 5's `level.ron` is `LevelFile` (`size`, `objects`), private in
    `crates/olr-format/src/levels/files.rs:27-39`.
  - Add `olr_format::levels::read_level_header(src) -> LevelHeader { name, size, art_scale, theme }`.
    Alternatively make `LevelFile` public.
  - `read_level_def` can then keep returning `(PackManifest, LevelDef)` with `spawns` filled from the
    objects, or `vec![]`.
- `read_preset_names` (`mod_pack.rs:123-135`) reads `restrictions.ron`.
  - Format 5 keeps `restrictions.ron` (`crates/olr-format/src/mods/files.rs:15`), written canonically.
  - Read it with `olr_format::ron_io::from_canonical` into `BTreeMap<String, Preset>`.
  - Check that the canonical reader accepts `crates/olr-content/tests/catalog.rs:71`'s hand-written
    text, or rewrite that test with `write_mod`.

**Level read without the thumbnail.**
- `read_level_files` always reads `thumb.png` if it exists, up to 64 MiB
  (`crates/olr-format/src/levels/files.rs:134-136`). A bad or huge one fails the whole read (13b
  carry-forward).
- olr-pack's `read_level` must never read it (`level_pack.rs:139-143`; matches and replays never pay for
  it).
- Options:
  - (a) olr-format gains `read_level_files_without_thumb` or a `thumb: bool` parameter;
  - (b) olr-pack wraps the source in a filtering `ContentSource` that hides `THUMB_FILE`;
  - (c) the thumbnail leaves `LevelSource` and becomes olr-pack's (presentation file).
- Separately, olr-format's own read should cap the thumbnail at olr-pack's `MAX_THUMB_BYTES` (1 MiB) and
  skip a bad one, not fail. That is the carry-forward from the 13b ledger.

**Tolerant core read.**
- `read_core_tolerant` (`core_pack.rs:80`) leaves out each undecodable sprite or missing sound and
  returns the errors. olr-content relies on it (`lib.rs:187`, CLAUDE.md "core pack read tolerantly").
- `read_core_files` (`crates/olr-format/src/core_pack/files.rs:70`) is all-or-nothing.
- Add a tolerant variant in olr-format: per-sprite and per-sound errors, `giblets` and
  `giblet-stains` dropped together. Or decide that a bad core pack is an error.

**Authoring level (importer, editors, 13d's `pack fmt/new`).**
- New `ModPackSource {manifest, source: ModSource, assets}` (and per kind) with `read_*_source` and
  `write_*_source`.
- The authoring-level writer must **clear the authoring files first**: `write_mod_files` never deletes
  (olr-format CLAUDE.md "write_mod_files never deletes").
- 13c only needs this if the importer writes sources directly (D2). Otherwise it can wait for 13d.

**Unchanged.** `read_manifest`, `write_manifest`, `content_hash`, `PRESENTATION_FILES`, `archive`,
`decode_thumb`, `read_level_thumb`, `THUMB_*`, `MANIFEST`, `image_io`.

---

## 2. The import pipeline

### 2.1 How a pack is produced today

```
olr-cli import (crates/olr-cli/src/import.rs:52-80)  ─┐
olr-ui ImportJob (crates/olr-ui/src/import.rs:248-291) ┴─▶ olr_content::import::import (import.rs:441)
   discover / ImportPlan → Job per item → run_job (import.rs:~325-350):
     Job::Mod   → olr_import_lgs::import_mod      (import.rs:333) → ImportedMod { pack: ModPack, warnings }
     Job::Level → olr_import_lxl::import_level + olr_import_gfx::level_thumbnail (import.rs:336-337)
     Job::Theme → olr_import_gfx::import_theme    (import.rs:340)
     Job::Skin  → olr_import_gfx::import_skin     (import.rs:342)
     Job::Core  → olr_import_gfx::import_core     (import.rs:346)
   Imported::write (import.rs:235-268): write_to(.import-<id>) → rename old aside (.replaced-<id>) → rename in
   Imported::write_to (import.rs:218-229): olr_pack::write_{mod,level,skin,theme,core}
   then import-report.ron (REPORT_FILE import.rs:46), prune of older runs
```

`xtask fetch-olx-content` and `check`'s `olx-content` step:
- `xtask/src/fetch.rs:200-232` runs `cargo run -p olr-cli -- import --olx <gamedir> --out <dir> --revision
  <commit> --mod … --level … --theme … --skins --core … --extra-skins …`.
- `run` (:115-124) writes into `content/packs`.
- `verify` (:129-148) writes into `target/content-fresh`. It then fails on any file difference
  (`differences` :159-174, every file including `import-report.ron`).
- `check.rs:199-208`: the `olx-content` step, skipped with `--offline`. It also sets `OLR_OLX_GAMEDIR`
  and `OLR_CONTENT_DIR` for the tests.

### 2.2 Where lift + write slot in

**Option A (minimal).** `Imported::write_to` is unchanged. olr-pack's `write_*` lift and write format 5
(§1.3). The importers change only where runtime meaning changes:
- **Mod gfx to alpha.** `olr-import-lgs` copies asset bytes verbatim (`crates/olr-import-lgs/src/convert.rs:186`).
  - For `gfx/*.png` it must decode, apply `olx_sprite_to_alpha`, and re-encode with
    `image_io::encode_rgba`.
  - The importer depends on `olr-pack` (whose `image_io` re-exports olr-format's), so no new crate
    edge.
  - Exact for the bundled mods: `crates/olr-format/tests/core_pack.rs:45-64`.
- **Core sprites to alpha.** `import_core` stores raw decoded sprites (`crates/olr-import-gfx/src/core_pack.rs:89-91`).
  - Either it keys them, and `lift_core` stops keying; or `write_core` keeps calling `lift_core` on raw
    sprites, and then `read_core(write_core(p)) != p` (D3).
- **Skins.** The importer keeps RGB under alpha 0 (`crates/olr-import-gfx/src/skin.rs:58-62`), while
  `compile_skin` writes `(0,0,0,0)` there and in the spacing columns.
  - The importer round-trip asserts `read_skin(write_skin(s)) == s` (`olr-import-gfx/tests/olx_gfx.rs:26-27`)
    and the corpus round trip.
  - These hold only if the importer normalises transparent pixels to `Rgba::TRANSPARENT`, or the tests
    compare colorized output.
- **Levels and themes.** No change needed. `lift_level`/`lift_theme` are exact on the bundled ones
  (13b tests).

**Option B (cleaner for editors).**
- Importers produce olr-format sources (`ImportedMod { manifest, source: ModSource, assets }`), and
  `write_to` calls the authoring-level writers.
- This needs `same_layer` pairs `(olr-import-*, olr-format)` in `xtask/layers.ron:44-48`, which today
  lists only `(olr-import-*, olr-pack)`.
- It touches all importer tests.

Option A is enough for 13c. B can come with the editors.

### 2.3 Who calls import

| Caller | Location |
|---|---|
| `olr-cli import` | `crates/olr-cli/src/import.rs:1-80`, `main.rs:33-34, 89`; tests `crates/olr-cli/tests/cli.rs` (makes a tiny OLX dir, e.g. :43) |
| xtask | `xtask/src/fetch.rs:115-124, 129-148, 200-232`; `xtask/src/check.rs:199-208`; also `bench`, `stress`, `e2e` call `fetch::verify` (xtask CLAUDE.md); `xtask/src/dist.rs:46` leaves `import-report.ron` out of the zips |
| game Import screen | `crates/olr-ui/src/import.rs:249-266` (`discover_until`, `import`, `ImportOptions { revision: LOCAL_REVISION, prune: false }`); shell glue `crates/olr-game/src/shell.rs:400-434` (§3) |
| tests | `crates/olr-content/tests/import.rs` (fixtures/olx-mini), `crates/olr-ui/tests/import.rs`, `crates/olr-game/tests/import.rs` |

### 2.4 Where the pipeline should live

Constraints:
- olr-content (layer 4) must drop `olr-import-lgs/lxl/gfx` (`crates/olr-content/Cargo.toml:21-23`;
  spec §5).
- The `olr` binary and the zips must carry no legacy code.
- Editors (later sub-projects) need "Import OLX…".

**Recommendation: a new crate `olr-import` at layer 3.**
- Its dependencies are all layer ≤ 2: olr-data, olr-vfs, olr-pack, olr-format, the three importers,
  plus anyhow, serde and ron. So layer 3 is legal with no `same_layer` entry. Register it in
  `xtask/layers.ron:8-43`.
- Should the `wasm` list (`layers.ron:76`) include it? It uses `std::fs`, so no.
- Users: olr-cli (layer 6) and future editors.
- **Not olr-game, olr-server or olr-ui.** `cargo xtask layers` cannot express "not a dependency of
  olr-game". A dist check (e.g. `cargo tree -p olr-game` not containing `olr-import`) would guard the
  "no legacy code in the zips" rule.

Rejected alternatives:
- **olr-cli module.** It needs no new crate, but an editor binary would have to depend on olr-cli's lib.
  olr-cli has `crates/olr-cli/src/lib.rs`, but depending on it is odd.
- **Keep it in olr-content.** That contradicts the spec.

**What moves with it:**
- All of `crates/olr-content/src/import.rs` (697 lines): `ImportPlan`, `SkinSource`, `ImportOptions`,
  `ImportEntry`, `ImportReport`, `Progress`, `import`, `prune`, `discover`, `discover_until`,
  `Cancelled`, `REPORT_FILE`, `MAX_SOURCE_BYTES`, `OLX_CORE_FILES`, the job and write logic.
- Its test `crates/olr-content/tests/import.rs`, plus fixtures/olx-mini (the path is relative to the
  crate).
- After the Import screen is removed, `discover`/`discover_until`/`Discovered`/`Cancelled` have no
  production caller (olr-cli does not use `discover`). Keep them for the editors' "Import OLX…", or
  drop them and rewrite the tests with explicit `ImportPlan`s.
- olr-content's CLAUDE.md "Import" bullet (113-140) moves to the new crate's CLAUDE.md.

**What must stay or move down:**
- `open_layers` uses `import::OLD_PREFIX` and `import::newest_aside_first` (`crates/olr-content/src/lib.rs:139,144`;
  `import.rs:274` `pub(crate) const OLD_PREFIX`).
- These describe the on-disk "pack being replaced" convention that both the writer (import) and the
  reader (`open_layers`) share.
- Move them, with `TMP_PREFIX` (`import.rs:272`), to olr-pack (e.g. `olr_pack::install::{TMP_PREFIX,
  OLD_PREFIX, newest_aside_first}`) or to olr-vfs. Both olr-import (layer 3) and olr-content
  (layer 4) can then use them.

**Layered content directories.**
- `Content::open_layers(&[user packs, bundled])` (`crates/olr-content/src/lib.rs:100-185`) stays.
- The shell's `roots()` layers `<data>/packs` over `--content` (`crates/olr-game/src/shell.rs:369-370`;
  `store.packs_dir()` `crates/olr-ui/src/store.rs:189-193`; set at `shell.rs:254`).
- With the Import screen gone, nothing in the game writes `<data>/packs`. Downloads go to the cache.
- Decision D7: keep the layering (a user can still `olr-cli import --out <data>/packs`) or drop it.
- olr-content's test `imported_packs_override_bundled` covers `open_layers` either way.

**Cargo after the move:**
- olr-content drops `olr-import-lgs/lxl/gfx`.
- Is `serde` still needed (`Serialize` on report types)? cache.rs and others use serde/ron, so keep it.
- `thiserror` stays (`cache.rs:59`).

---

## 3. The Import screen: complete removal list

The subagent's map was spot-checked here. No autoplay script (`fixtures/autoplay/{menus,online}.ron`,
`settings.ron`), e2e step (`xtask/src/e2e.rs`) or PNG golden (`fixtures/ui/` holds only the three HUD
PNGs) touches it.

### olr-ui: delete

- `crates/olr-ui/src/import.rs` (1-292): `LOCAL_REVISION` :27, `ImportResult` :29-37, `ImportProgress`
  :39-75, `clean_typed_path` :80, `resolve_typed_path` :93, `expand_typed_path` :110, `ImportJob`
  :133-238 (thread `"olx-import"` :151), `work`/`run` :241-291.
- `crates/olr-ui/src/screens/import.rs` (1-141).
- `crates/olr-ui/tests/import.rs` (tests `import_runs_off_the_ui_thread_and_reports` :38,
  `import_screen_flow` :136, `typed_paths_are_cleaned_and_resolved` :251,
  `a_long_report_lays_out_the_rows_in_view` :276).

### olr-ui: edit

- `src/lib.rs:10` `pub mod import;`, `:39` `ImportSettings` re-export.
- `src/state.rs`:
  - Docs :6 and :51-55.
  - `Screen::Import` :84-85; `Screen::MENUS` :101 (`[Screen; 7]` → 6) and :107.
  - `UiAction::{StartImport, CancelImport, ImportFinished}` :157-162; `AppCommand::{StartImport,
    CancelImport}` :260-266.
  - `importing` field :333-334, :356; `is_importing` :392-395.
  - `refused_while_importing` guards :462, :470-471, :928-930 and fn :807-814.
  - Match arms :606-617; `start_import` :790-805.
- `src/screens/mod.rs`:
  - :16 and :45.
  - `MenuData.{import_path, packs_dir, import}` :87-93, inits :164-167. Keep `packs_dir` or give the
    shell its own copy (trap: `Shell::roots`).
  - :196 doc, :278-280, :317.
- `src/screens/main.rs:2, 34` (the "Import" button).
- `src/screens/local_play.rs:60-69` (Start disabled while importing), `:143` ("No mods: import OLX's
  content first." should point to `olr-cli import`, or to the editors later).
- `src/settings.rs`: `Settings::import` :35-36, `ImportSettings` :331-337.
  - Old settings files keep `import: (…)`, which serde ignores (no `deny_unknown_fields`). Precedent
    test: `crates/olr-ui/tests/store.rs:94-97`. Add one for `import:`.
- `src/store.rs:46-47, 189-193`: docs only, since `packs_dir()` stays for layering.
- `src/theme.rs:18`: doc.
- `tests/integration.rs:9`; `tests/flows.rs:118` ("Import" in the main-menu labels).
- `tests/layout.rs`: :3, `SCREENS` :87 (15 → 14) and :95; `title_of` :178; primary-action list :195.
- `tests/state.rs:22`: iterates `Screen::MENUS`. Check its expectations.
- `crates/olr-ui/CLAUDE.md`: 39-45, 54, 87, 149-156, 285-298, 365, 432-433.

### olr-game

- Delete `tests/import.rs` (`import_refreshes_the_menus_and_overrides_bundled` :81,
  `quitting_mid_import_leaves_whole_packs` :151). It is the only shell-level test of user packs
  layered over bundled ones.
- Edit `tests/integration.rs:11`.
- `src/shell.rs`:
  - Doc :24-29; import :74.
  - Field `import: Option<ImportJob>` :160-161, inits :274, :337.
  - `IMPORT_STOP_WAIT` :245-246.
  - `importing()` :395-398, `start_import` :400-417, `poll_import` :419-434.
  - `carry` arms :566-571; `poll` :588-590 and :603 (`let mut out = self.poll_import();` → `Vec::new()`).
  - `finish` :895-905 (stop the job on quit).
  - Keep :254 and `roots()` :369-370 if layering stays.
- `src/autoplay.rs`: `SCREENS` :50 (13 → 12) and :59 (`("Import", Screen::Import)`).
- `src/main.rs:196` (`--data-dir` doc "imported packs").
- `crates/olr-game/CLAUDE.md`: 76-83, 109 (the `olx-import` thread), 891-893.

### Docs

- `README.md`: 57-63 (the "Import in the main menu" bullet), 83, 116, 155.
- `docs/porting/STATUS.md`: :69 (the "Import screen" row), :670-689 (the Import screen deviation
  section), :821.
- `crates/olr-cli/CLAUDE.md:9-10`.
- `crates/olr-content/CLAUDE.md:113` ("the game's Import screen's").
- `crates/olr-content/src/import.rs:1-4`, `:122-125` (`prune` "off for the game's user packs/"),
  `crates/olr-content/src/lib.rs:105, 111`.
- `CHANGELOG.md`: add a "Removed" entry under Unreleased (:7). Leave 0.1's :588-589.

### Dependencies

- Nothing becomes unused.
- olr-game never depended on `olr-import-*` or a file dialog.
- olr-ui keeps `directories` (`store.rs:157`) and its dev-deps.

---

## 4. Load-time colour keying outside the importers

| Site | What it keys | Replacement once images carry alpha |
|---|---|---|
| `crates/olr-scene/src/assets.rs:64-69` `fn keyed` | — | delete |
| `crates/olr-scene/src/assets.rs:83` `sprite(...)` | core `crosshair`, `muzzle`, `hook`, `bonus`, `health` | `core.sprite(name).clone()` as is (format-5 core sprites are alpha: `compile_core` returns them as stored; `crates/olr-format/tests/core_pack.rs:37-40` proves `new == keyed(old)` and `keyed(new) == new` for the bundled core) |
| `crates/olr-scene/src/assets.rs:94-97` | mod projectile images `mod:{file}` | `image` as is; requires the mod importer to write alpha gfx (§2.2); exact for bundled mods (`crates/olr-format/tests/core_pack.rs:45-64`) |
| `crates/olr-scene/src/assets.rs:41-42, 128` docs ("colour-keyed at load", "already colour-keyed") | — | reword |
| `crates/olr-scene/src/assets.rs` tests `keying_clears_only_magenta`, `keying_follows_olx_corner_rule` (~:160-183) | — | delete |
| `crates/olr-fx/src/assets.rs:97-103` `fn keyed` | — | delete |
| `crates/olr-fx/src/assets.rs:112` `add(...)` | core `explosion`, `smoke`, `chemsmoke`, `spawn`, `sparkle`, default `gibs` | image as is |
| `crates/olr-fx/src/assets.rs:141` `add_worm` | `colorize_gibs(&giblets, colour)` → atlas | `atlas.add_image(.., image.clone(), 2)` without keying |
| `crates/olr-fx/src/assets.rs:13, 79-81` docs; `crates/olr-fx/CLAUDE.md` "Sprites are keyed with OLX's corner rule" | — | reword |
| `crates/olr-fx/src/assets.rs` test `keying_follows_olx_corner_rule` (~:226-239) | — | delete |
| `crates/olr-content/src/maps.rs:54` doc "already keyed" | — | already true; reword to "alpha" |

**Colorized gibs and the gib stain (the subtle part).**
- `colorize_gibs` (`crates/olr-fx/src/assets.rs:164-189`) skips pixels whose RGB is magenta or a gun
  grey. It tints the rest and keeps alpha.
- The stain path uses the colorized sheet unkeyed:
  - `Fx::simulate` → `color.stain_image(x, y, assets.gibs(worm).1, src)` (`crates/olr-fx/src/lib.rs:221-224`)
  - `ColorMap::stain_image` skips `same_rgb(MAGENTA)` and `a == 0` (`crates/olr-terrain/src/color.rs:246`)
- With an alpha `giblets`, key pixels are `(0,0,0,0)`. `colorize_gibs` maps them to `(0,0,0,0)`
  (`tint(0, ..) = 0`, alpha kept). `stain_image` skips them by `a == 0`.
- So nothing in olr-terrain or `colorize_gibs` needs to change.
- **The magenta checks may stay.** They are inert on alpha art, except for a real visible magenta pixel,
  which `lift_core` keeps for a 254-keyed sprite
  (`crates/olr-format/src/core_pack/mod.rs` test `lifting_keys_once_and_keeps_pure_magenta_of_a_254_keyed_sprite`).
  There OLX's skip semantics remain plausible. Leave them as they are and say so.
- Equivalence to check in a test: for the bundled `giblets` and a few colours,
  - `colorize_gibs(alpha)` == old `keyed(colorize_gibs(raw))` (atlas path);
  - the stained pixel set and colours are unchanged (stain path).
- These hold when the bundled giblets are keyed by pure magenta. If their corner is `(254,0,254)`,
  the old path tinted the key colour before keying and the two differ.

**Theme images.**
- `compile_theme` re-creates OLX's colour codes for holes and the green ball (`compile_stamp`). Stones
  and misc get magenta back (`compile_sprite`; `crates/olr-format/src/themes/mod.rs:257-273`). So
  `Stamp::from_image` (`crates/olr-terrain/src/stamp.rs:31-33, 87`), `GreenMask`,
  `default_dirt_colour` (`crates/olr-fx/src/assets.rs:197-211`) and `place_dirt` are unchanged.
- Stones and misc have no runtime reader (only `crates/olr-cli/tests/corpus.rs:112` counts them).

**Skins** are already keyed at import (mask `Transparent`, alpha 0). `SceneAssets::add_worm`
(`assets.rs:124-127`) never keyed them.

**olr-ui previews** never key:
- The skin preview uses `LoadedSkin::colorized` + `skin_sprite`.
- Thumbnails are decoded as they are.
- There is no other `key_transparent` outside olr-data, the importers and olr-format.

**`RgbaImage::key_transparent` / `olx_color_key`** (`crates/olr-data/src/image.rs:96-110`) stay:
- the importers use them (`crates/olr-import-gfx/src/skin.rs:41`);
- so does `olx_sprite_to_alpha` (`crates/olr-format/src/image_io.rs:232-236`).

**Which tests and goldens notice:**
- Unit tests that break by design (delete):
  - olr-scene `keying_clears_only_magenta`, `keying_follows_olx_corner_rule`;
  - olr-fx `keying_follows_olx_corner_rule`.
- `crates/olr-fx/tests/entities.rs:154` asserts `drawn_image.get(0, 3).a == 0, "magenta is transparent in
  the atlas"`. It uses the synthetic sheet `crates/olr-fx/tests/common/mod.rs:101-115`, whose column 0
  is magenta.
  - Make the fixture's column 0 `Rgba::TRANSPARENT` (an alpha sheet).
  - Adapt `entities.rs:163` (`!c.same_rgb(MAGENTA)` → `c.a != 0`).
- `crates/olr-fx/tests/common/mod.rs:63` and `tests/events.rs:58` make holes from solid magenta. These
  are theme stamps, still colour-coded: no change.
- Render goldens: `fixtures/render/*.png` via `crates/olr-scenario/tests/render_golden.rs` and
  `crates/olr-cli/tests/render_golden.rs` (GPU tier in `full`).
  - They must stay byte-identical. This is the main proof that dropping the keying is exact.
  - They need an adapter (lavapipe), so they run in `cargo xtask full`, not `check`.
  - **Run `full` (or at least the GPU render goldens) on the switch commit.**
- HUD snapshots (`fixtures/ui/hud-*.png`, olr-ui `hud_snapshots`, GPU): unaffected (no core sprites).
- olr-scene insta snapshots (draw-list text, `crates/olr-scene/tests/snapshots/*.snap`): describe atlas
  ids and rectangles, not pixels. Unchanged unless the atlas registration order changes.

---

## 5. Skin fetching (N31)

The subagent's map was spot-checked here: `setup.rs:264-283`, `PROTOCOL_VERSION`
`crates/olr-proto/src/lib.rs:33` = 2, `ItemKind::Skin` `crates/olr-proto/src/content.rs:33`.

### How a member's skin is chosen today

- **Lobby: fetching.** All in `crates/olr-online/src/content/peer_side.rs`.
  - A member offers its own skin as a revision-0 `List` (`client_look` :1012-1043).
  - The host accepts it (:491-513) and fetches it, with rate limits and budgets by address
    (`host_skins` :726-797, `ip_of` `crates/olr-lobby/src/host.rs:888`). "Local packs win" (`may_take`
    :58-74).
  - The host lays it over (:685-705 → `lay_over` :590-629) and lists one skin per id to clients
    (`host_list` :821-846).
  - Clients fetch it (`process_list` :1046-1090; `fetch.rs` sorts required first, :121) and lay it over
    (`poll_client` :979-990).
- **Match: drawing, by id only.**
  - `crates/olr-game/src/shell/online.rs:1030-1058` → `setup::load_online`
    (`crates/olr-game/src/setup.rs:264-283`).
  - Per slot: `content.skin_id(&look.skin)` + `skin_or_default(&id)` (`crates/olr-content/src/maps.rs:164-175`).
  - Then `Assets::skin_of` (`setup.rs:412-415`) → `session.rs:208-219` → `present.add_worm` (`session.rs:431`).
  - **`Look.skin_hash` is never read here.** A local skin with the same id but other content draws as the
    local one.

### Minimal correct removal

**olr-proto** (`crates/olr-proto/src/content.rs`):
- Delete `MAX_SKIN_BYTES` :24, `ItemKind::Skin` :33 (the last variant, so the others keep their
  numbers), and the skin clause of `valid_content` :81.
- `Item.required` :46: drop the field, or make `valid_content` refuse `!required`.
- `List`'s doc :50-54: no client-to-host lists.
- Tests :145, :168-169; `tests/fuzz_lobby.rs:131`.
- `MAX_ITEMS` 64 (:19) may shrink.
- CLAUDE.md "Content" bullet.
- **`Look { name, colour, skin, skin_hash }` (`crates/olr-proto/src/lobby.rs:93-100`) stays.**
  `skin_hash` becomes the match key. So do `valid_look` (:390-393) and `SlotWire.look`.

**olr-online** (`src/content/peer_side.rs`). Delete:
- `item_kind`'s skin arm :47-56.
- `may_take`/`may_list` :58-74 and their call sites :500, :598, :736, :842, :1064.
- `SkinSource`, `drop_skin_fetch`, `SkinFetch` :126-153.
- `HostContent.{skins, skin_started, skin_starts, fetched}` :165-176, :204-207.
- `ClientContent.{offer, offer_sent}` :250-253, :316-317.
- `host_changed`'s re-offer :376.
- The host arms of `receive` :491-518.
- `peer_left` :526-528 and `results` :578-584.
- `poll_host` :678, :685-705; `host_skins` :726-797.
- `host_list` skin loop and retains :821-864, :885-887.
- `client_look`'s offer :1012-1043, keeping the hash fill :1010-1011, :1029-1033.
- `process_list`'s optional filter :1059-1064.
- The client's `Server::new(SKIN_RATE)` :334 becomes `cfg.upload_rate`.

The rest of olr-online:
- `src/content/mod.rs:31-39` (`SKIN_*` constants), :50 (`ItemKind::Skin` arm), :54-58.
- `src/content/fetch.rs:2, 121` and its tests :526-547, :551-572, :575-592, :727.
- `src/peer.rs:643-647, 745-756`; `src/events.rs:70-73`.
- CLAUDE.md content section and :437.

**olr-lobby:**
- `host.rs:888` `ip_of` (its only caller is the skin-fetch budget).
- Comments at `host.rs:1146-1157`, `CLAUDE.md:73-74, 81-85`.
- `LOOK_CHANGES` stays (each change still republishes).

**olr-server:**
- `src/lib.rs:1043-1063` (docs and warning text), `src/config.rs:109-111`,
  `fixtures/server/example.ron:6, 57-59`.
- Tests `tests/server.rs:844-943` (`partial_content`'s skin parameter, `cached_skins`,
  `the_server_serves_content_and_keeps_skins_in_its_cache`, `a_server_without_its_cache_still_serves_its_packs`).

**olr-content** (`src/cache.rs:37-51`):
- `MAX_SKIN_ARCHIVE`, `SKIN_LIMITS` and `archive_limits`' skin arm (re-exported at `lib.rs:39-40`).
- Keeping them is harmless.
- Deleting them reworks `tests/cache.rs`, which uses skin packs with `SKIN_LIMITS` as its small test
  pack: :7, 76, 120-177, 494, 608. It also reworks `crates/olr-pack/tests/archive.rs:186-188` (the
  default skin archive ≤ 64 KiB).
- Either way, fix the `Keeper` doc (`cache.rs:632-636`).

**olr-game:**
- `src/net.rs:283-289` (`download_view` "skin" arm) and its test :625-641.
- CLAUDE.md :203-206, :372.

**olr-ui:**
- `src/online.rs:597-599, 622, 660` (docs).
- `tests/online.rs:738-760` (the "skin" download row, a string only).

**Docs:**
- `docs/porting/STATUS.md:1084-1092`, the N31 entry :1098-1117, :1124-1126, :1141, :1150.
- CHANGELOG Unreleased ("skins are no longer downloaded", spec §7).

**Version:**
- Bump `PROTOCOL_VERSION` 2 → 3 (`crates/olr-proto/src/lib.rs:33`; checked with `FINGERPRINT` in
  `crates/olr-net/src/core.rs:463-464`, echoed in announcements).
- `ItemKind`/`Item`/`List` semantics change. A 0.1 client would still offer skins, and a host's
  `List` with skin items would not decode.
- Not needed: `GAME_BUNDLE_VERSION`, replay `FORMAT_VERSION`, olr-cli `BUNDLE_VERSION`.

**Unaffected:** `fixtures/net/cases/*.ron`, `fixtures/autoplay/online.ron`, tools/net-nat scenarios, insta
snapshots, xtask.

### Where "id + hash → that skin, else default" lives

- Add an olr-content helper next to `skin_or_default` (`crates/olr-content/src/maps.rs:164`), e.g.
  `Content::skin_matching(id, hash) -> LoadedSkin`:
  - exact id (no `resolve_id`);
  - `pack_kind(id) == Some(Skin)`;
  - `has(id, hash)` (`lib.rs:287-290`; memoised hash);
  - then `load_skin(id)`, else `skin_or_default(DEFAULT_SKIN)`, with a warning.
- Call it from `setup::load_online` (`crates/olr-game/src/setup.rs:273-280`) for every slot except the
  local player's own. Today `open()` loads that one by id (:187-196). Its own skin needs no hash check.
- `Assets::skins` is keyed by look id (`setup.rs:172-175`). Two slots with the same id and different
  hashes collide, so key it by (id, hash) or by slot.
- Bots carry `NO_HASH` and wear `DEFAULT_SKIN` (`crates/olr-game/src/net.rs:387`,
  `crates/olr-server/src/rotation.rs:144`). Rule: `NO_HASH` → the named skin only if it is
  `DEFAULT_SKIN`, else the default (D9).
- **Hashing cost.** `Content::has` hashes synchronously on first use (a few KB for a skin), and
  `load_online` already runs off the peer lock. Optionally pre-warm `known_hash` on the content
  worker.
- olr-online, olr-server and olr-lobby make no skin decision any more.
- New tests:
  - in olr-game or olr-content: a match, a same-id different-hash skin, a missing id, `NO_HASH`;
  - in olr-online: the host ignores a client's `List` and lists only required items.

---

## 6. Content hashes and layouts pinned in fixtures

### Main finding: nothing committed pins a pack content hash

- A search for 64-hex strings across `crates`, `xtask`, `tools`, `fixtures`, `docs/porting` and
  `content/olx-manifest.ron` finds only three things:
  - the PNG encoder's golden bytes (`crates/olr-format/src/image_io.rs:283-312`
    `encoded_bytes_are_pinned`; independent of the format);
  - NIP-44 vectors;
  - a secp256k1 constant.
- No `.olrp` or `.olpk` file is committed. `determinism` records replays into `target/determinism` at
  run time.
- Net case file names end in an FNV hash of the spec text (`crates/olr-cli/src/netsim.rs:98-106`), not of
  content.
- Every test that uses content hashes computes them at run time from the packs it builds or copies:
  - `olr-content/tests/cache.rs:56-97`
  - `olr-online/tests/content.rs:123-133, 291, 334, 372`
  - `olr-replay/tests/content.rs:195-203` (`"0".repeat(64)` as a wrong hash)
  - `olr-server/tests/server.rs:1015`
- **So spec §1's "hash re-bless" (`formats-design.md:73-74, 479`) has nothing to re-record.**
  `bless-regression` would print "nothing to bless" (`xtask/src/bless_regression.rs:88-98`).

### What changes and how it is regenerated

| Artefact | Pins | Changes with format 5? | Command |
|---|---|---|---|
| `content/packs/**` (3564 files) and `content/packs/import-report.ron` | everything | **yes, all of it** | `cargo xtask fetch-olx-content` (`xtask/src/main.rs:54-59, 234-240` → `fetch::run` `fetch.rs:115-124`), then commit with deletions (`git add -A content/packs`). A leftover `mask.png` fails `check`'s `differences` as a `-` line |
| `fixtures/matches/*.ron` (3) | `Golden { spec, end, ticks, checksums }` (`crates/olr-cli/src/golden.rs:14-25`); `World::checksum` covers terrain and state, not mod, theme or hashes (`crates/olr-sim/src/checksum.rs:100-169`) | no, if compile is lossless (a change is a bug) | `cargo xtask bless-regression --reason` (`xtask/src/bless_regression.rs:53-100`) |
| `fixtures/net/cases/*.ron` (29), `fixtures/net/feel.ron` | specs and net metrics; net-sim moves no content | no | `bless-regression --from`, `net-feel --bless` |
| `fixtures/oracle/**` | scenarios by OLX names (`olx_mod_id`/`olx_level_id`), OLX traces | no | `oracle-record`; `oracle-gen-weapons` (`xtask/src/oracle_gen.rs:88-121`) reads `content/packs` via `olr_pack::read_mod` (:95), so it works unchanged if `read_mod` keeps its signature. Counts are pinned at `oracle_gen.rs:157-171` (38 + 50) and `crates/olr-oracle/tests/fixtures.rs:83-84` |
| `fixtures/render/*.png` (5), `fixtures/ui/*.png` (3) | pixels | **must not** change (13b already re-blessed the mirror quirk) | `cargo xtask bless-render --reason` (`xtask/src/bless.rs:21-25, 107-168`); GPU ones need an adapter |
| `fixtures/bench/{baselines,stress}.ron` | timings and peak MiB | load time or peak memory could move a little; not a pin | `cargo xtask bench --bless --reason` |
| `fixtures/autoplay/*.ron`, `fixtures/server/{e2e,example}.ron` | pack ids only | no (`example.ron:6, 57-59` comments about skin fetching go stale with N31) | hand edit |
| `fixtures/olx-mini/**` | importer input | no | `fixtures/olx-mini.py` |
| `tools/net-nat`, e2e, dist, winbuild | mount or copy `content/packs` | no | — |

### Insta snapshots

1. `crates/olr-cli/tests/snapshots/integration__corpus__imported_content.snap`, from `corpus.rs:25-136`.
   - It records counts and names from the **runtime** data:
     - Classic: 38 weapons / 58 projectiles / 39 assets;
     - MW: 50 / 302 / 129;
     - core: 11 sprite names including `giblets`;
     - the theme's "5 holes, 8 stones, 8 misc, green ball 8x9";
     - "skins: 718".
   - It is unchanged if still computed from compiled data. It changes if computed from sources
     (`giblet-stains`) or if mod assets are renamed.
   - The test's per-kind round trips stay valid with the runtime-level API (§1.3, except D3 and D4).
   - Re-record: `INSTA_UPDATE=always cargo test -p olr-cli --test integration corpus::` (`crates/olr-cli/CLAUDE.md:528-529`).
2. `crates/olr-format/tests/snapshots/integration__equivalence__*.snap` (7 files, canonical mod text).
   - After the switch they should equal the committed files' bytes. Point the test at `content/packs`
     directly.
   - Re-record: `INSTA_UPDATE=always cargo test -p olr-format --test integration equivalence::`.
3. `crates/olr-scene/tests/snapshots/frame_120@*.snap` (4) pin draw-list text, including asset keys
   (`mod:gfx/icesmoke.png`, `mod:gfx/missile5.png`, `hook`, `crosshair`, `skin:0 36,0 20x18`).
   - They change only if mod gfx paths or core sprite names change: see D14, the kebab-case asset
     names.
   - Re-record: `INSTA_UPDATE=always cargo test -p olr-scene --test integration snapshots::`.

### Tests that hard-code the format-4 layout (rewrite with the switch)

- **olr-pack unit tests:**
  - `src/mod_pack.rs:249-266` (`writes_expected_layout`: `projectiles/bits.ron`, `projectiles/rocket.ron`,
    …; format 5 inlines single-use projectiles), `:307-315` (an error position in `weapons/rocket.ron`);
  - `src/skin_pack.rs:15, 85-98` (`mask.png`, `Vec<MaskKind>` round trip);
  - `src/theme_pack.rs:15, 88-111` (`holes/2.png`, `green_ball.png`; format 5 adds `.mask.png`s);
  - `src/level_pack.rs:293-306` (material palette edge cases);
  - `src/core_pack.rs:148-153, 180-196`.
  - `ron_io.rs:78-88` is version-agnostic. `tests/archive.rs:10` is only stale text.
- **olr-cli** `tests/cli.rs:52-80` (`import_then_validate`): file existence only (`olx-core/gfx/crosshair.png`,
  `core.ron` contents). Its all-magenta source crosshair (:43) becomes a fully transparent alpha image.
  That is fine, but say so.
- **olr-content:**
  - `tests/content.rs:250-274` (overwrites `gfx/hook.png` and `core.ron` of a written core pack; expects
    warnings naming them);
  - `:350-390` (`loads_every_manifest_item`: core stems → `has_sprite`; 5 holes; every skin loads
    warning-free);
  - `tests/import.rs:240-264` (bombs written into `olx-skin-default/skin.png`, `olx-tiny-mod/gfx/bomb.png`);
  - `tests/catalog.rs:68-71, 134-140`.
- **Size-dependent:** `crates/olr-online/tests/content.rs:307` asserts the Dirt Level archive is above
  `3 * CONTENT_CHUNK` (98 304 B).
  - Today it is 130 635 B (front 72 500, back 32 492, thumb 21 944, material 3 249).
  - Level PNGs should be byte-identical in format 5 (same encoder and palette), but a re-encode would
    eat into a 32 KB margin.
- **Importers:**
  - `crates/olr-import-gfx/tests/olx_gfx.rs:12-39` (exactly 409 skins from the checkout, write/read round
    trip), `:44-65` (thumbnails from `content/packs` via `read_level`).
- **N31-related:**
  - `crates/olr-online/tests/common/mod.rs:64-79` and `peer_side.rs:1350-1365` flip a `skin.png` pixel to
    make another hash.
  - The `crates/olr-server/tests/server.rs:846-858` comment claims a re-id'd copy has "another content
    hash", but `content_hash` leaves out `pack.ron`, so it is the same hash. That is a stale comment in
    a test N31's removal rewrites anyway.
- **No test counts the files or bytes of `content/packs` as a whole.**

---

## 7. `PACK_FORMAT_VERSION`, archives and downloaded packs

**Definition and checks:**
- Defined at `crates/olr-data/src/manifest.rs:15` (`= 4`).
- Checked only in `olr_pack::read_manifest` (`crates/olr-pack/src/ron_io.rs:53-64`): "unsupported
  format_version N (this build reads 4; re-import the pack)". Test `an_older_pack_format_is_refused_with_a_re_import_hint`
  (`ron_io.rs:75-87`).
- Written by the importers: `crates/olr-import-lgs/src/convert.rs:40`, `crates/olr-import-lxl/src/lib.rs:57`,
  `crates/olr-import-gfx/src/lib.rs:47`.
- Also by `crates/olr-content/src/core_assets.rs:90` (the empty core) and by test manifests via the
  constant (olr-content, olr-replay, olr-online tests).
- Hard-coded as text:
  - `crates/olr-pack/tests/archive.rs:10` (`format_version: 4`): not read, harmless;
  - `crates/olr-content/tests/content.rs:178` (`99`);
  - `crates/olr-content/tests/import.rs:208` (replaces the constant with 999).
- Bump in the commit that first writes format-5 files. olr-pack CLAUDE.md "Bump PACK_FORMAT_VERSION"
  bullet: add "version 5: the authoring formats (plan 13c)".

**`.olpk` archive** (`crates/olr-pack/src/archive.rs`):
- Layout-agnostic: a sorted list of path and bytes.
- Format 5 changes nothing in `encode`/`decode`. What changes is what's inside:
  - more, smaller files per mod (`weapons/<id>.ron` + `projectiles/<id>.ron`, as today);
  - theme `holes/<n>.mask.png`, `green_ball.mask.png`;
  - skin `tint.png` instead of `mask.png`;
  - core `gfx/giblet-stains.png`.
- `MAX_ARCHIVE_FILES` 4096, `MAX_DEPTH` 8 and `MAX_DIRS` 512 still fit:
  - the largest mod (`olx-mw-1-0`) has fewer than 300 files today;
  - the deepest path is `holes/<n>.mask.png`, depth 2.
- Check one bound concretely: the theme with 2 × 5 hole files + green ball + mask + tiles + stones/misc
  is well under all limits.

**Download cache** (`crates/olr-content/src/cache.rs`):
- `install` → `PendingInstall::verify` (:677-720):
  - `archive::decode` within `archive_limits(kind)` (:46-51);
  - `content_hash` (:702), compared with the list's hash;
  - `read_manifest` (:706): id and kind;
  - `validate` errors (:714).
- `read_manifest`'s version check refuses a format-4 pack sent by an old build: "re-import". Old and
  new builds are also kept apart by `PROTOCOL_VERSION` and `FINGERPRINT`.
- **Old caches clean themselves up.**
  - `DownloadCache::open` adopts only packs whose `read_manifest` succeeds (`:269`), so format-4
    entries are dropped from the index. They are then removed or evicted by reconciliation; verify
    that `open`'s reconcile deletes unlisted directories rather than leaving them.
  - `lookup` (`:328`) returns `None` for them, so old replays referencing cached packs fail cleanly.
- `SKIN_LIMITS` (64 KiB, 16 files, depth 2): format-5 skins (`pack.ron`, `skin.ron`, `skin.png`
  140 × 54, `tint.png`) fit. They become moot with N31's removal (§5).

**`PRESENTATION_FILES`** (`crates/olr-pack/src/hash.rs:9`) is `[THUMB_FILE]`.
- Format 5 keeps `thumb.png` at the level root, with the same name: olr-format's `THUMB_FILE` is
  `crates/olr-format/src/levels/mod.rs:29`.
- So hashing still leaves it out.
- Make olr-pack's `THUMB_FILE` and olr-format's one constant. Today they are two literals
  (`level_pack.rs:23`, `levels/mod.rs:29`).
- Reading the thumbnail stays olr-pack's: `read_level_thumb`/`decode_thumb`, with
  `MAX_THUMB_BYTES` 1 MiB and `MAX_THUMB_SIDE` 512. olr-content's `ThumbRef::probe`/`load` also stay.
- olr-format's `LevelSource.thumb` (bytes) is for editors, which preserve it on save.
- The 13b carry-forward: `read_level_files` must not fail a level over a thumbnail (§1.3). In 13c only
  the importer and the loaders need this. The importer generates 160 × 96 thumbnails
  (`olr-import-gfx/src/thumb.rs:40-43`, `import.rs:337`).

---

## 8. Validation today vs olr-format's checks

**olr-pack `validate`** (`crates/olr-pack/src/validate.rs:73-104`), never panics:
- **Errors:**
  - an unreadable manifest or wrong version;
  - an id that is not a lowercase slug (:79-87);
  - any `read_x` error (RON with `file:line:col` via `Diagnostic::from_error` :34-50, image, invalid
    value, I/O);
  - for mods (`check_mod` :106-162): a weapon listed twice, an unknown projectile referenced by a weapon
    or projectile, a missing asset (gfx or sfx named by the defs but absent), a projectile spawn cycle
    (`find_cycle` :167+, iterative).
- **Warnings:**
  - a projectile never used;
  - an asset never used;
  - a bad level thumbnail (:93-96).
- `Diagnostic { severity, file, line, col, message }` (:21-27), `Display` "error: file:l:c: msg".

**olr-format's checks** (`FormatError` with file + line/col or field path; olr-format CLAUDE.md):
- Parse errors with positions; `deny_unknown_fields`.
- Mods:
  - invalid or duplicate ids, a weapon file not listed, a shared projectile outside `projectiles/`;
  - the LX56 limits (effects per event, spawns), cycles, unknown spawn names;
  - bounds `MAX_INLINE_DEPTH`/`MAX_WEAPONS`/`MAX_SHARED`/`MAX_EFFECTS`/`MAX_COLOURS`/`MAX_AMOUNT`.
- Levels: size, art scale and objects bounds, material index, art size mismatch.
- Skins: layout, partial tint, `left.png` unsupported, sizes.
- Themes: colours that change meaning, stamp sizes, counts.
- Core: counts, names, giblet grid.
- Every `check()` applies its reader's pixel bound.

**Not covered by olr-format:**
- the manifest (id slug, kind, version): olr-pack's;
- **mod assets** (missing gfx or sfx, unused assets): assets are olr-pack's;
- unused shared projectiles (a warning; `lift_mod` shares a projectile referenced zero times, so they
  survive as `ModDef.projectiles` keys);
- the thumbnail warning.

**Proposed 13c `validate`:**
- `read_manifest` → slug check → the kind's runtime read (now olr-format parse + compile, errors
  mapped to `Diagnostic`) → for mods, today's `check_mod` on the compiled `ModDef` and the assets.
- That keeps unknown-reference, missing-asset and unused warnings working unchanged. Cycles are caught
  earlier by `compile_mod`, but keeping `find_cycle` is harmless.
- Map `FormatError` to `Diagnostic`:
  - `Ron`-like parse errors carry line and col;
  - field-path errors go in the message (`"weapons/bazooka.ron: fire.shot.projectile: …"`).
- `PackError` gains a `Format(FormatError)` variant, or a mapping. `Diagnostic::from_error`
  (`validate.rs:34-50`) gets one more arm.

**Users of `validate` that 13c must keep working:**
- the download cache install (`cache.rs:714`, errors only);
- `olr-cli validate` (`crates/olr-cli/src/validate.rs`);
- the corpus test (`crates/olr-cli/tests/corpus.rs:38-40`);
- the importer tests (`olr-import-lgs/src/convert.rs:314`, `tests/olx_mods.rs:27`,
  `olr-import-lxl/tests/olx_levels.rs:30`, `olr-import-gfx/tests/olx_gfx.rs:38`), which assert
  `validate == []` (no warnings either).

**Out of 13c** (13d): `olr-cli pack check|fmt|new`, the level operations, generated schema docs, and
`check` running `pack fmt --check`.
- `olr-cli validate` can stay as it is, renamed or aliased to `pack check` in 13d.

---

## 9. The skin importer "deviation" (`crates/olr-import-gfx/src/skin.rs`)

**The claim.**
- `docs/porting/STATUS.md:82-84` and `docs/porting/olx-notes/asset-formats.md:224-226`: OLX's
  `Colorize_Execute` draws a key-coloured image pixel under a tint mask colorized, while the importer
  hides it.
- `crates/olr-import-gfx/src/skin.rs:54-56`:
  `if px.same_rgb(key) || px.a == 0 || m.a == 0 { kind = MaskKind::Transparent; }`.
- 69 source skins have such pixels (e.g. `CT Cat.png`, 49).

**What OLX at d1ad953f actually does.**
1. `CGameSkin::Change` loads the sheet with `LoadGameImage("skins/" + file, true)`, i.e. with alpha
   (`target/olx-src/d1ad953f/src/client/CGameSkin.cpp:271, 277`).
   - With `withalpha`, the image is converted to an alpha surface (`GfxPrimitives.cpp:2958-2990`,
     `gfxCreateSurfaceAlpha`).
   - An alpha surface has `SDL_BLENDMODE_BLEND` by default (`GfxPrimitives.cpp:305`).
2. Then `SetColorKey(bmpSurface)` (`CGameSkin.cpp:281`, and again on a re-cut non-standard sheet at
   :288). On a surface with a blend mode it calls `SetColorKey_Alpha(dst, key)`
   (`GfxPrimitives.cpp:136-141`).
   - That replaces every pixel whose RGB equals the key with `MapRGBA(key, 0)`, i.e. alpha 0
     (`GfxPrimitives.cpp:88-110`).
   - This covers the image half and the mask half of the sheet, with the corner rule picking magenta or
     `(254,0,254)` (:122-133).
3. `Colorize_Execute` (`CGameSkin.cpp:722-790`) reads `pixel` raw:
   - mask black → copies `pixel`, alpha 0 included;
   - `IsTransparent(mask)` → skips;
   - else `Color colorized(fmt, pixel)`, which is `SDL_GetRGBA` and so **keeps alpha**
     (`include/Color.h:98`); writes `colorized.get(fmt)` = `SDL_MapRGBA(.., a)` (`Color.h:108`).
4. So a key-coloured image pixel under a tint mask is written with **alpha 0**, and OLX draws nothing
   there.

**Conclusion.**
- The importer's current behaviour (hide it) **matches OLX**. The deviation entry is wrong: the 13b
  research missed `SetColorKey_Alpha`.
- **Recommended 13c action:** retract the STATUS entry (and the olx-notes paragraph) with a short
  explanation and these citations, and change no code.
- Pin it with an importer unit test: an image pixel = key under a white mask → `Transparent`. The
  existing test at `skin.rs:~93-95` (x = 3 magenta with a colorize mask, "still transparent") already
  covers it. Re-word its comment to cite `SetColorKey_Alpha`.

**Smaller real differences found while reading** (all edge cases, for the plan to note or ignore):
- **Partial image alpha.** OLX keeps a pixel's partial alpha through colorize and copy. The importer
  forces `a = 255` (`skin.rs:61`). The 13b notes already record "partial alpha is dropped", and
  format 5's `compile_skin` cannot express partial tint anyway.
- **Mask alpha.**
  - OLX: a **black** mask pixel is checked first by RGB only (`EqualRGB(mask, black)`), so a black mask
    with alpha 0 still copies the pixel. The importer turns it Transparent (`m.a == 0`, `skin.rs:55`).
  - OLX: any non-opaque, non-black mask is transparent (`IsTransparent`: `Amask` not full,
    `GfxPrimitives.h:728-735`). The importer requires `m.a == 0`.
  - Whether any bundled skin has a non-opaque mask pixel is unmeasured. A one-off scan over the 718
    sources would settle it.
- **Re-cut sheets.** OLX keys twice on a non-standard sheet (the original corners, then the re-cut
  surface's), so the union of both keys hides. The importer uses only the uncropped sheet's corners
  (`skin.rs:41`).
- **Width.** OLX keeps widths that are multiples of 32 beyond 672 (`CGameSkin.cpp:285`). The importer
  crops to 672, which only drops frames OLX never draws.

**If the user nevertheless wants the "colorize" behaviour** (a new deviation from OLX, not a fix):
- In `skin.rs:54-56`, drop `px.same_rgb(key)` from the force-transparent test when `kind ==
  MaskKind::Colorize`, and set `a = 255`.
- In format 5 such a pixel is `skin.png` RGB = key colour, alpha 255, `tint.png` = 255.
- `compile_skin` accepts it: there is no magenta check for skins (`crates/olr-format/src/skins/mod.rs:120-170`).
- `colorize_pixel` turns an exact-magenta *result* into `(240,0,240)`
  (`crates/olr-data/src/skin.rs:80-95`). A magenta source tinted by white `(255,255,255)` gives
  `(255,0,255)` → `(240,0,240)`, never invisible.
- Under a Keep mask, a key pixel would then be visible magenta: decide that case too.

---

## 10. Risks and ordering constraints

1. **Alpha packs and keying loaders must change in one commit** (13b ledger, Task 1 carry-forward).
   - Keying an alpha image whose corners are `(0,0,0,0)` falls back to the magenta key and clears real
     magenta.
   - Not keying a raw OLX image shows magenta.
   - So the commit that makes core sprites and mod gfx alpha also deletes `keyed()` in olr-scene and
     olr-fx and re-imports `content/packs`.
2. **Every importer or format change must carry its re-import.**
   - `check`'s `olx-content` step fails on any difference between `content/packs` and a fresh import
     (`xtask/src/fetch.rs:129-148`).
   - There is no step where "packs are new but loaders old", or the reverse, that stays green.
   - The `--offline` check would pass but hide it, so the plan must run the online `check` for those
     steps.
3. **The content diff is large but mechanical.**
   - 21 MB, 3564 files; 718 skins dominate.
   - Every skin pack changes (`mask.png` → `tint.png`, new `skin.png` layout 140 × 54, `skin.ron`),
     plus the 4 levels (`level.ron` only; PNG bytes should be identical, same palette and encoder),
     the 2 mods, the theme, the core and `import-report.ron`.
   - Review the code diff and spot-check files. Expect a big git object churn.
4. **Splitting by pack kind is possible.**
   - Each step switches one kind's olr-pack read and write plus its importer and re-imports.
   - Order: levels → themes → skins → mods → core.
   - The core sprites and mod gfx steps carry the keying removal, so do mods and core in one commit.
   - Bump `PACK_FORMAT_VERSION` in the first step. Intermediate commits then never call a mixed
     layout "4", and no build ever ships mid-switch.
5. **The old-path equivalence tests stop meaning anything after the switch.**
   - `crates/olr-format/tests/{equivalence,levels,themes,skins,core_pack}.rs` compare `lift(read_old(packs))`
     with `compile(...)`.
   - Once olr-pack reads format 5 through olr-format, they become tautologies, or fail (core: `keyed(new)`
     comparisons still hold).
   - Spec §6: "Compared against the old path during the switch, then pinned as committed snapshots".
   - **So before the switch, add a runtime digest snapshot.** Per bundled pack, a hash of what loaders
     hand on:
     - `SimMod` (RON or Debug text) + `ModDef` RON + images **after today's keying** + sound bytes;
     - level material, front and back;
     - theme holes, green ball and tiles;
     - each skin's `colorize(c)` for a few colours (not the `Skin` struct: transparent RGB differs);
     - core sprites after keying + sounds + colours.
   - Commit it from the old path. Every switch step must leave it byte-identical. It replaces those
     tests' role, and the mod text snapshots (`crates/olr-format/tests/snapshots/`) stay as they
     are.
   - Then rewrite the olr-format tests to read `content/packs` directly as format 5
     (`read_*_files` → write → same bytes: the canonical round trip of spec §6).
6. **Importer round-trip tests compare runtime structs** (`read_x(write_x(imported)) == imported`):
   `olr-import-gfx/tests/olx_gfx.rs:26-27, 37-38, 57`, `olr-import-gfx/src/core_pack.rs:177-178`,
   `olr-import-lgs/src/convert.rs:302-315`, `olr-import-lgs/tests/olx_mods.rs:25-29`,
   `olr-import-lxl/tests/olx_levels.rs:29-31`.
   - These hold only if lift∘compile is the identity on importer output:
     - true for levels, themes and bundled mods;
     - **false for skins** (RGB under alpha 0) and **for core** (raw vs alpha).
   - Normalise the importer output, or compare drawn or colorized output (D3, D4).
7. **Synthetic `ModDef`s in tests may not lift** (`lift_mod` refuses non-LX56 structure):
   `olr-content/tests/content.rs:45-101` `broken_mod`, `olr-replay/tests/content.rs:45-81`
   `mod_pack`. Run them early in the mods step.
8. **Golden re-blessing.**
   - Nothing committed pins a content hash, and no replay is committed (§6). The spec's "hash re-bless"
     is a no-op. The plan should say so rather than run `bless-regression` for show.
   - Match goldens pin checksum streams, which must not change.
   - Render goldens (`cargo xtask bless-render`) must **not** need a re-bless. If they do, the switch
     changed pixels, which is a bug.
   - The oracle (`fixtures/oracle`) reads packs by id through `Content::sim_mod`, and must not change.
   - The corpus snapshot should not change: counts and lists only, and `CoreDef.sprites` is
     unchanged because `compile_core` re-composes `giblets`. Verify; if it changes, accept it with
     `INSTA_UPDATE=always` after reading the `.snap.new`.
9. **Removal ordering.**
   - The Import screen must go **before** olr-content drops the importers (the screen calls
     `olr_content::import`).
   - Moving the pipeline into `olr-import` must happen **before or together with** olr-content dropping
     the importer dependencies. olr-cli switches to `olr_import::import`.
   - The move needs `OLD_PREFIX`/`newest_aside_first` moved down first.
   - Skin fetching is independent of the format switch. It can go first or last; first shrinks the
     cache and skin-limit noise in later steps.
   - Keep `PROTOCOL_VERSION` 3 in that step.
10. **The `write_mod_files` no-delete hazard.**
    - The importer writes into a fresh hidden directory (`.import-<id>`, `import.rs:236-241`), so it
      is safe.
    - Only an in-place save (13d, editors) must clear files first.
11. **`pack.ron` stays olr-pack's non-canonical RON** (`crates/olr-pack/src/ron_io.rs`). 13d's
    `pack fmt` may want it canonical too (D10).
12. **Determinism of PNG bytes.**
    - Re-encoded mod gfx and core sprites come out of `image_io::encode_rgba`. png is pinned exactly,
      the compressor crates are not.
    - The golden-bytes test from 13b's final ruling guards this, and so does `check`'s fresh import.
13. **No release mid-switch.** The `full` gate (GPU render goldens, bot grid, e2e, net-nat) should run
    at the end of the switch, and ideally on the keying-removal commit (MEMORY: "full once at plan end";
    the render goldens are the only pixel-exact check of risk 1).

---

## Proposed task order for plan 13c

Each step leaves `cargo xtask check` green (online, so `olx-content` runs, from step 3 on).

1. **Pin the runtime digest (old path).**
   - Add a test, e.g. `crates/olr-content/tests/digest.rs` or in olr-cli's corpus, with an insta
     snapshot of per-pack hashes of the runtime data as loaders and renderers see it today (§10.5).
   - Also add the gib equivalence check (§4): the bundled `giblets`, old `keyed(colorize(raw))`
     versus `colorize(keyed(raw))`.
   - No behaviour change.
2. **Remove skin fetching (N31)** and add the id + hash rule (§5).
   - The olr-content helper; `setup::load_online`; the proto changes; `PROTOCOL_VERSION` 3; tests;
     STATUS, CHANGELOG.
   - Optionally drop `SKIN_LIMITS`/`MAX_SKIN_ARCHIVE` and rework `tests/cache.rs`.
   - Independent of formats.
3. **Remove the Import screen** (§3).
   - olr-ui, olr-game, docs.
   - Decide `packs_dir` and layering (D7).
4. **Move the pipeline to `olr-import`** (§2.4).
   - Move `OLD_PREFIX`/`TMP_PREFIX`/`newest_aside_first` to olr-pack.
   - New crate in `layers.ron` with its CLAUDE.md.
   - Move `import.rs` + `tests/import.rs`; olr-cli uses it.
   - olr-content drops `olr-import-*`; docs.
   - Pure move: `content/packs` is unchanged, `check`'s fresh import is byte-identical.
5. **olr-format gaps for the loaders** (no format change yet):
   - `read_level_header`;
   - a thumbnail-free `read_level_files` variant, plus the thumbnail bound and skip;
   - a tolerant `read_core_files`;
   - a non-keying core lift path (D3);
   - one shared `THUMB_FILE`;
   - `FormatError` → `PackError`/`Diagnostic` mapping.
   - Unit tests.
6. **Switch levels and themes** (`PACK_FORMAT_VERSION` → 5 here).
   - olr-pack `read_level`/`read_level_def`/`write_level` and `read_theme`/`write_theme` over olr-format.
   - Re-import (`cargo xtask fetch-olx-content`). The digest is unchanged; the render goldens are
     unaffected.
   - Rewrite `olr-format/tests/{levels,themes}.rs` to the format-5 canonical round trip.
7. **Switch skins.**
   - olr-pack `read_skin`/`write_skin` over olr-format.
   - The skin importer normalises transparent pixels (D4).
   - Retract the STATUS "deviation" (§9), or apply the user's choice.
   - Re-import (718 packs). Digest unchanged (colorized output).
   - Update `olr-import-gfx/tests/olx_gfx.rs`, `olr-format/tests/skins.rs`.
8. **Switch mods and core, and drop load-time keying (one commit).**
   - olr-pack `read_mod`/`read_preset_names`/`write_mod`/`read_core(_tolerant)`/`write_core` over
     olr-format.
   - The mod importer writes alpha gfx; the core importer or lift keys once (D3).
   - Delete `keyed()` in olr-scene and olr-fx and their unit tests.
   - Fix `olr-fx/tests/common/mod.rs` and `entities.rs:154,163`.
   - Re-import. The digest is unchanged: its "after keying" images now equal the raw ones.
   - Rewrite `olr-format/tests/{equivalence,core_pack}.rs` as format-5 round trips. Keep the mod
     snapshots.
   - Run the GPU render goldens (or `full`) here.
9. **Validation over the new readers** (§8).
   - `validate` maps `FormatError`; keeps `check_mod` (assets, unused); the importer tests'
     `validate == []` still hold.
   - `olr-cli validate` is unchanged. Could fold into steps 6-8.
10. **Hash pins: nothing to re-bless** (§6).
    - Confirm `bless-regression` reports "nothing to bless". Note in the plan and STATUS that content
      hashes changed, so old replays and caches are refused or dropped.
    - Re-record the insta snapshots only where §6 says they move.
11. **Records.**
    - `docs/porting/STATUS.md` rows 77-78 ("not yet the packs' format") → done.
    - CHANGELOG Unreleased: packs need re-importing, skins are no longer downloaded, the Import screen
      was removed.
    - CLAUDE.md files of olr-pack (layouts, version 5), olr-content, olr-format (13c done; remove
      "nothing applies it at load once plan 13c switches" wording at `image_io.rs:227-229`), olr-scene,
      olr-fx, olr-online, olr-ui, olr-game, olr-cli.
    - Then `cargo xtask full` once.

Steps 2, 3 and 4 are independent of each other and of 5. Steps 6-8 depend on 5 (and step 8 on the
importer changes). Step 4 could come after 8 if preferred, but then the importer edits of 6-8 land in
olr-content first and move afterwards.

## Decisions the plan must make

- **D1. olr-pack's API shape.** Recommended: keep the runtime-level `read_x`/`write_x` and pack structs,
  re-implemented over olr-format. Add authoring-level `*Source` read and write now or in 13d?
- **D2. Importer output.** Runtime structs written through `write_x` (option A, minimal), or olr-format
  sources directly (option B, needs `same_layer` `(olr-import-*, olr-format)`).
- **D3. Core keying.**
  - `lift_core` keys (`core_pack/mod.rs:222`). Who keys? (a) the importer keys and `lift_core` becomes
    structural, so `read_core(write_core(p)) == p`; or (b) `write_core` takes raw OLX sprites and
    `read_core` returns alpha (asymmetric; the corpus and importer round trips change).
  - Recommended: (a).
  - The same question for mod gfx: the importer keys (recommended), since olr-format has no mod-asset
    code.
- **D4. Skin normalisation.** The importer emits `Rgba::TRANSPARENT` for hidden pixels, so round trips
  compare equal (recommended). Or the tests compare colorized sheets.
- **D5. The skin "deviation".** Retract it (OLX hides those pixels, §9) and change no code
  (recommended). Or adopt colorizing as a deliberate new deviation. Also: scan the sources for
  non-opaque mask pixels and decide whether to match OLX's black-first and any-partial-alpha rules.
- **D6. Where the import pipeline lives.** A new `olr-import` crate at layer 3 (recommended) vs an
  olr-cli module. Keep `discover`/`discover_until` for the editors or drop them. Where
  `OLD_PREFIX`/`newest_aside_first` go (olr-pack recommended).
- **D7. User `packs/` layering.** Keep `<data>/packs` over `--content` (no writer left in the game, but
  `olr-cli import --out` can fill it) or drop it with the screen. Where `packs_dir` lives once
  `MenuData` loses the import fields.
- **D8. The thumbnail in the level format.**
  - How olr-pack's `read_level` skips it (an olr-format flag, a filtering source, or the thumbnail
    leaves `LevelSource`).
  - olr-format's thumbnail bound and skip policy (1 MiB, skip a bad one, as olr-pack).
- **D9. The skin match rule's edges.** The local player's own slot (by id, no hash). `NO_HASH` looks
  (bots: the default skin only). `Assets::skins` keyed by (id, hash). Whether to drop
  `SKIN_LIMITS`/`MAX_SKIN_ARCHIVE` and the optional `Item.required` field.
- **D10. `pack.ron`.** Stays olr-pack RON as today, or moves to canonical text (could wait for 13d's
  `fmt`).
- **D11. Splitting the switch.** Per kind (levels+themes, skins, mods+core) with the version bump at the
  first, vs one big commit. And whether `full` runs on the keying commit as well as at plan end.
- **D12. The tolerant core read.** Keep placeholders per bad sprite or sound (a tolerant olr-format read)
  or fail the core pack.
- **D13. Whether `olr-cli validate` is renamed now or in 13d** (`pack check`). 13c needs only the
  validator behind the loaders, cache and importer.
- **D14. Asset names.**
  - Spec §5 says "OLX file and folder names → lowercase kebab-case ids". Mod gfx and sfx keep OLX's
    lowercased names today (e.g. Classic's `cluster_bits.png`; `crates/olr-import-lgs/src/convert.rs:175-181`).
  - Renaming in 13c changes `ProjectileGfx::Image.file` strings. That is harmless to the sim (frame
    counts by path) but changes the olr-scene draw-list snapshots and the corpus asset text.
  - Decide whether 13c renames assets or defers.
- **D15. The spec's "hash re-bless" step.** Nothing pins a hash (§6). Record it as a no-op, or keep a
  token `bless-regression` run in the plan.
