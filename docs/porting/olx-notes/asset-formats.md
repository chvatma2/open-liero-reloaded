# Levels, skins, themes, the core pack and asset conventions (research for plan 13b)

Repo state: `main` at `edbb651`. OLX source: `target/olx-src/d1ad953f/`.
Bundled packs: `content/packs/`. OLX's original files: `target/olx/d1ad953f…/share/gamedir/`
and `content/olx-0.58-rc5/skins/`. I measured pixels with a stdlib-only PNG decoder
in the scratchpad (`pngdec.py`, `an_skins.py`, `an_theme_core.py`). No repository file was changed.

---

## 0. Headline findings (read first)

1. **Levels and themes can be exact.** For every bundled level and theme, `compile(lift(today))`
   can rebuild today's `Level`/`Theme` byte for byte. No bundled image is ambiguous: no
   (254,0,254) key, no partial alpha, and no paint colour that collides with a meaning colour.
2. **Skins cannot rebuild today's `Skin` sheet byte for byte under the spec's grid layout.** The new
   `skin.png` is cells of exactly 20×18 with no spacing. Today's sheet is 672×18 with 32-px frames,
   and the sprite sits 4 px in. Measured on the 718 bundled skins:
   - 401 have visible pixels in the spacing columns. Offsets 0..4 and 24..32 of a frame are never drawn facing right.
   - **288 have visible pixels in column offset 24**. Today's left-facing quirk *draws* that column
     (`olr-content/src/skins.rs:38-45`).

   So the skin switch and the dropped mirror quirk must ship together. Until then, equivalence can only be
   per frame: the 20×18 window at `f*32+4` and the colorized output.
3. **Every transparent pixel of today's `skin.png` keeps its RGB under alpha 0.** That is 6,285,386 such
   pixels in all 718 skins, because the importer only clears alpha (`olr-import-gfx/src/skin.rs:59-63`). `Skin::colorize` maps them
   to `(0,0,0,0)` anyway (`olr-data/src/skin.rs:67`). A canonical alpha PNG with zeroed RGB therefore
   gives a different `Skin` but an identical colorized result.
4. **The spec's skin example says `art_scale: 1`, but OLX skins are 2× art.** Every worm sheet is
   drawn with `px_per_world` 2 (`olr-scene/src/assets.rs:86,123`). Imported skins must declare `art_scale: 2`.
5. **Core sprites have mixed art scales, and one file has two grids.** `muzzle` is 1× (`assets.rs:84`, `worm.rs:165`).
   `giblets.png` (32×12) holds 4 gib frames of 8×8 at 2× in rows 0..8 (`olr-fx/src/draw.rs:78-81`). It also holds 4 stain
   frames of 4×4 at 1× at y=8 (`olr-fx/src/lib.rs:221-223`, `olr-terrain/src/color.rs:222-252`).
   One `frames: (c, r)` grid cannot describe both.
6. **Theme stamp meanings reach the simulation.** In holes, carve, untouched and paint all change
   the material and the dirt counts (`PlaceDirt` counts +1 under carve and +2 under paint). In the green ball, green, pink and rim
   change material and **RNG consumption** (one `rand()` per green pixel placed). Only the paint and rim
   *colours* are cosmetic.
7. **Nothing at runtime reads `LevelDef.spawns`.** It flows into `LoadedLevel.spawns`
   (`olr-content/src/maps.rs:24,117`) and stops there. **Nothing reads stones, misc or `back_tile`** either.
8. **`olr-format` has no PNG codec** (`crates/olr-format/Cargo.toml`). The codec and its bounds
   (`olr-pack/src/image_io.rs`) are in `olr-pack`, which plan 13c makes read *through* `olr-format`.
9. **No bound limits stamp size.** Holes and the green ball are limited only by `IMAGES` (16 Mpx). Carving costs
   one pass over the stamp's area per carve, and themes are downloaded as required content. A hostile theme
   could stall ticks, so the 13b validator should add a named bound.
10. **13a left mod `gfx/` untouched.** The spec's "one sprite description" (§4) and the §5 key-to-alpha
    conversion cover mod gfx too. One mod image does not fit "frames = width / height" exactly:
    `olx-mw-1-0/gfx/plasmaexp2.png` is 97×32, so 3 frames with 1 px left over.

---

## 1. Shared runtime pieces

### `olr_data::image` (`crates/olr-data/src/image.rs`)
- `Rgba { r, g, b, a }` (:9-14). `same_rgb` compares RGB and ignores alpha (:28-30). The constants are
  `MAGENTA` (255,0,255,255), `BLACK` and `TRANSPARENT` = (0,0,0,0) (:23-25).
- `RgbaImage { width, height, pixels: Vec<u8> }` is row-major RGBA8 (:35-39). It has `new` (transparent), `from_raw`, `get`, `set` and `crop`.
- **`olx_color_key()`** (:96-103) is OLX `SetColorKey`'s corner rule (`src/client/GfxPrimitives.cpp:114-150`).
  If *any of the image's own four corners* has RGB (254,0,254), the key for the whole image is (254,0,254),
  and pure magenta is then an ordinary colour. Otherwise the key is (255,0,255). An empty image gives `MAGENTA`.
  **The rule is decided per image, from that image's corners.**
- **`key_transparent()`** (:107-114) sets every pixel whose RGB equals the key to `[0,0,0,0]`. Alpha is ignored
  in the comparison.

### `olr_data::manifest` (`manifest.rs`)
- `PACK_FORMAT_VERSION = 4` (:15). Format 2 added the theme's green ball, 3 added restrictions, 4 added level thumbs. The spec moves it to 5 in 13c.
- `PackKind {Mod, Level, Skin, Theme, Core}` (:18-25). `PackManifest {format_version, id, name, version, kind, authors, origin, license}` (:39-48).

### `olr-pack` bounds (`crates/olr-pack/src/image_io.rs`, `ron_io.rs`)
- `MAX_FILE_BYTES` = 64 MiB (`ron_io.rs:12`). `MAX_RON_BYTES` = 4 MiB (`ron_io.rs:16`).
- `ImageBound { max_side, max_pixels }` is checked from the header before decoding (`image_io.rs:33-38`, `reader` :58-74).
  - `IMAGES` = side 16384, 16 Mi px (:42,47). Used for sprites, skins and theme images.
  - `LEVEL_ART` = side 32768, 64 Mi px (:54). Used for `front`, `back`, `material` and the hi-res chunk.
- `decode_rgba_within` (:85-115) accepts any PNG kind and `normalize_to_color8` (palette+tRNS → alpha, 16-bit → 8).
  It does no gamma or ICC handling and expands grey to RGB. `decode_indexed_within` (:140-153) **accepts only 8-bit indexed**.
  `encode_rgba` (:6-16) writes RGBA8 at the png crate's default compression. `encode_indexed` (:119-130) writes indexed 8.
  Both are deterministic for a given `png` crate version.
- `png_size` (:20-27) reads the size from IHDR. `declared_png` (:159-173) is a test helper for decompression bombs.

### Content hash (`crates/olr-pack/src/hash.rs:18-34`)
SHA-256 over every file in sorted path order (`olr_vfs::walk` sorts, `olr-vfs/src/lib.rs:84-95`). Each file adds
`path, 0, len as LE u64, bytes`. The hash skips `pack.ron`, hidden paths and `PRESENTATION_FILES = [thumb.png]` (root only, :9).
Any change to a file's bytes changes the hash, so the new format changes every hash (re-bless in 13c, as the spec says).

---

## 2. Levels

### 2.1 Runtime types (`crates/olr-data/src/level.rs`)

| Type / field | Meaning | Readers |
|---|---|---|
| `Material {Rock=0, Background=1, Dirt=2}` (:12-18) | OLX/Gusanos material indices (`src/gusanos/level.cpp:80-145`). The PNG stores them directly. `from_index` (:21-28) rejects 3 and above. `from_lx_flags` (:32-42): rock wins over dirt, anything else is background. | `olr-terrain` (`Terrain` stores them as `u8`, `lib.rs:31-64`), `olr-pack::level_pack` |
| `MaterialGrid {width, height, cells}` (:47-51) | One material per world pixel, row-major | `olr_terrain::Terrain::from_grid` (`lib.rs:56-64`) → sim; `ColorMap::new` via `Terrain`; `olr-content::LoadedLevel` |
| `SpawnPoint {x: i32, y: i32}` (:89-93) | World pixels | **Nobody.** It is carried into `LoadedLevel.spawns` (`olr-content/src/maps.rs:24,117`) and never read. The sim respawns with `FindSpot` (`olr-sim/src/respawn.rs`). |
| `LevelDef {name, width, height, art_scale, theme, spawns}` (:96-108) | `level.ron`. `art_scale` is art pixels per world pixel for front and back. `theme` is the theme pack id. Empty `spawns` means random spawning. | `read_level_def` (menus, `olr-content/src/catalog.rs:148-159`); `load_level` |
| `Level {def, material, front, back}` (:112-118) | `front` is the foreground art. **Its documented invariant: where the material is background, it equals `back`.** | `olr-content::LoadedLevel` (`maps.rs:16-25,104-119`) |

`LoadedLevel` (`olr-content/src/maps.rs:16-25`) is `{name, material, front, back, art_scale, theme, spawns}`.
`check_art_size` (:87-99) applies `MAX_ART_SCALE` = 8 (:78) and `MAX_COLOR_MAP_BYTES` = 256 MiB (:82).

**Who uses the level art:**
- `LoadedLevel::color_map` (`olr-content/src/color.rs:20-28`) calls `ColorMap::new(Terrain, front, back, art_scale, ThemeArt)`.
- `ColorMap::new` (`olr-terrain/src/color.rs:128-158`) uses `fit()` (:102-116). It copies whatever part fits, pads with black,
  and **forces alpha to 255**, so the alpha of `front` and `back` never matters at runtime.
- The composite starts as `front`. Carving copies `back` blocks in (`copy`, :440-446).
- `rebuild_area` (:260-279) redraws from the material: back for background, `front_tile` for dirt, front for rock.
- The material goes to the sim via `Terrain::from_grid` in `match_setup` (`olr-content/src/sandbox.rs:96`),
  `replay_assets_from` (`replay.rs:96-103`), `olr-scenario/src/lib.rs:150-153` and `olr-content/src/matches.rs:189`.
- Thumbnails: `olr-import-gfx/src/thumb.rs:42-45` box-filters `level.front` (RGB only) into 160×96 at import
  (`olr-content/src/import.rs:337`).

### 2.2 Today's pack (`crates/olr-pack/src/level_pack.rs`)

| File | Format | Read check |
|---|---|---|
| `level.ron` | `LevelDef` RON (≤ 4 MiB) | serde |
| `material.png` | **8-bit indexed**, palette `MATERIAL_PALETTE` = rock (128,128,128), bg (0,0,0), dirt (139,90,43) (:39) | `decode_indexed_within(LEVEL_ART)` (:151). Each index goes through `Material::from_index`; index 3 and up is "unknown material index v at (x,y)" (:153-162). The palette colours are **ignored**. Any other PNG kind, including indexed at 1, 2 or 4 bits, is an error ("palette"). |
| `front.png`, `back.png` | RGBA8 (`encode_rgba`) | `decode_rgba_within(LEVEL_ART)` (:124-127) |
| `thumb.png` (optional) | RGBA8 160×96 (`THUMB_SIZE` :26) | Not read by `read_level` (`thumb: None`, :166). `read_level_thumb`/`decode_thumb` (:102-121): ≤ `MAX_THUMB_BYTES` 1 MiB (:35) and ≤ `MAX_THUMB_SIDE` 512 (:30), checked from the header. `validate` gives a warning only (`validate.rs:90-95`). |

`check_dims` (:50-67) checks:
- `art_scale` ≥ 1;
- material = `width×height`;
- `width×art_scale` and `height×art_scale` do not overflow (checked multiply);
- front and back are exactly `size × art_scale`.

There is no upper bound on `art_scale` in `olr-pack`. That is `olr-content`'s `MAX_ART_SCALE`. The writer refuses what the reader refuses (`write_level` :69-87).

### 2.3 Importer (`crates/olr-import-lxl/src/lib.rs`)
- `parse_lxl` (:79-187) accepts:
  - the ids "LieroX Level" and "LieroX CTF Level" (the CTF data's 12 bytes are skipped with a warning, :138-141);
  - version 0 only;
  - a size of 1..=4096 per side (`MAX_SIDE`, :22);
  - `MPT_IMAGE` only (:98-101).
- It reads the zlib body (back RGB, front RGB, flags, `n*7` bytes; bounded inflate :111-119).
  - `back` = back RGB, opaque.
  - `front` = back RGB **where `flags & PX_EMPTY`**, else front RGB (:121-133).
  - `material = Material::from_lx_flags(flags)`.
- **Hi-res chunk "OLX hi-res data"** (:155-172, `hires` :191-208): a PNG of exactly 2W×4H. Front is on top, back below,
  alpha is dropped, `art_scale` becomes 2, and the 2×2 back block is copied over the front where `PX_EMPTY` is set.
  A malformed chunk is a warning and keeps the 1× art. Other chunks are warnings.
- `def.theme = "olx-theme-" + slug(theme)`, `spawns: Vec::new()` (:178-185). `.lxl` files carry no spawns (§7).
- **Kept from OLX's conventions:**
  - material indices are Gusanos';
  - art is stored at 1× ("OLX's 2× surfaces are plain pixel doubling", :5-7);
  - `front` holds back pixels wherever `PX_EMPTY`.
- **Edge case:** pixels whose flags have neither the empty, dirt nor rock bit (0x00, 0x08 shadow, …) become
  `Background` but keep the *front* RGB. The invariant "front = back at background" can then fail for an odd `.lxl`.
  It holds for all 4 bundled levels (measured, §6).
- No bundled level has a hi-res chunk (all `art_scale: 1`, import report has no warnings), and there is no hi-res fixture
  (`fixtures/olx-mini/levels/Tiny Level.lxl` is 290 B). Art scale 2 is covered only by unit tests (:263-303).

### 2.4 Conventions still applied when levels load or run
- **Material palette indices.** The index is the material, and the palette is decoration. This is already explicit, and matches spec §3.
- **Front at background cells.** The renderer shows `front` there until a rebuild (`ColorMap::new` starts from
  `front`; `rebuild_area` uses `back` for background). The two only agree if front = back there, which the importer guarantees.
- **Art scale.** Level art is in world-pixel blocks of `art_scale²` (`fill`/`copy` in `color.rs:428-446`). OLX levels use 1.

### 2.5 Proposed authoring form and exactness

`level.ron`, from spec §3:

```ron
(name: "Castle Strike", size: (1350, 550), art_scale: 1, theme: "olx-theme-dirt")
```

Plus `objects: [Spawn((x, y), notes?)]` and the reserved `look`. Files: `material.png`, `front.png`, `back.png`, `thumb.png`.

| Runtime value | Authoring | Exact? |
|---|---|---|
| `LevelDef.name/width/height/art_scale/theme` | `name`, `size`, `art_scale`, `theme` | yes |
| `LevelDef.spawns` | `objects: [Spawn((x, y))]` in order | yes. `SpawnPoint` is i32, so negative coordinates parse. Spec: a warning for objects outside the level. |
| `material` | `material.png`, 8-bit indexed. The writer uses the canonical palette; the reader ignores the palette as today. | yes (bundled material PNGs are already 3-entry canonical palettes) |
| `front` / `back` | RGBA PNGs at `size × art_scale` | **yes, if `front.png` keeps background cells verbatim**, as today. If the format instead leaves background cells transparent and `compile` fills them from `back`, it is exact for all 4 bundled levels (0 differing pixels). It is not exact for a `.lxl` with flag-0/shadow-only pixels (§2.3). |
| `thumb` | `thumb.png`, unchanged and outside the hash | yes (pass-through) |

- Front and back alpha: all bundled art is opaque, and runtime alpha is irrelevant (`fit` forces 255).
  `compile` can keep authored alpha. A level `Level ==` check on bundled packs is exact either way.
- 13b's runtime cannot express `look` or non-`Spawn` objects. Per spec they are compile errors.

---

## 3. Skins

### 3.1 Runtime types (`crates/olr-data/src/skin.rs`)

| Type / field | Meaning | Readers |
|---|---|---|
| `MaskKind {Keep, Colorize, Transparent}` (:11-18) | OLX: black mask keeps, magenta (key) skips, any other colour tints | `Skin::colorize` |
| `FrameLayout {frame_width 32, frame_height 18, frame_spacing 4, worm_width 20, worm_height 18}` (:22-34, `OLX` const :32-33) | `include/CGameSkin.h:37-41` | **Only compared for equality** with `FrameLayout::OLX` (`olr-content/src/maps.rs:152-153`). The drawing code uses `olr-content`'s constants `FRAME_WIDTH=32`, `FRAME_SPACING=4`, `SKIN_WIDTH=20`, `SKIN_HEIGHT=18`, `SHEET_FRAMES=21` (`skins.rs:14-20`). |
| `SkinDef {name, layout}` (:38-41) | `skin.ron` | `load_skin`; menus use the pack name |
| `Skin {def, image, mask: Vec<MaskKind>}` (:45-49) | Image and mask are the same size | `colorize` |
| `Skin::colorize(player)` (:60-74) | Keep → pixel as is (alpha kept); Transparent → `(0,0,0,0)`; Colorize → `colorize_pixel` | `LoadedSkin::colorized` (`olr-content/src/skins.rs:52-54`) → `SceneAssets::add_worm` (`olr-scene/src/assets.rs:121-126`, px_per_world 2, **not keyed**) and the skin preview (`olr-ui/src/skin_preview.rs:51-57`) |
| `colorize_pixel` (:80-93) | `min(255, c*p/div)` with divisors (96, 156, 252), integer, alpha kept. An exact (255,0,255) result becomes (240,0,240). | (OLX `CGameSkin.cpp:781-789`) |

**Frame cut** (`olr-content/src/skins.rs`):
- `skin_source(f, face)` (:38-45): `sx = f*32+4`. Right reads `x = sx`. **Left reads `x = sx+1`, mirrored**: OLX's
  quirk, `bmpMirrored->w - sx - 20 - 1` (`CGameSkin.cpp:597-601`). Left-facing frames show source columns `sx+1 ..= sx+20`.
- `skin_sprite` (:60-73) cuts and mirrors; pixels outside the sheet are transparent.
- `olr-scene::skin_frame` (`worm.rs:26-32`): `ang = min(6, int((angle+90)/151*7))`, `f = walk*7 + ang`, clamped to
  `frame_count-1`, where `frame_count = sheet.width / 32` = 21 (`worm.rs:160`). The blit is at px_per_world 2 (`worm.rs:162`).
- (pre-13c, format 4) `LoadedSkin` (`maps.rs:56-59`). `load_skin` (:147-162) **falls back to a placeholder** (warning) unless
  `layout == OLX` and the sheet is exactly 672×18. `skin_or_default` (:166-174) falls back to `olx-skin-default`, then the placeholder.
  `LoadedSkin::placeholder` (:66-74) is all `Colorize` in `PLACEHOLDER` (255,0,220).

### 3.2 The format-4 pack (pre-13c; `crates/olr-pack/src/skin_pack.rs`, removed)
- `skin.ron` holds `SkinDef`.
- `skin.png` is RGBA8.
- `mask.png` is 8-bit indexed, palette `[black, white, magenta]`: 0 = Keep, 1 = Colorize, 2 = Transparent (:16, :24-30, :64-72).
- The reader checks only that the mask size equals the image size (:61-63) and that indices are 0..2. No size bound beyond `IMAGES`.
  The game enforces 672×18 (above).

### 3.3 Importer (`crates/olr-import-gfx/src/skin.rs:24-74`)
- The OLX sheet must be at least 672×36: the top 18 rows are the image, the next 18 the mask. Larger sheets are cropped with a warning.
  12 bundled sources are larger, the largest 672×90 (import report).
- **The key comes from the uncropped sheet's corners** (`sheet.olx_color_key()`, :41).
- Per pixel (:43-66):
  - mask RGB black → Keep;
  - mask RGB = key → Transparent;
  - otherwise Colorize;
  - **then** an image pixel with RGB = key, or image alpha 0, or mask alpha 0, forces Transparent.
- Transparent pixels are stored as `a = 0` (RGB kept before plan 13c, zeroed since). All others get `a = 255`, so partial alpha is dropped.
- **Kept OLX conventions:** the 672×18 strip of 21 frames of 32 px with a 4 px inset, and the binary mask.
- Not a deviation: OLX's `SetColorKey_Alpha` gives key-coloured pixels alpha 0 before `Colorize_Execute`, which keeps alpha, so OLX hides them as the importer does (corrected by plan 13c).
  Mask-alpha edge cases, unchanged: OLX tests a black mask by RGB only (a black mask pixel with alpha 0 still copies its pixel) and treats any non-opaque non-black mask pixel as transparent, where the importer requires mask alpha 0; OLX keys a re-cut sheet twice, the importer once.
- Since plan 13c the importer stores every hidden pixel as `Rgba::TRANSPARENT` (RGB zeroed), so `read_skin(write_skin(s)) == s`.

### 3.4 Proposed authoring form and exactness
Spec §4: `skin.ron (name, art_scale, frame_size, aim_frames, walk_frames)`. `skin.png` is an RGBA grid with columns =
aim (7) and rows = walk (3), cells exactly 20×18, so 140×54. `tint.png` is greyscale of the same size. `left.png` is optional.

Proposed `compile` to today's `Skin`:
- Sheet 672×18. Cell (aim a, walk w) → sheet x = `(w*7+a)*32 + 4`.
- Spacing columns → `(0,0,0,0)` with mask `Transparent`.
- Mask per pixel: alpha 0 → `Transparent`; tint 0 → `Keep`; tint 255 → `Colorize`.
- `def.layout = FrameLayout::OLX`.

| Runtime value | Exact? |
|---|---|
| Right-facing frames: sheet window `[f*32+4, +20) × 18` and its mask | **yes**, pixel for pixel, provided the importer writes cell RGB as today (including RGB under alpha 0), or the comparison is on colorized output |
| Left-facing frames (today's quirk window `[f*32+5, +20)`) | **no for 288/718 skins** (column offset 24 is lost). Exact only against *true* mirroring, which the spec adopts (re-bless the render goldens). |
| Spacing pixels (offsets 0..4, 24..32) | **no**: 401/718 skins have visible pixels there. They are never drawn once the quirk is dropped. |
| RGB under alpha 0 | Lost if the canonical PNG zeroes it (recommended). `colorize` output is unchanged (`skin.rs:67`). |
| `MaskKind` from tint | Exact for 0 and 255. **Tint 1..=254 ("partial tint", spec) has no runtime representation.** In 13b it must be an error, or the runtime gains a tint amount (a renderer-side change; `olr-sim` never sees skins). |
| `art_scale ≠ 2`, `frame_size ≠ (20,18)`, `aim_frames ≠ 7`, `walk_frames ≠ 3`, `left.png` | Not representable at runtime (constants above). Options: a compile error, or what `load_skin` did before 13c (a placeholder with a warning). |

Bundled skin data (all 718, measured before 13c, in format 4):
- every `skin.png` is RGBA8 672×18; every `mask.png` is indexed 3-entry 672×18;
- alpha is only 0 or 255, and alpha 0 ⇔ mask Transparent (0 mismatches);
- kinds: Keep 1,386,695, Colorize 1,012,072, Transparent 6,286,161 px;
- **no visible pure magenta**. Visible (254,0,254) is an ordinary colour in `olx-skin-eyeball` (30 px) and
  `olx-skin-sandbag` (7 px). It is unambiguous once alpha carries transparency.
- In the OLX sources, `Wario.png` is the one skin with a (254,0,254) corner. 12 interlaced sources were not
  scanned by my decoder. Sources include 2 BMP-in-PNG files and indexed 4- and 8-bit files; the importer handles them.

---

## 4. Themes

### 4.1 Runtime types (`crates/olr-data/src/theme.rs`)

| Field | Meaning (raw OLX colours, doc :18-20) | Readers |
|---|---|---|
| `ThemeDef {name, holes, stones, misc}` (:11-16) | Name and image counts | `read_theme` |
| `holes: Vec<RgbaImage>` (:24) | Carve stamps (5 in OLX) | `LoadedTheme::stamps()` → `Stamp::from_image` (`olr-content/src/maps.rs:44-46`) → **sim** (`MatchSetup.holes`, `olr-sim/src/setup.rs:51`) and the colour map (`ThemeArt.holes`); `default_dirt_colour` (`olr-fx/src/assets.rs:197-211`) reads `holes[0]` raw |
| `stones`, `misc` (:25-26) | OLX map-editor and random-map art | **No runtime reader.** OLX: stones → rock where not keyed (`CMap.cpp:~1550`); misc paints over dirt (`~1635-1643`). Both are `SetColorKey`ed (`CMap.cpp:328,362`). |
| `front_tile` (:27) | Dirt colours, tiled from the map origin in world pixels | Colour map `tile()` (`color.rs:420-425`) in `place_dirt` and `rebuild_area`. Its pixel (0,0) seeds `iDefaultColour` (`olr-fx/src/assets.rs:199`). An empty tile gives `NO_TILE` (255,0,220) (:26). |
| `back_tile` (:28) | OLX `TileMap` (new or editor maps, `CMap.cpp:784-789`) | **No runtime reader.** It is loaded into `LoadedTheme.back_tile` (`maps.rs:38,140`) and never used. |
| `green_ball: Option` (:33) | OLX's shared `data/gfx/greenball.png` (`CMap.cpp:365-367`) | `LoadedTheme::green_mask()` → `GreenMask::from_image` (`maps.rs:49-51`) → **sim** (`MatchSetup.green_mask`, `setup.rs:54`) and the colour map |

`LoadedTheme` (`maps.rs:30-40`) is `{name, holes, green_ball, front_tile, back_tile, warnings}`. `load_theme` (:124-143)
warns when there are fewer than 5 holes ("the larger ones carve nothing") and when there is no green ball. Readers that build the sim's stamps:
`olr-content/src/sandbox.rs:98-99`, `replay.rs:100-101`, `color.rs:12-14`, `olr-scenario/src/lib.rs:152-153`.
They are excluded from the checksum (`olr-sim/src/checksum.rs:133-134`).

### 4.2 How each theme image is interpreted

**Hole stamps.** `Stamp::from_image` (`olr-terrain/src/stamp.rs:26-41`) classifies each pixel:
- RGB = (255,0,255) → `Carve`;
- RGB = (0,0,0) → `Skip`;
- else → `Paint(c)`, with the full RGBA kept.

Alpha is ignored, so (0,0,0,0) counts as Skip. There is **no corner rule** for holes. OLX keys holes with black
(`CMap.cpp:335`), which only matters for blitting.

| Operation | Material and count (sim) | Colour map (cosmetic) |
|---|---|---|
| `carve_hole` (`olr-terrain/src/lib.rs:99-120`; sim `effects.rs:36-66`, hole `clamp(size,0,4)`; digging uses hole index 3, `physics.rs:166`; beams `beam.rs:199,248`; modes `referee.rs:233-241`) | Dirt under `Carve` → Background, counted (dirt count). `Skip` and `Paint` leave the material. The stamp is not offset after edge clipping (kept quirk). | `color.rs:286-313`, dirt cells only: `Carve` copies `back`; `Paint(c)` fills `c` (opaque, alpha ignored); `Skip` does nothing |
| `place_dirt` (`dirt.rs:64-89`; sim `effects.rs:80-91` with hole 4, ×3 per `doMakeDirt`; modes) | Not `Skip` and not rock → Dirt. Count +1 if it was background. **`Paint` over background adds another +1** (OLX counts it in both branches). | `color.rs:319-344`: not `Skip` and not rock → `front_tile` colour; `Paint(rim)` over background → `rim` |

So Carve, Skip and Paint each change gameplay (material and dirt count). Only Paint's colour is cosmetic.
The event `TerrainEdit` carries the stamp's (w,h) (`effects.rs:63,84`).

**Green ball.** `GreenMask::from_image` (`stamp.rs:79-95`) compares the **whole RGBA, alpha included**
(raw `Uint32` against opaque colours):
- `== (0,255,0,255)` → `Green`;
- `== (255,0,255,255)` → `Pink`;
- else → `Rim(c)`.

| | Sim (`dirt.rs:96-125`, `effects.rs:97-106`) | Colour map (`color.rs:350-380`) |
|---|---|---|
| Green | Background → Dirt, count +1, **one `rand()` per placed pixel** (gameplay RNG) | One of `GREEN_SHADES` (`color.rs:21-22`, `CMap.cpp:1245-1248`) from the map's own xorshift (`next_shade` :412-417) |
| Rim | Background → Dirt, count +1 | Rim colour |
| Pink | nothing | nothing |

**`iDefaultColour`.** `olr-fx::default_dirt_colour` (`assets.rs:190-211`, OLX `CMap.cpp:338-352`):
- start from `front_tile(0,0)`;
- for each row of `holes[0]`, the first pixel that is neither black nor magenta replaces it. The `break` leaves only the row loop, so the last such row wins;
- alpha is forced to 255.

It is used for dirt-particle entities (cosmetic; OLX `CClient_Game.cpp:235`). Bundled value: **(108,56,0)**; front_tile(0,0) is (120,64,8).

**Green shades** are the engine constant `GREEN_SHADES`, not theme data.

### 4.3 Today's pack (`crates/olr-pack/src/theme_pack.rs`)
- Files: `theme.ron` (ThemeDef), `holes/<n>.png`, `stones/<n>.png`, `misc/<n>.png` (1-based, counts from `theme.ron`),
  `front_tile.png`, `back_tile.png`, optional `green_ball.png`.
- All are RGBA8, decoded with `decode_rgba`. The only check is that image counts match the RON on write (:38-41).
- **No size checks and no stamp bound** beyond `IMAGES`.

### 4.4 Importer (`crates/olr-import-gfx/src/theme.rs:41-81`)
- `theme.txt` `[General] NumStones/NumMisc` (missing → 0 with a warning).
- Always `Hole1..5`, `Stone<n>`, `misc<n>`, `Fronttile`, `Backtile`, matched case-insensitively.
- `data/gfx/greenball.png` is optional (warning).
- All images are **decoded raw**: no keying, OLX colours kept. A `.png` that is really a BMP is decoded too (`lib.rs:26-38`).

### 4.5 Proposed authoring form and exactness
`theme.ron`, as the spec has it ("name, default dirt colour (hex), stamp lists"):

```ron
(name: "Dirt", dirt_colour: "#6c3800",
 holes: ["1","2","3","4","5"], stones: [...], misc: [...])
```

The list order is the hole index. Files:
- `holes/<n>.png` (RGBA paint) + `holes/<n>.mask.png` (indexed: 0 untouched, 1 carve, 2 paint);
- `green_ball.png` + `green_ball.mask.png` (indexed: 0 untouched/pink, 1 green, 2 rim), so the green ball follows the same pattern;
- `stones/<n>.png`, `misc/<n>.png` as RGBA with alpha;
- `front_tile.png`, `back_tile.png`.

Compile to today's OLX-coded `Theme`, so `olr-terrain`, `olr-sim` and `olr-fx` stay unchanged:

| Runtime | Compile rule | Exact? |
|---|---|---|
| hole pixel | untouched → (0,0,0,255); carve → (255,0,255,255); paint → the paint layer's RGBA | **yes for bundled** (all opaque, 3 colours each: magenta, black, (108,56,0)). Error cases: a paint colour with RGB (0,0,0) or (255,0,255), any alpha, would read back as Skip or Carve. Today's images with non-opaque magenta or black would come back opaque: `Stamp`-equal, not byte-equal. |
| green ball pixel | untouched → (255,0,255,255); green → (0,255,0,255); rim → paint RGBA **with alpha as authored** | **yes for bundled** (8×9, opaque, colours G, pink, (108,80,0)). Error case: a rim colour exactly equal to (0,255,0,255) or (255,0,255,255). Forcing rim alpha to 255 would be wrong: (0,255,0,128) is a Rim today. |
| stones, misc | transparent → (255,0,255,255); keep opaque pixels | **yes for bundled** (all opaque, no 254, no visible magenta). Alternatively compile can keep the alpha image: nobody reads them, and only `Theme ==` and the doc comment change. |
| front_tile, back_tile | as is | yes (opaque, 128×128) |
| `dirt_colour` | **No runtime field.** Today it is computed by `olr-fx::default_dirt_colour`. | Options: (a) compile checks it equals the derived value (lift computes it); (b) a renderer-side field on `LoadedTheme`/a compile side output. See the decisions. |
| art scale | Theme images are world pixels (stamps, tiles and the ball are applied per world pixel; `fill` scales). | Must be 1 in 0.2. |

---

## 5. The core pack

### 5.1 Runtime types
- `olr_data::core_pack::CoreDef {sprites: Vec<String>, sounds: Vec<String>, colours: BTreeMap<String, Rgba>}`
  (`core_pack.rs:19-29`). Sprite `x` is `gfx/x.png`. Sound `x` (with extension) is `sounds/x`.
- `olr_pack::CorePack {manifest, def, sprites: BTreeMap<String, RgbaImage>, sounds: BTreeMap<String, Vec<u8>>}` (`olr-pack/src/core_pack.rs:18-26`).
  - `check_name`: lowercase, digits and `-_.`, not starting with `.` (:29-34).
  - `read_core_tolerant` (:80-121) skips bad sprites and sounds with errors; `read_core` (:67-73) is strict.
- `olr_content::CorePack` (`core_assets.rs:33-134`):
  - `from_pack` checks each of `CORE_SPRITES` (:18-30) for an **exact size** and replaces a missing or odd one with a placeholder of that size;
  - `check_sound` filters the sounds;
  - `game_sound` maps `olx:data/sounds/x.wav` to `x.wav`, then `x.ogg` (:123-128);
  - `colour(key)`.

### 5.2 Every core sprite: size, frames and art scale

| Sprite | Size | Frames (code) | Art scale | Cut by |
|---|---|---|---|---|
| crosshair | 12×6 | 2 of 6×6 (0 normal, 6 = target) | 2 | `olr-scene/src/worm.rs:155-158` |
| muzzle | 224×16 | 14 of 16×16 (7 right, then 7 left) | **1** | `worm.rs:39-48,163-166`; `assets.rs:84` |
| explosion | 512×32 | 16 of 32 (frame `15-size`, +40/s, ends > 15) | 2 | `olr-fx/src/draw.rs:75`, `entity.rs:86,172-174` |
| smoke | 70×14 | 5 of 14 (ends when the frame passes 4) | 2 | `draw.rs:76`, `entity.rs:176-178` |
| chemsmoke | 40×10 | **4** of 10, animates to 5 (frame 4 draws nothing; SDL clip, `draw.rs:31-50`) | 2 | `draw.rs:77` |
| spawn | 160×32 | **5** of 32, animates to 6 | 2 | `draw.rs:78`, `entity.rs:184-186` |
| sparkle | 30×10 | 3 of 10 | 2 | `draw.rs:85` |
| giblets | 32×12 | rows 0..8: 4 gib frames of 8×8 (`rotation*8`) at 2×; **row 8: 4 stain frames of 4×4 at 1×** (`(rotation*4, 8)`) | 2 / **1** | `draw.rs:79-82`; `olr-fx/src/lib.rs:221-223` → `ColorMap::stain_image` |
| hook | 6×6 | 1 | 2 | `worm.rs:196-204` (src 6×6 → 3×3 world) |
| bonus | 12×8 | 1 (opaque, **no magenta at all**) | 2 | `olr-scene/src/projectile.rs:137-152` |
| health | 10×10 | 1 | 2 | same |

All frame sizes are **hard-coded in the drawing code**. None is guessed from the image. `olr-content` only checks the
total size. The core art scale is `px_per_world` in `SceneAssets::new` (`assets.rs:83-85`) and `FxAssets::new` (`olr-fx/src/assets.rs:108-118`).

### 5.3 Keying at load
- `olr-scene` `keyed()` (`assets.rs:64-69`) is applied to crosshair, muzzle, hook, bonus and health (:83-85) and to every mod
  projectile image (:94-96).
- `olr-fx` `keyed()` (`assets.rs:98-103`) is applied to explosion, smoke, chemsmoke, spawn, sparkle and the default gibs (:110-120),
  and to each worm's colorized gibs (:138-144).
- **The corner rule is evaluated per image.** For the colorized gibs it is evaluated after colorizing; the corners stay magenta.
- Unkeyed copies are also used:
  - `colorize_gibs` (`olr-fx/src/assets.rs:165-188`, OLX `CWorm.cpp:671-757`, f32 arithmetic) runs on the raw sheet. It skips RGB
    magenta and the three gun greys, and nudges an exact magenta result to (240,0,240).
  - The gib stain reads the **unkeyed** colorized sheet and skips RGB magenta *or* alpha 0 (`color.rs:246`).
- Placeholders use (255,0,220) so they never key (`olr-content/src/placeholder.rs:7`).

### 5.4 Colours
- `colours.cfg`: 83 entries in the bundled `core.ron`, written today as `(r,g,b,a)` structs.
- Parsed by `StrToCol`-like rules: `#rrggbb` or `r,g,b` clamped (`olr-import-gfx/src/core_pack.rs:27-43`).
- **Only 4 are read at runtime:** `RopeColour1/2` and `LaserSightColour1/2`, with code defaults (`olr-scene/src/assets.rs:35-38,100`).

### 5.5 Today's pack and importer
- `core.ron` + `gfx/<name>.png` (RGBA8) + `sounds/<file>` (Ogg bytes).
- `import_core` (`olr-import-gfx/src/core_pack.rs:75-126`): `.png` → sprite named by its lowercase stem, decoded raw
  (**not keyed**); `.ogg` → sound; `colours.cfg` → colours. A missing file is a warning.
  The manifest list is in `content/olx-manifest.ron` (11 sprites, 6 sounds, `colours.cfg`).

### 5.6 Proposed authoring form and exactness
`core.ron`:
- `sprites: { "<id>": (file, frames: (c, r) | angles: n, art_scale) }`;
- `sounds: { "<id>": "sounds/<file>" }`;
- `colours: { "<key>": "#rrggbb" }` (the `Colour` type already exists, `olr-format/src/colour.rs`).

PNGs carry alpha: the importer applies `key_transparent` once, offline.

Compile output: `olr_pack::CorePack`-shaped data with **alpha images**.

| Check | Exact? |
|---|---|
| compiled sprite == `key_transparent(today's sprite)` | yes: `key_transparent` writes `(0,0,0,0)`, and the importer must write exactly that |
| `key_transparent(compiled) == compiled` (so today's loaders, which still key, see no change during 13b–13c) | **yes for all 11 bundled sprites**: all opaque before keying, no (254,0,254), so no magenta survives. It fails only for an image that is (254,0,254)-keyed *and* contains pure magenta, or whose alpha image has an opaque (254,0,254) corner. Neither is bundled. |
| gibs: `colorize_gibs`, atlas and stain results | yes: `(0,0,0,0)` tints to `(0,0,0,0)` (alpha kept), and the stain skips alpha 0 as it skipped magenta |
| `CoreDef.colours` | yes (hex round-trips `Rgba`; all are opaque) |
| sounds | yes (bytes pass through) |
| `giblets` described by one grid | **no** (two grids at two scales). Either split it into `giblets` (32×8, 4 frames, 2×) + `giblet-stains` (16×4, 4 frames, 1×) and compose the 32×12 runtime image in compile (exact: region x16..32, y8..12 is all key → (0,0,0,0)), or let a sprite carry several regions. |
| declared frames vs code | Informational only. Today's runtime uses code constants. chemsmoke and spawn animate one frame past their strips. |

---

## 6. Bundled content inventory (`content/packs`, measured)

| Kind | Packs | Details |
|---|---|---|
| Mods | 2 | `olx-classic`, `olx-mw-1-0` (13a). Mod gfx: no 254 key, all opaque; `olx-mw-1-0/gfx/plasmaexp2.png` 97×32 (3 frames, remainder 1). |
| Levels | 4 | castlestrike 1350×550, dirt-level 504×350, fossilfacility 640×480, liero-factory 1200×480. **All `art_scale` 1, `spawns: []`, theme `olx-theme-dirt`, no hi-res chunk.** Material PNGs are 8-bit, 3-entry palette. Front and back are RGBA8, fully opaque, no magenta. **Front = back at every background cell (0 exceptions).** Front = back at some non-background cells too (2692 in castlestrike: harmless). Thumbs 160×96 RGBA. |
| Theme | 1 | `olx-theme-dirt`: holes 5×5, 7×7, 9×9, 11×11, 15×15 (3 colours each: magenta, black, (108,56,0)). Green ball 8×9 (green, pink, rim (108,80,0)). 8 stones (8×8 to 30×26), 8 misc (7×5 to 16×15). front_tile and back_tile 128×128. All opaque, no 254. `iDefaultColour` (108,56,0). |
| Core | 1 | `olx-core`: 11 sprites (sizes in §5.2), all opaque RGBA8, no (254,0,254) anywhere, bonus has no magenta; 6 Ogg sounds; 83 colours. |
| Skins | **718** | 409 from the OLX repository + 309 from 0.58 rc5 (`docs/porting/STATUS.md:9`). All 672×18, alpha 0/255 only. Spacing and quirk statistics in §0 and §3.4. Visible (254,0,254): eyeball and sandbag. OLX source with a (254,0,254) corner: `Wario.png`. 12 oversized sources were cropped (import report). |

The import report (`content/packs/import-report.ron`) has no warnings for levels, theme or core.

---

## 7. Spawns: today and in OLX

- **Today:** `SpawnPoint {x: i32, y: i32}` (`olr-data/src/level.rs:89-93`); `LevelDef.spawns` ("empty means random spawn, as in OLX", :106-107).
- Always empty: lxl import :184, `olr-cli/src/genlevel.rs:164`, test `:241` "worms spawn by FindSpot".
- **No runtime reader:** worms spawn and respawn with `FindSpot` (`olr-sim/src/respawn.rs`; `IsGoodSpawnPoint` is
  `CMap.h:460`, "no rock in 6×6"). The sandbox's `SpawnSpec`/`w.pos` (`olr-content/src/sandbox.rs:94`) is a test or scenario position, not level data.
- **OLX:**
  - `.lxl` LieroX levels have no spawn points. A CTF `.lxl`'s old flag data is 12 bytes, ignored.
  - Gusanos and Teeworlds levels have `LevelConfig::spawnPoints` and `teamBases`, as `SpawnPoint(pos, team)` with team −1 for any
    (`src/gusanos/level.h:45-61`, used in `level.cpp:404-430`; Teeworlds loader `MapLoader_Teeworlds.cpp:905-917`).
  - **So a future `Spawn` may want a `team`.** The spec's `Spawn((x, y), notes?)` has none.
- The spec says "the 0.2 simulation reads only `Spawn`". Today it reads none. Wiring spawns into `olr-sim` changes sim
  behaviour only for levels that have spawns (none bundled), but it is a runtime change, so it is out of 13b's scope unless the plan decides otherwise.

---

## 8. Equivalence testing: risks and the minimal compile outputs

**Deterministic by construction:**
- No floats in these formats: integers, `u8` colours, `i32` spawns.
- `BTreeMap`s, sorted VFS walks, integer thumbnails.
- `colorize_gibs` uses f32 but is runtime code and is unchanged.

**Risks:**
- **PNG bytes.** `encode_rgba`/`encode_indexed` are deterministic for a pinned `png` version and default settings. A
  `png` upgrade can change bytes, and so content hashes. "Load then save gives the same bytes" holds only for PNGs our
  encoder wrote. An externally edited PNG would be re-encoded unless the writer passes untouched image bytes through.
- **Decoding.**
  - `normalize_to_color8` strips 16-bit to the high byte and expands palette+tRNS. It ignores gAMA, iCCP and sRGB, which is good: no colour management.
  - Indexed masks: today's reader takes **only 8-bit** indexed. GIMP and Aseprite may save 1/2/4-bit palettes for few colours, so the
    material and mask readers should unpack low bit depths or the editors will produce "palette" errors.
  - Greyscale `tint.png` needs a new decode path (grey 8; grey+alpha?). `decode_indexed` rejects it, and `decode_rgba` expands it to RGB.
- **Baseline choice.** As in 13a, compare against today's packs (`olr-pack::read_*` → lift → write → read → compile).
  Do not compare against fresh imports from OLX: this avoids BMP and interlaced decoder variance, and needs no network.
- **Untestable on bundled data:** art scale 2 (no hi-res level), 254-keyed sprites, partial alpha, and more than 5 holes. These need
  synthetic unit or proptest cases.
- **What `==` can't see:** `Skin`/`Theme`/`Level` `PartialEq` compares raw bytes, which is fine. For skins, compare
  colorized windows (several colours, including one that triggers the (240,0,240) nudge, such as (96,x,252)), not whole sheets.

**Minimal runtime-facing outputs**, so nothing downstream changes:
1. `compile_level(src) -> olr_data::level::Level` (plus thumb pass-through). Exact.
2. `compile_theme(src) -> olr_data::theme::Theme` with **OLX-coded images** (holes and green ball re-encoded to magenta, black, green), so
   `Stamp::from_image`, `GreenMask::from_image`, `default_dirt_colour` and `olr-terrain` are untouched. Plus, if
   adopted, the dirt colour as a side value.
3. `compile_skin(src) -> olr_data::skin::Skin` as the 672×18 OLX sheet (spacing transparent, `FrameLayout::OLX`).
4. `compile_core(src) -> (CoreDef, BTreeMap<String, RgbaImage>, BTreeMap<String, Vec<u8>>)` (the fields of
   `olr_pack::CorePack`) with alpha images, giblets composed back to 32×12.
5. Optionally, mod gfx: alpha images and declared frames, with compile checking `frames == width / height` because
   `olr-content/src/mods.rs:74` derives frames from the size and the sim uses them (`olr-sim/src/proj_sim.rs:170-181`; `Once` deletes the
   projectile).

**Equivalence assertions:**
- Levels: `Level ==`.
- Themes: `Theme ==`, plus `Stamp`, `GreenMask` and `default_dirt_colour` equal.
- Skins: right windows and masks equal, colorize equal on windows, left = true mirror.
- Core: compiled == keyed(old), keyed(compiled) == compiled, colours and sounds equal, gibs pipeline equal.
- Oracle and match goldens are unaffected in 13b, since loaders don't switch (13a's precedent).

---

## 9. Decisions the plan must make

1. **Where PNG lives.** Give `olr-format` its own codec module (the `png` workspace dependency; move `ImageBound`, `IMAGES`,
   `LEVEL_ART`, `png_size`) so 13c's `olr-pack → olr-format` edge has no cycle. The alternative is a temporary
   `olr-format → olr-pack` edge.
2. **Skin layout vs. the mirror quirk.**
   - Option A: drop the quirk in 13b (`skin_source` left = `sx`; re-bless render goldens and the preview tests; STATUS entry).
   - Option B: 13b compiles to the OLX sheet and tests only right windows and true-mirror left frames, and 13c drops the quirk when loaders switch.
   - Either way, today's left-facing frames cannot be reproduced for 288 skins.
3. **Skin `art_scale` for OLX skins is 2**, not 1 as in spec §4's example. Fix the spec example.
4. **Partial tint (1..=254) and `left.png` in 13b:** a compile error, or a runtime extension (renderer-side: `MaskKind`
   becomes an amount, and the left sheet is drawn instead of the mirror)? Also: non-OLX `frame_size`, `aim_frames`, `walk_frames` or
   `art_scale`: an error, or today's placeholder with a warning?
5. **Transparent-pixel RGB in skins and sprites:** zero it in canonical PNGs (recommended; `colorize` and `key_transparent` results
   are unchanged). Equivalence is then on outputs, not on `Skin ==`.
6. **`front.png` at background cells:** keep it verbatim (exact for any `.lxl`), or store it transparent and fill from `back` in compile
   (exact for bundled; changes odd `.lxl` imports with flag-0 or shadow-only pixels).
7. **Hole and green-ball mask encoding:** index order and canonical display palette (e.g. 0 untouched, 1 carve or green, 2 paint).
   Is `green_ball` a mask + paint pair like the holes? (Spec §4 lists only `green_ball.png`.) Colour collisions are errors:
   a hole paint RGB of black or magenta; a rim exactly (0,255,0,255) or (255,0,255,255).
8. **Theme `dirt_colour`:** validate it equals OLX's derived `iDefaultColour` (lift computes it), or make it authoritative
   through a renderer-side value (`LoadedTheme` field or compile side output) and switch `olr-fx::default_dirt_colour` in 13c.
9. **Stones and misc in compile:** re-encode magenta to keep `Theme ==`, or keep alpha and update `theme.rs`'s doc
   comment (no runtime reader). Do stones and misc get explicit meanings now (stone = rock, misc = paint over dirt), or later with the editor?
10. **Theme image art scale:** fixed at 1 in 0.2. Declare it, or leave it implicit?
11. **Core `giblets`:** split into two sprites composed by compile, or allow multi-region sprite descriptions?
12. **Core sprite frames and rates:** informational only, checked against the code's constants (error or placeholder?), or not declared
    for core sprites in 0.2? Note that chemsmoke and spawn animate one frame past their strips.
13. **Core colour keys:** keep OLX's `RopeColour1` names (runtime keys), or kebab-case in the file mapped by compile?
    Sound ids: `"throw"` → `sounds/throw.ogg`, with `.wav` accepted.
14. **Mod gfx in 13b?** Alpha conversion and sprite descriptions for `gfx/` (13a left them alone). If included, `frames` must equal `width / height`
    (`plasmaexp2` 97×32 → 3).
15. **Stamp bounds** (new named constants, never panics): maximum hole and green-ball side, maximum hole count (`StampId::Hole(u8)` →
    ≤ 256), maximum stones and misc. Also the material and mask readers accepting 1/2/4-bit indexed PNGs.
16. **Spawns:** compile `Spawn` objects to `LevelDef.spawns` only (still unread at runtime), or wire them into spawning (a sim
    change, outside "runtime unchanged")? Should `Spawn` reserve a `team` (Gusanos/Teeworlds have one)?
17. **`level.ron` theme reference:** stays a pack id (`olx-theme-dirt`), as today.
18. **Image byte stability:** pass through unedited image files byte for byte, or re-encode canonically on every save (and pin
    the `png` version)? This affects the "load then save gives the same bytes" test for PNGs.
19. **Test fixtures:** synthetic art-scale-2 level, 254-keyed sprite, partial-alpha and more-than-5-holes cases, since nothing bundled covers them.
20. **Retracted (plan 13c):** the importer's hiding of key-coloured image pixels under tint masks matches OLX (`SetColorKey_Alpha`).
