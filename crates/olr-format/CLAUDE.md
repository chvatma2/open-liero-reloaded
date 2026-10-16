# olr-format

Pack authoring formats (spec `docs/superpowers/specs/2026-10-16-formats-design.md`):
the readable files editors, agents and people write; their canonical text;
compilers to `olr-data`'s runtime structs; lifts back. Plan 13a: mods.
Plan 13b: levels, themes, skins and the core pack.

- **Layer 2, `std`.** Depends on `olr-data`, `olr-vfs`, `serde`, `ron`,
  `thiserror`, `png`. Tests also use `olr-pack` and `olr-sim`
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
    Depth and occurrence limits after expansion are left to plan 13c.
  - **`lift_mod(compile_mod(src))` is not the identity:** notes are dropped,
    spawn names become `"spawn"`, and sharing, effect order, an explicit
    default size and zero timers are normalised. Editors edit `ModSource`
    and never round-trip through `ModDef`.
  - **`write_mod_files` never deletes** files no longer in the `ModSource`:
    writing into an existing directory after removing a weapon leaves its
    file, which the next read rejects as "not listed". Plan 13c's save path
    must clear the mod files first. It validates every id before writing.
  - **Tests.** `tests/equivalence.rs`: the bundled mods, lifted, written,
    read and compiled, give the import back exactly, the same `SimMod`, and
    the same RON text (with snapshots of some files in `tests/snapshots/`).
    `tests/roundtrip.rs`: a proptest that any authoring value round-trips
    through canonical text. `tests/hostile.rs`: deep nesting and mutated
    files give errors, never panics. Unit tests in `compile.rs`, `lift.rs`,
    `files.rs` and `types.rs` cover their rules (spawn defaults are tested
    in `types.rs`).
- **Skins** (`skins/`): `SkinSource` is `skin.ron` (all layout fields written), `skin.png` (a grid of frames, aim columns × walk rows), `tint.png` (grey: 0 keep, 255 player colour) and an optional `left.png`. `compile_skin` builds OLX's 672 × 18 sheet; only OLX's layout, no `left.png` and no partial tint compile ("not supported by this runtime yet"). `lift_skin` is exact for every bundled skin (tests/skins.rs).
- **Core** (`core_pack/`, not `core`: that name would shadow the `core` crate):
  `CoreSource` is `core.ron` (per sprite `frames` and `art_scale`, a `sounds`
  list, `colours` by OLX's key), `gfx/<id>.png` (alpha) and `sounds/<file>`.
  `CORE_LAYOUT` holds OLX's frame grids and art scales. `giblets` (32 × 8) and
  `giblet-stains` (16 × 4) are two sprites that `compile_core` composes into
  OLX's 32 × 12 `giblets`; `lift_core` splits it and keys each sprite once
  (`image_io::olx_sprite_to_alpha`). `compile_core` checks that the grid divides
  the image; the drawing code keeps its own constants. tests/core_pack.rs also
  checks that the bundled mods' gfx key to alpha without change.
- **Levels** (`levels/`): `LevelSource` is `level.ron` (name, notes, `size`
  in world pixels, `art_scale`, theme id, `objects`), `material.png` (an
  indexed grid, `MATERIAL_PALETTE`: rock grey, background black, dirt brown,
  indexed by `Material as u8`), `front.png`, `back.png` (each `size` ×
  `art_scale`) and an optional `thumb.png`, passed through as bytes.
  `LevelObject::Spawn` is the only object; the runtime does not read it
  (spawns are `FindSpot`'s). Bounds: `MAX_SIDE` (4096), `MAX_ART_SCALE` (8),
  `MAX_OBJECTS` (4096); a material or art size that does not match is an
  error. Art decodes within `image_io::LEVEL_ART`.
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
- **Images** (`image_io`, moved from `olr-pack`, which re-exports it): the
  PNG codec, every decode bounded from the header (`IMAGES`, `LEVEL_ART`).
  `decode_indexed` takes 1, 2, 4 and 8-bit palettes and gives one byte per
  index; `encode_grey` and `decode_grey_within` serve the tint mask;
  `olx_sprite_to_alpha` applies OLX's colour key once. `png` is pinned to
  an exact version in the workspace: encoded bytes are content (hashes).
- **Tests of 13b** (`tests/`): `levels.rs`, `themes.rs` and `core_pack.rs`
  lift every bundled pack, write, read and compile it and compare with the
  import; `skins.rs` checks every bundled skin draws the same after a
  lift. Unit tests in each module's files cover its checks. The
  bundled-content tests skip when `content/packs` is absent.
