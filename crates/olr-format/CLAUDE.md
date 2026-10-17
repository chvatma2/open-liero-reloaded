# olr-format

Pack authoring formats (spec `docs/superpowers/specs/2026-10-16-formats-design.md`):
the readable files editors, agents and people write; their canonical text;
compilers to `olr-data`'s runtime structs; lifts back. Plan 13a: mods.
Plan 13b: levels, themes, skins and the core pack. Plan 13c: these are the
packs' format (version 5); `olr-pack` reads and writes through them.

- **Layer 2, `std`.** Depends on `olr-data`, `olr-vfs`, `serde`, `ron`,
  `thiserror`, `png`, `zune-jpeg` and `zune-core` (exact pins, default
  features off). Tests also use `olr-pack` and `olr-sim`
  (dev-dependencies).
- **Canonical text** (`ron_io`): `to_canonical`/`from_canonical` are the only
  path between values and text: 4-space indent, no struct names,
  `implicit_some` with no header, defaults left out, a trailing newline.
  Reading canonical text and writing it again gives the same bytes. The
  line break is LF on every platform (`new_line` is set; ron's default is
  CRLF on Windows).
- **Defaults compare floats by bits** (`defaults.rs`): `-0.0` is not `0.0`.
- **Never panic on content**; errors are `FormatError` with the file and a
  line and column (parse) or a field path (invalid values).
- **Mods** (`mods/`; names in `docs/porting/olx-notes/mod-fields.md` §13):
  - `ModSource` is a mod as its files hold it: `mod.ron`,
    `weapons/<id>.ron`, `projectiles/<id>.ron` (shared projectiles) and
    `restrictions.ron`. `read_mod_files` and `write_mod_files` move it
    between a `ContentSource`/`ContentSink` and memory; `compile_mod` makes
    the runtime `ModDef`; `lift_mod` goes back.
  - **`deny_unknown_fields` on every type:** a typo is an error. A spawn's
    canonical field order puts its parameters before `shared`/`projectile`.
  - **Key rule.** A shared projectile's key is its file id. An inline
    projectile's key is its `id`, else derived from its place: the weapon
    id for a shot, then `<parent>.spawn` or `<parent>.trail`. Keys matter
    because `SimMod` keeps definitions that name their children by key.
    `lift_mod` shares a projectile that is referenced other than once.
  - **What LX56 cannot express is a compile error:** more than one effect
    per event besides `Spawn`, more than one `Spawn`, events naming
    different spawns, more than one entry in `spawns`, timer effects with
    `after_s` not above 0, a cycle between projectiles, an id used twice.
  - **Context defaults.** `Spawn<C>` takes four defaults from its context
    (`ShotContext`, `EventContext`, `TrailContext`): `relative_to_parent`,
    `speed_var_diagonal`, `random_rotation`, `inherit_velocity`. Imported
    files carry none of them.
  - `LIFTED_SPAWN` (`"spawn"`) is the name `lift_mod` gives a projectile's
    event spawn.
  - `Effect::Halt` is OLX's `Disappear` (7), renamed because it does not
    remove the projectile; `Effect::Remove` (OLX's `Disappear2`) does.
  - **Bounds** (errors, never panics): `MAX_INLINE_DEPTH` (32),
    `MAX_WEAPONS` (512), `MAX_SHARED` (4096), `MAX_EFFECTS` (16 per event),
    `MAX_COLOURS` (256), `MAX_AMOUNT` (1000, a spawn's `amount`; mirrors
    `olr-sim`'s). Shared roots and weapon shots both start at depth 1.
    Depth and occurrence limits after expansion are left to a later plan.
  - **`lift_mod(compile_mod(src))` is not the identity:** notes are dropped,
    spawn names become `"spawn"`, and sharing, effect order, an explicit
    default size and zero timers are normalised. Editors edit `ModSource`
    and never round-trip through `ModDef`.
  - **`write_mod_files` never deletes** files no longer in the `ModSource`:
    writing into an existing directory after removing a weapon leaves its
    file, which the next read rejects as "not listed". The editors' save,
    `olr_edit::pack::save`, writes the whole pack afresh, so the file goes.
    It validates every id before writing.
  - **`read_mod_files` reads only definitions** in `weapons/` and
    `projectiles/`: a `.ron` file that is not hidden (plan 13d). Anything
    else there (`gun.ron~`, `.DS_Store`) is not read, and
    `olr_pack::validate` warns about it; a `.ron` in a subfolder of
    `projectiles/` is an error.
  - **Tests.** `tests/equivalence.rs`: the bundled mods, lifted, written,
    read and compiled, give the import back exactly, the same `SimMod`, and
    the same RON text (with snapshots of some files in `tests/snapshots/`).
    `tests/roundtrip.rs`: a proptest that any authoring value round-trips
    through canonical text. `tests/hostile.rs`: deep nesting and mutated
    files give errors, never panics. Unit tests in `compile.rs`, `lift.rs`,
    `files.rs` and `types.rs` cover their rules (spawn defaults are tested
    in `types.rs`).
- **Skins** (`skins/`): `SkinSource` is `skin.ron` (all layout fields
  written), `skin.png` (a grid of frames, aim columns × walk rows),
  `tint.png` (grey: 0 keep, 255 player colour) and an optional `left.png`.
  `compile_skin` builds OLX's 672 × 18 sheet; only OLX's layout, no
  `left.png` and no partial tint compile ("not supported by this runtime
  yet"). `lift_skin` is exact for every bundled skin (tests/skins.rs).
- **Core** (`core_pack/`, not `core`: that name would shadow the `core` crate):
  `CoreSource` is `core.ron` (per sprite `frames` and `art_scale`, a `sounds`
  list, `colours` by OLX's key), `gfx/<id>.png` (alpha) and `sounds/<file>`.
  `CORE_LAYOUT` holds OLX's frame grids and art scales. `giblets` (32 × 8) and
  `giblet-stains` (16 × 4) are two sprites that `compile_core` composes into
  OLX's 32 × 12 `giblets`; `lift_core` splits it and is purely structural: the
  importer keys the old sprites once (`image_io::olx_sprite_to_alpha`) before
  lifting. `read_core_files_tolerant` leaves out a bad sprite or missing sound
  with its error, and drops the giblets pair together. `compile_core`
  checks that the grid divides the image; the drawing code keeps its own
  constants. tests/core_pack.rs checks the committed core pack is
  canonical (written again, the same bytes) and that the bundled mods'
  gfx are the PNGs the importer writes; nothing keys at load (plan 13c
  Task 8).
- **Levels** (`levels/`): `LevelSource` is `level.ron` (name, notes, `size`
  in world pixels, `art_scale`, theme id, `objects`), `material.png` (an
  indexed grid, `MATERIAL_PALETTE`: rock grey, background black, dirt brown,
  indexed by `Material as u8`), `front.png`, `back.png` (each `size` ×
  `art_scale`) and an optional `thumb.png`, passed through as bytes (read tolerantly: at most
  `MAX_THUMB_BYTES`, undecodable or larger is `None`; `read_level_header` reads
  `level.ron` only; `read_level_files_without_thumb` skips the thumbnail).
  `LevelObject::Spawn` is the only object; the runtime does not read it
  (spawns are `FindSpot`'s). Bounds: `MAX_SIDE` (4096), `MAX_ART_SCALE` (8),
  `MAX_OBJECTS` (4096); a material or art size that does not match is an
  error. Art decodes within `image_io::LEVEL_ART`. `encode_material` is
  `material.png`'s encoding, shared with a raster element's.
- **Level composition** (`levels/composition.rs`, plan 14a): `Composition`
  is `editor/composition.ron` (`COMPOSITION_FILE`; `EDITOR_DIR` is
  `editor`), its `Element`s bottom to top (`id` a `valid_id` slug, `name`,
  `visible`, `locked`, `ElementKind::Image { material, source, at, size,
  pixel_size, colours, dither, alpha_threshold } | Drawn { material } |
  Raster`). `CompositionSource` adds the files' bytes by pack path
  (`image_file`, `layer_file`, `raster_files`, `element_files`,
  `composition_paths`). `read_composition` reads `composition.ron` alone
  (`olr-cli level element list`, `validate`'s read paths);
  `read_composition_files` gives `None` without the
  file, refuses more than `MAX_ELEMENTS` before reading any element file
  and a missing file; `write_composition_files` refuses a bad id or source
  before writing anything, then writes only the files the elements name. `check_composition(src, size, art_scale)` checks without
  decoding pixels: count, ids (valid, unique, their layer files unshared
  and portable), image fields (`MAX_SIDE`, `MAX_PIXEL_SIZE`, colours 2-256,
  the box's art within `LEVEL_ART`, sources unique case aside), the
  elements' pixels together within
  `MAX_COMPOSED_PIXELS` (256 Mi), every file present, originals' header
  size within `IMAGES`, layers' exact size (a raster's material the
  level's, others size × art scale). A missing file is an error on
  `composition.ron`; a wrong size on the file itself. `check_elements(c,
  size, art_scale)` is its metadata part without the files (the editor's
  live check, plan 14b); `check_composition` calls it first.
- **Themes** (`themes/`): `ThemeSource` is `theme.ron`, `holes/<i>.png` with
  `holes/<i>.mask.png`, `green_ball.png` with `green_ball.mask.png`,
  `stones/`, `misc/`, `front_tile.png` and `back_tile.png`. A stamp is a
  meaning mask (`HOLE_PALETTE`: 0 untouched, 1 carve, 2 paint;
  `GREEN_PALETTE`: 0 untouched, 1 green, 2 rim) plus a paint layer.
  `compile_theme` writes OLX's colour-coded images back (black untouched,
  magenta carve, green, the paint colour), so `olr-terrain` and the
  simulation stay unchanged. Colour errors: paint that is black or magenta
  (it would read back as untouched or carve), a rim exactly green or
  magenta, a visible magenta sprite pixel or a `(254, 0, 254)` corner (it
  would change OLX's colour key). Bounds: `MAX_STAMP_SIDE` (64),
  `MAX_HOLES` (16), `MAX_THEME_IMAGES` (64, stones and misc each).
- **Level thumbnails** (`levels/thumb.rs`, moved from `olr-import-gfx` in
  plan 13d): `thumbnail(front)` box-filters a level's front art into
  `THUMB_SIZE` (160 × 96, `olr-pack` re-exports it), `letterbox`ed; the
  importer and `olr_edit::level::refresh_thumb` both use it.
- **`canonical_text(path, text)`** (plan 13d): one authoring file (by its
  path in the pack: `mod.ron`, `weapons/<id>.ron` with a valid id,
  `level.ron`, ...) parsed as its type and written again canonically; it
  checks that file alone.
- **`file_paths(source)`** in each kind's module: the paths its writer
  writes, without encoding anything (`tests/paths.rs` holds them equal for
  every bundled pack). `read_level_header` also gives `level.ron`'s
  `objects`.
- **Doc comments are the schema docs' source.** Every serialized field and
  variant has one stating its meaning, unit, limits and OLX source;
  `cargo xtask format-docs` turns them into `docs/formats/*.md`, fails on a
  missing one, and `check` fails when the pages drift.
- **Images** (`image_io`, moved from `olr-pack`, which re-exports it): the
  PNG codec, every decode bounded from the header (`IMAGES`, `LEVEL_ART`).
  `decode_indexed` takes 1, 2, 4 and 8-bit palettes and gives one byte per
  index; `encode_grey` and `decode_grey_within` serve the tint mask;
  `olx_sprite_to_alpha` applies OLX's colour key once. `png` is pinned to
  an exact version in the workspace: encoded bytes are content (hashes).
  `image_size` reads a PNG's or JPEG's size from its header (a JPEG's
  marker walk, every index checked); `decode_image_within` decodes either
  (JPEG through `zune-jpeg`, capped to the header's size, opaque RGBA) and
  refuses a JPEG with no end of image after its scan (zune would fill a
  cut scan in grey). A JPEG is turned by its EXIF `Orientation`
  (`jpeg_orientation` reads IFD0 of the first Exif APP1, both byte orders,
  every index checked; `orient` turns; plan 14b ruling R5); `image_size`
  stays the stored frame's size. Test data in `tests/data/` (README).
- **Tests** (`tests/`): `levels.rs`, `themes.rs`, `core_pack.rs` and
  `skins.rs` read every bundled pack in `content/packs` (format 5) and check
  it is canonical: written again, the same bytes. Unit tests in each module's files cover its checks. The
  bundled-content tests skip when `OLR_CONTENT_DIR` is not a directory.
  Every `check()` applies the pixel bound its reader decodes with
  (`image_io::check_size`), so a saved file always reopens.
