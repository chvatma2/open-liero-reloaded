# Plan 14a: Level Composition Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A level can be composed of elements (imported images, drawn layers and a raster base), stored in the pack's `editor/` folder and baked deterministically into today's `material.png`, `front.png`, `back.png` and `thumb.png`. Agents can script it with `olr-cli level element …` and `level bake`. No GUI: that is plans 14b and 14c.

**Architecture:**
- `olr-format` gets the composition's file format (`levels::composition`) and JPEG decoding.
- `olr-pack` leaves `editor/` out of the content hash and the archives, and validates the composition's structure.
- `olr-edit` gets three modules:
  - the image pipeline (`pixelate`);
  - the bake (`compose`: `Baker`, `bake`, `bake_region`, `compose_from`);
  - the element operations (`element`).

  `olr_edit::pack::Pack` carries the composition.
- `olr-cli` wires `level element …`, `level bake`, the stale-bake check in `pack check`, and composition-aware `resize`.
- `olr-detcheck` prints a bake digest, so every `cargo xtask determinism` leg compares the bake across platforms.

**Tech Stack:** Rust 1.99, serde/ron through `olr_format::ron_io`, `png` (pinned), `zune-jpeg` `=0.5.15` with `zune-core` `=0.5.3` (both already in `Cargo.lock`), clap, proptest, sha2 (in `olr-detcheck`).

**Spec:** `docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md` §1, §2, §3, §7, §8 (14a's part) and §9.

## Global Constraints

- Never panic on content (repo rule 6). Originals, layers, `composition.ron` and command-line values are untrusted.
- Keep output deterministic (repo rule 7). No `HashMap` iteration in output.
- **Integer arithmetic only** in the image pipeline and the bake. Floats are banned there: the same composition must bake to the same bytes on Linux, Windows, WASI and arm64 (spec §3).
- `PACK_FORMAT_VERSION` stays 5. `editor/` is optional, and a level without it reads and behaves exactly as today.
- These must stay byte-identical, and are never blessed in this plan:
  - the oracle traces;
  - the match goldens;
  - the render goldens;
  - `crates/olr-cli/tests/snapshots/`;
  - `content/packs/`.
- Learn a dependency's API from `cargo xtask api` and `target/api/<crate>.txt`. For `zune-jpeg`, read its source in `~/.cargo/registry/src/*/zune-jpeg-0.5.15/` (it is not a workspace crate).
- Canonical text goes only through `olr_format::ron_io::{to_canonical, from_canonical}`.
- Limits (spec §2):
  - `MAX_ELEMENTS` = 256;
  - originals within `image_io::IMAGES`;
  - layers and processed art within `LEVEL_ART`;
  - an image box's sides 1..=`MAX_SIDE` (4096);
  - `pixel_size` 1..=`MAX_PIXEL_SIZE` (64);
  - `colours` 2..=256.
- Verify once per batch. Run targeted `cargo nextest run -p <crate>` while working, and the online `cargo xtask check` at the end of each batch:
  - Batch A is Tasks 1-3.
  - Batch B is Tasks 4-6.
  - Batch C is Tasks 7-9.
  - Task 10 is docs only, but it changes `docs/formats/`, so it runs `cargo xtask format-docs --check` and the guide test.
- **No `cargo xtask full`, bench or stress** without the user's consent. Task 9 runs `cargo xtask determinism` (the WASM leg, which is part of `check`) only.
- Commits are small, with an imperative subject (`olr-edit: …`), and end with the session's two trailer lines.

## Review Focus

1. **A hostile original:**
   - a JPEG or PNG whose header declares 100 000 × 100 000;
   - a truncated JPEG;
   - a file that is neither PNG nor JPEG.

   Each is refused before anything is allocated, with an error naming the file, and the pack is unchanged. Tests: Task 1 (`a_hostile_image_is_refused_before_decoding`), Task 8 (`add_image_refuses_a_hostile_file_and_writes_nothing`).
2. **An image box partly or wholly outside the level:** negative `at`, or a box larger than the level. It is clipped, never panics, and bakes the same in a region bake as in a full one. Test: Task 5 (`elements_outside_the_level_are_clipped`, and the proptest).
3. **Opening an OLX level changes no byte.**
   - A level composed only of its raster base bakes back to its exact files.
   - The first `level element` command on a plain level creates that base, then adds its element.

   Tests: Task 5 (`every_bundled_level_round_trips_through_a_raster_base`), Task 8 (`a_plain_level_gains_a_raster_base_first`).
4. **A hand-edited `composition.ron`:**
   - a duplicate id;
   - a missing image or layer file;
   - a layer of the wrong size;
   - too many elements.

   `pack check` reports each as an error naming the file. Edits under `editor/` never change the content hash or reach an archive. Tests: Task 2 (`check_composition_reports_each_problem`), Task 3 (`editor_files_are_not_content`).
5. **Colour reduction at the edges:**
   - fewer distinct colours than `colours`;
   - no covered pixel at all;
   - `colours: 2`;
   - a fully transparent original.

   None panics, and the output never holds more than `colours` colours. Test: Task 4 (`reduction_never_exceeds_n_and_tolerates_empty_input`).

## Rulings made while writing this plan

- **R1 Raster operations on a composed level are refused.** Spec §2 said `level carve|fill|paint|stamp` on a composed level "edit a drawn element named CLI edits". A drawn element has one material, while `carve` and `paint` act on several materials at once, so that cannot hold.
  - On a composed level the four commands exit 1 with: `<pack>: a composed level: edit its elements with olr-cli level element draw|erase, or in the editor`.
  - Agents paint with `level element draw` and `level element erase` (Task 8), which paint into a drawn layer.
  - Task 10 amends spec §2 and §7.
- **R2 `editor/` is "authoring", not hidden.** `olr_pack::is_authoring(path)` is true when the first component is `editor`. `content_hash` and `archive::encode` skip it, and `archive::decode` refuses it, so no archive (a download, a shared `.olpk`) ever holds it.
- **R3 Coverage per world pixel is the centre sample.**
  - An element *covers* world pixel (x, y) when it covers the art pixel `(x·s + s/2, y·s + s/2)`.
  - The material is the topmost visible covering element's material, or background when none covers.
  - Front is decided per art pixel: the topmost visible element covering that art pixel, else the back.
  - Then `front := back` on every background block, so the invariant holds by construction.
- **R4 The backdrop per art pixel** is the topmost visible element with a backdrop contribution at that pixel:
  - a background-material image or drawn element covering it gives its art;
  - a raster element gives its back (it covers everything);
  - dirt and rock image or drawn elements give none.

  With no contribution, the back is the theme's back tile at `(ax mod tw, ay mod th)`.
- **R5 Art is opaque.** Baked art is written with alpha 255; an element's colour keeps only its RGB. Drawn layers cover where alpha ≠ 0, and processed images where alpha = 255 (after the threshold).
- **R6 The raster base is the element `base`.** A level without a composition, composed (`compose_from`), becomes one raster element:
  - id `base`, name `Base`;
  - files `editor/layers/base.material.png`, `.front.png` and `.back.png`, encoded exactly as `write_level_files` encodes `material.png`, `front.png` and `back.png`.
- **R7 Image originals keep their bytes.** They are stored as imported, under `editor/images/<file name>`. The file name is a portable single component with the extension `png`, `jpg` or `jpeg` (any case); a taken name gets `-2`, `-3`, … before the extension.
- **R8 Ids** are slugs following `olr_format::mods::valid_id` (`[a-z0-9_.-]`, made crate-visible to `levels`), made from the element's name or file stem, lowercased, with other characters replaced by `-`, and `-2`, `-3`, … when taken.
- **R9 Back tile lookup.** `olr_edit::compose::theme_back_tile(content, theme_id)` finds the theme pack whose manifest id is `theme_id` among `content`'s immediate subdirectories, and reads `back_tile` with `olr_format::themes::read_theme_files`. The CLI's `--content` defaults to `content/packs`, as every other command's does.

---

### Task 1: JPEG decoding and image sizes from headers (`olr-format`)

**Files:**
- Modify: the workspace `Cargo.toml` (add `zune-jpeg = { version = "=0.5.15", default-features = false, features = ["std"] }` and `zune-core = "=0.5.3"` under `[workspace.dependencies]`, with a comment: "JPEG originals for level composition (plan 14a): exact pins, already locked through another dependency").
- Modify: `crates/olr-format/Cargo.toml` (`zune-jpeg.workspace = true`, `zune-core.workspace = true`).
- Modify: `crates/olr-format/src/image_io.rs`.

**Interfaces:**
- Produces, in `olr_format::image_io`:

  ```rust
  /// The width and height a PNG's `IHDR` or a JPEG's frame header gives,
  /// read without decoding pixels; `None` for anything else.
  pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)>;
  /// A PNG or a JPEG (by its first bytes: `\x89PNG` or `FF D8 FF`) decoded
  /// to RGBA8 within `bound`, the size checked from the header before any
  /// pixel is decoded. A JPEG is opaque (alpha 255).
  pub fn decode_image_within(bytes: &[u8], bound: ImageBound) -> Result<RgbaImage, String>;
  ```

- [ ] **Step 1: Write the failing tests** in `image_io.rs`'s `tests` module. The JPEG fixtures are made by a test helper that encodes nothing: commit two tiny JPEGs as test data under `crates/olr-format/tests/data/`.
  - `tiny.jpg`: 4 × 3, red.
  - `truncated.jpg`: the first 40 bytes of `tiny.jpg`.

  Make them once with any encoder (for example `python3 -c "from PIL import Image; Image.new('RGB',(4,3),(255,0,0)).save('tiny.jpg', quality=95)"`). If Pillow is missing, use `convert -size 4x3 xc:red tiny.jpg`. Commit them and note their origin in a `README.md` beside them.

  ```rust
  const TINY_JPG: &[u8] = include_bytes!("../tests/data/tiny.jpg");
  const TRUNCATED_JPG: &[u8] = include_bytes!("../tests/data/truncated.jpg");

  #[test]
  fn decodes_png_and_jpeg() {
      let png = encode_rgba(&RgbaImage::new(5, 2));
      assert_eq!(image_size(&png), Some((5, 2)));
      let img = decode_image_within(&png, IMAGES).unwrap();
      assert_eq!((img.width(), img.height()), (5, 2));
      assert_eq!(image_size(TINY_JPG), Some((4, 3)));
      let img = decode_image_within(TINY_JPG, IMAGES).unwrap();
      assert_eq!((img.width(), img.height()), (4, 3));
      let p = img.get(1, 1);
      assert!(p.r > 200 && p.g < 40 && p.b < 40 && p.a == 255, "{p:?}");
  }

  #[test]
  fn a_hostile_image_is_refused_before_decoding() {
      // A PNG declaring 100 000 × 100 000 (no pixel data).
      let huge = declared_png(100_000, 100_000, false);
      assert!(decode_image_within(&huge, IMAGES).unwrap_err().contains("100000"));
      // A JPEG whose frame header declares 60 000 × 60 000: patch tiny.jpg's SOF0 size.
      let mut big = TINY_JPG.to_vec();
      let sof = big.windows(2).position(|w| w == [0xFF, 0xC0]).expect("baseline SOF0");
      big[sof + 5..sof + 9].copy_from_slice(&[0xEA, 0x60, 0xEA, 0x60]);
      assert_eq!(image_size(&big), Some((60_000, 60_000)));
      assert!(decode_image_within(&big, IMAGES).is_err());
      assert!(decode_image_within(TRUNCATED_JPG, IMAGES).is_err());
      assert!(decode_image_within(b"GIF89a....", IMAGES).unwrap_err().contains("not a PNG or JPEG"));
      assert_eq!(image_size(b"nope"), None);
  }
  ```

- [ ] **Step 2:** Run `cargo nextest run -p olr-format image_io::`. Expected: FAIL (`image_size` and `decode_image_within` not found).
- [ ] **Step 3: Implement.**
  - `image_size`: for a PNG, return `png_size`'s result. For a JPEG, walk the markers from offset 2. At each `FF xx`, read the 2-byte big-endian segment length. Stop at the first SOF marker (`C0`-`C3`, `C5`-`C7`, `C9`-`CB`, `CD`-`CF`) and read height (2 bytes) and width (2 bytes) at offsets +5 and +7 from the marker's `FF`. Bound the walk by the slice: every index checked, never panicking.
  - `decode_image_within`: dispatch on the magic bytes. A PNG goes to `decode_rgba_within`. A JPEG:
    1. take its size from `image_size` and apply `check_size(w, h, bound)`, else `Err(format!("{w}x{h}: …"))`;
    2. decode with `zune_jpeg::JpegDecoder` with the output colourspace set to RGB (`zune_core::options::DecoderOptions::default().jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGB)`; check the exact 0.5.15 names in its source);
    3. after decoding, verify the size equals the header's;
    4. expand RGB to RGBA with alpha 255.

    Map every decoder error to `Err(String)`. Anything else gives `Err("not a PNG or JPEG")`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-format` and `cargo build -p olr-format --target wasm32-unknown-unknown` (olr-format is in `layers.ron`'s `wasm` list). Expected: PASS, and the wasm build succeeds. If `zune-jpeg` does not build for wasm32-unknown-unknown with these features, stop and ledger a ruling: move JPEG decoding to `olr-edit`, which is not a wasm crate, and keep `image_size` in `olr-format`.
- [ ] **Step 5:** Commit: `olr-format: JPEG decoding and image sizes from headers`.

### Task 2: The composition format (`olr-format`)

**Files:**
- Create: `crates/olr-format/src/levels/composition.rs`.
- Modify:
  - `crates/olr-format/src/levels/mod.rs` (`mod composition; pub use composition::*;`);
  - `crates/olr-format/src/mods/mod.rs` (re-export `valid_id` as `pub(crate)`; it is already `pub(crate) use compile::valid_id;`, so check);
  - `crates/olr-format/src/text.rs` (dispatch `editor/composition.ron`).
- Test: unit tests in `composition.rs`; `text.rs`'s tests.

**Interfaces:**
- Produces, in `olr_format::levels` (each item documented: these doc comments become `docs/formats/level.md` in Task 10, so state units, limits and defaults):

  ```rust
  pub const EDITOR_DIR: &str = "editor";
  pub const COMPOSITION_FILE: &str = "editor/composition.ron";
  pub const MAX_ELEMENTS: usize = 256;
  pub const MAX_PIXEL_SIZE: u32 = 64;

  /// `editor/composition.ron`: the level's elements, bottom to top.
  #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
  #[serde(deny_unknown_fields)]
  pub struct Composition {
      /// Bottom to top; at most `MAX_ELEMENTS` (256).
      pub elements: Vec<Element>,
  }

  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  #[serde(deny_unknown_fields)]
  pub struct Element {
      /// A slug unique in the level (`[a-z0-9_.-]`); names its files.
      pub id: String,
      /// Shown in the editor's element list.
      #[serde(default, skip_serializing_if = "String::is_empty")]
      pub name: String,
      /// Hidden elements are left out of the bake. Default true.
      #[serde(default = "yes", skip_serializing_if = "is_true")]
      pub visible: bool,
      /// The editor does not change a locked element.
      #[serde(default, skip_serializing_if = "d::is_false")]
      pub locked: bool,
      /// What the element is.
      pub kind: ElementKind,
  }

  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  #[serde(deny_unknown_fields)]
  pub enum ElementKind {
      /// An imported picture (`editor/images/<source>`), placed in a box and pixelated.
      Image {
          /// What its covered pixels are.
          material: ElementMaterial,
          /// The original's file name under `editor/images/` (`png`, `jpg`, `jpeg`).
          source: String,
          /// The box's top-left, world pixels; may lie outside the level.
          at: (i32, i32),
          /// The box's width and height, world pixels, each 1 to `MAX_SIDE` (4096).
          size: (u32, u32),
          /// Art pixels per element pixel, 1 to `MAX_PIXEL_SIZE` (64). Default 1.
          #[serde(default = "one", skip_serializing_if = "is_one")]
          pixel_size: u32,
          /// Colour reduction: none, or 2 to 256 colours (median cut).
          #[serde(default, skip_serializing_if = "Option::is_none")]
          colours: Option<u16>,
          /// Dithering when colours are reduced.
          #[serde(default, skip_serializing_if = "d::is_default")]
          dither: Dither,
          /// Alpha at or above which a pixel is covered, 0 to 255. Default 128.
          #[serde(default = "threshold", skip_serializing_if = "is_threshold")]
          alpha_threshold: u8,
      },
      /// A level-sized layer painted with brushes and shapes
      /// (`editor/layers/<id>.png`, art resolution; alpha 0 = not covered).
      Drawn {
          /// What its covered pixels are.
          material: ElementMaterial,
      },
      /// A whole level's own material, front and back
      /// (`editor/layers/<id>.material.png`, `.front.png`, `.back.png`).
      Raster,
  }

  /// An element's material.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum ElementMaterial {
      /// Indestructible.
      Rock,
      /// Destructible.
      Dirt,
      /// Open air; a background element is the backdrop seen where dirt is carved.
      Background,
  }
  impl From<ElementMaterial> for olr_data::level::Material {
      fn from(m: ElementMaterial) -> Self {
          match m {
              ElementMaterial::Rock => Self::Rock,
              ElementMaterial::Dirt => Self::Dirt,
              ElementMaterial::Background => Self::Background,
          }
      }
  }

  /// Dithering of an image element's colour reduction.
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
  pub enum Dither {
      /// Nearest colour.
      #[default]
      None,
      /// Bayer 8 × 8.
      Ordered,
      /// Floyd-Steinberg error diffusion.
      Diffusion,
  }

  /// A composition with its files' bytes, by pack path (`editor/images/…`, `editor/layers/…`).
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct CompositionSource {
      pub composition: Composition,
      pub files: std::collections::BTreeMap<String, Vec<u8>>,
  }

  pub fn image_file(source: &str) -> String;               // "editor/images/{source}"
  pub fn layer_file(id: &str) -> String;                   // "editor/layers/{id}.png"
  pub fn raster_files(id: &str) -> [String; 3];            // material, front, back
  /// The files `element` uses, by pack path.
  pub fn element_files(element: &Element) -> Vec<String>;
  /// `None` when the pack has no `editor/composition.ron`. Reads every
  /// referenced file (at most `MAX_FILE_BYTES` each); refuses a missing one.
  pub fn read_composition_files(src: &dyn ContentSource) -> Result<Option<CompositionSource>, FormatError>;
  /// `composition.ron` in canonical form, then every file as it is.
  pub fn write_composition_files(sink: &mut dyn ContentSink, src: &CompositionSource) -> Result<(), FormatError>;
  /// `COMPOSITION_FILE` and every element's files.
  pub fn composition_paths(src: &CompositionSource) -> Vec<String>;
  /// Everything checkable without decoding pixels.
  pub fn check_composition(src: &CompositionSource, size: (u32, u32), art_scale: u32) -> Result<(), FormatError>;
  ```

  `check_composition` reports, as `FormatError::invalid(file, path, message)` with `path` such as `elements[3].size`:
  - more than `MAX_ELEMENTS`;
  - an id that is not a valid slug, or a duplicate;
  - a `source` that is not one portable component with a `png`/`jpg`/`jpeg` extension;
  - an element file missing from `files`;
  - an image `size` side outside 1..=`MAX_SIDE`;
  - `pixel_size` outside 1..=`MAX_PIXEL_SIZE`;
  - `colours` outside 2..=256;
  - an original whose header size (`image_size`) is missing or outside `IMAGES`;
  - a drawn layer or a raster front/back whose PNG header size is not `size × art_scale`;
  - a raster material whose header size is not `size`.

- [ ] **Step 1: Write the failing tests** in `composition.rs`:

  ```rust
  fn sample() -> CompositionSource {
      let art = image_io::encode_rgba(&RgbaImage::new(8, 6)); // a 4 × 3 level at art scale 2
      let material = image_io::encode_indexed(4, 3, &[2; 12], &MATERIAL_PALETTE);
      let mut files = BTreeMap::new();
      files.insert(image_file("sky.png"), image_io::encode_rgba(&RgbaImage::new(10, 10)));
      files.insert(layer_file("ground"), art.clone());
      for (f, b) in raster_files("base").into_iter().zip([material, art.clone(), art]) {
          files.insert(f, b);
      }
      let element = |id: &str, kind| Element { id: id.into(), name: String::new(), visible: true, locked: false, kind };
      CompositionSource {
          composition: Composition {
              elements: vec![
                  element("base", ElementKind::Raster),
                  element("sky", ElementKind::Image {
                      material: ElementMaterial::Background, source: "sky.png".into(), at: (-1, 0), size: (4, 3),
                      pixel_size: 1, colours: Some(16), dither: Dither::Ordered, alpha_threshold: 128,
                  }),
                  element("ground", ElementKind::Drawn { material: ElementMaterial::Dirt }),
              ],
          },
          files,
      }
  }

  #[test]
  fn composition_round_trips_canonically() {
      let src = sample();
      let mut fs = MemFs::new();
      write_composition_files(&mut fs, &src).unwrap();
      let back = read_composition_files(&fs).unwrap().unwrap();
      assert_eq!(back, src);
      let text = String::from_utf8(fs.read(&VPath::new(COMPOSITION_FILE).unwrap()).unwrap()).unwrap();
      assert!(text.contains("colours: Some(16)") || text.contains("colours: 16"), "{text}");
      assert!(!text.contains("visible"), "defaults are left out: {text}");
      assert_eq!(crate::canonical_text(COMPOSITION_FILE, &text).unwrap(), text);
      let mut paths = composition_paths(&src);
      paths.sort();
      assert_eq!(paths, fs.files().map(|(p, _)| p.as_str().to_string()).collect::<Vec<_>>());
      check_composition(&src, (4, 3), 2).unwrap();
  }

  #[test]
  fn no_composition_reads_as_none() {
      assert_eq!(read_composition_files(&MemFs::new()).unwrap(), None);
  }

  #[test]
  fn check_composition_reports_each_problem() {
      let cases: Vec<(&str, Box<dyn Fn(&mut CompositionSource)>)> = vec![
          ("duplicate id", Box::new(|s| s.composition.elements[2].id = "sky".into())),
          ("not a valid id", Box::new(|s| s.composition.elements[2].id = "Bad Id".into())),
          ("missing", Box::new(|s| { s.files.remove(&layer_file("ground")); })),
          ("art_scale", Box::new(|s| { s.files.insert(layer_file("ground"), image_io::encode_rgba(&RgbaImage::new(4, 3))); })),
          ("pixel_size", Box::new(|s| if let ElementKind::Image { pixel_size, .. } = &mut s.composition.elements[1].kind { *pixel_size = 65 })),
          ("colours", Box::new(|s| if let ElementKind::Image { colours, .. } = &mut s.composition.elements[1].kind { *colours = Some(1) })),
          ("size", Box::new(|s| if let ElementKind::Image { size, .. } = &mut s.composition.elements[1].kind { *size = (0, 3) })),
          ("source", Box::new(|s| if let ElementKind::Image { source, .. } = &mut s.composition.elements[1].kind { *source = "../x.png".into() })),
          ("more than", Box::new(|s| { let e = s.composition.elements[0].clone(); s.composition.elements = vec![e; MAX_ELEMENTS + 1]; })),
      ];
      for (want, edit) in cases {
          let mut s = sample();
          edit(&mut s);
          let e = check_composition(&s, (4, 3), 2).unwrap_err().to_string();
          assert!(e.contains(want), "{want}: {e}");
      }
  }

  #[test]
  fn a_hostile_composition_is_an_error_never_a_panic() {
      for text in ["", "(", "(elements: [(id: 3)])", "(elements: [(id: \"a\", kind: Image())])", "\u{0}\u{ff}"] {
          let fs = MemFs::new().with(COMPOSITION_FILE, text);
          assert!(read_composition_files(&fs).is_err(), "{text:?}");
      }
  }

  #[test]
  fn a_missing_file_is_a_read_error() {
      let mut fs = MemFs::new();
      let mut src = sample();
      src.files.remove(&image_file("sky.png"));
      write_composition_files(&mut fs, &src).unwrap();
      let e = read_composition_files(&fs).unwrap_err().to_string();
      assert!(e.contains("editor/images/sky.png"), "{e}");
  }
  ```

  In `text.rs`'s tests, add `canonical_text("editor/composition.ron", "(elements:[])")` ⇒ `Ok("(\n    elements: [],\n)\n")`. Compare with the real output once and fix the expected string to it; it must be canonical.
- [ ] **Step 2:** Run `cargo nextest run -p olr-format composition text::`. Expected: FAIL (module missing).
- [ ] **Step 3: Implement.**
  - The local default helpers `fn yes() -> bool`, `fn is_true(v: &bool) -> bool`, `fn one() -> u32`, `fn is_one(v: &u32) -> bool`, `fn threshold() -> u8 { 128 }` and `fn is_threshold(v: &u8) -> bool` are literal-bodied, so `format-docs` reads their values.
  - `read_composition_files` reads `COMPOSITION_FILE` with `ron_io::read_text`, then `from_canonical`, then every `element_files` path with `read_max(MAX_FILE_BYTES)`. A missing file is a `FormatError::Io` naming it.
  - `check_composition`'s messages contain the words the test looks for:
    - `"more than 256 elements"`;
    - `"duplicate id \"sky\""`;
    - `"\"Bad Id\" is not a valid id"`;
    - `"editor/layers/ground.png: missing"`;
    - `"<file> is WxH, the level's size × art_scale is WxH"`, the word `art_scale` included;
    - for the image fields, the field path (`elements[1].pixel_size`, `.colours`, `.size`, `.source`) followed by the bound.

    `FormatError`'s display carries the file and the path.
  - `write_composition_files` validates nothing beyond what it needs to write: ids are paths, so a non-slug id is an error before any write.
  - Add `"editor/composition.ron" => again::<Composition>(path, text)` to `canonical_text`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-format`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-format: the level composition and its files`.

### Task 3: `editor/` in packs: the hash, archives and validation (`olr-pack`)

**Files:**
- Modify:
  - `crates/olr-pack/src/lib.rs` (`pub fn is_authoring`);
  - `crates/olr-pack/src/hash.rs`;
  - `crates/olr-pack/src/archive.rs`;
  - `crates/olr-pack/src/validate.rs`.

**Interfaces:**
- Produces:

  ```rust
  /// Whether `path` lies under a pack's authoring folder (`editor/`): the
  /// editors' sources, never content (not hashed, never in an archive).
  pub fn is_authoring(path: &str) -> bool; // path.split('/').next() == Some(olr_format::levels::EDITOR_DIR)
  ```

  - `content_hash` skips authoring paths.
  - `archive::encode` skips them.
  - `archive::decode` refuses one as `ArchiveError::BadPath("<path>: authoring files are never in an archive")`.
  - `validate` on a level reads the composition (`read_composition_files`). A read error, or a `check_composition(src, size, art_scale)` error, is an error diagnostic.
  - The composition's paths count as read: they are not "not read by the format". An `editor/` file the composition does not name is "not read by the format".

- [ ] **Step 1: Write the failing tests.** In `hash.rs`:

  ```rust
  #[test]
  fn editor_files_are_not_content() {
      let a = MemFs::new().with("pack.ron", "x").with("level.ron", "1");
      let b = a.clone().with("editor/composition.ron", "(elements: [])").with("editor/images/a.png", "x");
      assert_eq!(content_hash(&a).unwrap(), content_hash(&b).unwrap());
      let c = a.clone().with("editors/x", "content");
      assert_ne!(content_hash(&a).unwrap(), content_hash(&c).unwrap());
  }
  ```

  In `archive.rs`'s tests (the module already has helpers that hand-build archives for its refusal tests; reuse the one that writes a file entry, here called `entry`):

  ```rust
  #[test]
  fn editor_files_never_enter_an_archive() {
      let src = MemFs::new().with("pack.ron", "x").with("editor/x.png", "y").with("a.txt", "z");
      let bytes = encode(&src).unwrap();
      let mut out = MemFs::new();
      decode(&mut &bytes[..], &mut out, &Limits::PACK).unwrap();
      let paths: Vec<&str> = out.files().map(|(p, _)| p.as_str()).collect();
      assert_eq!(paths, ["a.txt", "pack.ron"]);

      // A hand-made archive that holds one anyway is refused.
      let mut forged = MAGIC.to_vec();
      forged.extend(1u32.to_le_bytes());
      entry(&mut forged, "editor/x", b"y");
      let e = decode(&mut &forged[..], &mut MemFs::new(), &Limits::PACK).unwrap_err();
      assert!(matches!(&e, ArchiveError::BadPath(p) if p.contains("editor/x")), "{e:?}");
  }
  ```

  If the module has no such helper, write `entry(out, path, data)`: a `u16` little-endian path length, the path, a `u64` little-endian data length, then the data (the format in `olr-pack/CLAUDE.md`).

  In `validate.rs`'s tests:

  ```rust
  /// `level_with_spawn((1, 1))` (8 × 8, art scale 1) composed of its own raster base.
  fn composed_level() -> MemFs {
      let fs = level_with_spawn((1, 1));
      let level = olr_format::levels::read_level_files(&fs).unwrap();
      let mut base = BTreeMap::new();
      let indices: Vec<u8> = level.material.iter().map(|m| *m as u8).collect();
      let [m, f, b] = olr_format::levels::raster_files("base");
      base.insert(m, crate::image_io::encode_indexed(8, 8, &indices, &olr_format::levels::MATERIAL_PALETTE));
      base.insert(f, crate::image_io::encode_rgba(&level.front));
      base.insert(b, crate::image_io::encode_rgba(&level.back));
      let element = olr_format::levels::Element {
          id: "base".into(), name: String::new(), visible: true, locked: false,
          kind: olr_format::levels::ElementKind::Raster,
      };
      let src = olr_format::levels::CompositionSource {
          composition: olr_format::levels::Composition { elements: vec![element] },
          files: base,
      };
      let mut fs = fs;
      olr_format::levels::write_composition_files(&mut fs, &src).unwrap();
      fs
  }

  #[test]
  fn a_composition_is_validated() {
      assert_eq!(validate(&composed_level()), vec![]);
      let small = crate::image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4));
      let d = validate(&composed_level().with("editor/layers/base.front.png", small));
      assert!(
          d.iter().any(|d| d.severity == Severity::Error
              && d.file.as_deref() == Some("editor/layers/base.front.png")),
          "{d:?}"
      );
      let d: Vec<String> =
          validate(&composed_level().with("editor/notes.txt", "x")).iter().map(|d| d.to_string()).collect();
      assert_eq!(d, ["warning: editor/notes.txt: not read by the format"]);
  }
  ```
- [ ] **Step 2:** Run `cargo nextest run -p olr-pack hash:: archive:: validate::`. Expected: FAIL.
- [ ] **Step 3:** Implement as specified. In `validate`'s `PackKind::Level` arm, after `read_level` succeeds, read the composition. Then:
  - errors go through `Diagnostic::from_error(&PackError::Format(e))`;
  - `unread`'s owned paths gain `composition_paths(&src)`.

  `unread` reads the composition again; that is cheap, since no pixels are decoded.
- [ ] **Step 4:** Run `cargo nextest run -p olr-pack -p olr-online -p olr-content` (downloads and the cache use the archive). Expected: PASS.
- [ ] **Step 5:** Commit: `olr-pack: editor/ is authoring: never hashed, never archived; validate the composition`.
- [ ] **Step 6 (batch A):** Run the online `cargo xtask check`. Expected: every step `ok`; `content/packs` unchanged.

### Task 4: The image pipeline (`olr-edit`)

**Files:**
- Create: `crates/olr-edit/src/pixelate.rs`; `crates/olr-edit/tests/pixelate.rs` (a `mod pixelate;` of `tests/integration.rs`).
- Modify: `crates/olr-edit/src/lib.rs`.

**Interfaces:**
- Produces, in `olr_edit::pixelate`:

  ```rust
  /// An image element's pixelation settings.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct Pixelation {
      pub pixel_size: u32,       // ≥ 1 (art pixels per element pixel)
      pub colours: Option<u16>,  // 2..=256
      pub dither: olr_format::levels::Dither,
      pub alpha_threshold: u8,
  }
  /// `original` processed into a box of `art_size` art pixels: resampled to
  /// the element grid (`ceil(art_size / pixel_size)`) by area averaging,
  /// thresholded, colour-reduced and dithered, then scaled up by
  /// `pixel_size` (nearest) and cropped to `art_size`. Alpha is 0 or 255.
  /// Integer arithmetic only.
  pub fn process(original: &RgbaImage, art_size: (u32, u32), p: &Pixelation) -> RgbaImage;
  /// The distinct opaque colours of `img` (alpha 255), sorted: for tests.
  pub fn palette_of(img: &RgbaImage) -> Vec<Rgba>;
  ```

  Rules:
  - **Resample.** Grid pixel (gx, gy) of a `gw × gh` grid averages the source rectangle `[gx·ow/gw, max((gx+1)·ow/gw, gx·ow/gw + 1))` × likewise (clamped to the image). Colour is weighted by alpha (`Σ c·a / Σ a`, 0 when `Σ a = 0`), and alpha is the mean. All `u64`.
  - **Threshold.** Covered iff `alpha ≥ alpha_threshold`. Covered pixels become alpha 255; others become `(0, 0, 0, 0)`.
  - **Median cut** over covered pixels.
    - Start with one box of all their colours (with repeats).
    - Repeatedly split the box whose widest channel range is largest (ties: the earlier box) at the median of that channel. Sort by (channel, r, g, b) for a stable order.
    - Stop when there are `colours` boxes or no box has a range.
    - Each box's colour is its integer mean.
  - **Nearest.** The smallest squared RGB distance; ties go to the lowest index.
  - **Ordered dither.** `bias = (BAYER8[y%8][x%8] as i32 * 2 - 63) * spread / 128` with `spread = 255 / cbrt_ceil(colours)` (the smallest `k` with `k³ ≥ colours`). Add it to each channel (clamped to 0..=255) before `nearest`.
  - **Diffusion.** Floyd-Steinberg in raster order, the error per channel `i32`, spread 7/16, 3/16, 5/16 and 1/16 to covered neighbours only.
  - **Dither** applies only when `colours` is `Some`.

- [ ] **Step 1: Write the failing tests** in `tests/pixelate.rs`:

  ```rust
  use olr_data::image::{Rgba, RgbaImage};
  use olr_edit::pixelate::{Pixelation, palette_of, process};
  use olr_format::levels::Dither;
  use proptest::prelude::*;

  fn p(pixel_size: u32, colours: Option<u16>, dither: Dither, alpha_threshold: u8) -> Pixelation {
      Pixelation { pixel_size, colours, dither, alpha_threshold }
  }

  fn gradient(w: u32, h: u32) -> RgbaImage {
      let mut img = RgbaImage::new(w, h);
      for y in 0..h {
          for x in 0..w {
              img.set(x, y, Rgba { r: (x * 255 / w.max(1)) as u8, g: (y * 255 / h.max(1)) as u8, b: 99, a: 255 });
          }
      }
      img
  }

  #[test]
  fn pixel_size_makes_square_blocks() {
      let out = process(&gradient(64, 64), (12, 12), &p(4, None, Dither::None, 128));
      assert_eq!((out.width(), out.height()), (12, 12));
      for y in 0..12 {
          for x in 0..12 {
              assert_eq!(out.get(x, y), out.get(x / 4 * 4, y / 4 * 4), "({x}, {y})");
          }
      }
  }

  #[test]
  fn the_threshold_makes_crisp_edges() {
      let mut img = RgbaImage::new(2, 1);
      img.set(0, 0, Rgba { r: 9, g: 9, b: 9, a: 127 });
      img.set(1, 0, Rgba { r: 9, g: 9, b: 9, a: 128 });
      let out = process(&img, (2, 1), &p(1, None, Dither::None, 128));
      assert_eq!(out.get(0, 0), Rgba { r: 0, g: 0, b: 0, a: 0 });
      assert_eq!(out.get(1, 0), Rgba { r: 9, g: 9, b: 9, a: 255 });
  }

  #[test]
  fn reduction_never_exceeds_n_and_tolerates_empty_input() {
      for dither in [Dither::None, Dither::Ordered, Dither::Diffusion] {
          for n in [2u16, 3, 16, 256] {
              let out = process(&gradient(40, 30), (40, 30), &p(1, Some(n), dither, 1));
              assert!(palette_of(&out).len() <= n as usize, "{dither:?} {n}");
          }
          // Fewer distinct colours than asked: kept as they are.
          let mut two = RgbaImage::new(4, 1);
          for x in 0..4 {
              two.set(x, 0, if x < 2 { Rgba::rgb(255, 0, 0) } else { Rgba::rgb(0, 0, 255) });
          }
          assert_eq!(palette_of(&process(&two, (4, 1), &p(1, Some(16), dither, 1))).len(), 2);
          // Nothing covered, or nothing at all.
          let clear = RgbaImage::new(5, 5);
          assert!(palette_of(&process(&clear, (5, 5), &p(1, Some(4), dither, 128))).is_empty());
          let _ = process(&RgbaImage::new(0, 0), (3, 3), &p(1, Some(4), dither, 128));
      }
  }

  /// Mid grey reduced to a black-and-white palette: without dither it all
  /// rounds to one colour; ordered and diffusion dither mix both.
  #[test]
  fn dither_mixes_the_two_nearest_colours() {
      let mut img = RgbaImage::new(16, 16);
      for y in 0..16 { for x in 0..16 { img.set(x, y, Rgba::rgb(120, 120, 120)); } }
      // One black and one white pixel make median cut's two colours black and white.
      img.set(0, 0, Rgba::rgb(0, 0, 0));
      img.set(15, 15, Rgba::rgb(255, 255, 255));
      let interior = |out: &RgbaImage| {
          let mut seen = std::collections::BTreeSet::new();
          for y in 4..12 { for x in 4..12 { seen.insert(out.get(x, y).r); } }
          seen
      };
      let flat = process(&img, (16, 16), &p(1, Some(2), Dither::None, 1));
      assert_eq!(interior(&flat).len(), 1, "no dither: one colour");
      for dither in [Dither::Ordered, Dither::Diffusion] {
          let out = process(&img, (16, 16), &p(1, Some(2), dither, 1));
          assert_eq!(palette_of(&out).len(), 2);
          assert_eq!(interior(&out).len(), 2, "{dither:?}: both colours in the grey area");
      }
  }

  proptest! {
      #![proptest_config(ProptestConfig::with_cases(48))]
      #[test]
      fn process_is_total_and_sized(ow in 0u32..20, oh in 0u32..20, aw in 1u32..30, ah in 1u32..30,
                                    ps in 1u32..6, n in proptest::option::of(2u16..40), d in 0u8..3, t in 0u8..=255) {
          let dither = [Dither::None, Dither::Ordered, Dither::Diffusion][d as usize];
          let out = process(&gradient(ow, oh), (aw, ah), &p(ps, n, dither, t));
          prop_assert_eq!((out.width(), out.height()), (aw, ah));
          prop_assert!(out.as_raw().chunks(4).all(|px| px[3] == 0 || px[3] == 255));
          if let Some(n) = n { prop_assert!(palette_of(&out).len() <= n as usize); }
      }
  }
  ```

- [ ] **Step 2:** Run `cargo nextest run -p olr-edit pixelate`. Expected: FAIL.
- [ ] **Step 3:** Implement `pixelate.rs` per the rules above, with private helpers `resample`, `threshold`, `median_cut`, `nearest`, `dither_ordered`, `dither_diffusion` and `BAYER8: [[u8; 8]; 8]` (the standard 0..63 matrix). An empty original or empty grid gives a fully transparent box.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit pixelate`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: the image pipeline: resample, threshold, median cut, dither`.

### Task 5: The bake (`olr-edit`)

**Files:**
- Create: `crates/olr-edit/src/compose.rs`; `crates/olr-edit/tests/compose.rs` (`mod compose;`).
- Modify: `crates/olr-edit/src/lib.rs`; `crates/olr-edit/src/level.rs` (make `covered` and `art_block` `pub(crate)`); `crates/olr-edit/Cargo.toml` (none expected).

**Interfaces:**
- Consumes: Task 2's `CompositionSource`, `Element`, `ElementKind`, `ElementMaterial`, `image_file`, `layer_file`, `raster_files`; Task 1's `image_io::decode_image_within`; Task 4's `pixelate::{Pixelation, process}`; `level::refresh_thumb`.
- Produces, in `olr_edit::compose`:

  ```rust
  /// A rectangle of world pixels.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct Rect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }

  #[derive(Debug, thiserror::Error)]
  pub enum ComposeError {
      /// An original, a layer or a raster file that does not decode, or of the wrong size.
      #[error("{file}: {message}")]
      File { file: String, message: String },
      #[error(transparent)]
      Format(#[from] olr_format::FormatError),
  }

  /// Decoded layers and processed images of one composition, ready to bake
  /// any region. `bake_region` never fails: everything was checked here.
  pub struct Baker { /* private: size, art scale, back tile, and per element its decoded layers or processed box */ }
  impl Baker {
      pub fn new(src: &CompositionSource, size: (u32, u32), art_scale: u32, back_tile: &RgbaImage)
          -> Result<Baker, ComposeError>;
      /// Rebakes `rect` (clipped to the level) of `level`'s material, front and back.
      pub fn bake_region(&self, level: &mut LevelSource, rect: Rect);
  }
  /// The whole bake: material, front, back and the thumbnail (`refresh_thumb`).
  pub fn bake(level: &mut LevelSource, src: &CompositionSource, back_tile: &RgbaImage) -> Result<(), ComposeError>;
  /// `level` as a composition of one raster element `base` (ruling R6).
  pub fn compose_from(level: &LevelSource) -> CompositionSource;
  ```

  The bake follows rulings R3-R5:
  - for each world pixel of the rect, the material comes from the topmost visible element covering its block's centre art pixel (background when none);
  - for each art pixel, the back comes from R4 and the front from the topmost visible element covering that art pixel (its RGB, alpha 255), else the back;
  - then `front := back` over every background block.

  Each element's per-art-pixel queries:
  - **Image:** its processed box (`process(original, size × art_scale, settings)`), placed at `at × art_scale`; covers where the box's alpha is 255.
  - **Drawn:** the layer; covers where alpha ≠ 0.
  - **Raster:** covers every pixel; material from its material layer (palette index to `Material`), front and back from its files.

- [ ] **Step 1: Write the failing tests** in `tests/compose.rs`. Helpers first, in full:

  ```rust
  use olr_data::image::{Rgba, RgbaImage};
  use olr_data::level::Material;
  use olr_edit::compose::{Baker, Rect, bake, compose_from};
  use olr_format::image_io;
  use olr_format::levels::{
      Composition, CompositionSource, Dither, Element, ElementKind, ElementMaterial, LevelSource, image_file, layer_file,
  };
  use olr_vfs::MemFs;
  use proptest::prelude::*;

  use crate::common::content;

  const BLUE: Rgba = Rgba::rgb(20, 40, 200);
  const BROWN: Rgba = Rgba::rgb(140, 90, 40);

  /// A `w` × `h` level at art scale `s`: all dirt, zero art.
  fn level(w: u32, h: u32, s: u32) -> LevelSource {
      LevelSource {
          name: "T".into(), notes: String::new(), size: (w, h), art_scale: s, theme: "olx-theme-dirt".into(),
          objects: Vec::new(), material: vec![Material::Dirt; (w * h) as usize],
          front: RgbaImage::new(w * s, h * s), back: RgbaImage::new(w * s, h * s), thumb: None,
      }
  }

  fn back_tile() -> RgbaImage {
      let mut t = RgbaImage::new(2, 2);
      for (i, c) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
          t.set(c.0, c.1, Rgba::rgb(10 * i as u8, 7, 7));
      }
      t
  }

  fn solid(w: u32, h: u32, c: Rgba) -> RgbaImage {
      let mut img = RgbaImage::new(w, h);
      for y in 0..h { for x in 0..w { img.set(x, y, c); } }
      img
  }

  fn element(id: &str, kind: ElementKind) -> Element {
      Element { id: id.into(), name: String::new(), visible: true, locked: false, kind }
  }

  /// Adds an image element `id` (source `<id>.png`) on top.
  fn with_image(src: &mut CompositionSource, id: &str, img: RgbaImage, at: (i32, i32), size: (u32, u32), m: ElementMaterial) {
      let source = format!("{id}.png");
      src.files.insert(image_file(&source), image_io::encode_rgba(&img));
      src.composition.elements.push(element(id, ElementKind::Image {
          material: m, source, at, size, pixel_size: 1, colours: None, dither: Dither::None, alpha_threshold: 128,
      }));
  }

  /// Adds a drawn layer `id` holding `art` (art resolution) on top.
  fn with_drawn(src: &mut CompositionSource, id: &str, art: &RgbaImage, m: ElementMaterial) {
      src.files.insert(layer_file(id), image_io::encode_rgba(art));
      src.composition.elements.push(element(id, ElementKind::Drawn { material: m }));
  }

  /// `front == back` on every art pixel of a background block.
  fn background_invariant(l: &LevelSource) -> bool {
      let s = l.art_scale;
      (0..l.size.1 * s).all(|ay| (0..l.size.0 * s).all(|ax| {
          l.material[((ay / s) * l.size.0 + ax / s) as usize] != Material::Background
              || l.front.get(ax, ay) == l.back.get(ax, ay)
      }))
  }

  /// Overwrites `r` (clipped) of `l` with junk.
  fn scramble(l: &mut LevelSource, r: Rect) {
      let s = l.art_scale;
      for y in r.y..(r.y + r.h).min(l.size.1) {
          for x in r.x..(r.x + r.w).min(l.size.0) {
              l.material[(y * l.size.0 + x) as usize] = Material::Rock;
              for ay in y * s..(y + 1) * s { for ax in x * s..(x + 1) * s {
                  l.front.set(ax, ay, Rgba::rgb(1, 2, 3));
                  l.back.set(ax, ay, Rgba::rgb(3, 2, 1));
              } }
          }
      }
  }

  /// A stack from `spec`: (kind, material, x, y, w, h, visible); kind 0 an
  /// image of a 3 × 3 gradient with a transparent corner, 1 a drawn layer
  /// of a checkerboard, 2 the raster base of a random-ish level.
  fn random_stack(spec: &[(u8, u8, i32, i32, u32, u32, bool)], size: (u32, u32), s: u32) -> CompositionSource {
      let mut src = CompositionSource::default();
      for (i, &(kind, m, x, y, w, h, visible)) in spec.iter().enumerate() {
          let m = [ElementMaterial::Rock, ElementMaterial::Dirt, ElementMaterial::Background][m as usize];
          let id = format!("e{i}");
          match kind {
              0 => {
                  let mut img = solid(3, 3, Rgba::rgb(x as u8, y as u8, 9));
                  img.set(0, 0, Rgba { r: 0, g: 0, b: 0, a: 0 });
                  with_image(&mut src, &id, img, (x, y), (w, h), m);
              }
              1 => {
                  let mut art = RgbaImage::new(size.0 * s, size.1 * s);
                  for ay in 0..size.1 * s { for ax in 0..size.0 * s {
                      if (ax + ay + i as u32) % 3 == 0 { art.set(ax, ay, Rgba::rgb(9, i as u8, 99)); }
                  } }
                  with_drawn(&mut src, &id, &art, m);
              }
              _ => {
                  let mut l = level(size.0, size.1, s);
                  for (k, cell) in l.material.iter_mut().enumerate() {
                      *cell = [Material::Rock, Material::Background, Material::Dirt][(k + i) % 3];
                  }
                  l.front = solid(size.0 * s, size.1 * s, BROWN);
                  l.back = solid(size.0 * s, size.1 * s, BLUE);
                  // Keep the raster's own invariant: front = back on background.
                  for (k, cell) in l.material.clone().iter().enumerate() {
                      if *cell == Material::Background {
                          let (x, y) = (k as u32 % size.0, k as u32 / size.0);
                          for ay in y * s..(y + 1) * s { for ax in x * s..(x + 1) * s { l.front.set(ax, ay, BLUE); } }
                      }
                  }
                  let raster = compose_from(&l);
                  let mut e = raster.composition.elements[0].clone();
                  e.id = id.clone();
                  for (path, bytes) in raster.files {
                      src.files.insert(path.replace("/base.", &format!("/{id}.")), bytes);
                  }
                  src.composition.elements.push(e);
              }
          }
          src.composition.elements.last_mut().unwrap().visible = visible;
      }
      src
  }
  ```

  The tests:

  ```rust
  #[test]
  fn an_empty_composition_is_open_air_over_the_theme_tile() {
      let mut l = level(4, 3, 2);
      bake(&mut l, &CompositionSource::default(), &back_tile()).unwrap();
      assert!(l.material.iter().all(|m| *m == Material::Background));
      for y in 0..6 { for x in 0..8 {
          assert_eq!(l.back.get(x, y), back_tile().get(x % 2, y % 2));
          assert_eq!(l.front.get(x, y), l.back.get(x, y));
      } }
      assert!(l.thumb.is_some());
  }

  #[test]
  fn topmost_wins_and_background_elements_are_the_backdrop() {
      let mut src = CompositionSource::default();
      with_image(&mut src, "sky", solid(4, 3, BLUE), (0, 0), (4, 3), ElementMaterial::Background);
      with_image(&mut src, "dirt", solid(2, 3, BROWN), (0, 0), (2, 3), ElementMaterial::Dirt);
      let mut l = level(4, 3, 1);
      bake(&mut l, &src, &back_tile()).unwrap();
      for y in 0..3 { for x in 0..4 {
          let dirt = x < 2;
          assert_eq!(l.material[(y * 4 + x) as usize], if dirt { Material::Dirt } else { Material::Background });
          assert_eq!(l.front.get(x, y), if dirt { BROWN } else { BLUE });
          assert_eq!(l.back.get(x, y), BLUE, "carving the dirt reveals the sky");
      } }
  }

  #[test]
  fn hidden_elements_are_left_out() {
      let mut src = CompositionSource::default();
      with_image(&mut src, "sky", solid(4, 3, BLUE), (0, 0), (4, 3), ElementMaterial::Background);
      with_image(&mut src, "dirt", solid(2, 3, BROWN), (0, 0), (2, 3), ElementMaterial::Dirt);
      src.composition.elements[1].visible = false;
      let mut l = level(4, 3, 1);
      bake(&mut l, &src, &back_tile()).unwrap();
      assert!(l.material.iter().all(|m| *m == Material::Background));
      assert!((0..3).all(|y| (0..4).all(|x| l.front.get(x, y) == BLUE)));
  }

  #[test]
  fn every_bundled_level_round_trips_through_a_raster_base() {
      let Some(root) = content() else { return };
      for id in ["olx-level-castlestrike", "olx-level-dirt-level", "olx-level-fossilfacility", "olx-level-liero-factory"] {
          let original = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(root.join(id))).unwrap();
          let src = compose_from(&original);
          let mut baked = original.clone();
          baked.material.fill(Material::Rock);
          baked.front = RgbaImage::new(baked.front.width(), baked.front.height());
          baked.back = baked.front.clone();
          bake(&mut baked, &src, &back_tile()).unwrap();
          let files = |l: &LevelSource| { let mut fs = MemFs::new(); olr_format::levels::write_level_files(&mut fs, l).unwrap(); fs };
          assert!(files(&baked) == files(&original), "{id} changed");
      }
  }

  #[test]
  fn elements_outside_the_level_are_clipped() {
      let mut src = CompositionSource::default();
      with_image(&mut src, "far", solid(3, 3, BLUE), (-100, -100), (3, 3), ElementMaterial::Rock);
      with_image(&mut src, "big", solid(9, 9, BROWN), (-2, -2), (4096, 4096), ElementMaterial::Dirt);
      with_image(&mut src, "edge", solid(3, 3, BLUE), (3, 2), (3, 3), ElementMaterial::Rock);
      let mut l = level(4, 3, 2);
      bake(&mut l, &src, &back_tile()).unwrap();
      assert_eq!(l.material[(2 * 4 + 3) as usize], Material::Rock, "the edge's one inside pixel");
  }

  #[test]
  fn a_bad_original_is_an_error_naming_it() {
      let mut src = CompositionSource::default();
      with_image(&mut src, "x", solid(1, 1, BLUE), (0, 0), (1, 1), ElementMaterial::Dirt);
      src.files.insert(olr_format::levels::image_file("x.png"), b"not an image".to_vec());
      let e = bake(&mut level(4, 3, 1), &src, &back_tile()).unwrap_err().to_string();
      assert!(e.contains("editor/images/x.png"), "{e}");
  }

  proptest! {
      #![proptest_config(ProptestConfig::with_cases(32))]
      #[test]
      fn region_bakes_equal_the_full_bake_and_keep_the_invariant(
          s in 1u32..=3, w in 1u32..=12, h in 1u32..=12,
          stack in prop::collection::vec((0u8..3, 0u8..3, -4i32..14, -4i32..14, 1u32..16, 1u32..16, any::<bool>()), 0..6),
          rect in (0u32..12, 0u32..12, 1u32..12, 1u32..12),
      ) {
          // Build images (kind 0), drawn layers (kind 1: a checkerboard layer) and a raster (kind 2: compose_from a random level).
          let src = random_stack(&stack, (w, h), s);
          let mut full = level(w, h, s);
          bake(&mut full, &src, &back_tile()).unwrap();
          prop_assert!(background_invariant(&full));
          let baker = Baker::new(&src, (w, h), s, &back_tile()).unwrap();
          let mut part = full.clone();
          let r = Rect { x: rect.0, y: rect.1, w: rect.2, h: rect.3 };
          // Scramble the region, then rebake it: it must come back as the full bake.
          scramble(&mut part, r);
          baker.bake_region(&mut part, r);
          prop_assert!(part.material == full.material && part.front == full.front && part.back == full.back);
      }
  }
  ```

- [ ] **Step 2:** Run `cargo nextest run -p olr-edit compose`. Expected: FAIL.
- [ ] **Step 3: Implement `compose.rs`.**
  - `Baker::new` decodes each element's files with `decode_image_within(LEVEL_ART)` for layers (`decode_indexed_within` for a raster material, mapped through `Material::from_index`; an index > 2 is a `ComposeError::File`), and `IMAGES` for originals.
  - Each decoded size is checked against `size × art_scale` (`size` for a raster material), else a `ComposeError::File`.
  - Images are processed once, in `new`.
  - `bake_region` clips the rect to the level, and iterates world rows, art rows and elements top down. Elements are stored bottom to top; iterate `.iter().rev()` skipping invisible ones.
  - Use `i64` for every position (`at × art_scale` may be negative).
  - `compose_from` encodes the raster files with `image_io::encode_indexed(w, h, &indices, &MATERIAL_PALETTE)` and `image_io::encode_rgba`, exactly as `write_level_files` does (ruling R6).
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit compose`, then the release timing check below. Expected: PASS.

  Add `#[test] #[ignore] fn bake_speed()`, which bakes Liero Factory composed as a raster base plus one full-level image element and prints the duration. Run it once with `cargo nextest run -p olr-edit --release --run-ignored only bake_speed` and note the time in the commit message. The spec's target is well under 1 s; a slower result is a finding to ledger, not a failure.
- [ ] **Step 5:** Commit: `olr-edit: bake a composition into the level, a region at a time`.

### Task 6: The pack carries its composition; theme tiles; stale bakes (`olr-edit`)

**Files:**
- Modify: `crates/olr-edit/src/pack.rs`, `crates/olr-edit/src/compose.rs`, `crates/olr-edit/src/new.rs` (construct `composition: None`), `crates/olr-cli/src/level_cmd.rs` (the `Pack { … }` pattern gains the field).
- Test: `crates/olr-edit/tests/pack.rs`, `crates/olr-edit/tests/compose.rs`.

**Interfaces:**
- Produces:
  - `olr_edit::pack::Pack` gains `pub composition: Option<olr_format::levels::CompositionSource>` (a level's `editor/` folder; always `None` for the other kinds).
    - `load` reads it for levels.
    - `Source::reads` is unchanged, but `load`'s owned set adds `composition_paths`.
    - `files` writes it with `write_composition_files`.
  - In `olr_edit::compose`:

    ```rust
    /// The `back_tile` of the theme pack whose manifest id is `theme` among
    /// `content`'s immediate subdirectories (ruling R9).
    pub fn theme_back_tile(content: &Path, theme: &str) -> Result<RgbaImage, ComposeError>;
    /// The baked files of `pack` (a composed level) that differ from what its
    /// composition bakes to: `material.png`, `front.png`, `back.png`, `thumb.png`.
    pub fn stale(pack: &crate::pack::Pack, back_tile: &RgbaImage) -> Result<Vec<String>, ComposeError>;
    ```

    `theme_back_tile`'s error when no theme matches is `ComposeError::File { file: theme.into(), message: format!("no theme pack with this id in {}", content.display()) }`.

- [ ] **Step 1: Write the failing tests.** In `tests/pack.rs`:

  ```rust
  /// Dirt Level, composed of its raster base and one drawn dirt layer.
  fn composed(dir: &std::path::Path) -> olr_edit::pack::Pack {
      let mut pack = load(dir).unwrap();
      let Source::Level(level) = &pack.source else { panic!("not a level") };
      let mut src = olr_edit::compose::compose_from(level);
      let (w, h) = (level.front.width(), level.front.height());
      src.files.insert(
          olr_format::levels::layer_file("ground"),
          olr_format::image_io::encode_rgba(&olr_data::image::RgbaImage::new(w, h)),
      );
      src.composition.elements.push(olr_format::levels::Element {
          id: "ground".into(), name: "Ground".into(), visible: true, locked: false,
          kind: olr_format::levels::ElementKind::Drawn { material: olr_format::levels::ElementMaterial::Dirt },
      });
      pack.composition = Some(src);
      pack
  }

  #[test]
  fn a_composition_survives_load_and_save() {
      let Some(root) = content() else { return };
      let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
      let pack = composed(&dir);
      let written = files(&pack).unwrap();
      assert!(written.contains_key(&olr_vfs::VPath::new("editor/composition.ron").unwrap()));
      assert!(written.contains_key(&olr_vfs::VPath::new("editor/layers/ground.png").unwrap()));
      save(&dir, &pack).unwrap();
      assert_eq!(load(&dir).unwrap().composition, pack.composition);
      assert_eq!(olr_pack::validate(&olr_vfs::DirFs::new(&dir)), vec![]);
  }

  #[test]
  fn a_pack_without_editor_loads_with_none() {
      let Some(root) = content() else { return };
      assert_eq!(load(&root.join("olx-level-dirt-level")).unwrap().composition, None);
  }

  /// `pack fmt` canonicalises `composition.ron` like any authoring file.
  #[test]
  fn fmt_rewrites_a_hand_edited_composition() {
      let Some(root) = content() else { return };
      let (_tmp, dir) = copy_pack(&root, "olx-level-dirt-level");
      save(&dir, &composed(&dir)).unwrap();
      let canonical = std::fs::read_to_string(dir.join("editor/composition.ron")).unwrap();
      std::fs::write(dir.join("editor/composition.ron"), canonical.replace("    ", "  ")).unwrap();
      let listed: Vec<String> =
          olr_edit::fmt::unformatted(&dir).unwrap().iter().map(|p| p.as_str().to_string()).collect();
      assert_eq!(listed, ["editor/composition.ron"]);
      olr_edit::fmt::format(&dir).unwrap();
      assert_eq!(std::fs::read_to_string(dir.join("editor/composition.ron")).unwrap(), canonical);
  }
  ```

  In `tests/compose.rs`:

  ```rust
  #[test]
  fn stale_lists_the_files_a_hand_edit_changed() {
      let Some(root) = content() else { return };
      let tmp = tempfile::tempdir().unwrap();
      let dir = tmp.path().join("dirt");
      crate::common::copy_dir(&root.join("olx-level-dirt-level"), &dir);
      let tile = olr_edit::compose::theme_back_tile(&root, "olx-theme-dirt").unwrap();
      let mut pack = olr_edit::pack::load(&dir).unwrap();
      let olr_edit::pack::Source::Level(level) = &mut pack.source else { panic!() };
      let src = compose_from(level);
      bake(level, &src, &tile).unwrap();
      pack.composition = Some(src);
      olr_edit::pack::save(&dir, &pack).unwrap();
      let pack = olr_edit::pack::load(&dir).unwrap();
      assert_eq!(olr_edit::compose::stale(&pack, &tile).unwrap(), Vec::<String>::new());
      let front = image_io::decode_rgba(&std::fs::read(dir.join("front.png")).unwrap()).unwrap();
      std::fs::write(dir.join("front.png"), image_io::encode_rgba(&solid(front.width(), front.height(), BLUE))).unwrap();
      let stale = olr_edit::compose::stale(&olr_edit::pack::load(&dir).unwrap(), &tile).unwrap();
      assert!(stale.contains(&"front.png".to_string()), "{stale:?}");
      assert!(!stale.contains(&"material.png".to_string()), "{stale:?}");
  }

  #[test]
  fn theme_back_tile_finds_the_theme_by_id() {
      let Some(root) = content() else { return };
      let tile = olr_edit::compose::theme_back_tile(&root, "olx-theme-dirt").unwrap();
      let theme = olr_format::themes::read_theme_files(&olr_vfs::DirFs::new(root.join("olx-theme-dirt"))).unwrap();
      assert!(tile == theme.back_tile);
      let e = olr_edit::compose::theme_back_tile(&root, "nope").unwrap_err().to_string();
      assert!(e.contains("nope"), "{e}");
  }
  ```

  The Dirt Level round trip through `compose_from` and `bake` leaves the bake equal to its files (Task 5's round-trip test), so the fresh composition's `stale` is empty.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit`. Expected: FAIL.
- [ ] **Step 3:** Implement. `stale` clones the level source, bakes it, writes both the clone and the original with `write_level_files` into `MemFs`es, and compares those four paths.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit -p olr-cli --test integration level:: cli::`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: packs carry their composition; theme tiles; stale bakes`.
- [ ] **Step 6 (batch B):** Run the online `cargo xtask check`. Expected: every step `ok`.

### Task 7: Element operations (`olr-edit`)

**Files:**
- Create: `crates/olr-edit/src/element.rs`; `crates/olr-edit/tests/element.rs` (`mod element;`).
- Modify: `crates/olr-edit/src/lib.rs`.

**Interfaces:**
- Consumes: Tasks 2, 5 and 6; `level::{Shape, Paint}` and `level::{covered, art_block}` (crate-visible since Task 5).
- Produces, in `olr_edit::element` (all operate on a `Pack` that is a level; each leaves the composition valid for `check_composition`):

  ```rust
  #[derive(Debug, thiserror::Error, PartialEq)]
  pub enum ElementError {
      #[error("not a level pack")] NotALevel,
      #[error("no element {0:?}")] NoElement(String),
      #[error("element {0:?} is locked")] Locked(String),
      #[error("element {id:?} is not {want}")] WrongKind { id: String, want: &'static str },
      #[error("a level holds at most {} elements", olr_format::levels::MAX_ELEMENTS)] TooMany,
      #[error("{0}")] BadValue(String),
  }
  /// A settable property of an element.
  #[derive(Clone, Debug, PartialEq)]
  pub enum Property {
      Name(String), Visible(bool), Locked(bool), Material(ElementMaterial),
      At((i32, i32)), Size((u32, u32)), PixelSize(u32), Colours(Option<u16>), Dither(Dither), Threshold(u8),
  }
  /// Gives a level without a composition its raster base (`compose_from`); no-op otherwise.
  pub fn ensure_composed(pack: &mut Pack) -> Result<(), ElementError>;
  /// Adds an image element on top; `file_name` is the original's (ruling R7). Returns its id.
  pub fn add_image(pack: &mut Pack, file_name: &str, bytes: Vec<u8>, at: (i32, i32), size: (u32, u32),
                   material: ElementMaterial, name: &str) -> Result<String, ElementError>;
  /// Adds an empty drawn layer on top. Returns its id.
  pub fn add_layer(pack: &mut Pack, material: ElementMaterial, name: &str) -> Result<String, ElementError>;
  pub fn set(pack: &mut Pack, id: &str, property: Property) -> Result<(), ElementError>;
  pub fn remove(pack: &mut Pack, id: &str) -> Result<(), ElementError>;   // drops its files too
  pub fn raise(pack: &mut Pack, id: &str) -> Result<(), ElementError>;    // one step up; top: no-op
  pub fn lower(pack: &mut Pack, id: &str) -> Result<(), ElementError>;    // one step down; bottom: no-op
  /// Paints `shape` (world pixels, whole art blocks) into drawn layer `id`, opaque.
  pub fn draw(pack: &mut Pack, id: &str, shape: &Shape, paint: &Paint) -> Result<(), ElementError>;
  /// Clears `shape` in drawn layer `id` (alpha 0).
  pub fn erase(pack: &mut Pack, id: &str, shape: &Shape) -> Result<(), ElementError>;
  /// Resizes the composition with the level (as `level::resize`): image boxes
  /// move by `offset`; drawn layers are re-laid (the new area uncovered);
  /// raster layers are re-laid with `material` and `paint` in the new area.
  pub fn resize(pack: &mut Pack, size: (u32, u32), offset: (i32, i32), material: Material, paint: &Paint)
      -> Result<(), ElementError>;
  ```

  Rules:
  - `set`, `draw`, `erase` and `remove` on a locked element fail with `Locked` (only `Property::Locked(false)` passes).
  - `set` validates as `check_composition` does: a bad value is `BadValue` naming the property.
  - `add_image` checks the bytes with `image_io::image_size` and `check_size(.., IMAGES)` before storing anything, and decodes once with `decode_image_within`. A failure is `BadValue("<file name>: <why>")`.
  - New ids follow ruling R8.
  - Layers are stored encoded (`encode_rgba`); `draw` and `erase` decode, paint and re-encode.

  - `add_image` and `add_layer` call `ensure_composed` first.
  - `element::resize` reads the old size from the pack's level source, so callers run it *before* `level::resize` (the CLI does, Task 8).

- [ ] **Step 1: Write the failing tests** in `tests/element.rs`:

  ```rust
  use std::collections::BTreeMap;

  use olr_data::image::{Rgba, RgbaImage};
  use olr_data::level::Material;
  use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
  use olr_edit::element::*;
  use olr_edit::level::{Paint, Shape};
  use olr_edit::pack::{Pack, Source};
  use olr_format::image_io;
  use olr_format::levels::{ElementKind, ElementMaterial, LevelSource, check_composition, layer_file};

  const RED: Rgba = Rgba::rgb(255, 0, 0);

  /// A plain `w` × `h` level pack at art scale `s` (all dirt), in memory.
  fn pack(w: u32, h: u32, s: u32) -> Pack {
      let manifest = PackManifest {
          format_version: PACK_FORMAT_VERSION, id: "t".into(), name: "T".into(), version: "1".into(),
          kind: PackKind::Level, authors: vec![], origin: None, license: None,
      };
      let level = LevelSource {
          name: "T".into(), notes: String::new(), size: (w, h), art_scale: s, theme: "olx-theme-dirt".into(),
          objects: Vec::new(), material: vec![Material::Dirt; (w * h) as usize],
          front: RgbaImage::new(w * s, h * s), back: RgbaImage::new(w * s, h * s), thumb: None,
      };
      Pack { manifest, source: Source::Level(level), composition: None, other: BTreeMap::new() }
  }

  fn ids(p: &Pack) -> Vec<String> {
      p.composition.as_ref().unwrap().composition.elements.iter().map(|e| e.id.clone()).collect()
  }

  fn layer(p: &Pack, id: &str) -> RgbaImage {
      image_io::decode_rgba(&p.composition.as_ref().unwrap().files[&layer_file(id)]).unwrap()
  }

  fn png(w: u32, h: u32) -> Vec<u8> {
      image_io::encode_rgba(&RgbaImage::new(w, h))
  }

  #[test]
  fn a_plain_level_gains_a_raster_base_first() {
      let mut p = pack(8, 6, 1);
      assert!(p.composition.is_none());
      assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground");
      assert_eq!(ids(&p), ["base", "ground"]);
      let src = p.composition.as_ref().unwrap();
      check_composition(src, (8, 6), 1).unwrap();
  }

  #[test]
  fn ids_are_unique_slugs() {
      let mut p = pack(8, 6, 1);
      assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground");
      assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground-2");
      let a = add_image(&mut p, "My Photo.PNG", png(4, 4), (0, 0), (4, 4), ElementMaterial::Rock, "").unwrap();
      let b = add_image(&mut p, "My Photo.PNG", png(4, 4), (0, 0), (4, 4), ElementMaterial::Rock, "").unwrap();
      assert_eq!((a.as_str(), b.as_str()), ("my-photo", "my-photo-2"));
      let sources: Vec<String> = p.composition.as_ref().unwrap().composition.elements.iter()
          .filter_map(|e| match &e.kind { ElementKind::Image { source, .. } => Some(source.clone()), _ => None })
          .collect();
      assert_eq!(sources, ["My Photo.PNG", "My Photo-2.PNG"]);
  }

  #[test]
  fn set_validates_and_respects_locks() {
      let mut p = pack(8, 6, 1);
      let id = add_image(&mut p, "a.png", png(4, 4), (0, 0), (4, 4), ElementMaterial::Dirt, "A").unwrap();
      assert!(matches!(set(&mut p, &id, Property::PixelSize(0)), Err(ElementError::BadValue(m)) if m.contains("pixel_size")));
      assert!(matches!(set(&mut p, &id, Property::Colours(Some(1))), Err(ElementError::BadValue(_))));
      set(&mut p, &id, Property::Locked(true)).unwrap();
      assert_eq!(set(&mut p, &id, Property::Name("x".into())), Err(ElementError::Locked(id.clone())));
      set(&mut p, &id, Property::Locked(false)).unwrap();
      set(&mut p, &id, Property::Name("x".into())).unwrap();
      assert_eq!(set(&mut p, "nope", Property::Visible(false)), Err(ElementError::NoElement("nope".into())));
  }

  #[test]
  fn raise_lower_remove_reorder_and_drop_files() {
      let mut p = pack(8, 6, 1);
      for name in ["a", "b", "c"] {
          add_layer(&mut p, ElementMaterial::Dirt, name).unwrap();
      }
      raise(&mut p, "a").unwrap();
      assert_eq!(ids(&p), ["base", "b", "a", "c"]);
      lower(&mut p, "c").unwrap();
      assert_eq!(ids(&p), ["base", "b", "c", "a"]);
      raise(&mut p, "a").unwrap(); // top: no-op
      lower(&mut p, "base").unwrap(); // bottom: no-op
      assert_eq!(ids(&p), ["base", "b", "c", "a"]);
      remove(&mut p, "b").unwrap();
      assert_eq!(ids(&p), ["base", "c", "a"]);
      assert!(!p.composition.as_ref().unwrap().files.contains_key(&layer_file("b")));
  }

  #[test]
  fn draw_and_erase_paint_whole_art_blocks() {
      let mut p = pack(8, 6, 2);
      let id = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
      draw(&mut p, &id, &Shape::Rect { x: 1, y: 1, w: 2, h: 2 }, &Paint::Colour(RED)).unwrap();
      let art = layer(&p, &id);
      for ay in 0..12 { for ax in 0..16 {
          let inside = (2..6).contains(&ax) && (2..6).contains(&ay);
          assert_eq!(art.get(ax, ay).a, if inside { 255 } else { 0 }, "({ax}, {ay})");
          if inside { assert_eq!(art.get(ax, ay), RED); }
      } }
      erase(&mut p, &id, &Shape::Circle { cx: 2, cy: 2, r: 0 }).unwrap();
      let art = layer(&p, &id);
      for ay in 4..6 { for ax in 4..6 { assert_eq!(art.get(ax, ay).a, 0); } }
      assert_eq!(art.get(2, 2).a, 255);
      assert!(matches!(draw(&mut p, "base", &Shape::Rect { x: 0, y: 0, w: 1, h: 1 }, &Paint::Colour(RED)),
          Err(ElementError::WrongKind { .. })));
  }

  #[test]
  fn add_image_refuses_a_hostile_file_and_writes_nothing() {
      let mut p = pack(8, 6, 1);
      add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
      let before = p.composition.clone();
      for (name, bytes) in [("huge.png", image_io::declared_png(100_000, 100_000, false)), ("junk.png", b"junk".to_vec())] {
          let e = add_image(&mut p, name, bytes, (0, 0), (4, 4), ElementMaterial::Dirt, "").unwrap_err();
          assert!(matches!(&e, ElementError::BadValue(m) if m.contains(name)), "{e}");
          assert_eq!(p.composition, before);
      }
  }

  #[test]
  fn resize_moves_boxes_and_relays_layers() {
      let mut p = pack(8, 6, 1);
      let img = add_image(&mut p, "a.png", png(2, 2), (3, 2), (2, 2), ElementMaterial::Rock, "").unwrap();
      let l = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
      draw(&mut p, &l, &Shape::Rect { x: 0, y: 0, w: 1, h: 1 }, &Paint::Colour(RED)).unwrap();
      resize(&mut p, (13, 9), (5, 3), Material::Rock, &Paint::Colour(RED)).unwrap();
      let src = p.composition.as_ref().unwrap();
      let at = src.composition.elements.iter().find(|e| e.id == img).map(|e| match &e.kind {
          ElementKind::Image { at, .. } => *at,
          _ => unreachable!(),
      });
      assert_eq!(at, Some((8, 5)));
      let art = layer(&p, &l);
      assert_eq!((art.width(), art.height()), (13, 9));
      assert_eq!(art.get(5, 3), RED);
      assert_eq!(art.get(0, 0).a, 0, "the new area is uncovered");
      let material = image_io::decode_indexed(&src.files["editor/layers/base.material.png"]).unwrap();
      assert_eq!((material.0, material.1), (13, 9));
      assert_eq!(material.2[0], Material::Rock as u8, "the raster's new area is the given material");
      check_composition(src, (13, 9), 1).unwrap();
  }
  ```

  `element::resize` leaves the level source to `level::resize`, which the CLI calls too (Task 8). So this test checks only the composition, against the new size.
- [ ] **Step 2:** Run `cargo nextest run -p olr-edit element`. Expected: FAIL.
- [ ] **Step 3:** Implement `element.rs` as specified.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit`. Expected: PASS.
- [ ] **Step 5:** Commit: `olr-edit: element operations on a composed level`.

### Task 8: The CLI: `level element …`, `level bake`, stale bakes, composed resize

**Files:**
- Create: `crates/olr-cli/src/element_cmd.rs`; `crates/olr-cli/tests/element.rs` (`mod element;` in `tests/integration.rs`).
- Modify:
  - `crates/olr-cli/src/level_cmd.rs` (an `Element` subcommand and `Bake`; ruling R1's refusal; composed `resize`);
  - `crates/olr-cli/src/pack_cmd.rs` (`pack check --content DIR`, the stale-bake diagnostics);
  - `crates/olr-cli/src/main.rs` (`mod element_cmd;`).

**Interfaces:**
- Produces the commands (spec §7, with ruling R1's `draw` and `erase`). Every command:
  - takes `--content DIR` (default `content/packs`) for the theme's back tile;
  - loads the pack, applies the operation, bakes (`compose::bake`) and saves (`pack::save`);
  - prints its messages after the save, and the save's warnings on stderr.

  ```
  olr-cli level element list      <pack>
  olr-cli level element add-image <pack> <file> --at X,Y --size WxH [--pixel-size N] [--colours N]
                                  [--dither none|ordered|diffusion] [--threshold N] [--material rock|dirt|background] [--name N]
  olr-cli level element add-layer <pack> [--material rock|dirt|background] [--name N]
  olr-cli level element set       <pack> <id> [--name N] [--visible true|false] [--locked true|false]
                                  [--material M] [--at X,Y] [--size WxH] [--pixel-size N] [--colours N|none]
                                  [--dither D] [--threshold N]
  olr-cli level element remove|raise|lower <pack> <id>
  olr-cli level element draw      <pack> <id> (--rect … | --circle …) (--colour RRGGBB | --tile F.png)
  olr-cli level element erase     <pack> <id> (--rect … | --circle …)
  olr-cli level bake              <pack>
  ```

  - **`list`** prints `<index>: <id> <kind> <material>[ "<name>"][ hidden][ locked]`, bottom first (index 0), for example `1: sky Image Background "Sky" hidden`. `<kind>` is `Image`, `Drawn` or `Raster`; a raster's material prints as `-`. A level without a composition prints nothing and exits 0.
  - **`add-image`** reads `<file>` bounded (a regular file, at most `MAX_FILE_BYTES`, as `level_cmd::parse_image` does) and prints `added element <id>`. `--material` defaults to `dirt`; `--pixel-size`, `--colours`, `--dither` and `--threshold` default as the format does.
  - **`--at`, `--rect` and `--circle`** take negative values (`allow_hyphen_values`).
  - **`level bake <pack>`** rebakes a composed level and prints `baked <pack>`. A level without a composition exits 1: `<pack>: no composition to bake`.
  - **`pack check [--content DIR]`:** for each composed level, prints each `compose::stale` file as an error: `<pack dir>: error: <file>: differs from the composition's bake (run olr-cli level bake)`. A theme not found is the warning `<pack dir>: warning: cannot check the bake: <error>`. These count in the summary.
  - **`level carve|fill|paint|stamp` on a composed level** exits 1 with ruling R1's message and writes nothing.
  - **`level resize` on a composed level** calls `element::resize` first (it reads the old size), then `level::resize` on the level source with the same arguments, then bakes.

- [ ] **Step 1: Write the failing tests** in `tests/element.rs`:

  ```rust
  //! `olr-cli level element …`, `level bake` and stale bakes (plan 14a Task 8).

  use std::collections::BTreeMap;
  use std::path::{Path, PathBuf};
  use std::process::{Command, Output};

  use olr_data::image::{Rgba, RgbaImage};

  use crate::common::{content_dir, copy_dir};

  /// A copy of Dirt Level and the content directory.
  fn dirt_level() -> Option<(tempfile::TempDir, PathBuf, String)> {
      let content = content_dir()?;
      let tmp = tempfile::tempdir().unwrap();
      let pack = tmp.path().join("dirt");
      copy_dir(&Path::new(&content).join("olx-level-dirt-level"), &pack);
      Some((tmp, pack, content))
  }

  /// `olr-cli <args…>` with `{pack}` and `{content}` replaced.
  fn cli(args: &[&str], pack: &Path, content: &str) -> Output {
      let args: Vec<String> = args
          .iter()
          .map(|a| a.replace("{pack}", pack.to_str().unwrap()).replace("{content}", content))
          .collect();
      Command::new(env!("CARGO_BIN_EXE_olr-cli")).args(&args).output().unwrap()
  }

  fn ok(o: &Output) -> String {
      assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
      String::from_utf8_lossy(&o.stdout).into_owned()
  }

  /// Every file under `dir`, by relative path.
  fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
      fn go(base: &Path, d: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
          for e in std::fs::read_dir(d).unwrap() {
              let p = e.unwrap().path();
              if p.is_dir() { go(base, &p, out) } else {
                  out.insert(p.strip_prefix(base).unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap());
              }
          }
      }
      let mut out = BTreeMap::new();
      go(dir, dir, &mut out);
      out
  }

  /// A 20 × 10 PNG, left half red, right half blue, beside the pack.
  fn sign_png(tmp: &Path) -> PathBuf {
      let mut img = RgbaImage::new(20, 10);
      for y in 0..10 { for x in 0..20 {
          img.set(x, y, if x < 10 { Rgba::rgb(255, 0, 0) } else { Rgba::rgb(0, 0, 255) });
      } }
      let path = tmp.join("sign.png");
      std::fs::write(&path, olr_format::image_io::encode_rgba(&img)).unwrap();
      path
  }

  /// Composes the pack: the sign image and a cave layer with a circle drawn.
  fn compose(tmp: &Path, pack: &Path, content: &str) {
      let png = sign_png(tmp);
      let out = ok(&cli(&["level", "element", "add-image", "{pack}", png.to_str().unwrap(), "--at", "-5,10",
          "--size", "40x20", "--pixel-size", "2", "--colours", "2", "--material", "rock", "--name", "Sign",
          "--content", "{content}"], pack, content));
      assert_eq!(out, "added element sign\n");
      ok(&cli(&["level", "element", "add-layer", "{pack}", "--material", "background", "--name", "Cave",
          "--content", "{content}"], pack, content));
      ok(&cli(&["level", "element", "draw", "{pack}", "cave", "--circle", "100,100,10", "--colour", "202020",
          "--content", "{content}"], pack, content));
  }

  #[test]
  fn compose_a_level_from_an_image_and_a_layer() {
      let Some((tmp, pack, content)) = dirt_level() else { return };
      let original = std::fs::read(pack.join("material.png")).unwrap();
      compose(tmp.path(), &pack, &content);
      let list = ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content));
      assert_eq!(list, "0: base Raster -\n1: sign Image Rock \"Sign\"\n2: cave Drawn Background \"Cave\"\n");
      let check = ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
      assert!(check.contains("0 errors"), "{check}");
      assert_ne!(std::fs::read(pack.join("material.png")).unwrap(), original);
      let preview = tmp.path().join("m.png");
      ok(&cli(&["level", "preview", "{pack}", "--out", preview.to_str().unwrap(), "--material"], &pack, &content));
      let img = olr_format::image_io::decode_rgba(&std::fs::read(&preview).unwrap()).unwrap();
      assert_eq!(img.get(100, 105), Rgba::rgb(0, 0, 0), "the drawn cave is background");
  }

  #[test]
  fn pack_check_reports_a_stale_bake() {
      let Some((tmp, pack, content)) = dirt_level() else { return };
      compose(tmp.path(), &pack, &content);
      let front = olr_format::image_io::decode_rgba(&std::fs::read(pack.join("front.png")).unwrap()).unwrap();
      std::fs::write(pack.join("front.png"),
          olr_format::image_io::encode_rgba(&RgbaImage::new(front.width(), front.height()))).unwrap();
      let out = cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content);
      assert_eq!(out.status.code(), Some(1));
      assert!(String::from_utf8_lossy(&out.stdout).contains("front.png: differs from the composition's bake"));
      ok(&cli(&["level", "bake", "{pack}", "--content", "{content}"], &pack, &content));
      ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
  }

  #[test]
  fn raster_operations_on_a_composed_level_are_refused() {
      let Some((_tmp, pack, content)) = dirt_level() else { return };
      ok(&cli(&["level", "element", "add-layer", "{pack}", "--content", "{content}"], &pack, &content));
      let before = tree(&pack);
      let out = cli(&["level", "carve", "{pack}", "--circle", "1,1,1"], &pack, &content);
      assert_eq!(out.status.code(), Some(1));
      assert!(String::from_utf8_lossy(&out.stderr).contains("a composed level: edit its elements"), "{out:?}");
      assert_eq!(tree(&pack), before);
  }

  #[test]
  fn add_image_refuses_a_hostile_file_and_writes_nothing() {
      let Some((tmp, pack, content)) = dirt_level() else { return };
      let junk = tmp.path().join("junk.png");
      std::fs::write(&junk, b"not an image").unwrap();
      let before = tree(&pack);
      let out = cli(&["level", "element", "add-image", "{pack}", junk.to_str().unwrap(), "--at", "0,0",
          "--size", "4x4", "--content", "{content}"], &pack, &content);
      assert!(!out.status.success());
      assert_eq!(tree(&pack), before);
  }

  #[test]
  fn set_hide_raise_remove() {
      let Some((tmp, pack, content)) = dirt_level() else { return };
      compose(tmp.path(), &pack, &content);
      ok(&cli(&["level", "element", "set", "{pack}", "sign", "--visible", "false", "--content", "{content}"], &pack, &content));
      assert!(ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content)).contains("1: sign Image Rock \"Sign\" hidden"));
      ok(&cli(&["level", "element", "raise", "{pack}", "sign", "--content", "{content}"], &pack, &content));
      assert!(ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content)).contains("2: sign"));
      ok(&cli(&["level", "element", "remove", "{pack}", "sign", "--content", "{content}"], &pack, &content));
      assert!(!ok(&cli(&["level", "element", "list", "{pack}"], &pack, &content)).contains("sign"));
      assert_eq!(std::fs::read_dir(pack.join("editor/images")).map(|d| d.count()).unwrap_or(0), 0);
  }

  #[test]
  fn resize_a_composed_level() {
      let Some((tmp, pack, content)) = dirt_level() else { return };
      compose(tmp.path(), &pack, &content);
      ok(&cli(&["level", "resize", "{pack}", "--size", "520x350", "--offset", "16,0", "--content", "{content}"], &pack, &content));
      let text = std::fs::read_to_string(pack.join("editor/composition.ron")).unwrap();
      assert!(text.contains("at: (11, 10)"), "{text}");
      ok(&cli(&["pack", "check", "{pack}", "--content", "{content}"], &pack, &content));
  }
  ```

  `level resize` gains `--content` (for the bake) like the element commands.
- [ ] **Step 2:** Run `cargo nextest run -p olr-cli --test integration element::`. Expected: FAIL.
- [ ] **Step 3:** Implement `element_cmd.rs`, and the `level_cmd.rs`, `pack_cmd.rs` and `main.rs` changes. Parse strictly with clap: reuse `level_cmd`'s `parse_ints`, `parse_point`, `parse_colour` and `parse_image` (make them `pub(crate)`) and `pack_cmd::parse_size`.
- [ ] **Step 4:** Run `cargo nextest run -p olr-cli --test integration element:: level:: cli:: corpus::`. Expected: PASS, with the corpus snapshot unchanged.
- [ ] **Step 5:** Commit: `olr-cli: level element commands, level bake, stale bakes in pack check`.

### Task 9: The bake on every platform (`olr-detcheck`, `cargo xtask determinism`)

**Files:**
- Create: `fixtures/edit/composed-level/`, a small composed level pack (see Step 1), and its `README.md` (how it was made).
- Modify:
  - `crates/olr-detcheck/Cargo.toml` (`olr-edit`, `olr-format`, `sha2`);
  - `crates/olr-detcheck/src/main.rs` (`--bake <pack dir>`);
  - `xtask/src/determinism.rs` (every leg also runs `--bake fixtures/edit/composed-level` and compares the line);
  - `crates/olr-edit/tests/compose.rs` (`the_fixture_is_baked`).

**Interfaces:**
- Produces:
  - `olr-detcheck --content <dir> --bake <pack dir>` prints one line, `bake <sha256 hex>`: the SHA-256 over `material.png`, `front.png` and `back.png` as `compose::bake` produces them, encoded by `write_level_files`, in that order, each prefixed with its 8-byte little-endian length. It exits 1 on any error.
  - `cargo xtask determinism`'s comparison includes that line for every leg (native, WASI, Windows, arm64).
- [ ] **Step 1: Make the fixture** with the Task 8 commands on a copy of `olx-level-dirt-level` renamed to `composed-level` (`pack new level composed-level --from content/packs/olx-level-dirt-level --out fixtures/edit/composed-level`):
  - an image element from `crates/olr-format/tests/data/tiny.jpg`, pixel size 3, 4 colours, ordered dither;
  - one from a small PNG with transparency, 8 colours, diffusion;
  - a drawn layer with two shapes.

  Then `level bake`. Commit the result, with the commands in its `README.md`.
- [ ] **Step 2: Write the failing test** `the_fixture_is_baked` in `tests/compose.rs`: `stale(&load(fixture)?, &theme_back_tile(content, "olx-theme-dirt")?)` is empty. Run `cargo nextest run -p olr-edit the_fixture`. Expected: PASS once the fixture exists, since the fixture was baked by the code under test. The test guards against later changes: confirm it by temporarily changing one Bayer matrix entry, watching it FAIL, and restoring it.
- [ ] **Step 3:** Implement `--bake` in `olr-detcheck`, and extend `xtask/src/determinism.rs`. Read its `step` and legs first: each leg's command output is a stream of lines compared to the native one; append the bake line to each leg's stream by running the leg's `olr-detcheck` once more with `--bake`.
  - Under WASI, the fixture path must lie under the repository root (preopened as `/`).
  - Check that `olr-detcheck` still builds for `wasm32-wasip1` (`cargo build -p olr-detcheck --target wasm32-wasip1`).
- [ ] **Step 4:** Run `cargo xtask determinism` (the WASM leg; about 15 s warm). Expected: `ok`, with the bake line equal. Do not run the Windows or arm64 legs without the user's consent; they run in `full`.
- [ ] **Step 5:** Commit: `olr-detcheck: the bake digest; determinism compares it on every leg`.
- [ ] **Step 6 (batch C):** Run the online `cargo xtask check`. Expected: every step `ok`.

### Task 10: Docs: the schema page, the guide, records

**Files:**
- Modify:
  - `xtask/src/format_docs.rs`: the level page's sources gain `crates/olr-format/src/levels/composition.rs`, and its roots gain `("Composition", "editor/composition.ron")`.
  - `docs/formats/README.md`: in "The model", a section "Composed levels" explaining elements, topmost wins, background elements as the backdrop, the raster base, `editor/` (not hashed, not downloaded) and the bake. Add a checked example block `<!-- file: level/editor/composition.ron -->`, with one image element (`source: "sign.png"`), one drawn layer and the raster base. In "The workflow", add the `level element …` and `level bake` commands.
  - `crates/olr-edit/tests/guide.rs`: the level pack built from the guide's blocks also gets the composition's files (`editor/images/sign.png` as a 4 × 4 PNG; the drawn layer and the raster base as level-sized PNGs of the example's size × art scale), so `the_guide_examples_make_packs_that_check_clean` still validates. `every_guide_example_parses_and_is_canonical` covers the block through `canonical_text`.
  - `docs/formats/*.md`: regenerated by `cargo xtask format-docs`.
  - The `CLAUDE.md` of `olr-format`, `olr-pack`, `olr-edit`, `olr-cli`, `olr-detcheck` and `xtask`: what each task added, in each file's style.
  - `CHANGELOG.md` `Unreleased`, Added: "Levels can be composed of elements (imported images, drawn layers) and baked: `olr-cli level element …` and `level bake`."
  - `docs/porting/STATUS.md`: a row for level composition (plan 14a), and a short "Level composition decisions (plan 14a)" section with rulings R1-R9.
  - The spec: amend §2's "CLI edits" rule and §7 for ruling R1 (`draw`/`erase`; raster operations refused on composed levels), and §9 (14a done).
- [ ] **Step 1:** Write the guide's section and block, and extend `guide.rs`. Run `cargo nextest run -p olr-edit guide`. Expected: FAIL until the block is canonical and the pack checks clean; fix the block to the canonical text the failure prints.
- [ ] **Step 2:** Add the composition to the level page; run `cargo xtask format-docs` and add any doc comment it reports missing. Run `cargo test -p xtask format_docs` and `cargo xtask format-docs --check`. Expected: PASS.
- [ ] **Step 3:** Write the records listed above.
- [ ] **Step 4:** Run `cargo nextest run -p olr-edit guide` and `cargo xtask format-docs --check` once more. Expected: PASS.
- [ ] **Step 5:** Commit: `docs: level composition: the schema page, the guide and plan 14a's records`.
