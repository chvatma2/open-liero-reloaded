# olr-format

Pack authoring formats (spec `docs/superpowers/specs/2026-10-16-formats-design.md`):
the readable files editors, agents and people write; their canonical text;
compilers to `olr-data`'s runtime structs; lifts back. Plan 13a: mods.
Plan 13b: levels, themes, skins and the core pack. Plan 13c: these are the
packs' format (version 5); `olr-pack` reads and writes through them.

- **Layer 2, `std`.** Depends on `olr-data`, `olr-vfs`, `olr-look` (the
  look's runtime types), `olr-terrain` (`GlowLayers`, plan 15a), `serde`,
  `ron`, `thiserror`, `png`, `zune-jpeg` and `zune-core` (exact pins,
  default features off). Tests also use `olr-pack` and `olr-sim`
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
    `restrictions.ron`. `mod.ron`'s optional `loadouts` (issue #5) are the
    shipped loadouts by preset name (`ShippedLoadout`); `compile_mod`
    refuses one naming no preset, named `Default` or not a clean loadout
    name, with a loading time outside 0-500, or more than `MAX_LOADOUTS`
    (64). `read_mod_files` and `write_mod_files` move it
    between a `ContentSource`/`ContentSink` and memory; `compile_mod` makes
    the runtime `ModDef`; `lift_mod` goes back. A loadout's error path is
    `loadouts["<name>"]`.
  - **Every error** (issue #16): the compiler records an error and goes
    on with a placeholder (`Compiler::fail`, `note`), so `mod_errors(src)`
    lists them all, each at its file and field path (`olr_edit::mods`'
    check); `compile_mod` returns the first. `valid_id` (an id's slug
    rule) is public for the editors.
  - **`editor/mod.ron`** (issue #17, `MOD_EDITOR_FILE`, `ModEditorFile {
    base }`): the mod editor's own file, the pack id of the mod a mod was
    copied from (its Original values). Authoring only (`olr_pack::
    is_authoring`: not hashed, never downloaded); `canonical_text` reads it.
  - **`deny_unknown_fields` on every type:** a typo is an error. A spawn's
    canonical field order puts its parameters before `shared`/`projectile`.
  - **Key rule.** A shared projectile's key is its file id. An inline
    projectile's key is its `id`, else derived from its place: the weapon
    id for a shot, then (issue #64, `olr_data::mod_def::child_key`) the
    chain as `s` (spawn) and `t` (trail), a run its letter and count:
    `gun.s`, `gun.s2t`; a parent ending in no such chain starts one
    (`crackler.trail` → `crackler.trail.s`). Keys matter
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
  - **Looks** (plan 15a Task 3): `Weapon::look` (`WeaponLookSource`, its
    `muzzle_light`; issue #69, a Beam weapon's `beam`, refused on any
    other: "only a `Beam` weapon has a beam look") and `Projectile::look` (`ProjectileLookSource`: `glow`
    0-100, a `light`, an `end_flash`, issue #15's `explosion`, issue #42's
    `explosion_heat` and `heat`), and
    `ModSource::look` (`mod.ron`'s `look`, `ModWideLookSource`: issue #15's
    default `explosion` and issue #42's default `explosion_heat` of every
    projectile without one), presentation
    only. `compile_mod_look` gives `ModLookSource`: each projectile's look by its
    `ModDef` key (shared ones included), the weapons' in order, empty when no weapon has one (readers
    index with `get`), and the mod-wide `explosion` and `explosion_heat`. `compile_mod` checks looks in the same walk
    (`compile_all`), so a bad look fails the mod. `lift_mod` gives every
    look `None` (`ModDef` carries none; Task 3 ruling), and the roundtrip
    proptest generates none: `tests/look.rs` covers looks' limits.
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
  `tint.png` (grey: 0 keep, 255 player colour), an optional `glow.png`
  (issue #12: grey, 0 none to 255 full, the sheet's size) and an optional
  `left.png`; `skin.ron`'s optional `light` (`SkinLightFile`: intensity
  1-`MAX_SKIN_LIGHT_INTENSITY` percent, reach 1-`MAX_SKIN_LIGHT_REACH`
  world pixels, `olr_look::engine`'s caps; above them refused).
  `compile_skin` builds OLX's 672 × 18 sheet (`OLX_SKIN`: art scale 2,
  20 × 18 frames) or its double, 1344 × 36 (`DOUBLE_SKIN`: art scale 4,
  40 × 36, `FrameLayout::DOUBLE`), both 7 × 3 frames, with the glow mask
  (empty without `glow.png`; glow on a transparent pixel is dropped) and
  the light in `SkinDef`; other layouts, `left.png` and partial tint do
  not compile ("not supported by this runtime yet"). `lift_skin` lifts
  either sheet; it is exact for every bundled skin (tests/skins.rs) and
  for double skins (`a_double_skin_compiles_and_lifts_back`). The reader
  checks `light` before decoding any image, and `glow.png`'s header size
  against the layout's before decoding it.
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
- **Distant layers** (`levels/`, issue #11; the page
  `docs/formats/level.md`): `LevelSource::distant`
  (`olr_data::level::DistantLayer`s, far to near), `level.ron`'s
  `distant` list (`DistantEntry { depth, at }`, left out when empty) and
  `distant/<i>.png` (`DISTANT_DIR`, `distant_file(i)`; RGBA at art
  resolution, each side a multiple of the art scale). Issue #62: a
  layer's `DistantLayer::glow` is `DistantEntry`'s `glow` (percent 1-100,
  0 none, left out) and `glow_group` (an id of the look's glow groups,
  read as 1 + its index: an unknown id reads as 0) and its mask
  `distant/<i>-glow.png` (`distant_glow_file(i)`, grey, the image's size,
  read only for a glowing layer); `check_distant_glows` (in `check`)
  refuses a strength outside 1-100, a group the look lacks and a mask of
  another size; the lenient reader plays the layers without glow ("…:
  the level's distant layers play without glow"), and a dropped look
  drops their glow; `file_paths` lists the masks
  (`distant_glows_round_trip_and_name_the_looks_groups`). `check_distant_layers`:
  at most `MAX_DISTANT` (16), depth 1-99, `at` within `MAX_DISTANT_AT`
  (4 × `MAX_SIDE`) of the origin, each image within `MAX_SIDE` × art scale
  a side and `LEVEL_ART`, their pixels together within
  `LEVEL_ART.max_pixels`; `file_paths` lists the files. The strict readers
  refuse a bad layer; `read_level_files_lenient` drops them all with a
  warning ("…: the level plays without its distant layers"). `back.png`
  is alpha 0 where a see-through back pixel lets them show (the bake's,
  `olr-edit`). A composition's `Element::distant` (depth, `None` for an
  ordinary element; `is_distant`): `check_elements` allows it on an image
  or a plain drawn (not mixed, no back art) Background element, never
  clear (it may glow, issue #62), 1-99, every distant element below every other
  (`check_distant`), at most `MAX_DISTANT`, `at` within `MAX_DISTANT_AT`,
  their art pixels within `LEVEL_ART.max_pixels`; an asset carries none
  (`check_asset`). The thumbnail (`thumb.rs`' `seen`) shows them from the
  level's centre in see-through pixels, lit as ambient plus (1 − depth) of
  the rest (`the_thumbnail_shows_distant_layers`). Tests in `files.rs`
  (`distant_layers_round_trip`, `bad_distant_layers_are_errors_and_played_without`,
  `a_level_without_distant_layers_writes_none`) and `composition.rs`
  (`distant_elements_follow_their_rules`).
- **Level looks** (`levels/`, plan 15a Tasks 2, 6 and 14b; spec
  `docs/superpowers/specs/2026-10-18-look-system-design.md` §Data):
  `LevelSource` gains `look` (`level.ron`'s `look`, a `LookSource`),
  `glow` (`GlowLayers`, re-exported from `olr-terrain`: the four `look/`
  images, `LOOK_FILES`, all or none: `GLOW_FILE` and `GLOW_BACK_FILE`
  strength per art pixel, `GROUPS_FILE` and `GROUPS_BACK_FILE` a group per
  world pixel, 0 no group and `n` the look's `glow_groups[n - 1]`) and
  `clear` (`CLEAR_FILE`, `look/clear.png`: a flag per world pixel, 0
  opaque, any other value a clear solid; optional on its own; the bake
  writes 255). All are 8-bit grey PNGs (`encode_grey`); a 16-bit one reads
  normalised to 8 bits (`decode_grey_within`, Task 2 ruling), each size
  checked from the header before decoding. `LevelHeader` gains `look`.
  Plan 15c: glowing pixels are a level's only light: `LevelObject::Light`
  is gone (a `Light(..)` object is an error naming it,
  `a_light_object_is_an_error`), so `LevelObject` is `Spawn` alone
  (helpers `spawn`, `at`, `set_at`, `spawn_point`).
  `compile_look(src)` is the runtime `olr_look::LevelLook` (`None`
  without a look); `check` runs it and checks the glow
  fits and names only groups the look has, and the clear flags' count.
  `read_level_files_lenient` (playing; `olr-pack`'s `read_level`) leaves
  out, with a warning each, glow or clear images that do not read or fit
  and a look that does not check (which drops its glow too);
  `level.ron` that does not parse still fails, and the strict readers stay
  `pack check`'s.
- **Looks** (`look.rs`, plan 15a): the shared authoring types of levels and
  mods and their compilers to `olr-look`. Every value is an integer
  (percent, milliseconds, world pixels); colours are the format's hex
  `Colour` strings and must be opaque (Task 2 ruling). `LightSource` (a
  mod's light, played as a glowing disc since plan 15c)
  (colour, intensity 0-`MAX_INTENSITY` 1000, `ReachSource::Pixels(1 to
  MAX_REACH 4096) | Unlimited`, `AnimationSource`, `phase_ms`),
  `AnimationSource` (`Steady`; `Pulse` with period 1-`MAX_PERIOD_MS`,
  `PulseShape`; `Flicker` speed 1-1000, depth 0-100; `Keys` of 1-`MAX_KEYS`
  (64) `KeySource`s, written `(at_ms, level)` or `(at_ms, level,
  "#rrggbb")`, times not decreasing), `LookSource` (`ambient`
  `AmbientSource`, `max_light` 100-200, `light_steps` 0 or 2-32,
  `softness` 0-4, `light_soak` 0-`olr_look::MAX_SOAK` default 8,
  `solid_floor` `SolidFloorSource` default white at 30 % (both plan 15a
  Stop 1b; issue #10: `occlusion`, an optional `OcclusionSource` of
  `strength` 1-100 (0 refused: absent is off) and `reach`
  `olr_look::MIN_OCCLUSION_REACH`-`MAX_OCCLUSION_REACH`, 2-32, default
  `DEFAULT_OCCLUSION_REACH` 12; plan 15c removed `shadows`: an unknown field,
  `shadows_is_an_unknown_field`), `grade` `GradeSource`, `glow_groups`
  at most `MAX_GROUPS` 255), `GlowGroupSource` (an id by `valid_group_id`,
  a slug of `[a-z0-9_.-]` of 1-64 bytes not starting with `.`, unique;
  `GlowColourSource::Own | Tint`, intensity, reach, animation, phase),
  `ProjectileLookSource`, `WeaponLookSource`, issue #15's
  `ExplosionLightSource` (colour, intensity 0-1000, `radius` 1-
  `olr_look::engine::MAX_EXPLOSION_RADIUS` 16, `reach` `Pixels` 1-
  `MAX_EXPLOSION_REACH` 4096 only, an optional `animation` (none: the
  engine's flash, `engine::explosion_flash`, `fitted`) and a `phase_ms`
  only with one), issue #42's `ExplosionHeatSource` (`amount` percent of
  the engine's heat 0-`olr_look::engine::MAX_EXPLOSION_HEAT_AMOUNT` 1000,
  default 100; `radius` 1-`MAX_EXPLOSION_HEAT_RADIUS` 48 world pixels,
  default none: the engine's) and `ProjectileHeatSource` (`amount` a tick
  1-`MAX_PROJECTILE_HEAT` 100, `radius` 1-`MAX_PROJECTILE_HEAT_RADIUS` 16,
  both required), and `ModWideLookSource`. `LookSource`'s `Default` is
  written by hand to equal its serde defaults (`the_default_look_is_the_empty_file`).
  `compile_reach`, `compile_animation`, `compile_light`,
  `compile_look_source` (a level's look),
  `compile_projectile_look`, `compile_weapon_look`,
  `compile_explosion_light`, `compile_explosion_heat`,
  `compile_projectile_heat` (issue #42), `compile_beam_look` (issue #69:
  `BeamLookSource { light, tip_light, heat: BeamHeatSource { amount
  0-100, radius 1-16 } }`, the lights' reach `Pixels` 1-
  `olr_look::engine::MAX_BEAM_REACH` 256, never `Unlimited`; each left out
  the engine's, 0 off; `tests/look.rs`'
  `a_beam_look_round_trips_and_compiles`, `beam_look_limits`): each value
  checked, an error naming its field path.
- **Surface materials** (`materials.rs`, `levels/`, issue #37; spec
  `docs/superpowers/specs/2026-10-22-materials-and-heat-design.md`; the
  page `docs/formats/material.md`; STATUS.md "Surface materials (issue
  #37)"): `MaterialSource { id, name, notes, light_soak: Option<u8>,
  heat: Option<HeatSource> }` (issue #57: `light_soak` 0-`MAX_SOAK` 12,
  none the level's `look.light_soak`; `light_soak_round_trips`)
  (`HeatSource { sensitivity 0-100 default 100, capacity 10-1000,
  conductivity 0-1000, cooling 1-1000, look: HeatLookSource { ramp of
  1-16 `RampKeySource { temperature 1-1000 rising, colour opaque, glow
  0-100 }` (empty: the engine's `olr_look::LAVA_RAMP`), light 0-100
  default 100 } }`; heat values percent of stone's), one file `<id>.ron`
  (`valid_material_id`, `material_file`, `read_material`: the id is the
  file's stem; `material_text`); the limits are `olr-look`'s constants,
  re-exported. `compile_material`/`compile_materials` give
  `olr_look::SurfaceMaterial`s (at most `MAX_MATERIALS` 255, ids
  unique). `LookSource::materials` (`level.ron`'s `look.materials`,
  copies of the library's). `LevelSource::surface` (`SurfaceMaps`, `olr-look`'s:
  `SURFACE_FILE` `look/surface.png` and `SURFACE_BACK_FILE`
  `look/surface-back.png`, 8-bit grey at world resolution, both or none,
  `v` the look's `materials[v - 1]`, 0 none; `check_surface`) and
  `distant_surfaces` (`DistantEntry::surface`, one per distant layer,
  trailing `None`s trimmed; `check_distant_surfaces`). The lenient reader
  plays bad maps or distant surfaces without, with a warning. A
  composition's `Element::{surface, back_surface, surface_map,
  back_surface_map}`: material ids by `valid_material_id`, a back surface
  or map only with back art (`has_back_art`: a drawn layer with `back`, a
  raster), a distant layer no map; maps `surface_file(id)`
  (`editor/surface/<id>.png`) and `surface_back_file(id)`
  (`<id>.back.png`) at `surface_map_size` (the box, a raster's the level)
  in world pixels, 0 the element's default, unshared, counted in the
  pixel budget. `check_surfaces(c, look)` (ids in the look) and
  `check_surface_maps(src, size, materials)` (decoded, values within) are
  `validate`'s; `check_asset` refuses any surface (materials 2/10).
  Tests: `materials.rs`' units, `files.rs`' `surface_maps_round_trip`,
  `bad_surface_maps_are_errors_and_played_without`,
  `distant_surfaces_round_trip_and_name_the_looks_materials`,
  `a_level_without_materials_writes_no_surface`, `composition.rs`'
  `element_surfaces_follow_their_rules`.
- **Level composition** (`levels/composition.rs`, plan 14a): `Composition`
  is `editor/composition.ron` (`COMPOSITION_FILE`; `EDITOR_DIR` is
  `editor`), its `Element`s bottom to top (`id` a `valid_id` slug, `name`,
  `visible`, `locked`, `ElementKind::Image { material, source, at, size,
  pixel_size, colours, dither, alpha_threshold, mask } | Drawn { material,
  at, size, mixed, back } | Raster`; plan 14d: a drawn layer's box, `at`
  and `size` (`None`: the level's), `drawn_box`; a mixed layer's
  `<id>.material.png` and a backed one's `<id>.back.png`, `drawn_files`; an
  image's mask `editor/masks/<id>.png`, `mask_file`; all optional, so no
  format bump; `ElementKind::drawn(m)` a plain level-sized layer). `CompositionSource` adds the files' bytes by pack path
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
  Plan 15a (Tasks 5 and 14b): `Element::glow` (`ElementGlow { strength
  1-100, group, mask }`; a group id by `valid_group_id`; a mask is
  `glow_file(id)`, `editor/glow/<id>.png`, grey, the element's box × art
  scale, listed by `element_files`) and `Element::clear` (its rock and
  dirt let light through; collisions unchanged), both optional and on the
  element, not its kind (Task 5 ruling); a raster element can do neither
  (`check_elements`). `element_box(kind, size)` is an image or drawn
  element's box (`None` for a raster). `check_glow_groups(c, look)`: every
  element's glow names a group of the level's look (an error on
  `composition.ron`); `olr-pack`'s `validate` calls it.
  Issue #59, layer folders (STATUS.md "Layer folders (issues #59, #60,
  #61)"): `Composition::folders` (`Folder { id, name, visible, locked,
  collapsed, parent }`, `Folder::new`) and `Element::folder` (its
  innermost), both left out when empty, so a composition without folders
  writes byte for byte as before. `check_folders` (the end of
  `check_elements`) holds the rules, each an error naming the folder: at
  most `MAX_FOLDERS` (256), ids valid and unique among elements and
  folders, every `parent` and `folder` known, no cycle, at most
  `MAX_FOLDER_DEPTH` (16) deep, every folder's elements (its subfolders'
  too) one contiguous run, never empty, all distant or none, and
  `folders` ordered by run start, a parent before its children.
  `folder_chain(c, folder)` walks up the parents, innermost first,
  stopping at an unknown id, a repeat and `MAX_FOLDER_DEPTH`: safe on an
  unchecked composition (`read_composition`, which also refuses more
  than `MAX_FOLDERS`). `check_asset` refuses an element naming a folder.
  Tests: `composition.rs`' `nested_folders_round_trip_in_canonical_order`,
  `folder_rules_are_refused_naming_the_folder`,
  `folder_chains_stop_on_a_cycle_and_at_the_depth_limit`,
  `compositions_without_folders_write_as_before` (every committed
  composition).
- **Assets** (`assets.rs`, issue #9; the level editor's library, not a
  pack kind): `AssetFile` is `asset.ron` (`ASSET_FILE`: name 1 to
  `MAX_ASSET_NAME` (64) characters, `art_scale`, `size`, composition
  `Element`s placed from its top-left, the `glow_groups` they name);
  `AssetSource` adds the elements' files by asset path (`asset_files`: a
  composition's `element_files` without `editor/`) and `thumb.png`
  (`ASSET_THUMB_FILE`, read tolerantly as a level's). `check_asset`
  checks the name, art scale and size, that it holds no raster, its
  groups (`compile_look_source`, every one named by an element,
  `check_glow_groups`) and the elements with `check_composition` over
  `as_composition` (the asset's size as the level's). `read_asset_files`
  refuses a bad id or source before reading any file and more than
  `MAX_ASSET_BYTES` (256 MiB) in all; `write_asset_files` writes only the
  files the elements name. Unit tests in `assets.rs`; the page is
  `docs/formats/asset.md`.
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
  importer uses it. `level_thumbnail(level)` (the user's note, 2026-10-19;
  `olr_edit::level::refresh_thumb` uses it) draws a level with a look lit:
  a CPU model at clock 0 of the ambient and the glow light (a grid of cells two thumbnail pixels a side,
  each casting its glow by the angle it fills, `side / (2π d)` at most 1,
  faded by its reach), each art sample never below its glow nor, solid,
  the solid floor, capped at max light, the mean graded; no soak, softness
  or steps. Without a look, or with one that does not compile, it is
  `thumbnail(front)` (`tests/look.rs`: the plain one kept, a dark level,
  glow spilling, the solid floor; the occlusion left out, issue #10).
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
  it is canonical: written again, the same bytes. `look.rs` (plan 15a): the
  spec's level and mod examples round-trip, a level without a look writes
  none, the glow and clear images round-trip (clear on its own; a wrong
  size fails strictly and warns leniently), `limits_are_enforced`,
  `hostile_look_files_are_errors_never_panics`, look files hashed and known
  to `olr_pack::validate`, inline projectile looks under their key,
  `bundled_mods_have_no_look`, `the_default_look_is_the_empty_file`,
  `solids_defaults_are_the_stop_1_verdict`, `a_light_object_is_an_error`,
  `shadows_is_an_unknown_field`; issue #15:
  `explosion_lights_round_trip_and_compile` (a projectile's and the mod's,
  the `ModDef` unchanged), `explosion_light_limits` (each cap, `Unlimited`,
  a phase without an animation, the mod's naming `mod.ron`); issue #42:
  `heat_round_trips_compiles_and_falls_back_to_the_mods` (a projectile's
  and the mod's, the `ModDef` unchanged, canonical text leaving the default
  amount out), `heat_limits` (each bound and cap, 0 % none, the mod's
  naming `mod.ron`, `heat`'s fields required); issue #10:
  `occlusion_round_trips_and_is_bounded`,
  `occlusion_changes_the_content_hash`, `the_thumbnail_leaves_occlusion_out`. Unit tests in each module's files cover its checks. The
  bundled-content tests skip when `OLR_CONTENT_DIR` is not a directory.
  Every `check()` applies the pixel bound its reader decodes with
  (`image_io::check_size`), so a saved file always reopens.
