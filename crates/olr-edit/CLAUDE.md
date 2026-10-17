# olr-edit

The pack authoring operations `olr-cli` and the editors share (plan 13d;
spec `docs/superpowers/specs/2026-10-16-formats-design.md` §6): a pack's
authoring source loaded and saved whole, the canonical formatter, pack
templates and the level operations.

- **Layer 3, `std`.** Depends on `olr-data`, `olr-vfs`, `olr-format`,
  `olr-pack`, `thiserror`. It is not a forbidden importer: the game may
  link it (the editors).
- **`pack`** (plan 13d ruling R2): `load(dir)` gives `Pack { manifest,
  source: Source, other }`. It reads the manifest and the kind's files
  through `olr-format` (`Source::read`; a level keeps its thumbnail);
  `other` holds every file the kind's writer would not produce (foreign
  files such as `level.ron~` or a README, and a mod's assets), byte for
  byte. A pack holding a name `DirFs` cannot address (not UTF-8, or one
  `VPath` refuses) does not load: the whole-pack save would lose it. Nor
  does one above `MAX_ARCHIVE_FILES` files or `MAX_UNPACKED` bytes in all
  (an archive's bounds), checked from the file lengths before any read.
  The format's files are `Source::reads()` (`olr-format`'s `file_paths`,
  no image encoded, and a mod's `restrictions.ron`).
  `files(pack)` is every file as its canonical writer gives it, plus
  `other` (a format file wins a clash). `save(dir, pack)` validates
  `files(pack)` in a `MemFs` with `olr_pack::validate` (an error is
  `SaveError::Invalid`, and nothing is written), resolves `dir` (`.` and
  `x/..` name the pack itself; a symbolic link is saved where it points
  and stays), then swaps the new pack in with
  `olr_pack::install::replace_dir`: the pack on disk is the old one or the
  new one, never a mix. Files the source no longer has are gone (a removed
  weapon's file). Empty directories and file permissions are not kept.
- **`fmt`**: `unformatted(dir)` lists the format files whose bytes differ
  from their canonical form, and those the writer leaves out (an empty
  `restrictions.ron`, which a save removes), never a foreign file; when it
  lists any, a pack that would not check is `SaveError::Invalid`, as
  `format` would refuse it. `format(dir)` saves when any differ and
  returns `Formatted { files, warnings }` (the save's warnings). `save`'s
  validation is `pack::check(files)`. Behind `olr-cli pack fmt`.
- **`new`** (R6): `new_pack(dir, kind, id, name, Template)`. `Mod` is the
  smallest mod that compiles (one weapon, `gun`, and a `Standard` preset
  enabling it); `Level { size, theme }` is dirt in a one-pixel rock border
  with a thumbnail; `From(dir)` copies a pack of the same kind, replacing
  the id, the name (the manifest's and the source's) and clearing the
  origin; it returns the save's warnings. A non-slug id, an existing
  non-empty `dir` or a template of another kind is refused before anything
  is written, and a failed save removes the directories it made (parents
  included).
- **`level`** (R3), on a `LevelSource` in world pixels: `Shape` (`Rect`,
  `Circle`; clipped to the level before iterating, in `i64`/`i128`, so a
  huge or far shape costs at most the level and never overflows), `Paint`
  (a colour, or a tile repeating from the art origin; composited over the
  art, so a transparent pixel leaves it; an empty tile paints nothing),
  `carve`, `fill`, `paint`, `stamp` (an image in art pixels at world ×
  art scale), `resize` (returns the objects it drops, as they were),
  `add_spawn`, `move_object`, `remove_object` (by index; `EditError` for a
  bad index, more than `MAX_OBJECTS`, a size or art out of bounds) and
  `refresh_thumb`. Every touched world pixel changes its whole art block,
  and `front == back` wherever the material is background, before and
  after every operation.
- **Level composition** (plan 14a; decisions in `docs/porting/STATUS.md`,
  "Level composition decisions"). `Pack::composition` holds a level's
  `editor/` (`None` for the other kinds; `files` refuses one there).
  - **`pixelate`**: `process(original, art_size, Pixelation)` (and the
    crate's `process_grid`, the unscaled element grid): area resample,
    alpha threshold, median cut (exact colours when they fit; splits at a
    channel-value boundary), Bayer 8 × 8 or Floyd-Steinberg in sixteenths
    (two rows of errors), nearest colour by an exact pruned search
    (`Palette`). Integer only.
  - **`compose`**: `Baker::new(src, size, art_scale, back_tile)` checks
    (`check_composition`), decodes and processes once; `bake_region` never
    fails and equals the full bake by construction; `bake` adds the
    thumbnail; `compose_from` makes a plain level one raster element
    `base`; a hidden image is decoded (its errors) but not processed;
    `theme_back_tile(content, theme)` (linked pack directories count);
    `stale(pack, tile)` lists
    the baked files that differ from the composition's bake.
  - **`element`**: `add_image` (`add_image_with` takes settings applied
    in the same step), `add_layer`, `set(Property)` (`set_all` for
    several, checked once), `remove`,
    `raise`, `lower`, `draw`, `erase`, `resize` (before `level::resize`),
    `ensure_composed`. Each edits a copy and keeps it only when
    `check_composition` passes (`ElementError::BadValue`, prefixed with
    the edit's property or file only when the composition checked before
    it). A locked element refuses `set` (but `Locked`), `remove`, `draw`
    and `erase`. Ids are at most 64 bytes. None bakes: callers do.
- **Tests** (`tests/`, one binary): `pack.rs` (every bundled pack loads and
  writes back byte for byte; foreign files kept; invalid results refused;
  saves through `x/..` and a symbolic link; a name a save would lose),
  `fmt.rs` (`bundled_packs_are_canonical`, R5's `check` guard), `new.rs`,
  `level.rs` (a proptest of the background invariant at art scales 1-4)
  and `guide.rs` (`docs/formats/README.md`'s examples parse, are canonical
  and check clean, the composed level's files generated); `pixelate.rs`,
  `compose.rs` (the raster round trip of every bundled level, the region
  proptest, `the_fixture_is_baked` over `fixtures/edit/composed-level`, the
  ignored `bake_speed`: about 0.3-0.4 s in release for a full-level
  256-colour diffused image) and `element.rs`. The bundled-content tests skip without
  `OLR_CONTENT_DIR`.

Test: `cargo nextest run -p olr-edit`.
