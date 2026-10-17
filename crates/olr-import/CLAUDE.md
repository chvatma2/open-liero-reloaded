# olr-import

Imports an OLX game directory into native content packs: the pipeline
`olr-cli import` runs (and so `cargo xtask fetch-olx-content`). The game, the
server and the UI do not link it (`xtask/layers.ron` `forbidden`, spec §5).

- **Layer 3, uses `std`.** Depends on `olr-data`, `olr-vfs`, `olr-pack`,
  the importers (`olr-import-lgs`, `olr-import-lxl`, `olr-import-gfx`),
  `anyhow`, `thiserror`, `serde` and `ron`. `olr-content`, `olr-sim`,
  `olr-modes` and `olr-bots` are dev-dependencies only (the tests open imported
  packs and play a match on them).
- **The prefixes** `.import-` and `.replaced-` and `newest_aside_first` live
  in `olr_pack::install`, shared with `olr-content`'s `open_layers`.
- **Never panic on content**; output is deterministic.
- **Import** (`src/lib.rs`, plan 7 ruling Q3; moved from `olr-content` in plan 13c): the one OLX import
  pipeline, `olr-cli import`'s.
  `import(olx, &ImportPlan, out, &ImportOptions, progress)` imports the
  plan's mods, levels (with their thumbnail), themes, core files and
  skins one by one (`Progress::Item`/`Done`; the callback returning
  `false` cancels between items), writes each pack hidden
  (`.import-<id>`) and renames it into place (the old one renamed aside
  to `.replaced-<id>`, then removed: a failure is that item's warning; a
  `.replaced-<id>` without its `<id>`, from an import killed between the
  renames, is listed by `open_layers` and put back by the next import,
  the newest of an id's (`.replaced-<id>~k` too: `olr_pack::install::newest_aside_first`, by
  the manifest's time, then the highest `k`);
  other leftovers removed, or `ImportReport::notes`), symlinked mod,
  level, skin, data and theme directories followed one level, ids
  colliding in a run get `-2`, ..., and writes `import-report.ron`
  (`ImportEntry`: source, pack, error, warnings; byte-identical to the
  CLI's before the move). `ImportPlan::extra_skins` (`SkinSource`: a
  directory of skin PNGs with its own origin source and revision) adds
  skins from outside the game directory in the same run (the manifest's
  `extra_skins`: OLX 0.58 rc5's release skins). `prune` (the CLI's) removes packs of earlier
  runs. Every file is read at most `MAX_SOURCE_BYTES`, every image
  decoded within `olr_pack::image_io`'s bounds. A failed item is in
  the report; a failed write (permission, full disk) is the import's
  error. `discover(path)` lists what an OLX game directory (or its
  `share/gamedir`) holds where OLX's menus look (directories with a
  `script.lgs`, `levels/*.lxl`, `data/themes/*`, `skins/`; the core pack
  only when all of `OLX_CORE_FILES` are there), case-insensitively, with
  `notes` for what it skips; not an OLX directory is an error;
  `discover_until(path, stop)` stops with `Cancelled`.
  `tests/import.rs` (with the layered `Content::open_layers` tests, so `olr-content` is a dev-dependency) runs on `fixtures/olx-mini` (made by
  `fixtures/olx-mini.py`).

Test: `cargo nextest run -p olr-import`.
