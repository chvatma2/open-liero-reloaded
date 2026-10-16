# Plan 13b: Levels, Themes, Skins and the Core Pack in the Authoring Format

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `olr-format` gains the authoring format for levels, themes, skins
and the core pack. Like plan 13a's mods, each kind gets types, a
canonical writer, a `compile` to today's unchanged runtime data and a
`lift` from it. OLX's colour conventions (magenta keys, black/magenta/green
stamp meanings, the skin mask) become explicit alpha, masks and tint
images, converted once by `lift`. Equivalence tests prove that every
bundled pack survives lift → write → read → compile. The skin mirror
quirk is dropped in play, as the user decided.

**Architecture:**
- **Images move into `olr-format`.** `olr-pack`'s PNG codec and bounds
  (`image_io`) move to `olr_format::image_io`, gaining low-bit indexed
  PNGs, greyscale and the one-time OLX key-to-alpha conversion.
  `olr-pack` re-exports it (a new same-layer pair, `olr-pack` →
  `olr-format`), so no caller changes.
- **One module per kind**, each with types plus `compile` and `lift`, and
  a `files` module:
  - `olr_format::levels` (`LevelSource`)
  - `olr_format::themes` (`ThemeSource`)
  - `olr_format::skins` (`SkinSource`)
  - `olr_format::core` (`CoreSource`)
- **Compile targets today's runtime values exactly:** `olr_data::level::Level`,
  `olr_data::theme::Theme` with OLX-coded images (so `olr-terrain` and
  `olr-sim` stay untouched), `olr_data::skin::Skin` as the 672 × 18 OLX
  sheet, and the parts of `olr_pack::CorePack` with alpha sprites.
- **Nothing reads the new files yet.** Plan 13c switches the packs and
  loaders.

**Tech Stack:** Rust 1.99 (edition 2024). Existing workspace dependencies
only (`png`, `serde`, `ron`, `thiserror`; `proptest` and `insta` for
tests). `png` is pinned exactly in this plan.

**Spec:** `docs/superpowers/specs/2026-10-16-formats-design.md` §1, §3,
§4, §5 and §6 (amended with this plan; the amendments are listed below).
The research behind every rule is
`docs/porting/olx-notes/asset-formats.md` (committed with this plan; read
its §0 and §9 first).

## Rulings (the research's decisions, settled for this plan)

1. **PNG lives in `olr-format`** (`image_io`, moved from `olr-pack`, which
   re-exports it). `png` is pinned to `=0.18.1`, so encoded bytes, and
   with them content hashes, stay stable.
2. **The skin mirror quirk is dropped now (Task 4).** A left-facing worm
   shows the true mirror of its right-facing frame. Today's quirk window
   cannot be rebuilt from a 20 × 18 cell for 288 of the 718 bundled skins,
   so the quirk must go before or with the format. The render and HUD
   goldens are re-blessed.
3. **OLX skins are 2× art:** `art_scale: 2` (the spec's example said 1).
4. **What the 0.2 runtime cannot draw is a compile error** ("not
   supported by this runtime yet"), as for mods:
   - a partial tint (1..=254);
   - a `left.png`;
   - a skin layout other than OLX's (`art_scale` 2, `frame_size`
     (20, 18), 7 aim frames, 3 walk frames).
5. **Transparent pixels are `(0, 0, 0, 0)`** in canonical images. Skins
   and sprites are compared on what is drawn (masks, colorized windows,
   keyed sprites), not on hidden RGB.
6. **`front.png` keeps background cells as imported** (exact for any
   `.lxl`).
7. **Stamps:**
   - **Hole masks:** 0 untouched, 1 carve, 2 paint.
   - **The green ball** is a mask and paint pair like the holes: 0
     untouched (pink), 1 green, 2 rim.
   - **Paint layer:** the paint pixel's RGBA, `(0, 0, 0, 0)` elsewhere.
   - **Colours that would change meaning are errors:** a hole paint
     colour of black or magenta, or a green-ball rim exactly
     `#00ff00ff` or `#ff00ffff`.
8. **There is no `dirt_colour` field in 0.2.** OLX derives it from
   `front_tile` and the first hole, and so does `olr-fx`. The field can
   come with the graphics work.
9. **Stones and misc** are stored with alpha and compiled back to OLX's
   magenta. They get no meaning of their own until the editor needs one.
10. **Theme images are world pixels.** They declare no art scale.
11. **The core `giblets` sprite is split** into `giblets` (32 × 8, 4
    frames at 2×) and `giblet-stains` (16 × 4, 4 frames at 1×), composed
    back into OLX's 32 × 12 image by `compile`.
12. **Core sprites declare their frame grid and art scale** for editors
    and agents; `compile` checks the grid divides the image. The drawing
    code keeps its own constants in 0.2.
13. **Colour keys keep OLX's names** (`RopeColour1`): they are runtime keys.
14. **Mod `gfx/` gets the key-to-alpha conversion** (a function and its
    bundled-content test here; plan 13c's importer applies it). A
    projectile image's frame count stays `width / height`, as the
    simulation reads it.
15. **Bounds** (named constants; errors, never panics):
    - `MAX_STAMP_SIDE` 64 px;
    - `MAX_HOLES` 16;
    - `MAX_THEME_IMAGES` 64 stones and 64 misc;
    - `MAX_ART_SCALE` 8;
    - `MAX_OBJECTS` 4096.
16. **Level `Spawn` objects compile to `LevelDef::spawns`.** The runtime
    still spawns with `FindSpot`; nothing reads them yet (spec amended).
    No `team` field until a mode needs one.
17. **`level.ron` names its theme by pack id**, as today.
18. **Writers always re-encode images canonically.** "Load then save gives
    the same bytes" holds for files this writer produced.
19. **Synthetic fixtures** cover what no bundled pack has: art scale 2,
    a 254-keyed sprite, partial alpha, more than 5 holes.
20. **Recorded, not fixed here:** the skin importer hides key-coloured
    image pixels under tint masks, where OLX draws them colorized (69
    source skins). It goes in STATUS.md; plan 13c's importer switch is
    the place to fix it.

## Spec amendments (committed with this plan)

The spec is amended in step with these rulings:

- **§3:** `Spawn(at: (x, y))` objects compile to `LevelDef::spawns`, which
  the runtime does not read yet.
- **§4:**
  - skins use `art_scale: 2`;
  - partial tint and `left.png` are compile errors in 0.2;
  - the green ball is a mask and paint pair;
  - there is no `dirt_colour` field;
  - theme images are world pixels;
  - `giblets` is split.
- **§5:** pin `png`.

## Global Constraints

- **Crate:** `olr-format` stays layer 2 and `std`. Add it to
  `xtask/layers.ron`'s `wasm` list (`olr-pack`, which is on it, now
  depends on it). Add the same-layer pair `("olr-pack", "olr-format")`.
  Its dependencies: `olr-data`, `olr-vfs`, `serde`, `ron`, `thiserror`,
  `png`.
- **Runtime unchanged except the mirror quirk.** No change to `olr-sim`,
  `olr-terrain` or the `olr-data` runtime structs. The only behaviour
  change is `olr_content::skin_source` facing left (Task 4). The oracle
  and the match goldens stay bit-exact.
- **Never panic on content.** Every read, compile and lift returns
  `Result`. Image sizes are checked from the PNG header before decoding.
- **Deterministic output:** `BTreeMap` only, and canonical PNGs through
  `image_io`'s encoders only.
- **Canonical RON:** `olr_format::ron_io` (plan 13a: LF line endings,
  `implicit_some`, defaults left out, `deny_unknown_fields`).
- **Verification cadence (the user's rule).**
  - Inside a task, run `cargo nextest run -p olr-format`, plus any crate
    the task touches.
  - Run `cargo xtask check` at the end of Task 1, Task 4 and Task 7 only.
  - Run `cargo xtask full` once, in Task 7: render goldens change.
- **Commits:** `olr-format: …` / `olr-content: …`, small, with the
  session's attribution lines.

## Review Focus

1. **PNGs from real editors.** 1-, 2- and 4-bit indexed material or mask
   PNGs (GIMP and Aseprite save these for few colours) must read like
   8-bit ones. A greyscale-with-alpha `tint.png` reads its grey. An RGB
   (non-indexed) `material.png` is an error saying "8-bit or lower
   indexed PNG". Task 1 tests it.
2. **A hostile theme.** A 10 000 × 10 000 hole stamp, 1000 holes, or a
   mask index of 7 must be an error naming the file, before the stamp is
   allocated. Task 3 tests it.
3. **Image and mask sizes disagree.** A `holes/2.mask.png` of another size
   than `holes/2.png`, a `tint.png` of another size than `skin.png`, or a
   `front.png` not `size × art_scale`: each is an error naming both files.
   Tasks 2, 3 and 5 test it.
4. **254-keyed and partially transparent OLX images.** Lifting a sprite
   whose corner is (254, 0, 254) keys 254 and keeps pure magenta as a
   colour. A partial-alpha pixel survives. Lifting a theme stamp whose
   meaning pixel is not opaque is a `LiftError`, never a silent change.
   Tasks 1, 3 and 6 test it.
5. **Left-facing worms.** After Task 4, every left-facing frame is the
   exact mirror of its right-facing frame, for every bundled skin. Task 4
   tests it.

---

## File Structure

```
crates/olr-format/src/image_io.rs       moved from olr-pack + low-bit indexed, grey, olx_sprite_to_alpha
crates/olr-format/src/levels/mod.rs     LevelSource, LevelObject, compile_level, lift_level
crates/olr-format/src/levels/files.rs   level.ron, material.png, front.png, back.png, thumb.png
crates/olr-format/src/themes/mod.rs     ThemeSource, StampImage, compile_theme, lift_theme
crates/olr-format/src/themes/files.rs   theme.ron, holes/, stones/, misc/, tiles, green_ball
crates/olr-format/src/skins/mod.rs      SkinSource, compile_skin, lift_skin
crates/olr-format/src/skins/files.rs    skin.ron, skin.png, tint.png
crates/olr-format/src/core/mod.rs       CoreSource, SpriteSource, CORE_LAYOUT, compile_core, lift_core
crates/olr-format/src/core/files.rs     core.ron, gfx/, sounds/
crates/olr-format/tests/levels.rs       bundled levels round-trip; synthetic art scale 2
crates/olr-format/tests/themes.rs       bundled theme; hostile stamps
crates/olr-format/tests/skins.rs        all bundled skins: windows, masks, colorized output
crates/olr-format/tests/core.rs         core pack; mod gfx key-to-alpha
crates/olr-pack/src/image_io.rs         becomes `pub use olr_format::image_io::*;`
crates/olr-content/src/skins.rs         skin_source facing left: the true mirror
docs/porting/olx-notes/asset-formats.md research (committed with the plan)
```

Every new test file is a `mod` of `crates/olr-format/tests/integration.rs`.

---

### Task 1: Images in `olr-format`

**Files:**
- Create: `crates/olr-format/src/image_io.rs` (moved from
  `crates/olr-pack/src/image_io.rs`, with its tests)
- Modify: `crates/olr-pack/src/image_io.rs` (becomes a re-export),
  `crates/olr-pack/Cargo.toml` (+ `olr-format`), `crates/olr-format/Cargo.toml`
  (+ `png`), `crates/olr-format/src/lib.rs`, `Cargo.toml` (`png = "=0.18.1"`),
  `xtask/layers.ron` (the pair, the `wasm` list)

**Interfaces:**
- Produces, in `olr_format::image_io`: everything `olr_pack::image_io` has
  today, under the same names and signatures:
  - `encode_rgba`, `png_size`;
  - `ImageBound`, `MAX_IMAGE_SIDE`, `IMAGES`, `LEVEL_ART`;
  - `decode_rgba`, `decode_rgba_within`;
  - `encode_indexed`, `decode_indexed`, `decode_indexed_within`;
  - `declared_png`.

  New:

  ```rust
  /// `decode_indexed_within` now also accepts 1-, 2- and 4-bit palettes (indices unpacked to one byte each).
  pub fn encode_grey(width: u32, height: u32, values: &[u8]) -> Vec<u8>   // 8-bit greyscale PNG
  pub fn decode_grey_within(bytes: &[u8], bound: ImageBound) -> Result<(u32, u32, Vec<u8>), String>
      // grey 1–16 bit → 8-bit values; grey+alpha → the grey; RGB/RGBA/indexed → error "a greyscale PNG"
  pub fn olx_sprite_to_alpha(img: &RgbaImage) -> RgbaImage
      // OLX's colour key, applied once: `img.clone()` with `key_transparent()` (corner rule per image)
  ```
- `olr_pack::image_io` stays a valid path:
  `pub use olr_format::image_io::*;`.

- [ ] **Step 1: Move the module and its tests**

`git mv crates/olr-pack/src/image_io.rs crates/olr-format/src/image_io.rs`.
Create a new `crates/olr-pack/src/image_io.rs` containing:

```rust
//! PNG encoding and decoding for packs: `olr_format::image_io` (plan 13b
//! moved it there, so `olr-format` reads and writes images without
//! depending on `olr-pack`).

pub use olr_format::image_io::*;
```

Then make these changes:
- Add `pub mod image_io;` to `crates/olr-format/src/lib.rs`, and
  `png.workspace = true` to `crates/olr-format/Cargo.toml`.
- Add `olr-format.workspace = true` to `crates/olr-pack/Cargo.toml`.
- In `xtask/layers.ron`, add `("olr-pack", "olr-format"),` to
  `same_layer` (with a `// Packs read and write images through the
  authoring crate (plan 13b).` comment), and `"olr-format"` to the `wasm`
  list.
- In the workspace `Cargo.toml`, change `png = "0.18.1"` to
  `png = "=0.18.1"`, with the comment `# Exact: encoded PNG bytes are
  content (hashes).`

- [ ] **Step 2: Write the failing tests for the new functions**

Append to `crates/olr-format/src/image_io.rs`'s test module:

```rust
    fn indexed_png(width: u32, height: u32, depth: png::BitDepth, packed_rows: &[u8], palette: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, width, height);
            enc.set_color(png::ColorType::Indexed);
            enc.set_depth(depth);
            enc.set_palette(palette.to_vec());
            enc.write_header().unwrap().write_image_data(packed_rows).unwrap();
        }
        out
    }

    #[test]
    fn low_bit_palettes_read_like_eight_bit_ones() {
        let pal = [0, 0, 0, 255, 255, 255, 255, 0, 255, 9, 9, 9];
        // 3×2 at 2 bits: rows [0,1,2] and [3,2,1], each padded to a byte.
        let png2 = indexed_png(3, 2, png::BitDepth::Two, &[0b00_01_10_00, 0b11_10_01_00], &pal);
        assert_eq!(decode_indexed_within(&png2, IMAGES).unwrap(), (3, 2, vec![0, 1, 2, 3, 2, 1]));
        // 4×1 at 1 bit: [1,0,1,1].
        let png1 = indexed_png(4, 1, png::BitDepth::One, &[0b1011_0000], &pal[..6]);
        assert_eq!(decode_indexed_within(&png1, IMAGES).unwrap(), (4, 1, vec![1, 0, 1, 1]));
        // 3×1 at 4 bits: [2,0,3].
        let png4 = indexed_png(3, 1, png::BitDepth::Four, &[0x20, 0x30], &pal);
        assert_eq!(decode_indexed_within(&png4, IMAGES).unwrap(), (3, 1, vec![2, 0, 3]));
    }

    #[test]
    fn an_rgb_png_is_not_an_indexed_one() {
        let rgb = encode_rgba(&RgbaImage::new(2, 2));
        let e = decode_indexed_within(&rgb, IMAGES).unwrap_err();
        assert!(e.contains("8-bit or lower indexed PNG"), "{e}");
    }

    #[test]
    fn grey_round_trips_and_grey_alpha_reads_its_grey() {
        let png = encode_grey(3, 1, &[0, 128, 255]);
        assert_eq!(decode_grey_within(&png, IMAGES).unwrap(), (3, 1, vec![0, 128, 255]));
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(png::ColorType::GrayscaleAlpha);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[200, 10, 7, 255]).unwrap();
        }
        assert_eq!(decode_grey_within(&out, IMAGES).unwrap(), (2, 1, vec![200, 7]));
        assert!(decode_grey_within(&encode_rgba(&RgbaImage::new(1, 1)), IMAGES).unwrap_err().contains("greyscale"));
        assert!(decode_grey_within(&declared_png(100_000, 100_000, false), IMAGES).is_err());
    }

    #[test]
    fn the_olx_key_becomes_alpha_once() {
        use olr_data::image::Rgba;
        let mut img = RgbaImage::new(3, 1);
        img.set(0, 0, Rgba::MAGENTA);
        img.set(1, 0, Rgba::rgb(1, 2, 3));
        img.set(2, 0, Rgba { r: 9, g: 9, b: 9, a: 128 });
        let a = olx_sprite_to_alpha(&img);
        assert_eq!(a.get(0, 0), Rgba::TRANSPARENT);
        assert_eq!(a.get(1, 0), Rgba::rgb(1, 2, 3));
        assert_eq!(a.get(2, 0), Rgba { r: 9, g: 9, b: 9, a: 128 }, "partial alpha survives");
        // A (254,0,254) corner keys 254, and pure magenta stays a colour.
        let mut k = RgbaImage::new(2, 1);
        k.set(0, 0, Rgba::rgb(254, 0, 254));
        k.set(1, 0, Rgba::MAGENTA);
        let a = olx_sprite_to_alpha(&k);
        assert_eq!((a.get(0, 0), a.get(1, 0)), (Rgba::TRANSPARENT, Rgba::MAGENTA));
        // Applying OLX's key again changes nothing.
        let mut again = a.clone();
        again.key_transparent();
        assert_eq!(again, a);
    }
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo nextest run -p olr-format image_io`
Expected: the build fails (the new functions are missing). The moved
tests compile.

- [ ] **Step 4: Implement**

**`decode_indexed_within`:** read with
`png::Transformations::IDENTITY` and accept `ColorType::Indexed` at
depths 1, 2, 4 and 8. For depths below 8, unpack each row's packed bits,
most significant bits first, into one byte per index. Rows are padded to
whole bytes, and `info.line_size` gives the stride. Any other colour type
returns `Err("not an 8-bit or lower indexed PNG (palette)".into())`; keep
the word "palette", which existing tests and messages use.

```rust
/// Unpacks `depth`-bit indices (1, 2 or 4), most significant first, of a
/// `width`-wide row.
fn unpack_row(row: &[u8], depth: u8, width: usize, out: &mut Vec<u8>) {
    let per_byte = 8 / depth as usize;
    let mask = (1u8 << depth) - 1;
    for i in 0..width {
        let byte = row[i / per_byte];
        let shift = 8 - depth as usize * (i % per_byte + 1);
        out.push((byte >> shift) & mask);
    }
}
```

**`encode_grey`:** `ColorType::Grayscale`, `BitDepth::Eight`.

**`decode_grey_within`:** `normalize_to_color8()`. `Grayscale` gives the
values directly, and `GrayscaleAlpha` gives every other byte. Any other
colour type returns `Err("not a greyscale PNG".into())`. Check the header
bound through `reader` as the other decoders do.

**`olx_sprite_to_alpha`:**

```rust
/// OLX's colour key applied once (`RgbaImage::key_transparent`: the corner
/// rule per image, `src/client/GfxPrimitives.cpp:114-150`): key pixels
/// become `(0, 0, 0, 0)`, everything else is unchanged. Importers use it;
/// nothing applies it at load once plan 13c switches the loaders.
pub fn olx_sprite_to_alpha(img: &RgbaImage) -> RgbaImage {
    let mut out = img.clone();
    out.key_transparent();
    out
}
```

- [ ] **Step 5: Run, then the repository check**

Run: `cargo nextest run -p olr-format -p olr-pack`
Expected: PASS.

Run: `cargo xtask check` (this plan's first).
Expected: `check passed`. The layers, wasm and every `olr_pack::image_io`
caller must build unchanged.

- [ ] **Step 6: Commit**

```bash
git add -A crates/olr-format crates/olr-pack Cargo.toml Cargo.lock xtask/layers.ron
git commit -m "olr-format: images move here; low-bit palettes, grey, OLX key to alpha"
```

---

### Task 2: Levels

**Files:**
- Create: `crates/olr-format/src/levels/mod.rs`, `levels/files.rs`,
  `crates/olr-format/tests/levels.rs`
- Modify: `crates/olr-format/src/lib.rs` (`pub mod levels;`),
  `tests/integration.rs` (`mod levels;`)

**Interfaces:**
- Consumes: `image_io` (Task 1), `ron_io`, `FormatError`, `LiftError`.
- Produces:

```rust
pub struct LevelSource {
    pub name: String,
    pub notes: String,
    pub size: (u32, u32),        // world pixels
    pub art_scale: u32,          // 1..=MAX_ART_SCALE
    pub theme: String,           // theme pack id
    pub objects: Vec<LevelObject>,
    pub material: Vec<Material>, // size.0 * size.1, row-major
    pub front: RgbaImage,        // size × art_scale
    pub back: RgbaImage,         // size × art_scale
    pub thumb: Option<Vec<u8>>,  // thumb.png bytes, passed through (presentation only)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum LevelObject {
    /// A spawn point (world pixels). The 0.2 runtime does not read spawns yet.
    Spawn { at: (i32, i32), #[serde(default, skip_serializing_if = "String::is_empty")] notes: String },
}
pub const MAX_ART_SCALE: u32 = 8;
pub const MAX_OBJECTS: usize = 4096;
pub const MATERIAL_PALETTE: [u8; 9] = [128, 128, 128, 0, 0, 0, 139, 90, 43]; // rock, background, dirt
pub fn compile_level(src: &LevelSource) -> Result<olr_data::level::Level, FormatError>
pub fn lift_level(level: &olr_data::level::Level, thumb: Option<Vec<u8>>) -> Result<LevelSource, LiftError>
pub fn write_level_files(sink: &mut dyn ContentSink, src: &LevelSource) -> Result<(), FormatError>
pub fn read_level_files(src: &dyn ContentSource) -> Result<LevelSource, FormatError>
```

**`level.ron`:** `(name, notes?, size: (w, h), art_scale? (default 1),
theme, objects? (default []))`, with `deny_unknown_fields`. This is the
`LevelFile` wire struct in `files.rs`.

**Rules:**
- **Compile checks:**
  - `size` is at least 1 × 1 and at most 4096 × 4096 (OLX's 12-bit
    positions);
  - `art_scale` is 1..=8;
  - `material.len() == w * h`;
  - `front` and `back` are exactly `(w * art_scale, h * art_scale)`,
    computed with checked multiplication;
  - `objects.len() <= MAX_OBJECTS`.

  Each failure is a `FormatError::invalid("level.ron", <field>, …)`, or
  names the image file for size mismatches.
- **Compile output:** `LevelDef { name, width, height, art_scale, theme,
  spawns }` (one `SpawnPoint` per `Spawn` object, in order), and
  `MaterialGrid::from_cells`.
- **Lift** is the inverse; `notes` stay empty.
- **Files:**
  - `material.png` is written with `encode_indexed` and
    `MATERIAL_PALETTE`. It is read with `decode_indexed_within(LEVEL_ART)`,
    and each index goes through `Material::from_index`. An index of 3 or
    more is `"material.png: unknown material index v at (x, y)"`.
  - `front.png` and `back.png` are `encode_rgba` /
    `decode_rgba_within(LEVEL_ART)`.
  - `thumb.png` is passed through as bytes when present.
  - Other files are not this module's; reading ignores them.

- [ ] **Step 1: Write the failing tests**

In `levels/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::image::Rgba;

    pub(crate) fn tiny(art_scale: u32) -> LevelSource {
        let (w, h) = (3, 2);
        let mut front = RgbaImage::new(w * art_scale, h * art_scale);
        front.set(0, 0, Rgba::rgb(1, 2, 3));
        LevelSource {
            name: "Tiny".into(),
            notes: String::new(),
            size: (w, h),
            art_scale,
            theme: "olx-theme-dirt".into(),
            objects: vec![LevelObject::Spawn { at: (1, 1), notes: "here".into() }],
            material: vec![Material::Rock, Material::Background, Material::Dirt, Material::Dirt, Material::Background, Material::Rock],
            front,
            back: RgbaImage::new(w * art_scale, h * art_scale),
            thumb: None,
        }
    }

    #[test]
    fn compile_and_lift_are_inverse() {
        for scale in [1, 2] {
            let src = tiny(scale);
            let level = compile_level(&src).unwrap();
            assert_eq!(level.def.spawns, vec![olr_data::level::SpawnPoint { x: 1, y: 1 }]);
            assert_eq!(level.material.get(2, 0), Material::Dirt);
            let mut back = lift_level(&level, None).unwrap();
            assert_eq!(back.objects, vec![LevelObject::Spawn { at: (1, 1), notes: String::new() }]);
            back.objects = src.objects.clone(); // notes are authoring-only
            assert_eq!(back, src);
        }
    }

    #[test]
    fn sizes_must_agree() {
        let mut s = tiny(1);
        s.material.pop();
        assert!(compile_level(&s).unwrap_err().to_string().contains("material"));
        let mut s = tiny(2);
        s.front = RgbaImage::new(6, 3);
        let e = compile_level(&s).unwrap_err().to_string();
        assert!(e.contains("front.png") && e.contains("6x4"), "{e}");
        let mut s = tiny(1);
        s.art_scale = 9;
        assert!(compile_level(&s).is_err());
        let mut s = tiny(1);
        s.size = (5000, 1);
        assert!(compile_level(&s).is_err());
    }
}
```

In `levels/files.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use olr_vfs::MemFs;

    #[test]
    fn files_round_trip_byte_stably() {
        let mut src = crate::levels::tests::tiny(2);
        src.thumb = Some(b"not even a png".to_vec());
        let mut a = MemFs::new();
        write_level_files(&mut a, &src).unwrap();
        let names: Vec<String> = a.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(names, ["back.png", "front.png", "level.ron", "material.png", "thumb.png"]);
        let back = read_level_files(&a).unwrap();
        assert_eq!(back, src);
        let mut b = MemFs::new();
        write_level_files(&mut b, &back).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_bad_material_index_names_the_pixel() {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &crate::levels::tests::tiny(1)).unwrap();
        let bad = image_io::encode_indexed(3, 2, &[0, 1, 2, 7, 1, 0], &[0; 24]);
        let fs = fs.with("material.png", bad);
        let e = read_level_files(&fs).unwrap_err().to_string();
        assert!(e.contains("material.png") && e.contains("index 7 at (0, 1)"), "{e}");
    }

    #[test]
    fn level_ron_is_short() {
        let mut fs = MemFs::new();
        let mut src = crate::levels::tests::tiny(1);
        src.objects.clear();
        write_level_files(&mut fs, &src).unwrap();
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec()).unwrap();
        assert!(!text.contains("art_scale") && !text.contains("objects") && !text.contains("notes"), "{text}");
    }
}
```

`tests/levels.rs` checks the bundled levels and a synthetic art-scale-2
level, using today's reader as the baseline:

```rust
//! Bundled levels: today's `olr_pack::read_level`, lifted, written, read
//! and compiled, give the same `Level` (spec §6).

use std::path::PathBuf;

use olr_format::levels::{compile_level, lift_level, read_level_files, write_level_files};
use olr_vfs::{DirFs, MemFs};

const LEVELS: [&str; 4] =
    ["olx-level-castlestrike", "olx-level-dirt-level", "olx-level-fossilfacility", "olx-level-liero-factory"];

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

#[test]
fn bundled_levels_survive_lift_write_read_and_compile() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    for id in LEVELS {
        let dir = DirFs::new(root.join(id));
        let pack = olr_pack::read_level(&dir).unwrap();
        let thumb = std::fs::read(root.join(id).join("thumb.png")).ok();
        let src = lift_level(&pack.level, thumb.clone()).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &src).unwrap();
        let back = read_level_files(&fs).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(back, src, "{id}");
        assert_eq!(compile_level(&back).unwrap(), pack.level, "{id}: Level");
        assert_eq!(back.thumb, thumb, "{id}: thumbnail passes through");
    }
}
```

Use the field name `olr_pack::LevelPack` really has for the `Level`. Read
`crates/olr-pack/src/level_pack.rs`; if it is `level`, the code above
stands.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p olr-format levels`
Expected: the build fails.

- [ ] **Step 3: Implement `levels/mod.rs` and `levels/files.rs`**

Follow the rules above. The error helpers are `FormatError::invalid` and
`FormatError::Io` (13a's). Image size errors read like
`"front.png is 6x3, the level needs 6x4 (size × art_scale)"`.

`LevelSource`'s `PartialEq` compares images and materials as values, so
the round trip compares everything.

- [ ] **Step 4: Run the tests**

Run: `cargo nextest run -p olr-format`
Expected: PASS. The bundled-levels test runs; check for no `skip:` line
with `--no-capture`.

- [ ] **Step 5: Commit**

```bash
git add -A crates/olr-format
git commit -m "olr-format: levels"
```

---

### Task 3: Themes

**Files:**
- Create: `crates/olr-format/src/themes/mod.rs`, `themes/files.rs`,
  `crates/olr-format/tests/themes.rs`
- Modify: `crates/olr-format/src/lib.rs` (`pub mod themes;`),
  `tests/integration.rs`; `crates/olr-format/Cargo.toml` (dev-dependency
  `olr-terrain.workspace = true`)

**Interfaces:**

```rust
/// A stamp: what each pixel means, and the paint layer.
#[derive(Clone, Debug, PartialEq)]
pub struct StampImage {
    pub mask: Vec<u8>,      // row-major, paint.width() * paint.height(); 0, 1 or 2
    pub paint: RgbaImage,   // the paint (or rim) colour where mask == 2, (0,0,0,0) elsewhere
}
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSource {
    pub name: String,
    pub notes: String,
    /// Index = hole size (OLX's Hole1..): mask 0 untouched, 1 carve, 2 paint (the dirt edge).
    pub holes: Vec<StampImage>,
    /// Green dirt: mask 0 untouched (OLX's pink), 1 green, 2 rim.
    pub green_ball: Option<StampImage>,
    pub stones: Vec<RgbaImage>,   // alpha; OLX map-editor art, no runtime reader in 0.2
    pub misc: Vec<RgbaImage>,
    pub front_tile: RgbaImage,
    pub back_tile: RgbaImage,
}
pub const MAX_STAMP_SIDE: u32 = 64;
pub const MAX_HOLES: usize = 16;
pub const MAX_THEME_IMAGES: usize = 64;
pub const HOLE_PALETTE: [u8; 9] = [0, 0, 0, 255, 0, 255, 255, 255, 255];   // untouched, carve, paint
pub const GREEN_PALETTE: [u8; 9] = [0, 0, 0, 0, 255, 0, 255, 255, 255];    // untouched, green, rim
pub fn compile_theme(src: &ThemeSource) -> Result<olr_data::theme::Theme, FormatError>
pub fn lift_theme(t: &olr_data::theme::Theme) -> Result<ThemeSource, LiftError>
pub fn write_theme_files(sink: &mut dyn ContentSink, src: &ThemeSource) -> Result<(), FormatError>
pub fn read_theme_files(src: &dyn ContentSource) -> Result<ThemeSource, FormatError>
```

**`theme.ron`:** `(name, notes?, holes: n, stones: n?, misc: n?,
green_ball: bool?)` (counts default 0; `green_ball` defaults to false).

**Files:**

| Content | Files |
|---|---|
| Holes | `holes/<i>.png` (paint, RGBA) and `holes/<i>.mask.png` (indexed, `HOLE_PALETTE`), 1-based |
| Green ball | `green_ball.png` and `green_ball.mask.png` (`GREEN_PALETTE`) |
| Stones and misc | `stones/<i>.png`, `misc/<i>.png` (RGBA with alpha) |
| Tiles | `front_tile.png`, `back_tile.png` |

**Compile to today's OLX-coded `Theme`**, which `olr-terrain` reads
unchanged:

| Image | Mask 0 | Mask 1 | Mask 2 | Error |
|---|---|---|---|---|
| Hole pixel | `(0,0,0,255)` | `(255,0,255,255)` | the paint RGBA | a paint pixel whose RGB is black or magenta (it would read back as untouched or carve), naming the hole and pixel |
| Green-ball pixel | `(255,0,255,255)` | `(0,255,0,255)` | the paint RGBA as authored | a paint exactly `(0,255,0,255)` or `(255,0,255,255)` |

- **Stones and misc:** alpha 0 becomes `(255,0,255,255)`; every other
  pixel is kept.
- **The definition:** `ThemeDef { name, holes, stones, misc }` (counts).
- **Bounds:** every stamp side at most `MAX_STAMP_SIDE`; at most
  `MAX_HOLES` holes; at most `MAX_THEME_IMAGES` stones and misc; mask
  values 0..=2; mask length equal to the image's area.

**Lift from today's `Theme`:**
- **A hole pixel** with RGB magenta is carve, RGB black is untouched, and
  anything else is paint (kept). A carve or untouched pixel whose alpha is
  not 255 is a `LiftError` (Review Focus 4).
- **A green-ball pixel** that is exactly `(255,0,255,255)` is untouched,
  exactly `(0,255,0,255)` is green, and anything else is rim (kept).
- **Stones and misc** go through `image_io::olx_sprite_to_alpha`. It is a
  `LiftError` if:
  - the image's key is (254, 0, 254) (compile writes 255);
  - a magenta pixel's alpha is not 255;
  - a non-key pixel has alpha 0.

- [ ] **Step 1: Write the failing tests**

In `themes/mod.rs`, unit tests:
- A synthetic theme (two 3 × 3 holes using all three meanings, a green
  ball with all three, one stone with a transparent pixel, 2 × 2 tiles)
  compiles to the expected OLX colours pixel by pixel, and
  `lift_theme(compile_theme(src)) == src`.
- A hole paint pixel `(0,0,0,255)` with mask 2 is an error naming
  `holes[0]` and the pixel.
- A green rim of `(0,255,0,255)` is an error.
- `compile_theme` with 17 holes, or a 65-px-wide stamp, is an error that
  names the bound.
- `lift_theme` of a theme whose hole has a `(255,0,255,128)` pixel is a
  `LiftError`.
- `lift_theme` of a stone keyed by a (254,0,254) corner is a `LiftError`.

In `themes/files.rs`, unit tests:
- The files round-trip byte-stably (write, read equal, write again equal).
- The listed file layout is exactly the table above.
- A `holes/2.mask.png` whose size differs from `holes/2.png` names both
  files.
- A mask index of 3 names the file and pixel.
- A hole PNG declaring 10 000 × 10 000 (`image_io::declared_png`) is an
  error before decoding, naming the file. It is read with
  `ImageBound { max_side: MAX_STAMP_SIDE, max_pixels: (MAX_STAMP_SIDE * MAX_STAMP_SIDE) as u64 }`.
- A `theme.ron` claiming 1000 holes is an error before any image is read.

`tests/themes.rs` (bundled `olx-theme-dirt`):

```rust
//! The bundled theme: today's `olr_pack::read_theme`, lifted, written, read
//! and compiled, gives the same `Theme` and so the same stamps and green
//! mask for the simulation.

use std::path::PathBuf;

use olr_format::themes::{compile_theme, lift_theme, read_theme_files, write_theme_files};
use olr_terrain::{GreenMask, Stamp};
use olr_vfs::{DirFs, MemFs};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

#[test]
fn the_bundled_theme_survives_lift_write_read_and_compile() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let pack = olr_pack::read_theme(&DirFs::new(root.join("olx-theme-dirt"))).unwrap();
    let src = lift_theme(&pack.theme).unwrap();
    let mut fs = MemFs::new();
    write_theme_files(&mut fs, &src).unwrap();
    let back = read_theme_files(&fs).unwrap();
    assert_eq!(back, src);
    let theme = compile_theme(&back).unwrap();
    assert_eq!(theme, pack.theme, "Theme");
    for (a, b) in theme.holes.iter().zip(&pack.theme.holes) {
        assert_eq!(Stamp::from_image(a), Stamp::from_image(b));
    }
    let gm = |t: &olr_data::theme::Theme| t.green_ball.as_ref().map(GreenMask::from_image);
    assert_eq!(gm(&theme), gm(&pack.theme));
}
```

Use the field names and paths `olr_pack::ThemePack` and `olr_terrain`
really export. Read `crates/olr-pack/src/theme_pack.rs` and
`target/api/olr_terrain.txt` (`cargo xtask api`). If `Stamp` or
`GreenMask` lack `PartialEq`, compare their `Debug` text instead of
changing `olr-terrain`.

- [ ] **Step 2: Run them to see them fail** (`cargo nextest run -p olr-format themes`; a build failure)

- [ ] **Step 3: Implement** `themes/mod.rs` and `themes/files.rs` per the
  rules. Masks are read with `decode_indexed_within` (low-bit palettes
  accepted) and written with `encode_indexed` and the palette of their
  kind.

- [ ] **Step 4: Run the tests** (`cargo nextest run -p olr-format`; PASS, and the bundled test runs)

- [ ] **Step 5: Commit** (`olr-format: themes`)

---

### Task 4: The skin mirror quirk is dropped

**Files:**
- Modify: `crates/olr-content/src/skins.rs` (`skin_source`), its tests,
  any `olr-scene` or `olr-ui` test that pins a left-facing source
  rectangle, `docs/porting/STATUS.md`, `CHANGELOG.md`
- Re-bless: `fixtures/render/*.png`, `fixtures/ui/*.png` (only those
  that change)

**Interfaces:** `olr_content::skin_source(f, Dir::Left)` returns
`x = f * 32 + 4` (as facing right) with `mirrored: true`.

- [ ] **Step 1: Write the failing test**

In `crates/olr-content/src/skins.rs`'s tests:

```rust
    /// A left-facing frame is the exact mirror of the right-facing one
    /// (OLX's one-column offset, `CGameSkin.cpp:597-601`, dropped: spec
    /// 2026-10-16 formats §4).
    #[test]
    fn left_is_the_mirror_of_right() {
        let mut sheet = RgbaImage::new(672, 18);
        for x in 0..672 {
            for y in 0..18 {
                sheet.set(x, y, Rgba { r: (x % 251) as u8, g: y as u8, b: (x / 251) as u8, a: 255 });
            }
        }
        for f in 0..21 {
            let (r, l) = (skin_sprite(&sheet, f, Dir::Right), skin_sprite(&sheet, f, Dir::Left));
            for y in 0..18 {
                for x in 0..20 {
                    assert_eq!(l.get(x, y), r.get(19 - x, y), "frame {f} ({x}, {y})");
                }
            }
        }
    }
```

Use the imports the module's tests already have (`Rgba`, `RgbaImage`,
`Dir`).

- [ ] **Step 2: Run it to see it fail** (`cargo nextest run -p olr-content skins`)

- [ ] **Step 3: Implement**

Change `Dir::Left => (sx + 1, true)` to `Dir::Left => (sx, true)`. Rewrite
`skin_source`'s doc comment: say that facing left mirrors the
right-facing rectangle exactly, and that OLX's one-column offset
(`CGameSkin.cpp:584-600`) is dropped (spec 2026-10-16 formats §4). Update
any existing test that pins the old `sx + 1`.

- [ ] **Step 4: Every bundled skin**

Add an `olr-content` integration test (in `tests/content.rs`, which reads
the imported content through `content_dir()`). It loads every
`olx-skin-*` pack with `Content::load_skin` and checks
`left_is_the_mirror_of_right`'s property on its colorized sheet (any
colour). That is Review Focus 5.

- [ ] **Step 5: Re-bless what changed**

Run `cargo nextest run -p olr-content -p olr-scene -p olr-ui` and fix
the tests that pinned the quirk, keeping what they test. Then run
`cargo xtask bless-render --reason "Left-facing worms mirror their
right-facing frame exactly (OLX's one-column skin offset dropped, spec
2026-10-16 formats §4)"`. Look at every re-blessed PNG's diff
(`git diff --stat fixtures/`): only worms facing left may differ.

- [ ] **Step 6: Records**

**STATUS.md:** a deviation entry. "Left-facing skin frames mirror the
right-facing frame exactly. OLX draws the source columns one to the
right (`CGameSkin.cpp:597-601`), showing a column of the frame's spacing
and losing one of the sprite. Dropped 2026-10-16 (the user's ruling);
render goldens re-blessed."

**CHANGELOG.md** `Unreleased`, under a `### Changed` heading: "Worms
facing left are drawn as the exact mirror of facing right, without
OLX's one-pixel offset."

- [ ] **Step 7: The repository check, then commit**

Run: `cargo xtask check` (this plan's second).
Expected: `check passed`.

```bash
git add -A crates/olr-content crates/olr-scene crates/olr-ui fixtures docs/porting/STATUS.md CHANGELOG.md
git commit -m "olr-content: left-facing worms mirror their right-facing frame exactly"
```

---

### Task 5: Skins

**Files:**
- Create: `crates/olr-format/src/skins/mod.rs`, `skins/files.rs`,
  `crates/olr-format/tests/skins.rs`
- Modify: `crates/olr-format/src/lib.rs`, `tests/integration.rs`

**Interfaces:**

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct SkinSource {
    pub name: String,
    pub notes: String,
    pub art_scale: u32,             // OLX: 2
    pub frame_size: (u32, u32),     // OLX: (20, 18)
    pub aim_frames: u32,            // OLX: 7 (columns)
    pub walk_frames: u32,           // OLX: 3 (rows)
    pub image: RgbaImage,           // (frame_size.0 * aim_frames) × (frame_size.1 * walk_frames)
    pub tint: Vec<u8>,              // greyscale, same size: 0 keep, 255 player colour
    pub left: Option<RgbaImage>,    // explicit left-facing frames; not supported by the 0.2 runtime
}
pub const OLX_SKIN: (u32, (u32, u32), u32, u32) = (2, (20, 18), 7, 3); // art_scale, frame_size, aim, walk
pub fn compile_skin(src: &SkinSource) -> Result<olr_data::skin::Skin, FormatError>
pub fn lift_skin(skin: &olr_data::skin::Skin) -> Result<SkinSource, LiftError>
pub fn write_skin_files(sink: &mut dyn ContentSink, src: &SkinSource) -> Result<(), FormatError>
pub fn read_skin_files(src: &dyn ContentSource) -> Result<SkinSource, FormatError>
```

**`skin.ron`:** `(name, notes?, art_scale, frame_size, aim_frames,
walk_frames)`, all written: the layout is the point of the file.

**Files:**
- `skin.png` (RGBA);
- `tint.png` (`encode_grey`; read with `decode_grey_within(IMAGES)`, so
  grey with alpha reads its grey: Review Focus 1);
- `left.png` (optional, RGBA).

**Compile (to today's OLX sheet):**
- **Errors:**
  - "not supported by this runtime yet" for a layout other than
    `OLX_SKIN`, a `left` image, or a tint value other than 0 or 255
    (naming the first pixel);
  - `image` not exactly 140 × 54, or `tint.len()` not equal to its area
    (naming `skin.png` and `tint.png`).
- **The sheet** is a 672 × 18 `RgbaImage::new` (transparent) with
  `mask = vec![Transparent; 672 * 18]`.
- **Each cell** (aim `a`, walk `w`) has frame `f = w * 7 + a` and sheet
  x `f * 32 + 4`. For each cell pixel `(x, y)`:
  - alpha 0 → stays `(0,0,0,0)`, `Transparent`;
  - else the pixel, with tint 0 → `Keep` and 255 → `Colorize`.
- **The definition:** `SkinDef { name, layout: FrameLayout::OLX }`.

**Lift:**
- `layout != FrameLayout::OLX` or a sheet other than 672 × 18 is a
  `LiftError`.
- Cut each frame's 20 × 18 window at `f * 32 + 4`. Per pixel:
  - `Transparent` → `(0,0,0,0)`, tint 0;
  - `Keep` → the pixel, tint 0;
  - `Colorize` → the pixel, tint 255.
- `art_scale` 2, `frame_size` (20, 18), 7 aim and 3 walk frames,
  `left: None`.

- [ ] **Step 1: Write the failing tests**

Unit tests in `skins/mod.rs`:
- A synthetic `SkinSource` compiles. The sheet pixel at
  `(f * 32 + 4 + x, y)` equals the cell pixel (or transparent), and every
  spacing column (offsets 0..4 and 24..32) is `Transparent` and
  `(0,0,0,0)`. Then `lift_skin(compile_skin(src)) == src`.
- A tint value of 128 is an error naming the pixel and "partial tint".
- A `left` image is an error.
- `art_scale: 1` is an error.
- A 141 × 54 image is an error naming `skin.png`.

Unit tests in `skins/files.rs`:
- The files round-trip byte-stably.
- `skin.ron` contains all six layout fields.
- A `tint.png` of another size names both files.
- A grey-with-alpha `tint.png` reads its grey.

`tests/skins.rs` (every bundled skin):

```rust
//! Every bundled skin: lifted from today's pack, written, read and
//! compiled, it draws the same: the same frame windows (masks, and pixels
//! where visible) and the same colorized frames.

use std::path::PathBuf;

use olr_data::image::Rgba;
use olr_data::skin::MaskKind;
use olr_format::skins::{compile_skin, lift_skin, read_skin_files, write_skin_files};
use olr_vfs::{DirFs, MemFs};

#[test]
fn every_bundled_skin_draws_the_same() {
    let Some(root) = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir()) else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let colours = [Rgba::rgb(255, 0, 0), Rgba::rgb(96, 128, 252), Rgba::rgb(10, 200, 30)];
    let mut n = 0;
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        let id = path.file_name().unwrap().to_string_lossy().into_owned();
        if !id.starts_with("olx-skin-") {
            continue;
        }
        let old = olr_pack::read_skin(&DirFs::new(&path)).unwrap().skin;
        let src = lift_skin(&old).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_skin_files(&mut fs, &src).unwrap();
        let back = read_skin_files(&fs).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(back, src, "{id}");
        let new = compile_skin(&back).unwrap_or_else(|e| panic!("{id}: {e}"));
        for f in 0..21u32 {
            for y in 0..18 {
                for x in 0..20 {
                    let sx = f * 32 + 4 + x;
                    let (mo, mn) = (old.mask_at(sx, y), new.mask_at(sx, y));
                    assert_eq!(mo, mn, "{id}: mask frame {f} ({x}, {y})");
                    if mo != MaskKind::Transparent {
                        assert_eq!(old.image.get(sx, y), new.image.get(sx, y), "{id}: pixel frame {f} ({x}, {y})");
                    }
                }
            }
        }
        for c in colours {
            let (a, b) = (old.colorize(c), new.colorize(c));
            for f in 0..21u32 {
                assert_eq!(a.crop(f * 32 + 4, 0, 20, 18), b.crop(f * 32 + 4, 0, 20, 18), "{id}: colour {c:?} frame {f}");
            }
        }
        n += 1;
    }
    assert!(n >= 700, "only {n} skins found");
}
```

Use the field names `olr_pack::SkinPack` really has (read
`crates/olr-pack/src/skin_pack.rs`). The test reads about 720 skins; if
it takes more than about 15 s in the dev profile, report the time (it is
then a candidate for the `full` tier, not something to cut).

- [ ] **Step 2: Run them to see them fail** (a build failure)

- [ ] **Step 3: Implement** per the rules.

- [ ] **Step 4: Run the tests** (PASS; the bundled test runs over 700 or more skins)

- [ ] **Step 5: Commit** (`olr-format: skins`)

---

### Task 6: The core pack, and mod gfx's key-to-alpha

**Files:**
- Create: `crates/olr-format/src/core/mod.rs`, `core/files.rs`,
  `crates/olr-format/tests/core.rs`
- Modify: `crates/olr-format/src/lib.rs`, `tests/integration.rs`

**Interfaces:**

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteSource {
    pub image: RgbaImage,        // alpha
    pub frames: (u32, u32),      // columns, rows; must divide the image
    pub art_scale: u32,          // 1..=8
}
#[derive(Clone, Debug, PartialEq)]
pub struct CoreSource {
    pub notes: String,
    pub sprites: BTreeMap<String, SpriteSource>,
    pub sounds: BTreeMap<String, Vec<u8>>,     // file name ("throw.ogg") → bytes
    pub colours: BTreeMap<String, Colour>,     // OLX's keys, e.g. "RopeColour1"
}
/// Frames and art scale of OLX's core sprites (`docs/porting/olx-notes/asset-formats.md` §5.2).
pub const CORE_LAYOUT: [(&str, (u32, u32), u32); 12] = [
    ("bonus", (1, 1), 2), ("chemsmoke", (4, 1), 2), ("crosshair", (2, 1), 2), ("explosion", (16, 1), 2),
    ("giblet-stains", (4, 1), 1), ("giblets", (4, 1), 2), ("health", (1, 1), 2), ("hook", (1, 1), 2),
    ("muzzle", (14, 1), 1), ("smoke", (5, 1), 2), ("sparkle", (3, 1), 2), ("spawn", (5, 1), 2),
];
pub fn compile_core(src: &CoreSource)
    -> Result<(olr_data::core_pack::CoreDef, BTreeMap<String, RgbaImage>, BTreeMap<String, Vec<u8>>), FormatError>
pub fn lift_core(def: &CoreDef, sprites: &BTreeMap<String, RgbaImage>, sounds: &BTreeMap<String, Vec<u8>>)
    -> Result<CoreSource, LiftError>
pub fn write_core_files(sink: &mut dyn ContentSink, src: &CoreSource) -> Result<(), FormatError>
pub fn read_core_files(src: &dyn ContentSource) -> Result<CoreSource, FormatError>
```

**`core.ron`:** `(notes?, sprites: {"crosshair": (frames: (2, 1),
art_scale: 2), …}, sounds: ["bump.ogg", …], colours: {"RopeColour1":
"#rrggbb", …})`.

**Files:** `gfx/<id>.png` (RGBA with alpha), `sounds/<file>` (bytes).

**Compile:**
- **Each sprite:** `frames` must divide the image (`width % c == 0`,
  `height % r == 0`, both at least 1), and `art_scale` is 1..=8;
  otherwise an error naming `gfx/<id>.png`.
- **Giblets:** if both `giblets` (32 × 8) and `giblet-stains` (16 × 4)
  exist, the runtime `giblets` is a 32 × 12 transparent image with
  `giblets` at (0, 0) and `giblet-stains` at (0, 8). `giblet-stains` is
  not a runtime sprite. One without the other, or other sizes, is an
  error.
- **The definition:**

  ```rust
  CoreDef {
      sprites: <runtime sprite names, sorted>,
      sounds: <sound file names, sorted>,
      colours: <Rgba by key>,
  }
  ```

**Lift:**
- Each sprite becomes `image_io::olx_sprite_to_alpha(img)`.
- `giblets` (32 × 12) is split: rows 0..8 are `giblets`; the region
  x 0..16, y 8..12 is `giblet-stains`. It is a `LiftError` if the rest of
  rows 8..12 (x 16..32) is not all transparent after keying.
- Frames and art scale come from `CORE_LAYOUT`; a sprite not in it gets
  (1, 1) and 2.
- Colours are copied; sounds are copied.

- [ ] **Step 1: Write the failing tests**

Unit tests in `core/mod.rs`:
- A synthetic core round-trips: `lift_core(compile_core(src)) == src`,
  with giblets composed and split.
- `frames: (3, 1)` on a 10-wide image is an error naming the file.
- `giblets` without `giblet-stains` is an error.
- A colour key round-trips.

Unit tests in `core/files.rs`:
- The files round-trip byte-stably.
- `core.ron` has no `Some(` and no struct names.

`tests/core.rs`:

```rust
//! The bundled core pack, and the bundled mods' gfx: OLX's colour key
//! becomes alpha once, and nothing a loader keys changes (spec §5, §6).

use std::path::PathBuf;

use olr_format::core::{compile_core, lift_core, read_core_files, write_core_files};
use olr_format::image_io::{decode_rgba, olx_sprite_to_alpha};
use olr_vfs::{DirFs, MemFs};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

fn keyed(img: &olr_data::image::RgbaImage) -> olr_data::image::RgbaImage {
    let mut k = img.clone();
    k.key_transparent();
    k
}

#[test]
fn the_bundled_core_pack_survives_lift_write_read_and_compile() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let pack = olr_pack::read_core(&DirFs::new(root.join("olx-core"))).unwrap();
    let src = lift_core(&pack.def, &pack.sprites, &pack.sounds).unwrap();
    let mut fs = MemFs::new();
    write_core_files(&mut fs, &src).unwrap();
    let back = read_core_files(&fs).unwrap();
    assert_eq!(back, src);
    let (def, sprites, sounds) = compile_core(&back).unwrap();
    assert_eq!(def, pack.def, "CoreDef");
    assert_eq!(sounds, pack.sounds, "sounds");
    assert_eq!(sprites.keys().collect::<Vec<_>>(), pack.sprites.keys().collect::<Vec<_>>());
    for (name, old) in &pack.sprites {
        let new = &sprites[name];
        assert_eq!(*new, keyed(old), "{name}: the alpha image is what a loader's key gives");
        assert_eq!(keyed(new), *new, "{name}: a loader keying it again changes nothing");
    }
}

#[test]
fn the_bundled_mods_gfx_key_to_alpha_without_change() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    for id in ["olx-classic", "olx-mw-1-0"] {
        let gfx = root.join(id).join("gfx");
        for entry in std::fs::read_dir(&gfx).unwrap() {
            let path = entry.unwrap().path();
            let img = decode_rgba(&std::fs::read(&path).unwrap()).unwrap();
            let alpha = olx_sprite_to_alpha(&img);
            assert_eq!(alpha, keyed(&img), "{}", path.display());
            assert_eq!(keyed(&alpha), alpha, "{}", path.display());
            assert_eq!((alpha.width(), alpha.height()), (img.width(), img.height()), "frames stay width / height");
        }
    }
}
```

- [ ] **Step 2: Run them to see them fail** (a build failure)

- [ ] **Step 3: Implement** per the rules.

- [ ] **Step 4: Run the tests** (PASS; both bundled tests run)

- [ ] **Step 5: Commit** (`olr-format: the core pack; OLX's key to alpha for mod gfx`)

---

### Task 7: Records, the check and the single `full`

**Files:**
- Modify: `crates/olr-format/CLAUDE.md`, `crates/olr-pack/CLAUDE.md`,
  `docs/porting/STATUS.md`

- [ ] **Step 1: Crate notes**

**`crates/olr-format/CLAUDE.md`:** add a paragraph per kind:
- **Levels:** the material palette, art scale, `Spawn` objects (unread by
  the runtime) and the bounds.
- **Themes:** the mask meanings and palettes, the compile-to-OLX-colours
  rule and why (`olr-terrain` unchanged), the colour errors and the
  bounds.
- **Skins:** the OLX layout as the only supported one, and what is a
  compile error.
- **Core:** `CORE_LAYOUT` and the giblets split.
- **Images:** `image_io` (low-bit palettes, grey, `olx_sprite_to_alpha`)
  and the exact `png` pin.

Add the dependency on `png`, and the tests and what each guards.

**`crates/olr-pack/CLAUDE.md`:** `image_io` is now `olr_format::image_io`,
re-exported.

- [ ] **Step 2: STATUS**

- Add a row to the main table for plan 13b's formats, like plan 13a's
  row.
- Add the recorded importer deviation (Ruling 20): "The skin importer
  hides an image pixel of the key colour under a tint mask, where OLX's
  `Colorize_Execute` draws it colorized (69 OLX source skins, e.g.
  `CT Cat.png`). To be fixed when plan 13c re-imports the skins."
- Add a note that `LevelDef::spawns` is not read by the runtime (spawns
  are `FindSpot`'s).

- [ ] **Step 3: The repository check and the single `full`**

Run: `cargo xtask check` (this plan's third). Expected: `check passed`.

Run: `cargo xtask full` (this plan's only one: the render goldens
changed in Task 4). Expected: `full passed`.

- [ ] **Step 4: Commit** (`docs: plan 13b's records`)
