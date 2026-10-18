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
- **`compose::split_layers` / `split_from`** (plan 14c, the playtest): a
  level's material, front and back as three drawn layers, bottom to top
  `background` (the back art everywhere), `dirt` and `rock` (the front art
  over their cells, clear elsewhere); they bake back to the level's files
  byte for byte (`every_bundled_level_round_trips_through_three_layers`).
  The editor opens a plain level this way; `ensure_composed` (the CLI's
  element commands) keeps its raster base.
- **`level`** (R3), on a `LevelSource` in world pixels: `Rect` (a world
  rectangle; `compose` re-exports it), `Shape` (`Rect`, `Circle`,
  `Ellipse` and `Line` (plan 14b R10: by pixel centre, in doubled
  coordinates, a line's distance compared in 256 bits); clipped to the
  level before iterating, so a huge or far shape costs at most the level
  and never overflows; `shape_bounds`), `draw_art`/`erase_art` (paint a
  level-sized art layer in memory, opaque, returning the world rectangle;
  `element::draw|erase` use them), `rescale_art` (nearest neighbour
  between art scales), `Paint`
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
    plan 14b: `Decoded` (an element's data decoded: an image's processed
    grid, a drawn layer's art, a raster's material, front and back),
    `decode_element`, `bake_elements` (the one bake, from decoded items;
    data of a wrong size is left out, never a panic; `Baker` keeps
    `(Element, Decoded)` pairs and calls it), `encode_decoded` (a drawn
    or raster element's files), `resize_decoded` (`element::resize` uses
    it), and `ImageCache` (originals decoded once within `ORIGINAL_PIXELS`,
    4 grids per source by box art size and pixelation and `GRID_PIXELS` in
    all, entries
    fingerprinted by their bytes; `Baker::new_cached` shares one);
    `stale(pack, tile)` lists
    the baked files that differ from the composition's bake.
  - **`element`**: `add_image` (`add_image_with` takes settings applied
    in the same step), `add_layer`, `set(Property)` (`set_all` for
    several, checked once), `remove`,
    `raise`, `lower`, `draw`, `erase`, `resize` (before `level::resize`),
    `ensure_composed`; `new_element_id` and `new_source_name` (ruling R7
    and R8 of 14a over plain data, for the editor). New ids and names skip
    the pack's stray files (`other`): one in `editor/` is never overwritten. Each edits a copy and keeps it only when
    `check_composition` passes (`ElementError::BadValue`, prefixed with
    the edit's property or file only when the composition checked before
    it). A locked element refuses `set` (but `Locked`), `remove`, `draw`
    and `erase`. Ids are at most 64 bytes. None bakes: callers do.
- **Plan 14d** (decisions in `docs/porting/STATUS.md`, "Level editor
  decisions (plan 14d)"):
  - **`mask`**: `Mask` (one byte per original pixel, 0 erased; grey PNG),
    `mask_shape` (an original pixel by the world pixel holding its
    centre), `original_pixel`, `mask_colour` (4-neighbour flood of alike
    colours, `level::similar`'s percent rule; an explicit stack),
    `diff`/`crop`/`patch` for the history. `ImageCache::grid` takes the
    mask (keyed by its fingerprint) and applies it before `process_grid`.
  - **The bake** places a drawn layer at its box and reads its cells and
    back art (R4); `Decoded::Drawn { art, cells, back }`, `Decoded::Image
    { grid, mask }`; `resize_decoded` moves a boxed layer and pins a
    level-sized one off the origin.
  - **`drawn`**: `draw`/`erase` on a placed layer (the paint's phase from
    the level's art origin; erasing digs dirt or rock to its back art, R5),
    `mix`, `priority`, `Placed`, `placed`, `merge` (decision 4, C1, C2,
    decision 7; `None` beyond `MAX_SIDE` or `LEVEL_ART`), `split`,
    `into_layer`, `merge_down`, `split_element` (callers check locks).
  - **`element`**: `draw(…, material)`, `erase`, `merge_down`, `split`,
    `mask(MaskOp)`; `set At` on a drawn layer pins its box (or frees it at
    the origin); `set Material` on a mixed layer makes it plain.
- **Plan 14e** (decisions in `docs/porting/STATUS.md`, "Level editor
  decisions (plan 14e)"):
  - **`textures`**: `builtin()` (R1: Dirt, Rock, Grass, Brick, Metal,
    Wood; 64 × 64, tileable, integer hash; `builtin_texel` the generators,
    periodic every 64 pixels), `MAX_TEXTURE_SIDE` (512),
    `MAX_USER_TEXTURES` (256).
  - **`level`**: `Shape` is `Clone`, not `Copy`; `Shape::Stamp { x, y,
    scale, mask: Arc<StampMask> }` (a theme hole centred, scale clamped to
    `MAX_STAMP_SCALE` 16; 0 or a cover not matching its size covers
    nothing); `StampMask::from_hole` (covered where neither black nor
    transparent).
  - **`drawn`**: `fill` (R5: 4-neighbour from a seed within box ∩ level,
    an explicit stack and a visited `Vec<bool>`; `None` for a seed outside
    or no change; `fill_speed` ignored test: about 0.21 s in release for
    an open 4096 × 2048 layer), `crop` (to the covered art or back-art
    box), `unmix_plain` (a stroke that painted nothing leaves a plain
    layer plain). `new_source_name` never runs out (one more suffix than
    there are names).
  - **`element`**: `fill` (a seed outside the layer refused, the pack
    unchanged: `paint_layer`'s closure is fallible), `duplicate` (directly
    above; an image shares its original, its own files copied under the
    new id), `copy_name` ("Crate" → "Crate 2", "Crate 2" → "Crate 3").
- **Tests** (`tests/`, one binary): `pack.rs` (every bundled pack loads and
  writes back byte for byte; foreign files kept; invalid results refused;
  saves through `x/..` and a symbolic link; a name a save would lose),
  `fmt.rs` (`bundled_packs_are_canonical`, R5's `check` guard), `new.rs`,
  `level.rs` (a proptest of the background invariant at art scales 1-4)
  and `guide.rs` (`docs/formats/README.md`'s examples parse, are canonical
  and check clean, the composed level's files generated); `pixelate.rs`, `textures.rs`,
  `compose.rs` (the raster round trip of every bundled level, the region
  proptest, `the_fixture_is_baked` over `fixtures/edit/composed-level`, the
  ignored `bake_speed`: about 0.3-0.4 s in release for a full-level
  256-colour diffused image) and `element.rs`. The bundled-content tests skip without
  `OLR_CONTENT_DIR`.

Test: `cargo nextest run -p olr-edit`.
