# Plan 2: OLX Fidelity Oracle, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the oracle that turns "feels identical" into a test. It plays scripted scenarios in a patched OpenLieroX, records per-frame state traces, commits six movement scenarios with their traces, and provides first-divergence comparison for the Rust simulation (milestone M2).

**Architecture:**
- **OLX side (`tools/olx-oracle/`).** A small C++ patch to OLX at the pinned commit, built in Docker, adds two test commands:
  - `simLoadInputs`: drives worms through OLX's real keyboard handler with scripted virtual keys;
  - `simRecord`: writes a JSON line per frame.
  A Python control script runs one scenario through OLX's dedicated-control pipe, entirely under manual frame stepping.
- **Rust side (`rl-oracle`).** A layer-2 crate with the scenario format, the trace model and `compare`.
- **Tooling.** `cargo xtask oracle-record` builds the image, records every scenario twice (the two recordings must be byte-identical), and writes or checks `fixtures/oracle/<name>.jsonl.gz`.

**Tech stack:**
- Rust 1.99: `serde`, `serde_json`, `ron`, `flate2` (gzip), `sha2` and `hex` (image tag).
- Docker running Ubuntu 22.04, which builds OLX: cmake, gcc, SDL2 and the other OLX build dependencies.
- Python 3 inside the image.

**Spec:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`. Read §6, §10 (M2) and the §16 amendments (B1–B7), which this plan implements.

## How this plan was verified

The OLX patch was developed against OLX commit `d1ad953f` and built in Docker from a clean checkout. Each scenario was recorded twice and the recordings were byte-identical. Before the physics-clock alignment in the patch, three runs gave three different traces; with it, they match. Every Rust file was compiled, linted and tested against those recordings. The plan was then **replayed task by task in a fresh clone of the repository** by a script that writes exactly the code blocks below and runs exactly the commands below. Every `Expected:` line is observed output from that replay. If an output differs, stop and investigate; do not edit the expectation.

## Conventions

- **Run every command from the repository root.**
- **Steps.** A step either creates or replaces a file with the exact content shown, or runs a command.
- **Test placement.** Rust unit tests live in the same file as their code, so a module and its unit tests arrive together. Integration tests that need recorded data come first and are watched failing (Task 4).
- **Docker is required from Task 3 on.** Tasks 1–2 need only Rust.
- **`cargo xtask check --offline`** is the per-task gate. The online `cargo xtask check` runs in Task 4.

## Global Constraints

- **Toolchain and license.** Rust 1.99, edition 2024, GPL-3.0-or-later. The crate prefix is `rl-`, and every crate has a `CLAUDE.md` and is registered in `xtask/layers.ron`.
- **OLX source pinned** at commit **`d1ad953f4f20610f2648cce26c9739e7c088409d`** of `https://github.com/openlierox/openlierox.git`.
- **`rl-oracle` is layer 2**, with no simulation code (spec §16, B6).
- **Traces** (B1, B2):
  - stored at `fixtures/oracle/<name>.jsonl.gz`, gzip with mtime 0;
  - a header line with OLX's version and all feature values, then frames −1..n−1;
  - floats printed with `%.9g`.
- **Comparison is bit-exact.** `-0.0 != 0.0`, all NaNs are equal, and a `Tolerance` requires a written `reason`.
- **Key bits are the same in `rl_oracle::Keys` and `OracleHooks.h`:** up 1, down 2, left 4, right 8, jump 16, fire 32, rope 64, change 128.
- **Recordings use LX56 aim** (`ForceLX56Aim` = 1), and every scenario is recorded twice and must match byte for byte (B4).
- **No hosted CI.** `cargo xtask oracle-record --check` is a manual check after touching `tools/olx-oracle/` (B7).
- **Never panic on content.** Scenario files and traces are inputs, and malformed ones must produce errors.

## Review Focus

Conditions the spec implies that would bite a user of the oracle. Each is pinned by a named test in the task that owns the code.

1. **A worm with no scripted keys.** If it kept OLX's bot AI, the trace would be random. Every worm must get a frame-0 entry. Pinned by `schedule_adds_frame_zero_for_unscripted_worms` (Task 1).
2. **Typos in scenario files.** An unknown key name, an input past the last frame, or a missing worm must be a clear error, not a silent no-op. Pinned by `rejects_invalid_scenarios` and `unknown_key_is_an_error` (Task 1).
3. **Unusual floats in traces.** NaN, ±infinity and negative zero must parse and compare correctly rather than crash or compare equal by accident. Pinned by `parses_olx_output` and `negative_zero_differs_from_zero` (Task 2).
4. **A stale Docker image.** Editing the patch, Dockerfile or control script must not silently reuse an old image. Pinned by `image_tag_depends_on_names_and_contents` (Task 3).
5. **Traces that lose their meaning.** A committed trace could stop showing what its scenario claims, for example the rope no longer hooking after a scenario edit. Pinned by the behaviour tests in `crates/rl-oracle/tests/fixtures.rs` (Task 4).

## File structure after this plan

```text
crates/rl-oracle/            L2   keys, scenario, trace, compare; tests/fixtures.rs
tools/olx-oracle/                 Dockerfile, oracle-hooks.patch, oracle_control.py, CLAUDE.md
fixtures/oracle/                  {fall,walk,jump,aim,rope,dig}.ron + .jsonl.gz
xtask/src/oracle.rs               cargo xtask oracle-record [--check] [name...]
```

## After this plan

**Plan 3** is the core simulation (M3): `rl-math`, `rl-terrain` and `rl-sim` (timing, worm movement and collision, rope). Its acceptance test runs each of these six scenarios through the Rust simulation and requires `rl_oracle::compare` to report no divergence.


---

### Task 1: `rl-oracle`: key masks and scenario files

Creates the oracle crate with the scenario format: what to load, where worms spawn, and which keys they hold in each frame. Traces and comparison follow in Task 2.

**Files:**

- Modify: `Cargo.toml` (workspace deps `rl-oracle`, `serde_json`)
- Create: `crates/rl-oracle/{Cargo.toml,CLAUDE.md}`, `crates/rl-oracle/src/{lib,keys,scenario}.rs`
- Modify: `xtask/layers.ron` (register `rl-oracle`, layer 2)

**Interfaces:**

- Produces: `rl_oracle::Keys(pub u8)`, with `Keys::NONE`, `Keys::parse("right+jump") -> Result<Keys, String>`, `contains(name)` and `Display`. Bits: up 1, down 2, left 4, right 8, jump 16, fire 32, rope 64, change 128 (same as `OracleHooks.h` in Task 3).
- Produces: `Scenario { description, level, mod_dir (RON key "mod"), seed: u32, frames: u32, settings: BTreeMap<String,String>, worms: Vec<WormSpawn{x,y: i32}>, inputs: Vec<InputChange{frame: u32, worm: usize, keys: String}>, tolerances: Vec<Tolerance{field, max_abs: f32, reason}> }`.
- Produces: `Scenario::from_ron(&str) -> Result<Scenario, OracleError>` (validates), `schedule() -> Vec<(u32, usize, Keys)>` (a frame-0 entry for every worm, sorted), `keys_per_frame() -> Vec<Vec<Keys>>`, and `control_json(out: &str) -> String` (input for `oracle_control.py`).
- Produces: `OracleError::{Scenario(String), Trace{line, message}, Io}`.

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[workspace]
resolver = "3"
members = ["crates/*", "xtask"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "GPL-3.0-or-later"
rust-version = "1.99"
publish = false

[workspace.dependencies]
rl-data = { path = "crates/rl-data" }
rl-vfs = { path = "crates/rl-vfs" }
rl-pack = { path = "crates/rl-pack" }
rl-import-lgs = { path = "crates/rl-import-lgs" }
rl-import-lxl = { path = "crates/rl-import-lxl" }
rl-import-gfx = { path = "crates/rl-import-gfx" }
rl-oracle = { path = "crates/rl-oracle" }
serde = { version = "1.0.229", default-features = false, features = ["derive", "alloc"] }
ron = "0.12.2"
serde_json = "1.0.151"
png = "0.18.1"
flate2 = "1.1.10"
sha2 = "0.11.0"
hex = "0.4.3"
thiserror = "2.0.21"
anyhow = "1.0.104"
clap = { version = "4.6.7", features = ["derive"] }
cargo_metadata = "0.23.1"
syn = { version = "2.0.105", features = ["full"] }
prettyplease = "0.2.37"
quote = "1"
proptest = "1.11.0"
insta = "1.49.0"
tempfile = "3"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }

[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
```

- [ ] **Step 2: Create `crates/rl-oracle/Cargo.toml`**

```toml
[package]
name = "rl-oracle"
description = "OLX fidelity oracle: scenarios, recorded OLX traces, and first-divergence comparison."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
serde = { workspace = true, features = ["std"] }
serde_json.workspace = true
ron.workspace = true
flate2.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Create `crates/rl-oracle/CLAUDE.md`**

```markdown
# rl-oracle

The fidelity oracle (spec §6): scenario files, traces recorded from a patched
OpenLieroX, and first-divergence comparison.

- **Layer 2.** No simulation code. In M3 the simulation turns a `Scenario`
  into a `Trace`, and `compare` checks it against the recording.
- **Scenarios** (`fixtures/oracle/<name>.ron`): level, mod, seed, frame count,
  worm spawns, and key changes (`"right+jump"`).
  - Each worm holds its keys until its next change.
  - `Scenario::schedule()` adds a frame-0 entry for every worm, so OLX never runs bot AI.
- **Traces** (`fixtures/oracle/<name>.jsonl.gz`): a header line with every OLX
  feature value, then frame −1 (the state when recording started) and frames 0..n.
  - Floats are OLX's `%.9g`, which round-trips `f32` exactly.
  - `Num` compares bitwise (`-0.0 != 0.0`); all NaNs are equal.
- **Comparison is bit-exact.** A `Tolerance` needs a written `reason` in the scenario file.
- **Key masks** match `OracleHooks.h` in `tools/olx-oracle/oracle-hooks.patch`.
  Change both together.

Test: `cargo nextest run -p rl-oracle`. Recording needs Docker: `cargo xtask oracle-record`.
```

- [ ] **Step 4: Create `crates/rl-oracle/src/lib.rs`**

```rust
//! The fidelity oracle (spec §6).
//!
//! A [`Scenario`] describes a deterministic OLX run: level, mod, seed, worm
//! spawns and a per-frame key schedule. `cargo xtask oracle-record` plays it
//! in a patched OpenLieroX (`tools/olx-oracle/`) and stores the resulting
//! [`Trace`] in `fixtures/oracle/<name>.jsonl.gz`. The Rust simulation later
//! replays the same scenario and [`compare`] reports the first diverging field.

mod keys;
mod scenario;

pub use keys::Keys;
pub use scenario::{InputChange, Scenario, Tolerance, WormSpawn};

/// Errors from reading scenarios and traces.
#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("{0}")]
    Scenario(String),
    #[error("trace line {line}: {message}")]
    Trace { line: usize, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 5: Create `crates/rl-oracle/src/keys.rs`**

```rust
//! Key masks shared with the OLX hooks (`OracleHooks.h`: KEY_UP = 1 ... KEY_CHANGE = 128).

use std::fmt;

/// The keyboard actions a scenario can hold down during a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Keys(pub u8);

const NAMES: [(&str, u8); 8] = [
    ("up", 1),
    ("down", 2),
    ("left", 4),
    ("right", 8),
    ("jump", 16),
    ("fire", 32),
    ("rope", 64),
    ("change", 128),
];

impl Keys {
    pub const NONE: Keys = Keys(0);

    /// Parses `"right+jump"`; the empty string means no keys.
    pub fn parse(s: &str) -> Result<Keys, String> {
        let mut mask = 0;
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            let bit = NAMES.iter().find(|(n, _)| *n == part).map(|(_, b)| *b).ok_or_else(|| {
                format!(
                    "unknown key {part:?} (expected one of up, down, left, right, jump, fire, rope, change)"
                )
            })?;
            mask |= bit;
        }
        Ok(Keys(mask))
    }

    pub fn contains(self, name: &str) -> bool {
        NAMES.iter().any(|(n, b)| *n == name && self.0 & b != 0)
    }
}

impl fmt::Display for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = NAMES.iter().filter(|(_, b)| self.0 & b != 0).map(|(n, _)| *n).collect();
        f.write_str(&names.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        assert_eq!(Keys::parse("").unwrap(), Keys::NONE);
        assert_eq!(Keys::parse("right+jump").unwrap(), Keys(8 | 16));
        assert_eq!(Keys::parse(" up + change ").unwrap(), Keys(1 | 128));
        assert_eq!(Keys(8 | 16).to_string(), "right+jump");
        assert!(Keys(64).contains("rope"));
    }

    #[test]
    fn unknown_key_is_an_error() {
        let err = Keys::parse("right+jmup").unwrap_err();
        assert!(err.contains("\"jmup\""), "{err}");
    }
}
```

- [ ] **Step 6: Create `crates/rl-oracle/src/scenario.rs`**

```rust
//! Scenario files: `fixtures/oracle/<name>.ron`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Keys, OracleError};

/// A deterministic OLX run to record and to replay in Rust.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub description: String,
    /// Level file name inside OLX's `share/gamedir/levels/`, such as `"Dirt Level.lxl"`.
    pub level: String,
    /// Mod directory inside OLX's `share/gamedir/`, such as `"Classic"`.
    #[serde(rename = "mod")]
    pub mod_dir: String,
    /// Seed for OLX's `rand()` (`simSetSeed`).
    pub seed: u32,
    /// Number of frames to simulate (100 per second).
    pub frames: u32,
    /// Extra OLX `setvar` values, such as `"GameOptions.GameInfo.GameSpeed": "1"`.
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
    pub worms: Vec<WormSpawn>,
    /// Key changes; a worm holds its keys until its next change.
    #[serde(default)]
    pub inputs: Vec<InputChange>,
    /// Fields allowed to differ from OLX, each with a written reason (spec §6).
    #[serde(default)]
    pub tolerances: Vec<Tolerance>,
}

/// Where a worm is spawned, in world pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WormSpawn {
    pub x: i32,
    pub y: i32,
}

/// From frame `frame` on, worm `worm` (index into `worms`) holds `keys`,
/// written as `"right+jump"`; `""` releases everything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputChange {
    pub frame: u32,
    pub worm: usize,
    pub keys: String,
}

/// Allows numeric field `field` (a trace path such as `"w[0].vy"`, where `[*]`
/// matches any index) to differ by at most `max_abs`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    pub field: String,
    pub max_abs: f32,
    pub reason: String,
}

impl Scenario {
    /// Parses and validates a scenario file.
    pub fn from_ron(text: &str) -> Result<Scenario, OracleError> {
        let sc: Scenario = ron::from_str(text).map_err(|e| {
            OracleError::Scenario(format!("{}:{}: {}", e.span.start.line, e.span.start.col, e.code))
        })?;
        sc.validate()?;
        Ok(sc)
    }

    fn validate(&self) -> Result<(), OracleError> {
        let bad = |m: String| Err(OracleError::Scenario(m));
        if self.frames == 0 {
            return bad("frames must be at least 1".into());
        }
        if self.worms.is_empty() {
            return bad("at least one worm is required".into());
        }
        for c in &self.inputs {
            if c.frame >= self.frames {
                return bad(format!("input at frame {} is past the last frame {}", c.frame, self.frames - 1));
            }
            if c.worm >= self.worms.len() {
                return bad(format!("input for worm {} but only {} worms", c.worm, self.worms.len()));
            }
            Keys::parse(&c.keys).map_err(OracleError::Scenario)?;
        }
        for t in &self.tolerances {
            if t.reason.trim().is_empty() || t.max_abs.is_nan() || t.max_abs < 0.0 {
                return bad(format!("tolerance for {} needs a reason and max_abs >= 0", t.field));
            }
        }
        Ok(())
    }

    /// The key schedule as `(frame, worm, keys)`, sorted by frame. Every worm
    /// gets an entry at frame 0, so OLX drives all of them by script and none
    /// runs its bot AI.
    pub fn schedule(&self) -> Vec<(u32, usize, Keys)> {
        let mut out: Vec<(u32, usize, Keys)> = (0..self.worms.len())
            .filter(|w| !self.inputs.iter().any(|c| c.frame == 0 && c.worm == *w))
            .map(|w| (0, w, Keys::NONE))
            .collect();
        out.extend(self.inputs.iter().map(|c| (c.frame, c.worm, Keys::parse(&c.keys).expect("validated"))));
        out.sort_by_key(|(f, _, _)| *f);
        out
    }

    /// The keys each worm holds during each frame: `result[frame][worm]`.
    pub fn keys_per_frame(&self) -> Vec<Vec<Keys>> {
        let mut held = vec![Keys::NONE; self.worms.len()];
        let mut changes = self.schedule().into_iter().peekable();
        (0..self.frames)
            .map(|f| {
                while let Some((_, w, k)) = changes.next_if(|(cf, _, _)| *cf == f) {
                    held[w] = k;
                }
                held.clone()
            })
            .collect()
    }

    /// The JSON that `tools/olx-oracle/oracle_control.py` reads, writing the
    /// trace to `out` (a path inside the container).
    pub fn control_json(&self, out: &str) -> String {
        let inputs: Vec<(u32, usize, u8)> =
            self.schedule().into_iter().map(|(f, w, k)| (f, w, k.0)).collect();
        let worms: Vec<serde_json::Value> =
            self.worms.iter().map(|w| serde_json::json!({ "x": w.x, "y": w.y })).collect();
        serde_json::json!({
            "level": self.level,
            "mod": self.mod_dir,
            "seed": self.seed,
            "frames": self.frames,
            "settings": self.settings,
            "worms": worms,
            "inputs": inputs,
            "out": out,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"(
        description: "walk",
        level: "Liero Factory.lxl",
        mod: "Classic",
        seed: 12345,
        frames: 10,
        worms: [(x: 190, y: 320), (x: 300, y: 320)],
        inputs: [
            (frame: 2, worm: 0, keys: "right"),
            (frame: 0, worm: 1, keys: "up"),
            (frame: 5, worm: 0, keys: ""),
        ],
    )"#;

    #[test]
    fn parses_with_defaults() {
        let sc = Scenario::from_ron(SAMPLE).unwrap();
        assert_eq!(sc.mod_dir, "Classic");
        assert!(sc.settings.is_empty() && sc.tolerances.is_empty());
    }

    #[test]
    fn schedule_adds_frame_zero_for_unscripted_worms() {
        let sc = Scenario::from_ron(SAMPLE).unwrap();
        assert_eq!(
            sc.schedule(),
            vec![(0, 0, Keys::NONE), (0, 1, Keys(1)), (2, 0, Keys(8)), (5, 0, Keys::NONE)]
        );
    }

    #[test]
    fn keys_are_held_until_changed() {
        let k = Scenario::from_ron(SAMPLE).unwrap().keys_per_frame();
        assert_eq!(k.len(), 10);
        assert_eq!(k[1], vec![Keys::NONE, Keys(1)]);
        assert_eq!(k[2][0], Keys(8));
        assert_eq!(k[4][0], Keys(8));
        assert_eq!(k[5][0], Keys::NONE);
        assert_eq!(k[9][1], Keys(1));
    }

    #[test]
    fn control_json_has_what_the_script_reads() {
        let v: serde_json::Value =
            serde_json::from_str(&Scenario::from_ron(SAMPLE).unwrap().control_json("/work/t.jsonl")).unwrap();
        assert_eq!(v["mod"], "Classic");
        assert_eq!(v["worms"][1]["x"], 300);
        assert_eq!(v["inputs"][2], serde_json::json!([2, 0, 8]));
        assert_eq!(v["out"], "/work/t.jsonl");
    }

    #[test]
    fn rejects_invalid_scenarios() {
        let cases = [
            ("frames: 10", "frames: 0", "at least 1"),
            ("frame: 5, worm: 0", "frame: 10, worm: 0", "past the last frame"),
            ("frame: 5, worm: 0", "frame: 5, worm: 2", "only 2 worms"),
            ("keys: \"right\"", "keys: \"rigth\"", "unknown key"),
        ];
        for (from, to, msg) in cases {
            let err = Scenario::from_ron(&SAMPLE.replace(from, to)).unwrap_err().to_string();
            assert!(err.contains(msg), "{to}: {err}");
        }
        let err = Scenario::from_ron("(oops").unwrap_err().to_string();
        assert!(err.starts_with("1:"), "{err}");
    }

    #[test]
    fn tolerances_need_a_reason() {
        let with = SAMPLE.replace(
            "inputs: [",
            "tolerances: [(field: \"w[0].vy\", max_abs: 0.001, reason: \" \")], inputs: [",
        );
        assert!(Scenario::from_ron(&with).unwrap_err().to_string().contains("needs a reason"));
    }
}
```

- [ ] **Step 7: Create `xtask/layers.ron`**

```ron
// Crate layering (spec §4). A crate may depend on crates in lower layers, and on
// same-layer crates only when the pair is listed in `same_layer`. Every
// workspace crate must be listed here. `no_std` crates must declare
// #![no_std]; `wasm` crates must build for wasm32-unknown-unknown.
(
    layers: {
        "rl-data": 1,
        "rl-vfs": 1,
        "rl-pack": 2,
        "rl-import-lgs": 2,
        "rl-import-lxl": 2,
        "rl-import-gfx": 2,
        "rl-oracle": 2,
        "rl-cli": 6,
        "xtask": 6,
    },
    same_layer: [
        ("rl-import-lgs", "rl-pack"),
        ("rl-import-lxl", "rl-pack"),
        ("rl-import-gfx", "rl-pack"),
    ],
    no_std: ["rl-data"],
    wasm: ["rl-data", "rl-vfs", "rl-pack", "rl-import-lgs", "rl-import-lxl", "rl-import-gfx"],
)
```

- [ ] **Step 8: Run**

```bash
cargo nextest run -p rl-oracle
```

Expected: output contains `8 passed`. Covers key parsing, frame-0 entries for worms without input, held keys, the control JSON, and rejection of invalid scenarios and reason-less tolerances.

- [ ] **Step 9: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "rl-oracle: add key masks and scenario files"
```


---

### Task 2: `rl-oracle`: traces and first-divergence comparison

The trace model matches what the patched OLX writes (Task 3). `compare` reports only the first frame and field that differ.

**Files:**

- Create: `crates/rl-oracle/src/{trace,compare}.rs`
- Modify: `crates/rl-oracle/src/lib.rs` (add both modules and their re-exports)

**Interfaces:**

- Produces: `Trace { header: Header{olx: String, features: BTreeMap<String,String>}, frames: Vec<Frame> }`, with `Trace::from_jsonl(&str)`, `from_gz(&[u8])`, `to_jsonl()` and `frame(i64)`.
- Produces: `Frame { f: i64, w: Vec<WormRec>, p: Vec<ProjRec>, b: Vec<BonusRec>, mh: String }`.
- Produces: `WormRec { id, alive, x, y, vx, vy, hp, ang, angv, face, move_dir (JSON "move"), ground, keys: u32, wpn, slots: Vec<SlotRec{id, ch, rl, lf}>, rope: RopeRec{rel, sh, at, pw, x, y, vx, vy}, lives, kills, deaths, dirt }`, `ProjRec { i, t, x, y, vx, vy, life }` and `BonusRec { i, t, x, y }`.
- Produces: `Num(pub f32)`, which reads a JSON number or `"NaN"`, `"inf"` or `"-inf"`. Equality is bitwise, except that all NaNs are equal.
- Produces: `gzip(&str) -> Vec<u8>` (deterministic: mtime 0, no name) and `gunzip(&[u8]) -> Result<String, OracleError>`.
- Produces: `compare(expected: &Trace, actual: &Trace, tol: &[Tolerance]) -> Result<usize, Divergence>`; `Divergence { frame, path, expected, actual, more }` displays as `frame 412: w[0].vy expected 31.25 got 31.249998 (2 more fields differ in this frame)`.

- [ ] **Step 1: Create `crates/rl-oracle/src/trace.rs`**

```rust
//! Trace files: one JSON header line, then one line per frame, as written by
//! the OLX hooks (`tools/olx-oracle/oracle-hooks.patch`, `OracleHooks.cpp`).
//! Frame -1 is the state when recording started; frame `n` is the state after
//! simulating frame `n` with the keys the scenario holds during frame `n`.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::OracleError;

/// An `f32` as OLX recorded it: a JSON number, or `"NaN"`, `"inf"`, `"-inf"`.
/// Equality is bitwise (so `-0.0 != 0.0`), except that all NaNs are equal.
#[derive(Clone, Copy, Debug, Default)]
pub struct Num(pub f32);

impl PartialEq for Num {
    fn eq(&self, other: &Self) -> bool {
        (self.0.is_nan() && other.0.is_nan()) || self.0.to_bits() == other.0.to_bits()
    }
}

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            v if v.is_nan() => s.serialize_str("NaN"),
            v if v == f32::INFINITY => s.serialize_str("inf"),
            v if v == f32::NEG_INFINITY => s.serialize_str("-inf"),
            v => s.serialize_f32(v),
        }
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Num, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(f64),
            S(String),
        }
        // OLX prints 9 significant digits, so the nearest f32 is the original value.
        match Raw::deserialize(d)? {
            Raw::N(v) => Ok(Num(v as f32)),
            Raw::S(s) => match s.as_str() {
                "NaN" => Ok(Num(f32::NAN)),
                "inf" => Ok(Num(f32::INFINITY)),
                "-inf" => Ok(Num(f32::NEG_INFINITY)),
                other => Err(serde::de::Error::custom(format!("not a number: {other:?}"))),
            },
        }
    }
}

/// First line of a trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// OLX version string.
    pub olx: String,
    /// Every OLX gameplay feature (`src/common/FeatureList.cpp`) as OLX printed it.
    pub features: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub f: i64,
    pub w: Vec<WormRec>,
    pub p: Vec<ProjRec>,
    pub b: Vec<BonusRec>,
    /// FNV-1a 64 of the material grid, row by row, as 16 hex digits.
    pub mh: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WormRec {
    pub id: i32,
    pub alive: bool,
    pub x: Num,
    pub y: Num,
    pub vx: Num,
    pub vy: Num,
    pub hp: Num,
    pub ang: Num,
    pub angv: Num,
    pub face: i32,
    #[serde(rename = "move")]
    pub move_dir: i32,
    pub ground: bool,
    /// `worm_state_t::asInt()`: the movement/shoot/jump/carve state OLX derived from the keys.
    pub keys: u32,
    pub wpn: i32,
    pub slots: Vec<SlotRec>,
    pub rope: RopeRec,
    pub lives: i32,
    pub kills: i32,
    pub deaths: i32,
    pub dirt: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotRec {
    pub id: i32,
    pub ch: Num,
    pub rl: bool,
    pub lf: Num,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RopeRec {
    pub rel: bool,
    pub sh: bool,
    pub at: bool,
    pub pw: i32,
    pub x: Num,
    pub y: Num,
    pub vx: Num,
    pub vy: Num,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjRec {
    /// Slot in OLX's projectile pool.
    pub i: i32,
    /// Projectile type: its load-order index in the mod (`-1` if unknown).
    pub t: i32,
    pub x: Num,
    pub y: Num,
    pub vx: Num,
    pub vy: Num,
    pub life: Num,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BonusRec {
    pub i: i32,
    pub t: i32,
    pub x: Num,
    pub y: Num,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    pub header: Header,
    pub frames: Vec<Frame>,
}

fn line_err(line: usize, e: impl fmt::Display) -> OracleError {
    OracleError::Trace { line, message: e.to_string() }
}

impl Trace {
    /// Parses the JSON-lines text OLX wrote.
    pub fn from_jsonl(text: &str) -> Result<Trace, OracleError> {
        let mut lines = text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty());
        let (_, first) = lines.next().ok_or_else(|| line_err(1, "empty trace"))?;
        let header: Header = serde_json::from_str(first).map_err(|e| line_err(1, e))?;
        let frames = lines
            .map(|(i, l)| serde_json::from_str::<Frame>(l).map_err(|e| line_err(i + 1, e)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Trace { header, frames })
    }

    /// Parses a gzip-compressed trace (`.jsonl.gz`).
    pub fn from_gz(bytes: &[u8]) -> Result<Trace, OracleError> {
        Trace::from_jsonl(&gunzip(bytes)?)
    }

    pub fn to_jsonl(&self) -> String {
        let mut out = serde_json::to_string(&self.header).expect("serializable") + "\n";
        for f in &self.frames {
            out += &(serde_json::to_string(f).expect("serializable") + "\n");
        }
        out
    }

    /// The frame recorded with number `f`.
    pub fn frame(&self, f: i64) -> Option<&Frame> {
        self.frames.iter().find(|fr| fr.f == f)
    }
}

/// Gzip with a fixed header (no file name, mtime 0), so equal input gives equal bytes.
pub fn gzip(text: &str) -> Vec<u8> {
    let mut enc = flate2::GzBuilder::new().mtime(0).write(Vec::new(), flate2::Compression::best());
    enc.write_all(text.as_bytes()).expect("in-memory write");
    enc.finish().expect("in-memory gzip")
}

pub fn gunzip(bytes: &[u8]) -> Result<String, OracleError> {
    let mut s = String::new();
    flate2::read::GzDecoder::new(bytes).read_to_string(&mut s)?;
    Ok(s)
}

#[cfg(test)]
// Float literals below are copied from OLX's 9-digit output on purpose.
#[allow(clippy::excessive_precision)]
pub(crate) mod tests {
    use super::*;

    // Abbreviated from a real OLX recording (fall on Dirt Level).
    pub(crate) const OLX: &str = concat!(
        r#"{"olx":"OpenLieroX/20260912.1+git.d1ad953.dirty","features":{"ForceLX56Aim":"true","LX56PhysicsFPS":"84"}}"#,
        "\n",
        r#"{"f":-1,"w":[{"id":0,"alive":true,"x":456,"y":212,"vx":0,"vy":0,"hp":100,"ang":0,"angv":0,"face":1,"move":1,"ground":false,"keys":0,"wpn":0,"slots":[{"id":13,"ch":1,"rl":false,"lf":0}],"rope":{"rel":false,"sh":false,"at":false,"pw":-1,"x":0,"y":0,"vx":0,"vy":0},"lives":-2,"kills":0,"deaths":0,"dirt":0}],"p":[],"b":[],"mh":"86afa75d225d80a7"}"#,
        "\n",
        r#"{"f":0,"w":[{"id":0,"alive":true,"x":456,"y":218.001709,"vx":"-inf","vy":34.1000061,"hp":"NaN","ang":-0,"angv":0,"face":1,"move":1,"ground":false,"keys":4,"wpn":0,"slots":[{"id":13,"ch":1,"rl":false,"lf":0}],"rope":{"rel":false,"sh":false,"at":false,"pw":-1,"x":0,"y":0,"vx":0,"vy":0},"lives":-2,"kills":0,"deaths":0,"dirt":0}],"p":[{"i":3,"t":7,"x":1.5,"y":2,"vx":0,"vy":0,"life":0.25}],"b":[],"mh":"86afa75d225d80a7"}"#,
        "\n",
    );

    #[test]
    fn parses_olx_output() {
        let t = Trace::from_jsonl(OLX).unwrap();
        assert_eq!(t.header.features["LX56PhysicsFPS"], "84");
        assert_eq!(t.frames.len(), 2);
        let w = &t.frame(0).unwrap().w[0];
        assert_eq!(w.y.0, 218.001709_f32);
        assert_eq!(w.vx.0, f32::NEG_INFINITY);
        assert!(w.hp.0.is_nan());
        assert_eq!(w.ang.0.to_bits(), (-0.0_f32).to_bits());
        assert_eq!(t.frame(0).unwrap().p[0].t, 7);
    }

    #[test]
    fn nine_digit_floats_round_trip_exactly() {
        for v in [218.001709_f32, 34.1000061, 1.0e-30, -123456.789, f32::MAX, f32::MIN_POSITIVE] {
            let text = format!("{v:.9e}");
            let back: Num = serde_json::from_str(&text).unwrap();
            assert_eq!(back, Num(v), "{text}");
        }
    }

    #[test]
    fn jsonl_and_gzip_round_trip() {
        let t = Trace::from_jsonl(OLX).unwrap();
        assert_eq!(Trace::from_jsonl(&t.to_jsonl()).unwrap(), t);
        let gz = gzip(OLX);
        assert_eq!(gz, gzip(OLX), "gzip output must be deterministic");
        assert_eq!(Trace::from_gz(&gz).unwrap(), t);
    }

    #[test]
    fn errors_name_the_line() {
        let bad = OLX.replace(r#""f":0"#, r#""f":"zero""#);
        let err = Trace::from_jsonl(&bad).unwrap_err().to_string();
        assert!(err.starts_with("trace line 3:"), "{err}");
        assert!(Trace::from_jsonl("").unwrap_err().to_string().contains("empty trace"));
    }

    #[test]
    fn num_equality_is_bitwise_except_nan() {
        assert_eq!(Num(f32::NAN), Num(-f32::NAN));
        assert_ne!(Num(0.0), Num(-0.0));
        assert_ne!(Num(1.0), Num(1.0000001));
    }
}
```

- [ ] **Step 2: Create `crates/rl-oracle/src/compare.rs`**

```rust
//! First-divergence comparison of two traces (spec §6): the report names only
//! the first frame and field that differ, so failures stay short.

use std::fmt;

use crate::{Frame, Num, Tolerance, Trace};

/// The first difference between an expected (OLX) and an actual (Rust) trace.
#[derive(Clone, Debug, PartialEq)]
pub struct Divergence {
    pub frame: i64,
    /// Trace path such as `w[0].vy`, `p.len` or `frames`.
    pub path: String,
    pub expected: String,
    pub actual: String,
    /// How many other fields differ in the same frame.
    pub more: usize,
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame {}: {} expected {} got {}", self.frame, self.path, self.expected, self.actual)?;
        if self.more > 0 {
            write!(f, " ({} more fields differ in this frame)", self.more)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Num(Num),
    Int(i64),
    Bool(bool),
    Str(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Num(n) => write!(f, "{n}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) => write!(f, "{s}"),
        }
    }
}

/// Every comparable field of a frame, in a fixed order, with its trace path.
/// List lengths come before list items, so a missing projectile is reported
/// as `p.len`, not as a confusing field mismatch.
fn fields(fr: &Frame) -> Vec<(String, Value)> {
    use Value::*;
    let mut out = vec![
        ("f".to_string(), Int(fr.f)),
        ("w.len".into(), Int(fr.w.len() as i64)),
        ("p.len".into(), Int(fr.p.len() as i64)),
        ("b.len".into(), Int(fr.b.len() as i64)),
    ];
    for (i, w) in fr.w.iter().enumerate() {
        let p = |k: &str| format!("w[{i}].{k}");
        out.extend([
            (p("id"), Int(w.id.into())),
            (p("alive"), Bool(w.alive)),
            (p("x"), Num(w.x)),
            (p("y"), Num(w.y)),
            (p("vx"), Num(w.vx)),
            (p("vy"), Num(w.vy)),
            (p("hp"), Num(w.hp)),
            (p("ang"), Num(w.ang)),
            (p("angv"), Num(w.angv)),
            (p("face"), Int(w.face.into())),
            (p("move"), Int(w.move_dir.into())),
            (p("ground"), Bool(w.ground)),
            (p("keys"), Int(w.keys.into())),
            (p("wpn"), Int(w.wpn.into())),
            (p("slots.len"), Int(w.slots.len() as i64)),
        ]);
        for (j, s) in w.slots.iter().enumerate() {
            let q = |k: &str| format!("w[{i}].slots[{j}].{k}");
            out.extend([
                (q("id"), Int(s.id.into())),
                (q("ch"), Num(s.ch)),
                (q("rl"), Bool(s.rl)),
                (q("lf"), Num(s.lf)),
            ]);
        }
        let r = &w.rope;
        out.extend([
            (p("rope.rel"), Bool(r.rel)),
            (p("rope.sh"), Bool(r.sh)),
            (p("rope.at"), Bool(r.at)),
            (p("rope.pw"), Int(r.pw.into())),
            (p("rope.x"), Num(r.x)),
            (p("rope.y"), Num(r.y)),
            (p("rope.vx"), Num(r.vx)),
            (p("rope.vy"), Num(r.vy)),
            (p("lives"), Int(w.lives.into())),
            (p("kills"), Int(w.kills.into())),
            (p("deaths"), Int(w.deaths.into())),
            (p("dirt"), Int(w.dirt.into())),
        ]);
    }
    for (i, pr) in fr.p.iter().enumerate() {
        let p = |k: &str| format!("p[{i}].{k}");
        out.extend([
            (p("i"), Int(pr.i.into())),
            (p("t"), Int(pr.t.into())),
            (p("x"), Num(pr.x)),
            (p("y"), Num(pr.y)),
            (p("vx"), Num(pr.vx)),
            (p("vy"), Num(pr.vy)),
            (p("life"), Num(pr.life)),
        ]);
    }
    for (i, b) in fr.b.iter().enumerate() {
        let p = |k: &str| format!("b[{i}].{k}");
        out.extend([
            (p("i"), Int(b.i.into())),
            (p("t"), Int(b.t.into())),
            (p("x"), Num(b.x)),
            (p("y"), Num(b.y)),
        ]);
    }
    out.push(("mh".into(), Str(fr.mh.clone())));
    out
}

/// Whether `pattern` (where `[*]` matches any index) matches `path`.
fn path_matches(pattern: &str, path: &str) -> bool {
    let (mut p, mut s) = (pattern, path);
    while let Some(i) = p.find("[*]") {
        let Some(rest) = s.strip_prefix(&p[..i + 1]) else { return false };
        let Some(close) = rest.find(']') else { return false };
        if close == 0 || !rest[..close].bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        p = &p[i + 3..];
        s = &rest[close + 1..];
    }
    p == s
}

fn equal(path: &str, e: &Value, a: &Value, tol: &[Tolerance]) -> bool {
    if e == a {
        return true;
    }
    match (e, a) {
        (Value::Num(e), Value::Num(a)) => {
            tol.iter().any(|t| path_matches(&t.field, path) && (e.0 - a.0).abs() <= t.max_abs)
        }
        _ => false,
    }
}

/// Compares `actual` against `expected` frame by frame and returns the number
/// of frames compared, or the first divergence. Header lines are not compared.
pub fn compare(expected: &Trace, actual: &Trace, tol: &[Tolerance]) -> Result<usize, Divergence> {
    for (e, a) in expected.frames.iter().zip(&actual.frames) {
        let (ef, af) = (fields(e), fields(a));
        let mut diffs = ef.iter().zip(&af).filter(|((p, ev), (_, av))| !equal(p, ev, av, tol));
        if let Some(((path, ev), (_, av))) = diffs.next() {
            return Err(Divergence {
                frame: e.f,
                path: path.clone(),
                expected: ev.to_string(),
                actual: av.to_string(),
                more: diffs.count(),
            });
        }
    }
    if expected.frames.len() != actual.frames.len() {
        let n = expected.frames.len().min(actual.frames.len());
        return Err(Divergence {
            frame: n as i64 - 1,
            path: "frames".into(),
            expected: expected.frames.len().to_string(),
            actual: actual.frames.len().to_string(),
            more: 0,
        });
    }
    Ok(expected.frames.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjRec;
    use crate::trace::tests::OLX;

    fn olx() -> Trace {
        Trace::from_jsonl(OLX).unwrap()
    }

    #[test]
    fn identical_traces_match() {
        assert_eq!(compare(&olx(), &olx(), &[]), Ok(2));
    }

    #[test]
    fn reports_first_frame_and_field_only() {
        let mut a = olx();
        a.frames[1].w[0].vy = Num(31.249998);
        a.frames[1].w[0].x = Num(457.0);
        a.frames[1].mh = "0".into();
        let d = compare(&olx(), &a, &[]).unwrap_err();
        assert_eq!(
            d.to_string(),
            "frame 0: w[0].x expected 456 got 457 (2 more fields differ in this frame)"
        );
    }

    #[test]
    fn negative_zero_differs_from_zero() {
        let mut a = olx();
        a.frames[1].w[0].ang = Num(0.0);
        assert_eq!(compare(&olx(), &a, &[]).unwrap_err().path, "w[0].ang");
    }

    #[test]
    fn missing_projectile_is_a_length_divergence() {
        let mut a = olx();
        a.frames[1].p.clear();
        let d = compare(&olx(), &a, &[]).unwrap_err();
        assert_eq!((d.path.as_str(), d.expected.as_str(), d.actual.as_str()), ("p.len", "1", "0"));
        let mut b = olx();
        let extra = ProjRec { i: 4, ..b.frames[1].p[0].clone() };
        b.frames[1].p.push(extra);
        assert_eq!(compare(&olx(), &b, &[]).unwrap_err().path, "p.len");
    }

    #[test]
    fn missing_frames_are_reported() {
        let mut a = olx();
        a.frames.pop();
        let d = compare(&olx(), &a, &[]).unwrap_err();
        assert_eq!((d.path.as_str(), d.expected.as_str(), d.actual.as_str()), ("frames", "2", "1"));
    }

    #[test]
    fn tolerances_apply_to_matching_numeric_fields_only() {
        let mut a = olx();
        let vy = a.frames[1].w[0].vy.0;
        a.frames[1].w[0].vy = Num(vy + 0.0005);
        let tol = |field: &str| Tolerance { field: field.into(), max_abs: 0.001, reason: "test".into() };
        assert_eq!(compare(&olx(), &a, &[tol("w[*].vy")]), Ok(2));
        assert_eq!(compare(&olx(), &a, &[tol("w[0].vy")]), Ok(2));
        assert!(compare(&olx(), &a, &[tol("w[1].vy")]).is_err());
        assert!(compare(&olx(), &a, &[tol("w[*].vx")]).is_err());
    }

    #[test]
    fn wildcard_paths() {
        assert!(path_matches("w[*].rope.x", "w[12].rope.x"));
        assert!(path_matches("p[*].x", "p[0].x"));
        assert!(!path_matches("p[*].x", "p[].x"));
        assert!(!path_matches("p[*].x", "p[a].x"));
        assert!(!path_matches("w[*].x", "w[0].vx"));
    }
}
```

- [ ] **Step 3: Replace `crates/rl-oracle/src/lib.rs`**

```rust
//! The fidelity oracle (spec §6).
//!
//! A [`Scenario`] describes a deterministic OLX run: level, mod, seed, worm
//! spawns and a per-frame key schedule. `cargo xtask oracle-record` plays it
//! in a patched OpenLieroX (`tools/olx-oracle/`) and stores the resulting
//! [`Trace`] in `fixtures/oracle/<name>.jsonl.gz`. The Rust simulation later
//! replays the same scenario and [`compare`] reports the first diverging field.

mod compare;
mod keys;
mod scenario;
mod trace;

pub use compare::{Divergence, compare};
pub use keys::Keys;
pub use scenario::{InputChange, Scenario, Tolerance, WormSpawn};
pub use trace::{BonusRec, Frame, Header, Num, ProjRec, RopeRec, SlotRec, Trace, WormRec, gunzip, gzip};

/// Errors from reading scenarios and traces.
#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("{0}")]
    Scenario(String),
    #[error("trace line {line}: {message}")]
    Trace { line: usize, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 4: Run**

```bash
cargo nextest run -p rl-oracle
```

Expected: output contains `20 passed`. Includes parsing real OLX output (with `-inf`, `NaN`, `-0`), exact 9-digit float round trips, deterministic gzip, list-length divergences, missing frames, and wildcard tolerances.

- [ ] **Step 5: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "rl-oracle: add traces and first-divergence comparison"
```


---

### Task 3: Patched OLX in Docker and `cargo xtask oracle-record`

Adds the patched OpenLieroX (built from the pinned commit in Docker) and the xtask command that plays a scenario in it. Each scenario is recorded twice, and the two recordings must be byte-identical. The first scenario, `fall`, proves the whole pipeline.

**Files:**

- Create: `tools/olx-oracle/{Dockerfile,oracle-hooks.patch,oracle_control.py,CLAUDE.md}`
- Create: `xtask/src/oracle.rs`, `fixtures/oracle/fall.ron`
- Modify: `xtask/src/main.rs` (the `oracle-record` command), `xtask/Cargo.toml` (deps `rl-oracle`, `sha2`, `hex`)

**Interfaces:**

- Consumes: `Scenario::from_ron` and `control_json`, `Trace::from_jsonl` and `from_gz`, `compare`, `gzip` (Tasks 1–2).
- Produces: `cargo xtask oracle-record [--check] [name...]`. It writes `fixtures/oracle/<name>.jsonl.gz`; with `--check` it compares instead. Lines read `ok   <name>: ...` or `FAIL <name>: ...`, and the exit status is 0 only if all pass.
- Produces: the Docker image `rl-olx-oracle:<12 hex digits>`, tagged by a hash of the three tool files (`oracle::image_tag`).
- Requires: Docker. The first image build compiles OLX and takes a few minutes; later runs take seconds.

- [ ] **Step 1: Create `tools/olx-oracle/Dockerfile`**

```text
# OpenLieroX at the commit pinned in content/olx-manifest.ron, plus the
# rust-liero oracle test hooks (oracle-hooks.patch), built as a headless
# dedicated server. `cargo xtask oracle-record` builds and runs this image.
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential cmake git ca-certificates python3 \
        libsdl2-dev libsdl2-image-dev libsdl2-mixer-dev libxml2-dev libgd-dev \
        zlib1g-dev libzip-dev libx11-dev libcurl4-openssl-dev libboost-dev \
        libopenal-dev libalut-dev libvorbis-dev libyaml-cpp-dev \
        binutils-dev libiberty-dev \
    && rm -rf /var/lib/apt/lists/*

ARG OLX_COMMIT=d1ad953f4f20610f2648cce26c9739e7c088409d
RUN git init -q /olx && cd /olx \
    && git remote add origin https://github.com/openlierox/openlierox.git \
    && git fetch -q --depth 1 origin "$OLX_COMMIT" && git checkout -q FETCH_HEAD

COPY oracle-hooks.patch /olx/
RUN cd /olx && git apply oracle-hooks.patch \
    && cmake -DDEDICATED_ONLY=Yes -DHAWKNL_BUILTIN=Yes -DOLX_DOWNLOAD_GAMECONTROLLERDB=OFF -DDEBUG=No . \
    && make -j"$(nproc)"

COPY oracle_control.py /olx/oracle_control.py
RUN chmod 755 /olx/oracle_control.py
WORKDIR /olx/share/gamedir
```

- [ ] **Step 2: Create `tools/olx-oracle/oracle-hooks.patch`**

```text
diff --git a/include/CGameScript.h b/include/CGameScript.h
index bd9eaa4..fdbf335 100644
--- a/include/CGameScript.h
+++ b/include/CGameScript.h
@@ -178,6 +178,12 @@ public:
 	bool	getNeedCollisionInfo()		{ return needCollisionInfo; }
 	
 	size_t	getProjectileCount() const	{ return projectiles.size(); }
+	// Load-order index of a projectile type, or -1 (OracleHooks).
+	int		projectileIndex(const proj_t* p) const {
+		for(Projectiles::const_iterator it = projectiles.begin(); it != projectiles.end(); ++it)
+			if(it->second == p) return it->first;
+		return -1;
+	}
 	
 	bool	Compile(const std::string& dir);
 
diff --git a/include/CInput.h b/include/CInput.h
index 7ca1b1f..7b32e76 100644
--- a/include/CInput.h
+++ b/include/CInput.h
@@ -48,6 +48,9 @@
 
 struct KeyboardEvent;
 
+// Scripted key state for the rust-liero oracle test hooks (OracleHooks.h).
+struct VirtualKey { bool down; bool once; VirtualKey() : down(false), once(false) {} };
+
 class CInput {
 public:
 	CInput();
@@ -128,6 +131,9 @@ public:
 	int		wasUp(); // checks if there was an keyup-event; returns the count of up-events
 
 	std::string getEventName() { return m_EventName; }
+
+	// Test hook: when set, this input reads the scripted key instead of devices.
+	VirtualKey* virt = NULL;
 	// onlyAvailable drops bindings that can't currently fire (e.g. an absent gamepad)
 	std::string getEventName(bool onlyAvailable) const;
 
diff --git a/include/OracleHooks.h b/include/OracleHooks.h
new file mode 100644
index 0000000..906b893
--- /dev/null
+++ b/include/OracleHooks.h
@@ -0,0 +1,35 @@
+/*
+	OpenLieroX
+
+	Test hooks for the rust-liero fidelity oracle.
+	Scripted keyboard input for worms and per-frame state recording,
+	used with simSetSeed/simStep to record reference traces.
+
+	code under LGPL
+*/
+
+#ifndef __ORACLEHOOKS_H__
+#define __ORACLEHOOKS_H__
+
+#include <string>
+
+namespace OracleHooks {
+	// Key bits used by the input schedule (simLoadInputs).
+	enum {
+		KEY_UP = 1, KEY_DOWN = 2, KEY_LEFT = 4, KEY_RIGHT = 8,
+		KEY_JUMP = 16, KEY_FIRE = 32, KEY_ROPE = 64, KEY_CHANGE = 128,
+	};
+
+	// Loads "frame worm keys" lines. Frame 0 is the first frame simulated after
+	// this call. The listed worms switch to a human input handler driven by the
+	// schedule. Also aligns the LX56 physics clock with the frame clock; spawn
+	// worms after this call. Returns false and sets err on failure.
+	bool loadInputs(const std::string& file, std::string& err);
+	// Starts writing one JSON line per simulated frame to file ("off" stops).
+	bool record(const std::string& file, std::string& err);
+	// Called by the game loop around each simulated frame.
+	void beforeFrame();
+	void afterFrame();
+}
+
+#endif
diff --git a/src/client/CInput.cpp b/src/client/CInput.cpp
index 5ae81c1..90200b2 100644
--- a/src/client/CInput.cpp
+++ b/src/client/CInput.cpp
@@ -879,6 +879,7 @@ bool CInput::Binding::isDown() const
 
 bool CInput::isDown() const
 {
+	if(virt) return virt->down;
 	for(std::vector<Binding>::const_iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		if(it->isDown()) return true;
 	return false;
@@ -889,6 +890,7 @@ bool CInput::isDown() const
 // Returns if the input was pushed down once
 bool CInput::isDownOnce()
 {
+	if(virt) return virt->once;
 	for(std::vector<Binding>::iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		if(it->nDownOnce != 0) return true;
 	return false;
@@ -967,12 +969,14 @@ bool CInput::isJoystick() const {
 }
 
 bool CInput::isJoystickDown() const {
+	if(virt) return false;
 	for(std::vector<Binding>::const_iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		if(it->isJoystick() && it->isDown()) return true;
 	return false;
 }
 
 bool CInput::isJoystickDownOnce() const {
+	if(virt) return false;
 	for(std::vector<Binding>::const_iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		if(it->isJoystick() && it->nDownOnce != 0) return true;
 	return false;
@@ -1007,6 +1011,7 @@ bool CInput::usesKeyboardKey(int sym) const {
 }
 
 void CInput::reset() {
+	if(virt) virt->once = false;
 	for(std::vector<Binding>::iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		it->reset();
 }
diff --git a/src/common/Command.cpp b/src/common/Command.cpp
index feb1223..c8aa587 100644
--- a/src/common/Command.cpp
+++ b/src/common/Command.cpp
@@ -19,6 +19,7 @@
 
 #include <limits.h>
 #include "LieroX.h"
+#include "OracleHooks.h"
 #include "Debug.h"
 #include "CServer.h"
 #include "CClient.h"
@@ -2308,6 +2309,18 @@ void Cmd_setWormInput::exec(CmdLineIntf* caller, const std::vector<std::string>&
 }
 
 
+COMMAND(simLoadInputs, "load a scripted key schedule: lines 'frame worm keys' (test hook)", "file", 1, 1);
+void Cmd_simLoadInputs::exec(CmdLineIntf* caller, const std::vector<std::string>& params) {
+	std::string err;
+	if(!OracleHooks::loadInputs(params[0], err)) caller->writeMsg(err, CNC_ERROR);
+}
+
+COMMAND(simRecord, "record one JSON line per simulated frame, or 'off' (test hook)", "file|off", 1, 1);
+void Cmd_simRecord::exec(CmdLineIntf* caller, const std::vector<std::string>& params) {
+	std::string err;
+	if(!OracleHooks::record(params[0], err)) caller->writeMsg(err, CNC_ERROR);
+}
+
 COMMAND(dumpGameState, "dump game state", "", 0, 0);
 void Cmd_dumpGameState::exec(CmdLineIntf* caller, const std::vector<std::string>& params) {
 	caller->writeMsg("GameState: " + Game::StateAsStr(game.state), CNC_DEV);
diff --git a/src/common/OracleHooks.cpp b/src/common/OracleHooks.cpp
new file mode 100644
index 0000000..59bbcd1
--- /dev/null
+++ b/src/common/OracleHooks.cpp
@@ -0,0 +1,247 @@
+/*
+	OpenLieroX
+
+	Test hooks for the rust-liero fidelity oracle (see OracleHooks.h).
+
+	code under LGPL
+*/
+
+#include <cmath>
+#include <cstdio>
+#include <fstream>
+#include <map>
+#include <sstream>
+#include <vector>
+
+#include "LieroX.h"
+#include "OracleHooks.h"
+#include "CClient.h"
+#include "CBonus.h"
+#include "CGameScript.h"
+#include "CInput.h"
+#include "CProjectile.h"
+#include "CWormHuman.h"
+#include "FeatureList.h"
+#include "Version.h"
+#include "game/CMap.h"
+#include "game/CWorm.h"
+#include "game/Game.h"
+#include "gusanos/allegro.h"
+
+namespace OracleHooks {
+
+struct Keys {
+	VirtualKey up, down, left, right, jump, fire, rope, change, strafe, prev, next;
+};
+
+struct Event { int worm; unsigned mask; };
+
+static std::map<int, Keys> g_keys;                  // worm id -> scripted keys
+static std::map<long, std::vector<Event> > g_plan;  // frame -> key changes
+static long g_base = -1;                            // serverFrame before frame 0
+static FILE* g_rec = NULL;
+
+// Reads CWorm state that has no public getter (CWorm declares this a friend).
+struct Access {
+	static float angleSpeed(const CWorm* w) { return w->fAngleSpeed; }
+};
+
+static void writeHeader(FILE* o);
+static void writeState(long frame);
+
+static void setKey(VirtualKey& k, bool down) {
+	// A press is latched until the first getInput() consumes it, like CInput's nDownOnce.
+	if(down && !k.down) k.once = true;
+	k.down = down;
+}
+
+static void apply(Keys& k, unsigned m) {
+	setKey(k.up, m & KEY_UP);
+	setKey(k.down, m & KEY_DOWN);
+	setKey(k.left, m & KEY_LEFT);
+	setKey(k.right, m & KEY_RIGHT);
+	setKey(k.jump, m & KEY_JUMP);
+	setKey(k.fire, m & KEY_FIRE);
+	setKey(k.rope, m & KEY_ROPE);
+	setKey(k.change, m & KEY_CHANGE);
+}
+
+static bool installHumanHandler(int wormId, Keys& keys, std::string& err) {
+	CWorm* w = game.wormById(wormId, false);
+	if(!w) { err = "no worm " + itoa(wormId); return false; }
+	if(!w->getLocal()) { err = "worm " + itoa(wormId) + " is not local"; return false; }
+	CWormHumanInputHandler* h = dynamic_cast<CWormHumanInputHandler*>(w->inputHandler());
+	if(!h) {
+		h = new CWormHumanInputHandler(w);
+		w->replaceInputHandlerForTest(h);
+	}
+	h->getInputUp().virt = &keys.up;
+	h->getInputDown().virt = &keys.down;
+	h->getInputLeft().virt = &keys.left;
+	h->getInputRight().virt = &keys.right;
+	h->getInputJump().virt = &keys.jump;
+	h->getInputShoot().virt = &keys.fire;
+	h->getInputRope().virt = &keys.rope;
+	h->getInputWeapon().virt = &keys.change;
+	h->getInputStrafe().virt = &keys.strafe;
+	h->getInputPrevWeapon().virt = &keys.prev;
+	h->getInputNextWeapon().virt = &keys.next;
+	return true;
+}
+
+bool loadInputs(const std::string& file, std::string& err) {
+	std::ifstream in(file.c_str());
+	if(!in) { err = "cannot open " + file; return false; }
+	g_plan.clear();
+	std::string line;
+	while(std::getline(in, line)) {
+		if(line.empty() || line[0] == '#') continue;
+		std::istringstream ls(line);
+		long frame; int worm; unsigned mask;
+		if(!(ls >> frame >> worm >> mask)) { err = "bad line: " + line; return false; }
+		g_plan[frame].push_back(Event{worm, mask});
+		if(!g_keys.count(worm) && !installHumanHandler(worm, g_keys[worm], err)) return false;
+	}
+	g_base = (long)game.serverFrame;
+	// Align the LX56 physics clock (84 Hz) with the 100 Hz frame clock, so frame 0
+	// does not catch up on however much time the setup took (as simStep does once).
+	if(cClient) cClient->fLastSimulationTime = game.simulationAbsTime();
+	return true;
+}
+
+bool record(const std::string& file, std::string& err) {
+	if(g_rec) { fclose(g_rec); g_rec = NULL; }
+	if(file == "off") return true;
+	g_rec = fopen(file.c_str(), "w");
+	if(!g_rec) { err = "cannot write " + file; return false; }
+	if(g_base < 0) g_base = (long)game.serverFrame;
+	writeHeader(g_rec);
+	writeState(-1); // the state frame 0 starts from
+	return true;
+}
+
+static long frameIndex() { return (long)game.serverFrame - g_base - 1; }
+
+void beforeFrame() {
+	if(g_base < 0) return;
+	std::map<long, std::vector<Event> >::iterator it = g_plan.find(frameIndex());
+	if(it == g_plan.end()) return;
+	for(size_t i = 0; i < it->second.size(); i++)
+		apply(g_keys[it->second[i].worm], it->second[i].mask);
+}
+
+// Floats as JSON numbers with 9 significant digits (round-trips f32), or a string if not finite.
+static void f(FILE* o, float v) {
+	if(std::isnan(v)) fputs("\"NaN\"", o);
+	else if(std::isinf(v)) fputs(v > 0 ? "\"inf\"" : "\"-inf\"", o);
+	else fprintf(o, "%.9g", v);
+}
+
+static unsigned long long materialHash() {
+	CMap* m = game.gameMap();
+	unsigned long long h = 1469598103934665603ULL; // FNV-1a 64
+	if(!m || !m->material) return 0;
+	for(uint y = 0; y < m->GetHeight(); y++)
+		for(uint x = 0; x < m->GetWidth(); x++) {
+			h ^= m->material->line[y][x];
+			h *= 1099511628211ULL;
+		}
+	return h;
+}
+
+// JSON string with quotes and backslashes escaped.
+static void str(FILE* o, const std::string& s) {
+	fputc('"', o);
+	for(size_t i = 0; i < s.size(); i++) {
+		if(s[i] == '"' || s[i] == '\\') fputc('\\', o);
+		if((unsigned char)s[i] >= 0x20) fputc(s[i], o);
+	}
+	fputc('"', o);
+}
+
+// First line of a recording: the OLX version and every gameplay setting in effect.
+static void writeHeader(FILE* o) {
+	fputs("{\"olx\":", o);
+	str(o, GetFullGameName());
+	fputs(",\"features\":{", o);
+	for(size_t i = 0; i < featureArrayLen(); i++) {
+		if(i) fputc(',', o);
+		str(o, featureArray[i].name);
+		fputc(':', o);
+		str(o, cClient->getGameLobby()[(FeatureIndex)i].toString());
+	}
+	fputs("}}\n", o);
+}
+
+static void writeState(long frame) {
+	FILE* o = g_rec;
+	fprintf(o, "{\"f\":%ld,\"w\":[", frame);
+	bool first = true;
+	for_each_iterator(CWorm*, it, game.worms()) {
+		CWorm* w = it->get();
+		if(!first) fputc(',', o);
+		first = false;
+		const CNinjaRope& r = w->cNinjaRope.get();
+		fprintf(o, "{\"id\":%d,\"alive\":%s,\"x\":", w->getID(), w->getAlive() ? "true" : "false");
+		f(o, w->getPos().x); fputs(",\"y\":", o); f(o, w->getPos().y);
+		fputs(",\"vx\":", o); f(o, w->getVelocity().x); fputs(",\"vy\":", o); f(o, w->getVelocity().y);
+		fputs(",\"hp\":", o); f(o, w->getHealth());
+		fputs(",\"ang\":", o); f(o, w->getAngle()); fputs(",\"angv\":", o); f(o, Access::angleSpeed(w));
+		fprintf(o, ",\"face\":%d,\"move\":%d,\"ground\":%s,\"keys\":%u,\"wpn\":%d,\"slots\":[",
+			(int)w->getFaceDirectionSide(), (int)w->getMoveDirectionSide(), w->isOnGround() ? "true" : "false",
+			(unsigned)w->tState.get().asInt(), (int)w->iCurrentWeapon);
+		for(size_t i = 0; i < w->weaponSlots.get().size(); i++) {
+			const wpnslot_t& s = w->weaponSlots.get()[i];
+			if(i) fputc(',', o);
+			fprintf(o, "{\"id\":%d,\"ch\":", (int)s.WeaponId); f(o, s.Charge);
+			fprintf(o, ",\"rl\":%s,\"lf\":", s.Reloading ? "true" : "false"); f(o, s.LastFire);
+			fputc('}', o);
+		}
+		fprintf(o, "],\"rope\":{\"rel\":%s,\"sh\":%s,\"at\":%s,\"pw\":%d,\"x\":",
+			r.isReleased() ? "true" : "false", r.isShooting() ? "true" : "false",
+			r.isAttached() ? "true" : "false", r.getAttachedPlayer() ? r.getAttachedPlayer()->getID() : -1);
+		f(o, r.getPos().x); fputs(",\"y\":", o); f(o, r.getPos().y);
+		fputs(",\"vx\":", o); f(o, r.getVelocity().x); fputs(",\"vy\":", o); f(o, r.getVelocity().y);
+		fprintf(o, "},\"lives\":%d,\"kills\":%d,\"deaths\":%d,\"dirt\":%d}",
+			(int)w->getLives(), w->getKills(), w->getDeaths(), w->getDirtCount());
+	}
+	fputs("],\"p\":[", o);
+	first = true;
+	auto& ps = cClient->getProjectiles();
+	for(int i = 0; i <= ps.lastUsed(); i++) {
+		if(!ps.isUsed(i)) continue;
+		CProjectile& p = ps[i];
+		if(!first) fputc(',', o);
+		first = false;
+		fprintf(o, "{\"i\":%d,\"t\":%d,\"x\":", i, game.gameScript()->projectileIndex(p.getProjInfo()));
+		f(o, p.getPos().x); fputs(",\"y\":", o); f(o, p.getPos().y);
+		fputs(",\"vx\":", o); f(o, p.getVelocity().x); fputs(",\"vy\":", o); f(o, p.getVelocity().y);
+		fputs(",\"life\":", o); f(o, p.getLife());
+		fputc('}', o);
+	}
+	fputs("],\"b\":[", o);
+	first = true;
+	CBonus* bs = cClient->getBonusList();
+	for(int i = 0; i < MAX_BONUSES; i++) {
+		if(!bs[i].getUsed()) continue;
+		if(!first) fputc(',', o);
+		first = false;
+		fprintf(o, "{\"i\":%d,\"t\":%d,\"x\":", i, bs[i].getType());
+		f(o, bs[i].getPosition().x); fputs(",\"y\":", o); f(o, bs[i].getPosition().y);
+		fputc('}', o);
+	}
+	fprintf(o, "],\"mh\":\"%016llx\"}\n", materialHash());
+	fflush(o);
+}
+
+void afterFrame() {
+	if(g_rec) writeState(frameIndex());
+}
+
+}
+
+void CWorm::replaceInputHandlerForTest(CWormInputHandler* h) {
+	if(m_inputHandler) m_inputHandler->deleteThis();
+	m_inputHandler = h;
+	m_inputHandler->assignNetworkRole();
+}
diff --git a/src/game/CWorm.h b/src/game/CWorm.h
index ede70ca..43dafff 100644
--- a/src/game/CWorm.h
+++ b/src/game/CWorm.h
@@ -47,6 +47,7 @@
 #include <vector>
 
 
+namespace OracleHooks { struct Access; }
 class CWormInputHandler;
 class Weapon;
 class WeaponType;
@@ -166,6 +167,7 @@ class CWorm: public CGameObject {
 	friend class Client;
 	friend class Game;
 	friend class PhysicsEngine;
+friend struct OracleHooks::Access; // test hook reads fAngleSpeed
 public:
 	CWorm();
 	~CWorm();
@@ -621,6 +623,8 @@ public:
 	float&		frame()						{ return fFrame; }
 	
 	CWormInputHandler* inputHandler() { return m_inputHandler; }
+	// Test hook (OracleHooks): drive this worm with another input handler.
+	void replaceInputHandlerForTest(CWormInputHandler* h);
 	void reinitInputHandler();
 	
 	std::map< int, DamageReport> & getDamageReport() { return cDamageReport; }
diff --git a/src/game/Game.cpp b/src/game/Game.cpp
index 8603bbd..82a969c 100644
--- a/src/game/Game.cpp
+++ b/src/game/Game.cpp
@@ -8,6 +8,7 @@
  */
 
 #include "Game.h"
+#include "OracleHooks.h"
 #include "AuxLib.h"
 #include "Debug.h"
 #include "LieroX.h"
@@ -948,9 +949,11 @@ void Game::frameInner()
 				gusLogicFrame();
 			}
 
+			OracleHooks::beforeFrame();
 			cClient->Frame();
 			if(isServer())
 				cServer->Frame();
+			OracleHooks::afterFrame();
 
 			if(manualStep) manualStepBudget--;
 			simulationTime += frameDt;
```

- [ ] **Step 3: Create `tools/olx-oracle/oracle_control.py`**

```text
#!/usr/bin/env python3
"""Records one rust-liero oracle scenario in a dedicated OpenLieroX.

OLX runs this script with `-dedicated -script` and talks to it over the
line protocol of tests/headless/control/_olx_pipe.py: we print a console
command, OLX answers with ":arg" lines and a lone ".".

The scenario comes from the JSON file named by ORACLE_SCENARIO, written by
`cargo xtask oracle-record`:
  level, mod, seed, frames, settings {setvar name: value}, worms [{x, y}],
  inputs [[frame, worm index, key mask]], out (trace path).
The whole run is driven by simStep, so no frame depends on the wall clock.
"""

import json
import os
import sys


def emit(msg):
    sys.stderr.write(msg + "\n")
    sys.stderr.flush()


def command(cmd):
    sys.stdout.write(cmd + "\n")
    sys.stdout.flush()
    args = []
    while True:
        line = sys.stdin.readline()
        if line == "":
            raise SystemExit(0)
        line = line.rstrip("\n")
        if line == ".":
            return args
        if line.startswith(":"):
            args.append(line[1:])


def state():
    res = command("getGameState")
    return res[0] if res else "?"


def frame():
    return int(command("getFrame")[0])


def step_until(cond, limit, what):
    for _ in range(limit):
        if cond():
            return
        command("simStep 1")
    if not cond():
        emit("ORACLE_ERROR timeout waiting for " + what)
        raise SystemExit(1)


def main():
    sc = json.load(open(os.environ["ORACLE_SCENARIO"]))
    settings = {
        "GameOptions.Network.RegisterServer": 0,
        "GameOptions.Network.UseIpToCountry": 0,
        "GameOptions.GameInfo.GameType": "Death Match",
        "GameOptions.GameInfo.LevelName": sc["level"],
        "GameOptions.GameInfo.ModName": sc["mod"],
        "GameOptions.GameInfo.ImmediateStart": 1,
        "GameOptions.GameInfo.AllowEmptyGames": 1,
        "GameOptions.GameInfo.ForceRandomWeapons": 1,
        # LX56 aim speed and acceleration, independent of client options.
        "GameOptions.GameInfo.ForceLX56Aim": 1,
    }
    settings.update(sc.get("settings", {}))
    for key, value in settings.items():
        command('setvar %s "%s"' % (key, value))
    command("simSetSeed %d" % sc["seed"])
    command("simStep 0")  # manual stepping from here on
    command("startlobby 23400")
    command("addBots %d" % len(sc["worms"]))
    command("startgame")
    step_until(lambda: state() == "Playing", 2000, "Playing")
    ids = command("getWormList")
    if len(ids) != len(sc["worms"]):
        emit("ORACLE_ERROR expected %d worms, got %s" % (len(sc["worms"]), ids))
        raise SystemExit(1)
    schedule = os.path.join(os.path.dirname(sc["out"]), "inputs.txt")
    with open(schedule, "w") as fh:
        for frame_no, worm, keys in sc["inputs"]:
            fh.write("%d %s %d\n" % (frame_no, ids[worm], keys))
    command("simSetSeed %d" % sc["seed"])
    command("simLoadInputs " + schedule)  # also aligns the physics clock
    for wid, w in zip(ids, sc["worms"]):
        command('spawnWorm %s "%d,%d"' % (wid, w["x"], w["y"]))
    command("simRecord " + sc["out"])
    target = frame() + sc["frames"]
    command("simStep %d" % sc["frames"])
    step_until(lambda: frame() >= target, 10, "last frame")
    command("simRecord off")
    emit("ORACLE_DONE frames=%d" % sc["frames"])
    command("quit")


main()
```

- [ ] **Step 4: Create `tools/olx-oracle/CLAUDE.md`**

```markdown
# tools/olx-oracle

A patched OpenLieroX, built in Docker, that records reference traces for the
fidelity oracle (spec §6). `cargo xtask oracle-record` builds it and runs it;
you rarely need to touch these files.

## Files

- `Dockerfile`: OLX at commit `d1ad953f` from GitHub, plus the patch, built as a
  headless dedicated server (Ubuntu 22.04, as OLX's own Docker image).
  The image tag is a hash of the three files here, so any edit rebuilds it.
- `oracle-hooks.patch`: the test hooks, 8 OLX files and about 320 lines.
- `oracle_control.py`: drives one recording over OLX's dedicated-control pipe.

## What the patch adds

- **`simLoadInputs <file>`:** loads `frame worm keys` lines (key bits in `OracleHooks.h`).
  - The listed worms switch to OLX's human input handler, driven by virtual keys
    (`CInput::virt`), so aim, facing, rope and weapon switching run through
    OLX's real keyboard code.
  - A key press is latched until the first `getInput()` consumes it, as
    `CInput::nDownOnce` does. `getInput()` runs once per 84 Hz physics
    sub-step, not once per 100 Hz frame.
  - It also aligns the LX56 physics clock with the frame clock. Without that,
    frame 0 would replay however much time the setup took.
- **`simRecord <file|off>`:** writes one JSON line per simulated frame.
  - First a header with the OLX version and every feature value, then frame −1
    (the state when recording starts), then frames 0..n.
  - Floats use `%.9g`.
- **Game loop hooks** (`Game::frameInner`): `OracleHooks::beforeFrame()` and
  `afterFrame()` around each simulated frame.

## The recording sequence (`oracle_control.py`)

1. **Settings.** It forces LX56 aim (`ForceLX56Aim`), so traces don't depend on client options.
2. **Manual stepping from the lobby on.** `simSetSeed`, `simStep 0`, then
   `startlobby`, `addBots`, `startgame`, stepping until `Playing`. No frame
   depends on the wall clock.
3. **Inputs, then spawns.** `simSetSeed` again, `simLoadInputs`, then
   `spawnWorm` for each worm (after the clock alignment), then `simRecord` and
   `simStep frames`.

## Changing the patch

1. Edit an OLX checkout at commit `d1ad953f`.
2. Regenerate the patch: `git add -N <new files> && git diff > oracle-hooks.patch`.
3. Run `cargo xtask oracle-record --check`. If behaviour is meant to stay the
   same, every committed trace must still match.
```

- [ ] **Step 5: Run**

```bash
chmod 755 tools/olx-oracle/oracle_control.py && docker version --format "{{.Server.Version}}"
```

Expected: output contains `.`. The control script must be executable (the image also sets this), and the Docker daemon must answer.

- [ ] **Step 6: Create `xtask/Cargo.toml`**

```toml
[package]
name = "xtask"
description = "Developer workflows: cargo xtask check | layers | api | fetch-olx-content | oracle-record | install-hooks."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
anyhow.workspace = true
clap.workspace = true
cargo_metadata.workspace = true
serde = { workspace = true, features = ["std"] }
ron.workspace = true
syn.workspace = true
prettyplease.workspace = true
quote.workspace = true
rl-oracle.workspace = true
sha2.workspace = true
hex.workspace = true

[lints]
workspace = true
```

- [ ] **Step 7: Create `xtask/src/oracle.rs`**

```rust
//! `cargo xtask oracle-record`: records OLX reference traces for the scenarios
//! in `fixtures/oracle/*.ron` with the patched OpenLieroX in `tools/olx-oracle/`
//! (needs Docker). Each scenario is recorded twice and must come out identical,
//! which proves the recording is deterministic. `--check` compares with the
//! committed traces instead of writing them.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use rl_oracle::{Scenario, Trace};
use sha2::{Digest, Sha256};

use crate::root;

const TOOL_DIR: &str = "tools/olx-oracle";
const FIXTURES: &str = "fixtures/oracle";
const TOOL_FILES: [&str; 3] = ["Dockerfile", "oracle-hooks.patch", "oracle_control.py"];
/// Kills a hung OLX after this many seconds.
const RUN_TIMEOUT_SECS: u32 = 300;

/// Docker image tag derived from the oracle's input files, so editing any of
/// them builds a new image instead of silently reusing a stale one.
pub fn image_tag(files: &[(&str, &[u8])]) -> String {
    let mut h = Sha256::new();
    for (name, data) in files {
        h.update(name.as_bytes());
        h.update([0u8]);
        h.update((data.len() as u64).to_le_bytes());
        h.update(data);
    }
    format!("rl-olx-oracle:{}", &hex::encode(h.finalize())[..12])
}

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn ensure_image(root: &Path) -> anyhow::Result<String> {
    let mut files = Vec::new();
    for f in TOOL_FILES {
        let path = root.join(TOOL_DIR).join(f);
        files.push((f, std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?));
    }
    let tag = image_tag(&files.iter().map(|(n, d)| (*n, d.as_slice())).collect::<Vec<_>>());
    let exists = Command::new("docker")
        .args(["image", "inspect", &tag])
        .output()
        .context("running docker (is Docker installed and running?)")?
        .status
        .success();
    if !exists {
        println!("building {tag} (OLX from source; takes a few minutes the first time)");
        let out = Command::new("docker").args(["build", "-t", &tag]).arg(root.join(TOOL_DIR)).output()?;
        if !out.status.success() {
            bail!("docker build failed:\n{}", tail(&String::from_utf8_lossy(&out.stderr), 40));
        }
    }
    Ok(tag)
}

fn id(flag: &str) -> anyhow::Result<String> {
    let out = Command::new("id").arg(flag).output().context("running id")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Plays `sc` once in OLX and returns the trace text.
fn record_once(image: &str, user: &str, work: &Path, sc: &Scenario) -> anyhow::Result<String> {
    if work.exists() {
        std::fs::remove_dir_all(work)?;
    }
    std::fs::create_dir_all(work.join("home"))?;
    std::fs::write(work.join("scenario.json"), sc.control_json("/work/trace.jsonl"))?;
    let out = Command::new("docker")
        .args(["run", "--rm", "--init", "-u", user, "-v"])
        .arg(format!("{}:/work", work.display()))
        .args(["-e", "HOME=/work/home", "-e", "ORACLE_SCENARIO=/work/scenario.json", image])
        .args(["timeout", "-s", "KILL", &RUN_TIMEOUT_SECS.to_string(), "/olx/bin/openlierox"])
        .args(["-dedicated", "-disablestdincli", "-script", "/olx/oracle_control.py"])
        .output()
        .context("running docker")?;
    let log = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    std::fs::write(work.join("olx.log"), &log)?;
    if !log.contains("ORACLE_DONE") {
        let errors: Vec<&str> =
            log.lines().filter(|l| l.contains("ORACLE_ERROR") || l.starts_with("E:")).collect();
        bail!("OLX did not finish the scenario:\n{}\n...\n{}", errors.join("\n"), tail(&log, 20));
    }
    std::fs::read_to_string(work.join("trace.jsonl")).context("OLX wrote no trace")
}

/// Scenario files in `fixtures/oracle/`, sorted, optionally only `names`.
fn scenarios(root: &Path, names: &[String]) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut all = Vec::new();
    for e in std::fs::read_dir(root.join(FIXTURES)).context("reading fixtures/oracle")? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "ron") {
            let stem = p.file_stem().expect("file name").to_string_lossy().to_string();
            all.push((stem, p));
        }
    }
    all.sort();
    for n in names {
        if !all.iter().any(|(s, _)| s == n) {
            bail!("no scenario fixtures/oracle/{n}.ron");
        }
    }
    Ok(all.into_iter().filter(|(s, _)| names.is_empty() || names.contains(s)).collect())
}

pub fn run(names: &[String], check: bool) -> anyhow::Result<bool> {
    let root = root();
    let list = scenarios(&root, names)?;
    let image = ensure_image(&root)?;
    let user = format!("{}:{}", id("-u")?, id("-g")?);
    let mut ok = true;
    for (name, path) in list {
        let sc = Scenario::from_ron(&std::fs::read_to_string(&path)?)
            .with_context(|| format!("{}", path.display()))?;
        let work = root.join("target/oracle").join(&name);
        let a = record_once(&image, &user, &work.join("a"), &sc).with_context(|| name.clone())?;
        let b = record_once(&image, &user, &work.join("b"), &sc).with_context(|| name.clone())?;
        let (ta, tb) = (Trace::from_jsonl(&a)?, Trace::from_jsonl(&b)?);
        if a != b {
            let why =
                rl_oracle::compare(&ta, &tb, &[]).err().map_or("header differs".into(), |d| d.to_string());
            println!("FAIL {name}: two recordings differ, OLX is not deterministic here: {why}");
            ok = false;
            continue;
        }
        if ta.frames.len() != sc.frames as usize + 1 {
            println!(
                "FAIL {name}: expected {} frames (-1..{}), got {}",
                sc.frames + 1,
                sc.frames - 1,
                ta.frames.len()
            );
            ok = false;
            continue;
        }
        let gz_path = root.join(FIXTURES).join(format!("{name}.jsonl.gz"));
        if check {
            let committed = match std::fs::read(&gz_path) {
                Ok(bytes) => Trace::from_gz(&bytes)?,
                Err(_) => {
                    println!("FAIL {name}: no committed trace {}", gz_path.display());
                    ok = false;
                    continue;
                }
            };
            match rl_oracle::compare(&committed, &ta, &[]) {
                Ok(n) if committed.header == ta.header => {
                    println!("ok   {name}: matches the committed trace ({n} frames)")
                }
                Ok(_) => {
                    println!(
                        "FAIL {name}: header (OLX version or settings) differs from the committed trace"
                    );
                    ok = false;
                }
                Err(d) => {
                    println!("FAIL {name}: {d}");
                    ok = false;
                }
            }
        } else {
            std::fs::write(&gz_path, rl_oracle::gzip(&a))?;
            println!("ok   {name}: recorded {} frames", ta.frames.len());
        }
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_tag_depends_on_names_and_contents() {
        let t = image_tag(&[("a", b"1"), ("b", b"2")]);
        assert!(t.starts_with("rl-olx-oracle:") && t.len() == "rl-olx-oracle:".len() + 12, "{t}");
        assert_eq!(t, image_tag(&[("a", b"1"), ("b", b"2")]));
        assert_ne!(t, image_tag(&[("a", b"1"), ("b", b"3")]));
        assert_ne!(t, image_tag(&[("a", b"12"), ("b", b"")]));
    }

    #[test]
    fn every_scenario_is_listed_and_unknown_names_fail() {
        let all = scenarios(&root(), &[]).unwrap();
        assert!(!all.is_empty());
        assert!(scenarios(&root(), &["no-such-scenario".into()]).is_err());
    }
}
```

- [ ] **Step 8: Create `xtask/src/main.rs`**

```rust
//! Developer workflows. Run from anywhere in the workspace with `cargo xtask <command>`.

mod api;
mod check;
mod fetch;
mod layers;
mod oracle;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cargo xtask")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// The fast gate: fmt, clippy, layering, wasm build, OLX content, tests. Prints only failures.
    Check {
        /// Do not download OLX content; tests that need it are skipped.
        #[arg(long)]
        offline: bool,
    },
    /// Check crate layering rules from xtask/layers.ron.
    Layers,
    /// Write public API summaries to target/api/<crate>.txt.
    Api,
    /// Download the pinned OLX content and import it into target/content.
    FetchOlxContent {
        /// Only download; print the game directory path and skip importing.
        #[arg(long)]
        no_import: bool,
    },
    /// Install a git pre-push hook that runs `cargo xtask check`.
    InstallHooks,
    /// Record OLX reference traces for fixtures/oracle/*.ron (needs Docker).
    OracleRecord {
        /// Re-record and compare with the committed traces instead of writing them.
        #[arg(long)]
        check: bool,
        /// Scenario names (file stems in fixtures/oracle/); all if none.
        names: Vec<String>,
    },
}

/// The workspace root (the parent of this crate's directory).
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the workspace").to_path_buf()
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Check { offline } => check::run(offline),
        Command::Layers => layers::run().map(|()| true),
        Command::Api => api::run().map(|()| true),
        Command::FetchOlxContent { no_import } => fetch::run(!no_import).map(|p| {
            println!("RL_OLX_GAMEDIR={}", p.gamedir.display());
            if !no_import {
                println!("RL_CONTENT_DIR={}", p.content.display());
            }
            true
        }),
        Command::InstallHooks => install_hooks().map(|()| true),
        Command::OracleRecord { check, names } => oracle::run(&names, check),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn install_hooks() -> anyhow::Result<()> {
    let hook = root().join(".git/hooks/pre-push");
    std::fs::create_dir_all(hook.parent().expect("hooks dir"))?;
    std::fs::write(&hook, "#!/bin/sh\nexec cargo xtask check\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
    }
    println!("installed {}", hook.display());
    Ok(())
}
```

- [ ] **Step 9: Create `fixtures/oracle/fall.ron`**

```ron
(
    description: "A worm dropped in the air onto Dirt Level falls and lands. Gravity, ground contact. Same start as OLX's own tests/headless/test_physics.py.",
    level: "Dirt Level.lxl",
    mod: "Classic",
    seed: 12345,
    frames: 200,
    worms: [(x: 456, y: 212)],
)
```

- [ ] **Step 10: Run**

```bash
cargo nextest run -p xtask
```

Expected: output contains `6 passed`. `image_tag_depends_on_names_and_contents` guards against reusing a stale image.

- [ ] **Step 11: Run**

```bash
cargo xtask oracle-record fall
```

Expected: output contains `ok   fall: recorded 201 frames`. The first run builds the image (`building rl-olx-oracle:...`, a few minutes), then records `fall` twice and writes `fixtures/oracle/fall.jsonl.gz`.

- [ ] **Step 12: Run**

```bash
cargo xtask oracle-record --check fall
```

Expected: output contains `ok   fall: matches the committed trace (201 frames)`.

- [ ] **Step 13: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 14: Commit**

```bash
git add -A
git commit -m "Add the patched OLX oracle and xtask oracle-record"
```


---

### Task 4: Movement scenarios, recorded traces and fixture checks

Records the five remaining M2 scenarios and adds a test that keeps the committed traces consistent with their scenarios and meaningful: each scenario must keep showing the behaviour it claims to cover.

**Files:**

- Create: `fixtures/oracle/{walk,jump,aim,rope,dig}.ron` and their recorded `.jsonl.gz`
- Create: `crates/rl-oracle/tests/fixtures.rs`
- Modify: `docs/porting/STATUS.md`, `CLAUDE.md` (oracle section)

**Interfaces:**

- Consumes: `cargo xtask oracle-record` (Task 3), `Scenario` and `Trace` (Tasks 1–2).
- Produces: six committed scenario + trace pairs. These are the M3 acceptance tests for the simulation port.

- [ ] **Step 1: Create `fixtures/oracle/walk.ron`**

```ron
(
    description: "On Liero Factory's open rock floor: walk right for a second, stop, walk left for a second, stop. Walking speed, friction, facing and move direction.",
    level: "Liero Factory.lxl",
    mod: "Classic",
    seed: 12345,
    frames: 330,
    worms: [(x: 190, y: 320)],
    inputs: [
        (frame: 60, worm: 0, keys: "right"),
        (frame: 160, worm: 0, keys: ""),
        (frame: 170, worm: 0, keys: "left"),
        (frame: 270, worm: 0, keys: ""),
    ],
)
```

- [ ] **Step 2: Create `fixtures/oracle/jump.ron`**

```ron
(
    description: "Jumps while walking right, while standing, and while walking left. Jump force, air control, landing. Jump is a 2-frame tap.",
    level: "Liero Factory.lxl",
    mod: "Classic",
    seed: 12345,
    frames: 300,
    worms: [(x: 190, y: 320)],
    inputs: [
        (frame: 50, worm: 0, keys: "right"),
        (frame: 60, worm: 0, keys: "right+jump"),
        (frame: 62, worm: 0, keys: "right"),
        (frame: 120, worm: 0, keys: "right+jump"),
        (frame: 122, worm: 0, keys: ""),
        (frame: 180, worm: 0, keys: "left+jump"),
        (frame: 182, worm: 0, keys: "left"),
        (frame: 240, worm: 0, keys: ""),
    ],
)
```

- [ ] **Step 3: Create `fixtures/oracle/aim.ron`**

```ron
(
    description: "Aim up to the -90 degree limit, down to the 60 degree limit, then turn around by tapping left and right. LX56 aim speed, acceleration, slow-down and clamping.",
    level: "Liero Factory.lxl",
    mod: "Classic",
    seed: 12345,
    frames: 260,
    worms: [(x: 190, y: 320)],
    inputs: [
        (frame: 40, worm: 0, keys: "up"),
        (frame: 100, worm: 0, keys: "down"),
        (frame: 200, worm: 0, keys: "left"),
        (frame: 203, worm: 0, keys: ""),
        (frame: 220, worm: 0, keys: "right"),
        (frame: 223, worm: 0, keys: "up"),
        (frame: 240, worm: 0, keys: ""),
    ],
)
```

- [ ] **Step 4: Create `fixtures/oracle/rope.ron`**

```ron
(
    description: "Aim up, fire the ninja rope, swing right then left while hooked, then release it with a jump. Hook flight, attaching, rope pull, release.",
    level: "Liero Factory.lxl",
    mod: "Classic",
    seed: 12345,
    frames: 400,
    worms: [(x: 190, y: 320)],
    inputs: [
        (frame: 40, worm: 0, keys: "up"),
        (frame: 70, worm: 0, keys: ""),
        (frame: 80, worm: 0, keys: "rope"),
        (frame: 82, worm: 0, keys: ""),
        (frame: 150, worm: 0, keys: "right"),
        (frame: 200, worm: 0, keys: "left"),
        (frame: 260, worm: 0, keys: ""),
        (frame: 330, worm: 0, keys: "jump"),
        (frame: 332, worm: 0, keys: ""),
    ],
)
```

- [ ] **Step 5: Create `fixtures/oracle/dig.ron`**

```ron
(
    description: "In Dirt Level's solid dirt: hold right and tap left to dig to the right, then hold left and tap right to dig back. Digging changes the material map (mh).",
    level: "Dirt Level.lxl",
    mod: "Classic",
    seed: 12345,
    frames: 360,
    worms: [(x: 456, y: 212)],
    inputs: [
        (frame: 80, worm: 0, keys: "right"),
        (frame: 90, worm: 0, keys: "right+left"),
        (frame: 91, worm: 0, keys: "right"),
        (frame: 110, worm: 0, keys: "right+left"),
        (frame: 111, worm: 0, keys: "right"),
        (frame: 130, worm: 0, keys: "right+left"),
        (frame: 131, worm: 0, keys: "right"),
        (frame: 150, worm: 0, keys: "right+left"),
        (frame: 151, worm: 0, keys: "right"),
        (frame: 200, worm: 0, keys: "left"),
        (frame: 210, worm: 0, keys: "left+right"),
        (frame: 211, worm: 0, keys: "left"),
        (frame: 230, worm: 0, keys: "left+right"),
        (frame: 231, worm: 0, keys: "left"),
        (frame: 260, worm: 0, keys: ""),
    ],
)
```

- [ ] **Step 6: Create `crates/rl-oracle/tests/fixtures.rs`**

```rust
//! The committed oracle fixtures (`fixtures/oracle/`) are complete, consistent
//! with their scenarios, and still show the behaviour each scenario describes.
//! A failure here usually means a trace must be re-recorded with
//! `cargo xtask oracle-record <name>` (needs Docker).

use std::path::PathBuf;

use rl_oracle::{Scenario, Trace};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle")
}

fn load(name: &str) -> (Scenario, Trace) {
    let sc =
        Scenario::from_ron(&std::fs::read_to_string(dir().join(format!("{name}.ron"))).unwrap()).unwrap();
    let gz = std::fs::read(dir().join(format!("{name}.jsonl.gz")))
        .unwrap_or_else(|_| panic!("missing trace; run `cargo xtask oracle-record {name}`"));
    (sc, Trace::from_gz(&gz).unwrap())
}

#[test]
fn every_scenario_has_a_consistent_trace() {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["aim", "dig", "fall", "jump", "rope", "walk"]);
    for name in names {
        let (sc, t) = load(&name);
        assert_eq!(t.header.features["ForceLX56Aim"], "true", "{name}");
        assert_eq!(t.header.features["LX56PhysicsFPS"], "84", "{name}");
        assert_eq!(t.frames.len(), sc.frames as usize + 1, "{name}: frames -1..{}", sc.frames - 1);
        for (i, f) in t.frames.iter().enumerate() {
            assert_eq!(f.f, i as i64 - 1, "{name}");
            assert_eq!(f.w.len(), sc.worms.len(), "{name} frame {}", f.f);
        }
        let start = &t.frames[0].w[0];
        assert_eq!((start.x.0, start.y.0), (sc.worms[0].x as f32, sc.worms[0].y as f32), "{name}: spawn");
    }
}

fn worm(t: &Trace) -> impl Iterator<Item = &rl_oracle::WormRec> {
    t.frames.iter().map(|f| &f.w[0])
}

#[test]
fn fall_lands() {
    let (_, t) = load("fall");
    assert!(!t.frames[0].w[0].ground && t.frames.last().unwrap().w[0].ground);
}

#[test]
fn walk_moves_both_ways() {
    let (_, t) = load("walk");
    let xs: Vec<f32> = worm(&t).map(|w| w.x.0).collect();
    let max = xs.iter().cloned().fold(f32::MIN, f32::max);
    assert!(max - xs[0] > 30.0, "walked right {}", max - xs[0]);
    assert!(max - xs.last().unwrap() > 30.0, "walked back left");
    assert!(worm(&t).any(|w| w.face == 0) && worm(&t).any(|w| w.face == 1), "faced both ways");
}

#[test]
fn jump_leaves_the_ground() {
    let (sc, t) = load("jump");
    let lowest_y = worm(&t).map(|w| w.y.0).fold(f32::MAX, f32::min);
    assert!(lowest_y < sc.worms[0].y as f32 - 5.0, "peak y {lowest_y}");
}

#[test]
fn aim_reaches_both_limits() {
    let (_, t) = load("aim");
    assert!(worm(&t).any(|w| w.ang.0 == -90.0) && worm(&t).any(|w| w.ang.0 == 60.0));
}

#[test]
fn rope_hooks_and_releases() {
    let (_, t) = load("rope");
    let attached = worm(&t).filter(|w| w.rope.at).count();
    assert!(attached > 100, "attached for {attached} frames");
    assert!(!t.frames.last().unwrap().w[0].rope.at, "released at the end");
}

#[test]
fn dig_changes_the_material_map() {
    let (_, t) = load("dig");
    let hashes: std::collections::BTreeSet<&str> = t.frames.iter().map(|f| f.mh.as_str()).collect();
    assert!(hashes.len() > 3, "material changed {} times", hashes.len() - 1);
}
```

- [ ] **Step 7: Run**

```bash
cargo nextest run -p rl-oracle --test fixtures
```

 Expected to FAIL: only `fall` has a recorded trace so far. The message says `run cargo xtask oracle-record <name>`.

- [ ] **Step 8: Run**

```bash
cargo xtask oracle-record
```

Expected: output contains `ok   walk: recorded 331 frames`. All six record, each twice and identically: `aim` 261, `dig` 361, `fall` 201, `jump` 301, `rope` 401, `walk` 331 frames.

- [ ] **Step 9: Run**

```bash
cargo nextest run -p rl-oracle
```

Expected: output contains `27 passed`. The fixture tests check that `walk` moves more than 30 px both ways, `jump` rises more than 5 px, `aim` reaches −90° and 60°, `rope` stays attached for more than 100 frames, and `dig` changes the material map.

- [ ] **Step 10: Create `docs/porting/STATUS.md`**

```markdown
# Porting status

What has been ported from OpenLieroX (commit `d1ad953f`) and how it is verified.

| Area | OLX source | Rust | Status | Verified by |
|---|---|---|---|---|
| Gamescript v7 loader | `src/common/CGameScript.cpp:582-1152` | `rl-import-lgs` | ported | unit and property tests; Classic and MW 1.0 import exactly |
| `.lxl` image levels | `src/common/MapLoader_LieroX.cpp` | `rl-import-lxl` | ported | unit tests; 4 levels in scope, dirt counts match |
| Skin colorization | `src/client/CGameSkin.cpp:720-790` | `rl-data::skin`, `rl-import-gfx` | ported | unit tests; all 409 skins import |
| Theme loading | `src/common/CMap.cpp:308-370` | `rl-import-gfx` | ported | Dirt theme imports |
| Gamescript v8 | `CGameScript.cpp` (v8 branches) | — | not started | no content in scope |
| Pixmap `.lxl` | `MapLoader_LieroX.cpp:440+` | — | not started | no content in scope |
| Fidelity oracle (test hooks) | `src/game/Game.cpp`, `src/client/CInput.cpp`, `src/common/CWormHuman.cpp` | `tools/olx-oracle`, `rl-oracle` | done (plan 2) | 6 movement scenarios recorded twice and identical; `cargo xtask oracle-record --check` |
| Simulation | `src/common/PhysicsLX56*.cpp` | — | not started (plan 3) | the 6 oracle scenarios |

## Deliberate deviations

- Importers reject malformed input that OLX would read as garbage: truncated
  files, unknown weapon or projectile types, and nesting deeper than 128 levels.
- A missing projectile image is an import error. OLX would load the mod and draw nothing.
- Imported level art is stored at 1×, because OLX's 2× surfaces are plain pixel doubling.

## Kept OLX quirks

- `ROF /= 1000` and `Recharge /= 10` are applied after loading (`CGameScript.cpp:776-777`).
- Beam and pixel colors stored as ints are truncated to bytes, as with OLX's `Color(Uint8, …)`.
- Front pixels where the `PX_EMPTY` flag is set become the back pixel, even when other flags are set.
- **Colour key with a corner rule.** OLX uses (254,0,254) as the colour key when
  any corner pixel of the image has that colour, otherwise (255,0,255)
  (`SetColorKey`, `src/client/GfxPrimitives.cpp:116-150`).
  - Skins: applied at import.
  - Mod sprites and theme images: still stored raw, so the renderer must apply
    the same rule (plan 5). One affected file in scope is `skins/Wario.png`.
- **Hi-res level art.** With an "OLX hi-res data" chunk, the front 2×2 block
  becomes the back block wherever `PX_EMPTY` is set, and PNG alpha is ignored
  (`LoadLevelImageHiRes`, `MapLoader_LieroX.cpp:285-360`).

## Facts the oracle established (needed by the simulation port)

- **Frames:** a frame is 10 ms. LX56 physics runs in 84 Hz sub-steps from its
  own clock (`CClient::fLastSimulationTime`), so some 100 Hz frames run no
  physics sub-step at all (in `fall`, frame 0 does not move the worm). The
  oracle aligns that clock with the frame clock when a scenario starts.
- **Input:** OLX's keyboard handler (`CWormHumanInputHandler::getInput`,
  `src/common/CWormHuman.cpp:81-560`) runs once per physics sub-step.
  - Held keys are read directly.
  - "Pressed once" events are latched until the next sub-step consumes them
    (`CInput::nDownOnce`, reset by `CInput::reset()`).
  - The simulation's `PlayerInput` must keep the same latch semantics.
- **Digging** with the keyboard is a tap of the opposite direction while
  holding a direction (`leftOnce` / `rightOnce`), not a dedicated key.
- **Aim:** the traces use LX56 aim (`ForceLX56Aim`: max speed 100,
  acceleration 500, no friction, `REDUCE_CONST(200)` when no key is held). The
  angle is clamped to [−90°, 60°].
```

- [ ] **Step 11: Create `CLAUDE.md`**

```markdown
# rust-liero

A Rust reimplementation of OpenLieroX (OLX) that feels identical and is
modernized for current displays.

- **Design:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`.
- **Plans:** `docs/superpowers/plans/`.
- **Porting status:** `docs/porting/STATUS.md`.

## Rules

1. **Run `cargo xtask check` before every commit.** It prints only failures.
   Without network access, use `cargo xtask check --offline`.
2. **One crate per concern, under `crates/`.** Each crate has a `CLAUDE.md`:
   read it before editing the crate. Register every new crate in `xtask/layers.ron`.
   A crate may depend only on lower layers (`cargo xtask layers`).
3. **Learn a dependency's API from its summary.** Run `cargo xtask api`, then
   read `target/api/<crate>.txt` instead of the crate's source.
4. **Cite OLX for ported behavior.** Ported code gets a doc comment naming the
   OLX source, such as `/// OLX: src/common/PhysicsLX56.cpp:272-469`, at
   commit `d1ad953f`. Faithfulness beats elegance; record deliberate deviations
   in `docs/porting/STATUS.md`.
5. **Keep the simulation pure.** Simulation-layer crates are `#![no_std]` and
   must also build for `wasm32-unknown-unknown`.
6. **Never panic on content.** Inputs from mods, levels and packs are untrusted.
7. **Keep output deterministic.** Use no `HashMap` iteration in output, and no wall clock in logic.
8. **Commits:** small, imperative subject (`rl-pack: validate projectile cycles`).

## Oracle

`fixtures/oracle/<name>.ron` are scenarios, and `<name>.jsonl.gz` are their
traces recorded from a patched OLX (`tools/olx-oracle/`).
- `cargo xtask oracle-record [name...]` re-records them; it needs Docker.
- `--check` compares fresh recordings with the committed traces.
- The simulation must reproduce the traces bit-exactly (crate `rl-oracle`).

## Content

`cargo xtask fetch-olx-content` sparse-clones OLX at the commit pinned in
`content/olx-manifest.ron` into `target/olx/<commit>/`, then imports the
listed items into `target/content/`. Converted assets are never committed.
```

- [ ] **Step 12: Run**

```bash
cargo xtask check
```

Expected: output contains `check passed`.

- [ ] **Step 13: Run**

```bash
cargo xtask oracle-record --check
```

Expected: output contains `ok   walk: matches the committed trace (331 frames)`. All six match.

- [ ] **Step 14: Commit**

```bash
git add -A
git commit -m "Add movement scenarios and their OLX traces"
```
