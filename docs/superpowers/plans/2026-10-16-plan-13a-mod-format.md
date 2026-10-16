# Plan 13a: The Mod Authoring Format Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate `olr-format` holds the mod authoring format: the
readable, nested files that editors, agents and people write. It provides
a canonical writer (load then save gives the same bytes), a compiler to
today's runtime `ModDef`, and a lift from `ModDef` back to the authoring
form. Tests prove that both bundled OLX mods survive lift, write, read
and compile unchanged, down to the simulation's `SimMod`.

**Architecture:**
- `olr-format` (layer 2) depends only on `olr-data`, `olr-vfs`, `serde`,
  `ron` and `thiserror`.
- **The authoring types** (`mods::types`, `mods::spawn`) are plain serde
  structs and enums. Every field equal to its default is left out when
  written, with defaults compared by bits for floats.
- **`ron_io`** is the one place that turns values into text and back, with
  `implicit_some` on so `Some(…)` is never written.
- **`mods::compile`** maps authoring data to `olr_data::mod_def::ModDef`.
  **`mods::lift`** maps a `ModDef`, such as today's import, back.
- **`mods::files`** reads and writes the pack's `mod.ron`, `weapons/`,
  `projectiles/` and `restrictions.ron`.
- **Nothing outside the new crate changes behaviour.** The packs on disk,
  the loaders and the game keep using `olr-pack` until plan 13c switches
  them.

**Tech Stack:** Rust 1.99 (edition 2024). Existing workspace dependencies
only: `serde`, `ron` 0.12.2, `thiserror`, plus `proptest` and `insta`
for tests. **No new dependency.**

**Spec:** `docs/superpowers/specs/2026-10-16-formats-design.md` §1, §2
and §6. The research behind every name, unit and default is
`docs/porting/olx-notes/mod-fields.md` (committed with this plan); read
its §0 and §1 first.

## Global Constraints

- **The crate:** `olr-format`, layer 2 in `xtask/layers.ron`, `std`; not
  `no_std`. Its dependencies are `olr-data`, `olr-vfs`, `serde`, `ron`
  and `thiserror`. Dev-dependencies may be any crate (the layer check
  skips them).
- **No other crate's behaviour changes.** `olr-sim`, `olr-data`,
  `olr-pack`, `olr-content` and the packs in `content/packs` stay
  byte-identical.
- **Never panic on content.** Every read, compile and lift returns
  `Result`; no `unwrap` or `expect` on input-derived data.
- **Deterministic output.** `BTreeMap`/`BTreeSet` only; no `HashMap`.
- **Floats are compared and defaulted by bits** (`f32::to_bits`). So
  `-0.0` is written and read back as `-0.0`, and a default of `0.5` is
  left out only when the value is exactly `0.5f32`.
- **Canonical text:**
  - 4-space indent, no struct names, `implicit_some`, no `#![enable]`
    header, a trailing newline;
  - fields in declaration order; defaults left out;
  - colours as lowercase `"#rrggbb"` (alpha 255) or `"#rrggbbaa"`.
- **Units are named, never rescaled.** A number keeps the runtime value
  exactly. Field docs state the unit from
  `docs/porting/olx-notes/mod-fields.md`.
- **Verification cadence (the user's rule, 2026-10-16).**
  - Inside a task, run `cargo nextest run -p olr-format`.
  - Run `cargo xtask check` only where a step says so: the end of Task 4
    and the end of Task 7.
  - No `cargo xtask full` in this plan. It changes no shipped code path,
    and 13c's switch-over runs `full`.
- **Commits:** small and imperative, `olr-format: …`, with the
  session's attribution lines.

## Review Focus

1. **Signed zero and subnormal floats** (`-0.0`, `1e-40`) in any field
   must survive write and read bit for bit, never be dropped as "the
   default". Task 1 tests the helpers and Task 6's proptest covers every
   field.
2. **Colours written by hand.**
   - `"#FFAA00"` reads (and is rewritten lowercase).
   - `"#fa0"`, `"ffaa00"` and `"#ffaa0g"` are errors that say what a
     colour looks like.
   - Task 1 tests all four.
3. **Files from Windows editors and agents.** A UTF-8 byte-order mark and
   CRLF line endings must read like plain LF files. Task 1 tests both.
4. **A pack whose files and list disagree.** A weapon listed in `mod.ron`
   whose file is missing, an unlisted `weapons/*.ron`, a duplicate id, a
   nested `projectiles/a/b.ron`: each is an error naming the file. Task 5
   tests each.
5. **Hostile nesting.** A projectile nested 200 levels deep (text) or 40
   levels (values) must be an error, never a stack overflow. Task 5 tests
   the text case and Task 3 the value case.

---

## File Structure

```
crates/olr-format/
  Cargo.toml
  CLAUDE.md               crate notes (layer, rules, use)
  src/lib.rs              modules, re-exports
  src/error.rs            FormatError, LiftError
  src/ron_io.rs           canonical text <-> values; bounded file reads
  src/colour.rs           Colour: hex strings
  src/defaults.rs         default and skip helpers (bit-exact floats)
  src/mods/mod.rs         ModSource, Worm, Rope; the mod module's API
  src/mods/types.rs       Weapon, Fire, Projectile, Gfx, Timer, Trail, Effect, …
  src/mods/spawn.rs       Spawn<C> with shot / event / trail context defaults
  src/mods/compile.rs     ModSource -> olr_data::mod_def::ModDef
  src/mods/lift.rs        ModDef -> ModSource
  src/mods/files.rs       read and write a pack's mod files
  tests/integration.rs    the one integration binary (mod … per file)
  tests/equivalence.rs    bundled mods: lift, write, read, compile = import; SimMod
  tests/roundtrip.rs      proptest: canonical round trip of random authoring values
  tests/hostile.rs        fuzzed and malformed files never panic
  tests/snapshots/        insta snapshots of canonical files
docs/porting/olx-notes/mod-fields.md   research table (committed with the plan; Task 7 appends the final names)
xtask/layers.ron                       + "olr-format": 2
Cargo.toml                             + workspace member dependency
```

---

### Task 1: The crate, canonical RON, colours and defaults

**Files:**
- Create: `crates/olr-format/Cargo.toml`, `src/lib.rs`, `src/error.rs`,
  `src/ron_io.rs`, `src/colour.rs`, `src/defaults.rs`,
  `src/mods/mod.rs` (an empty module for now), `CLAUDE.md`,
  `tests/integration.rs`
- Modify: `Cargo.toml` (workspace dependency), `xtask/layers.ron`

**Interfaces:**
- Produces:
  - `olr_format::FormatError`:
    - `Parse { file: String, line: usize, col: usize, message: String }`
    - `Invalid { file: String, path: String, message: String }`
    - `Io { file: String, message: String }`

    `Display` gives `file:line:col: message`, `file: path: message` and
    `file: message` respectively.
  - `olr_format::LiftError { item: String, message: String }` (`Display`
    gives `item: message`).
  - `olr_format::ron_io::{to_canonical, from_canonical, read_text,
    write_text, MAX_RON_BYTES, RECURSION_LIMIT}`:

    ```rust
    pub fn to_canonical<T: Serialize>(file: &str, value: &T) -> Result<String, FormatError>
    pub fn from_canonical<T: DeserializeOwned>(file: &str, text: &str) -> Result<T, FormatError>
    pub(crate) fn read_text(src: &dyn ContentSource, file: &str) -> Result<String, FormatError>
    pub(crate) fn write_text(sink: &mut dyn ContentSink, file: &str, text: &str) -> Result<(), FormatError>
    ```
  - `olr_format::Colour(pub Rgba)` with `to_hex()` and
    `parse(&str) -> Result<Colour, String>`, serialized as a string.
  - The `crate::defaults` helpers (the names below).

- [ ] **Step 1: Create the crate and register it**

`crates/olr-format/Cargo.toml`:

```toml
[package]
name = "olr-format"
description = "Pack authoring formats: readable files for editors and agents, a canonical writer, and compilers to the runtime data."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true
autotests = false

[dependencies]
olr-data.workspace = true
olr-vfs.workspace = true
serde = { workspace = true, features = ["std"] }
ron.workspace = true
thiserror.workspace = true

[dev-dependencies]
olr-pack.workspace = true
olr-sim.workspace = true
proptest.workspace = true
insta.workspace = true

[[test]]
name = "integration"
path = "tests/integration.rs"

[lints]
workspace = true
```

In the workspace `Cargo.toml` `[workspace.dependencies]`, after
`olr-pack = …`, add `olr-format = { path = "crates/olr-format" }`. In
`xtask/layers.ron` `layers`, after `"olr-pack": 2,`, add
`"olr-format": 2,`.

`tests/integration.rs`:

```rust
//! `olr-format`'s integration tests: one binary (CLAUDE.md rule 2).

mod equivalence;
mod hostile;
mod roundtrip;
```

Create empty `tests/equivalence.rs`, `tests/hostile.rs` and
`tests/roundtrip.rs`, each holding only a `//!` line that says what the
file will test. Later tasks fill them.

- [ ] **Step 2: Write the failing tests**

`src/ron_io.rs`, test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound: Option<String>,
        gravity: Option<i32>,
        size: (i32, i32),
    }

    #[test]
    fn options_are_implicit_and_no_header_is_written() {
        let s = Sample { sound: Some("sfx/a.ogg".into()), gravity: Some(0), size: (4, 4) };
        let text = to_canonical("t.ron", &s).unwrap();
        assert!(!text.contains("Some("), "{text}");
        assert!(!text.contains("#!["), "{text}");
        assert!(text.ends_with(")\n"), "{text}");
        assert_eq!(from_canonical::<Sample>("t.ron", &text).unwrap(), s);
    }

    #[test]
    fn parse_errors_name_the_file_line_and_column() {
        let err = from_canonical::<Sample>("weapons/x.ron", "(\n    gravity: \"no\",\n)").unwrap_err();
        match err {
            FormatError::Parse { file, line, .. } => {
                assert_eq!(file, "weapons/x.ron");
                assert_eq!(line, 2);
            }
            e => panic!("unexpected {e}"),
        }
    }

    #[test]
    fn a_byte_order_mark_and_crlf_read_like_plain_text() {
        let text = "\u{feff}(\r\n    gravity: 3,\r\n    size: (1, 2),\r\n)\r\n";
        let s: Sample = from_canonical("t.ron", text).unwrap();
        assert_eq!(s, Sample { sound: None, gravity: Some(3), size: (1, 2) });
    }

    #[test]
    fn unknown_fields_are_errors_when_the_type_denies_them() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Strict {
            #[allow(dead_code)]
            a: i32,
        }
        assert!(from_canonical::<Strict>("t.ron", "(a: 1, b: 2)").is_err());
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let text = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
        assert!(from_canonical::<ron::Value>("t.ron", &text).is_err());
    }
}
```

`src/colour.rs`, test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_colours_are_six_digits_and_others_eight() {
        assert_eq!(Colour(Rgba::rgb(0x94, 0x88, 0x00)).to_hex(), "#948800");
        assert_eq!(Colour(Rgba { r: 1, g: 2, b: 3, a: 4 }).to_hex(), "#01020304");
    }

    #[test]
    fn hex_reads_in_any_case_and_round_trips() {
        assert_eq!(Colour::parse("#FFAA00").unwrap(), Colour(Rgba::rgb(255, 170, 0)));
        for c in [Colour(Rgba::rgb(1, 2, 3)), Colour(Rgba { r: 9, g: 8, b: 7, a: 0 })] {
            assert_eq!(Colour::parse(&c.to_hex()).unwrap(), c);
        }
    }

    #[test]
    fn malformed_colours_say_what_a_colour_looks_like() {
        for bad in ["#fa0", "ffaa00", "#ffaa0g", "#ffaa00f", ""] {
            let e = Colour::parse(bad).unwrap_err();
            assert!(e.contains("#rrggbb"), "{bad}: {e}");
        }
    }
}
```

`src/defaults.rs`, test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_defaults_compare_bits() {
        assert!(is_zero(&0.0));
        assert!(!is_zero(&-0.0), "-0.0 must be written, or it reads back as 0.0");
        assert!(!is_zero(&1e-40), "a subnormal is not zero");
        assert!(is_half(&0.5));
        assert!(!is_half(&0.500_000_06));
        assert!(is_tenth(&0.1));
    }
}
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo nextest run -p olr-format`
Expected: the build fails, because `to_canonical`, `Colour` and
`is_zero` do not exist yet.

- [ ] **Step 4: Implement**

`src/lib.rs`:

```rust
//! Pack authoring formats (spec `docs/superpowers/specs/2026-10-16-formats-design.md`):
//! the files editors, agents and people write, their canonical text, and
//! their compilers to `olr-data`'s runtime structs. Plan 13a: mods.

mod colour;
pub(crate) mod defaults;
mod error;
pub mod mods;
pub mod ron_io;

pub use colour::Colour;
pub use error::{FormatError, LiftError};
```

`src/error.rs`:

```rust
use thiserror::Error;

/// Why a pack's authoring files could not be read, written or compiled.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FormatError {
    /// The text is not valid RON for its type.
    #[error("{file}:{line}:{col}: {message}")]
    Parse { file: String, line: usize, col: usize, message: String },
    /// The value is valid RON but not a valid pack: `path` is the field
    /// path inside the file, such as `fire.shot.projectile.hit_worm[1]`.
    #[error("{file}: {path}: {message}")]
    Invalid { file: String, path: String, message: String },
    /// The file could not be read or written.
    #[error("{file}: {message}")]
    Io { file: String, message: String },
}

impl FormatError {
    pub(crate) fn invalid(file: &str, path: &str, message: impl Into<String>) -> Self {
        Self::Invalid { file: file.into(), path: path.into(), message: message.into() }
    }
}

/// A runtime value the authoring format cannot express losslessly. Today's
/// imports never produce one; a hand-built `ModDef` can.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{item}: {message}")]
pub struct LiftError {
    /// Such as `weapon "bazooka"` or `projectile "bazooka.spawn"`.
    pub item: String,
    pub message: String,
}
```

`src/ron_io.rs`:

```rust
//! The one path between values and pack text. Canonical text is what
//! [`to_canonical`] writes: reading it and writing it again gives the same
//! bytes.

use olr_vfs::{ContentSink, ContentSource, VPath};
use ron::extensions::Extensions;
use ron::ser::PrettyConfig;
use ron::Options;
use serde::{Serialize, de::DeserializeOwned};

use crate::FormatError;

/// The longest authoring file read, in bytes (the largest import, MW 1.0's
/// `restrictions.ron`, is about 21 KB).
pub const MAX_RON_BYTES: u64 = 4 << 20;

/// RON nesting allowed while reading or writing. One projectile level is
/// about four RON levels, so this leaves room above
/// [`crate::mods::MAX_INLINE_DEPTH`] (32), and deeper text is an error
/// before it can exhaust the stack.
pub const RECURSION_LIMIT: usize = 256;

fn options() -> Options {
    Options::default()
        .with_default_extension(Extensions::IMPLICIT_SOME)
        .with_recursion_limit(RECURSION_LIMIT)
}

fn pretty() -> PrettyConfig {
    PrettyConfig::default()
        .indentor("    ".to_string())
        .struct_names(false)
        .extensions(Extensions::IMPLICIT_SOME)
}

/// `value` as canonical text, with a trailing newline. It fails only when
/// `value` nests deeper than [`RECURSION_LIMIT`].
pub fn to_canonical<T: Serialize>(file: &str, value: &T) -> Result<String, FormatError> {
    let mut s = options()
        .to_string_pretty(value, pretty())
        .map_err(|e| FormatError::Io { file: file.into(), message: e.to_string() })?;
    s.push('\n');
    Ok(s)
}

/// Parses `text` (canonical or not; a leading byte-order mark is skipped).
pub fn from_canonical<T: DeserializeOwned>(file: &str, text: &str) -> Result<T, FormatError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    options().from_str(text).map_err(|e| FormatError::Parse {
        file: file.into(),
        line: e.span.start.line,
        col: e.span.start.col,
        message: e.code.to_string(),
    })
}

pub(crate) fn read_text(src: &dyn ContentSource, file: &str) -> Result<String, FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    let path = VPath::new(file).map_err(|e| io(e.to_string()))?;
    let bytes = src.read_max(&path, MAX_RON_BYTES).map_err(|e| io(e.to_string()))?;
    String::from_utf8(bytes).map_err(|e| io(format!("not UTF-8: {e}")))
}

pub(crate) fn write_text(sink: &mut dyn ContentSink, file: &str, text: &str) -> Result<(), FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    let path = VPath::new(file).map_err(|e| io(e.to_string()))?;
    sink.write(&path, text.as_bytes()).map_err(|e| io(e.to_string()))
}
```

`src/colour.rs`:

```rust
//! Colours as hex strings: `"#rrggbb"` (opaque) or `"#rrggbbaa"`.

use olr_data::image::Rgba;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A colour in an authoring file. Written lowercase; read in any case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour(pub Rgba);

const SHAPE: &str = "a colour is \"#rrggbb\" or \"#rrggbbaa\" (hex digits)";

impl Colour {
    pub fn to_hex(&self) -> String {
        let Rgba { r, g, b, a } = self.0;
        if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
    }

    pub fn parse(s: &str) -> Result<Colour, String> {
        let bad = || format!("{s:?}: {SHAPE}");
        let hex = s.strip_prefix('#').ok_or_else(bad)?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        let a = if hex.len() == 8 { byte(6)? } else { 255 };
        Ok(Colour(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a }))
    }
}

impl Serialize for Colour {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Colour {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Colour::parse(&s).map_err(serde::de::Error::custom)
    }
}
```

`src/defaults.rs`:

```rust
//! Default values and `skip_serializing_if` predicates for the authoring
//! types. Floats compare by bits: `-0.0` and subnormals are never "the
//! default" (Review Focus 1).

macro_rules! f32_defaults {
    ($($get:ident / $is:ident = $v:expr;)*) => {$(
        pub(crate) fn $get() -> f32 { $v }
        pub(crate) fn $is(v: &f32) -> bool { v.to_bits() == ($v as f32).to_bits() }
    )*};
}

f32_defaults! {
    zero / is_zero = 0.0;
    one / is_one = 1.0;
    half / is_half = 0.5;
    tenth / is_tenth = 0.1;
    three_tenths / is_three_tenths = 0.3;
    worm_ground_acceleration / is_worm_ground_acceleration = 8.0;
    worm_air_acceleration / is_worm_air_acceleration = 4.0;
    worm_gravity / is_worm_gravity = 50.0;
    worm_jump_speed / is_worm_jump_speed = -75.0;
    worm_unused_angle_speed / is_worm_unused_angle_speed = 150.0;
    worm_unused_ground_friction / is_worm_unused_ground_friction = 0.4;
    rope_strength / is_rope_strength = 3.5;
}

macro_rules! i32_defaults {
    ($($get:ident / $is:ident = $v:expr;)*) => {$(
        pub(crate) fn $get() -> i32 { $v }
        pub(crate) fn $is(v: &i32) -> bool { *v == $v }
    )*};
}

i32_defaults! {
    one_i32 / is_one_i32 = 1;
    rope_max_length / is_rope_max_length = 300;
    rope_rest_length / is_rope_rest_length = 30;
}

pub(crate) fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}

pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}
```

`src/mods/mod.rs` for now:

```rust
//! The mod authoring format (spec §2).
```

`CLAUDE.md`:

```markdown
# olr-format

Pack authoring formats (spec `docs/superpowers/specs/2026-10-16-formats-design.md`):
the readable files editors, agents and people write; their canonical text;
compilers to `olr-data`'s runtime structs; lifts back. Plan 13a: mods.

- **Layer 2, `std`.** Depends on `olr-data`, `olr-vfs`, `serde`, `ron`,
  `thiserror`. Tests also use `olr-pack` and `olr-sim` (dev-dependencies).
- **Canonical text** (`ron_io`): `to_canonical`/`from_canonical` are the only
  path between values and text: 4-space indent, no struct names,
  `implicit_some` with no header, defaults left out, a trailing newline.
  Reading canonical text and writing it again gives the same bytes.
- **Defaults compare floats by bits** (`defaults.rs`): `-0.0` is not `0.0`.
- **Never panic on content**; errors are `FormatError` with the file and a
  line and column (parse) or a field path (invalid values).
```

- [ ] **Step 5: Run the tests**

Run: `cargo nextest run -p olr-format`
Expected: PASS (11 tests).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock xtask/layers.ron crates/olr-format
git commit -m "olr-format: canonical RON, colours and defaults"
```

---

### Task 2: The mod authoring types

**Files:**
- Create: `crates/olr-format/src/mods/types.rs`, `src/mods/spawn.rs`
- Modify: `crates/olr-format/src/mods/mod.rs`

**Interfaces:**
- Consumes: Task 1's `defaults`, `Colour`, `ron_io`.
- Produces (all `pub`, re-exported from `olr_format::mods`):
  - `ModSource { name: String, notes: String, worm: Worm, rope: Rope,
    weapons: Vec<(String, Weapon)>, shared: BTreeMap<String, Projectile>,
    restrictions: BTreeMap<String, olr_data::restrictions::Preset> }`
    (`Clone, Debug, PartialEq`).
  - `Worm { ground_acceleration, air_acceleration, gravity, jump_speed,
    air_drag, unused_angle_speed, unused_ground_friction }` (all `f32`)
    with `Default` (OLX's FeatureList values) and `is_default(&self)`.
  - `Rope { max_length: i32, rest_length: i32, strength: f32 }` with
    `Default` and `is_default(&self)`.
  - `Weapon { name, notes, recharge: f32, drain: f32, between_shots_s:
    f32, laser_sight: bool, fire: Fire }`.
  - `Fire::{Projectile { class, recoil, sound, shot }, Beam { recoil,
    colour, hole_size, worm_damage, length, width, width_growth,
    split_damage_over_width }, Special { kind, thrust } }`.
  - `WeaponClass::{Automatic, PowerGun, Grenade, Missile, CloseRange,
    Unknown(i32)}` and `SpecialKind::{Off, Jetpack, Unknown(i32)}`.
  - `Projectile { id: Option<String>, notes, gfx: Gfx, size:
    Option<(i32, i32)>, gravity: Option<i32>, velocity_factor: f32,
    timer: Option<Timer>, trail: Trail, hit_terrain: Vec<Effect>,
    hit_worm: Vec<Effect>, spawns: BTreeMap<String, EventSpawn> }`.
  - `Gfx::{Pixels(Vec<Colour>), Image { file, rotating,
    rotation_step_deg, rotation_speed, frames_follow_heading,
    frames_follow_heading_mirrored, heading_frames, animation:
    Option<Animation> }}`.
  - `Animation { fps: f32, mode: AnimMode }` and
    `AnimMode::{Once, Loop, PingPong, Unknown(i32)}`.
  - `Timer { after_s: f32, variation_s: f32, effects: Vec<Effect> }`.
  - `Trail::{Off, Smoke, ChemSmoke, Doomsday, Explosive, Projectile {
    every_s: f32, spawn: TrailSpawn }, Unknown(i32)}`.
  - `Effect` (variants listed in Step 4).
  - `Spawn<C: SpawnContext>` and the aliases `ShotSpawn`, `EventSpawn`
    and `TrailSpawn`.

- [ ] **Step 1: Write the failing tests**

In `src/mods/types.rs`, test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ron_io::{from_canonical, to_canonical};
    use olr_data::image::Rgba;

    fn bazooka() -> Weapon {
        let shrapnel = Projectile {
            gfx: Gfx::Pixels(vec![Colour(Rgba::rgb(0x94, 0x88, 0x00))]),
            ..Projectile::new(Gfx::Pixels(vec![]))
        };
        let mut rocket = Projectile::new(Gfx::Image {
            file: "gfx/missile.png".into(),
            rotating: false,
            rotation_step_deg: 0,
            rotation_speed: 0,
            frames_follow_heading: true,
            frames_follow_heading_mirrored: false,
            heading_frames: 18,
            animation: None,
        });
        rocket.gravity = Some(0);
        rocket.velocity_factor = 1.001;
        rocket.trail = Trail::Smoke;
        rocket.hit_terrain = vec![
            Effect::Explode { damage: 4, shake: 2, sound: Some("sfx/exp2.ogg".into()) },
            Effect::Spawn("spawn".into()),
        ];
        rocket.hit_worm = vec![Effect::Explode { damage: 27, shake: 0, sound: None }, Effect::Spawn("spawn".into())];
        rocket.spawns.insert(
            "spawn".into(),
            EventSpawn {
                amount: 12,
                speed: 105,
                speed_var: 14.0,
                spread: 360.0,
                projectile: Some(Box::new(shrapnel)),
                ..EventSpawn::default()
            },
        );
        Weapon {
            name: "Bazooka".into(),
            notes: String::new(),
            recharge: 0.171,
            drain: 33.334,
            between_shots_s: 1.071,
            laser_sight: false,
            fire: Fire::Projectile {
                class: WeaponClass::Automatic,
                recoil: 15,
                sound: Some("sfx/bazooka.ogg".into()),
                shot: ShotSpawn { speed: 140, projectile: Some(Box::new(rocket)), ..ShotSpawn::default() },
            },
        }
    }

    #[test]
    fn a_weapon_round_trips_through_canonical_text() {
        let w = bazooka();
        let text = to_canonical("weapons/bazooka.ron", &w).unwrap();
        assert_eq!(from_canonical::<Weapon>("weapons/bazooka.ron", &text).unwrap(), w);
        assert_eq!(to_canonical("weapons/bazooka.ron", &from_canonical::<Weapon>("x", &text).unwrap()).unwrap(), text);
    }

    #[test]
    fn defaults_are_left_out() {
        let text = to_canonical("weapons/bazooka.ron", &bazooka()).unwrap();
        for absent in [
            "Some(", "laser_sight", "class", "notes", "amount: 1,", "relative_to_parent", "random_rotation",
            "inherit_velocity", "inherit_velocity_factor", "parent_velocity", "speed_var_diagonal", "size",
            "timer", "rotating", "animation", "angle:", "bounce", "struct",
        ] {
            assert!(!text.contains(absent), "{absent:?} should be left out:\n{text}");
        }
        for present in ["\"#948800\"", "Smoke", "heading_frames: 18", "gravity: 0", "Spawn(\"spawn\")"] {
            assert!(text.contains(present), "{present:?} missing:\n{text}");
        }
    }

    #[test]
    fn spawn_defaults_depend_on_the_context() {
        let shot = to_canonical("t", &ShotSpawn::default()).unwrap();
        let event = to_canonical("t", &EventSpawn::default()).unwrap();
        let trail = to_canonical("t", &TrailSpawn::default()).unwrap();
        assert_eq!(shot, "()\n");
        assert_eq!(event, "()\n");
        assert_eq!(trail, "()\n");
        let shot_off = ShotSpawn { relative_to_parent: false, ..ShotSpawn::default() };
        assert!(to_canonical("t", &shot_off).unwrap().contains("relative_to_parent: false"));
        let event_on = EventSpawn { relative_to_parent: true, ..EventSpawn::default() };
        assert!(to_canonical("t", &event_on).unwrap().contains("relative_to_parent: true"));
        assert!(TrailSpawn::default().speed_var_diagonal);
        assert!(!EventSpawn::default().speed_var_diagonal);
        assert_eq!(ShotSpawn::default().amount, 1);
    }

    #[test]
    fn negative_zero_is_kept() {
        let mut p = Projectile::new(Gfx::Pixels(vec![]));
        p.velocity_factor = -0.0;
        let text = to_canonical("t", &p).unwrap();
        let back: Projectile = from_canonical("t", &text).unwrap();
        assert_eq!(back.velocity_factor.to_bits(), (-0.0f32).to_bits(), "{text}");
    }

    #[test]
    fn misspelt_fields_are_errors() {
        let e = from_canonical::<Projectile>("t", "(gfx: Pixels([]), hit_wrom: [])").unwrap_err();
        assert!(e.to_string().contains("hit_wrom"), "{e}");
    }

    #[test]
    fn worm_and_rope_defaults_are_olx_feature_list_values() {
        let w = Worm::default();
        assert_eq!((w.ground_acceleration, w.air_acceleration, w.gravity, w.jump_speed), (8.0, 4.0, 50.0, -75.0));
        assert!(w.is_default());
        assert!(!Worm { gravity: 100.0, ..w.clone() }.is_default());
        assert_eq!((Rope::default().max_length, Rope::default().rest_length, Rope::default().strength), (300, 30, 3.5));
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo nextest run -p olr-format mods::types`
Expected: the build fails (the types do not exist).

- [ ] **Step 3: Implement `spawn.rs`**

```rust
//! Spawns: how a weapon's shot, a projectile's events and its trail
//! create child projectiles (OLX `Proj_SpawnInfo`). LX56 fixes several
//! flags per context (`docs/porting/olx-notes/mod-fields.md` §1.5), so
//! each context has its own defaults and imported files carry none of
//! them.

use std::fmt::Debug;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use super::types::Projectile;

/// The defaults that differ between spawn contexts.
pub trait SpawnContext: Clone + Copy + Debug + Default + PartialEq + 'static {
    const RELATIVE_TO_PARENT: bool;
    const SPEED_VAR_DIAGONAL: bool;
    const RANDOM_ROTATION: bool;
    const INHERIT_VELOCITY: bool;
}

/// A weapon's shot: aimed along the worm's aim, random start rotation,
/// inherits the worm's velocity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShotContext;
impl SpawnContext for ShotContext {
    const RELATIVE_TO_PARENT: bool = true;
    const SPEED_VAR_DIAGONAL: bool = false;
    const RANDOM_ROTATION: bool = true;
    const INHERIT_VELOCITY: bool = true;
}

/// A projectile's event spawn (`GeneralSpawnInfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventContext;
impl SpawnContext for EventContext {
    const RELATIVE_TO_PARENT: bool = false;
    const SPEED_VAR_DIAGONAL: bool = false;
    const RANDOM_ROTATION: bool = false;
    const INHERIT_VELOCITY: bool = false;
}

/// A projectile trail's spawn (`Proj_Trail`): speed variation along (1, 1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrailContext;
impl SpawnContext for TrailContext {
    const RELATIVE_TO_PARENT: bool = false;
    const SPEED_VAR_DIAGONAL: bool = true;
    const RANDOM_ROTATION: bool = false;
    const INHERIT_VELOCITY: bool = false;
}

pub type ShotSpawn = Spawn<ShotContext>;
pub type EventSpawn = Spawn<EventContext>;
pub type TrailSpawn = Spawn<TrailContext>;

/// Per child `i` in `0..amount`, the direction is
/// `parent_velocity_as_direction ? parent.vel * parent_velocity_direction_scale
/// : angle + (relative_to_parent ? parent heading : 0) + r1 * spread` (degrees),
/// and the velocity is `dir * speed + (speed_var_diagonal ? (1, 1) : dir) *
/// speed_var * r2 + (inherit_velocity ? inherit_velocity_factor * parent.vel : 0)`,
/// with r1, r2 in [-1, 1] (OLX `PhysicsLX56_Projectiles.cpp:562-640`,
/// `olr-sim/src/spawn.rs`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "SpawnWire", into = "SpawnWire", bound = "C: SpawnContext")]
pub struct Spawn<C: SpawnContext> {
    /// The id of a shared projectile (`projectiles/<id>.ron`). Exactly one
    /// of `shared` and `projectile` is set.
    pub shared: Option<String>,
    /// The spawned projectile, defined here.
    pub projectile: Option<Box<Projectile>>,
    /// Children per spawn. Default 1.
    pub amount: i32,
    /// px/s (unitless when `parent_velocity_as_direction`).
    pub speed: i32,
    /// px/s, ± range.
    pub speed_var: f32,
    /// Degrees, ± half-width (360 = any direction).
    pub spread: f32,
    /// Adds the parent's heading to `angle`.
    pub relative_to_parent: bool,
    /// Degrees; 0 = right, 90 = down.
    pub angle: i32,
    /// Direction from the parent's velocity instead of an angle.
    pub parent_velocity_as_direction: bool,
    /// Scale of that velocity. Default 0.3.
    pub parent_velocity_direction_scale: f32,
    /// Speed variation along (1, 1) instead of the direction.
    pub speed_var_diagonal: bool,
    /// A random start rotation for a rotating child.
    pub random_rotation: bool,
    /// Adds `inherit_velocity_factor` × the parent's velocity.
    pub inherit_velocity: bool,
    /// Default 1.0.
    pub inherit_velocity_factor: f32,
    pub context: PhantomData<C>,
}

impl<C: SpawnContext> Default for Spawn<C> {
    fn default() -> Self {
        Self {
            shared: None,
            projectile: None,
            amount: 1,
            speed: 0,
            speed_var: 0.0,
            spread: 0.0,
            relative_to_parent: C::RELATIVE_TO_PARENT,
            angle: 0,
            parent_velocity_as_direction: false,
            parent_velocity_direction_scale: 0.3,
            speed_var_diagonal: C::SPEED_VAR_DIAGONAL,
            random_rotation: C::RANDOM_ROTATION,
            inherit_velocity: C::INHERIT_VELOCITY,
            inherit_velocity_factor: 1.0,
            context: PhantomData,
        }
    }
}

/// The text form: a field is written only when it differs from its
/// context's default (floats by bits).
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shared: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projectile: Option<Box<Projectile>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    amount: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speed: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speed_var: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    spread: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relative_to_parent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_velocity_as_direction: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_velocity_direction_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speed_var_diagonal: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    random_rotation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inherit_velocity: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inherit_velocity_factor: Option<f32>,
}

fn differs<T: PartialEq>(v: T, d: T) -> Option<T> {
    (v != d).then_some(v)
}

fn differs_f32(v: f32, d: f32) -> Option<f32> {
    (v.to_bits() != d.to_bits()).then_some(v)
}

impl<C: SpawnContext> From<Spawn<C>> for SpawnWire {
    fn from(s: Spawn<C>) -> Self {
        let d = Spawn::<C>::default();
        SpawnWire {
            shared: s.shared,
            projectile: s.projectile,
            amount: differs(s.amount, d.amount),
            speed: differs(s.speed, d.speed),
            speed_var: differs_f32(s.speed_var, d.speed_var),
            spread: differs_f32(s.spread, d.spread),
            relative_to_parent: differs(s.relative_to_parent, d.relative_to_parent),
            angle: differs(s.angle, d.angle),
            parent_velocity_as_direction: differs(s.parent_velocity_as_direction, d.parent_velocity_as_direction),
            parent_velocity_direction_scale: differs_f32(
                s.parent_velocity_direction_scale,
                d.parent_velocity_direction_scale,
            ),
            speed_var_diagonal: differs(s.speed_var_diagonal, d.speed_var_diagonal),
            random_rotation: differs(s.random_rotation, d.random_rotation),
            inherit_velocity: differs(s.inherit_velocity, d.inherit_velocity),
            inherit_velocity_factor: differs_f32(s.inherit_velocity_factor, d.inherit_velocity_factor),
        }
    }
}

impl<C: SpawnContext> From<SpawnWire> for Spawn<C> {
    fn from(w: SpawnWire) -> Self {
        let d = Spawn::<C>::default();
        Spawn {
            shared: w.shared,
            projectile: w.projectile,
            amount: w.amount.unwrap_or(d.amount),
            speed: w.speed.unwrap_or(d.speed),
            speed_var: w.speed_var.unwrap_or(d.speed_var),
            spread: w.spread.unwrap_or(d.spread),
            relative_to_parent: w.relative_to_parent.unwrap_or(d.relative_to_parent),
            angle: w.angle.unwrap_or(d.angle),
            parent_velocity_as_direction: w.parent_velocity_as_direction.unwrap_or(d.parent_velocity_as_direction),
            parent_velocity_direction_scale: w
                .parent_velocity_direction_scale
                .unwrap_or(d.parent_velocity_direction_scale),
            speed_var_diagonal: w.speed_var_diagonal.unwrap_or(d.speed_var_diagonal),
            random_rotation: w.random_rotation.unwrap_or(d.random_rotation),
            inherit_velocity: w.inherit_velocity.unwrap_or(d.inherit_velocity),
            inherit_velocity_factor: w.inherit_velocity_factor.unwrap_or(d.inherit_velocity_factor),
            context: PhantomData,
        }
    }
}
```

If `serde`'s derive rejects `bound` together with `from`/`into` on the
generic struct, implement `Serialize` and `Deserialize` for `Spawn<C>` by
hand through `SpawnWire` (a five-line impl each), keeping the behaviour
above.

- [ ] **Step 4: Implement `types.rs`**

Every field's doc states its meaning, unit and OLX source; take them
from `docs/porting/olx-notes/mod-fields.md` §5–§12, one line each. The
docs are part of the deliverable: plan 13c generates the schema reference
from them. The code below shows them for the less obvious fields; write
the same for every field.

```rust
//! Weapons and projectiles as authoring files (spec §2). Field docs give
//! each field's meaning, unit and OLX source
//! (`docs/porting/olx-notes/mod-fields.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::spawn::{EventSpawn, ShotSpawn, TrailSpawn};
use crate::Colour;
use crate::defaults as d;

/// `weapons/<id>.ron` (OLX `weapon_t`, `include/WeaponDesc.h:90-121`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weapon {
    /// Shown in menus; restrictions and bot loadouts match on it.
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Full magazines per second at LoadingTime 100 % (OLX stores the
    /// file's value ÷ 10, `CGameScript.cpp:777`; this is that value).
    pub recharge: f32,
    /// Percent of a full magazine used per shot (> 100 empties it).
    pub drain: f32,
    /// Seconds between shots (OLX's `ROF` ÷ 1000, `CGameScript.cpp:776`).
    pub between_shots_s: f32,
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub laser_sight: bool,
    pub fire: Fire,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Fire {
    /// `WPN_PROJECTILE`.
    Projectile {
        /// Unused by OLX and the bots; kept for the record.
        #[serde(default, skip_serializing_if = "d::is_default")]
        class: WeaponClass,
        /// px/s velocity impulse against the aim, once per shot.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        recoil: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound: Option<String>,
        shot: ShotSpawn,
    },
    /// `WPN_BEAM` (`include/WeaponDesc.h:71-85`).
    Beam {
        /// Unused by the simulation (bots read its sign).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        recoil: i32,
        /// Not drawn when `#ff00ff`.
        colour: Colour,
        /// Hole carved at the first dirt or rock pixel (0..4); -1 = none.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        hole_size: i32,
        /// Health points to the first worm within 5 px of the trace.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        worm_damage: i32,
        /// px.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        length: i32,
        /// px. Default 1.
        #[serde(default = "d::one_i32", skip_serializing_if = "d::is_one_i32")]
        width: i32,
        /// px of width per px of length.
        #[serde(default, skip_serializing_if = "d::is_zero")]
        width_growth: f32,
        #[serde(default, skip_serializing_if = "d::is_false")]
        split_damage_over_width: bool,
    },
    /// `WPN_SPECIAL`.
    Special {
        kind: SpecialKind,
        /// Jetpack: × 50 px/s² upward per 10 ms frame.
        thrust: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponClass {
    #[default]
    Automatic,
    PowerGun,
    Grenade,
    Missile,
    CloseRange,
    Unknown(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecialKind {
    /// OLX `SPC_NONE`: drains the charge, no thrust.
    Off,
    Jetpack,
    Unknown(i32),
}

/// A projectile (OLX `proj_t`, `include/ProjectileDesc.h:80-121`), inline in
/// a spawn or in `projectiles/<id>.ron` (shared).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projectile {
    /// The runtime id, written only when it differs from the one derived
    /// from its place (`<weapon id>`, then `.spawn` / `.trail` per level).
    /// Never set in `projectiles/` files (their id is the file name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Simulation data, not only looks: the colour count, rotation and
    /// animation affect play (`mod-fields.md` §1.2).
    pub gfx: Gfx,
    /// Collision box in px. Default (2, 2) for `Pixels`, (4, 4) for `Image`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<(i32, i32)>,
    /// px/s²; absent = the world's gravity (100). `0` and `100` are
    /// written: they are not "absent" (`mod-fields.md` §1.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravity: Option<i32>,
    /// Velocity × this once per 11 ms physics step (> 1 accelerates).
    /// OLX's `Dampening`.
    #[serde(default = "d::one", skip_serializing_if = "d::is_one")]
    pub velocity_factor: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer: Option<Timer>,
    #[serde(default, skip_serializing_if = "Trail::is_off")]
    pub trail: Trail,
    /// When the projectile hits rock or dirt. `[]` = OLX's `Nothing`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hit_terrain: Vec<Effect>,
    /// When it hits a worm. `[]` = `Nothing`, which also turns worm
    /// collision off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hit_worm: Vec<Effect>,
    /// Named spawns that `Spawn(<name>)` effects use. This runtime takes at
    /// most one, shared by all events (OLX `GeneralSpawnInfo`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub spawns: BTreeMap<String, EventSpawn>,
}

impl Projectile {
    /// A projectile with `gfx` and every other field at its default.
    pub fn new(gfx: Gfx) -> Self {
        Self {
            id: None,
            notes: String::new(),
            gfx,
            size: None,
            gravity: None,
            velocity_factor: 1.0,
            timer: None,
            trail: Trail::Off,
            hit_terrain: Vec::new(),
            hit_worm: Vec::new(),
            spawns: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Gfx {
    /// One colour is drawn at spawn (one `rand()`), as a 1 px square.
    Pixels(Vec<Colour>),
    Image {
        /// Pack path. Frames are square: frame count = width ÷ height.
        file: String,
        #[serde(default, skip_serializing_if = "d::is_false")]
        rotating: bool,
        /// Degrees per image frame; the random start rotation's step.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        rotation_step_deg: i32,
        /// Degrees per second while `rotating`.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        rotation_speed: i32,
        /// Render only: the frame follows the heading.
        #[serde(default, skip_serializing_if = "d::is_false")]
        frames_follow_heading: bool,
        /// Render only: left- and right-moving halves over 151°.
        #[serde(default, skip_serializing_if = "d::is_false")]
        frames_follow_heading_mirrored: bool,
        /// Render only: frames used by the two heading schemes.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        heading_frames: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        animation: Option<Animation>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    /// Frames per second.
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub fps: f32,
    /// At the last frame. `Once` removes the projectile.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub mode: AnimMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimMode {
    #[default]
    Once,
    Loop,
    PingPong,
    Unknown(i32),
}

/// OLX `Proj_LX56Timer`. It fires every step once its time has passed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timer {
    /// Seconds of projectile life. The effects need `after_s > 0`.
    pub after_s: f32,
    /// Seconds, ± range (× the projectile's fixed random in [-1, 1]).
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub variation_s: f32,
    /// `[]` = `Nothing`, which also cancels a spawn on a timer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
}

/// OLX `Proj_Trail`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Trail {
    #[default]
    Off,
    /// A smoke entity every 75 ms.
    Smoke,
    /// A chemical-smoke entity every 75 ms.
    ChemSmoke,
    /// A doomsday entity every 50 ms.
    Doomsday,
    /// A cosmetic explosion entity (size 10) every 50 ms.
    Explosive,
    /// A spawn every `every_s` seconds (whole ms), the first on the first step.
    Projectile {
        #[serde(default = "d::tenth", skip_serializing_if = "d::is_tenth")]
        every_s: f32,
        spawn: TrailSpawn,
    },
    Unknown(i32),
}

impl Trail {
    pub fn is_off(&self) -> bool {
        *self == Trail::Off
    }
}

/// What an event does (OLX `Proj_ActionType`, `include/ProjAction.h:147-175`).
/// An event's list holds at most one effect besides `Spawn` in this runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// A blast: `damage` health points to every worm within 5 px and a hole
    /// of size `clamp(damage, 0, 4)`; -1 = no blast (still dies and spawns).
    /// `shake`: world px of camera shake.
    Explode {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound: Option<String>,
    },
    /// Velocity × `factor` on the colliding side; `explode` > 0 blasts too.
    Bounce {
        #[serde(default = "d::half", skip_serializing_if = "d::is_half")]
        factor: f32,
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        explode: i32,
    },
    /// On a worm: `damage` health points, then the projectile dies.
    Injure {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
    },
    /// A hole of `size` (0..4) at once, then dies.
    Carve {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        size: i32,
    },
    /// Three dirt balls, then dies. `shake` applies on a timer.
    Dirt {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
    },
    /// Green dirt, then dies. `shake` applies on a timer.
    GreenDirt {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
    },
    /// OLX's `Disappear` (7): mostly ignored in LX56, it does **not** remove
    /// the projectile. `Remove` does.
    Disappear,
    /// OLX's `Disappear2` (9): the projectile dies.
    Remove,
    /// Passes through terrain (`hit_terrain`) or worms (`hit_worm`).
    GoThrough,
    /// No effect in LX56 events (no targets).
    InjureProjectile,
    /// Plays `file` (never set by LX56 files).
    Sound {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
    /// On a worm: `damage` health points; keeps flying.
    InjureWorm {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
    },
    ChangeRadius,
    SetSpeed,
    MultiplySpeed,
    AddSpeed,
    SetTargetSpeed,
    MultiplyTargetSpeed,
    AddTargetSpeed,
    HomeOnNearestWorm,
    HomeOnOwner,
    HomeOnNearestOtherWorm,
    HomeOnNearestEnemy,
    HomeOnNearestTeamMate,
    PullTargetsToUs,
    /// An OLX action number this format has no name for.
    Unknown(i32),
    /// Spawns the named entry of the projectile's `spawns`.
    Spawn(String),
}
```

- [ ] **Step 5: `mods/mod.rs`: `ModSource`, `Worm` and `Rope`**

```rust
//! The mod authoring format (spec §2): `mod.ron`, `weapons/<id>.ron`,
//! `projectiles/<id>.ron` (shared projectiles) and `restrictions.ron`.

use std::collections::BTreeMap;

use olr_data::restrictions::Preset;
use serde::{Deserialize, Serialize};

use crate::defaults as d;

mod spawn;
mod types;

pub use spawn::{EventContext, EventSpawn, ShotContext, ShotSpawn, Spawn, SpawnContext, TrailContext, TrailSpawn};
pub use types::*;

/// A mod as its authoring files hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModSource {
    pub name: String,
    pub notes: String,
    pub worm: Worm,
    pub rope: Rope,
    /// In order: a weapon's index is its id in the game.
    pub weapons: Vec<(String, Weapon)>,
    /// `projectiles/<id>.ron`: projectiles several places use.
    pub shared: BTreeMap<String, Projectile>,
    pub restrictions: BTreeMap<String, Preset>,
}

/// Worm physics of the mod (OLX `gs_worm_t`, `CGameScript.cpp:45-55`).
/// Defaults are OLX's FeatureList values (`FeatureList.cpp:67-71`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worm {
    /// × 90 px/s² while walking on the ground.
    #[serde(default = "d::worm_ground_acceleration", skip_serializing_if = "d::is_worm_ground_acceleration")]
    pub ground_acceleration: f32,
    /// × 90 px/s² while moving in the air.
    #[serde(default = "d::worm_air_acceleration", skip_serializing_if = "d::is_worm_air_acceleration")]
    pub air_acceleration: f32,
    /// px/s².
    #[serde(default = "d::worm_gravity", skip_serializing_if = "d::is_worm_gravity")]
    pub gravity: f32,
    /// px/s, negative = up: a ground jump sets the vertical speed to it.
    #[serde(default = "d::worm_jump_speed", skip_serializing_if = "d::is_worm_jump_speed")]
    pub jump_speed: f32,
    /// Quadratic drag in the air, 1/px.
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub air_drag: f32,
    /// Ignored by OLX ("was always ignored", `CGameScript.cpp:45`).
    #[serde(default = "d::worm_unused_angle_speed", skip_serializing_if = "d::is_worm_unused_angle_speed")]
    pub unused_angle_speed: f32,
    /// Ignored by OLX (`CGameScript.cpp:53`).
    #[serde(default = "d::worm_unused_ground_friction", skip_serializing_if = "d::is_worm_unused_ground_friction")]
    pub unused_ground_friction: f32,
}

impl Default for Worm {
    fn default() -> Self {
        Self {
            ground_acceleration: d::worm_ground_acceleration(),
            air_acceleration: d::worm_air_acceleration(),
            gravity: d::worm_gravity(),
            jump_speed: d::worm_jump_speed(),
            air_drag: 0.0,
            unused_angle_speed: d::worm_unused_angle_speed(),
            unused_ground_friction: d::worm_unused_ground_friction(),
        }
    }
}

impl Worm {
    /// Every field equals its default, by bits.
    pub fn is_default(&self) -> bool {
        d::is_worm_ground_acceleration(&self.ground_acceleration)
            && d::is_worm_air_acceleration(&self.air_acceleration)
            && d::is_worm_gravity(&self.gravity)
            && d::is_worm_jump_speed(&self.jump_speed)
            && d::is_zero(&self.air_drag)
            && d::is_worm_unused_angle_speed(&self.unused_angle_speed)
            && d::is_worm_unused_ground_friction(&self.unused_ground_friction)
    }
}

/// Ninja rope of the mod (`CGameScript.cpp:787-800`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rope {
    /// px: a flying hook farther than this stops and falls.
    #[serde(default = "d::rope_max_length", skip_serializing_if = "d::is_rope_max_length")]
    pub max_length: i32,
    /// px: no pull while the worm is closer.
    #[serde(default = "d::rope_rest_length", skip_serializing_if = "d::is_rope_rest_length")]
    pub rest_length: i32,
    /// × 100 px/s² toward the hook.
    #[serde(default = "d::rope_strength", skip_serializing_if = "d::is_rope_strength")]
    pub strength: f32,
}

impl Default for Rope {
    fn default() -> Self {
        Self { max_length: 300, rest_length: 30, strength: 3.5 }
    }
}

impl Rope {
    pub fn is_default(&self) -> bool {
        d::is_rope_max_length(&self.max_length)
            && d::is_rope_rest_length(&self.rest_length)
            && d::is_rope_strength(&self.strength)
    }
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo nextest run -p olr-format`
Expected: PASS. If `defaults_are_left_out` fails on how `ron` prints a
value (for example `Bounce()` for an all-default struct variant), fix the
type's attributes, not the test's expectations. The one exception: `ron`
spacing inside tuples is acceptable either way.

- [ ] **Step 7: Commit**

```bash
git add crates/olr-format
git commit -m "olr-format: the mod authoring types"
```

---

### Task 3: Compiling to `ModDef`

**Files:**
- Create: `crates/olr-format/src/mods/compile.rs`
- Modify: `crates/olr-format/src/mods/mod.rs` (`mod compile; pub use
  compile::{compile_mod, MAX_INLINE_DEPTH, MAX_WEAPONS, MAX_SHARED,
  MAX_EFFECTS, MAX_COLOURS};`)

**Interfaces:**
- Consumes: Task 2's types.
- Produces:
  - `pub fn compile_mod(src: &ModSource) -> Result<olr_data::mod_def::ModDef,
    FormatError>`.
  - The constants `MAX_INLINE_DEPTH = 32`, `MAX_WEAPONS = 512`,
    `MAX_SHARED = 4096`, `MAX_EFFECTS = 16` and `MAX_COLOURS = 256`.
  - Also `pub(crate) const MOD_FILE: &str = "mod.ron";` and the file-name
    helpers `weapon_file(id)` and `shared_file(id)`, which Task 5 reuses.

**Rules the compiler implements:**
- **Projectile keys** (`ModDef::projectiles`):
  - a shared projectile's key is its file id;
  - an inline projectile's key is its `id` if set, else derived: the
    weapon id for a shot, then `<parent key>.spawn` for the event spawn
    and `<parent key>.trail` for the trail spawn;
  - a key used twice is an error.
- **Events:**
  - Each effect list compiles to one `ActionDef`: its one non-`Spawn`
    effect sets `kind` and the fields that kind uses (an empty list means
    `Nothing`).
  - `Spawn(n)` sets `spawns_projectiles`.
  - More than one non-`Spawn` effect, or two `Spawn`s, is "not supported
    by this runtime".
  - All events must name the same spawn.
  - `spawns` may hold at most one entry (its only entry becomes
    `event_spawn`, even when no event names it).
- **Timer:** `None` gives `TimerDef { time: 0.0, time_var: 0.0, action:
  None }`. A timer with `after_s > 0` gets `action: Some(…)`. With
  `after_s <= 0` it has no action, and non-empty effects there are an
  error.
- **Trail:** a non-`Projectile` trail compiles to `delay: 0.1, spawn:
  None`.
- **Size:** `size.unwrap_or` (2, 2) for `Pixels` and (4, 4) for `Image`.
- **Bounds:** more than `MAX_INLINE_DEPTH` nested inline projectiles,
  more than `MAX_WEAPONS` weapons, `MAX_SHARED` shared projectiles,
  `MAX_EFFECTS` effects in one list or `MAX_COLOURS` pixel colours is an
  error.
- **Cycles:** a cycle between shared projectiles is an error (OLX's
  files cannot express one; spec §2).
- **Ids:** weapon ids, shared ids and explicit `id`s must be lowercase
  ASCII letters, digits, `-`, `_` and `.`, and must not start with `.`
  (as `olr-pack` checks today).

- [ ] **Step 1: Write the failing tests**

In `compile.rs`, test module. The helper builds a one-weapon mod:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::mod_def as rt;

    fn one_weapon(shot: ShotSpawn) -> ModSource {
        ModSource {
            name: "Test".into(),
            notes: String::new(),
            worm: Worm::default(),
            rope: Rope::default(),
            weapons: vec![(
                "gun".into(),
                Weapon {
                    name: "Gun".into(),
                    notes: String::new(),
                    recharge: 0.5,
                    drain: 30.0,
                    between_shots_s: 0.25,
                    laser_sight: false,
                    fire: Fire::Projectile { class: WeaponClass::Missile, recoil: 5, sound: None, shot },
                },
            )],
            shared: BTreeMap::new(),
            restrictions: BTreeMap::new(),
        }
    }

    fn pixel() -> Projectile {
        Projectile::new(Gfx::Pixels(vec![]))
    }

    fn inline(p: Projectile) -> Option<Box<Projectile>> {
        Some(Box::new(p))
    }

    #[test]
    fn a_tree_compiles_to_derived_keys_and_lx56_actions() {
        let mut rocket = pixel();
        rocket.hit_terrain = vec![Effect::Explode { damage: 4, shake: 2, sound: Some("sfx/x.ogg".into()) }, Effect::Spawn("bits".into())];
        rocket.spawns.insert("bits".into(), EventSpawn { amount: 5, projectile: inline(pixel()), ..EventSpawn::default() });
        rocket.trail = Trail::Projectile {
            every_s: 0.04,
            spawn: TrailSpawn { projectile: inline(pixel()), ..TrailSpawn::default() },
        };
        let def = compile_mod(&one_weapon(ShotSpawn { speed: 100, projectile: inline(rocket), ..ShotSpawn::default() })).unwrap();
        let keys: Vec<&str> = def.projectiles.keys().map(String::as_str).collect();
        assert_eq!(keys, ["gun", "gun.spawn", "gun.trail"]);
        let gun = &def.projectiles["gun"];
        assert_eq!(gun.hit.kind, rt::ActionKind::Explode);
        assert_eq!((gun.hit.damage, gun.hit.shake, gun.hit.spawns_projectiles), (4, 2, true));
        assert_eq!(gun.hit.sound.as_deref(), Some("sfx/x.ogg"));
        assert_eq!(gun.player_hit, rt::ActionDef::olx_default(rt::ActionKind::Nothing));
        assert_eq!(gun.event_spawn.as_ref().unwrap().projectile, "gun.spawn");
        assert_eq!(gun.event_spawn.as_ref().unwrap().amount, 5);
        assert_eq!(gun.trail.kind, rt::TrailKind::Projectile);
        assert_eq!(gun.trail.delay, 0.04);
        let trail = gun.trail.spawn.as_ref().unwrap();
        assert!(trail.use_special11_vec_for_speed_var && !trail.use_angle);
        assert_eq!((gun.width, gun.height), (2, 2));
        let rt::WeaponKind::Projectile { spawn, class, recoil, .. } = &def.weapons[0].kind else { panic!() };
        assert!(spawn.use_angle && spawn.use_random_rot && spawn.add_parent_vel);
        assert_eq!((spawn.projectile.as_str(), spawn.speed, *class, *recoil), ("gun", 100, rt::WeaponClass::Missile, 5));
        assert_eq!(def.weapons[0].id, "gun");
        assert_eq!(def.weapons[0].rof, 0.25);
    }

    #[test]
    fn an_explicit_id_and_a_shared_projectile_keep_their_keys() {
        let mut src = one_weapon(ShotSpawn { shared: Some("shell".into()), ..ShotSpawn::default() });
        let mut shell = pixel();
        shell.spawns.insert("s".into(), EventSpawn { projectile: inline(Projectile { id: Some("odd-name".into()), ..pixel() }), ..EventSpawn::default() });
        src.shared.insert("shell".into(), shell);
        let def = compile_mod(&src).unwrap();
        let keys: Vec<&str> = def.projectiles.keys().map(String::as_str).collect();
        assert_eq!(keys, ["odd-name", "shell"]);
    }

    #[test]
    fn what_lx56_cannot_express_is_an_error_with_a_path() {
        let mut two = pixel();
        two.hit_worm = vec![Effect::Injure { damage: 1 }, Effect::Explode { damage: 1, shake: 0, sound: None }];
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(two), ..ShotSpawn::default() })).unwrap_err();
        assert!(e.to_string().contains("fire.shot.projectile.hit_worm[1]"), "{e}");
        assert!(e.to_string().contains("not supported by this runtime"), "{e}");
    }

    #[test]
    fn a_spawn_names_an_entry_of_spawns() {
        let mut p = pixel();
        p.hit_terrain = vec![Effect::Spawn("missing".into())];
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() })).unwrap_err();
        assert!(e.to_string().contains("no spawn named \"missing\""), "{e}");
    }

    #[test]
    fn events_must_share_one_spawn() {
        let mut p = pixel();
        p.hit_terrain = vec![Effect::Spawn("a".into())];
        p.hit_worm = vec![Effect::Spawn("b".into())];
        p.spawns.insert("a".into(), EventSpawn { projectile: inline(pixel()), ..EventSpawn::default() });
        p.spawns.insert("b".into(), EventSpawn { projectile: inline(pixel()), ..EventSpawn::default() });
        assert!(compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() })).is_err());
    }

    #[test]
    fn a_spawn_takes_exactly_one_of_shared_and_projectile() {
        assert!(compile_mod(&one_weapon(ShotSpawn::default())).is_err());
        let both = ShotSpawn { shared: Some("x".into()), projectile: inline(pixel()), ..ShotSpawn::default() };
        assert!(compile_mod(&one_weapon(both)).is_err());
        let unknown = ShotSpawn { shared: Some("nope".into()), ..ShotSpawn::default() };
        let e = compile_mod(&one_weapon(unknown)).unwrap_err();
        assert!(e.to_string().contains("projectiles/nope.ron"), "{e}");
    }

    #[test]
    fn a_key_used_twice_is_an_error() {
        let mut p = pixel();
        p.spawns.insert("s".into(), EventSpawn { projectile: inline(Projectile { id: Some("gun".into()), ..pixel() }), ..EventSpawn::default() });
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() })).unwrap_err();
        assert!(e.to_string().contains("\"gun\""), "{e}");
    }

    #[test]
    fn shared_cycles_are_errors() {
        let mut src = one_weapon(ShotSpawn { shared: Some("a".into()), ..ShotSpawn::default() });
        let refer = |to: &str| {
            let mut p = pixel();
            p.spawns.insert("s".into(), EventSpawn { shared: Some(to.into()), ..EventSpawn::default() });
            p
        };
        src.shared.insert("a".into(), refer("b"));
        src.shared.insert("b".into(), refer("a"));
        let e = compile_mod(&src).unwrap_err();
        assert!(e.to_string().contains("cycle"), "{e}");
    }

    #[test]
    fn deep_inline_nesting_is_an_error_not_a_crash() {
        let mut p = pixel();
        for _ in 0..40 {
            let mut parent = pixel();
            parent.spawns.insert("s".into(), EventSpawn { projectile: inline(p), ..EventSpawn::default() });
            p = parent;
        }
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() })).unwrap_err();
        assert!(e.to_string().contains("nested deeper than 32"), "{e}");
    }

    #[test]
    fn timer_effects_need_a_positive_time() {
        let mut p = pixel();
        p.timer = Some(Timer { after_s: 0.0, variation_s: 0.0, effects: vec![Effect::Remove] });
        assert!(compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() })).is_err());
        let mut q = pixel();
        q.timer = Some(Timer { after_s: 2.0, variation_s: 0.5, effects: vec![] });
        let def = compile_mod(&one_weapon(ShotSpawn { projectile: inline(q), ..ShotSpawn::default() })).unwrap();
        let t = &def.projectiles["gun"].timer;
        assert_eq!((t.time, t.time_var), (2.0, 0.5));
        assert_eq!(t.action.as_ref().unwrap().kind, rt::ActionKind::Nothing);
    }

    #[test]
    fn invalid_ids_are_errors() {
        let mut src = one_weapon(ShotSpawn { projectile: inline(pixel()), ..ShotSpawn::default() });
        src.weapons[0].0 = "../evil".into();
        assert!(compile_mod(&src).is_err());
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p olr-format mods::compile`
Expected: the build fails (`compile_mod` is missing).

- [ ] **Step 3: Implement `compile.rs`**

```rust
//! Authoring mod -> runtime `ModDef` (spec §2). Pure, deterministic, never
//! panics. Projectile keys follow `olr-import-lgs`'s naming, so lifting an
//! import and compiling it gives the import back (Task 6).

use std::collections::{BTreeMap, BTreeSet};

use olr_data::mod_def as rt;

use super::spawn::{Spawn, SpawnContext};
use super::types::*;
use super::{ModSource, Rope, Worm};
use crate::FormatError;

pub const MAX_INLINE_DEPTH: usize = 32;
pub const MAX_WEAPONS: usize = 512;
pub const MAX_SHARED: usize = 4096;
pub const MAX_EFFECTS: usize = 16;
pub const MAX_COLOURS: usize = 256;

pub(crate) const MOD_FILE: &str = "mod.ron";

pub(crate) fn weapon_file(id: &str) -> String {
    format!("weapons/{id}.ron")
}

pub(crate) fn shared_file(id: &str) -> String {
    format!("projectiles/{id}.ron")
}

/// Pack ids name files: lowercase ASCII, digits, `-`, `_`, `.`; not starting with `.`.
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.'))
}

fn at(path: &str, seg: &str) -> String {
    if path.is_empty() { seg.to_string() } else { format!("{path}.{seg}") }
}

struct Compiler<'a> {
    src: &'a ModSource,
    file: String,
    taken: BTreeSet<String>,
    out: BTreeMap<String, rt::ProjectileDef>,
}

pub fn compile_mod(src: &ModSource) -> Result<rt::ModDef, FormatError> {
    if src.weapons.len() > MAX_WEAPONS {
        return Err(FormatError::invalid(MOD_FILE, "weapons", format!("more than {MAX_WEAPONS} weapons")));
    }
    if src.shared.len() > MAX_SHARED {
        return Err(FormatError::invalid("projectiles/", "", format!("more than {MAX_SHARED} shared projectiles")));
    }
    check_cycles(src)?;
    let mut c = Compiler { src, file: String::new(), taken: BTreeSet::new(), out: BTreeMap::new() };
    for (id, p) in &src.shared {
        c.file = shared_file(id);
        if p.id.is_some() {
            return Err(c.err("id", "a shared projectile's id is its file name: remove `id`"));
        }
        c.projectile(p, id, "", 0)?;
    }
    let mut weapons = Vec::with_capacity(src.weapons.len());
    let mut ids = BTreeSet::new();
    for (id, w) in &src.weapons {
        c.file = weapon_file(id);
        if !valid_id(id) {
            return Err(FormatError::invalid(MOD_FILE, "weapons", format!("invalid weapon id {id:?}")));
        }
        if !ids.insert(id.as_str()) {
            return Err(FormatError::invalid(MOD_FILE, "weapons", format!("weapon id {id:?} is listed twice")));
        }
        weapons.push(c.weapon(id, w)?);
    }
    Ok(rt::ModDef {
        name: src.name.clone(),
        worm: worm(&src.worm),
        rope: rope(&src.rope),
        weapons,
        projectiles: c.out,
        restrictions: src.restrictions.clone(),
    })
}

fn worm(w: &Worm) -> rt::WormParams {
    rt::WormParams {
        angle_speed: w.unused_angle_speed,
        ground_speed: w.ground_acceleration,
        air_speed: w.air_acceleration,
        gravity: w.gravity,
        jump_force: w.jump_speed,
        air_friction: w.air_drag,
        ground_friction: w.unused_ground_friction,
    }
}

fn rope(r: &Rope) -> rt::RopeParams {
    rt::RopeParams { max_length: r.max_length, rest_length: r.rest_length, strength: r.strength }
}

impl Compiler<'_> {
    fn err(&self, path: &str, message: impl Into<String>) -> FormatError {
        FormatError::invalid(&self.file, path, message)
    }

    fn weapon(&mut self, id: &str, w: &Weapon) -> Result<rt::WeaponDef, FormatError> {
        let kind = match &w.fire {
            Fire::Projectile { class, recoil, sound, shot } => rt::WeaponKind::Projectile {
                class: weapon_class(*class),
                recoil: *recoil,
                sound: sound.clone(),
                spawn: self.spawn(shot, id, "fire.shot", 0)?,
            },
            Fire::Beam { recoil, colour, hole_size, worm_damage, length, width, width_growth, split_damage_over_width } => {
                rt::WeaponKind::Beam {
                    recoil: *recoil,
                    beam: rt::BeamDef {
                        colour: colour.0,
                        damage: *hole_size,
                        player_damage: *worm_damage,
                        length: *length,
                        init_width: *width,
                        width_increase: *width_growth,
                        distribute_damage_over_width: *split_damage_over_width,
                    },
                }
            }
            Fire::Special { kind, thrust } => rt::WeaponKind::Special {
                special: match kind {
                    SpecialKind::Off => rt::SpecialKind::None,
                    SpecialKind::Jetpack => rt::SpecialKind::Jetpack,
                    SpecialKind::Unknown(i) => rt::SpecialKind::Other(*i),
                },
                thrust: *thrust,
            },
        };
        Ok(rt::WeaponDef {
            id: id.to_string(),
            name: w.name.clone(),
            recharge: w.recharge,
            drain: w.drain,
            rof: w.between_shots_s,
            laser_sight: w.laser_sight,
            kind,
        })
    }

    fn spawn<C: SpawnContext>(
        &mut self,
        s: &Spawn<C>,
        default_key: &str,
        path: &str,
        depth: usize,
    ) -> Result<rt::SpawnDef, FormatError> {
        let projectile = match (&s.shared, &s.projectile) {
            (Some(id), None) => {
                if !self.src.shared.contains_key(id) {
                    return Err(self.err(&at(path, "shared"), format!("no shared projectile {id:?} ({})", shared_file(id))));
                }
                id.clone()
            }
            (None, Some(p)) => {
                let key = p.id.clone().unwrap_or_else(|| default_key.to_string());
                self.projectile(p, &key, &at(path, "projectile"), depth + 1)?;
                key
            }
            (Some(_), Some(_)) => return Err(self.err(path, "a spawn takes either `shared` or `projectile`, not both")),
            (None, None) => return Err(self.err(path, "a spawn needs `shared` or `projectile`")),
        };
        Ok(rt::SpawnDef {
            projectile,
            amount: s.amount,
            speed: s.speed,
            speed_var: s.speed_var,
            spread: s.spread,
            use_angle: s.relative_to_parent,
            angle: s.angle,
            use_parent_velocity_for_spread: s.parent_velocity_as_direction,
            parent_vel_spread_factor: s.parent_velocity_direction_scale,
            use_special11_vec_for_speed_var: s.speed_var_diagonal,
            use_random_rot: s.random_rotation,
            add_parent_vel: s.inherit_velocity,
            parent_vel_factor: s.inherit_velocity_factor,
        })
    }

    fn projectile(&mut self, p: &Projectile, key: &str, path: &str, depth: usize) -> Result<(), FormatError> {
        if depth > MAX_INLINE_DEPTH {
            return Err(self.err(path, format!("projectiles nested deeper than {MAX_INLINE_DEPTH}")));
        }
        if !valid_id(key) {
            return Err(self.err(&at(path, "id"), format!("invalid projectile id {key:?}")));
        }
        if !self.taken.insert(key.to_string()) {
            return Err(self.err(path, format!("projectile id {key:?} is used twice; give one an `id`")));
        }
        let timed = p.timer.as_ref().filter(|t| t.after_s > 0.0);
        if let Some(t) = &p.timer
            && t.after_s <= 0.0
            && !t.effects.is_empty()
        {
            return Err(self.err(&at(path, "timer.effects"), "a timer's effects need after_s > 0 (OLX ignores them otherwise)"));
        }
        let (hit, hit_spawn) = self.effects(&p.hit_terrain, &at(path, "hit_terrain"))?;
        let (player_hit, worm_spawn) = self.effects(&p.hit_worm, &at(path, "hit_worm"))?;
        let timer_action = match timed {
            Some(t) => Some(self.effects(&t.effects, &at(path, "timer.effects"))?),
            None => None,
        };
        let named: BTreeSet<&str> = [hit_spawn.as_deref(), worm_spawn.as_deref(), timer_action.as_ref().and_then(|(_, s)| s.as_deref())]
            .into_iter()
            .flatten()
            .collect();
        if named.len() > 1 {
            let list: Vec<&str> = named.iter().copied().collect();
            return Err(self.err(path, format!("this runtime gives a projectile's events one spawn, but they name {}", list.join(", "))));
        }
        if p.spawns.len() > 1 {
            return Err(self.err(&at(path, "spawns"), "this runtime takes at most one named spawn per projectile"));
        }
        if let Some(name) = named.first()
            && !p.spawns.contains_key(*name)
        {
            let have: Vec<&str> = p.spawns.keys().map(String::as_str).collect();
            return Err(self.err(path, format!("no spawn named {name:?} (spawns: {})", have.join(", "))));
        }
        let event_spawn = match p.spawns.iter().next() {
            Some((name, s)) => Some(self.spawn(s, &format!("{key}.spawn"), &at(path, &format!("spawns[{name:?}]")), depth)?),
            None => None,
        };
        let trail = match &p.trail {
            Trail::Projectile { every_s, spawn } => rt::TrailDef {
                kind: rt::TrailKind::Projectile,
                delay: *every_s,
                spawn: Some(self.spawn(spawn, &format!("{key}.trail"), &at(path, "trail.spawn"), depth)?),
            },
            other => rt::TrailDef {
                kind: match other {
                    Trail::Off => rt::TrailKind::None,
                    Trail::Smoke => rt::TrailKind::Smoke,
                    Trail::ChemSmoke => rt::TrailKind::ChemSmoke,
                    Trail::Doomsday => rt::TrailKind::Doomsday,
                    Trail::Explosive => rt::TrailKind::Explosive,
                    Trail::Unknown(i) => rt::TrailKind::Other(*i),
                    Trail::Projectile { .. } => unreachable!("matched above"),
                },
                delay: 0.1,
                spawn: None,
            },
        };
        let gfx = self.gfx(&p.gfx, &at(path, "gfx"))?;
        let (width, height) = p.size.unwrap_or(match gfx {
            rt::ProjectileGfx::Pixel { .. } => (2, 2),
            rt::ProjectileGfx::Image { .. } => (4, 4),
        });
        let timer = match &p.timer {
            Some(t) => rt::TimerDef { time: t.after_s, time_var: t.variation_s, action: timer_action.map(|(a, _)| a) },
            None => rt::TimerDef { time: 0.0, time_var: 0.0, action: None },
        };
        self.out.insert(
            key.to_string(),
            rt::ProjectileDef {
                gfx,
                gravity: p.gravity,
                dampening: p.velocity_factor,
                width,
                height,
                timer,
                trail,
                hit,
                player_hit,
                event_spawn,
            },
        );
        Ok(())
    }

    fn gfx(&self, g: &Gfx, path: &str) -> Result<rt::ProjectileGfx, FormatError> {
        Ok(match g {
            Gfx::Pixels(cs) => {
                if cs.len() > MAX_COLOURS {
                    return Err(self.err(path, format!("more than {MAX_COLOURS} colours")));
                }
                rt::ProjectileGfx::Pixel { colours: cs.iter().map(|c| c.0).collect() }
            }
            Gfx::Image {
                file,
                rotating,
                rotation_step_deg,
                rotation_speed,
                frames_follow_heading,
                frames_follow_heading_mirrored,
                heading_frames,
                animation,
            } => rt::ProjectileGfx::Image {
                file: file.clone(),
                rotating: *rotating,
                rot_increment: *rotation_step_deg,
                rot_speed: *rotation_speed,
                use_angle: *frames_follow_heading,
                use_spec_angle: *frames_follow_heading_mirrored,
                angle_images: *heading_frames,
                animation: animation.as_ref().map(|a| rt::Animation {
                    rate: a.fps,
                    kind: match a.mode {
                        AnimMode::Once => rt::AnimKind::Once,
                        AnimMode::Loop => rt::AnimKind::Loop,
                        AnimMode::PingPong => rt::AnimKind::PingPong,
                        AnimMode::Unknown(i) => rt::AnimKind::Other(i),
                    },
                }),
            },
        })
    }

    /// One event's effect list as LX56's single action, and the spawn it names.
    fn effects(&self, list: &[Effect], path: &str) -> Result<(rt::ActionDef, Option<String>), FormatError> {
        if list.len() > MAX_EFFECTS {
            return Err(self.err(path, format!("more than {MAX_EFFECTS} effects")));
        }
        let mut action = rt::ActionDef::olx_default(rt::ActionKind::Nothing);
        let (mut kind_set, mut spawn) = (false, None::<String>);
        for (i, e) in list.iter().enumerate() {
            let here = format!("{path}[{i}]");
            if let Effect::Spawn(name) = e {
                if spawn.is_some() {
                    return Err(self.err(&here, "one Spawn per event is supported by this runtime"));
                }
                spawn = Some(name.clone());
                continue;
            }
            if kind_set {
                return Err(self.err(&here, "one effect per event besides Spawn is supported by this runtime"));
            }
            kind_set = true;
            apply(&mut action, e);
        }
        action.spawns_projectiles = spawn.is_some();
        Ok((action, spawn))
    }
}

/// Sets `a.kind` and the fields that kind uses (`mod-fields.md` §12).
fn apply(a: &mut rt::ActionDef, e: &Effect) {
    use rt::ActionKind as K;
    a.kind = match e {
        Effect::Explode { damage, shake, sound } => {
            (a.damage, a.shake, a.sound) = (*damage, *shake, sound.clone());
            K::Explode
        }
        Effect::Bounce { factor, explode } => {
            (a.bounce_coeff, a.bounce_explode) = (*factor, *explode);
            K::Bounce
        }
        Effect::Injure { damage } => {
            a.damage = *damage;
            K::Injure
        }
        Effect::InjureWorm { damage } => {
            a.damage = *damage;
            K::InjureWorm
        }
        Effect::Carve { size } => {
            a.damage = *size;
            K::Carve
        }
        Effect::Dirt { shake } => {
            a.shake = *shake;
            K::Dirt
        }
        Effect::GreenDirt { shake } => {
            a.shake = *shake;
            K::GreenDirt
        }
        Effect::Sound { file } => {
            a.sound = file.clone();
            K::PlaySound
        }
        Effect::Disappear => K::Disappear,
        Effect::Remove => K::Disappear2,
        Effect::GoThrough => K::GoThrough,
        Effect::InjureProjectile => K::InjureProj,
        Effect::ChangeRadius => K::ChangeRadius,
        Effect::SetSpeed => K::OverwriteOwnSpeed,
        Effect::MultiplySpeed => K::MultiplyOwnSpeed,
        Effect::AddSpeed => K::DiffOwnSpeed,
        Effect::SetTargetSpeed => K::OverwriteTargetSpeed,
        Effect::MultiplyTargetSpeed => K::MultiplyTargetSpeed,
        Effect::AddTargetSpeed => K::DiffTargetSpeed,
        Effect::HomeOnNearestWorm => K::HeadingToNextWorm,
        Effect::HomeOnOwner => K::HeadingToOwner,
        Effect::HomeOnNearestOtherWorm => K::HeadingToNextOtherWorm,
        Effect::HomeOnNearestEnemy => K::HeadingToNextEnemyWorm,
        Effect::HomeOnNearestTeamMate => K::HeadingToNextTeamMate,
        Effect::PullTargetsToUs => K::HeadTargetToUs,
        Effect::Unknown(i) => K::Other(*i),
        Effect::Spawn(_) => unreachable!("handled by the caller"),
    };
}

fn weapon_class(c: WeaponClass) -> rt::WeaponClass {
    match c {
        WeaponClass::Automatic => rt::WeaponClass::Automatic,
        WeaponClass::PowerGun => rt::WeaponClass::PowerGun,
        WeaponClass::Grenade => rt::WeaponClass::Grenade,
        WeaponClass::Missile => rt::WeaponClass::Missile,
        WeaponClass::CloseRange => rt::WeaponClass::CloseRange,
        WeaponClass::Unknown(i) => rt::WeaponClass::Other(i),
    }
}

/// The shared ids each shared projectile's tree references.
fn shared_refs(p: &Projectile, out: &mut BTreeSet<String>, depth: usize) {
    if depth > MAX_INLINE_DEPTH {
        return; // compile reports the depth with its path
    }
    let mut visit = |shared: &Option<String>, inline: &Option<Box<Projectile>>| {
        if let Some(id) = shared {
            out.insert(id.clone());
        }
        if let Some(child) = inline {
            shared_refs(child, out, depth + 1);
        }
    };
    for s in p.spawns.values() {
        visit(&s.shared, &s.projectile);
    }
    if let Trail::Projectile { spawn, .. } = &p.trail {
        visit(&spawn.shared, &spawn.projectile);
    }
}

fn check_cycles(src: &ModSource) -> Result<(), FormatError> {
    let graph: BTreeMap<&str, BTreeSet<String>> = src
        .shared
        .iter()
        .map(|(id, p)| {
            let mut refs = BTreeSet::new();
            shared_refs(p, &mut refs, 0);
            (id.as_str(), refs)
        })
        .collect();
    // Iterative depth-first search: 0 = new, 1 = on the path, 2 = done.
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    for &start in graph.keys() {
        if state.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut path: Vec<(&str, Vec<&str>)> = vec![(start, graph[start].iter().map(String::as_str).collect())];
        state.insert(start, 1);
        loop {
            let Some((node, next)) = path.last_mut() else { break };
            let node = *node;
            let child = next.pop(); // the borrow of `path` ends here
            match child {
                Some(child) if graph.contains_key(child) => match state.get(child).copied().unwrap_or(0) {
                    0 => {
                        state.insert(child, 1);
                        let kids = graph[child].iter().map(String::as_str).collect();
                        path.push((child, kids));
                    }
                    1 => {
                        let mut cycle: Vec<&str> = path.iter().map(|(n, _)| *n).skip_while(|n| *n != child).collect();
                        cycle.push(child);
                        return Err(FormatError::invalid(
                            &shared_file(node),
                            "",
                            format!("shared projectiles form a cycle: {}", cycle.join(" -> ")),
                        ));
                    }
                    _ => {}
                },
                Some(_) => {} // an unknown id: compile reports it with its path
                None => {
                    state.insert(node, 2);
                    path.pop();
                }
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo nextest run -p olr-format`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/olr-format
git commit -m "olr-format: compile mods to ModDef"
```

---

### Task 4: Lifting `ModDef` back to the authoring form

**Files:**
- Create: `crates/olr-format/src/mods/lift.rs`
- Modify: `crates/olr-format/src/mods/mod.rs` (`mod lift; pub use
  lift::{lift_mod, LIFTED_SPAWN};`)

**Interfaces:**
- Consumes: Tasks 2–3 (types, `MAX_INLINE_DEPTH`).
- Produces:
  - `pub fn lift_mod(def: &olr_data::mod_def::ModDef) -> Result<ModSource,
    LiftError>`.
  - `pub const LIFTED_SPAWN: &str = "spawn";`: the name a lifted
    projectile's event spawn gets.

**Rules:**
- **Inline or shared.** A projectile key referenced exactly once (by a
  weapon's shot, or as an event or trail child) is lifted inline at its
  reference. Every other key (0 or 2+ references) becomes
  `shared[key]`.
- **`id` only on mismatch.** An inline projectile gets `id: Some(key)`
  only when the key differs from the one compile would derive there.
- **Spawns:** `event_spawn` becomes `spawns[LIFTED_SPAWN]`. Every action
  with `spawns_projectiles` lists `Effect::Spawn(LIFTED_SPAWN)` after its
  kind's effect.
- **Unused fields must be defaults.** An `ActionDef` field that its kind
  does not use (`mod-fields.md` §12) must hold its `olx_default` value;
  otherwise it is a `LiftError` naming the field.

  | Kind | Uses |
  |---|---|
  | `Explode` | `damage`, `shake`, `sound` |
  | `Bounce` | `bounce_coeff`, `bounce_explode` |
  | `Injure`, `InjureWorm`, `Carve` | `damage` |
  | `Dirt`, `GreenDirt` | `shake` |
  | `PlaySound` | `sound` |
  | the others | nothing |
- **Timer:**
  - `time` and `time_var` both 0 (by bits) with no action → no timer;
  - `time > 0` → a `Timer` with the action's effects (the action must be
    `Some`);
  - otherwise → a `Timer { after_s: time, variation_s: time_var,
    effects: [] }` (the action must be `None`).
- **Trail:** a non-`Projectile` trail must have `delay == 0.1f32` (by
  bits) and `spawn: None`; a `Projectile` trail must have `spawn: Some`.
- **Size:** `size` is `None` when it equals the gfx default.
- **Errors, not panics:** a missing key, a cycle, nesting deeper than
  `MAX_INLINE_DEPTH`, or a projectile that cannot be reached (a cycle of
  single references) is a `LiftError`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::compile_mod;
    use olr_data::image::Rgba;
    use olr_data::mod_def::*;

    /// Like `olr-pack`'s sample: a rocket spawning bits, and bits shared by
    /// a second weapon.
    fn sample() -> ModDef {
        let bits = ProjectileDef {
            gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(200, 0, 0)] },
            gravity: None,
            dampening: 1.0,
            width: 2,
            height: 2,
            timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
            trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
            hit: ActionDef::olx_default(ActionKind::Bounce),
            player_hit: ActionDef { damage: 3, ..ActionDef::olx_default(ActionKind::Injure) },
            event_spawn: None,
        };
        let rocket = ProjectileDef {
            gfx: ProjectileGfx::Image {
                file: "gfx/missile.png".into(),
                rotating: false,
                rot_increment: 0,
                rot_speed: 0,
                use_angle: true,
                use_spec_angle: false,
                angle_images: 16,
                animation: None,
            },
            width: 4,
            height: 4,
            hit: ActionDef { damage: 10, spawns_projectiles: true, sound: Some("sfx/boom.ogg".into()), ..ActionDef::olx_default(ActionKind::Explode) },
            event_spawn: Some(SpawnDef { amount: 5, ..SpawnDef::olx_default("bits".into()) }),
            ..bits.clone()
        };
        let weapon = |id: &str, proj: &str| WeaponDef {
            id: id.into(),
            name: id.to_uppercase(),
            recharge: 0.5,
            drain: 30.0,
            rof: 0.25,
            laser_sight: false,
            kind: WeaponKind::Projectile {
                class: WeaponClass::Missile,
                recoil: 5,
                sound: None,
                spawn: SpawnDef { amount: 1, speed: 100, use_angle: true, use_random_rot: true, add_parent_vel: true, ..SpawnDef::olx_default(proj.into()) },
            },
        };
        ModDef {
            name: "Test".into(),
            worm: WormParams { angle_speed: 150.0, ground_speed: 8.0, air_speed: 4.0, gravity: 100.0, jump_force: -65.0, air_friction: 0.0, ground_friction: 0.4 },
            rope: RopeParams { max_length: 300, rest_length: 27, strength: 4.5 },
            weapons: vec![weapon("rocket", "rocket"), weapon("pellet", "bits")],
            projectiles: [("bits".to_string(), bits), ("rocket".to_string(), rocket)].into_iter().collect(),
            restrictions: Default::default(),
        }
    }

    #[test]
    fn lift_then_compile_is_the_identity() {
        let def = sample();
        let src = lift_mod(&def).unwrap();
        assert_eq!(compile_mod(&src).unwrap(), def);
    }

    #[test]
    fn single_references_inline_and_multiple_ones_share() {
        let src = lift_mod(&sample()).unwrap();
        assert_eq!(src.shared.keys().collect::<Vec<_>>(), ["bits"]);
        let Fire::Projectile { shot, .. } = &src.weapons[0].1.fire else { panic!() };
        let rocket = shot.projectile.as_ref().unwrap();
        assert_eq!(rocket.id, None, "\"rocket\" is the derived key");
        assert_eq!(rocket.spawns[LIFTED_SPAWN].shared.as_deref(), Some("bits"));
        assert_eq!(rocket.hit_terrain, vec![
            Effect::Explode { damage: 10, shake: 0, sound: Some("sfx/boom.ogg".into()) },
            Effect::Spawn(LIFTED_SPAWN.into()),
        ]);
        assert_eq!(rocket.size, None);
    }

    #[test]
    fn an_unused_action_field_is_a_lift_error() {
        let mut def = sample();
        def.projectiles.get_mut("bits").unwrap().hit.damage = 7; // Bounce does not use damage
        let e = lift_mod(&def).unwrap_err();
        assert!(e.to_string().contains("damage"), "{e}");
    }

    #[test]
    fn a_missing_key_is_a_lift_error() {
        let mut def = sample();
        def.projectiles.remove("bits");
        assert!(lift_mod(&def).is_err());
    }

    #[test]
    fn a_cycle_of_single_references_is_a_lift_error() {
        let mut def = sample();
        let mut a = def.projectiles["bits"].clone();
        a.event_spawn = Some(SpawnDef::olx_default("b".into()));
        let mut b = def.projectiles["bits"].clone();
        b.event_spawn = Some(SpawnDef::olx_default("a".into()));
        def.projectiles.insert("a".into(), a);
        def.projectiles.insert("b".into(), b);
        assert!(lift_mod(&def).is_err());
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p olr-format mods::lift`
Expected: the build fails (`lift_mod` is missing).

- [ ] **Step 3: Implement `lift.rs`**

```rust
//! Runtime `ModDef` -> authoring mod: how imports become authoring files.
//! The inverse of `compile_mod` on everything today's importers produce.

use std::collections::{BTreeMap, BTreeSet};

use olr_data::mod_def as rt;

use super::compile::MAX_INLINE_DEPTH;
use super::spawn::{Spawn, SpawnContext};
use super::types::*;
use super::{ModSource, Rope, Worm};
use crate::{Colour, LiftError};

/// The name of a lifted projectile's event spawn.
pub const LIFTED_SPAWN: &str = "spawn";

struct Lifter<'a> {
    def: &'a rt::ModDef,
    shared: BTreeSet<&'a str>,
    emitted: BTreeSet<&'a str>,
    stack: Vec<&'a str>,
}

fn err(item: impl Into<String>, message: impl Into<String>) -> LiftError {
    LiftError { item: item.into(), message: message.into() }
}

pub fn lift_mod(def: &rt::ModDef) -> Result<ModSource, LiftError> {
    let mut refs: BTreeMap<&str, usize> = def.projectiles.keys().map(|k| (k.as_str(), 0)).collect();
    for w in &def.weapons {
        if let Some(p) = w.projectile() {
            *refs.entry(p).or_default() += 1;
        }
    }
    for p in def.projectiles.values() {
        for c in p.children() {
            *refs.entry(c).or_default() += 1;
        }
    }
    let shared = refs
        .iter()
        .filter(|(k, n)| **n != 1 && def.projectiles.contains_key(**k))
        .map(|(k, _)| *k)
        .collect();
    let mut l = Lifter { def, shared, emitted: BTreeSet::new(), stack: Vec::new() };
    let mut weapons = Vec::with_capacity(def.weapons.len());
    for w in &def.weapons {
        weapons.push((w.id.clone(), l.weapon(w)?));
    }
    let mut shared_out = BTreeMap::new();
    let ids: Vec<&str> = l.shared.iter().copied().collect();
    for id in ids {
        shared_out.insert(id.to_string(), l.projectile(id)?);
    }
    if l.emitted.len() != def.projectiles.len() {
        let lost: Vec<&str> = def.projectiles.keys().map(String::as_str).filter(|k| !l.emitted.contains(k)).collect();
        return Err(err("mod", format!("projectiles no weapon reaches, in a cycle: {}", lost.join(", "))));
    }
    Ok(ModSource {
        name: def.name.clone(),
        notes: String::new(),
        worm: Worm {
            ground_acceleration: def.worm.ground_speed,
            air_acceleration: def.worm.air_speed,
            gravity: def.worm.gravity,
            jump_speed: def.worm.jump_force,
            air_drag: def.worm.air_friction,
            unused_angle_speed: def.worm.angle_speed,
            unused_ground_friction: def.worm.ground_friction,
        },
        rope: Rope { max_length: def.rope.max_length, rest_length: def.rope.rest_length, strength: def.rope.strength },
        weapons,
        shared: shared_out,
        restrictions: def.restrictions.clone(),
    })
}

impl<'a> Lifter<'a> {
    fn weapon(&mut self, w: &'a rt::WeaponDef) -> Result<Weapon, LiftError> {
        let fire = match &w.kind {
            rt::WeaponKind::Projectile { class, recoil, sound, spawn } => Fire::Projectile {
                class: match class {
                    rt::WeaponClass::Automatic => WeaponClass::Automatic,
                    rt::WeaponClass::PowerGun => WeaponClass::PowerGun,
                    rt::WeaponClass::Grenade => WeaponClass::Grenade,
                    rt::WeaponClass::Missile => WeaponClass::Missile,
                    rt::WeaponClass::CloseRange => WeaponClass::CloseRange,
                    rt::WeaponClass::Other(i) => WeaponClass::Unknown(*i),
                },
                recoil: *recoil,
                sound: sound.clone(),
                shot: self.spawn(spawn, &w.id)?,
            },
            rt::WeaponKind::Beam { recoil, beam } => Fire::Beam {
                recoil: *recoil,
                colour: Colour(beam.colour),
                hole_size: beam.damage,
                worm_damage: beam.player_damage,
                length: beam.length,
                width: beam.init_width,
                width_growth: beam.width_increase,
                split_damage_over_width: beam.distribute_damage_over_width,
            },
            rt::WeaponKind::Special { special, thrust } => Fire::Special {
                kind: match special {
                    rt::SpecialKind::None => SpecialKind::Off,
                    rt::SpecialKind::Jetpack => SpecialKind::Jetpack,
                    rt::SpecialKind::Other(i) => SpecialKind::Unknown(*i),
                },
                thrust: *thrust,
            },
        };
        Ok(Weapon {
            name: w.name.clone(),
            notes: String::new(),
            recharge: w.recharge,
            drain: w.drain,
            between_shots_s: w.rof,
            laser_sight: w.laser_sight,
            fire,
        })
    }

    fn spawn<C: SpawnContext>(&mut self, s: &'a rt::SpawnDef, default_key: &str) -> Result<Spawn<C>, LiftError> {
        let (shared, projectile) = if self.shared.contains(s.projectile.as_str()) {
            (Some(s.projectile.clone()), None)
        } else {
            let mut p = self.projectile(&s.projectile)?;
            if s.projectile != default_key {
                p.id = Some(s.projectile.clone());
            }
            (None, Some(Box::new(p)))
        };
        Ok(Spawn {
            shared,
            projectile,
            amount: s.amount,
            speed: s.speed,
            speed_var: s.speed_var,
            spread: s.spread,
            relative_to_parent: s.use_angle,
            angle: s.angle,
            parent_velocity_as_direction: s.use_parent_velocity_for_spread,
            parent_velocity_direction_scale: s.parent_vel_spread_factor,
            speed_var_diagonal: s.use_special11_vec_for_speed_var,
            random_rotation: s.use_random_rot,
            inherit_velocity: s.add_parent_vel,
            inherit_velocity_factor: s.parent_vel_factor,
            context: Default::default(),
        })
    }

    fn projectile(&mut self, key: &'a str) -> Result<Projectile, LiftError> {
        let item = || format!("projectile {key:?}");
        let p = self.def.projectiles.get(key).ok_or_else(|| err(item(), "no such projectile"))?;
        if self.stack.contains(&key) {
            return Err(err(item(), "spawns one of its own ancestors"));
        }
        if self.stack.len() > MAX_INLINE_DEPTH {
            return Err(err(item(), format!("nested deeper than {MAX_INLINE_DEPTH}")));
        }
        let key = self.def.projectiles.get_key_value(key).map(|(k, _)| k.as_str()).unwrap_or(key);
        self.emitted.insert(key);
        self.stack.push(key);
        let spawns_any = p.hit.spawns_projectiles
            || p.player_hit.spawns_projectiles
            || p.timer.action.as_ref().is_some_and(|a| a.spawns_projectiles);
        if spawns_any && p.event_spawn.is_none() {
            return Err(err(item(), "an action spawns projectiles but there is no event spawn"));
        }
        let hit_terrain = action(&p.hit).map_err(|m| err(item(), format!("hit: {m}")))?;
        let hit_worm = action(&p.player_hit).map_err(|m| err(item(), format!("player_hit: {m}")))?;
        let zero = |v: f32| v.to_bits() == 0f32.to_bits();
        let timer = match (&p.timer.action, p.timer.time > 0.0) {
            (None, _) if zero(p.timer.time) && zero(p.timer.time_var) => None,
            (Some(a), true) => Some(Timer {
                after_s: p.timer.time,
                variation_s: p.timer.time_var,
                effects: action(a).map_err(|m| err(item(), format!("timer: {m}")))?,
            }),
            (None, false) => Some(Timer { after_s: p.timer.time, variation_s: p.timer.time_var, effects: Vec::new() }),
            _ => return Err(err(item(), "a timer has an action exactly when its time is above 0")),
        };
        let mut spawns = BTreeMap::new();
        if let Some(s) = &p.event_spawn {
            spawns.insert(LIFTED_SPAWN.to_string(), self.spawn(s, &format!("{key}.spawn"))?);
        }
        let trail = match (p.trail.kind, &p.trail.spawn) {
            (rt::TrailKind::Projectile, Some(s)) => Trail::Projectile { every_s: p.trail.delay, spawn: self.spawn(s, &format!("{key}.trail"))? },
            (rt::TrailKind::Projectile, None) => return Err(err(item(), "a projectile trail without a spawn")),
            (kind, None) if p.trail.delay.to_bits() == 0.1f32.to_bits() => match kind {
                rt::TrailKind::None => Trail::Off,
                rt::TrailKind::Smoke => Trail::Smoke,
                rt::TrailKind::ChemSmoke => Trail::ChemSmoke,
                rt::TrailKind::Doomsday => Trail::Doomsday,
                rt::TrailKind::Explosive => Trail::Explosive,
                rt::TrailKind::Other(i) => Trail::Unknown(i),
                rt::TrailKind::Projectile => unreachable!("matched above"),
            },
            _ => return Err(err(item(), "only a projectile trail has a spawn or a delay other than 0.1")),
        };
        let gfx = match &p.gfx {
            rt::ProjectileGfx::Pixel { colours } => Gfx::Pixels(colours.iter().map(|c| Colour(*c)).collect()),
            rt::ProjectileGfx::Image { file, rotating, rot_increment, rot_speed, use_angle, use_spec_angle, angle_images, animation } => Gfx::Image {
                file: file.clone(),
                rotating: *rotating,
                rotation_step_deg: *rot_increment,
                rotation_speed: *rot_speed,
                frames_follow_heading: *use_angle,
                frames_follow_heading_mirrored: *use_spec_angle,
                heading_frames: *angle_images,
                animation: animation.map(|a| Animation {
                    fps: a.rate,
                    mode: match a.kind {
                        rt::AnimKind::Once => AnimMode::Once,
                        rt::AnimKind::Loop => AnimMode::Loop,
                        rt::AnimKind::PingPong => AnimMode::PingPong,
                        rt::AnimKind::Other(i) => AnimMode::Unknown(i),
                    },
                }),
            },
        };
        let default_size = match gfx {
            Gfx::Pixels(_) => (2, 2),
            Gfx::Image { .. } => (4, 4),
        };
        self.stack.pop();
        Ok(Projectile {
            id: None,
            notes: String::new(),
            size: ((p.width, p.height) != default_size).then_some((p.width, p.height)),
            gfx,
            gravity: p.gravity,
            velocity_factor: p.dampening,
            timer,
            trail,
            hit_terrain,
            hit_worm,
            spawns,
        })
    }
}

/// An `ActionDef` as an effect list; `Err` names a field its kind does not use
/// that holds a value.
fn action(a: &rt::ActionDef) -> Result<Vec<Effect>, String> {
    use rt::ActionKind as K;
    let (damage, shake, sound, bounce) = match a.kind {
        K::Explode => (true, true, true, false),
        K::Bounce => (false, false, false, true),
        K::Injure | K::InjureWorm | K::Carve => (true, false, false, false),
        K::Dirt | K::GreenDirt => (false, true, false, false),
        K::PlaySound => (false, false, true, false),
        _ => (false, false, false, false),
    };
    let d = rt::ActionDef::olx_default(a.kind);
    let unused = |field: &str| Err(format!("{:?} never uses `{field}`, which is set", a.kind));
    if !damage && a.damage != d.damage {
        return unused("damage");
    }
    if !shake && a.shake != d.shake {
        return unused("shake");
    }
    if !sound && a.sound != d.sound {
        return unused("sound");
    }
    if !bounce && (a.bounce_coeff.to_bits() != d.bounce_coeff.to_bits() || a.bounce_explode != d.bounce_explode) {
        return unused("bounce_coeff or bounce_explode");
    }
    let effect = match a.kind {
        K::Nothing => None,
        K::Explode => Some(Effect::Explode { damage: a.damage, shake: a.shake, sound: a.sound.clone() }),
        K::Bounce => Some(Effect::Bounce { factor: a.bounce_coeff, explode: a.bounce_explode }),
        K::Injure => Some(Effect::Injure { damage: a.damage }),
        K::InjureWorm => Some(Effect::InjureWorm { damage: a.damage }),
        K::Carve => Some(Effect::Carve { size: a.damage }),
        K::Dirt => Some(Effect::Dirt { shake: a.shake }),
        K::GreenDirt => Some(Effect::GreenDirt { shake: a.shake }),
        K::PlaySound => Some(Effect::Sound { file: a.sound.clone() }),
        K::Disappear => Some(Effect::Disappear),
        K::Disappear2 => Some(Effect::Remove),
        K::GoThrough => Some(Effect::GoThrough),
        K::InjureProj => Some(Effect::InjureProjectile),
        K::ChangeRadius => Some(Effect::ChangeRadius),
        K::OverwriteOwnSpeed => Some(Effect::SetSpeed),
        K::MultiplyOwnSpeed => Some(Effect::MultiplySpeed),
        K::DiffOwnSpeed => Some(Effect::AddSpeed),
        K::OverwriteTargetSpeed => Some(Effect::SetTargetSpeed),
        K::MultiplyTargetSpeed => Some(Effect::MultiplyTargetSpeed),
        K::DiffTargetSpeed => Some(Effect::AddTargetSpeed),
        K::HeadingToNextWorm => Some(Effect::HomeOnNearestWorm),
        K::HeadingToOwner => Some(Effect::HomeOnOwner),
        K::HeadingToNextOtherWorm => Some(Effect::HomeOnNearestOtherWorm),
        K::HeadingToNextEnemyWorm => Some(Effect::HomeOnNearestEnemy),
        K::HeadingToNextTeamMate => Some(Effect::HomeOnNearestTeamMate),
        K::HeadTargetToUs => Some(Effect::PullTargetsToUs),
        K::Other(i) => Some(Effect::Unknown(i)),
    };
    let mut v: Vec<Effect> = effect.into_iter().collect();
    if a.spawns_projectiles {
        v.push(Effect::Spawn(LIFTED_SPAWN.into()));
    }
    Ok(v)
}
```

Note: `self.stack.pop()` must also run on the error paths after the push.
Either restructure so the body after `push` is a closure whose result is
matched after the `pop`, or move the body into `fn projectile_body`. An
early `?` that skips the pop only leaves a stale stack on the error path,
but keep it correct anyway.

- [ ] **Step 4: Run the tests, then the repository check**

Run: `cargo nextest run -p olr-format`
Expected: PASS.

Run: `cargo xtask check` (first of this plan's two).
Expected: `check passed`. If `fmt` or `clippy` complain, fix them here.

- [ ] **Step 5: Commit**

```bash
git add crates/olr-format
git commit -m "olr-format: lift ModDef to the authoring form"
```

---

### Task 5: The pack's mod files

**Files:**
- Create: `crates/olr-format/src/mods/files.rs`
- Modify: `crates/olr-format/src/mods/mod.rs` (`mod files; pub use
  files::{read_mod_files, write_mod_files, RESTRICTIONS_FILE};`)
- Test: `crates/olr-format/tests/hostile.rs`

**Interfaces:**
- Consumes: Tasks 1–3 (`ron_io`, types, `valid_id`, `weapon_file`,
  `shared_file`, `MOD_FILE`).
- Produces:

  ```rust
  pub fn write_mod_files(sink: &mut dyn ContentSink, m: &ModSource) -> Result<(), FormatError>
  pub fn read_mod_files(src: &dyn ContentSource) -> Result<ModSource, FormatError>
  pub const RESTRICTIONS_FILE: &str = "restrictions.ron";
  ```

**Layout and rules:**
- **`mod.ron`** is `(name, notes?, worm?, rope?, weapons: [ids])`; the
  worm and rope are left out when `is_default()`.
- **`weapons/<id>.ron`** holds one `Weapon` for each listed id.
- **`projectiles/<id>.ron`** holds one shared `Projectile`, with no
  nested folders.
- **`restrictions.ron`** is written only when the presets are not empty.
- **Other files** (`pack.ron`, `gfx/`, `sfx/`, any other path) are not
  this module's and are ignored when reading.
- **Errors, each naming the file:**
  - a listed weapon without its file;
  - a `weapons/*.ron` that `mod.ron` does not list;
  - a duplicate or invalid id;
  - a non-`.ron` file or a nested folder in `projectiles/`.

- [ ] **Step 1: Write the failing tests**

In `files.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::{Fire, Gfx, Projectile, ShotSpawn, Weapon, WeaponClass};
    use olr_vfs::MemFs;

    fn sample() -> ModSource {
        let shot = ShotSpawn { speed: 100, projectile: Some(Box::new(Projectile::new(Gfx::Pixels(vec![])))), ..ShotSpawn::default() };
        let w = Weapon {
            name: "Gun".into(),
            notes: String::new(),
            recharge: 0.5,
            drain: 30.0,
            between_shots_s: 0.25,
            laser_sight: false,
            fire: Fire::Projectile { class: WeaponClass::Automatic, recoil: 0, sound: None, shot },
        };
        ModSource {
            name: "Test".into(),
            notes: String::new(),
            worm: Worm { gravity: 100.0, ..Worm::default() },
            rope: Rope::default(),
            weapons: vec![("gun".into(), w)],
            shared: [("shell".to_string(), Projectile::new(Gfx::Pixels(vec![])))].into_iter().collect(),
            restrictions: Default::default(),
        }
    }

    fn files(fs: &MemFs) -> Vec<String> {
        fs.files().map(|(p, _)| p.to_string()).collect()
    }

    #[test]
    fn writes_the_layout_and_reads_it_back() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        assert_eq!(files(&fs), ["mod.ron", "projectiles/shell.ron", "weapons/gun.ron"]);
        assert_eq!(read_mod_files(&fs).unwrap(), sample());
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "mod.ron").unwrap().1.to_vec()).unwrap();
        assert!(text.contains("gravity: 100.0") && !text.contains("rope"), "{text}");
    }

    #[test]
    fn writing_what_was_read_gives_the_same_bytes() {
        let mut a = MemFs::new();
        write_mod_files(&mut a, &sample()).unwrap();
        let mut b = MemFs::new();
        write_mod_files(&mut b, &read_mod_files(&a).unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_listed_weapon_without_its_file_names_it() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        let fs = MemFs::new().with("mod.ron", fs.files().find(|(p, _)| p.as_str() == "mod.ron").unwrap().1.to_vec());
        let e = read_mod_files(&fs).unwrap_err();
        assert!(e.to_string().contains("weapons/gun.ron"), "{e}");
    }

    #[test]
    fn an_unlisted_weapon_file_is_an_error() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        let fs = fs.with("weapons/stray.ron", "()");
        let e = read_mod_files(&fs).unwrap_err();
        assert!(e.to_string().contains("weapons/stray.ron") && e.to_string().contains("not listed"), "{e}");
    }

    #[test]
    fn a_duplicate_id_and_nested_projectile_folders_are_errors() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        let dup = fs.clone().with("mod.ron", "(name: \"T\", weapons: [\"gun\", \"gun\"])");
        assert!(read_mod_files(&dup).unwrap_err().to_string().contains("twice"));
        let nested = fs.clone().with("projectiles/a/b.ron", "(gfx: Pixels([]))");
        assert!(read_mod_files(&nested).unwrap_err().to_string().contains("projectiles/a/b.ron"));
        let stray = fs.with("projectiles/notes.txt", "hi");
        assert!(read_mod_files(&stray).is_err());
    }

    #[test]
    fn assets_and_the_manifest_are_ignored() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        let fs = fs.with("pack.ron", "()").with("gfx/a.png", "png").with("sfx/b.ogg", "ogg");
        assert_eq!(read_mod_files(&fs).unwrap(), sample());
    }
}
```

`tests/hostile.rs` (Review Focus 5 for text, plus a fuzz pass):

```rust
//! Malformed and hostile mod files: errors, never panics.

use olr_format::mods::{compile_mod, read_mod_files};
use olr_vfs::MemFs;
use proptest::prelude::*;

const MOD: &str = "(name: \"T\", weapons: [\"w\"])";
const WEAPON: &str = r#"(
    name: "W",
    recharge: 0.5,
    drain: 30.0,
    between_shots_s: 0.25,
    fire: Projectile(
        shot: (
            speed: 100,
            projectile: (
                gfx: Pixels(["#948800"]),
                hit_terrain: [Explode(damage: 4), Spawn("s")],
                spawns: {"s": (amount: 3, projectile: (gfx: Pixels([])))},
            ),
        ),
    ),
)
"#;

#[test]
fn the_fixture_reads_and_compiles() {
    let fs = MemFs::new().with("mod.ron", MOD).with("weapons/w.ron", WEAPON);
    compile_mod(&read_mod_files(&fs).unwrap()).unwrap();
}

#[test]
fn two_hundred_nested_projectiles_are_an_error_not_a_crash() {
    let mut p = String::from("(gfx: Pixels([]))");
    for _ in 0..200 {
        p = format!("(gfx: Pixels([]), spawns: {{\"s\": (projectile: {p})}})");
    }
    let w = format!("(name: \"W\", recharge: 1.0, drain: 1.0, between_shots_s: 1.0, fire: Projectile(shot: (projectile: {p})))");
    let fs = MemFs::new().with("mod.ron", MOD).with("weapons/w.ron", w);
    assert!(read_mod_files(&fs).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn mutated_files_never_panic(edits in proptest::collection::vec((0usize..WEAPON.len(), any::<u8>()), 1..8)) {
        let mut bytes = WEAPON.as_bytes().to_vec();
        for (at, b) in edits {
            bytes[at] = b;
        }
        let fs = MemFs::new().with("mod.ron", MOD).with("weapons/w.ron", bytes);
        if let Ok(src) = read_mod_files(&fs) {
            let _ = compile_mod(&src);
        }
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p olr-format`
Expected: the build fails (`read_mod_files` and `write_mod_files` are
missing).

- [ ] **Step 3: Implement `files.rs`**

```rust
//! A pack's mod files: `mod.ron`, `weapons/<id>.ron`,
//! `projectiles/<id>.ron` and `restrictions.ron`. Other paths
//! (`pack.ron`, assets) belong to `olr-pack` and are ignored here.

use std::collections::BTreeSet;

use olr_vfs::{ContentSink, ContentSource, VPath, walk};
use serde::{Deserialize, Serialize};

use super::compile::{MOD_FILE, shared_file, valid_id, weapon_file};
use super::{ModSource, Rope, Worm};
use crate::ron_io::{from_canonical, read_text, to_canonical, write_text};
use crate::FormatError;

pub const RESTRICTIONS_FILE: &str = "restrictions.ron";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModFile {
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    #[serde(default, skip_serializing_if = "Worm::is_default")]
    worm: Worm,
    #[serde(default, skip_serializing_if = "Rope::is_default")]
    rope: Rope,
    weapons: Vec<String>,
}

pub fn write_mod_files(sink: &mut dyn ContentSink, m: &ModSource) -> Result<(), FormatError> {
    let file = ModFile {
        name: m.name.clone(),
        notes: m.notes.clone(),
        worm: m.worm.clone(),
        rope: m.rope.clone(),
        weapons: m.weapons.iter().map(|(id, _)| id.clone()).collect(),
    };
    write_text(sink, MOD_FILE, &to_canonical(MOD_FILE, &file)?)?;
    for (id, w) in &m.weapons {
        if !valid_id(id) {
            return Err(FormatError::invalid(MOD_FILE, "weapons", format!("invalid weapon id {id:?}")));
        }
        let f = weapon_file(id);
        write_text(sink, &f, &to_canonical(&f, w)?)?;
    }
    for (id, p) in &m.shared {
        if !valid_id(id) {
            return Err(FormatError::invalid(&shared_file(id), "", format!("invalid projectile id {id:?}")));
        }
        let f = shared_file(id);
        write_text(sink, &f, &to_canonical(&f, p)?)?;
    }
    if !m.restrictions.is_empty() {
        write_text(sink, RESTRICTIONS_FILE, &to_canonical(RESTRICTIONS_FILE, &m.restrictions)?)?;
    }
    Ok(())
}

pub fn read_mod_files(src: &dyn ContentSource) -> Result<ModSource, FormatError> {
    let file: ModFile = from_canonical(MOD_FILE, &read_text(src, MOD_FILE)?)?;
    let mut listed = BTreeSet::new();
    let mut weapons = Vec::with_capacity(file.weapons.len());
    for id in &file.weapons {
        if !valid_id(id) {
            return Err(FormatError::invalid(MOD_FILE, "weapons", format!("invalid weapon id {id:?}")));
        }
        if !listed.insert(id.clone()) {
            return Err(FormatError::invalid(MOD_FILE, "weapons", format!("weapon id {id:?} is listed twice")));
        }
        let f = weapon_file(id);
        weapons.push((id.clone(), from_canonical(&f, &read_text(src, &f)?)?));
    }
    let io = |file: &str, message: String| FormatError::Io { file: file.into(), message };
    let mut shared = std::collections::BTreeMap::new();
    let mut restrictions = std::collections::BTreeMap::new();
    for path in walk(src, &VPath::root()).map_err(|e| io("", e.to_string()))? {
        let s = path.as_str();
        if let Some(rest) = s.strip_prefix("weapons/") {
            let id = rest.strip_suffix(".ron").unwrap_or(rest);
            if !listed.contains(id) {
                return Err(io(s, "not listed in mod.ron's weapons".into()));
            }
        } else if let Some(rest) = s.strip_prefix("projectiles/") {
            let id = rest
                .strip_suffix(".ron")
                .filter(|id| !id.contains('/'))
                .ok_or_else(|| io(s, "shared projectiles are `projectiles/<id>.ron`, directly in the folder".into()))?;
            if !valid_id(id) {
                return Err(io(s, format!("invalid projectile id {id:?}")));
            }
            shared.insert(id.to_string(), from_canonical(s, &read_text(src, s)?)?);
        } else if s == RESTRICTIONS_FILE {
            restrictions = from_canonical(s, &read_text(src, s)?)?;
        }
    }
    Ok(ModSource { name: file.name, notes: file.notes, worm: file.worm, rope: file.rope, weapons, shared, restrictions })
}
```

The `projectiles/` count is bounded by `compile_mod`'s `MAX_SHARED`, and
reading is bounded per file by `MAX_RON_BYTES`. Also bound it here:
stop with an error once `shared.len() > MAX_SHARED`, so a hostile pack
cannot make the reader parse millions of files.

- [ ] **Step 4: Run the tests**

Run: `cargo nextest run -p olr-format`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/olr-format
git commit -m "olr-format: read and write a pack's mod files"
```

---

### Task 6: Equivalence with today's import, and the canonical round trip

**Files:**
- Test: `crates/olr-format/tests/equivalence.rs`,
  `crates/olr-format/tests/roundtrip.rs`,
  `crates/olr-format/tests/snapshots/*.snap` (created by `insta`)

**Interfaces:**
- Consumes: `olr_pack::read_mod` and `olr_pack::image_io::decode_rgba`
  (today's reader), `olr_sim::SimMod::compile`, and everything in
  `olr_format::mods`.

- [ ] **Step 1: Write the equivalence test**

`tests/equivalence.rs`:

```rust
//! The bundled OLX mods, read by today's `olr-pack`, lifted, written as
//! authoring files, read back and compiled, give the import back exactly,
//! and the same `SimMod` (spec §6).

use std::collections::BTreeMap;
use std::path::PathBuf;

use olr_data::mod_def::ModDef;
use olr_format::mods::{compile_mod, lift_mod, read_mod_files, write_mod_files};
use olr_vfs::{DirFs, MemFs};

const MODS: [&str; 2] = ["olx-classic", "olx-mw-1-0"];

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// Frame counts by pack path, as `olr-content` computes them.
fn frames(pack: &olr_pack::ModPack) -> BTreeMap<String, u32> {
    pack.assets
        .iter()
        .filter(|(p, _)| p.as_str().ends_with(".png"))
        .filter_map(|(p, bytes)| {
            let img = olr_pack::image_io::decode_rgba(bytes).ok()?;
            (img.height() > 0).then(|| (p.as_str().to_string(), img.width() / img.height()))
        })
        .collect()
}

/// A readable first difference, so a failure names the weapon or projectile.
fn assert_same(id: &str, got: &ModDef, want: &ModDef) {
    assert_eq!(got.name, want.name, "{id}: name");
    assert_eq!(got.worm, want.worm, "{id}: worm");
    assert_eq!(got.rope, want.rope, "{id}: rope");
    assert_eq!(got.restrictions, want.restrictions, "{id}: restrictions");
    assert_eq!(got.weapons.len(), want.weapons.len(), "{id}: weapon count");
    for (g, w) in got.weapons.iter().zip(&want.weapons) {
        assert_eq!(g, w, "{id}: weapon {}", w.id);
    }
    let gk: Vec<&String> = got.projectiles.keys().collect();
    let wk: Vec<&String> = want.projectiles.keys().collect();
    assert_eq!(gk, wk, "{id}: projectile keys");
    for (k, w) in &want.projectiles {
        assert_eq!(&got.projectiles[k], w, "{id}: projectile {k}");
    }
}

#[test]
fn bundled_mods_survive_lift_write_read_and_compile() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    for id in MODS {
        let pack = olr_pack::read_mod(&DirFs::new(root.join(id))).unwrap();
        let src = lift_mod(&pack.def).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &src).unwrap();
        let back = read_mod_files(&fs).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(back, src, "{id}: read differs from what was written");
        let def = compile_mod(&back).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_same(id, &def, &pack.def);
        let f = frames(&pack);
        assert_eq!(
            olr_sim::SimMod::compile(&def, &f).unwrap(),
            olr_sim::SimMod::compile(&pack.def, &f).unwrap(),
            "{id}: SimMod"
        );
        let mut again = MemFs::new();
        write_mod_files(&mut again, &back).unwrap();
        assert_eq!(again, fs, "{id}: writing what was read changed bytes");
    }
}

#[test]
fn canonical_files_of_the_bundled_mods() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let picks = [
        ("olx-classic", ["mod.ron", "weapons/bazooka.ron", "weapons/bouncy-larpa.ron"]),
        ("olx-mw-1-0", ["mod.ron", "weapons/rocket-launcher.ron", "weapons/floating-bomber.ron"]),
    ];
    for (id, files) in picks {
        let pack = olr_pack::read_mod(&DirFs::new(root.join(id))).unwrap();
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &lift_mod(&pack.def).unwrap()).unwrap();
        for f in files {
            let Some((_, bytes)) = fs.files().find(|(p, _)| p.as_str() == f) else {
                panic!("{id}: no {f}; pick another weapon that exists in the pack");
            };
            insta::assert_snapshot!(format!("{id}__{}", f.replace('/', "__")), String::from_utf8_lossy(bytes));
        }
    }
}
```

If an MW weapon id in `picks` does not exist (check
`content/packs/olx-mw-1-0/mod.ron`), pick the MW weapon with the deepest
projectile tree (the research names `floating-bomber`, whose projectile
nests 5 levels) and one plain rocket weapon instead.

- [ ] **Step 2: Write the round-trip proptest**

`tests/roundtrip.rs`:

```rust
//! Random authoring values: written, read back equal; written again, the
//! same bytes (spec §6, Review Focus 1).

use std::collections::BTreeMap;

use olr_data::image::Rgba;
use olr_format::Colour;
use olr_format::mods::*;
use olr_vfs::MemFs;
use proptest::collection::{btree_map, vec};
use proptest::option;
use proptest::prelude::*;

fn f32_any() -> impl Strategy<Value = f32> {
    prop_oneof![
        Just(0.0f32),
        Just(-0.0f32),
        Just(1e-40f32),
        Just(0.1f32),
        Just(0.5f32),
        any::<f32>().prop_filter("finite", |v| v.is_finite()),
    ]
}

fn name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,6}"
}

fn text() -> impl Strategy<Value = String> {
    "[ -~äöüé\"\\\\]{0,12}"
}

fn colour() -> impl Strategy<Value = Colour> {
    any::<[u8; 4]>().prop_map(|[r, g, b, a]| Colour(Rgba { r, g, b, a }))
}

fn effect() -> impl Strategy<Value = Effect> {
    prop_oneof![
        (any::<i32>(), any::<i32>(), option::of(text())).prop_map(|(damage, shake, sound)| Effect::Explode { damage, shake, sound }),
        (f32_any(), any::<i32>()).prop_map(|(factor, explode)| Effect::Bounce { factor, explode }),
        any::<i32>().prop_map(|damage| Effect::Injure { damage }),
        any::<i32>().prop_map(|size| Effect::Carve { size }),
        any::<i32>().prop_map(|shake| Effect::GreenDirt { shake }),
        option::of(text()).prop_map(|file| Effect::Sound { file }),
        Just(Effect::Disappear),
        Just(Effect::Remove),
        Just(Effect::HomeOnOwner),
        any::<i32>().prop_map(Effect::Unknown),
        name().prop_map(Effect::Spawn),
    ]
}

fn gfx() -> impl Strategy<Value = Gfx> {
    prop_oneof![
        vec(colour(), 0..3).prop_map(Gfx::Pixels),
        (text(), any::<bool>(), any::<i32>(), any::<i32>(), any::<bool>(), any::<i32>(), option::of((f32_any(), 0..4i32))).prop_map(
            |(file, rotating, step, speed, heading, frames, anim)| Gfx::Image {
                file,
                rotating,
                rotation_step_deg: step,
                rotation_speed: speed,
                frames_follow_heading: heading,
                frames_follow_heading_mirrored: !heading,
                heading_frames: frames,
                animation: anim.map(|(fps, m)| Animation {
                    fps,
                    mode: [AnimMode::Once, AnimMode::Loop, AnimMode::PingPong, AnimMode::Unknown(m)][m as usize],
                }),
            }
        ),
    ]
}

fn spawn_fields<C: SpawnContext>(
    p: Option<Projectile>,
) -> impl Strategy<Value = Spawn<C>> {
    (any::<i32>(), any::<i32>(), f32_any(), f32_any(), any::<[bool; 6]>(), any::<i32>(), f32_any(), f32_any(), option::of(name())).prop_map(
        move |(amount, speed, speed_var, spread, b, angle, scale, factor, shared)| Spawn {
            shared,
            projectile: p.clone().map(Box::new),
            amount,
            speed,
            speed_var,
            spread,
            relative_to_parent: b[0],
            angle,
            parent_velocity_as_direction: b[1],
            parent_velocity_direction_scale: scale,
            speed_var_diagonal: b[2],
            random_rotation: b[3],
            inherit_velocity: b[4],
            inherit_velocity_factor: factor,
            context: Default::default(),
        },
    )
}

fn leaf() -> impl Strategy<Value = Projectile> {
    (
        option::of(name()),
        text(),
        gfx(),
        option::of((any::<i32>(), any::<i32>())),
        option::of(any::<i32>()),
        f32_any(),
        option::of((f32_any(), f32_any(), vec(effect(), 0..3))),
        vec(effect(), 0..3),
        vec(effect(), 0..3),
        0..6u8,
    )
        .prop_map(|(id, notes, gfx, size, gravity, velocity_factor, timer, hit_terrain, hit_worm, trail)| Projectile {
            id,
            notes,
            gfx,
            size,
            gravity,
            velocity_factor,
            timer: timer.map(|(after_s, variation_s, effects)| Timer { after_s, variation_s, effects }),
            trail: match trail {
                0 => Trail::Off,
                1 => Trail::Smoke,
                2 => Trail::ChemSmoke,
                3 => Trail::Doomsday,
                4 => Trail::Explosive,
                _ => Trail::Unknown(7),
            },
            hit_terrain,
            hit_worm,
            spawns: BTreeMap::new(),
        })
}

fn projectile() -> impl Strategy<Value = Projectile> {
    leaf().prop_recursive(3, 12, 2, |inner| {
        // `shape`: bit 0 adds an event spawn, bit 1 a projectile trail.
        (leaf(), inner.clone(), inner, name(), f32_any(), 0..4u8)
            .prop_flat_map(|(base, event_p, trail_p, spawn_name, every_s, shape)| {
                (
                    Just(base),
                    Just(spawn_name),
                    Just(every_s),
                    Just(shape),
                    spawn_fields::<EventContext>(Some(event_p)),
                    spawn_fields::<TrailContext>(Some(trail_p)),
                )
            })
            .prop_map(|(mut base, spawn_name, every_s, shape, ev, tr)| {
                if shape & 1 != 0 {
                    base.spawns.insert(spawn_name, ev);
                }
                if shape & 2 != 0 {
                    base.trail = Trail::Projectile { every_s, spawn: tr };
                }
                base
            })
    })
}

fn weapon() -> impl Strategy<Value = Weapon> {
    (text(), f32_any(), f32_any(), f32_any(), any::<bool>(), projectile(), 0..3u8, colour(), any::<u32>()).prop_flat_map(
        move |(name, recharge, drain, rof, laser, p, kind, colour, thrust)| {
            (spawn_fields::<ShotContext>(Some(p)), any::<i32>()).prop_map(move |(shot, n)| Weapon {
                name: name.clone(),
                notes: String::new(),
                recharge,
                drain,
                between_shots_s: rof,
                laser_sight: laser,
                fire: match kind {
                    0 => Fire::Projectile { class: WeaponClass::Unknown(n), recoil: n, sound: None, shot },
                    1 => Fire::Beam { recoil: n, colour, hole_size: n, worm_damage: n, length: n, width: n, width_growth: drain, split_damage_over_width: laser },
                    _ => Fire::Special { kind: SpecialKind::Jetpack, thrust },
                },
            })
        },
    )
}

fn mod_source() -> impl Strategy<Value = ModSource> {
    (text(), f32_any(), any::<i32>(), btree_map(name(), weapon(), 1..3), btree_map(name(), leaf(), 0..2)).prop_map(
        |(name, gravity, rest, weapons, shared)| ModSource {
            name,
            notes: String::new(),
            worm: Worm { gravity, ..Worm::default() },
            rope: Rope { rest_length: rest, ..Rope::default() },
            weapons: weapons.into_iter().collect(),
            shared: shared.into_iter().map(|(k, mut p)| {
                p.id = None;
                (k, p)
            }).collect(),
            restrictions: BTreeMap::new(),
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn authoring_values_round_trip_canonically(src in mod_source()) {
        let mut a = MemFs::new();
        write_mod_files(&mut a, &src).unwrap();
        let back = read_mod_files(&a).unwrap();
        prop_assert_eq!(&back, &src);
        let mut b = MemFs::new();
        write_mod_files(&mut b, &back).unwrap();
        prop_assert_eq!(a, b);
    }
}
```

`Spawn`'s and the authoring types' `PartialEq` compare floats with `==`.
Values produced by the strategies are finite, but `0.0 == -0.0`, so the
test also compares the written bytes, which catches a lost `-0.0`.

- [ ] **Step 3: Run the tests, then accept the snapshots**

Run: `cargo nextest run -p olr-format`
Expected:
- `bundled_mods_survive_lift_write_read_and_compile` passes;
- `authoring_values_round_trip_canonically` passes;
- `canonical_files_of_the_bundled_mods` fails, because there are no
  snapshots yet.

Then run `INSTA_UPDATE=always cargo nextest run -p olr-format
canonical_files` and **read every new `.snap` file**:
- `weapons/bazooka.ron` should match the spec's §2 example in content;
- no default field should appear;
- no `Some(` should appear.

Fix any surprise in the types (Task 2), not in the snapshot. Run the
whole `-p olr-format` suite again; it passes.

**If the equivalence test fails,** the message names the weapon or
projectile.
- **A lift error naming an unused action field** means today's importer
  stores a value that this plan's kind table calls unused. Read
  `crates/olr-import-lgs/src/parse.rs` for that kind and event, then
  either add the field to that kind's effect (types, compile and lift
  together) or show the value is a default. Never weaken the
  equivalence assertion.
- **A key mismatch** means the derived-key rule (Task 3) and the
  importer's naming (`crates/olr-import-lgs/src/convert.rs:100-131`)
  disagree for that case. The lift should then have written an `id`, so
  fix the lift.

- [ ] **Step 4: Commit**

```bash
git add crates/olr-format
git commit -m "olr-format: bundled mods round-trip exactly; canonical snapshots"
```

---

### Task 7: Records

**Files:**
- Modify: `docs/porting/olx-notes/mod-fields.md` (append §13 "Final
  names"), `docs/porting/STATUS.md`, `crates/olr-format/CLAUDE.md`

- [ ] **Step 1: The final names**

Append to `docs/porting/olx-notes/mod-fields.md` a section `## 13. Final
names (plan 13a)`. It is a table with the columns runtime type.field,
authoring field and default, covering every field of §2–§12. Copy the
names exactly from `crates/olr-format/src/mods/` (for example
`WeaponDef.rof → Weapon.between_shots_s`, required;
`ProjectileDef.dampening → Projectile.velocity_factor`, 1.0;
`SpawnDef.use_special11_vec_for_speed_var → Spawn.speed_var_diagonal`,
true in trails and false elsewhere). Where the research proposed another
name, say which one was used.

- [ ] **Step 2: STATUS and the crate notes**

In `docs/porting/STATUS.md`'s main table, add a row:

| Mod authoring format (spec 2026-10-16 formats §2): authoring types, canonical writer, compile to `ModDef`, lift from it | — | `olr-format::mods` | done (plan 13a); not yet the packs' format (plan 13c) | `bundled_mods_survive_lift_write_read_and_compile`, `authoring_values_round_trip_canonically`, `mutated_files_never_panic`, the compile and lift unit tests |

Extend `crates/olr-format/CLAUDE.md` with a **Mods** paragraph:
- `ModSource`, `compile_mod`, `lift_mod`, `read_mod_files` and
  `write_mod_files`;
- the key rule;
- what LX56 cannot express (compile errors);
- the context spawn defaults;
- `LIFTED_SPAWN`;
- `Effect::Disappear` (does not remove) versus `Remove`;
- the bounds constants;
- the tests and what each guards.

- [ ] **Step 3: The repository check**

Run: `cargo xtask check` (the second and last of this plan).
Expected: `check passed`.

- [ ] **Step 4: Commit**

```bash
git add docs/porting crates/olr-format/CLAUDE.md
git commit -m "docs: plan 13a's records"
```
