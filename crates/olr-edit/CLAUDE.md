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
  `editor/` layers (`None` for the other kinds; `files` refuses one there).
  Issue #74 (STATUS.md "Asset-based levels: the level format (issues
  #73, #74)"): they are the ground asset's (`editor/asset.ron`), in
  memory as a composition so `element`, `level element …`, `asset
  save`/`place` and the editor work on the ground unchanged:
  - **`Pack`** gains `placed: Option<PlacedSource>` (`editor/placed.ron`,
    `read_placed_files`), `legacy_composition` and `verbatim` (`Verbatim`:
    the other assets' files as read); `Pack::new(manifest, source)`.
    `load` never reads `editor/composition.ron`: without a placement such
    a pack loads with `legacy_composition` set (the file kept in `other`;
    beside a placement it is a leftover there). `composition` is the
    ground's (`ground_composition`) when the placement is the ground
    alone (`ground_alone`: one `Own` ground, one copy of it at the origin,
    the level's size and art scale, nothing Live, no state, values or
    script), else `None`. `placement(pack)` is the placement to write:
    the composition's elements, folders and files in the ground (as
    `GROUND_NAME` "Ground", the level's size and art scale), synthesized
    for a composition without one (`ground_placement`), and while the
    ground is alone its glow groups and materials the look's (so
    `level.ron`'s look and `asset.ron` agree). `files` writes it
    (`write_placed_files`) and every other asset's files byte for byte
    while unedited (`verbatim`); `save` bakes nothing itself.
  - **The refusal**: `Pack::ground_refusal()` is `UNCONVERTED` ("editor/
    composition.ron: a level from OLR 0.2: run olr-cli level convert
    <pack>") for a legacy pack and `NOT_THE_GROUND_ALONE` for a placement
    of more than the ground; `element::ensure_composed`, `commit`,
    `resize` (`ElementError::NotTheGround`), `asset::DecodedLevel::of_pack`
    (so `asset save`/`place`) and `placed::bake_pack` refuse it.
    `ensure_composed` gives a plain level its ground and a placement of it
    alone; `new`'s template stays plain.
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
    the baked files that differ from the composition's bake (`differing`
    of two levels, which `placed::stale` shares).
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
    four `look/` glow images, the two glow beam maps and `look/clear.png`.
  - **Glow beams** (`LayerGlow::beam`): a world pixel's beam texel
    (direction / 2 + 1, spread) comes from the same layer and the same
    pick (`cell_pick`) as its group, so the maps agree; the front
    (composite) map holds beams on Rock cells only, Background and Dirt
    cells get `(0, 0)` there, so a backdrop's beam is baked and counted
    once; the back map follows `groups_back`'s pick. `glow_beams` is
    `None` when no texel is set. The haze map has no element source: it
    lives only in `look/haze.png`, set by `look::set_haze_map` (refuses a
    misfit), re-laid by `level::resize` (the new area 255, the level's whole
    haze: ruling T6c, as a new map's in `olr-cli level haze paint`, T6a;
    `a_grown_levels_haze_map_is_full`) and left alone
    by the bake.
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
  as asset). Issue #62 (STATUS.md "Glowing distant layers (issue
  #62)"): a distant element may glow. `bake_distant(level, items, masks)`
  gives each layer its `DistantGlow` (the element's strength, its group's
  value, its mask when it fits); in the pixel bake a see-through back
  pixel (no element covers it) takes the glow of the nearest visible
  distant layer covering it at its reference placement (its box: the
  camera at the level's centre) into `glow.back` and `groups_back` (and
  the front over background), exactly as a glowing back element would,
  and that layer's RGB at alpha 0 where it glows; a nearer layer that
  does not glow there hides a farther one; an opaque back pixel never
  takes one. `distant_elements` gives a glowing layer back as a glowing
  element with its mask's file (`editor/glow/<id>.png`; `compose_from`
  and `split_from` add it), `stale` compares the glows and
  `distant/<i>-glow.png`, and `look::remove_group` unglows a plain level's
  distant layers of the group and renumbers the rest. Tests:
  `tests/distant_glow.rs` (the glow and group taken, a nearer layer
  hiding, a mask, no glow baking as before, a distant sky baking the maps
  an ordinary one does, split and bake back, `stale`), `look.rs`'
  `removing_a_group_renumbers_and_unglows`. Issue #11's tests: `compose.rs`'s
  `distant_layers_make_uncovered_back_pixels_see_through`,
  `distant_layers_split_and_bake_back`; `element.rs`'s
  `distant_layers_keep_below_the_rest`, `an_asset_carries_no_distant_layer`.
- **Background glass** (spec
  `docs/superpowers/specs/2026-10-25-background-glass-design.md`; the
  format is `olr-format`'s `Element::glass`): in a see-through level
  (`has_distant`) a glass element's own background art (its backdrop
  when it is its own art: a mixed layer's background pixels) bakes at
  its opacity (`glass_alpha`: `round(opacity × 255 / 100)`), laid over the
  topmost back below it that is not glass with a correct straight-alpha
  `glass_over` (a see-through destination gives no colour; `level.rs`'s
  `over`, painting's, is unchanged); several panes stack bottom up. Without
  distant layers glass is opaque, its own colour. `level.glass`
  (`GlassMap`) is baked only when some glass tints or frosts: per art
  pixel, from the topmost element giving the back when it is glass, the
  filter `mix(255, colour, tint)` (rounded) and the frost; white and 0
  elsewhere; `None` when nothing filters or frosts. A distant layer's glow
  never passes through glass (only alpha 0 takes it). `level::resize`
  re-lays the map (the new area none); `stale` lists `look/glass.png`
  after a tint or frost change (`stale_lists_the_glass_map_after_a_tint_change`);
  `asset::plain` keeps glass (issue #71: assets carry it). `element::Property::Glass` (a raster `WrongKind`; the
  composition check refuses the rest). Tests: `tests/glass.rs`.
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
  Issue #71 (the format is `olr_format::assets`' states; STATUS.md
  "Asset-based levels: the asset format (issue #71)"; `asset/posed.rs`):
  `Asset` gains `folders`, `states` (`State { name, own: BTreeMap<id,
  OwnPicture { decoded, glow_mask }>, values, loop_sound, enter_sound }`),
  `start` (an index), `exposed`, `sounds` (by name), `script` and
  `health`; `Asset::new` is one state and nothing else, `Asset::state`
  finds one by name. `pieces` stay the starting state's layers; `encode`
  stacks each owned layer's pictures (art, cells, back art, glow mask)
  below the start's in state order and `decode` unstacks them (a tall
  element, `decode_element`, then cut), and checks each sound as a mod's
  (`olr_pack::sounds::is_sound` and `check_sound`). `posed(asset, state,
  copy_values)` (`Posed { pieces: Vec<PosedPiece { piece, opacity }>,
  groups, materials }`) is a static copy's look: a state is a complete
  look, each property taking the state's value, else the copy's (exposed
  only; an unknown name or bad value an error), else the exposed
  default, else the layer's own; the state's pictures replace the
  start's, then the values apply (offsets, `LayerVisible`/`Opacity` 0
  drop a piece, `Opacity` otherwise carried, glow strength replacing the
  strength with the mask kept, group colour/intensity/reach/animation,
  beam and glass), then `Facing` mirrors every piece within the box (a
  beam `d` to `(180 − d) mod 360`), `X`/`Y` move all and `Visible` false
  leaves none. `offers(asset)` (`Offer { property, kind, range, sharing
  }`: the format's `property_type` over every candidate, a group
  property's other layers in `sharing`) is the list #72's commands and
  #80's tree share. `live_refusal(asset)` (`LiveRefusal::NothingToDrive`
  first, then `Dirt`: a plain Dirt layer or a mixed layer's dirt cell in
  any state, then `RasterOrDistant`; `Display` texts). `Summary` gains
  `states` and `may_be_live` (from `asset.ron` and whether `script.rhai`
  exists). Landing until #75/#82: `landed_pieces` (the start posed with
  its defaults; `place_in_pack`, the editor's `place_asset` and the
  thumbnail use it) and `left_behind` (`Placement::note`: "<name>'s
  states, values, sounds and script stay in the asset"). `plain` keeps
  glass. Tests: `asset.rs`' `decode_and_encode_round_trip_states`,
  `posed_takes_the_state_then_the_copy_then_the_default`,
  `a_state_is_a_complete_look`, `posed_mirrors_offsets_hides_and_tints`,
  `the_starting_state_poses_as_the_pieces`, `live_refusal_says_why`,
  `offers_follow_the_design`, `asset_sound_limit_is_the_mods`,
  `landing_a_stated_asset_lands_its_start`; `glass.rs`'
  `an_asset_of_glass_keeps_its_glass`.
  Issue #72 (`asset/edit.rs`; STATUS.md "Asset-based levels"): the
  operations the asset tab (#79, #80) and `olr-cli asset` share, each on
  `&mut Asset` through `commit` (a copy edited, kept only when the folder
  `encode` would write, without its thumbnail (`unchecked_files`), passes
  `check_asset`; a refusal is `AssetError::Bad` naming the state, layer,
  property or sound, the asset unchanged): `add_state(name, from)` (none:
  the start's values, owning nothing, no sounds; `from`: that state's
  values, pictures and sounds; returns the index), `remove_state` (never
  the start: "make another state the start first", nor `health.at_zero`'s),
  `rename_state` (the start and `at_zero` follow), `set_start` (every
  state's look kept: the old start owns the base, followers a copy, an
  owner without a mask the old base's mask, the base becomes the new
  start's picture), `set_state_sounds` (`None` leaves, `Some(None)`
  clears), `own_picture` (the box at art scale, cells for a mixed layer
  only, back art iff the layer has it, a mask only on a masked layer;
  never the start, an image, a raster or a distant layer), `follow_start`,
  `set_state_value` (`None` clears; a value of the same `property_key`
  replaced in place; the type said plainly), `forget_layer` (`Forgotten {
  own, values, exposed }`, for #79's removal, merge and split; not
  checked), `expose` (one of `offers`, an `Int`'s `min`/`max` filled from
  its range, a position needing both, one entry per glow group, an `Own`
  colour needing a default, `Health`'s default its points),
  `unexpose`, `rename_exposed`, `add_sound` (`valid_sound_name`, new case
  aside, `is_sound` and `check_sound`), `remove_sound` (refused while a
  state plays it), `set_script` (`MAX_SCRIPT_BYTES`, not parsed) and
  `set_health` (`at_zero` a state; an exposed `Health` follows the points;
  clearing it while exposed: "unexpose health first"). The CLI's syntax:
  `parse_property`/`property_label` (`x`, `<layer>.glow-colour`, …, split
  at the last dot) and `parse_value`/`value_label` (the type from the
  form; an animation one line of RON); `default_value` (the exposed
  default, else the own setting). The thumbnail is already the start posed
  with its defaults (`landed_pieces`). Tests: `asset.rs`'
  `a_new_state_copies_the_start`, the proptest
  `set_start_keeps_every_states_look`, `removing_states_is_refused_when_needed`,
  `renaming_a_state_follows_start_and_health`,
  `own_picture_checks_fit_and_kind`, `state_values_are_set_and_cleared`,
  `forget_layer_drops_every_reference`,
  `expose_offers_only_what_offers_lists`,
  `sounds_are_checked_and_kept_while_named`,
  `health_needs_its_state_and_unexposing_first`,
  `the_thumbnail_shows_the_starting_state`.
- **`placed`, issue #73's bake of a level's placed copies** (the format
  is `olr_format::levels`' `PlacedFile`; STATUS.md "Asset-based levels:
  the level format (issues #73, #74); issue #74's pack functions below):
  `bake(level, src, back_tile) -> Result<Baked, PlacedError>` checks
  (`check_placed`, the level's art scale within `MAX_ART_SCALE`), decodes
  each asset (`asset::decode_own` for the ground, `check_own_asset`; else
  `decode`), poses each copy (`posed`; a Live copy refused by
  `live_refusal`, "cannot be Live: …", and otherwise baked as static for
  now), gathers its posed groups and materials in copy order
  (`olr_format::levels::gather_groups`/`gather_materials`: shared when
  identical, else `<id>-2`, …; `Baked::groups_renamed`/
  `materials_renamed` list the renames) and renumbers its painted surface
  maps, then lays each piece (folders folded) as an element
  `<copy id>.<element id>`: its box before the posed move mapped by
  `stretch_span` (floor), the move by `stretch_offset` (half away from
  zero, `PosedPiece::offset`), its data resampled nearest (`resample`,
  per box, an image's processed grid not its original; art across art
  scales by `level::rescale_art`). Every distant piece goes first, then
  the rest; the gathered groups are checked against `MAX_GROUPS` again
  (a copy's values may change settings); the look is the level's with
  the gathered groups and materials (a default one, `Baked::note` saying
  `ADDED_LIGHTING`, when the level had none and something was gathered)
  and must compile. The level's sizes stay; its buffers are made fit and
  everything is baked with `compose::bake_elements_with_opacity` (the
  full rect) and `refresh_thumb`. Nothing is written. Errors are
  `PlacedError` (`Format`, `Asset { key }`, `Copy { copy }`, `Level`).
  Opacity (`compose`): `bake_elements_with_opacity` takes one percent per
  non-distant item (missing: 100); a Background piece below 100 mixes
  its back pixels over the back below as glass does (straight alpha,
  `round(opacity × 255 / 100)`) and never over a see-through (alpha 0)
  pixel; `bake_elements_with` passes none, so every composition bakes as
  before.
  Issue #74, on a `Pack`: `bake_pack(pack, tile)` bakes `pack::placement`
  into the level and keeps it (refusing an unconverted level);
  `stale(pack, tile)` lists the baked files differing from the
  placement's bake, and `level.ron` when its look's groups or materials
  are not the ground's (`asset.ron` edited by hand); `from_composition
  (pack)` converts an OLR 0.2 level (`Conversion::{Converted, Already,
  Plain}`): the ground "Ground" of the level's size and art scale, the
  composition's elements, folders and files (paths kept), the look's
  groups and materials, one locked copy `ground`; `composition.ron` left
  out of `other`. `edit_ground(pack, f)` runs a look edit on the ground
  asset's groups and materials (by id) when other copies are placed,
  `brought_by(pack, id, material)` names the copy whose asset alone
  brings one (`brought_by_message`: "it comes with copy <id>'s asset:
  edit that asset"). Tests: `tests/placed.rs`
  (`a_ground_alone_bakes_as_its_composition`, byte for byte against
  `compose::bake` on two levels, `a_stretched_copy_maps_columns_by_floor`,
  `an_art_scale_change_resamples_nearest`,
  `positions_and_facing_scale_with_the_copy`,
  `opacity_mixes_a_background_piece_over_the_back_below`,
  `distant_pieces_bake_below_everything`,
  `gathering_shares_equal_groups_and_renames_others`,
  `a_live_copy_with_dirt_cells_is_refused_by_the_bake`,
  `live_copies_bake_as_static_for_now`; issue #74's
  `converting_a_02_level_keeps_its_bake`,
  `a_02_pack_is_refused_by_ensure_composed`,
  `save_writes_unedited_copies_verbatim`,
  `a_ground_edit_lands_in_the_ground_asset`,
  `stale_lists_level_ron_when_its_groups_drift`,
  `the_editor_scope_is_the_ground_alone`,
  `hostile_placed_levels_never_panic`).
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
    re-lays a plain level's own maps as its clear flags.
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
- **Issue #38, assets carry materials and the CLI's operations**
  (STATUS.md "Surface materials in assets and olr-cli (issue #38)"):
  - **`asset`**: `Piece`/`PieceRef` hold `surface_map` and
    `back_surface_map` (the element's box at world resolution, a
    raster's the level's; values index the materials of the level or
    asset holding the piece) and `Asset` its `materials`. `from_elements`
    and `cut_rect` keep surfaces, re-lay the maps with the crop
    (`element::relay_surface`) and carry the look's materials the pieces
    name (`surface`, `back_surface` or a painted value), in the look's
    order, the maps re-numbered to them; a map all 0 or not fitting goes,
    the flags follow the maps. `land` still strips surfaces (the editor's
    paste); `with_materials` (`MaterialChange`: joined by id, the level's
    settings kept, or added; `renumbering`) and `land_surfaces` give a
    landed element its surfaces and re-numbered maps: `place_in_pack` and
    the editor's `place_asset` use them. `encode`/`decode` write and read
    `surface/<id>.png` and `.back.png`; an asset without materials is
    byte for byte as before.
  - **`element`**: `Property::Surface`/`BackSurface` (an id the look
    lacks refused before editing; a back surface needs back art),
    `set_surface_map(pack, id, back, values)` (the whole map; a value past
    the look's materials, another size, a distant or locked element
    refused; none or all 0 removes it) and `surface_by_colour(pack, id,
    back, rules)` (`SurfaceRule { kind, colour, tolerance, material }`:
    each covered world pixel, read at its block's centre art pixel (the
    back art for `back`), takes the first matching rule's material
    (`level::similar`; `None` the default); the rest keep theirs;
    returns the pixels matched).
  - Tests: `asset.rs` (`materials_and_painted_maps_travel_with_an_asset`
    at art scales 1 and 2, joined and added, baked where placed;
    `placing_materials_gives_a_level_a_look_and_checks`;
    `an_asset_without_materials_is_as_before`) and `surface_ops.rs` (the
    element operations, rules by kind and colour reproducible, refusals
    changing nothing).
- **Issue #43, the level editor's materials** (STATUS.md "Level editor
  materials (issue #43)"): `look::material_users(pack, id)` (the
  elements naming it or painting its value, by name; for a plain level
  its distant layers and "the level"), `look::remove_material` (refused
  while used, naming the users; the painted and baked maps renumbered by
  `renumber_removed`: the removed value 0, those above down one) and
  `names_material`, which the editor shares. Tests: `surface_ops.rs`'s
  `a_material_in_use_is_not_removed`, `renumbering_drops_the_removed_value`.
- **Issue #59, layer folders** (the format is `olr-format`'s
  `Composition::folders`, `Element::folder` and `check_folders`; STATUS.md
  "Layer folders (issues #59, #60, #61)"):
  - **`folders`** holds every tree computation once (the editor's
    document calls it in issue #60): it re-exports the format's
    `check_folders` and `folder_chain`; `shown`, `locked`,
    `folder_locked`, `fold`/`folded` (effective flags: shown when it and
    every folder holding it are visible, locked when any is), `holds`,
    `members`, `parent_of`, `depth`, `tidy` (drops folders holding no
    element and orders the rest canonically; safe unchecked),
    `default_name` ("Folder n"), `new_folder_id`, `tree`/`flatten`
    (`Node`: the runs as a tree, written back canonical, empty folders
    left out), `group` (inside the innermost shared folder; scattered
    members first to the topmost one's place), `ungroup`, `move_to`
    (into a folder on top or at a position; out to a holding folder
    just above the one it leaves), `raise`/`lower` (past a sibling, a
    folder's whole run; out at a folder's edge; `DISTANT_ORDER`) and
    `remove_folder`. Each refuses a result breaking a rule or the
    distant order, leaving the composition as it was (`FolderError`).
  - **`element`**: `commit` runs `folders::tidy` after every edit (an
    edit emptying a folder removes it); `unlocked` and `set` use the
    effective lock (a lock refuses content, not order); `raise`, `lower`
    and `remove` take folder ids (a folder removed with its contents,
    refused while any is locked); `set_all` on a folder id takes `Name`,
    `Visible` and `Locked` (a locked folder only `Locked`); `group`,
    `ungroup`, `move_to`; `new_element_id` takes the folders (one id
    namespace) and `new_folder_id` is re-exported; `split`'s parts take
    the element's folder, `merge_down` the lower one's, `duplicate` the
    original's; `Property::Distant` takes an element out of its folders.
  - **The bake**: `Baker::new_cached` bakes folded clones and decodes by
    the element's own flags (a folder's visibility rebakes, never
    re-decodes); `asset::DecodedLevel::of_pack` folds too; `asset::plain`
    and `land` clear `folder` (`Landing::folders` keeps landed ids off
    the folders'); `place_in_pack` groups two pieces or more in a folder
    named after the asset (`Placement::folder`).
  - Tests: `tests/folders.rs` (group contiguous, scattered and nested,
    ungroup, move in and out, raise and lower at folder edges, distant
    layers, remove, `tidy`, equal trees writing alike, cycle-safe walks,
    the proptest `every_operation_keeps_the_rules`, a hidden folder in
    the bake, no re-decode, a locked folder, duplicate/merge/split/
    distant in folders, a folder's properties).
- **Tests** (`tests/`, one binary): `asset.rs` (issue #9: round trips
  through a folder at art scales 1 and 2, masks and groups joined or
  added, resampling across scales, the rectangle's rules, a picture kept
  whole, a raster's cells and back art, bad folders refused, the
  library), `placed.rs` (issue #73's bake, above), `pack.rs` (every bundled pack loads and
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
