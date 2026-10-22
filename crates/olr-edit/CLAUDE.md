# olr-edit

The pack authoring operations `olr-cli` and the editors share (plan 13d;
spec `docs/superpowers/specs/2026-10-16-formats-design.md` §6): a pack's
authoring source loaded and saved whole, the canonical formatter, pack
templates, the level operations, the skin operations and the mod
operations.

- **Layer 3, `std`.** Depends on `olr-data`, `olr-vfs`, `olr-format`,
  `olr-pack`, `thiserror`, `serde` (`mods`' RON values). It is not a
  forbidden importer: the game may link it (the editors).
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
  weapon's file). Empty directories and file permissions are not kept. `mod_pack(pack)` (issue #18, the mod editor's
  Play) is a mod `Pack` as the game reads a saved one: `files` checked as
  `save` checks them, then `olr_pack::read_mod` from a `MemFs`, nothing
  written (`an_unsaved_mod_reads_as_a_saved_one`).
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
  included). Issue #13: `Skin { double }` is a blank skin
  (`skin::blank`); `new_pack_with(…, edit)` changes the renamed source
  before the save (an error there, `NewError::Edit`, writes nothing).
- **`skin`** (issue #13; `olr-cli skin`, and issue #14's skin editor):
  `blank(name, double)` (every pixel transparent, OLX's layout or double
  resolution's), `double(src)` (a classic skin to double resolution by
  Scale2x per frame, a frame's edge its own neighbour; each art pixel
  takes colour, tint and glow from the one source pixel it copies, pixels
  compared by all three, so a tinted edge stays tinted: nearest scaling
  where Scale2x keeps the pixel; `left.png` by its own colours; name,
  notes and light kept; any other layout is `SkinError::NotClassic`, a
  size mismatch `SkinError::Size`, never a panic), `new_skin(dir, id,
  name, from, doubled)` (a copy of the skin pack at `from`, doubled when
  asked, or a blank one), `check_dir(dir)` (`Checked`: another kind of
  pack, then `olr_pack::validate`'s errors and warnings on the files as
  they are, then the skin as the game loads it) and `preview(skin,
  colour, scale)` (`preview_size`: 14 × 6 cells of `PREVIEW_CELL` 16 × 14
  world pixels at `scale` 1 to `MAX_PREVIEW_SCALE` 16 output pixels each,
  so both art scales preview alike; seven aim frames facing right then
  left, three walk rows on `PREVIEW_LIGHT` then three in the dark at a
  fifth of their colour, glowing pixels at their glow quarter
  (`olr_data::skin::glow_level`) and the light, if any, as a disc of its
  reach around the frame's glowing pixels, clipped to the cell: a preview,
  not the game's pixels); `preview_frame(skin, colour, f, left, dark,
  scale)` is one of its cells alone (the skin editor's live preview,
  issue #14).
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
  `refresh_thumb` (lit for a level with a look:
  `olr_format::levels::level_thumbnail`, `refresh_thumb_draws_a_lit_level_lit`). Every touched world pixel changes its whole art block,
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
- **Plan 15a, the look system** (spec
  `docs/superpowers/specs/2026-10-18-look-system-design.md`; the format's
  side is `olr-format`'s `look` and level looks):
  - **`look`** (Task 7): `set_look` (`None` removes it), `add_group` (a new
    id; returns its index) and `set_group` (keeps the id) on a
    `LevelSource` (plan 15c: no light objects, glowing pixels are a level's
    light); `rename_group` (the
    elements naming it follow) and `remove_group` on a `Pack`: the level's
    glow loses the group's pixels (strength and group 0), the groups above
    move down one, an all-zero glow becomes none, and every element naming
    it loses its glow and mask, locked or not (returns their ids; the
    caller rebakes a composed level). Each edits a copy and keeps it only
    when `compile_level` checks (`EditError::Look`).
  - **`level::resize`** re-lays the glow (`GlowLayers`) and clear flags
    with the art, the new area glowing none and opaque; a glow or flags not
    fitting the level are dropped, never indexed. Spawns outside the new
    level are removed and returned; the others' positions shift
    saturating.
  - **The bake** (`compose`, Tasks 5 and 14b): a front art pixel glows as
    the element giving its colour, a back art pixel as the background
    element giving it (a dirt or rock layer's back art never glows); a
    world pixel's group is that of the element glowing at its centre art
    pixel, else at the first glowing art pixel of its block, 0 none (batch
    A review, `a_cell_glowing_off_centre_takes_its_glow_group`). Strength
    percent bakes to 0-255, × the mask's value / 255. A glow naming no
    group of the level's look, or a mask not fitting its box, is left out.
    A solid world pixel is clear (255) when the element giving its
    material is. The level's `glow` and `clear` are `None` when nothing
    glows or nothing is clear (no `look/` files written). `GlowMasks`
    (element id → mask), `decode_glow_masks(src, size, s)` (each grey, its
    box × art scale; a missing or misfit one is `ComposeError::File`),
    `bake_elements_with` (`bake_elements` passes no masks: a masked glow
    bakes uniform; `Baker` decodes and passes them). `stale` covers the
    four `look/` glow images and `look/clear.png`.
  - **`glow`**: `paint_mask(mask, at, size, s, shape, value)` (a box mask at
    art resolution, clipped to the box) and `colour_region(art, seed,
    tolerance)` (glow by colour, the user's playtest, 2026-10-19: the
    4-connected art pixels `level::similar` to the seed's, an explicit
    stack; `None` for a seed outside).
  - **`element`**: `Property::Glow(Option<ElementGlow>)` and
    `Property::Clear(bool)` (an image or drawn element, `WrongKind` for a
    raster; `set_all` refuses a glow naming no group of the level's look
    before editing). `paint_glow(pack, id, shape, strength)` paints the
    mask (0-100, 0 erasing; a first mask starts at 255) and
    `glow_colour(pack, id, at, tolerance, strength)` (the user's playtest,
    2026-10-19; `olr-cli level element glow --colour-at`) lights the region
    alike in colour to world pixel `at` (a glow without a mask starts
    dark: only the region lights). Both need the element to glow already,
    refuse a locked one, write `editor/glow/<id>.png` and set its `mask`.
    `merge_down` and `split` results lose their glow (Task 5 ruling);
    `duplicate` copies the mask and `remove` drops it, with the element's
    files (`element_files` lists it).
    `element::resize` re-lays a painted glow mask with its layer (a
    level-sized layer's moves by the offset) and `set_all` resamples a
    resized image's (`glow::relay_mask`, `glow::resample_mask`, shared
    with the editor; plan 15a's final review).
- **Issue #11, distant layers** (the format is `olr-format`'s
  `Element::distant`): the bake leaves distant elements out of the
  material, front and back. With a visible distant element that bakes
  (`has_distant`), every back pixel no other background covers (a raster's
  back pixel of alpha 0 counts as uncovered) is the back tile's colour at
  alpha 0, see-through, and so is the front over background; a solid
  block's front stays opaque. Without one the bake is as before (every
  bundled level bakes byte for byte). A whole-level bake remakes
  `level.distant` (`bake_distant`: each visible distant element's art at
  its box, an image's grid expanded, drawn art's covered pixels opaque;
  a region bake leaves it: callers remake it when distant items change).
  `distant_elements(level)` gives a level's distant layers back as
  elements (`distant-N`, boxed drawn Background layers at their depth),
  which `compose_from` and `split_from` put at the bottom. `stale()`
  covers `level.ron`'s distant entries and `distant/<i>.png`. `element`:
  `Property::Distant(Option<u8>)` (an image or drawn element) moves the
  element to the top of the distant layers, or just above them when no
  longer distant; `raise`/`lower` refuse to cross the boundary
  (`DISTANT_ORDER`), `merge_down` and `split` refuse a distant layer;
  `asset::from_elements` and `cut_rect` leave distant elements out (Save
  as asset). Tests: `compose.rs`'s
  `distant_layers_make_uncovered_back_pixels_see_through`,
  `distant_layers_split_and_bake_back`; `element.rs`'s
  `distant_layers_keep_below_the_rest`, `an_asset_carries_no_distant_layer`.
- **Issue #9, the asset library** (`asset`; the format is
  `olr_format::assets`; STATUS.md "Level editor asset library (issue
  #9)"): `Piece` (an element, its `Decoded`, an image's original, its glow
  mask; `PieceRef` borrowed) and `Asset` (name, art scale, size, pieces
  bottom to top relative to its top-left, the glow groups they name).
  `from_elements` (each drawn or raster piece cropped to what it covers,
  `drawn::crop`; an image whole) and `cut_rect(level, rect, layers, name)`
  (every visible element, or those of `layers`; an image wholly inside
  kept whole; anything else cropped as drawn pixels with its cells and
  back art; a piece with no pixels, an all-zero glow mask (its group not
  carried) and a piece identical in pixels, material and glow to one below
  left out). `land(to, piece, from, offset, name)` lands a piece as a new
  element (new id and name, art and mask resampled across art scales,
  originals shared or renamed; the editor's paste uses it too);
  `with_groups` the placing's look (`GroupChange`: joined, added with the
  asset's settings, `added_look` for a level without one; `ADDED_LIGHTING`
  says so); `piece_name`. `encode` writes `AssetSource` (`images/`,
  `layers/`, `masks/`, `glow/`, and `thumbnail`: baked under
  `preview_look`, ambient `PREVIEW_AMBIENT` `#304060` at 20 %, through
  `level_thumbnail`); `decode` checks (`check_asset`) and decodes
  (`decode_element`, `decode_glow_masks`). `load_dir`, `read_dir`,
  `save_dir` (`replace_dir`), `summary` (`asset.ron` and the thumbnail
  only: the Assets tab's entry). `DecodedLevel::of_pack` (a plain level
  as its three layers) and `place_in_pack` (`olr-cli asset place`: one
  checked edit of a copy, the caller bakes). The library:
  `MAX_USER_ASSETS` (256), `valid_asset_id`, `asset_slug`,
  `list_library`, `add_to_library` (a new folder named after it, or over
  `replace`), `import_into_library` (read and checked whole, written
  anew, a name clash numbered), `remove_from_library`.
- **Issue #16, the mod operations** (`mods`; `olr-cli mod`; STATUS.md
  "Mod editing operations (issue #16)"), on a loaded mod `Pack`
  (`source(pack)`, else `ModError::NotAMod`):
  - **`path`**: `File` (`mod`, `restrictions`, `weapons/<id>`,
    `projectiles/<id>`, the longest existing id), `Seg` (`.name`, `[i]`,
    `["key"]`; printed plain when it can be), `FieldPath`, `parse`.
  - **`node`** (private): the `Node` trait over every authoring type
    (`ron`, `parse`, `children`, `child` (a spawn's shared `projectile` is
    `Step::Shared`, followed into `projectiles/<id>`), `child_mut`,
    `choices`/`set_variant` (a variant's defaults: `Variants::made`, else
    `Name`, else `Name()`; `Unknown` never a choice), `insert` for maps),
    through the `leaf!`, `fields!` and `composite!` macros. `fields!`
    patterns name every field, so a new authoring field does not compile
    until it is listed; a new enum variant needs its `CHOICES` entry.
  - `get`, `set` (a choice by name, RON, or a bare string; `weapons/<id>.name`
    and `mod.name` rename; a missing last map key is inserted;
    `mod.weapons` read-only), `children`, `choices`, `variant`, `leaves`.
  - **`commit`** (every operation): refuses an edit adding an error the
    mod did not have (`ModError::Invalid`, the new `Issue`s) or whose
    changed files do not read back as set (`ModError::Refused`); an
    unchanged mod returns at once.
    Issue #17: `set_lenient` (the mod editor's field edits) keeps a change
    adding errors and returns them (the editor lists them; its Save
    refuses a mod with errors, as `pack::save` does); a value of the wrong
    type, a result not reading back and a refused rename still change
    nothing.
  - **`weapons`**: `blank_weapon` (also `new`'s template), `add_weapon`
    (`WeaponFrom::{Blank, Copy { pack, id }}`: shared projectiles and
    asset files brought along, equal ones reused, others renamed),
    `remove_weapon` (never the last), `move_weapon`, `copy_weapon`,
    `rename_weapon` (presets follow, OLX's name match), `rename_mod`.
  - **`shared`**: `used_by`, `inline`, `share`.
  - **`check`**: `Issue { severity, file, path, line, col, message }`;
    `check(pack)`, `check_dir(dir)`, `source_issues`: validate's, every
    `mod_errors` error and missing asset at its path, unparsable files,
    unused assets and shared projectiles; errors first.
  - **`art`** (issue #19, the mod editor's Art and Sounds, `olr-cli mod
    gfx|sfx`): `images` (`gfx/*.png` but the masks), `sounds`, `users`
    (`AssetUser { path, label }`: "Bazooka's shot", "Bazooka's fire
    sound"), `image_frames` (a PNG within `IMAGES`, at least as wide as
    tall), `check_sound` (`olr_pack::sounds`: WAV or Ogg in the limits),
    `import_image`/`import_sound` (a new plain name in `gfx/`/`sfx/`),
    `set_image` (`ImageChange`: frames before and after, `changes_play`;
    a mask that no longer fits goes), `glow`/`set_glow` (values of the
    image's size; all 0 removes the mask)/`set_glow_png`, `remove`
    (refused while used, listing the users; an image's mask goes with
    it), `rename` (file, mask and every field naming it, one `commit`),
    `blank_image`. A refusal (`ModError::Refused`) changes nothing.
  - `walk` (private): every spawn and projectile with its path and key,
    `fresh_keys` (explicit ids where inline keys would clash), shared
    references and asset references.
- **Issue #37, surface materials** (the format is `olr_format::materials`
  and the composition's `Element::{surface, back_surface, surface_map,
  back_surface_map}`; STATUS.md "Surface materials (issue #37)"):
  - **The bake** (`compose`): `bake_elements_with` takes
    `PaintedSurfaces` (the painted maps' values by file,
    `decode_surface_maps(src, size)`; `Baker` decodes them; one of the
    wrong size is left out). A solid world pixel's front material is the
    surface of the element giving its collision material (a painted value
    1-n overriding it, 0 or past the look's materials keeping it); a
    background pixel's is 0. The back map follows the backdrop as the
    glow groups do (the element at the centre art pixel, else the first
    of the block): its own art gives `surface`, its back art
    `back_surface`. Ids the look lacks name nothing. `level.surface` is
    `None` when every value is 0, so a level without materials bakes as
    before, byte for byte. `bake_distant` remakes `distant_surfaces`;
    `distant_elements`, `compose_from` (the raster takes both maps as its
    painted ones) and `split_from` (background the back map, dirt and
    rock the front) keep a plain level's maps. `stale` covers the two
    `look/surface*.png` and the distant surfaces.
  - **`element`**: `fit_surface_maps` after every edit drops a back
    surface without back art and maps that no longer fit or sit on a
    distant layer; `resize` re-lays the maps with the layer
    (`relay_surface`, shared with the editor; the new area 0) and
    `set_all` resamples a resized image's, each after checking the new
    size's bounds (no map laid out at a refused size). `level::resize`
    re-lays a plain level's own maps as its clear flags. `asset::from_elements`, `cut_rect` and `land` strip surfaces
    (assets carry none until materials 2/10).
  - **`material`**: the libraries: `read_file` (bounded by
    `MAX_RON_BYTES`), `list_library(dir, max)` (by name then id; an
    unreadable file a warning "material <id>: …: not offered"; past `max`
    a warning) and `list_materials(builtin, user)`: `MaterialEntry {
    builtin, material }`, the built-in set first, then the user's capped
    at `MAX_USER_MATERIALS` (256).
  - Tests: `surface.rs` (each kind's landing, a mixed layer per pixel,
    back art, a raster, painted maps, distant surfaces, no materials no
    maps, a plain level's maps through one raster or three layers,
    `level::resize`, a huge size refused) and
    `material.rs` (`the_built_in_set_loads_cleanly`, the user's library
    after it and capped).
- **Tests** (`tests/`, one binary): `asset.rs` (issue #9: round trips
  through a folder at art scales 1 and 2, masks and groups joined or
  added, resampling across scales, the rectangle's rules, a picture kept
  whole, a raster's cells and back art, bad folders refused, the
  library), `pack.rs` (every bundled pack loads and
  writes back byte for byte; foreign files kept; invalid results refused;
  saves through `x/..` and a symbolic link; a name a save would lose),
  `fmt.rs` (`bundled_packs_are_canonical`, R5's `check` guard), `new.rs`,
  `skin.rs` (issue #13: a copy of OLX's default skin and of Firefly
  round-trips exactly; `double` keeps each 2 × 2 block where Scale2x keeps
  the pixel and rounds a tinted, glowing corner within its frame; blank
  skins; `check_dir` refusing another kind, a wrong-size glow, a light
  over the caps, partial tint, a cut PNG; the preview's size and glow;
  issue #14's `a_preview_frame_is_the_previews_cell`; on the committed `content/`, offline too),
  `mods.rs` (issue #16: set changes exactly the fields set on a copy of
  Classic, every leaf of Classic and MW round-trips through set, paths
  through shared projectiles and defaults, variants, new keys, refusals
  (a cycle, two effects in an event) with their paths, weapon add,
  remove, move, copy and rename with presets and a shipped loadout
  following, a weapon of MW with its projectiles and files, used-by
  against the files, inline and share round trips, check listing several
  errors, unparsable files never a panic; issue #17's
  `a_lenient_set_keeps_the_error_and_the_save_refuses_it`), `art.rs`
  (issue #19: users named and a used file kept, imports checked, a glow
  mask round-tripping and refused at another size, a replaced image's
  frames, a rename following its users),
  `level.rs` (a proptest of the background invariant at art scales 1-4)
  and `guide.rs` (`docs/formats/README.md`'s examples parse, are canonical
  and check clean, the composed level's files generated; the asset
  example's files too); `pixelate.rs`, `textures.rs`,
  `compose.rs` (the raster round trip of every bundled level, the region
  proptest, `the_fixture_is_baked` over `fixtures/edit/composed-level`, the
  ignored `bake_speed`: about 0.3-0.4 s in release for a full-level
  256-colour diffused image) and `element.rs`; plan 15a: `glow.rs` (the
  channel rules, the region bake equal to the full bake, no glow writing
  no `look/` files, a painted mask, an unknown group refused by `set` and
  by the pack check, a duplicate keeping its glow and mask, a cell glowing
  off centre, clear following the element giving the material, a raster
  refusing clear, a resize re-laying clear flags) and `look.rs` (look
  edits validate, unique group ids, removing a group renumbers and
  unglows, renaming follows the elements, the last group's removal drops
  an empty glow, `a_resize_drops_a_glow_that_does_not_fit`). `glow_colour` is tested through `olr-cli`'s
  `cli_glows_pixels_by_colour` and the editor's
  `glow_pixels_picked_by_colour`. The bundled-content tests skip without
  `OLR_CONTENT_DIR`.

Test: `cargo nextest run -p olr-edit`.
