# Plan 1: Workspace and OLX Content Import, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the rust-liero workspace and its `cargo xtask check` gate, the native content model and pack format, and importers. Together they turn the pinned OpenLieroX content in scope into validated native packs: Liero 1.0, Modern Warfare 1.0, four levels, all skins and the Dirt theme.

**Architecture:**
- **Layers.** One Cargo workspace of small layered crates.
  - Layer 1: `rl-data` (pure `no_std` data types) and `rl-vfs` (content I/O behind traits).
  - Layer 2: `rl-pack` (the on-disk format and validation) and three importers (`rl-import-lgs`, `rl-import-lxl`, `rl-import-gfx`), each a faithful port of the matching OLX loader.
  - Layer 6: `rl-cli` (the headless entry point).
- **Tooling.** `xtask` enforces the layering, fetches OLX content at a pinned commit through a sparse git checkout, and runs every gate locally.

**Tech stack:**
- Rust 1.99, edition 2024.
- Formats: `serde` and `ron` (text formats), `png`, `flate2` (`.lxl` zlib), `sha2` (content hash).
- Errors and CLI: `thiserror`, `anyhow`, `clap`.
- Tests: `proptest`, `insta`, `tempfile`, `cargo-nextest`.
- Tooling: `cargo_metadata`, `syn`, `prettyplease` and `quote` (API summaries).

**Spec:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`. Read its §15 amendments, which this plan implements. The plan covers milestones **M0 and M1**.

## How this plan was verified

Every file in this plan was compiled, linted and tested in a scratch workspace against the real OLX content at the pinned commit. The plan was then **replayed task by task in a fresh repository** by a script that writes exactly the code blocks below and runs exactly the commands below. Every `Expected:` line is observed output from that replay. The full run takes about 2.5 minutes on a warm toolchain.

If a step's output differs from `Expected:`, stop and investigate before changing anything. Do not edit the expectation to match.

## Conventions

- **Run every command from the repository root.**
- **Steps.** A step either creates or replaces a file with the exact content shown, or runs a command.
- **Test placement.** Rust unit tests live in the same file as the code (`#[cfg(test)] mod tests`), so a module and its unit tests arrive in one step.
- **Test-first for integration tests.** Where a task has integration tests against real content (Tasks 8–10) or the CLI (Task 11), the test file is written before the code.
- **Real-content tests skip themselves** when `RL_OLX_GAMEDIR` or `RL_CONTENT_DIR` is unset, and print a notice when they do. `cargo xtask check` (online) sets both variables.
- **`cargo xtask check --offline`** is the gate until Task 12. It skips the OLX download.

## Global Constraints

- Toolchain: Rust **1.99**, edition **2024**, pinned in `rust-toolchain.toml` (with `rustfmt`, `clippy` and the `wasm32-unknown-unknown` target).
- License: **GPL-3.0-or-later** for every crate (`license.workspace = true`). The `LICENSE` file holds the GPL-3.0 text.
- Crate names use the prefix **`rl-`**. Every crate has a `CLAUDE.md` and is registered in `xtask/layers.ron`. A crate may depend only on lower layers, or on same-layer pairs listed in `same_layer`.
- OLX source pinned at commit **`d1ad953f4f20610f2648cce26c9739e7c088409d`** of `https://github.com/openlierox/openlierox.git`.
- Content in scope (spec §1.2):
  - mods `Classic` ("Liero v1.0", 38 weapons) and `MW 1.0` ("Modern Warfare1.0", 50 weapons);
  - levels `levels/Liero Factory.lxl`, `levels/CastleStrike.lxl`, `levels/FossilFacility.lxl`, `levels/Dirt Level.lxl`;
  - all 409 `skins/*.png`;
  - the theme `data/themes/Dirt`.
- `rl-data` is `#![no_std]` + `alloc`. Every library crate builds for `wasm32-unknown-unknown`.
- Workspace lints: `unsafe_code = "forbid"`, `clippy::all`, and `cargo clippy -D warnings` must pass. `rustfmt` uses `max_width = 110` and `use_small_heuristics = "Max"`.
- **Deterministic output:** writing the same value twice gives byte-identical files. Use `BTreeMap`/`BTreeSet`, never `HashMap` iteration, in anything written.
- **Never panic on content.** Mods, levels, skins and packs are untrusted input.
- Ported behavior cites its OLX source (`file:line` at the pinned commit).
- Converted OLX assets are never committed; they live under `target/`.
- No hosted CI: `cargo xtask check` is the gate (spec §15, A6).
- Target size: under 500 lines per file.

## Review Focus

Conditions the spec implies but no feature test would naturally hit. Each is pinned by a named test in the task that owns the code.

1. **Re-import.** Re-importing into an output directory that holds an older import must not leave stale files in the pack. Pinned by `reimport_replaces_stale_files` (Task 11).
2. **NaN and ±infinity.** Mod files may hold these floats, and they must survive the RON round trip. Pinned by `non_finite_floats_survive_ron` (Task 3).
3. **Awkward weapon names.** Non-ASCII, punctuation-only or duplicated weapon names must still produce unique, file-safe ids. Pinned by `non_ascii_names_get_unique_ids` and `ids_names_and_dedup` (Task 8).
4. **Unsafe asset names.** OLX asset names with backslashes or `..` must never cause a read outside the game directory. Pinned by `rejects_escapes` and `from_olx_converts_backslashes_but_still_rejects_escapes` (Task 4), and `unsafe_asset_names_never_escape` (Task 8).
5. **Case-only duplicates.** Skin files whose names differ only by case (`Default.png` and `default.png` on Linux) must both import under distinct pack ids. Pinned by `import_then_validate` (Task 11).

## File structure after this plan

```text
Cargo.toml  rust-toolchain.toml  rustfmt.toml  .cargo/config.toml  .gitignore
LICENSE  README.md  CLAUDE.md  AGENTS.md
content/olx-manifest.ron            what to fetch from OLX, and at which commit
docs/porting/STATUS.md              ported / verified / deviations / kept quirks
xtask/                              check, layers, api, fetch-olx-content, install-hooks
  layers.ron                        crate layers, same-layer allow-list, no_std and wasm lists
crates/
  rl-data/       L1 no_std   content types: image, level, skin, theme, manifest, mod_def, slug
  rl-vfs/        L1          VPath, ContentSource/Sink, MemFs, DirFs, find_ci
  rl-pack/       L2          pack read/write per kind, image_io, content_hash, validate
  rl-import-lgs/ L2          script.lgs v7: reader, parse (bytes to tree), convert (tree to ModPack)
  rl-import-lxl/ L2          .lxl image levels (+ hi-res chunk)
  rl-import-gfx/ L2          skins (image + mask) and themes
  rl-cli/        L6          `import`, `validate`; corpus test + snapshot
```

## After this plan

The next plans are written once this one is done, because each depends on what the previous one teaches:

- **Plan 2:** the OLX oracle (M2): Docker build of OLX at the pinned commit, the hooks patch, the scenario format, recording, and first-divergence comparison.
- **Plan 3:** the core simulation (M3).
- **Plan 4:** weapons (M4).
- **Plan 5:** presentation (M5).
- **Plan 6:** game modes and bots (M6).
- **Plan 7:** menus and HUD, reaching v0.1 (M7).


---

### Task 1: Toolchain, workspace skeleton and `xtask`

Creates the Cargo workspace, pins the toolchain, and adds the `xtask` developer tool, the single gate every later task runs. `xtask` already contains every command this plan needs (`check`, `layers`, `api`, `fetch-olx-content`, `install-hooks`). Commands that need crates from later tasks are simply not run until then.

**Files:**

- Create: `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `rustfmt.toml`, `.gitignore`
- Create: `LICENSE` (GPL-3.0 text), `README.md`, `CLAUDE.md`, `AGENTS.md`
- Create: `content/olx-manifest.ron`
- Create: `xtask/Cargo.toml`, `xtask/layers.ron`, `xtask/src/{main,check,layers,fetch,api}.rs`

**Interfaces:**

- Produces: `cargo xtask check [--offline]` prints `ok|FAIL <step> (time)` per step and `check passed` or `check failed: <steps>`; exit status 0 only when every step passes.
- Produces: `cargo xtask layers`, which enforces `xtask/layers.ron`. Every crate must be listed there and have a `CLAUDE.md`.
- Produces: `cargo xtask fetch-olx-content [--no-import]`, which prints `RL_OLX_GAMEDIR=<path>` (and `RL_CONTENT_DIR=<path>`).
- Produces: `cargo xtask api`, which writes `target/api/<crate>.txt`.

- [ ] **Step 1: Install the toolchain (skip anything already installed).**

Install rustup, then cargo-nextest, the test runner `xtask check` prefers. `rust-toolchain.toml` (next step) makes rustup fetch Rust 1.99 with rustfmt, clippy and the `wasm32-unknown-unknown` target on first use.

```bash
command -v rustup || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
command -v cargo-nextest || curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C "${CARGO_HOME:-$HOME/.cargo}/bin"
git --version
```

Expected: every command succeeds, and `git` is 2.25 or newer (sparse checkout).

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[workspace]
resolver = "3"
members = ["xtask"]

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
serde = { version = "1.0.229", default-features = false, features = ["derive", "alloc"] }
ron = "0.12.2"
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

- [ ] **Step 2: Create `rust-toolchain.toml`**

```toml
[toolchain]
channel = "1.99"
components = ["rustfmt", "clippy"]
targets = ["wasm32-unknown-unknown"]
```

- [ ] **Step 3: Create `.cargo/config.toml`**

```toml
[alias]
xtask = "run --quiet --package xtask --"
```

- [ ] **Step 4: Create `rustfmt.toml`**

```toml
max_width = 110
use_small_heuristics = "Max"
```

- [ ] **Step 5: Create `.gitignore`**

```text
/target
```

- [ ] **Step 6: Run**

```bash
curl -sSfL https://www.gnu.org/licenses/gpl-3.0.txt -o LICENSE && head -2 LICENSE
```

Expected: output contains `GNU GENERAL PUBLIC LICENSE`. Download the license text.

- [ ] **Step 7: Create `README.md`**

````markdown
# rust-liero

A Rust reimplementation of [OpenLieroX](https://github.com/openlierox/openlierox),
built so physics, movement and shooting feel identical, and modernized for
today's displays. It loads classic OLX mods and levels by importing them into
an open, text-based pack format.

Status: early development. The current milestone imports OLX content.

## Quick start

```sh
cargo xtask check          # format, lint, layering, web build, OLX content, tests
cargo xtask fetch-olx-content
cargo run -p rl-cli -- validate target/content
```

## Credits

Based on OpenLieroX by Albert Zeyer, Karel Petránek and the OpenLieroX team,
which is based on Liero Xtreme by Jason Boettcher, which is inspired by Liero
by Joosa Riekkinen. Bundled OLX mods and levels belong to their authors.

## License

GPL-3.0-or-later. See `LICENSE`.
````

- [ ] **Step 8: Create `CLAUDE.md`**

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

## Content

`cargo xtask fetch-olx-content` sparse-clones OLX at the commit pinned in
`content/olx-manifest.ron` into `target/olx/<commit>/`, then imports the
listed items into `target/content/`. Converted assets are never committed.
```

- [ ] **Step 9: Create `AGENTS.md`**

```markdown
# AGENTS.md

Read `CLAUDE.md`; it applies to every agent. Per-crate notes are in `crates/*/CLAUDE.md`.
```

- [ ] **Step 10: Create `content/olx-manifest.ron`**

```ron
// OpenLieroX content imported by `cargo xtask fetch-olx-content` (spec §9.3).
// Paths are relative to `share/gamedir` at `commit`. Git verifies every file
// against the commit hash. Remove a line to stop bundling that item.
(
    repository: "https://github.com/openlierox/openlierox.git",
    commit: "d1ad953f4f20610f2648cce26c9739e7c088409d",
    shared: ["data/gfx", "data/sounds"],
    mods: ["Classic", "MW 1.0"],
    levels: [
        "levels/Liero Factory.lxl",
        "levels/CastleStrike.lxl",
        "levels/FossilFacility.lxl",
        "levels/Dirt Level.lxl",
    ],
    themes: ["data/themes/Dirt"],
    skins: true,
)
```

- [ ] **Step 11: Create `xtask/Cargo.toml`**

```toml
[package]
name = "xtask"
description = "Developer workflows: cargo xtask check | layers | api | fetch-olx-content | install-hooks."
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

[lints]
workspace = true
```

- [ ] **Step 12: Create `xtask/layers.ron`**

```ron
// Crate layering (spec §4). A crate may depend on crates in lower layers, and on
// same-layer crates only when the pair is listed in `same_layer`. Every
// workspace crate must be listed here. `no_std` crates must declare
// #![no_std]; `wasm` crates must build for wasm32-unknown-unknown.
(
    layers: {
        "xtask": 6,
    },
    same_layer: [],
    no_std: [],
    wasm: [],
)
```

- [ ] **Step 13: Create `xtask/src/layers.rs`**

```rust
//! Enforces the crate layering in `xtask/layers.ron` (spec §4).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Rules {
    pub layers: BTreeMap<String, u32>,
    pub same_layer: Vec<(String, String)>,
    pub no_std: Vec<String>,
    pub wasm: Vec<String>,
}

pub fn load_rules() -> anyhow::Result<Rules> {
    let path = crate::root().join("xtask/layers.ron");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// A workspace crate and the workspace crates it depends on (normal and build
/// dependencies; dev-dependencies are only used by tests and are not checked).
pub struct Krate {
    pub name: String,
    pub deps: Vec<String>,
    pub lib_rs: Option<String>,
    pub has_claude_md: bool,
}

/// Every rule violation, as human-readable lines.
pub fn violations(rules: &Rules, krates: &[Krate]) -> Vec<String> {
    let mut out = Vec::new();
    let same: BTreeSet<(&str, &str)> =
        rules.same_layer.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    for k in krates {
        let Some(&layer) = rules.layers.get(&k.name) else {
            out.push(format!("{}: not listed in xtask/layers.ron", k.name));
            continue;
        };
        if !k.has_claude_md && k.name != "xtask" {
            out.push(format!("{}: missing CLAUDE.md", k.name));
        }
        for d in &k.deps {
            let Some(&dl) = rules.layers.get(d) else { continue };
            let allowed = dl < layer || (dl == layer && same.contains(&(k.name.as_str(), d.as_str())));
            if !allowed {
                out.push(format!("{} (layer {layer}) must not depend on {d} (layer {dl})", k.name));
            }
        }
        if rules.no_std.contains(&k.name) && !k.lib_rs.as_deref().unwrap_or("").contains("#![no_std]") {
            out.push(format!("{}: listed as no_std but src/lib.rs lacks #![no_std]", k.name));
        }
    }
    out
}

pub fn workspace_krates() -> anyhow::Result<Vec<Krate>> {
    let meta =
        cargo_metadata::MetadataCommand::new().manifest_path(crate::root().join("Cargo.toml")).exec()?;
    let members: BTreeSet<String> = meta.workspace_packages().iter().map(|p| p.name.to_string()).collect();
    Ok(meta
        .workspace_packages()
        .iter()
        .map(|p| {
            let dir = p.manifest_path.parent().expect("manifest has a dir");
            Krate {
                name: p.name.to_string(),
                deps: p
                    .dependencies
                    .iter()
                    .filter(|d| d.kind != cargo_metadata::DependencyKind::Development)
                    .filter(|d| members.contains(&d.name))
                    .map(|d| d.name.clone())
                    .collect(),
                lib_rs: std::fs::read_to_string(dir.join("src/lib.rs")).ok(),
                has_claude_md: dir.join("CLAUDE.md").is_file(),
            }
        })
        .collect())
}

pub fn run() -> anyhow::Result<()> {
    let v = violations(&load_rules()?, &workspace_krates()?);
    if v.is_empty() {
        return Ok(());
    }
    anyhow::bail!("layering violations:\n  {}", v.join("\n  "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules {
            layers: [("a", 1), ("b", 2), ("c", 2), ("d", 3)]
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            same_layer: vec![("b".into(), "c".into())],
            no_std: vec!["a".into()],
            wasm: vec![],
        }
    }

    fn k(name: &str, deps: &[&str]) -> Krate {
        Krate {
            name: name.into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            lib_rs: Some("#![no_std]".into()),
            has_claude_md: true,
        }
    }

    #[test]
    fn allows_downward_and_listed_same_layer() {
        assert!(violations(&rules(), &[k("a", &[]), k("b", &["a", "c"]), k("d", &["b"])]).is_empty());
    }

    #[test]
    fn rejects_upward_unlisted_same_layer_and_unknown_crates() {
        let v = violations(&rules(), &[k("a", &["b"]), k("c", &["b"]), k("zzz", &[])]);
        assert_eq!(
            v,
            vec![
                "a (layer 1) must not depend on b (layer 2)",
                "c (layer 2) must not depend on b (layer 2)",
                "zzz: not listed in xtask/layers.ron",
            ]
        );
    }

    #[test]
    fn requires_no_std_and_claude_md() {
        let mut a = k("a", &[]);
        a.lib_rs = Some("//! no attribute".into());
        a.has_claude_md = false;
        let v = violations(&rules(), &[a]);
        assert_eq!(v, vec!["a: missing CLAUDE.md", "a: listed as no_std but src/lib.rs lacks #![no_std]"]);
    }

    #[test]
    fn real_workspace_obeys_the_rules() {
        let v = violations(&load_rules().unwrap(), &workspace_krates().unwrap());
        assert!(v.is_empty(), "{v:#?}");
    }
}
```

- [ ] **Step 14: Create `xtask/src/check.rs`**

```rust
//! `cargo xtask check`: every step runs; only failing steps print their output.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use crate::{fetch, layers, root};

/// Captured result of one step: `Ok(())` or the output to show.
type StepResult = Result<(), String>;

fn cargo(args: &[&str], envs: &[(&str, PathBuf)]) -> StepResult {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(args).current_dir(root());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines.len().saturating_sub(200);
    Err(lines[tail..].join("\n"))
}

fn has_nextest() -> bool {
    Command::new("cargo").args(["nextest", "--version"]).output().is_ok_and(|o| o.status.success())
}

pub fn run(offline: bool) -> anyhow::Result<bool> {
    let rules = layers::load_rules()?;
    let mut failed = Vec::new();
    let mut step = |name: &str, f: &mut dyn FnMut() -> StepResult| {
        let t = Instant::now();
        match f() {
            Ok(()) => println!("ok   {name} ({:.1}s)", t.elapsed().as_secs_f32()),
            Err(out) => {
                println!("FAIL {name} ({:.1}s)\n{out}", t.elapsed().as_secs_f32());
                failed.push(name.to_string());
            }
        }
    };

    step("fmt", &mut || cargo(&["fmt", "--all", "--", "--check"], &[]));
    step("clippy", &mut || {
        cargo(&["clippy", "--quiet", "--workspace", "--all-targets", "--", "-D", "warnings"], &[])
    });
    step("layers", &mut || layers::run().map_err(|e| format!("{e:#}")));
    step("wasm", &mut || {
        if rules.wasm.is_empty() {
            return Ok(());
        }
        let mut args = vec!["build", "--quiet", "--target", "wasm32-unknown-unknown"];
        for c in &rules.wasm {
            args.extend(["-p", c.as_str()]);
        }
        cargo(&args, &[])
    });
    let mut envs = Vec::new();
    if offline {
        println!("skip olx-content (--offline)");
    } else {
        step("olx-content", &mut || match fetch::run(true) {
            Ok(paths) => {
                envs = vec![("RL_OLX_GAMEDIR", paths.gamedir), ("RL_CONTENT_DIR", paths.content)];
                Ok(())
            }
            Err(e) => Err(format!("{e:#}")),
        });
    }
    step("test", &mut || {
        if has_nextest() {
            let args = [
                "nextest",
                "run",
                "--workspace",
                "--no-fail-fast",
                "--status-level",
                "fail",
                "--final-status-level",
                "fail",
            ];
            cargo(&args, &envs)
        } else {
            cargo(&["test", "--quiet", "--workspace"], &envs)
        }
    });

    if failed.is_empty() {
        println!("check passed");
        Ok(true)
    } else {
        println!("check failed: {}", failed.join(", "));
        Ok(false)
    }
}
```

- [ ] **Step 15: Create `xtask/src/fetch.rs`**

```rust
//! `cargo xtask fetch-olx-content`: sparse-clones the pinned OpenLieroX commit
//! listed in `content/olx-manifest.ron` (git verifies integrity by hash) and
//! imports the listed items with `rl-cli import`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::root;

#[derive(Deserialize)]
struct Manifest {
    repository: String,
    commit: String,
    /// Directories needed by importers but not imported themselves (shared images, sounds).
    shared: Vec<String>,
    mods: Vec<String>,
    levels: Vec<String>,
    themes: Vec<String>,
    skins: bool,
}

pub struct Paths {
    /// `share/gamedir` of the checkout.
    pub gamedir: PathBuf,
    /// Imported native packs.
    pub content: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().context("running git")?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn patterns(m: &Manifest) -> Vec<String> {
    let mut p: Vec<String> = Vec::new();
    for d in m.shared.iter().chain(&m.mods).chain(&m.themes) {
        p.push(format!("/share/gamedir/{d}/"));
    }
    for f in &m.levels {
        p.push(format!("/share/gamedir/{f}"));
    }
    if m.skins {
        p.push("/share/gamedir/skins/".into());
    }
    p
}

fn checkout(m: &Manifest, dir: &Path) -> anyhow::Result<()> {
    let pats = patterns(m);
    let stamp = dir.join(".rl-fetched");
    let want = format!("{}\n{}", m.commit, pats.join("\n"));
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s == want) {
        return Ok(());
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    git(dir, &["init", "--quiet"])?;
    git(dir, &["remote", "add", "origin", &m.repository])?;
    let mut sparse = vec!["sparse-checkout", "set", "--no-cone"];
    sparse.extend(pats.iter().map(String::as_str));
    git(dir, &sparse)?;
    git(dir, &["fetch", "--quiet", "--depth", "1", "--filter=blob:none", "origin", &m.commit])?;
    git(dir, &["checkout", "--quiet", &m.commit])?;
    let head = git(dir, &["rev-parse", "HEAD"])?;
    if head != m.commit {
        bail!("checked out {head}, expected {}", m.commit);
    }
    std::fs::write(stamp, want)?;
    Ok(())
}

/// Fetches the pinned checkout and, if `import` is set, imports it with `rl-cli`.
pub fn run(import: bool) -> anyhow::Result<Paths> {
    let root = root();
    let text = std::fs::read_to_string(root.join("content/olx-manifest.ron"))?;
    let m: Manifest = ron::from_str(&text).context("parsing content/olx-manifest.ron")?;
    let src = root.join("target/olx").join(&m.commit);
    checkout(&m, &src).context("fetching OpenLieroX content")?;
    let gamedir = src.join("share/gamedir");
    let content = root.join("target/content");
    if !import {
        return Ok(Paths { gamedir, content });
    }

    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(&root).args(["run", "--quiet", "-p", "rl-cli", "--", "import", "--olx"]);
    cmd.arg(&gamedir).arg("--out").arg(&content).args(["--revision", &m.commit]);
    for x in &m.mods {
        cmd.args(["--mod", x]);
    }
    for x in &m.levels {
        cmd.args(["--level", x]);
    }
    for x in &m.themes {
        cmd.args(["--theme", x]);
    }
    if m.skins {
        cmd.arg("--skins");
    }
    let out = cmd.output().context("running rl-cli import")?;
    if !out.status.success() {
        bail!(
            "rl-cli import failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(Paths { gamedir, content })
}
```

- [ ] **Step 16: Create `xtask/src/api.rs`**

```rust
//! `cargo xtask api`: compact public-API summaries so agents can learn a crate
//! without reading its source. One file per library crate in `target/api/`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use quote::ToTokens;
use syn::{Attribute, ImplItem, Item, TraitItem, Visibility};

use crate::root;

fn is_pub(v: &Visibility) -> bool {
    matches!(v, Visibility::Public(_))
}

fn is_test_cfg(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("cfg") && a.meta.to_token_stream().to_string().contains("test"))
}

/// Keeps only the first line of doc comments and `derive` attributes.
fn trim_attrs(attrs: &mut Vec<Attribute>) {
    let mut seen_doc = false;
    attrs.retain(|a| {
        if a.path().is_ident("doc") {
            let keep = !seen_doc;
            seen_doc = true;
            keep
        } else {
            a.path().is_ident("derive")
        }
    });
}

fn empty_block() -> syn::Block {
    syn::parse_quote!({})
}

/// Reduces an item to its public surface; returns `None` if nothing is public.
fn summarize(item: &Item) -> Option<Item> {
    let mut item = item.clone();
    let keep = match &mut item {
        Item::Fn(f) => {
            *f.block = empty_block();
            trim_attrs(&mut f.attrs);
            is_pub(&f.vis)
        }
        Item::Struct(s) => {
            if let syn::Fields::Named(n) = &mut s.fields {
                n.named = n
                    .named
                    .iter()
                    .filter(|f| is_pub(&f.vis))
                    .cloned()
                    .map(|mut f| {
                        trim_attrs(&mut f.attrs);
                        f
                    })
                    .collect();
            }
            trim_attrs(&mut s.attrs);
            is_pub(&s.vis)
        }
        Item::Enum(e) => {
            for v in &mut e.variants {
                trim_attrs(&mut v.attrs);
                v.attrs.retain(|a| a.path().is_ident("doc"));
            }
            trim_attrs(&mut e.attrs);
            is_pub(&e.vis)
        }
        Item::Trait(t) => {
            for ti in &mut t.items {
                if let TraitItem::Fn(f) = ti {
                    f.default = None;
                    f.semi_token = Some(Default::default());
                    trim_attrs(&mut f.attrs);
                }
            }
            trim_attrs(&mut t.attrs);
            is_pub(&t.vis)
        }
        Item::Impl(i) => {
            let is_trait_impl = i.trait_.is_some();
            i.items.retain_mut(|ii| match ii {
                ImplItem::Fn(f) if is_trait_impl || is_pub(&f.vis) => {
                    f.block = empty_block();
                    trim_attrs(&mut f.attrs);
                    !is_trait_impl
                }
                ImplItem::Const(c) => is_pub(&c.vis),
                _ => false,
            });
            i.attrs.clear();
            is_trait_impl || !i.items.is_empty()
        }
        Item::Const(c) => {
            trim_attrs(&mut c.attrs);
            is_pub(&c.vis)
        }
        Item::Type(t) => {
            trim_attrs(&mut t.attrs);
            is_pub(&t.vis)
        }
        Item::Use(u) => is_pub(&u.vis),
        _ => false,
    };
    keep.then_some(item)
}

/// Collects `(module path, items)` for `file` and the modules it declares.
fn walk(path: &str, file: &Path, out: &mut Vec<(String, Vec<Item>)>) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let ast = syn::parse_file(&text).with_context(|| format!("parsing {}", file.display()))?;
    let dir: PathBuf = if file.ends_with("lib.rs") || file.ends_with("mod.rs") {
        file.parent().expect("dir").to_path_buf()
    } else {
        file.with_extension("")
    };
    let mut items = Vec::new();
    for item in &ast.items {
        if let Item::Mod(m) = item {
            if is_test_cfg(&m.attrs) {
                continue;
            }
            let sub = format!("{path}::{}", m.ident);
            if let Some((_, inner)) = &m.content {
                let mut nested = Vec::new();
                nested.extend(inner.iter().filter_map(summarize));
                out.push((sub, nested));
            } else {
                let a = dir.join(format!("{}.rs", m.ident));
                let b = dir.join(m.ident.to_string()).join("mod.rs");
                walk(&sub, if a.is_file() { &a } else { &b }, out)?;
            }
        } else if let Some(s) = summarize(item) {
            items.push(s);
        }
    }
    out.insert(0, (path.to_string(), items));
    Ok(())
}

pub fn run() -> anyhow::Result<()> {
    let root = root();
    let out_dir = root.join("target/api");
    std::fs::create_dir_all(&out_dir)?;
    let mut crates: Vec<PathBuf> =
        std::fs::read_dir(root.join("crates"))?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
    crates.sort();
    for dir in crates {
        let lib = dir.join("src/lib.rs");
        if !lib.is_file() {
            continue;
        }
        let name = dir.file_name().expect("name").to_string_lossy().replace('-', "_");
        let mut mods = Vec::new();
        walk(&name, &lib, &mut mods)?;
        mods.sort_by(|a, b| a.0.cmp(&b.0));
        let mut text = format!("// Public API of {name} (generated by `cargo xtask api`; do not edit)\n");
        for (path, items) in mods {
            if items.is_empty() {
                continue;
            }
            let file = syn::File { shebang: None, attrs: vec![], items };
            text.push_str(&format!("\n// ---- {path}\n{}", prettyplease::unparse(&file)));
        }
        let target = out_dir.join(format!("{name}.txt"));
        std::fs::write(&target, text)?;
        println!("wrote {}", target.display());
    }
    Ok(())
}
```

- [ ] **Step 17: Create `xtask/src/main.rs`**

```rust
//! Developer workflows. Run from anywhere in the workspace with `cargo xtask <command>`.

mod api;
mod check;
mod fetch;
mod layers;

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

- [ ] **Step 18: Run**

```bash
cargo nextest run -p xtask
```

Expected: output contains `4 passed`. The layering rules are unit-tested, and `real_workspace_obeys_the_rules` checks this repo.

- [ ] **Step 19: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`. Every step prints `ok`. `wasm` does nothing yet; `olx-content` is skipped offline.

- [ ] **Optional:** `cargo xtask install-hooks` installs a git pre-push hook that runs `cargo xtask check`.

- [ ] **Step 20: Commit**

```bash
git add -A
git commit -m "Add workspace skeleton and xtask"
```


---

### Task 2: `rl-data`: images, levels, skins, themes, manifests

Creates the `no_std` data crate with every non-mod content type. Mod definitions follow in Task 3.

**Files:**

- Modify: `Cargo.toml` (`members = ["crates/*", "xtask"]`)
- Create: `crates/rl-data/{Cargo.toml,CLAUDE.md}`
- Create: `crates/rl-data/src/{lib,image,slug,level,skin,theme,manifest}.rs`
- Modify: `xtask/layers.ron` (register `rl-data` as layer 1, `no_std`, `wasm`)

**Interfaces:**

- Produces: `rl_data::image::{Rgba, RgbaImage}`: `RgbaImage::new(w,h)`, `from_raw(w,h,Vec<u8>) -> Option`, `get/set(x,y)`, `crop(x,y,w,h)`, `as_raw()`, `width()`, `height()`; `Rgba::{rgb, MAGENTA, BLACK, TRANSPARENT, same_rgb}`.
- Produces: `rl_data::slug::slug(&str) -> String`, e.g. `"MW 1.0"` → `"mw-1-0"`.
- Produces: `rl_data::level::{Material{Rock=0,Background=1,Dirt=2}, MaterialGrid, LevelDef, Level, SpawnPoint}`; `Material::from_lx_flags(u8)`, `Material::from_index(u8)`, `MaterialGrid::{new, from_cells, get, count, cells}`.
- Produces: `rl_data::skin::{Skin, SkinDef, FrameLayout::OLX, MaskKind, colorize_pixel}`; `Skin::colorize(Rgba) -> RgbaImage`.
- Produces: `rl_data::theme::{Theme, ThemeDef}`; `rl_data::manifest::{PackManifest, PackKind, Origin, PACK_FORMAT_VERSION}`.

- [ ] Cargo rejects a `crates/*` member glob that matches nothing, so the glob arrives with the first crate.

- [ ] **Step 1: Replace `Cargo.toml`**

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
serde = { version = "1.0.229", default-features = false, features = ["derive", "alloc"] }
ron = "0.12.2"
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

- [ ] **Step 2: Create `crates/rl-data/Cargo.toml`**

```toml
[package]
name = "rl-data"
description = "Engine-independent content definitions: mods, weapons, projectiles, levels, skins, themes, packs."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
serde.workspace = true

[dev-dependencies]
ron.workspace = true
proptest.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Create `crates/rl-data/CLAUDE.md`**

```markdown
# rl-data

Content definitions shared by importers, packs and (later) the simulation:
mods, weapons, projectiles and their actions, levels with material grids,
skins with colorization masks, themes, and pack manifests.

- **Layer 1, `#![no_std]` + `alloc`.** No I/O, no `std`. Only dependency: `serde`.
- **Values mirror OLX exactly** as OLX holds them in memory after loading
  (for example `WeaponDef::rof` is already divided by 1000). Every type cites
  the OLX source it mirrors; keep those references when editing.
- **Floats must round-trip bit-exactly** through RON; a property test enforces it.
- Enums keep unknown OLX codes as `Other(i32)` instead of failing.

Test: `cargo nextest run -p rl-data`. Build for the web: `cargo build -p rl-data --target wasm32-unknown-unknown`.
```

- [ ] **Step 4: Create `crates/rl-data/src/lib.rs`**

```rust
//! Content definitions shared by importers, the pack format and the simulation.
//!
//! This crate is `no_std` + `alloc`: it holds data only, never performs I/O.
//! Field semantics mirror OpenLieroX (OLX) at commit `d1ad953f`; each type
//! documents the OLX source it corresponds to.
#![no_std]

extern crate alloc;

pub mod image;
pub mod level;
pub mod manifest;
pub mod skin;
pub mod slug;
pub mod theme;
```

- [ ] **Step 5: Create `crates/rl-data/src/image.rs`**

```rust
//! Plain RGBA images. Encoding to and from PNG lives in `rl-pack`.

use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// An 8-bit-per-channel color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// Opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// OLX's color key: pure magenta marks "transparent" in skins, sprites and stamps.
    pub const MAGENTA: Rgba = Rgba::rgb(255, 0, 255);
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };

    /// True when red, green and blue match, ignoring alpha.
    pub fn same_rgb(self, other: Rgba) -> bool {
        self.r == other.r && self.g == other.g && self.b == other.b
    }
}

/// A row-major RGBA8 image: `pixels.len() == width * height * 4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaImage {
    /// A fully transparent image.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, pixels: vec![0; width as usize * height as usize * 4] }
    }

    /// Wraps raw RGBA8 bytes. Returns `None` if the length does not match.
    pub fn from_raw(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        (pixels.len() == width as usize * height as usize * 4).then_some(Self { width, height, pixels })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn as_raw(&self) -> &[u8] {
        &self.pixels
    }

    /// Panics if `(x, y)` is out of bounds.
    pub fn get(&self, x: u32, y: u32) -> Rgba {
        let i = self.index(x, y);
        let p = &self.pixels[i..i + 4];
        Rgba { r: p[0], g: p[1], b: p[2], a: p[3] }
    }

    /// Panics if `(x, y)` is out of bounds.
    pub fn set(&mut self, x: u32, y: u32, c: Rgba) {
        let i = self.index(x, y);
        self.pixels[i..i + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
    }

    /// Copies the `w`×`h` region at `(x, y)` into a new image. Panics if out of bounds.
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        assert!(x + w <= self.width && y + h <= self.height, "crop out of bounds");
        let mut out = RgbaImage::new(w, h);
        for row in 0..h {
            let src = self.index(x, y + row);
            let dst = out.index(0, row);
            let len = w as usize * 4;
            out.pixels[dst..dst + len].copy_from_slice(&self.pixels[src..src + len]);
        }
        out
    }

    fn index(&self, x: u32, y: u32) -> usize {
        assert!(x < self.width && y < self.height, "pixel ({x},{y}) out of bounds");
        (y as usize * self.width as usize + x as usize) * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_roundtrip() {
        let mut img = RgbaImage::new(3, 2);
        img.set(2, 1, Rgba::rgb(1, 2, 3));
        assert_eq!(img.get(2, 1), Rgba::rgb(1, 2, 3));
        assert_eq!(img.get(0, 0), Rgba::TRANSPARENT);
    }

    #[test]
    fn from_raw_rejects_wrong_length() {
        assert!(RgbaImage::from_raw(2, 2, vec![0; 15]).is_none());
        assert!(RgbaImage::from_raw(2, 2, vec![0; 16]).is_some());
    }

    #[test]
    fn crop_copies_region() {
        let mut img = RgbaImage::new(4, 4);
        img.set(1, 2, Rgba::rgb(9, 9, 9));
        let c = img.crop(1, 1, 2, 2);
        assert_eq!((c.width(), c.height()), (2, 2));
        assert_eq!(c.get(0, 1), Rgba::rgb(9, 9, 9));
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn get_out_of_bounds_panics() {
        RgbaImage::new(1, 1).get(1, 0);
    }
}
```

- [ ] **Step 6: Create `crates/rl-data/src/slug.rs`**

```rust
//! Canonical identifiers for pack ids and file names.

use alloc::string::String;

/// Lowercases ASCII letters and digits, maps every other run of characters to
/// a single `-`, and trims `-` from both ends. Returns `"unnamed"` if nothing remains.
///
/// `slug("MW 1.0")` is `"mw-1-0"`; `slug("Liero Factory")` is `"liero-factory"`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() { String::from("unnamed") } else { out }
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn examples() {
        assert_eq!(slug("MW 1.0"), "mw-1-0");
        assert_eq!(slug("Liero Factory"), "liero-factory");
        assert_eq!(slug("  --Bazooka!!"), "bazooka");
        assert_eq!(slug("Amy'sLion"), "amy-slion");
        assert_eq!(slug("ŽŽŽ"), "unnamed");
        assert_eq!(slug(""), "unnamed");
    }
}
```

- [ ] **Step 7: Create `crates/rl-data/src/level.rs`**

```rust
//! Levels: a material grid at physics resolution plus color art layers.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::RgbaImage;

/// Terrain material of one world pixel. The numeric values are OLX's material
/// indices (`src/gusanos/level.cpp:80-145`), which the material PNG stores directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Material {
    Rock = 0,
    Background = 1,
    Dirt = 2,
}

impl Material {
    pub fn from_index(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Rock),
            1 => Some(Self::Background),
            2 => Some(Self::Dirt),
            _ => None,
        }
    }

    /// OLX `Material::indexFromLxFlag` (`src/gusanos/material.h:44-49`):
    /// rock wins over dirt, and anything else is background.
    pub fn from_lx_flags(flags: u8) -> Self {
        const PX_DIRT: u8 = 0x02;
        const PX_ROCK: u8 = 0x04;
        if flags & PX_ROCK != 0 {
            Self::Rock
        } else if flags & PX_DIRT != 0 {
            Self::Dirt
        } else {
            Self::Background
        }
    }
}

/// One material per world pixel, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialGrid {
    width: u32,
    height: u32,
    cells: Vec<Material>,
}

impl MaterialGrid {
    /// A grid filled with `fill`.
    pub fn new(width: u32, height: u32, fill: Material) -> Self {
        Self { width, height, cells: vec![fill; width as usize * height as usize] }
    }

    /// Returns `None` if `cells.len() != width * height`.
    pub fn from_cells(width: u32, height: u32, cells: Vec<Material>) -> Option<Self> {
        (cells.len() == width as usize * height as usize).then_some(Self { width, height, cells })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn cells(&self) -> &[Material] {
        &self.cells
    }

    /// Panics if `(x, y)` is out of bounds.
    pub fn get(&self, x: u32, y: u32) -> Material {
        assert!(x < self.width && y < self.height, "cell ({x},{y}) out of bounds");
        self.cells[y as usize * self.width as usize + x as usize]
    }

    /// Number of cells with the given material.
    pub fn count(&self, m: Material) -> usize {
        self.cells.iter().filter(|c| **c == m).count()
    }
}

/// A spawn point in world pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnPoint {
    pub x: i32,
    pub y: i32,
}

/// The `level.ron` part of a level pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelDef {
    pub name: String,
    /// Size of the material grid in world pixels.
    pub width: u32,
    pub height: u32,
    /// Art pixels per world pixel for `front.png` and `back.png`.
    pub art_scale: u32,
    /// Pack id of the theme used for carving holes.
    pub theme: String,
    /// Empty means "random spawn", as in OLX.
    pub spawns: Vec<SpawnPoint>,
}

/// A loaded level.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub def: LevelDef,
    pub material: MaterialGrid,
    /// Foreground art; where the material is background it equals `back`.
    pub front: RgbaImage,
    pub back: RgbaImage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lx_flags_follow_olx_priority() {
        assert_eq!(Material::from_lx_flags(0x01), Material::Background);
        assert_eq!(Material::from_lx_flags(0x02), Material::Dirt);
        assert_eq!(Material::from_lx_flags(0x04), Material::Rock);
        assert_eq!(Material::from_lx_flags(0x06), Material::Rock);
        assert_eq!(Material::from_lx_flags(0x00), Material::Background);
        assert_eq!(Material::from_lx_flags(0x18), Material::Background);
    }

    #[test]
    fn index_roundtrip() {
        for m in [Material::Rock, Material::Background, Material::Dirt] {
            assert_eq!(Material::from_index(m as u8), Some(m));
        }
        assert_eq!(Material::from_index(3), None);
    }

    #[test]
    fn grid_rejects_wrong_size_and_counts() {
        assert!(MaterialGrid::from_cells(2, 2, vec![Material::Dirt; 3]).is_none());
        let g = MaterialGrid::from_cells(2, 1, vec![Material::Dirt, Material::Rock]).unwrap();
        assert_eq!(g.get(1, 0), Material::Rock);
        assert_eq!(g.count(Material::Dirt), 1);
    }
}
```

- [ ] **Step 8: Create `crates/rl-data/src/skin.rs`**

```rust
//! Worm skins: a sprite sheet plus a per-pixel colorization mask.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::{Rgba, RgbaImage};

/// What happens to a skin pixel when the player's color is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaskKind {
    /// Copy the pixel unchanged (OLX: black mask pixel).
    Keep,
    /// Tint with the player color (OLX: any other mask color).
    Colorize,
    /// Do not draw (OLX: magenta mask pixel).
    Transparent,
}

/// Frame layout of the sheet (`include/CGameSkin.h:37-41`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameLayout {
    pub frame_width: u32,
    pub frame_height: u32,
    pub frame_spacing: u32,
    pub worm_width: u32,
    pub worm_height: u32,
}

impl FrameLayout {
    /// The layout every OLX skin uses.
    pub const OLX: FrameLayout =
        FrameLayout { frame_width: 32, frame_height: 18, frame_spacing: 4, worm_width: 20, worm_height: 18 };
}

/// The `skin.ron` part of a skin pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkinDef {
    pub name: String,
    pub layout: FrameLayout,
}

/// A loaded skin. `image` and `mask` have the same size.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    pub def: SkinDef,
    pub image: RgbaImage,
    pub mask: Vec<MaskKind>,
}

impl Skin {
    /// The mask at `(x, y)`. Panics if out of bounds.
    pub fn mask_at(&self, x: u32, y: u32) -> MaskKind {
        assert!(x < self.image.width() && y < self.image.height(), "mask ({x},{y}) out of bounds");
        self.mask[y as usize * self.image.width() as usize + x as usize]
    }

    /// The sheet tinted with `player`, following OLX `CGameSkin::Colorize_Execute`
    /// (`src/client/CGameSkin.cpp:720-790`).
    pub fn colorize(&self, player: Rgba) -> RgbaImage {
        let mut out = RgbaImage::new(self.image.width(), self.image.height());
        for y in 0..self.image.height() {
            for x in 0..self.image.width() {
                let px = self.image.get(x, y);
                let c = match self.mask_at(x, y) {
                    MaskKind::Keep => px,
                    MaskKind::Transparent => Rgba::TRANSPARENT,
                    MaskKind::Colorize => colorize_pixel(px, player),
                };
                out.set(x, y, c);
            }
        }
        out
    }
}

/// OLX's tint formula: `min(255, c * player / divisor)` with divisors 96, 156, 252,
/// and pure magenta nudged to (240, 0, 240) so it does not become the color key.
/// Alpha is kept from `px`.
pub fn colorize_pixel(px: Rgba, player: Rgba) -> Rgba {
    let tint = |c: u8, p: u8, div: u32| ((c as u32 * p as u32) / div).min(255) as u8;
    let mut out = Rgba {
        r: tint(px.r, player.r, 96),
        g: tint(px.g, player.g, 156),
        b: tint(px.b, player.b, 252),
        a: px.a,
    };
    if out.same_rgb(Rgba::MAGENTA) {
        out.r = 240;
        out.b = 240;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn colorize_pixel_matches_olx_formula() {
        let white = Rgba::rgb(255, 255, 255);
        assert_eq!(colorize_pixel(white, Rgba::rgb(96, 156, 252)), white);
        assert_eq!(colorize_pixel(Rgba::rgb(100, 100, 100), Rgba::rgb(48, 78, 126)), Rgba::rgb(50, 50, 50));
        assert_eq!(colorize_pixel(white, Rgba::rgb(0, 0, 0)), Rgba::rgb(0, 0, 0));
    }

    #[test]
    fn colorize_pixel_avoids_color_key() {
        // r: 255*96/96 = 255, g: 0, b: 255*252/252 = 255 -> would be magenta.
        let out = colorize_pixel(Rgba::rgb(255, 0, 255), Rgba::rgb(96, 0, 252));
        assert_eq!(out, Rgba::rgb(240, 0, 240));
    }

    #[test]
    fn colorize_respects_mask() {
        let mut image = RgbaImage::new(3, 1);
        for x in 0..3 {
            image.set(x, 0, Rgba::rgb(200, 200, 200));
        }
        let skin = Skin {
            def: SkinDef { name: "t".into(), layout: FrameLayout::OLX },
            image,
            mask: vec![MaskKind::Keep, MaskKind::Colorize, MaskKind::Transparent],
        };
        let out = skin.colorize(Rgba::rgb(0, 0, 0));
        assert_eq!(out.get(0, 0), Rgba::rgb(200, 200, 200));
        assert_eq!(out.get(1, 0), Rgba::rgb(0, 0, 0));
        assert_eq!(out.get(2, 0), Rgba::TRANSPARENT);
    }
}
```

- [ ] **Step 9: Create `crates/rl-data/src/theme.rs`**

```rust
//! Level themes: stamps used when carving terrain, plus pixmap-level decorations.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::RgbaImage;

/// The `theme.ron` part of a theme pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeDef {
    pub name: String,
    pub holes: u32,
    pub stones: u32,
    pub misc: u32,
}

/// A loaded theme. Images keep OLX's raw colors: in hole stamps, magenta means
/// "make empty", black means "leave alone" and other colors paint the dirt edge
/// (`src/common/CMap.cpp:1007-1106`). Stones and misc use magenta as transparency.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub def: ThemeDef,
    pub holes: Vec<RgbaImage>,
    pub stones: Vec<RgbaImage>,
    pub misc: Vec<RgbaImage>,
    pub front_tile: RgbaImage,
    pub back_tile: RgbaImage,
}
```

- [ ] **Step 10: Create `crates/rl-data/src/manifest.rs`**

```rust
//! `pack.ron`: what every pack declares about itself.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Current native pack format. Bump it when the on-disk layout changes and add
/// a migration in `rl-pack`.
pub const PACK_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackKind {
    Mod,
    Level,
    Skin,
    Theme,
}

/// Where imported content came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// For example `"OpenLieroX"`.
    pub source: String,
    /// Path inside the source, for example `"share/gamedir/MW 1.0"`.
    pub path: String,
    /// Source revision, for example a git commit.
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackManifest {
    pub format_version: u32,
    /// Lowercase slug, unique among installed packs, such as `"olx-mw-1-0"`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PackKind,
    pub authors: Vec<String>,
    pub origin: Option<Origin>,
    pub license: Option<String>,
}
```

- [ ] **Step 11: Replace `xtask/layers.ron`**

```ron
// Crate layering (spec §4). A crate may depend on crates in lower layers, and on
// same-layer crates only when the pair is listed in `same_layer`. Every
// workspace crate must be listed here. `no_std` crates must declare
// #![no_std]; `wasm` crates must build for wasm32-unknown-unknown.
(
    layers: {
        "rl-data": 1,
        "xtask": 6,
    },
    same_layer: [],
    no_std: ["rl-data"],
    wasm: ["rl-data"],
)
```

- [ ] **Step 12: Run**

```bash
cargo nextest run -p rl-data
```

Expected: output contains `11 passed`. Covers the OLX material priority, the colorize formula and its magenta nudge, and image bounds.

- [ ] **Step 13: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`. `wasm` now builds `rl-data` for `wasm32-unknown-unknown`.

- [ ] **Step 14: Commit**

```bash
git add -A
git commit -m "rl-data: add image, level, skin, theme and manifest types"
```


---

### Task 3: `rl-data`: mod definitions

Adds the mod model with LX56 semantics made explicit: weapons, projectiles, spawn info, trails, timers and actions. Values are stored as OLX holds them after loading.

**Files:**

- Create: `crates/rl-data/src/mod_def.rs`
- Modify: `crates/rl-data/src/lib.rs` (add `pub mod mod_def;`)

**Interfaces:**

- Produces: `rl_data::mod_def::{ModDef, WormParams, RopeParams, WeaponDef, WeaponKind, WeaponClass, SpecialKind, BeamDef, SpawnDef, ProjectileDef, ProjectileGfx, Animation, AnimKind, TimerDef, TrailDef, TrailKind, ActionDef, ActionKind}`.
- Produces: `SpawnDef::olx_default(String)` and `ActionDef::olx_default(ActionKind)`, OLX constructor defaults; `*::from_lx(i32)` for every OLX enum, with unknown codes kept as `Other(i32)`.
- Produces: `ProjectileDef::children()` and `ProjectileDef::assets()` iterators; `WeaponDef::projectile()` and `WeaponDef::sound()`.
- `WeaponDef::id` is `#[serde(skip)]`: the id is the file name in a pack.

- [ ] **Step 1: Create `crates/rl-data/src/mod_def.rs`**

```rust
//! Mod definitions: worm and rope physics, weapons, projectiles.
//!
//! Mirrors OLX's classic gamescript model (`include/WeaponDesc.h`,
//! `include/ProjectileDesc.h`, `include/ProjAction.h`) with the LX56 (v7)
//! semantics made explicit. Values are stored exactly as OLX holds them in
//! memory after loading, e.g. `rof` is already divided by 1000
//! (`src/common/CGameScript.cpp:776-777`).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::Rgba;

/// A complete mod. `weapons` keeps the original order (it defines weapon ids);
/// projectiles are shared definitions referenced by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModDef {
    pub name: String,
    pub worm: WormParams,
    pub rope: RopeParams,
    pub weapons: Vec<WeaponDef>,
    pub projectiles: BTreeMap<String, ProjectileDef>,
}

/// OLX `gs_worm_t` (`src/common/CGameScript.cpp:45-55`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WormParams {
    /// Read from the file but ignored by OLX.
    pub angle_speed: f32,
    pub ground_speed: f32,
    pub air_speed: f32,
    pub gravity: f32,
    pub jump_force: f32,
    pub air_friction: f32,
    /// Read from the file but ignored by OLX.
    pub ground_friction: f32,
}

/// Ninja rope settings stored in the mod (`CGameScript.cpp:788-800`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RopeParams {
    pub max_length: i32,
    pub rest_length: i32,
    pub strength: f32,
}

/// OLX `weapon_t`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponDef {
    /// Stable identifier; it is the file name in a pack, so it is not serialized.
    #[serde(skip)]
    pub id: String,
    pub name: String,
    /// Already divided by 10, as OLX does after loading.
    pub recharge: f32,
    pub drain: f32,
    /// Already divided by 1000 (seconds between shots).
    pub rof: f32,
    pub laser_sight: bool,
    pub kind: WeaponKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WeaponKind {
    /// `WPN_PROJECTILE`.
    Projectile { class: WeaponClass, recoil: i32, sound: Option<String>, spawn: SpawnDef },
    /// `WPN_BEAM`.
    Beam { recoil: i32, beam: BeamDef },
    /// `WPN_SPECIAL`.
    Special { special: SpecialKind, thrust: u32 },
}

/// OLX `Wpn_Class`; only used by bot AI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponClass {
    Automatic,
    PowerGun,
    Grenade,
    Missile,
    CloseRange,
    Other(i32),
}

impl WeaponClass {
    pub fn from_lx(v: i32) -> Self {
        match v {
            0 => Self::Automatic,
            1 => Self::PowerGun,
            3 => Self::Grenade,
            4 => Self::Missile,
            5 => Self::CloseRange,
            v => Self::Other(v),
        }
    }
}

/// OLX `Wpn_Special`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecialKind {
    None,
    Jetpack,
    Other(i32),
}

impl SpecialKind {
    pub fn from_lx(v: i32) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Jetpack,
            v => Self::Other(v),
        }
    }
}

/// OLX `Wpn_Beam`. LX56 files carry only colour, damage, player damage and
/// length; the other fields keep OLX's defaults.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeamDef {
    pub colour: Rgba,
    pub damage: i32,
    pub player_damage: i32,
    pub length: i32,
    pub init_width: i32,
    pub width_increase: f32,
    pub distribute_damage_over_width: bool,
}

/// OLX `Proj_SpawnInfo`: how child projectiles are spawned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpawnDef {
    /// Name of the spawned projectile in [`ModDef::projectiles`].
    pub projectile: String,
    pub amount: i32,
    pub speed: i32,
    pub speed_var: f32,
    pub spread: f32,
    pub use_angle: bool,
    pub angle: i32,
    pub use_parent_velocity_for_spread: bool,
    pub parent_vel_spread_factor: f32,
    pub use_special11_vec_for_speed_var: bool,
    pub use_random_rot: bool,
    pub add_parent_vel: bool,
    pub parent_vel_factor: f32,
}

impl SpawnDef {
    /// OLX's `Proj_SpawnInfo()` constructor defaults, spawning `projectile`.
    pub fn olx_default(projectile: String) -> Self {
        Self {
            projectile,
            amount: 0,
            speed: 0,
            speed_var: 0.0,
            spread: 0.0,
            use_angle: false,
            angle: 0,
            use_parent_velocity_for_spread: false,
            parent_vel_spread_factor: 0.3,
            use_special11_vec_for_speed_var: false,
            use_random_rot: false,
            add_parent_vel: false,
            parent_vel_factor: 1.0,
        }
    }
}

/// OLX `proj_t`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectileDef {
    pub gfx: ProjectileGfx,
    /// `Some` when `UseCustomGravity` is set.
    pub gravity: Option<i32>,
    pub dampening: f32,
    pub width: i32,
    pub height: i32,
    pub timer: TimerDef,
    pub trail: TrailDef,
    /// Terrain hit.
    pub hit: ActionDef,
    /// Worm hit.
    pub player_hit: ActionDef,
    /// `GeneralSpawnInfo`: what `spawns_projectiles` actions spawn.
    pub event_spawn: Option<SpawnDef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProjectileGfx {
    /// `PRJ_PIXEL`: one of `colours` is picked per projectile.
    Pixel { colours: Vec<Rgba> },
    /// `PRJ_IMAGE`. `file` is a path inside the mod pack, such as `gfx/missile.png`.
    Image {
        file: String,
        rotating: bool,
        rot_increment: i32,
        rot_speed: i32,
        use_angle: bool,
        use_spec_angle: bool,
        angle_images: i32,
        animation: Option<Animation>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub rate: f32,
    pub kind: AnimKind,
}

/// OLX `Proj_AnimType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimKind {
    Once,
    Loop,
    PingPong,
    Other(i32),
}

impl AnimKind {
    pub fn from_lx(v: i32) -> Self {
        match v {
            0 => Self::Once,
            1 => Self::Loop,
            2 => Self::PingPong,
            v => Self::Other(v),
        }
    }
}

/// OLX `Proj_LX56Timer`. `action` is `Some` exactly when `time > 0`,
/// because only then does the file contain the timer action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimerDef {
    pub time: f32,
    pub time_var: f32,
    pub action: Option<ActionDef>,
}

/// OLX `Proj_Trail`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrailDef {
    pub kind: TrailKind,
    /// Seconds (OLX divides the stored milliseconds by 1000).
    pub delay: f32,
    /// `Some` exactly when `kind` is [`TrailKind::Projectile`].
    pub spawn: Option<SpawnDef>,
}

/// OLX `Proj_TrailType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrailKind {
    None,
    Smoke,
    ChemSmoke,
    Projectile,
    Doomsday,
    Explosive,
    Other(i32),
}

impl TrailKind {
    pub fn from_lx(v: i32) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Smoke,
            2 => Self::ChemSmoke,
            3 => Self::Projectile,
            4 => Self::Doomsday,
            5 => Self::Explosive,
            v => Self::Other(v),
        }
    }
}

/// The LX56 part of OLX `Proj_Action`. Fields a file does not contain keep the
/// constructor defaults from `include/ProjAction.h:211-215`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDef {
    pub kind: ActionKind,
    pub damage: i32,
    pub spawns_projectiles: bool,
    /// Path inside the mod pack, such as `sfx/bazooka.ogg`.
    pub sound: Option<String>,
    pub shake: i32,
    pub bounce_coeff: f32,
    pub bounce_explode: i32,
}

impl ActionDef {
    /// OLX's `Proj_Action()` defaults with the given kind.
    pub fn olx_default(kind: ActionKind) -> Self {
        Self {
            kind,
            damage: 0,
            spawns_projectiles: false,
            sound: None,
            shake: 0,
            bounce_coeff: 0.5,
            bounce_explode: 0,
        }
    }
}

/// OLX `Proj_ActionType` (`include/ProjAction.h:147-175`). Value 3 is unused by OLX.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    Bounce,
    Explode,
    Injure,
    Carve,
    Dirt,
    GreenDirt,
    Disappear,
    Nothing,
    Disappear2,
    GoThrough,
    InjureProj,
    PlaySound,
    InjureWorm,
    ChangeRadius,
    OverwriteOwnSpeed,
    MultiplyOwnSpeed,
    DiffOwnSpeed,
    OverwriteTargetSpeed,
    MultiplyTargetSpeed,
    DiffTargetSpeed,
    HeadingToNextWorm,
    HeadingToOwner,
    HeadingToNextOtherWorm,
    HeadingToNextEnemyWorm,
    HeadingToNextTeamMate,
    HeadTargetToUs,
    Other(i32),
}

impl ActionKind {
    pub fn from_lx(v: i32) -> Self {
        use ActionKind::*;
        match v {
            0 => Bounce,
            1 => Explode,
            2 => Injure,
            4 => Carve,
            5 => Dirt,
            6 => GreenDirt,
            7 => Disappear,
            8 => Nothing,
            9 => Disappear2,
            10 => GoThrough,
            11 => InjureProj,
            12 => PlaySound,
            13 => InjureWorm,
            14 => ChangeRadius,
            15 => OverwriteOwnSpeed,
            16 => MultiplyOwnSpeed,
            17 => DiffOwnSpeed,
            18 => OverwriteTargetSpeed,
            19 => MultiplyTargetSpeed,
            20 => DiffTargetSpeed,
            21 => HeadingToNextWorm,
            22 => HeadingToOwner,
            23 => HeadingToNextOtherWorm,
            24 => HeadingToNextEnemyWorm,
            25 => HeadingToNextTeamMate,
            26 => HeadTargetToUs,
            v => Other(v),
        }
    }
}

impl ProjectileDef {
    /// Names of the projectiles this one can spawn (event spawn, then trail).
    pub fn children(&self) -> impl Iterator<Item = &str> {
        self.event_spawn.iter().chain(self.trail.spawn.iter()).map(|s| s.projectile.as_str())
    }

    /// Paths of pack assets this projectile references (image, sounds).
    pub fn assets(&self) -> impl Iterator<Item = &str> {
        let image = match &self.gfx {
            ProjectileGfx::Image { file, .. } => Some(file.as_str()),
            ProjectileGfx::Pixel { .. } => None,
        };
        let timer_sound = self.timer.action.as_ref().and_then(|a| a.sound.as_deref());
        image
            .into_iter()
            .chain(self.hit.sound.as_deref())
            .chain(timer_sound)
            .chain(self.player_hit.sound.as_deref())
    }
}

impl WeaponDef {
    /// Name of the projectile fired by this weapon, if it is a projectile weapon.
    pub fn projectile(&self) -> Option<&str> {
        match &self.kind {
            WeaponKind::Projectile { spawn, .. } => Some(spawn.projectile.as_str()),
            _ => None,
        }
    }

    /// Path of the firing sound inside the pack, if any.
    pub fn sound(&self) -> Option<&str> {
        match &self.kind {
            WeaponKind::Projectile { sound, .. } => sound.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    pub(crate) fn sample_projectile() -> ProjectileDef {
        ProjectileDef {
            gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(255, 255, 0)] },
            gravity: Some(50),
            dampening: 1.0,
            width: 2,
            height: 2,
            timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
            trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
            hit: ActionDef { damage: 5, ..ActionDef::olx_default(ActionKind::Explode) },
            player_hit: ActionDef::olx_default(ActionKind::Injure),
            event_spawn: Some(SpawnDef::olx_default("bits".to_string())),
        }
    }

    #[test]
    fn lx_codes_map_to_variants() {
        assert_eq!(ActionKind::from_lx(4), ActionKind::Carve);
        assert_eq!(ActionKind::from_lx(3), ActionKind::Other(3));
        assert_eq!(TrailKind::from_lx(3), TrailKind::Projectile);
        assert_eq!(WeaponClass::from_lx(2), WeaponClass::Other(2));
        assert_eq!(AnimKind::from_lx(2), AnimKind::PingPong);
        assert_eq!(SpecialKind::from_lx(1), SpecialKind::Jetpack);
    }

    #[test]
    fn projectile_ron_roundtrip() {
        let p = sample_projectile();
        let s = ron::ser::to_string_pretty(&p, ron::ser::PrettyConfig::default()).unwrap();
        let back: ProjectileDef = ron::from_str(&s).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn weapon_id_is_not_serialized() {
        let w = WeaponDef {
            id: "bazooka".to_string(),
            name: "Bazooka".to_string(),
            recharge: 1.0,
            drain: 2.0,
            rof: 0.5,
            laser_sight: false,
            kind: WeaponKind::Special { special: SpecialKind::Jetpack, thrust: 3 },
        };
        let s = ron::to_string(&w).unwrap();
        assert!(!s.contains("bazooka"), "id leaked into RON: {s}");
        let back: WeaponDef = ron::from_str(&s).unwrap();
        assert_eq!(back.id, "");
        assert_eq!(back.name, "Bazooka");
    }

    #[test]
    fn children_and_assets() {
        let mut p = sample_projectile();
        p.hit.sound = Some("sfx/boom.ogg".to_string());
        assert_eq!(p.children().collect::<Vec<_>>(), vec!["bits"]);
        assert_eq!(p.assets().collect::<Vec<_>>(), vec!["sfx/boom.ogg"]);
    }

    #[test]
    fn non_finite_floats_survive_ron() {
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let back: f32 = ron::from_str(&ron::to_string(&v).unwrap()).unwrap();
            assert_eq!(back.is_nan(), v.is_nan());
            if !v.is_nan() {
                assert_eq!(back, v);
            }
        }
    }

    proptest::proptest! {
        /// Every f32, including subnormals, must survive a RON round trip bit-exactly,
        /// because physics parameters have to match OLX exactly.
        #[test]
        fn f32_survives_ron_bit_exactly(bits in proptest::num::u32::ANY) {
            let v = f32::from_bits(bits);
            proptest::prop_assume!(v.is_finite());
            let s = ron::to_string(&v).unwrap();
            let back: f32 = ron::from_str(&s).unwrap();
            proptest::prop_assert_eq!(back.to_bits(), v.to_bits());
        }
    }
}
```

- [ ] **Step 2: Replace `crates/rl-data/src/lib.rs`**

```rust
//! Content definitions shared by importers, the pack format and the simulation.
//!
//! This crate is `no_std` + `alloc`: it holds data only, never performs I/O.
//! Field semantics mirror OpenLieroX (OLX) at commit `d1ad953f`; each type
//! documents the OLX source it corresponds to.
#![no_std]

extern crate alloc;

pub mod image;
pub mod level;
pub mod manifest;
pub mod mod_def;
pub mod skin;
pub mod slug;
pub mod theme;
```

- [ ] **Step 3: Run**

```bash
cargo nextest run -p rl-data
```

Expected: output contains `17 passed`. Includes `f32_survives_ron_bit_exactly` (a property test over all finite floats) and `non_finite_floats_survive_ron`.

- [ ] **Step 4: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "rl-data: add mod, weapon and projectile definitions"
```


---

### Task 4: `rl-vfs`: content sources, sinks and safe paths

Creates the I/O abstraction every loader uses: validated relative paths, in-memory and directory backends, and the case-insensitive lookup that OLX data needs.

**Files:**

- Create: `crates/rl-vfs/{Cargo.toml,CLAUDE.md}`, `crates/rl-vfs/src/{lib,path,mem,dir,lookup}.rs`
- Modify: `xtask/layers.ron` (register `rl-vfs`, layer 1, `wasm`)

**Interfaces:**

- Produces: `rl_vfs::VPath`: `new(&str)`, `from_olx(&str)` (converts `\`), `root()`, `join(&str)`, `parent()`, `file_name()`, `components()`, `strip_prefix(&VPath)`, `as_str()`; `Ord`, `Hash`, `Display`.
- Produces: `trait ContentSource { fn read(&self, &VPath) -> Result<Vec<u8>, VfsError>; fn list(&self, &VPath) -> Result<Vec<String>, VfsError>; fn exists(&self, &VPath) -> bool }`, where `list` is sorted and directories end in `/`.
- Produces: `trait ContentSink { fn write(&mut self, &VPath, &[u8]) -> Result<(), VfsError> }`.
- Produces: `MemFs` (`new`, `with(path, data)`, `files()`), `DirFs::new(root)`, `walk(src, dir) -> Vec<VPath>`, `find_ci(src, &VPath) -> Option<VPath>`, `find_ci_any(src, &[VPath])`.
- Produces: `VfsError::{NotFound(String), InvalidPath{path, reason}, Io{path, source}}`.

- [ ] **Step 1: Create `crates/rl-vfs/Cargo.toml`**

```toml
[package]
name = "rl-vfs"
description = "Content sources and sinks: in-memory and directory backends, safe relative paths, case-insensitive lookup."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
proptest.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/rl-vfs/CLAUDE.md`**

```markdown
# rl-vfs

Content I/O: the `ContentSource` / `ContentSink` traits, `MemFs` (tests, later
the web) and `DirFs` (native), plus case-insensitive lookup for OLX data.

- **Layer 1, uses `std`.** Everything else reaches files only through these traits.
- **`VPath` is the only path type.** It rejects `..`, absolute paths, empty
  components, backslashes and NUL, so content can never escape its root.
  OLX names with `\` go through `VPath::from_olx`.
- `list` returns sorted names with a trailing `/` for directories.
- `find_ci` prefers exact matches, then the first case-insensitive match in sorted order.

Test: `cargo nextest run -p rl-vfs`.
```

- [ ] **Step 3: Create `crates/rl-vfs/src/lib.rs`**

```rust
//! Content I/O abstraction.
//!
//! All content is addressed by [`VPath`]: a relative, `/`-separated path with no
//! `..`, so a malicious mod or pack can never reach outside its root.
//! Backends: [`MemFs`] (tests, and later the web) and [`DirFs`] (native).

mod dir;
mod lookup;
mod mem;
mod path;

pub use dir::DirFs;
pub use lookup::{find_ci, find_ci_any};
pub use mem::MemFs;
pub use path::VPath;

/// Errors from content backends.
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid path {path:?}: {reason}")]
    InvalidPath { path: String, reason: &'static str },
    #[error("i/o error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Read-only access to a tree of files.
pub trait ContentSource {
    /// The bytes of the file at `path`.
    fn read(&self, path: &VPath) -> Result<Vec<u8>, VfsError>;

    /// Names (not full paths) of the entries directly inside `dir`, sorted.
    /// Directories are listed with a trailing `/`. The root is `VPath::root()`.
    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError>;

    /// Whether a file exists at `path`.
    fn exists(&self, path: &VPath) -> bool {
        self.read(path).is_ok()
    }
}

/// Write access to a tree of files.
pub trait ContentSink {
    /// Creates or replaces the file at `path`, creating parent directories.
    fn write(&mut self, path: &VPath, data: &[u8]) -> Result<(), VfsError>;
}

/// All file paths under `dir`, recursively, sorted.
pub fn walk(src: &dyn ContentSource, dir: &VPath) -> Result<Vec<VPath>, VfsError> {
    let mut out = Vec::new();
    for name in src.list(dir)? {
        if let Some(sub) = name.strip_suffix('/') {
            out.extend(walk(src, &dir.join(sub)?)?);
        } else {
            out.push(dir.join(&name)?);
        }
    }
    out.sort();
    Ok(out)
}
```

- [ ] **Step 4: Create `crates/rl-vfs/src/path.rs`**

```rust
use std::fmt;

use crate::VfsError;

/// A validated relative path: components separated by `/`, none of them empty,
/// `.` or `..`, no leading `/`, no backslashes and no NUL. The empty path is the root.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VPath(String);

impl VPath {
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Validates `s`. Use [`VPath::from_olx`] for OLX names that may contain `\`.
    pub fn new(s: &str) -> Result<Self, VfsError> {
        if s.is_empty() {
            return Ok(Self::root());
        }
        let bad = |reason| Err(VfsError::InvalidPath { path: s.to_string(), reason });
        if s.contains('\\') {
            return bad("contains a backslash");
        }
        if s.contains('\0') {
            return bad("contains NUL");
        }
        if s.starts_with('/') {
            return bad("is absolute");
        }
        for c in s.split('/') {
            match c {
                "" => return bad("has an empty component"),
                "." | ".." => return bad("has a . or .. component"),
                _ => {}
            }
        }
        Ok(Self(s.to_string()))
    }

    /// Converts a file name found inside OLX data: `\` becomes `/`, then the
    /// result is validated like [`VPath::new`].
    pub fn from_olx(s: &str) -> Result<Self, VfsError> {
        Self::new(&s.replace('\\', "/"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// `self/rel`, where `rel` is validated.
    pub fn join(&self, rel: &str) -> Result<Self, VfsError> {
        let rel = VPath::new(rel)?;
        if self.is_root() {
            Ok(rel)
        } else if rel.is_root() {
            Ok(self.clone())
        } else {
            Ok(Self(format!("{}/{}", self.0, rel.0)))
        }
    }

    /// The last component, or `""` for the root.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }

    /// Everything before the last component; the root for single-component paths.
    pub fn parent(&self) -> VPath {
        match self.0.rfind('/') {
            Some(i) => Self(self.0[..i].to_string()),
            None => Self::root(),
        }
    }

    /// The components of the path.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|c| !c.is_empty())
    }

    /// `self` with `prefix` removed, if `self` is inside `prefix`.
    pub fn strip_prefix(&self, prefix: &VPath) -> Option<VPath> {
        if prefix.is_root() {
            return Some(self.clone());
        }
        let rest = self.0.strip_prefix(&prefix.0)?;
        if rest.is_empty() { Some(Self::root()) } else { rest.strip_prefix('/').map(|r| Self(r.to_string())) }
    }
}

impl fmt::Display for VPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_paths() {
        assert_eq!(VPath::new("gfx/missile.png").unwrap().as_str(), "gfx/missile.png");
        assert!(VPath::new("").unwrap().is_root());
        assert_eq!(VPath::new("MW 1.0/script.lgs").unwrap().file_name(), "script.lgs");
    }

    #[test]
    fn rejects_escapes() {
        for bad in ["../x", "a/../b", "/etc/passwd", "a//b", "a/", "./a", "a\\b", "a\0b"] {
            assert!(VPath::new(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn from_olx_converts_backslashes_but_still_rejects_escapes() {
        assert_eq!(VPath::from_olx("gfx\\a.png").unwrap().as_str(), "gfx/a.png");
        assert!(VPath::from_olx("..\\..\\boot.ini").is_err());
    }

    #[test]
    fn join_parent_strip() {
        let p = VPath::new("a/b").unwrap().join("c.txt").unwrap();
        assert_eq!(p.as_str(), "a/b/c.txt");
        assert_eq!(p.parent().as_str(), "a/b");
        assert_eq!(VPath::new("x").unwrap().parent(), VPath::root());
        assert_eq!(p.strip_prefix(&VPath::new("a").unwrap()).unwrap().as_str(), "b/c.txt");
        assert!(p.strip_prefix(&VPath::new("ab").unwrap()).is_none());
        assert!(VPath::root().join("../x").is_err());
    }

    proptest::proptest! {
        /// No string that `VPath` accepts can start with `/` or contain a `..` component.
        #[test]
        fn accepted_paths_never_escape(s in ".{0,40}") {
            if let Ok(p) = VPath::new(&s) {
                proptest::prop_assert!(!p.as_str().starts_with('/'));
                proptest::prop_assert!(p.components().all(|c| c != ".." && c != "."));
            }
        }
    }
}
```

- [ ] **Step 5: Create `crates/rl-vfs/src/mem.rs`**

```rust
use std::collections::BTreeMap;

use crate::{ContentSink, ContentSource, VPath, VfsError};

/// An in-memory file tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemFs {
    files: BTreeMap<VPath, Vec<u8>>,
}

impl MemFs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file; panics if `path` is invalid. Meant for tests and fixtures.
    pub fn with(mut self, path: &str, data: impl Into<Vec<u8>>) -> Self {
        self.files.insert(VPath::new(path).expect("valid path"), data.into());
        self
    }

    /// All files, sorted by path.
    pub fn files(&self) -> impl Iterator<Item = (&VPath, &[u8])> {
        self.files.iter().map(|(p, d)| (p, d.as_slice()))
    }
}

impl ContentSource for MemFs {
    fn read(&self, path: &VPath) -> Result<Vec<u8>, VfsError> {
        self.files.get(path).cloned().ok_or_else(|| VfsError::NotFound(path.to_string()))
    }

    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError> {
        let mut names = std::collections::BTreeSet::new();
        for p in self.files.keys() {
            if let Some(rest) = p.strip_prefix(dir) {
                if rest.is_root() {
                    continue;
                }
                let mut comps = rest.components();
                let first = comps.next().unwrap_or_default();
                if comps.next().is_some() {
                    names.insert(format!("{first}/"));
                } else {
                    names.insert(first.to_string());
                }
            }
        }
        if names.is_empty() && !dir.is_root() {
            return Err(VfsError::NotFound(dir.to_string()));
        }
        Ok(names.into_iter().collect())
    }
}

impl ContentSink for MemFs {
    fn write(&mut self, path: &VPath, data: &[u8]) -> Result<(), VfsError> {
        self.files.insert(path.clone(), data.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk;

    fn fs() -> MemFs {
        MemFs::new().with("a.txt", "1").with("d/b.txt", "2").with("d/e/c.txt", "3")
    }

    #[test]
    fn read_and_list() {
        let fs = fs();
        assert_eq!(fs.read(&VPath::new("d/b.txt").unwrap()).unwrap(), b"2");
        assert_eq!(fs.list(&VPath::root()).unwrap(), vec!["a.txt", "d/"]);
        assert_eq!(fs.list(&VPath::new("d").unwrap()).unwrap(), vec!["b.txt", "e/"]);
        assert!(matches!(fs.list(&VPath::new("nope").unwrap()), Err(VfsError::NotFound(_))));
    }

    #[test]
    fn walk_is_recursive_and_sorted() {
        let all: Vec<String> = walk(&fs(), &VPath::root()).unwrap().iter().map(|p| p.to_string()).collect();
        assert_eq!(all, vec!["a.txt", "d/b.txt", "d/e/c.txt"]);
    }
}
```

- [ ] **Step 6: Create `crates/rl-vfs/src/dir.rs`**

```rust
use std::path::{Path, PathBuf};

use crate::{ContentSink, ContentSource, VPath, VfsError};

/// A directory on the native file system. Paths are resolved below `root` only.
#[derive(Clone, Debug)]
pub struct DirFs {
    root: PathBuf,
}

impl DirFs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn native(&self, p: &VPath) -> PathBuf {
        let mut out = self.root.clone();
        for c in p.components() {
            out.push(c);
        }
        out
    }
}

fn io(path: &VPath, source: std::io::Error) -> VfsError {
    if source.kind() == std::io::ErrorKind::NotFound {
        VfsError::NotFound(path.to_string())
    } else {
        VfsError::Io { path: path.to_string(), source }
    }
}

impl ContentSource for DirFs {
    fn read(&self, path: &VPath) -> Result<Vec<u8>, VfsError> {
        std::fs::read(self.native(path)).map_err(|e| io(path, e))
    }

    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(self.native(dir)).map_err(|e| io(dir, e))? {
            let entry = entry.map_err(|e| io(dir, e))?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue; // non-UTF-8 names cannot be addressed by VPath
            };
            if VPath::new(&name).is_err() {
                continue;
            }
            let is_dir = entry.file_type().map_err(|e| io(dir, e))?.is_dir();
            names.push(if is_dir { format!("{name}/") } else { name });
        }
        names.sort();
        Ok(names)
    }
}

impl ContentSink for DirFs {
    fn write(&mut self, path: &VPath, data: &[u8]) -> Result<(), VfsError> {
        let native = self.native(path);
        if let Some(parent) = native.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(path, e))?;
        }
        std::fs::write(native, data).map_err(|e| io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk;

    #[test]
    fn write_read_list() {
        let tmp = tempfile::tempdir().unwrap();
        let mut fs = DirFs::new(tmp.path());
        fs.write(&VPath::new("x/y/z.bin").unwrap(), b"hi").unwrap();
        fs.write(&VPath::new("top.txt").unwrap(), b"t").unwrap();
        assert_eq!(fs.read(&VPath::new("x/y/z.bin").unwrap()).unwrap(), b"hi");
        assert_eq!(fs.list(&VPath::root()).unwrap(), vec!["top.txt", "x/"]);
        let all: Vec<String> = walk(&fs, &VPath::root()).unwrap().iter().map(|p| p.to_string()).collect();
        assert_eq!(all, vec!["top.txt", "x/y/z.bin"]);
        assert!(matches!(fs.read(&VPath::new("missing").unwrap()), Err(VfsError::NotFound(_))));
    }
}
```

- [ ] **Step 7: Create `crates/rl-vfs/src/lookup.rs`**

```rust
//! Case-insensitive lookup, needed because OLX content names files inconsistently
//! (`GFX/`, `Gfx/`, `SCRIPT.LGS`) and OLX itself matches case-insensitively.

use crate::{ContentSource, VPath};

/// Resolves `path` component by component, ignoring ASCII case. Exact matches win;
/// among case-insensitive matches the first in sorted order wins, as on OLX.
pub fn find_ci(src: &dyn ContentSource, path: &VPath) -> Option<VPath> {
    let mut resolved = VPath::root();
    let comps: Vec<&str> = path.components().collect();
    for (i, want) in comps.iter().enumerate() {
        let last = i + 1 == comps.len();
        let entries = src.list(&resolved).ok()?;
        let pick = |exact: bool| {
            entries.iter().find(|e| {
                let (name, is_dir) = match e.strip_suffix('/') {
                    Some(n) => (n, true),
                    None => (e.as_str(), false),
                };
                (is_dir != last) && if exact { name == *want } else { name.eq_ignore_ascii_case(want) }
            })
        };
        let entry = pick(true).or_else(|| pick(false))?;
        resolved = resolved.join(entry.trim_end_matches('/')).ok()?;
    }
    Some(resolved)
}

/// The first of `candidates` that [`find_ci`] resolves.
pub fn find_ci_any(src: &dyn ContentSource, candidates: &[VPath]) -> Option<VPath> {
    candidates.iter().find_map(|c| find_ci(src, c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemFs;

    #[test]
    fn resolves_mixed_case() {
        let fs = MemFs::new().with("Classic/GFX/Missile.png", "m").with("Classic/script.lgs", "s");
        let p = find_ci(&fs, &VPath::new("classic/gfx/missile.PNG").unwrap()).unwrap();
        assert_eq!(p.as_str(), "Classic/GFX/Missile.png");
        assert!(find_ci(&fs, &VPath::new("classic/gfx").unwrap()).is_none(), "directory is not a file");
        assert!(find_ci(&fs, &VPath::new("classic/nope.png").unwrap()).is_none());
    }

    #[test]
    fn exact_match_wins() {
        let fs = MemFs::new().with("a/X.png", "upper").with("a/x.png", "lower");
        assert_eq!(find_ci(&fs, &VPath::new("a/x.png").unwrap()).unwrap().as_str(), "a/x.png");
    }

    #[test]
    fn any_takes_first_hit() {
        let fs = MemFs::new().with("data/gfx/a.png", "d");
        let c = [VPath::new("mod/gfx/a.png").unwrap(), VPath::new("data/gfx/A.png").unwrap()];
        assert_eq!(find_ci_any(&fs, &c).unwrap().as_str(), "data/gfx/a.png");
    }
}
```

- [ ] **Step 8: Replace `xtask/layers.ron`**

```ron
// Crate layering (spec §4). A crate may depend on crates in lower layers, and on
// same-layer crates only when the pair is listed in `same_layer`. Every
// workspace crate must be listed here. `no_std` crates must declare
// #![no_std]; `wasm` crates must build for wasm32-unknown-unknown.
(
    layers: {
        "rl-data": 1,
        "rl-vfs": 1,
        "xtask": 6,
    },
    same_layer: [],
    no_std: ["rl-data"],
    wasm: ["rl-data", "rl-vfs"],
)
```

- [ ] **Step 9: Run**

```bash
cargo nextest run -p rl-vfs
```

Expected: output contains `11 passed`. Includes `rejects_escapes` and the property test `accepted_paths_never_escape`.

- [ ] **Step 10: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "rl-vfs: add content sources, sinks and safe paths"
```


---

### Task 5: `rl-pack`: native pack read/write

Implements the on-disk pack format for all four kinds, with deterministic output and positioned RON errors.

**Files:**

- Create: `crates/rl-pack/{Cargo.toml,CLAUDE.md}`
- Create: `crates/rl-pack/src/{lib,error,ron_io,hash,image_io,mod_pack,level_pack,skin_pack,theme_pack}.rs`
- Modify: `xtask/layers.ron` (register `rl-pack`, layer 2, `wasm`)

**Interfaces:**

- Produces: `ModPack { manifest, def: ModDef, assets: BTreeMap<VPath, Vec<u8>> }`, `LevelPack { manifest, level }`, `SkinPack { manifest, skin }`, `ThemePack { manifest, theme }`.
- Produces: `read_mod/write_mod`, `read_level/write_level`, `read_skin/write_skin`, `read_theme/write_theme`: `fn read_x(&dyn ContentSource) -> Result<XPack, PackError>`, `fn write_x(&mut dyn ContentSink, &XPack) -> Result<(), PackError>`.
- Produces: `read_manifest`, `write_manifest`, `content_hash(&dyn ContentSource) -> Result<String, PackError>` (64 hex characters), `MANIFEST = "pack.ron"`.
- Produces: `PackError::{Vfs, Ron{file,line,col,message}, Image{file,message}, Invalid{file,message}}`.
- Produces: `rl_pack::image_io::{encode_rgba, decode_rgba, encode_indexed, decode_indexed}`. `decode_rgba` accepts any PNG kind; `decode_indexed` accepts only 8-bit palette PNGs.
- Test helpers for later tasks: `mod_pack::tests::{manifest(kind), sample_mod()}` and `level_pack::tests::sample_level()`, all `pub(crate)`.

- [ ] **Step 1: Create `crates/rl-pack/Cargo.toml`**

```toml
[package]
name = "rl-pack"
description = "Native pack format: read, write and validate mod, level, skin and theme packs."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
rl-data.workspace = true
rl-vfs.workspace = true
serde = { workspace = true, features = ["std"] }
ron.workspace = true
png.workspace = true
sha2.workspace = true
hex.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/rl-pack/CLAUDE.md`**

```markdown
# rl-pack

The native pack format (spec §9.1): `pack.ron` plus kind-specific files, for
mod, level, skin and theme packs; PNG helpers; content hashing; validation.

- **Layer 2.** Depends on `rl-data`, `rl-vfs`, `ron`, `png`, `sha2`.
- **Deterministic output.** Writing the same value twice gives byte-identical
  files. Never introduce `HashMap` iteration or timestamps into output.
- **Layouts:**
  - mod: `mod.ron`, `weapons/<id>.ron`, `projectiles/<name>.ron`, assets (`gfx/`, `sfx/`)
  - level: `level.ron`, `material.png` (8-bit palette; index = `Material`), `front.png`, `back.png`
  - skin: `skin.ron`, `skin.png`, `mask.png` (palette: 0 keep, 1 colorize, 2 transparent)
  - theme: `theme.ron`, `holes/N.png`, `stones/N.png`, `misc/N.png`, `front_tile.png`, `back_tile.png`
- `validate` never panics and reports `file:line:col` for RON errors.
  Errors: unknown references, cycles, missing assets. Warnings: unused items.
- Bump `PACK_FORMAT_VERSION` (in `rl-data`) when a layout changes, and add a migration here.

Test: `cargo nextest run -p rl-pack`.
```

- [ ] **Step 3: Create `crates/rl-pack/src/lib.rs`**

```rust
//! The native pack format (spec §9.1).
//!
//! A pack is a directory tree reachable through an [`rl_vfs::ContentSource`]
//! whose root holds `pack.ron` ([`rl_data::manifest::PackManifest`]).
//! Readers and writers are deterministic: writing the same value twice yields
//! byte-identical files.

mod error;
mod hash;
pub mod image_io;
mod level_pack;
mod mod_pack;
mod ron_io;
mod skin_pack;
mod theme_pack;

pub use error::PackError;
pub use hash::content_hash;
pub use level_pack::{LevelPack, read_level, write_level};
pub use mod_pack::{ModPack, read_mod, write_mod};
pub use ron_io::{read_manifest, write_manifest};
pub use skin_pack::{SkinPack, read_skin, write_skin};
pub use theme_pack::{ThemePack, read_theme, write_theme};

/// Path of the manifest inside every pack.
pub const MANIFEST: &str = "pack.ron";
```

- [ ] **Step 4: Create `crates/rl-pack/src/error.rs`**

```rust
use rl_vfs::VfsError;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error(transparent)]
    Vfs(#[from] VfsError),
    #[error("{file}:{line}:{col}: {message}")]
    Ron { file: String, line: usize, col: usize, message: String },
    #[error("{file}: {message}")]
    Image { file: String, message: String },
    #[error("{file}: {message}")]
    Invalid { file: String, message: String },
}

impl PackError {
    pub(crate) fn invalid(file: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Invalid { file: file.into(), message: message.into() }
    }

    /// The file the error refers to, when there is one.
    pub fn file(&self) -> Option<&str> {
        match self {
            Self::Vfs(_) => None,
            Self::Ron { file, .. } | Self::Image { file, .. } | Self::Invalid { file, .. } => Some(file),
        }
    }
}
```

- [ ] **Step 5: Create `crates/rl-pack/src/ron_io.rs`**

```rust
use rl_data::manifest::{PACK_FORMAT_VERSION, PackManifest};
use rl_vfs::{ContentSink, ContentSource, VPath};
use serde::{Serialize, de::DeserializeOwned};

use crate::{MANIFEST, PackError};

/// Serializes `value` as pretty RON with a trailing newline.
pub(crate) fn to_ron<T: Serialize>(value: &T) -> String {
    let cfg = ron::ser::PrettyConfig::default().indentor("    ".to_string());
    let mut s = ron::ser::to_string_pretty(value, cfg).expect("content types always serialize");
    s.push('\n');
    s
}

pub(crate) fn read_ron<T: DeserializeOwned>(src: &dyn ContentSource, file: &str) -> Result<T, PackError> {
    let bytes = src.read(&VPath::new(file)?)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|e| PackError::invalid(file, format!("not UTF-8: {e}")))?;
    ron::from_str(text).map_err(|e| PackError::Ron {
        file: file.to_string(),
        line: e.span.start.line,
        col: e.span.start.col,
        message: e.code.to_string(),
    })
}

pub(crate) fn write_ron<T: Serialize>(
    sink: &mut dyn ContentSink,
    file: &str,
    value: &T,
) -> Result<(), PackError> {
    sink.write(&VPath::new(file)?, to_ron(value).as_bytes())?;
    Ok(())
}

/// Reads `pack.ron` and checks its format version.
pub fn read_manifest(src: &dyn ContentSource) -> Result<PackManifest, PackError> {
    let m: PackManifest = read_ron(src, MANIFEST)?;
    if m.format_version != PACK_FORMAT_VERSION {
        return Err(PackError::invalid(
            MANIFEST,
            format!(
                "unsupported format_version {} (this build reads {PACK_FORMAT_VERSION})",
                m.format_version
            ),
        ));
    }
    Ok(m)
}

pub fn write_manifest(sink: &mut dyn ContentSink, m: &PackManifest) -> Result<(), PackError> {
    write_ron(sink, MANIFEST, m)
}
```

- [ ] **Step 6: Create `crates/rl-pack/src/hash.rs`**

```rust
use rl_vfs::{ContentSource, VPath, walk};
use sha2::{Digest, Sha256};

use crate::{MANIFEST, PackError};

/// SHA-256 over every file of the pack except `pack.ron`, as lowercase hex.
/// Files are taken in sorted path order; each contributes its path, a 0 byte,
/// its length as little-endian u64, and its bytes. Replays use this hash to
/// identify the exact content they were recorded with.
pub fn content_hash(src: &dyn ContentSource) -> Result<String, PackError> {
    let mut h = Sha256::new();
    for path in walk(src, &VPath::root())? {
        if path.as_str() == MANIFEST {
            continue;
        }
        let data = src.read(&path)?;
        h.update(path.as_str().as_bytes());
        h.update([0u8]);
        h.update((data.len() as u64).to_le_bytes());
        h.update(&data);
    }
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_vfs::MemFs;

    #[test]
    fn ignores_manifest_and_depends_on_paths_and_bytes() {
        let a = MemFs::new().with("pack.ron", "x").with("a.txt", "1");
        let b = MemFs::new().with("pack.ron", "y").with("a.txt", "1");
        let c = MemFs::new().with("pack.ron", "x").with("b.txt", "1");
        let d = MemFs::new().with("pack.ron", "x").with("a.txt", "2");
        assert_eq!(content_hash(&a).unwrap(), content_hash(&b).unwrap());
        assert_ne!(content_hash(&a).unwrap(), content_hash(&c).unwrap());
        assert_ne!(content_hash(&a).unwrap(), content_hash(&d).unwrap());
        assert_eq!(content_hash(&a).unwrap().len(), 64);
    }
}
```

- [ ] **Step 7: Create `crates/rl-pack/src/image_io.rs`**

```rust
//! PNG encoding and decoding for packs.

use rl_data::image::RgbaImage;

/// Encodes an RGBA8 PNG.
pub fn encode_rgba(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width(), img.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("in-memory PNG header");
        w.write_image_data(img.as_raw()).expect("in-memory PNG data");
    }
    out
}

/// Decodes any PNG (palette, gray, RGB, RGBA, 1–16 bit) to RGBA8.
pub fn decode_rgba(bytes: &[u8]) -> Result<RgbaImage, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("palette was not expanded".into()),
    };
    RgbaImage::from_raw(info.width, info.height, rgba).ok_or_else(|| "decoded size mismatch".into())
}

/// Encodes an 8-bit palette PNG. `palette` holds RGB triples; every index must be
/// smaller than `palette.len() / 3`.
pub fn encode_indexed(width: u32, height: u32, indices: &[u8], palette: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(palette.to_vec());
        let mut w = enc.write_header().expect("in-memory PNG header");
        w.write_image_data(indices).expect("in-memory PNG data");
    }
    out
}

/// Decodes an 8-bit palette PNG to `(width, height, indices)`, without expanding
/// the palette. Any other PNG kind is an error, because the indices are the data.
pub fn decode_indexed(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::IDENTITY);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.color_type != png::ColorType::Indexed || info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "expected an 8-bit palette PNG, found {:?} at {:?} bits",
            info.color_type, info.bit_depth
        ));
    }
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::image::Rgba;

    #[test]
    fn rgba_roundtrip() {
        let mut img = RgbaImage::new(3, 2);
        img.set(0, 0, Rgba::rgb(1, 2, 3));
        img.set(2, 1, Rgba { r: 4, g: 5, b: 6, a: 7 });
        assert_eq!(decode_rgba(&encode_rgba(&img)).unwrap(), img);
    }

    #[test]
    fn rgba_encoding_is_deterministic() {
        let img = RgbaImage::new(8, 8);
        assert_eq!(encode_rgba(&img), encode_rgba(&img));
    }

    #[test]
    fn indexed_roundtrip_and_rgba_expansion() {
        let png = encode_indexed(2, 1, &[0, 1], &[10, 20, 30, 40, 50, 60]);
        assert_eq!(decode_indexed(&png).unwrap(), (2, 1, vec![0, 1]));
        let rgba = decode_rgba(&png).unwrap();
        assert_eq!(rgba.get(1, 0), Rgba::rgb(40, 50, 60));
    }

    #[test]
    fn indexed_rejects_rgba_png() {
        let png = encode_rgba(&RgbaImage::new(1, 1));
        assert!(decode_indexed(&png).unwrap_err().contains("palette"));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_rgba(b"not a png").is_err());
        assert!(decode_indexed(&[0x89, b'P', b'N', b'G']).is_err());
    }
}
```

- [ ] **Step 8: Create `crates/rl-pack/src/mod_pack.rs`**

```rust
//! Mod packs: `mod.ron`, `weapons/<id>.ron`, `projectiles/<name>.ron`, and the
//! asset files (`gfx/`, `sfx/`) the definitions reference.

use std::collections::BTreeMap;

use rl_data::manifest::{PackKind, PackManifest};
use rl_data::mod_def::{ModDef, ProjectileDef, RopeParams, WeaponDef, WormParams};
use rl_vfs::{ContentSink, ContentSource, VPath, walk};
use serde::{Deserialize, Serialize};

use crate::ron_io::{read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const MOD_FILE: &str = "mod.ron";
const WEAPONS_DIR: &str = "weapons";
const PROJECTILES_DIR: &str = "projectiles";

#[derive(Clone, Debug, PartialEq)]
pub struct ModPack {
    pub manifest: PackManifest,
    pub def: ModDef,
    /// Asset files by pack path, such as `gfx/missile.png`.
    pub assets: BTreeMap<VPath, Vec<u8>>,
}

/// On-disk form of `mod.ron`: everything but the weapon and projectile bodies.
#[derive(Serialize, Deserialize)]
struct ModFile {
    name: String,
    worm: WormParams,
    rope: RopeParams,
    /// Weapon ids in order; each is a file `weapons/<id>.ron`.
    weapons: Vec<String>,
}

fn check_name(file: &str, kind: &str, name: &str) -> Result<(), PackError> {
    let ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.' || b == b'_')
        && !name.starts_with('.');
    if ok { Ok(()) } else { Err(PackError::invalid(file, format!("invalid {kind} name {name:?}"))) }
}

pub fn write_mod(sink: &mut dyn ContentSink, pack: &ModPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Mod {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Mod"));
    }
    write_manifest(sink, &pack.manifest)?;
    let def = &pack.def;
    let file = ModFile {
        name: def.name.clone(),
        worm: def.worm,
        rope: def.rope,
        weapons: def.weapons.iter().map(|w| w.id.clone()).collect(),
    };
    write_ron(sink, MOD_FILE, &file)?;
    for w in &def.weapons {
        check_name(MOD_FILE, "weapon", &w.id)?;
        write_ron(sink, &format!("{WEAPONS_DIR}/{}.ron", w.id), w)?;
    }
    for (name, p) in &def.projectiles {
        check_name(MOD_FILE, "projectile", name)?;
        write_ron(sink, &format!("{PROJECTILES_DIR}/{name}.ron"), p)?;
    }
    for (path, data) in &pack.assets {
        sink.write(path, data)?;
    }
    Ok(())
}

pub fn read_mod(src: &dyn ContentSource) -> Result<ModPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Mod {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Mod"));
    }
    let file: ModFile = read_ron(src, MOD_FILE)?;
    let mut weapons = Vec::with_capacity(file.weapons.len());
    for id in &file.weapons {
        check_name(MOD_FILE, "weapon", id)?;
        let mut w: WeaponDef = read_ron(src, &format!("{WEAPONS_DIR}/{id}.ron"))?;
        w.id = id.clone();
        weapons.push(w);
    }
    let mut projectiles = BTreeMap::new();
    let mut assets = BTreeMap::new();
    for path in walk(src, &VPath::root())? {
        let s = path.as_str();
        if s == MANIFEST || s == MOD_FILE || s.starts_with("weapons/") {
            continue;
        }
        if let Some(rest) = s.strip_prefix("projectiles/") {
            let name = rest
                .strip_suffix(".ron")
                .ok_or_else(|| PackError::invalid(s, "projectile files must end in .ron"))?;
            check_name(s, "projectile", name)?;
            let p: ProjectileDef = read_ron(src, s)?;
            projectiles.insert(name.to_string(), p);
        } else {
            assets.insert(path.clone(), src.read(&path)?);
        }
    }
    let def = ModDef { name: file.name, worm: file.worm, rope: file.rope, weapons, projectiles };
    Ok(ModPack { manifest, def, assets })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rl_data::image::Rgba;
    use rl_data::manifest::PACK_FORMAT_VERSION;
    use rl_data::mod_def::*;
    use rl_vfs::MemFs;

    pub(crate) fn manifest(kind: PackKind) -> PackManifest {
        PackManifest {
            format_version: PACK_FORMAT_VERSION,
            id: "test-pack".into(),
            name: "Test".into(),
            version: "1".into(),
            kind,
            authors: vec![],
            origin: None,
            license: None,
        }
    }

    pub(crate) fn sample_mod() -> ModPack {
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
            hit: ActionDef {
                damage: 10,
                spawns_projectiles: true,
                sound: Some("sfx/boom.ogg".into()),
                ..ActionDef::olx_default(ActionKind::Explode)
            },
            event_spawn: Some(SpawnDef { amount: 5, ..SpawnDef::olx_default("bits".into()) }),
            ..bits.clone()
        };
        let weapon = WeaponDef {
            id: "rocket".into(),
            name: "Rocket".into(),
            recharge: 0.5,
            drain: 30.0,
            rof: 0.25,
            laser_sight: false,
            kind: WeaponKind::Projectile {
                class: WeaponClass::Missile,
                recoil: 5,
                sound: Some("sfx/boom.ogg".into()),
                spawn: SpawnDef { amount: 1, speed: 100, ..SpawnDef::olx_default("rocket".into()) },
            },
        };
        ModPack {
            manifest: manifest(PackKind::Mod),
            def: ModDef {
                name: "Test Mod".into(),
                worm: WormParams {
                    angle_speed: 150.0,
                    ground_speed: 8.0,
                    air_speed: 4.0,
                    gravity: 100.0,
                    jump_force: -65.0,
                    air_friction: 0.0,
                    ground_friction: 0.4,
                },
                rope: RopeParams { max_length: 300, rest_length: 27, strength: 4.5 },
                weapons: vec![weapon],
                projectiles: [("bits".to_string(), bits), ("rocket".to_string(), rocket)]
                    .into_iter()
                    .collect(),
            },
            assets: [
                (VPath::new("gfx/missile.png").unwrap(), b"png".to_vec()),
                (VPath::new("sfx/boom.ogg").unwrap(), b"ogg".to_vec()),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn roundtrip() {
        let pack = sample_mod();
        let mut fs = MemFs::new();
        write_mod(&mut fs, &pack).unwrap();
        assert_eq!(read_mod(&fs).unwrap(), pack);
    }

    #[test]
    fn writes_expected_layout() {
        let mut fs = MemFs::new();
        write_mod(&mut fs, &sample_mod()).unwrap();
        let paths: Vec<String> = fs.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(
            paths,
            vec![
                "gfx/missile.png",
                "mod.ron",
                "pack.ron",
                "projectiles/bits.ron",
                "projectiles/rocket.ron",
                "sfx/boom.ogg",
                "weapons/rocket.ron"
            ]
        );
    }

    #[test]
    fn writing_twice_is_byte_identical() {
        let (mut a, mut b) = (MemFs::new(), MemFs::new());
        write_mod(&mut a, &sample_mod()).unwrap();
        write_mod(&mut b, &sample_mod()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_unsafe_weapon_id() {
        let mut pack = sample_mod();
        pack.def.weapons[0].id = "../evil".into();
        assert!(write_mod(&mut MemFs::new(), &pack).is_err());
    }

    #[test]
    fn ron_errors_have_positions() {
        let mut fs = MemFs::new();
        write_mod(&mut fs, &sample_mod()).unwrap();
        let fs = fs.with("weapons/rocket.ron", "(\n  name: 5,\n)");
        match read_mod(&fs).unwrap_err() {
            PackError::Ron { file, line, .. } => {
                assert_eq!(file, "weapons/rocket.ron");
                assert_eq!(line, 2);
            }
            e => panic!("unexpected error {e}"),
        }
    }
}
```

- [ ] **Step 9: Create `crates/rl-pack/src/level_pack.rs`**

```rust
//! Level packs: `level.ron`, `material.png` (8-bit palette PNG whose indices are
//! [`Material`] values), and `front.png` / `back.png` at `art_scale`.

use rl_data::image::RgbaImage;
use rl_data::level::{Level, LevelDef, Material, MaterialGrid};
use rl_data::manifest::{PackKind, PackManifest};
use rl_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{decode_indexed, decode_rgba, encode_indexed, encode_rgba};
use crate::ron_io::{read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const LEVEL_FILE: &str = "level.ron";
pub const MATERIAL_FILE: &str = "material.png";
pub const FRONT_FILE: &str = "front.png";
pub const BACK_FILE: &str = "back.png";

/// Palette for `material.png`, indexed by `Material as u8`: rock is gray,
/// background black, dirt brown, so the file is readable in any image viewer.
const MATERIAL_PALETTE: [u8; 9] = [128, 128, 128, 0, 0, 0, 139, 90, 43];

#[derive(Clone, Debug, PartialEq)]
pub struct LevelPack {
    pub manifest: PackManifest,
    pub level: Level,
}

fn check_dims(level: &Level) -> Result<(), PackError> {
    let d = &level.def;
    if d.art_scale == 0 {
        return Err(PackError::invalid(LEVEL_FILE, "art_scale must be at least 1"));
    }
    if (level.material.width(), level.material.height()) != (d.width, d.height) {
        return Err(PackError::invalid(MATERIAL_FILE, format!("material must be {}x{}", d.width, d.height)));
    }
    let (aw, ah) = (d.width * d.art_scale, d.height * d.art_scale);
    for (file, img) in [(FRONT_FILE, &level.front), (BACK_FILE, &level.back)] {
        if (img.width(), img.height()) != (aw, ah) {
            return Err(PackError::invalid(file, format!("art must be {aw}x{ah} (size x art_scale)")));
        }
    }
    Ok(())
}

pub fn write_level(sink: &mut dyn ContentSink, pack: &LevelPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Level {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Level"));
    }
    check_dims(&pack.level)?;
    let l = &pack.level;
    write_manifest(sink, &pack.manifest)?;
    write_ron(sink, LEVEL_FILE, &l.def)?;
    let indices: Vec<u8> = l.material.cells().iter().map(|m| *m as u8).collect();
    let mat_png = encode_indexed(l.material.width(), l.material.height(), &indices, &MATERIAL_PALETTE);
    sink.write(&VPath::new(MATERIAL_FILE)?, &mat_png)?;
    sink.write(&VPath::new(FRONT_FILE)?, &encode_rgba(&l.front))?;
    sink.write(&VPath::new(BACK_FILE)?, &encode_rgba(&l.back))?;
    Ok(())
}

fn read_png(src: &dyn ContentSource, file: &str) -> Result<RgbaImage, PackError> {
    let bytes = src.read(&VPath::new(file)?)?;
    decode_rgba(&bytes).map_err(|message| PackError::Image { file: file.into(), message })
}

pub fn read_level(src: &dyn ContentSource) -> Result<LevelPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Level {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Level"));
    }
    let def: LevelDef = read_ron(src, LEVEL_FILE)?;
    let bytes = src.read(&VPath::new(MATERIAL_FILE)?)?;
    let (w, h, indices) =
        decode_indexed(&bytes).map_err(|message| PackError::Image { file: MATERIAL_FILE.into(), message })?;
    let cells = indices
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            Material::from_index(v).ok_or_else(|| {
                let (x, y) = (i as u32 % w.max(1), i as u32 / w.max(1));
                PackError::invalid(MATERIAL_FILE, format!("unknown material index {v} at ({x},{y})"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let material = MaterialGrid::from_cells(w, h, cells).expect("decoder returns w*h indices");
    let level = Level { def, material, front: read_png(src, FRONT_FILE)?, back: read_png(src, BACK_FILE)? };
    check_dims(&level)?;
    Ok(LevelPack { manifest, level })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use rl_data::image::Rgba;
    use rl_vfs::MemFs;

    pub(crate) fn sample_level() -> LevelPack {
        let cells = vec![Material::Rock, Material::Background, Material::Dirt, Material::Dirt];
        let mut front = RgbaImage::new(4, 4);
        front.set(3, 3, Rgba::rgb(1, 2, 3));
        LevelPack {
            manifest: manifest(PackKind::Level),
            level: Level {
                def: LevelDef {
                    name: "Tiny".into(),
                    width: 2,
                    height: 2,
                    art_scale: 2,
                    theme: "olx-theme-dirt".into(),
                    spawns: vec![],
                },
                material: MaterialGrid::from_cells(2, 2, cells).unwrap(),
                front,
                back: RgbaImage::new(4, 4),
            },
        }
    }

    #[test]
    fn roundtrip() {
        let pack = sample_level();
        let mut fs = MemFs::new();
        write_level(&mut fs, &pack).unwrap();
        assert_eq!(read_level(&fs).unwrap(), pack);
    }

    #[test]
    fn rejects_art_of_wrong_size() {
        let mut pack = sample_level();
        pack.level.front = RgbaImage::new(3, 4);
        assert!(write_level(&mut MemFs::new(), &pack).unwrap_err().to_string().contains("front.png"));
    }

    #[test]
    fn rejects_unknown_material_index() {
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        let bad = encode_indexed(2, 2, &[0, 1, 2, 7], &[0; 24]);
        let fs = fs.with(MATERIAL_FILE, bad);
        let err = read_level(&fs).unwrap_err().to_string();
        assert!(err.contains("unknown material index 7 at (1,1)"), "{err}");
    }

    #[test]
    fn rejects_rgb_material_png() {
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        let fs = fs.with(MATERIAL_FILE, encode_rgba(&RgbaImage::new(2, 2)));
        assert!(read_level(&fs).unwrap_err().to_string().contains("palette"));
    }
}
```

- [ ] **Step 10: Create `crates/rl-pack/src/skin_pack.rs`**

```rust
//! Skin packs: `skin.ron`, `skin.png` (RGBA sheet) and `mask.png`, an 8-bit
//! palette PNG with index 0 = keep (black), 1 = colorize (white),
//! 2 = transparent (magenta), matching OLX's mask colors.

use rl_data::manifest::{PackKind, PackManifest};
use rl_data::skin::{MaskKind, Skin, SkinDef};
use rl_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{decode_indexed, decode_rgba, encode_indexed, encode_rgba};
use crate::ron_io::{read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const SKIN_FILE: &str = "skin.ron";
pub const IMAGE_FILE: &str = "skin.png";
pub const MASK_FILE: &str = "mask.png";
const MASK_PALETTE: [u8; 9] = [0, 0, 0, 255, 255, 255, 255, 0, 255];

#[derive(Clone, Debug, PartialEq)]
pub struct SkinPack {
    pub manifest: PackManifest,
    pub skin: Skin,
}

fn mask_index(m: MaskKind) -> u8 {
    match m {
        MaskKind::Keep => 0,
        MaskKind::Colorize => 1,
        MaskKind::Transparent => 2,
    }
}

pub fn write_skin(sink: &mut dyn ContentSink, pack: &SkinPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Skin {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Skin"));
    }
    let s = &pack.skin;
    if s.mask.len() != s.image.width() as usize * s.image.height() as usize {
        return Err(PackError::invalid(MASK_FILE, "mask size does not match skin.png"));
    }
    write_manifest(sink, &pack.manifest)?;
    write_ron(sink, SKIN_FILE, &s.def)?;
    sink.write(&VPath::new(IMAGE_FILE)?, &encode_rgba(&s.image))?;
    let idx: Vec<u8> = s.mask.iter().map(|m| mask_index(*m)).collect();
    sink.write(
        &VPath::new(MASK_FILE)?,
        &encode_indexed(s.image.width(), s.image.height(), &idx, &MASK_PALETTE),
    )?;
    Ok(())
}

pub fn read_skin(src: &dyn ContentSource) -> Result<SkinPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Skin {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Skin"));
    }
    let def: SkinDef = read_ron(src, SKIN_FILE)?;
    let image = decode_rgba(&src.read(&VPath::new(IMAGE_FILE)?)?)
        .map_err(|message| PackError::Image { file: IMAGE_FILE.into(), message })?;
    let (w, h, idx) = decode_indexed(&src.read(&VPath::new(MASK_FILE)?)?)
        .map_err(|message| PackError::Image { file: MASK_FILE.into(), message })?;
    if (w, h) != (image.width(), image.height()) {
        return Err(PackError::invalid(MASK_FILE, "mask size does not match skin.png"));
    }
    let mask = idx
        .iter()
        .map(|v| match v {
            0 => Ok(MaskKind::Keep),
            1 => Ok(MaskKind::Colorize),
            2 => Ok(MaskKind::Transparent),
            v => Err(PackError::invalid(MASK_FILE, format!("unknown mask index {v}"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SkinPack { manifest, skin: Skin { def, image, mask } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use rl_data::image::{Rgba, RgbaImage};
    use rl_data::skin::FrameLayout;
    use rl_vfs::MemFs;

    #[test]
    fn roundtrip() {
        let mut image = RgbaImage::new(3, 1);
        image.set(1, 0, Rgba::rgb(9, 8, 7));
        let pack = SkinPack {
            manifest: manifest(PackKind::Skin),
            skin: Skin {
                def: SkinDef { name: "s".into(), layout: FrameLayout::OLX },
                image,
                mask: vec![MaskKind::Keep, MaskKind::Colorize, MaskKind::Transparent],
            },
        };
        let mut fs = MemFs::new();
        write_skin(&mut fs, &pack).unwrap();
        assert_eq!(read_skin(&fs).unwrap(), pack);
    }
}
```

- [ ] **Step 11: Create `crates/rl-pack/src/theme_pack.rs`**

```rust
//! Theme packs: `theme.ron`, `holes/<n>.png`, `stones/<n>.png`, `misc/<n>.png`
//! (numbered from 1), `front_tile.png`, `back_tile.png`.

use rl_data::image::RgbaImage;
use rl_data::manifest::{PackKind, PackManifest};
use rl_data::theme::{Theme, ThemeDef};
use rl_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{decode_rgba, encode_rgba};
use crate::ron_io::{read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const THEME_FILE: &str = "theme.ron";

#[derive(Clone, Debug, PartialEq)]
pub struct ThemePack {
    pub manifest: PackManifest,
    pub theme: Theme,
}

fn put(sink: &mut dyn ContentSink, file: &str, img: &RgbaImage) -> Result<(), PackError> {
    sink.write(&VPath::new(file)?, &encode_rgba(img))?;
    Ok(())
}

fn get(src: &dyn ContentSource, file: &str) -> Result<RgbaImage, PackError> {
    decode_rgba(&src.read(&VPath::new(file)?)?)
        .map_err(|message| PackError::Image { file: file.into(), message })
}

pub fn write_theme(sink: &mut dyn ContentSink, pack: &ThemePack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Theme {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Theme"));
    }
    let t = &pack.theme;
    let counts = (t.holes.len() as u32, t.stones.len() as u32, t.misc.len() as u32);
    if counts != (t.def.holes, t.def.stones, t.def.misc) {
        return Err(PackError::invalid(THEME_FILE, "image counts do not match theme.ron"));
    }
    write_manifest(sink, &pack.manifest)?;
    write_ron(sink, THEME_FILE, &t.def)?;
    for (dir, imgs) in [("holes", &t.holes), ("stones", &t.stones), ("misc", &t.misc)] {
        for (i, img) in imgs.iter().enumerate() {
            put(sink, &format!("{dir}/{}.png", i + 1), img)?;
        }
    }
    put(sink, "front_tile.png", &t.front_tile)?;
    put(sink, "back_tile.png", &t.back_tile)
}

pub fn read_theme(src: &dyn ContentSource) -> Result<ThemePack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Theme {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Theme"));
    }
    let def: ThemeDef = read_ron(src, THEME_FILE)?;
    let list = |dir: &str, n: u32| {
        (1..=n).map(|i| get(src, &format!("{dir}/{i}.png"))).collect::<Result<Vec<_>, _>>()
    };
    let theme = Theme {
        holes: list("holes", def.holes)?,
        stones: list("stones", def.stones)?,
        misc: list("misc", def.misc)?,
        front_tile: get(src, "front_tile.png")?,
        back_tile: get(src, "back_tile.png")?,
        def,
    };
    Ok(ThemePack { manifest, theme })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use rl_vfs::MemFs;

    #[test]
    fn roundtrip() {
        let img = RgbaImage::new(2, 2);
        let pack = ThemePack {
            manifest: manifest(PackKind::Theme),
            theme: Theme {
                def: ThemeDef { name: "Dirt".into(), holes: 2, stones: 1, misc: 0 },
                holes: vec![img.clone(), img.clone()],
                stones: vec![img.clone()],
                misc: vec![],
                front_tile: img.clone(),
                back_tile: img,
            },
        };
        let mut fs = MemFs::new();
        write_theme(&mut fs, &pack).unwrap();
        assert!(fs.exists(&VPath::new("holes/2.png").unwrap()));
        assert_eq!(read_theme(&fs).unwrap(), pack);
    }
}
```

- [ ] **Step 12: Replace `xtask/layers.ron`**

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
        "xtask": 6,
    },
    same_layer: [],
    no_std: ["rl-data"],
    wasm: ["rl-data", "rl-vfs", "rl-pack"],
)
```

- [ ] **Step 13: Run**

```bash
cargo nextest run -p rl-pack
```

Expected: output contains `17 passed`. Includes byte-identical rewrites, layout, rejection of unsafe ids, RON error positions, and material index and size checks.

- [ ] **Step 14: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 15: Commit**

```bash
git add -A
git commit -m "rl-pack: add native pack read and write"
```


---

### Task 6: `rl-pack`: validation

Adds `validate`, which reports every problem with file and position. It is used by `rl-cli validate` and the corpus test.

**Files:**

- Create: `crates/rl-pack/src/validate.rs`
- Modify: `crates/rl-pack/src/lib.rs` (add `mod validate;` and its re-export)

**Interfaces:**

- Produces: `validate(&dyn ContentSource) -> Vec<Diagnostic>`, errors first; an empty list means valid.
- Produces: `Diagnostic { severity: Severity, file: Option<String>, line: Option<usize>, col: Option<usize>, message: String }`, which displays as `error: file:line:col: message`; `Severity::{Error, Warning}`.
- Errors: unknown projectile references, spawn cycles, missing assets, duplicate weapon ids, a non-slug pack id, parse failures. Warnings: unused projectiles and unused assets.

- [ ] **Step 1: Create `crates/rl-pack/src/validate.rs`**

```rust
//! Pack validation with file and position information (spec §9.1).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rl_data::manifest::PackKind;
use rl_data::mod_def::ModDef;
use rl_vfs::{ContentSource, VPath};

use crate::{ModPack, PackError, read_level, read_manifest, read_mod, read_skin, read_theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
}

impl Diagnostic {
    fn new(severity: Severity, file: impl Into<String>, message: impl Into<String>) -> Self {
        Self { severity, file: Some(file.into()), line: None, col: None, message: message.into() }
    }

    fn from_error(e: &PackError) -> Self {
        match e {
            PackError::Ron { file, line, col, message } => Self {
                severity: Severity::Error,
                file: Some(file.clone()),
                line: Some(*line),
                col: Some(*col),
                message: message.clone(),
            },
            PackError::Image { file, message } | PackError::Invalid { file, message } => {
                Self::new(Severity::Error, file.clone(), message.clone())
            }
            PackError::Vfs(v) => {
                Self { severity: Severity::Error, file: None, line: None, col: None, message: v.to_string() }
            }
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{sev}: ")?;
        if let Some(file) = &self.file {
            write!(f, "{file}")?;
            if let (Some(l), Some(c)) = (self.line, self.col) {
                write!(f, ":{l}:{c}")?;
            }
            write!(f, ": ")?;
        }
        f.write_str(&self.message)
    }
}

/// Checks a pack and returns every problem found, errors first. An empty list
/// means the pack is valid.
pub fn validate(src: &dyn ContentSource) -> Vec<Diagnostic> {
    let manifest = match read_manifest(src) {
        Ok(m) => m,
        Err(e) => return vec![Diagnostic::from_error(&e)],
    };
    let mut out = Vec::new();
    let id_ok = !manifest.id.is_empty()
        && manifest.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !id_ok {
        out.push(Diagnostic::new(
            Severity::Error,
            "pack.ron",
            format!("id {:?} must be a lowercase slug", manifest.id),
        ));
    }
    let result = match manifest.kind {
        PackKind::Mod => read_mod(src).map(|p| out.extend(check_mod(&p))),
        PackKind::Level => read_level(src).map(|_| ()),
        PackKind::Skin => read_skin(src).map(|_| ()),
        PackKind::Theme => read_theme(src).map(|_| ()),
    };
    if let Err(e) = result {
        out.push(Diagnostic::from_error(&e));
    }
    out.sort_by_key(|d| d.severity);
    out
}

fn check_mod(pack: &ModPack) -> Vec<Diagnostic> {
    use Severity::*;
    let def = &pack.def;
    let mut out = Vec::new();

    let mut ids = BTreeSet::new();
    for w in &def.weapons {
        if !ids.insert(w.id.as_str()) {
            out.push(Diagnostic::new(Error, "mod.ron", format!("weapon {:?} is listed twice", w.id)));
        }
    }

    let mut referenced: BTreeSet<&str> = BTreeSet::new();
    let mut used_assets: BTreeSet<&str> = BTreeSet::new();
    for w in &def.weapons {
        let file = format!("weapons/{}.ron", w.id);
        if let Some(p) = w.projectile() {
            referenced.insert(p);
            if !def.projectiles.contains_key(p) {
                out.push(Diagnostic::new(Error, &file, format!("unknown projectile {p:?}")));
            }
        }
        if let Some(s) = w.sound() {
            used_assets.insert(s);
        }
    }
    for (name, p) in &def.projectiles {
        let file = format!("projectiles/{name}.ron");
        for c in p.children() {
            referenced.insert(c);
            if !def.projectiles.contains_key(c) {
                out.push(Diagnostic::new(Error, &file, format!("unknown projectile {c:?}")));
            }
        }
        used_assets.extend(p.assets());
    }
    for asset in &used_assets {
        let present = VPath::new(asset).map(|p| pack.assets.contains_key(&p)).unwrap_or(false);
        if !present {
            out.push(Diagnostic::new(Error, "mod.ron", format!("missing asset {asset:?}")));
        }
    }
    for name in def.projectiles.keys() {
        if !referenced.contains(name.as_str()) {
            out.push(Diagnostic::new(Warning, format!("projectiles/{name}.ron"), "projectile is never used"));
        }
    }
    for path in pack.assets.keys() {
        if !used_assets.contains(path.as_str()) {
            out.push(Diagnostic::new(Warning, path.as_str(), "asset is never used"));
        }
    }
    if let Some(cycle) = find_cycle(def) {
        out.push(Diagnostic::new(
            Error,
            format!("projectiles/{}.ron", cycle[0]),
            format!("projectiles spawn each other in a cycle: {}", cycle.join(" -> ")),
        ));
    }
    out
}

/// Returns the first cycle in the projectile spawn graph, if any, as a list of
/// names starting and ending with the same projectile.
fn find_cycle(def: &ModDef) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Visiting,
        Done,
    }
    fn visit<'a>(
        def: &'a ModDef,
        name: &'a str,
        state: &mut BTreeMap<&'a str, State>,
        stack: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        match state.get(name) {
            Some(State::Done) => return None,
            Some(State::Visiting) => {
                let start = stack.iter().position(|n| *n == name).expect("visiting nodes are on the stack");
                let mut cycle: Vec<String> = stack[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(name.to_string());
                return Some(cycle);
            }
            None => {}
        }
        let p = def.projectiles.get(name)?;
        state.insert(name, State::Visiting);
        stack.push(name);
        for c in p.children() {
            if let Some(cycle) = visit(def, c, state, stack) {
                return Some(cycle);
            }
        }
        stack.pop();
        state.insert(name, State::Done);
        None
    }
    let mut state = BTreeMap::new();
    for name in def.projectiles.keys() {
        if let Some(c) = visit(def, name, &mut state, &mut Vec::new()) {
            return Some(c);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level_pack::tests::sample_level;
    use crate::mod_pack::tests::sample_mod;
    use crate::{write_level, write_mod};
    use rl_data::mod_def::SpawnDef;
    use rl_vfs::MemFs;

    fn written(pack: &ModPack) -> MemFs {
        let mut fs = MemFs::new();
        write_mod(&mut fs, pack).unwrap();
        fs
    }

    #[test]
    fn valid_packs_have_no_diagnostics() {
        assert_eq!(validate(&written(&sample_mod())), vec![]);
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        assert_eq!(validate(&fs), vec![]);
    }

    #[test]
    fn reports_unknown_projectile() {
        let mut pack = sample_mod();
        pack.def.projectiles.get_mut("rocket").unwrap().event_spawn =
            Some(SpawnDef::olx_default("nope".into()));
        let d = validate(&written(&pack));
        assert!(
            d.iter().any(|d| d.to_string() == "error: projectiles/rocket.ron: unknown projectile \"nope\""),
            "{d:?}"
        );
    }

    #[test]
    fn reports_cycles() {
        let mut pack = sample_mod();
        pack.def.projectiles.get_mut("bits").unwrap().event_spawn =
            Some(SpawnDef::olx_default("rocket".into()));
        let d = validate(&written(&pack));
        let msg = d.iter().find(|d| d.message.contains("cycle")).expect("cycle reported");
        assert_eq!(msg.severity, Severity::Error);
        assert!(msg.message.contains("bits -> rocket -> bits"), "{}", msg.message);
    }

    #[test]
    fn reports_missing_asset_and_unused_items() {
        let mut pack = sample_mod();
        pack.assets.remove(&VPath::new("sfx/boom.ogg").unwrap());
        pack.assets.insert(VPath::new("gfx/extra.png").unwrap(), b"x".to_vec());
        let d: Vec<String> = validate(&written(&pack)).iter().map(|d| d.to_string()).collect();
        assert!(d.contains(&"error: mod.ron: missing asset \"sfx/boom.ogg\"".to_string()), "{d:?}");
        assert!(d.contains(&"warning: gfx/extra.png: asset is never used".to_string()), "{d:?}");
    }

    #[test]
    fn reports_parse_errors_with_position() {
        let fs = written(&sample_mod()).with("mod.ron", "(name: \"x\",\n oops)");
        let d = validate(&fs);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].file.as_deref(), d[0].line), (Some("mod.ron"), Some(2)));
    }

    #[test]
    fn reports_missing_manifest() {
        let d = validate(&MemFs::new());
        assert_eq!(d[0].severity, Severity::Error);
        assert!(d[0].message.contains("pack.ron"), "{}", d[0].message);
    }
}
```

- [ ] **Step 2: Replace `crates/rl-pack/src/lib.rs`**

```rust
//! The native pack format (spec §9.1).
//!
//! A pack is a directory tree reachable through an [`rl_vfs::ContentSource`]
//! whose root holds `pack.ron` ([`rl_data::manifest::PackManifest`]).
//! Readers and writers are deterministic: writing the same value twice yields
//! byte-identical files.

mod error;
mod hash;
pub mod image_io;
mod level_pack;
mod mod_pack;
mod ron_io;
mod skin_pack;
mod theme_pack;
mod validate;

pub use error::PackError;
pub use hash::content_hash;
pub use level_pack::{LevelPack, read_level, write_level};
pub use mod_pack::{ModPack, read_mod, write_mod};
pub use ron_io::{read_manifest, write_manifest};
pub use skin_pack::{SkinPack, read_skin, write_skin};
pub use theme_pack::{ThemePack, read_theme, write_theme};
pub use validate::{Diagnostic, Severity, validate};

/// Path of the manifest inside every pack.
pub const MANIFEST: &str = "pack.ron";
```

- [ ] **Step 3: Run**

```bash
cargo nextest run -p rl-pack
```

Expected: output contains `23 passed`.

- [ ] **Step 4: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "rl-pack: validate packs with positioned diagnostics"
```


---

### Task 7: `rl-import-lgs`: gamescript v7 parser

A byte-exact port of `CGameScript::Load` and `LoadProjectile` for v7 files, producing a tree that mirrors the file. Inline child projectiles are owned by their parent.

**Files:**

- Create: `crates/rl-import-lgs/{Cargo.toml,CLAUDE.md}`, `crates/rl-import-lgs/src/{lib,reader,parse}.rs`
- Modify: `xtask/layers.ron` (register `rl-import-lgs`, layer 2, allow `rl-import-lgs` → `rl-pack`)

**Interfaces:**

- Produces: `parse_lgs(&[u8]) -> Result<RawMod, LgsError>`.
- Produces: `RawMod { name, weapons: Vec<RawWeapon>, rope, worm, trailing_bytes }`, `RawWeapon { def: WeaponDef, projectile: Option<ProjNode> }`, `ProjNode { def: ProjectileDef, event_child: Option<Box<ProjNode>>, trail_child: Option<Box<ProjNode>> }`. Spawn targets are empty strings, and image and sound names are raw OLX names.
- Produces: `LgsError` with the variants `Truncated{offset,what}`, `BadId`, `UnsupportedVersion(u32)`, `Implausible{what,value}`, `TooDeep`, `UnknownProjectileType(i32)`, `UnknownWeaponType(i32,String)`, `MissingFile`, `MissingImage`, `AssetConflict`, `Vfs`.
- Test helper for Task 8: `parse::tests::Lgs`, a v7 byte builder: `header(name, n)`, `projectile_weapon(name, sound)`, `pixel_projectile(damage, child)`, `footer()`, `i`, `f`, `s`.

- [ ] **Step 1: Create `crates/rl-import-lgs/Cargo.toml`**

```toml
[package]
name = "rl-import-lgs"
description = "Imports OpenLieroX script.lgs (gamescript v7) mods into native mod packs."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
rl-data.workspace = true
rl-vfs.workspace = true
rl-pack.workspace = true
thiserror.workspace = true

[dev-dependencies]
proptest.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/rl-import-lgs/CLAUDE.md`**

```markdown
# rl-import-lgs

Imports OLX `script.lgs` gamescript **version 7** mods into `ModPack`s.

- **Layer 2.** Ports `CGameScript::Load` / `LoadProjectile`
  (OLX `src/common/CGameScript.cpp:582-1152`) at commit `d1ad953f`.
- `parse.rs` follows the file byte by byte; `convert.rs` names projectiles,
  shares identical ones, and copies assets.
- **Asset lookup is OLX's:** `<mod>/gfx/` then `data/gfx/`; `<mod>/sfx/` then
  `data/sounds/`, case-insensitive, and sounds also try `.ogg`. A missing sound
  is dropped with a warning (OLX does the same); a missing image is an error.
- **Never panic on input.** Property tests feed corrupted files.
- Version 8 files are rejected (`UnsupportedVersion`); no mod in scope uses v8.

Test: `cargo nextest run -p rl-import-lgs`. Real mods are tested when
`RL_OLX_GAMEDIR` is set; `cargo xtask check` sets it.
```

- [ ] **Step 3: Create `crates/rl-import-lgs/src/lib.rs`**

```rust
//! OpenLieroX `script.lgs` importer.
//!
//! Reads gamescript version 7 (LX 0.56), the version of every mod in scope,
//! following `CGameScript::Load` and `CGameScript::LoadProjectile`
//! (`src/common/CGameScript.cpp:582-1152`), and produces a self-contained
//! [`rl_pack::ModPack`] including the image and sound files the mod uses.

mod parse;
mod reader;

pub use parse::{ProjNode, RawMod, RawWeapon, parse_lgs};

/// Everything that can go wrong while importing a mod.
#[derive(Debug, thiserror::Error)]
pub enum LgsError {
    #[error("file ends early at byte {offset} while reading {what}")]
    Truncated { offset: usize, what: &'static str },
    #[error("not a Liero Game Script (bad header id)")]
    BadId,
    #[error("unsupported gamescript version {0} (only version 7 is supported)")]
    UnsupportedVersion(u32),
    #[error("implausible {what}: {value}")]
    Implausible { what: &'static str, value: i64 },
    #[error("projectiles nested deeper than 128 levels")]
    TooDeep,
    #[error("unknown projectile type {0}")]
    UnknownProjectileType(i32),
    #[error("unknown weapon type {0} in weapon {1:?}")]
    UnknownWeaponType(i32, String),
    #[error("{0} not found")]
    MissingFile(String),
    #[error("image {0:?} not found in the mod's gfx/ or in data/gfx/")]
    MissingImage(String),
    #[error("two different files map to {0}")]
    AssetConflict(String),
    #[error(transparent)]
    Vfs(#[from] rl_vfs::VfsError),
}
```

- [ ] **Step 4: Create `crates/rl-import-lgs/src/reader.rs`**

```rust
use crate::LgsError;

/// Little-endian cursor over a byte slice. Every read reports truncation instead
/// of returning garbage, unlike OLX's `fread`-based loader.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub(crate) fn bytes(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], LgsError> {
        if self.remaining() < n {
            return Err(LgsError::Truncated { offset: self.pos, what });
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    pub(crate) fn i32(&mut self, what: &'static str) -> Result<i32, LgsError> {
        Ok(i32::from_le_bytes(self.bytes(4, what)?.try_into().expect("4 bytes")))
    }

    pub(crate) fn u32(&mut self, what: &'static str) -> Result<u32, LgsError> {
        Ok(u32::from_le_bytes(self.bytes(4, what)?.try_into().expect("4 bytes")))
    }

    pub(crate) fn f32(&mut self, what: &'static str) -> Result<f32, LgsError> {
        Ok(f32::from_le_bytes(self.bytes(4, what)?.try_into().expect("4 bytes")))
    }

    /// A 32-bit int read as a boolean, as OLX does with `fread_endian<int>` into a `bool`.
    pub(crate) fn flag(&mut self, what: &'static str) -> Result<bool, LgsError> {
        Ok(self.i32(what)? != 0)
    }

    /// OLX `readString`: one length byte, then that many bytes. Decoded as Latin-1
    /// so every byte maps to a character.
    pub(crate) fn pstring(&mut self, what: &'static str) -> Result<String, LgsError> {
        let len = self.bytes(1, what)?[0] as usize;
        Ok(latin1(self.bytes(len, what)?))
    }

    /// A fixed-size C string field: stops at the first NUL, because OLX files
    /// contain leftover memory after it.
    pub(crate) fn fixed_cstr(&mut self, len: usize, what: &'static str) -> Result<String, LgsError> {
        let raw = self.bytes(len, what)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(latin1(&raw[..end]))
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_and_strings() {
        let data = [1, 0, 0, 0, 0, 0, 128, 63, 3, b'a', b'b', 0xE9, b'x', 0, b'g'];
        let mut r = Reader::new(&data);
        assert_eq!(r.i32("i").unwrap(), 1);
        assert_eq!(r.f32("f").unwrap(), 1.0);
        assert_eq!(r.pstring("s").unwrap(), "abé");
        assert_eq!(r.fixed_cstr(3, "c").unwrap(), "x");
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn truncation_reports_offset() {
        let mut r = Reader::new(&[5, b'a']);
        match r.pstring("name") {
            Err(LgsError::Truncated { offset: 1, what: "name" }) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}
```

- [ ] **Step 5: Create `crates/rl-import-lgs/src/parse.rs`**

```rust
//! Byte-level parsing of `script.lgs` v7 into a tree that mirrors the file:
//! v7 stores every spawned projectile inline, so children are owned here and
//! names are assigned later by `convert`.

use rl_data::image::Rgba;
use rl_data::mod_def::*;

use crate::LgsError;
use crate::reader::Reader;

const ID: &[u8] = b"Liero Game Script";
const VERSION_LX56: u32 = 7;
const HEADER_LEN: usize = 88;
const MAX_WEAPONS: i32 = 4096;
const MAX_LIST: u32 = 4096;
const MAX_DEPTH: usize = 128;

// OLX enum values used while parsing (include/WeaponDesc.h, ProjectileDesc.h, ProjAction.h).
const WPN_PROJECTILE: i32 = 0;
const WPN_SPECIAL: i32 = 1;
const WPN_BEAM: i32 = 2;
const PRJ_PIXEL: i32 = 0;
const PRJ_IMAGE: i32 = 1;
#[cfg(test)]
const PJ_EXPLODE: i32 = 1;
#[cfg(test)]
const PJ_INJURE: i32 = 2;
const TRL_PROJECTILE: i32 = 3;

/// A parsed mod before projectile naming and asset resolution. Image and sound
/// names are exactly as written in the file; spawn targets are left empty.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMod {
    pub name: String,
    pub weapons: Vec<RawWeapon>,
    pub rope: RopeParams,
    pub worm: WormParams,
    /// Bytes after the worm block; OLX ignores them.
    pub trailing_bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawWeapon {
    pub def: WeaponDef,
    pub projectile: Option<ProjNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjNode {
    pub def: ProjectileDef,
    pub event_child: Option<Box<ProjNode>>,
    pub trail_child: Option<Box<ProjNode>>,
}

/// Parses a complete `script.lgs` file.
pub fn parse_lgs(data: &[u8]) -> Result<RawMod, LgsError> {
    let mut r = Reader::new(data);
    let header = r.bytes(HEADER_LEN, "header")?;
    let id_end = header[..18].iter().position(|&b| b == 0).unwrap_or(18);
    if &header[..id_end] != ID {
        return Err(LgsError::BadId);
    }
    // ID[18], 2 bytes of struct padding, then Version at offset 20 (gs_header_t).
    let version = u32::from_le_bytes(header[20..24].try_into().expect("4 bytes"));
    if version != VERSION_LX56 {
        return Err(LgsError::UnsupportedVersion(version));
    }
    let name = Reader::new(&header[24..88]).fixed_cstr(64, "mod name")?;

    let count = r.i32("weapon count")?;
    if !(0..=MAX_WEAPONS).contains(&count) {
        return Err(LgsError::Implausible { what: "weapon count", value: count as i64 });
    }
    let mut weapons = Vec::with_capacity(count as usize);
    for _ in 0..count {
        weapons.push(parse_weapon(&mut r)?);
    }

    let rope = RopeParams {
        max_length: r.i32("rope length")?,
        rest_length: r.i32("rope rest length")?,
        strength: r.f32("rope strength")?,
    };
    let worm = WormParams {
        angle_speed: r.f32("worm")?,
        ground_speed: r.f32("worm")?,
        air_speed: r.f32("worm")?,
        gravity: r.f32("worm")?,
        jump_force: r.f32("worm")?,
        air_friction: r.f32("worm")?,
        ground_friction: r.f32("worm")?,
    };
    Ok(RawMod { name, weapons, rope, worm, trailing_bytes: r.remaining() })
}

fn parse_weapon(r: &mut Reader) -> Result<RawWeapon, LgsError> {
    let name = r.pstring("weapon name")?;
    let ty = r.i32("weapon type")?;
    let mut projectile = None;
    let (kind, recharge, drain, rof, laser_sight) = match ty {
        WPN_SPECIAL => {
            let special = SpecialKind::from_lx(r.i32("special")?);
            let thrust = r.u32("thrust")?;
            let (recharge, drain, rof) = (r.f32("recharge")?, r.f32("drain")?, r.f32("rof")?);
            let laser = r.flag("laser sight")?;
            (WeaponKind::Special { special, thrust }, recharge, drain, rof, laser)
        }
        WPN_BEAM => {
            let recoil = r.i32("recoil")?;
            let (recharge, drain, rof) = (r.f32("recharge")?, r.f32("drain")?, r.f32("rof")?);
            let laser = r.flag("laser sight")?;
            // Wpn_Beam::read for v7 (CGameScript.cpp:2058-2083): colour as three ints,
            // truncated to bytes like OLX's Color(Uint8, Uint8, Uint8).
            let colour = Rgba::rgb(
                r.i32("beam colour")? as u8,
                r.i32("beam colour")? as u8,
                r.i32("beam colour")? as u8,
            );
            let beam = BeamDef {
                colour,
                damage: r.i32("beam damage")?,
                player_damage: r.i32("beam player damage")?,
                length: r.i32("beam length")?,
                init_width: 1,
                width_increase: 0.0,
                distribute_damage_over_width: false,
            };
            (WeaponKind::Beam { recoil, beam }, recharge, drain, rof, laser)
        }
        WPN_PROJECTILE => {
            let class = WeaponClass::from_lx(r.i32("weapon class")?);
            let recoil = r.i32("recoil")?;
            let (recharge, drain, rof) = (r.f32("recharge")?, r.f32("drain")?, r.f32("rof")?);
            // weapon_t's constructor sets UseRandomRot, Useangle and AddParentVel for shots.
            let mut spawn = SpawnDef::olx_default(String::new());
            spawn.speed = r.i32("speed")?;
            spawn.speed_var = r.f32("speed var")?;
            spawn.spread = r.f32("spread")?;
            spawn.amount = r.i32("amount")?;
            spawn.use_random_rot = true;
            spawn.use_angle = true;
            spawn.add_parent_vel = true;
            let laser = r.flag("laser sight")?;
            let sound = if r.flag("use sound")? { Some(r.pstring("sound name")?) } else { None };
            projectile = Some(parse_projectile(r, 0)?);
            (WeaponKind::Projectile { class, recoil, sound, spawn }, recharge, drain, rof, laser)
        }
        other => return Err(LgsError::UnknownWeaponType(other, name)),
    };
    let def = WeaponDef {
        id: String::new(),
        name,
        // CGameScript.cpp:776-777
        recharge: recharge / 10.0,
        drain,
        rof: rof / 1000.0,
        laser_sight,
        kind,
    };
    Ok(RawWeapon { def, projectile })
}

/// The Hit, Timer, PlayerHit, Exp and Touch actions of a v7 projectile.
/// Exp and Touch are obsolete in OLX but their "projectiles" flags still decide
/// whether the file contains the event spawn block.
fn parse_projectile(r: &mut Reader, depth: usize) -> Result<ProjNode, LgsError> {
    if depth > MAX_DEPTH {
        return Err(LgsError::TooDeep);
    }
    let ty = r.i32("projectile type")?;
    let time = r.f32("timer time")?;
    let time_var = r.f32("timer time var")?;
    let trail_type = r.i32("trail type")?;
    let gravity = if r.flag("use custom gravity")? { Some(r.i32("gravity")?) } else { None };
    // Read with sizeof(int) into a float field: the bytes are an f32.
    let dampening = r.f32("dampening")?;
    let (width, height) = if ty == PRJ_PIXEL { (2, 2) } else { (4, 4) };

    let gfx = match ty {
        PRJ_PIXEL => {
            let n = r.u32("colour count")?;
            if n > MAX_LIST {
                return Err(LgsError::Implausible { what: "colour count", value: n as i64 });
            }
            let mut colours = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (cr, cg, cb) = (r.i32("colour")?, r.i32("colour")?, r.i32("colour")?);
                colours.push(Rgba::rgb(cr as u8, cg as u8, cb as u8));
            }
            ProjectileGfx::Pixel { colours }
        }
        PRJ_IMAGE => {
            let file = r.pstring("image name")?;
            let rotating = r.flag("rotating")?;
            let rot_increment = r.i32("rot increment")?;
            let rot_speed = r.i32("rot speed")?;
            let use_angle = r.flag("use angle")?;
            let use_spec_angle = r.flag("use spec angle")?;
            let angle_images = if use_angle || use_spec_angle { r.i32("angle images")? } else { 0 };
            let animation = if r.flag("animating")? {
                Some(Animation { rate: r.f32("anim rate")?, kind: AnimKind::from_lx(r.i32("anim type")?) })
            } else {
                None
            };
            ProjectileGfx::Image {
                file,
                rotating,
                rot_increment,
                rot_speed,
                use_angle,
                use_spec_angle,
                angle_images,
                animation,
            }
        }
        // Circle, polygon and rect (2-4) only exist in v8 files.
        other => return Err(LgsError::UnknownProjectileType(other)),
    };

    // Hit (terrain).
    let mut hit = ActionDef::olx_default(ActionKind::from_lx(r.i32("hit type")?));
    match hit.kind {
        ActionKind::Explode => {
            hit.damage = r.i32("hit damage")?;
            hit.spawns_projectiles = r.flag("hit projectiles")?;
            let use_sound = r.flag("hit use sound")?;
            hit.shake = r.i32("hit shake")?;
            if use_sound {
                hit.sound = Some(r.pstring("hit sound")?);
            }
        }
        ActionKind::Bounce => {
            hit.bounce_coeff = r.f32("hit bounce coeff")?;
            hit.bounce_explode = r.i32("hit bounce explode")?;
        }
        ActionKind::Carve => hit.damage = r.i32("hit damage")?,
        _ => {}
    }

    // Timer: only present when Time > 0.
    let timer_action = if time > 0.0 {
        let mut a = ActionDef::olx_default(ActionKind::from_lx(r.i32("timer type")?));
        if a.kind == ActionKind::Explode {
            a.damage = r.i32("timer damage")?;
            a.spawns_projectiles = r.flag("timer projectiles")?;
            a.shake = r.i32("timer shake")?;
        }
        Some(a)
    } else {
        None
    };

    // Player hit.
    let mut player_hit = ActionDef::olx_default(ActionKind::from_lx(r.i32("player hit type")?));
    match player_hit.kind {
        ActionKind::Injure | ActionKind::Explode => {
            player_hit.damage = r.i32("player hit damage")?;
            player_hit.spawns_projectiles = r.flag("player hit projectiles")?;
        }
        ActionKind::Bounce => player_hit.bounce_coeff = r.f32("player hit bounce coeff")?,
        _ => {}
    }

    // Obsolete Exp and Touch actions: read and dropped, except their projectile flags.
    let mut obsolete_spawns = false;
    for what in ["explode action", "touch action"] {
        let _ty = r.i32(what)?;
        let _damage = r.i32(what)?;
        obsolete_spawns |= r.flag(what)?;
        if r.flag(what)? {
            r.pstring(what)?;
        }
    }

    let any_spawns = obsolete_spawns
        || hit.spawns_projectiles
        || player_hit.spawns_projectiles
        || timer_action.as_ref().is_some_and(|a| a.spawns_projectiles);
    let mut event_child = None;
    let event_spawn = if any_spawns {
        let mut s = SpawnDef::olx_default(String::new());
        s.use_angle = r.flag("spawn use angle")?;
        s.angle = r.i32("spawn angle")?;
        s.amount = r.i32("spawn amount")?;
        s.spread = r.f32("spawn spread")?;
        s.speed = r.i32("spawn speed")?;
        s.speed_var = r.f32("spawn speed var")?;
        event_child = Some(Box::new(parse_projectile(r, depth + 1)?));
        Some(s)
    } else {
        None
    };

    let kind = TrailKind::from_lx(trail_type);
    let mut trail = TrailDef { kind, delay: 0.1, spawn: None };
    let mut trail_child = None;
    if trail_type == TRL_PROJECTILE {
        // Proj_Trail's constructor sets UseSpecial11VecForSpeedVar.
        let mut s = SpawnDef::olx_default(String::new());
        s.use_special11_vec_for_speed_var = true;
        s.use_parent_velocity_for_spread = r.flag("trail use parent velocity")?;
        trail.delay = r.f32("trail delay")? / 1000.0;
        s.amount = r.i32("trail amount")?;
        s.speed = r.i32("trail speed")?;
        s.speed_var = r.f32("trail speed var")?;
        s.spread = r.f32("trail spread")?;
        trail_child = Some(Box::new(parse_projectile(r, depth + 1)?));
        trail.spawn = Some(s);
    }

    let def = ProjectileDef {
        gfx,
        gravity,
        dampening,
        width,
        height,
        timer: TimerDef { time, time_var, action: timer_action },
        trail,
        hit,
        player_hit,
        event_spawn,
    };
    Ok(ProjNode { def, event_child, trail_child })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds v7 files byte by byte in the order `CGameScript::Load` reads them.
    #[derive(Default)]
    pub(crate) struct Lgs(pub Vec<u8>);

    impl Lgs {
        pub(crate) fn header(name: &str, weapons: i32) -> Self {
            let mut b = Lgs::default();
            let mut id = [0u8; 20];
            id[..ID.len()].copy_from_slice(ID);
            b.0.extend_from_slice(&id);
            b.0.extend_from_slice(&VERSION_LX56.to_le_bytes());
            let mut n = [0xAAu8; 64]; // garbage after the NUL, like real files
            n[..name.len()].copy_from_slice(name.as_bytes());
            n[name.len()] = 0;
            b.0.extend_from_slice(&n);
            b.i(weapons)
        }
        pub(crate) fn i(mut self, v: i32) -> Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub(crate) fn f(mut self, v: f32) -> Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub(crate) fn s(mut self, v: &str) -> Self {
            self.0.push(v.len() as u8);
            self.0.extend_from_slice(v.as_bytes());
            self
        }
        /// A pixel projectile with one colour, no timer, no trail, `hit` explode with
        /// optional spawning of `child`, player hit injure.
        pub(crate) fn pixel_projectile(self, damage: i32, child: Option<fn(Lgs) -> Lgs>) -> Self {
            let b = self.i(PRJ_PIXEL).f(0.0).f(0.0).i(0).i(0).f(1.0); // type, timer, trail, no gravity, damp
            let b = b.i(1).i(255).i(0).i(300); // 1 colour; blue 300 truncates to 44
            let b = b.i(PJ_EXPLODE).i(damage).i(child.is_some() as i32).i(0).i(0); // hit: no sound, shake 0
            let b = b.i(PJ_INJURE).i(7).i(0); // player hit
            let b = b.i(0).i(0).i(0).i(0).i(0).i(0).i(0).i(0); // exp + touch
            match child {
                Some(c) => c(b.i(1).i(90).i(3).f(0.5).i(40).f(0.25)),
                None => b,
            }
        }
        pub(crate) fn footer(self) -> Self {
            self.i(300).i(27).f(4.5).f(150.0).f(8.0).f(4.0).f(100.0).f(-65.0).f(0.0).f(0.4)
        }
        pub(crate) fn projectile_weapon(self, name: &str, sound: Option<&str>) -> Self {
            let b = self.s(name).i(WPN_PROJECTILE).i(4).i(5).f(5.0).f(30.0).f(250.0);
            let b = b.i(100).f(0.1).f(2.0).i(1).i(0); // speed, speed var, spread, amount, laser
            match sound {
                Some(s) => b.i(1).s(s),
                None => b.i(0),
            }
        }
    }

    fn one_weapon_mod() -> Vec<u8> {
        Lgs::header("Tiny Mod", 1)
            .projectile_weapon("Rocket", Some("boom.wav"))
            .pixel_projectile(12, Some(|b| b.pixel_projectile(3, None)))
            .footer()
            .0
    }

    #[test]
    fn parses_header_and_footer() {
        let m = parse_lgs(&one_weapon_mod()).unwrap();
        assert_eq!(m.name, "Tiny Mod");
        assert_eq!(m.rope, RopeParams { max_length: 300, rest_length: 27, strength: 4.5 });
        assert_eq!(m.worm.jump_force, -65.0);
        assert_eq!(m.trailing_bytes, 0);
    }

    #[test]
    fn applies_olx_post_processing_and_shot_flags() {
        let m = parse_lgs(&one_weapon_mod()).unwrap();
        let w = &m.weapons[0].def;
        assert_eq!(w.rof, 250.0 / 1000.0);
        assert_eq!(w.recharge, 5.0 / 10.0);
        let WeaponKind::Projectile { class, sound, spawn, .. } = &w.kind else { panic!("projectile weapon") };
        assert_eq!(*class, WeaponClass::Missile);
        assert_eq!(sound.as_deref(), Some("boom.wav"));
        assert!(spawn.use_random_rot && spawn.use_angle && spawn.add_parent_vel);
        assert_eq!((spawn.speed, spawn.amount), (100, 1));
    }

    #[test]
    fn parses_nested_event_spawn() {
        let m = parse_lgs(&one_weapon_mod()).unwrap();
        let p = m.weapons[0].projectile.as_ref().unwrap();
        assert_eq!(p.def.hit.kind, ActionKind::Explode);
        assert_eq!(p.def.hit.damage, 12);
        assert!(p.def.hit.spawns_projectiles);
        let s = p.def.event_spawn.as_ref().unwrap();
        assert_eq!((s.use_angle, s.angle, s.amount, s.speed), (true, 90, 3, 40));
        let child = p.event_child.as_ref().unwrap();
        assert_eq!(child.def.hit.damage, 3);
        assert!(child.event_child.is_none());
        assert_eq!(p.def.gfx, ProjectileGfx::Pixel { colours: vec![Rgba::rgb(255, 0, 44)] });
        assert_eq!((p.def.width, p.def.height), (2, 2));
        assert_eq!(p.def.player_hit.damage, 7);
    }

    #[test]
    fn rejects_bad_header() {
        let mut data = one_weapon_mod();
        data[0] = b'X';
        assert!(matches!(parse_lgs(&data), Err(LgsError::BadId)));
        let mut data = one_weapon_mod();
        data[20] = 8;
        assert!(matches!(parse_lgs(&data), Err(LgsError::UnsupportedVersion(8))));
    }

    #[test]
    fn rejects_implausible_weapon_count() {
        let data = Lgs::header("x", -1).0;
        assert!(matches!(parse_lgs(&data), Err(LgsError::Implausible { what: "weapon count", .. })));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let data = one_weapon_mod();
        for len in 0..data.len() {
            assert!(parse_lgs(&data[..len]).is_err(), "prefix of {len} bytes parsed");
        }
    }

    #[test]
    fn rejects_deep_nesting() {
        fn nest(b: Lgs, depth: usize) -> Lgs {
            if depth == 0 {
                return b.pixel_projectile(1, None);
            }
            let b = b.i(PRJ_PIXEL).f(0.0).f(0.0).i(0).i(0).f(1.0).i(0);
            let b = b.i(PJ_EXPLODE).i(1).i(1).i(0).i(0).i(8);
            let b = b.i(0).i(0).i(0).i(0).i(0).i(0).i(0).i(0);
            nest(b.i(0).i(0).i(1).f(0.0).i(0).f(0.0), depth - 1)
        }
        let data = nest(Lgs::header("deep", 1).projectile_weapon("w", None), 200).footer().0;
        assert!(matches!(parse_lgs(&data), Err(LgsError::TooDeep)));
    }

    proptest::proptest! {
        #[test]
        fn never_panics_on_arbitrary_bytes(tail in proptest::collection::vec(proptest::num::u8::ANY, 0..512)) {
            let mut data = Lgs::header("fuzz", 1).0;
            data.extend_from_slice(&tail);
            let _ = parse_lgs(&data);
        }

        #[test]
        fn never_panics_on_corrupted_real_layout(pos in 0usize..200, byte in proptest::num::u8::ANY) {
            let mut data = one_weapon_mod();
            let pos = pos % data.len();
            data[pos] = byte;
            let _ = parse_lgs(&data);
        }
    }
}
```

- [ ] **Step 6: Replace `xtask/layers.ron`**

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
        "xtask": 6,
    },
    same_layer: [
        ("rl-import-lgs", "rl-pack"),
    ],
    no_std: ["rl-data"],
    wasm: ["rl-data", "rl-vfs", "rl-pack", "rl-import-lgs"],
)
```

- [ ] **Step 7: Run**

```bash
cargo nextest run -p rl-import-lgs
```

Expected: output contains `11 passed`. Includes `every_truncation_is_an_error`, `rejects_deep_nesting`, and two never-panic property tests.

- [ ] **Step 8: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "rl-import-lgs: parse gamescript v7"
```


---

### Task 8: `rl-import-lgs`: conversion to mod packs, verified on Classic and MW 1.0

Names and shares projectiles, resolves assets exactly as OLX does, and produces a self-contained `ModPack`. A real-content test imports both mods in scope.

**Files:**

- Create: `crates/rl-import-lgs/tests/olx_mods.rs`, `crates/rl-import-lgs/src/convert.rs`
- Modify: `crates/rl-import-lgs/src/lib.rs` (add `mod convert;` and its re-export)

**Interfaces:**

- Consumes: `parse_lgs`, `ProjNode` (Task 7); `ModPack`, `write_mod`, `read_mod`, `validate` (Tasks 5–6); `find_ci`, `find_ci_any` (Task 4).
- Produces: `import_mod(olx: &dyn ContentSource, mod_dir: &VPath, origin: Origin) -> Result<ImportedMod, LgsError>`; `ImportedMod { pack: ModPack, warnings: Vec<String> }`.
- Naming: the pack id is `olx-<slug(dir)>`; weapon ids are `slug(name)`, with `-2`, `-3` on collision; projectiles are named `<weapon>`, `<parent>.spawn` and `<parent>.trail`, and identical definitions share one name.
- Assets: images map to `gfx/<lowercase file>` and sounds to `sfx/<lowercase file>`.

- [ ] **Step 1: Run**

```bash
cargo xtask fetch-olx-content --no-import
```

Expected: output contains `RL_OLX_GAMEDIR=`. Sparse-clone the pinned OLX commit (about 16 MB). The import step needs `rl-cli` (Task 11), so skip it.

- [ ] **Step 2: Create `crates/rl-import-lgs/tests/olx_mods.rs`**

```rust
//! Imports the real OLX mods in scope when `RL_OLX_GAMEDIR` points at an OLX
//! `share/gamedir`; `cargo xtask check` sets it after `fetch-olx-content`.

use rl_data::manifest::Origin;
use rl_vfs::{DirFs, MemFs, VPath};

fn gamedir() -> Option<DirFs> {
    match std::env::var("RL_OLX_GAMEDIR") {
        Ok(dir) => Some(DirFs::new(dir)),
        Err(_) => {
            eprintln!("RL_OLX_GAMEDIR not set; skipping real-mod import test");
            None
        }
    }
}

fn check(dir: &str, weapons: usize, name: &str) {
    let Some(fs) = gamedir() else { return };
    let origin = Origin { source: "OpenLieroX".into(), path: dir.into(), revision: "test".into() };
    let m = rl_import_lgs::import_mod(&fs, &VPath::new(dir).unwrap(), origin).unwrap();
    assert_eq!(m.pack.def.name, name);
    assert_eq!(m.pack.def.weapons.len(), weapons);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    let mut out = MemFs::new();
    rl_pack::write_mod(&mut out, &m.pack).unwrap();
    let errors: Vec<_> =
        rl_pack::validate(&out).into_iter().filter(|d| d.severity == rl_pack::Severity::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rl_pack::read_mod(&out).unwrap(), m.pack);
}

#[test]
fn liero_1_0() {
    check("Classic", 38, "Liero v1.0");
}

#[test]
fn modern_warfare_1_0() {
    check("MW 1.0", 50, "Modern Warfare1.0");
}
```

- [ ] **Step 3: Run**

```bash
RL_OLX_GAMEDIR=$PWD/target/olx/d1ad953f4f20610f2648cce26c9739e7c088409d/share/gamedir cargo nextest run -p rl-import-lgs --test olx_mods
```

 Expected: FAIL to compile with "cannot find function `import_mod`", since the converter does not exist yet.

- [ ] **Step 4: Create `crates/rl-import-lgs/src/convert.rs`**

```rust
//! Turns a parsed [`RawMod`] into a native [`ModPack`]:
//! - weapons get stable ids (slugs of their names);
//! - inline projectiles get names, and identical ones are shared;
//! - images and sounds are found the way OLX finds them and copied into the pack.

use std::collections::BTreeMap;

use rl_data::manifest::{Origin, PACK_FORMAT_VERSION, PackKind, PackManifest};
use rl_data::mod_def::{ModDef, ProjectileDef, ProjectileGfx, WeaponDef, WeaponKind};
use rl_data::slug::slug;
use rl_pack::ModPack;
use rl_vfs::{ContentSource, VPath, find_ci, find_ci_any};

use crate::LgsError;
use crate::parse::{ProjNode, RawMod, parse_lgs};

/// The result of importing one mod.
#[derive(Debug)]
pub struct ImportedMod {
    pub pack: ModPack,
    /// Non-fatal findings, such as sounds OLX would also fail to load.
    pub warnings: Vec<String>,
}

/// Imports the mod in directory `mod_dir` of an OLX game directory (`share/gamedir`).
/// Images are looked up in `<mod_dir>/gfx/` then `data/gfx/`, sounds in
/// `<mod_dir>/sfx/` then `data/sounds/`, ignoring case and retrying sounds with
/// `.ogg` (`CGameScript::LoadGSImage`, `LoadGSSample`, `src/sound/Sounds.cpp:113`).
pub fn import_mod(olx: &dyn ContentSource, mod_dir: &VPath, origin: Origin) -> Result<ImportedMod, LgsError> {
    let script = find_ci(olx, &mod_dir.join("script.lgs")?)
        .ok_or_else(|| LgsError::MissingFile(format!("{mod_dir}/script.lgs")))?;
    let raw = parse_lgs(&olx.read(&script)?)?;
    let mut cx =
        Converter { olx, mod_dir, assets: BTreeMap::new(), sources: BTreeMap::new(), warnings: Vec::new() };
    let def = cx.convert(raw)?;
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: format!("olx-{}", slug(mod_dir.file_name())),
        name: def.name.clone(),
        version: "1".to_string(),
        kind: PackKind::Mod,
        authors: Vec::new(),
        origin: Some(origin),
        license: None,
    };
    Ok(ImportedMod { pack: ModPack { manifest, def, assets: cx.assets }, warnings: cx.warnings })
}

struct Converter<'a> {
    olx: &'a dyn ContentSource,
    mod_dir: &'a VPath,
    assets: BTreeMap<VPath, Vec<u8>>,
    /// Pack path -> OLX path it was copied from, to detect conflicts.
    sources: BTreeMap<VPath, VPath>,
    warnings: Vec<String>,
}

#[derive(Clone, Copy)]
enum AssetKind {
    Image,
    Sound,
}

impl Converter<'_> {
    fn convert(&mut self, raw: RawMod) -> Result<ModDef, LgsError> {
        if raw.trailing_bytes > 0 {
            self.warnings.push(format!("{} bytes after the worm block were ignored", raw.trailing_bytes));
        }
        let mut weapons = Vec::with_capacity(raw.weapons.len());
        let mut projectiles = BTreeMap::new();
        let mut interned: Vec<(ProjectileDef, String)> = Vec::new();
        let mut used_ids = BTreeMap::new();
        for rw in raw.weapons {
            let mut w: WeaponDef = rw.def;
            w.id = unique(&slug(&w.name), |id| used_ids.contains_key(id));
            used_ids.insert(w.id.clone(), ());
            if let WeaponKind::Projectile { sound, spawn, .. } = &mut w.kind {
                *sound = self.sound(sound.take(), &format!("weapon {:?}", w.name))?;
                let node = rw.projectile.expect("projectile weapons always carry a projectile");
                spawn.projectile = self.intern(&node, &w.id, &mut projectiles, &mut interned)?;
            }
            weapons.push(w);
        }
        Ok(ModDef { name: raw.name, worm: raw.worm, rope: raw.rope, weapons, projectiles })
    }

    /// Names `node` (and its children, depth first), reusing the name of an
    /// identical definition seen before.
    fn intern(
        &mut self,
        node: &ProjNode,
        base: &str,
        out: &mut BTreeMap<String, ProjectileDef>,
        interned: &mut Vec<(ProjectileDef, String)>,
    ) -> Result<String, LgsError> {
        let mut def = node.def.clone();
        if let (Some(child), Some(spawn)) = (&node.event_child, def.event_spawn.as_mut()) {
            spawn.projectile = self.intern(child, &format!("{base}.spawn"), out, interned)?;
        }
        if let (Some(child), Some(spawn)) = (&node.trail_child, def.trail.spawn.as_mut()) {
            spawn.projectile = self.intern(child, &format!("{base}.trail"), out, interned)?;
        }
        let ctx = format!("projectile {base}");
        if let ProjectileGfx::Image { file, .. } = &mut def.gfx {
            *file = self.image(file, &ctx)?;
        }
        def.hit.sound = self.sound(def.hit.sound.take(), &ctx)?;
        if let Some(a) = def.timer.action.as_mut() {
            a.sound = self.sound(a.sound.take(), &ctx)?;
        }
        def.player_hit.sound = self.sound(def.player_hit.sound.take(), &ctx)?;

        if let Some((_, name)) = interned.iter().find(|(d, _)| *d == def) {
            return Ok(name.clone());
        }
        let name = unique(base, |n| out.contains_key(n));
        out.insert(name.clone(), def.clone());
        interned.push((def, name.clone()));
        Ok(name)
    }

    fn image(&mut self, name: &str, ctx: &str) -> Result<String, LgsError> {
        match self.asset(AssetKind::Image, name)? {
            Some(p) => Ok(p),
            None => Err(LgsError::MissingImage(format!("{name} ({ctx})"))),
        }
    }

    /// Like OLX, a sound that cannot be found is dropped (`UseSound = false`).
    fn sound(&mut self, name: Option<String>, ctx: &str) -> Result<Option<String>, LgsError> {
        let Some(name) = name else { return Ok(None) };
        let found = self.asset(AssetKind::Sound, &name)?;
        if found.is_none() {
            self.warnings.push(format!("sound {name:?} not found ({ctx}); dropped as OLX does"));
        }
        Ok(found)
    }

    /// Finds an OLX asset, copies it into the pack, and returns its pack path.
    fn asset(&mut self, kind: AssetKind, name: &str) -> Result<Option<String>, LgsError> {
        let Ok(rel) = VPath::from_olx(name) else {
            self.warnings.push(format!("unsafe asset name {name:?} ignored"));
            return Ok(None);
        };
        let (mod_sub, data_dir, pack_dir) = match kind {
            AssetKind::Image => ("gfx", "data/gfx", "gfx"),
            AssetKind::Sound => ("sfx", "data/sounds", "sfx"),
        };
        let mut names = vec![rel.clone()];
        if let AssetKind::Sound = kind {
            let s = rel.as_str();
            let stem = s.rsplit_once('.').map_or(s, |(stem, _)| stem);
            if !s.to_ascii_lowercase().ends_with(".ogg") {
                names.push(VPath::new(&format!("{stem}.ogg"))?);
            }
        }
        let mut candidates = Vec::new();
        for base in [self.mod_dir.join(mod_sub)?, VPath::new(data_dir)?] {
            for n in &names {
                candidates.push(base.join(n.as_str())?);
            }
        }
        let Some(found) = find_ci_any(self.olx, &candidates) else { return Ok(None) };
        let file_name = found.file_name().to_ascii_lowercase();
        let pack_path = VPath::new(pack_dir)?.join(&file_name)?;
        match self.sources.get(&pack_path) {
            Some(prev) if *prev != found => return Err(LgsError::AssetConflict(pack_path.to_string())),
            Some(_) => {}
            None => {
                self.assets.insert(pack_path.clone(), self.olx.read(&found)?);
                self.sources.insert(pack_path.clone(), found);
            }
        }
        Ok(Some(pack_path.to_string()))
    }
}

/// `base`, or `base-2`, `base-3`, … whichever is not yet taken.
fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base}-{i}")).find(|c| !taken(c)).expect("unbounded")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::tests::Lgs;
    use rl_vfs::MemFs;

    fn origin() -> Origin {
        Origin { source: "test".into(), path: "Tiny".into(), revision: "0".into() }
    }

    fn gamedir(script: Vec<u8>) -> MemFs {
        MemFs::new()
            .with("Tiny/SCRIPT.LGS", script)
            .with("Tiny/Gfx/Shared.png", "mod-image")
            .with("data/sounds/boom.ogg", "ogg-from-data")
            .with("data/gfx/shared.png", "data-image")
    }

    fn two_weapons() -> Vec<u8> {
        // Both weapons fire an identical projectile that spawns an identical child.
        let child = |b: Lgs| b.pixel_projectile(3, None);
        Lgs::header("Tiny", 2)
            .projectile_weapon("Rocket", Some("Boom.wav"))
            .pixel_projectile(12, Some(child))
            .projectile_weapon("Rocket", Some("missing.wav"))
            .pixel_projectile(12, Some(child))
            .footer()
            .0
    }

    #[test]
    fn ids_names_and_dedup() {
        let fs = gamedir(two_weapons());
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        let d = &m.pack.def;
        assert_eq!(m.pack.manifest.id, "olx-tiny");
        assert_eq!(d.weapons.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), vec!["rocket", "rocket-2"]);
        assert_eq!(d.projectiles.keys().collect::<Vec<_>>(), vec!["rocket", "rocket.spawn"]);
        assert_eq!(d.weapons[1].projectile(), Some("rocket"));
        assert_eq!(d.projectiles["rocket"].event_spawn.as_ref().unwrap().projectile, "rocket.spawn");
    }

    #[test]
    fn sounds_resolve_like_olx_and_missing_ones_are_dropped() {
        let fs = gamedir(two_weapons());
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        assert_eq!(m.pack.def.weapons[0].sound(), Some("sfx/boom.ogg"));
        assert_eq!(m.pack.assets[&VPath::new("sfx/boom.ogg").unwrap()], b"ogg-from-data");
        assert_eq!(m.pack.def.weapons[1].sound(), None);
        assert!(m.warnings.iter().any(|w| w.contains("missing.wav")), "{:?}", m.warnings);
    }

    #[test]
    fn images_prefer_the_mod_directory() {
        let script = Lgs::header("Tiny", 1).projectile_weapon("Gun", None);
        // Image projectile: type 1, no timer, no trail, no gravity, damp 1.
        let script = script.i(1).f(0.0).f(0.0).i(0).i(0).f(1.0).s("shared.PNG");
        let script = script.i(0).i(0).i(0).i(0).i(0).i(0); // rotating, inc, speed, angle, spec angle, animating
        let script = script.i(8).i(8).i(0).i(0).i(0).i(0).i(0).i(0).i(0).i(0).footer().0; // hit nothing, plyhit nothing, exp, touch
        let fs = gamedir(script);
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        let ProjectileGfx::Image { file, .. } = &m.pack.def.projectiles["gun"].gfx else { panic!("image") };
        assert_eq!(file, "gfx/shared.png");
        assert_eq!(m.pack.assets[&VPath::new("gfx/shared.png").unwrap()], b"mod-image");
    }

    #[test]
    fn missing_image_is_an_error() {
        let script = Lgs::header("Tiny", 1).projectile_weapon("Gun", None);
        let script = script.i(1).f(0.0).f(0.0).i(0).i(0).f(1.0).s("nope.png").i(0).i(0).i(0).i(0).i(0).i(0);
        let script = script.i(8).i(8).i(0).i(0).i(0).i(0).i(0).i(0).i(0).i(0).footer().0;
        let err = import_mod(&gamedir(script), &VPath::new("Tiny").unwrap(), origin()).unwrap_err();
        assert!(matches!(err, LgsError::MissingImage(ref n) if n.contains("nope.png")), "{err}");
    }

    #[test]
    fn unsafe_asset_names_never_escape() {
        let script = Lgs::header("Tiny", 1).projectile_weapon("Gun", Some("..\\..\\secret.ogg"));
        let script = script.pixel_projectile(1, None).footer().0;
        let fs = gamedir(script).with("secret.ogg", "do not copy");
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        assert_eq!(m.pack.def.weapons[0].sound(), None);
        assert!(m.pack.assets.values().all(|v| v != b"do not copy"));
    }

    #[test]
    fn imported_pack_validates_and_roundtrips() {
        let fs = gamedir(two_weapons());
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        let mut out = MemFs::new();
        rl_pack::write_mod(&mut out, &m.pack).unwrap();
        assert_eq!(rl_pack::validate(&out), vec![]);
        assert_eq!(rl_pack::read_mod(&out).unwrap(), m.pack);
    }

    #[test]
    fn non_ascii_names_get_unique_ids() {
        let script = Lgs::header("Tiny", 2);
        let script = script.projectile_weapon("\u{e9}\u{e9}", None).pixel_projectile(1, None);
        let script = script.projectile_weapon("!!", None).pixel_projectile(2, None).footer().0;
        let m = import_mod(&gamedir(script), &VPath::new("Tiny").unwrap(), origin()).unwrap();
        let ids: Vec<&str> = m.pack.def.weapons.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, vec!["unnamed", "unnamed-2"]);
    }

    #[test]
    fn missing_script_is_reported() {
        let err =
            import_mod(&MemFs::new().with("x/a.txt", ""), &VPath::new("x").unwrap(), origin()).unwrap_err();
        assert!(matches!(err, LgsError::MissingFile(_)));
    }
}
```

- [ ] **Step 5: Replace `crates/rl-import-lgs/src/lib.rs`**

```rust
//! OpenLieroX `script.lgs` importer.
//!
//! Reads gamescript version 7 (LX 0.56), the version of every mod in scope,
//! following `CGameScript::Load` and `CGameScript::LoadProjectile`
//! (`src/common/CGameScript.cpp:582-1152`), and produces a self-contained
//! [`rl_pack::ModPack`] including the image and sound files the mod uses.

mod convert;
mod parse;
mod reader;

pub use convert::{ImportedMod, import_mod};
pub use parse::{ProjNode, RawMod, RawWeapon, parse_lgs};

/// Everything that can go wrong while importing a mod.
#[derive(Debug, thiserror::Error)]
pub enum LgsError {
    #[error("file ends early at byte {offset} while reading {what}")]
    Truncated { offset: usize, what: &'static str },
    #[error("not a Liero Game Script (bad header id)")]
    BadId,
    #[error("unsupported gamescript version {0} (only version 7 is supported)")]
    UnsupportedVersion(u32),
    #[error("implausible {what}: {value}")]
    Implausible { what: &'static str, value: i64 },
    #[error("projectiles nested deeper than 128 levels")]
    TooDeep,
    #[error("unknown projectile type {0}")]
    UnknownProjectileType(i32),
    #[error("unknown weapon type {0} in weapon {1:?}")]
    UnknownWeaponType(i32, String),
    #[error("{0} not found")]
    MissingFile(String),
    #[error("image {0:?} not found in the mod's gfx/ or in data/gfx/")]
    MissingImage(String),
    #[error("two different files map to {0}")]
    AssetConflict(String),
    #[error(transparent)]
    Vfs(#[from] rl_vfs::VfsError),
}
```

- [ ] **Step 6: Run**

```bash
RL_OLX_GAMEDIR=$PWD/target/olx/d1ad953f4f20610f2648cce26c9739e7c088409d/share/gamedir cargo nextest run -p rl-import-lgs
```

Expected: output contains `21 passed`. `liero_1_0` (38 weapons) and `modern_warfare_1_0` (50 weapons) import with no warnings, validate with no errors, and round-trip.

- [ ] **Step 7: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "rl-import-lgs: convert mods to packs with OLX asset lookup"
```


---

### Task 9: `rl-import-lxl`: level importer, verified on the four levels in scope

Ports the image-type `.lxl` loader, including the hi-res chunk, with bounded decompression.

**Files:**

- Create: `crates/rl-import-lxl/{Cargo.toml,CLAUDE.md}`, `crates/rl-import-lxl/tests/olx_levels.rs`, `crates/rl-import-lxl/src/lib.rs`
- Modify: `xtask/layers.ron` (register `rl-import-lxl`; allow `rl-import-lxl` → `rl-pack`)

**Interfaces:**

- Produces: `import_lxl(olx: &dyn ContentSource, path: &VPath, origin: Origin) -> Result<ImportedLevel, LxlError>`, `parse_lxl(&[u8]) -> Result<(Level, Vec<String>), LxlError>`, and `ImportedLevel { pack: LevelPack, warnings }`.
- Produces: pack id `olx-level-<slug(file stem)>`; `LevelDef.theme = "olx-theme-<slug(theme)>"`; `art_scale` 1, or 2 with a hi-res chunk.

- [ ] **Step 1: Create `crates/rl-import-lxl/Cargo.toml`**

```toml
[package]
name = "rl-import-lxl"
description = "Imports OpenLieroX .lxl levels into native level packs."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
rl-data.workspace = true
rl-vfs.workspace = true
rl-pack.workspace = true
flate2.workspace = true
thiserror.workspace = true

[dev-dependencies]
proptest.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/rl-import-lxl/CLAUDE.md`**

```markdown
# rl-import-lxl

Imports OLX `.lxl` levels (image type) into `LevelPack`s.

- **Layer 2.** Ports `ML_LieroX` (OLX `src/common/MapLoader_LieroX.cpp`).
- **Material:** `Material::from_lx_flags`; rock beats dirt, anything else is background.
- **Front pixels** where `PX_EMPTY` is set are replaced by the back pixel, exactly as OLX does.
- **Art scale:** art is stored at 1×, because OLX's 2× is plain pixel doubling.
  An "OLX hi-res data" chunk gives 2× art instead.
- **Decompression is bounded** by width × height × 7 + 1 bytes.
- Pixmap levels (type 0) are rejected; none are in scope.

Test: `cargo nextest run -p rl-import-lxl` (real levels when `RL_OLX_GAMEDIR` is set).
```

- [ ] **Step 3: Create `crates/rl-import-lxl/tests/olx_levels.rs`**

```rust
//! Imports the real OLX levels in scope when `RL_OLX_GAMEDIR` is set.

use rl_data::level::Material;
use rl_data::manifest::Origin;
use rl_vfs::{DirFs, MemFs, VPath};

#[test]
fn levels_in_scope() {
    let Ok(dir) = std::env::var("RL_OLX_GAMEDIR") else {
        eprintln!("RL_OLX_GAMEDIR not set; skipping real-level import test");
        return;
    };
    let fs = DirFs::new(dir);
    // (file, name, width, height, dirt cells) from the pinned OLX commit.
    let cases = [
        ("levels/Liero Factory.lxl", "Liero Factory", 1200, 480, 24490),
        ("levels/CastleStrike.lxl", "Castle Strike", 1350, 550, 5536),
        ("levels/FossilFacility.lxl", "Fossil Facility", 640, 480, 19261),
        ("levels/Dirt Level.lxl", "Dirt Level", 504, 350, 159596),
    ];
    for (file, name, w, h, dirt) in cases {
        let origin = Origin { source: "OpenLieroX".into(), path: file.into(), revision: "test".into() };
        let l = rl_import_lxl::import_lxl(&fs, &VPath::new(file).unwrap(), origin).unwrap();
        assert!(l.warnings.is_empty(), "{file}: {:?}", l.warnings);
        let lv = &l.pack.level;
        assert_eq!((lv.def.name.as_str(), lv.def.width, lv.def.height), (name, w, h));
        assert_eq!(lv.material.count(Material::Dirt), dirt, "{file}");
        let mut out = MemFs::new();
        rl_pack::write_level(&mut out, &l.pack).unwrap();
        assert_eq!(rl_pack::validate(&out), vec![], "{file}");
        assert_eq!(rl_pack::read_level(&out).unwrap(), l.pack, "{file}");
    }
}
```

- [ ] **Step 4: Replace `xtask/layers.ron`**

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
        "xtask": 6,
    },
    same_layer: [
        ("rl-import-lgs", "rl-pack"),
        ("rl-import-lxl", "rl-pack"),
    ],
    no_std: ["rl-data"],
    wasm: ["rl-data", "rl-vfs", "rl-pack", "rl-import-lgs", "rl-import-lxl"],
)
```

- [ ] **Step 5: Create `crates/rl-import-lxl/src/lib.rs`**

```rust
//! OpenLieroX `.lxl` level importer, following `ML_LieroX`
//! (`src/common/MapLoader_LieroX.cpp`). Only image-type levels (`MPT_IMAGE`)
//! are supported; every level in scope is one.
//!
//! Imported art is stored at `art_scale` 1 because OLX's 2x surfaces are plain
//! pixel doubling (`PutPixel2x2`), unless the file carries an
//! "OLX hi-res data" chunk, which is imported at `art_scale` 2.

use std::io::Read;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Level, LevelDef, Material, MaterialGrid};
use rl_data::manifest::{Origin, PACK_FORMAT_VERSION, PackKind, PackManifest};
use rl_data::slug::slug;
use rl_pack::LevelPack;
use rl_vfs::{ContentSource, VPath};

const HEADER_LEN: usize = 148;
const MPT_IMAGE: i32 = 1;
const PX_EMPTY: u8 = 0x01;
/// OLX encodes worm positions in 12 bits, so no level is larger than this.
const MAX_SIDE: i32 = 4096;

#[derive(Debug, thiserror::Error)]
pub enum LxlError {
    #[error("file ends early at byte {0}")]
    Truncated(usize),
    #[error("not a LieroX level (id {0:?})")]
    BadId(String),
    #[error("unsupported level version {0}")]
    UnsupportedVersion(i32),
    #[error("implausible level size {0}x{1}")]
    BadSize(i32, i32),
    #[error("pixmap levels (type {0}) are not supported yet")]
    UnsupportedType(i32),
    #[error("level data does not decompress: {0}")]
    Decompress(String),
    #[error("level data has {got} bytes, needs {need}")]
    TooSmall { got: usize, need: usize },
    #[error(transparent)]
    Vfs(#[from] rl_vfs::VfsError),
}

/// The result of importing one level.
#[derive(Debug)]
pub struct ImportedLevel {
    pub pack: LevelPack,
    pub warnings: Vec<String>,
}

/// Imports the `.lxl` file at `path` of an OLX game directory.
pub fn import_lxl(olx: &dyn ContentSource, path: &VPath, origin: Origin) -> Result<ImportedLevel, LxlError> {
    let data = olx.read(path)?;
    let (level, warnings) = parse_lxl(&data)?;
    let stem = path.file_name().rsplit_once('.').map_or(path.file_name(), |(s, _)| s);
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: format!("olx-level-{}", slug(stem)),
        name: level.def.name.clone(),
        version: "1".to_string(),
        kind: PackKind::Level,
        authors: Vec::new(),
        origin: Some(origin),
        license: None,
    };
    Ok(ImportedLevel { pack: LevelPack { manifest, level }, warnings })
}

fn le_i32(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| c as char).collect()
}

/// Parses a complete `.lxl` file into a level plus warnings.
pub fn parse_lxl(data: &[u8]) -> Result<(Level, Vec<String>), LxlError> {
    if data.len() < HEADER_LEN + 8 {
        return Err(LxlError::Truncated(data.len()));
    }
    let id = cstr(&data[0..32]);
    let ctf = match id.as_str() {
        "LieroX Level" => false,
        "LieroX CTF Level" => true,
        _ => return Err(LxlError::BadId(id)),
    };
    let version = le_i32(data, 32);
    if version != 0 {
        return Err(LxlError::UnsupportedVersion(version));
    }
    let name = cstr(&data[36..100]);
    let (w, h) = (le_i32(data, 100), le_i32(data, 104));
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return Err(LxlError::BadSize(w, h));
    }
    let ty = le_i32(data, 108);
    if ty != MPT_IMAGE {
        return Err(LxlError::UnsupportedType(ty));
    }
    let theme = cstr(&data[112..144]);
    let (w, h) = (w as u32, h as u32);
    let n = w as usize * h as usize;
    let need = n * 7;

    let size = u32::from_le_bytes(data[148..152].try_into().expect("4 bytes")) as usize;
    let body_start = HEADER_LEN + 8;
    let body_end =
        body_start.checked_add(size).filter(|&e| e <= data.len()).ok_or(LxlError::Truncated(data.len()))?;
    let mut raw = Vec::with_capacity(need + 1);
    // Bounded read: a forged size cannot make us inflate more than one byte past what we use.
    flate2::read::ZlibDecoder::new(&data[body_start..body_end])
        .take(need as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| LxlError::Decompress(e.to_string()))?;
    if raw.len() < need {
        return Err(LxlError::TooSmall { got: raw.len(), need });
    }

    let rgb = |i: usize| Rgba::rgb(raw[i * 3], raw[i * 3 + 1], raw[i * 3 + 2]);
    let mut back = RgbaImage::new(w, h);
    let mut front = RgbaImage::new(w, h);
    let mut cells = Vec::with_capacity(n);
    for i in 0..n {
        let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
        let b = rgb(i);
        let flags = raw[n * 6 + i];
        back.set(x, y, b);
        // LoadImageFormat copies the back pixel wherever PX_EMPTY is set.
        front.set(x, y, if flags & PX_EMPTY != 0 { b } else { rgb(n + i) });
        cells.push(Material::from_lx_flags(flags));
    }
    let material = MaterialGrid::from_cells(w, h, cells).expect("n cells");

    let mut warnings = Vec::new();
    let mut rest = &data[body_end..];
    if ctf {
        warnings.push("old-format CTF data ignored".to_string());
        rest = rest.get(12..).unwrap_or(&[]);
    }
    let mut art_scale = 1;
    while !rest.is_empty() {
        if rest.len() < 20 {
            warnings.push("truncated chunk header ignored".to_string());
            break;
        }
        let chunk = cstr(&rest[..16]);
        let len = u32::from_le_bytes(rest[16..20].try_into().expect("4 bytes")) as usize;
        let Some(body) = rest.get(20..20 + len) else {
            warnings.push(format!("truncated chunk {chunk:?} ignored"));
            break;
        };
        rest = &rest[20 + len..];
        if chunk.eq_ignore_ascii_case("OLX hi-res data") {
            match hires(body, w, h) {
                Ok((f, b)) => {
                    front = f;
                    back = b;
                    art_scale = 2;
                }
                Err(e) => warnings.push(format!("hi-res chunk ignored: {e}")),
            }
        } else {
            warnings.push(format!("chunk {chunk:?} ignored"));
        }
    }

    let def = LevelDef {
        name,
        width: w,
        height: h,
        art_scale,
        theme: format!("olx-theme-{}", slug(&theme)),
        spawns: Vec::new(),
    };
    Ok((Level { def, material, front, back }, warnings))
}

/// "OLX hi-res data": a PNG of 2W x 4H with the front image on top and the back
/// image below (`LoadLevelImageHiRes`, `MapLoader_LieroX.cpp:285-360`).
fn hires(png: &[u8], w: u32, h: u32) -> Result<(RgbaImage, RgbaImage), String> {
    let img = rl_pack::image_io::decode_rgba(png)?;
    if (img.width(), img.height()) != (w * 2, h * 4) {
        return Err(format!("size {}x{} is not {}x{}", img.width(), img.height(), w * 2, h * 4));
    }
    Ok((img.crop(0, 0, w * 2, h * 2), img.crop(0, h * 2, w * 2, h * 2)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Builds an image-type `.lxl` from back/front colors and flags.
    fn lxl(w: i32, h: i32, back: &[[u8; 3]], front: &[[u8; 3]], flags: &[u8], extra: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let fixed = |s: &str, len: usize| {
            let mut b = vec![0xCCu8; len]; // garbage after NUL like real files
            b[..s.len()].copy_from_slice(s.as_bytes());
            b[s.len()] = 0;
            b
        };
        out.extend(fixed("LieroX Level", 32));
        out.extend(0i32.to_le_bytes());
        out.extend(fixed("Test Level", 64));
        out.extend(w.to_le_bytes());
        out.extend(h.to_le_bytes());
        out.extend(MPT_IMAGE.to_le_bytes());
        out.extend(fixed("dirt", 32));
        out.extend(0i32.to_le_bytes());
        let mut raw: Vec<u8> = back.iter().chain(front).flatten().copied().collect();
        raw.extend_from_slice(flags);
        raw.push(0); // real files have one extra byte
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        let z = z.finish().unwrap();
        out.extend((z.len() as u32).to_le_bytes());
        out.extend((raw.len() as u32).to_le_bytes());
        out.extend(z);
        out.extend_from_slice(extra);
        out
    }

    fn two_by_one() -> Vec<u8> {
        lxl(2, 1, &[[10, 10, 10], [20, 20, 20]], &[[1, 2, 3], [4, 5, 6]], &[0x01, 0x02], &[])
    }

    #[test]
    fn parses_header_material_and_art() {
        let (level, warnings) = parse_lxl(&two_by_one()).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(level.def.name, "Test Level");
        assert_eq!((level.def.width, level.def.height, level.def.art_scale), (2, 1, 1));
        assert_eq!(level.def.theme, "olx-theme-dirt");
        assert_eq!(level.material.cells(), &[Material::Background, Material::Dirt]);
        // Empty pixel shows the back image; dirt shows the front image.
        assert_eq!(level.front.get(0, 0), Rgba::rgb(10, 10, 10));
        assert_eq!(level.front.get(1, 0), Rgba::rgb(4, 5, 6));
        assert_eq!(level.back.get(1, 0), Rgba::rgb(20, 20, 20));
    }

    #[test]
    fn hires_chunk_replaces_art() {
        let mut hi = RgbaImage::new(4, 4);
        hi.set(0, 0, Rgba::rgb(7, 7, 7)); // front, top half
        hi.set(3, 3, Rgba::rgb(9, 9, 9)); // back, bottom half
        let png = rl_pack::image_io::encode_rgba(&hi);
        let mut chunk = b"OLX hi-res data\0".to_vec();
        chunk.extend((png.len() as u32).to_le_bytes());
        chunk.extend(&png);
        let data = lxl(2, 1, &[[0; 3]; 2], &[[0; 3]; 2], &[1, 1], &chunk);
        let (level, warnings) = parse_lxl(&data).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(level.def.art_scale, 2);
        assert_eq!(level.front.get(0, 0), Rgba::rgb(7, 7, 7));
        assert_eq!(level.back.get(3, 1), Rgba::rgb(9, 9, 9));
    }

    #[test]
    fn bad_hires_chunk_is_a_warning() {
        let mut chunk = b"OLX hi-res data\0".to_vec();
        chunk.extend(3u32.to_le_bytes());
        chunk.extend(b"bad");
        let (level, warnings) = parse_lxl(&lxl(2, 1, &[[0; 3]; 2], &[[0; 3]; 2], &[1, 1], &chunk)).unwrap();
        assert_eq!(level.def.art_scale, 1);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn rejects_bad_inputs() {
        let mut d = two_by_one();
        d[0] = b'X';
        assert!(matches!(parse_lxl(&d), Err(LxlError::BadId(_))));
        let mut d = two_by_one();
        d[108] = 0;
        assert!(matches!(parse_lxl(&d), Err(LxlError::UnsupportedType(0))));
        let d = lxl(0, 1, &[], &[], &[], &[]);
        assert!(matches!(parse_lxl(&d), Err(LxlError::BadSize(0, 1))));
        let d = lxl(3, 1, &[[0; 3]; 2], &[[0; 3]; 2], &[1, 1], &[]);
        assert!(matches!(parse_lxl(&d), Err(LxlError::TooSmall { .. })));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let d = two_by_one();
        for len in 0..d.len() {
            assert!(parse_lxl(&d[..len]).is_err(), "prefix of {len} bytes parsed");
        }
    }

    proptest::proptest! {
        #[test]
        fn never_panics(pos in 0usize..400, byte in proptest::num::u8::ANY) {
            let mut d = two_by_one();
            let pos = pos % d.len();
            d[pos] = byte;
            let _ = parse_lxl(&d);
        }
    }
}
```

- [ ] **Step 6: Run**

```bash
RL_OLX_GAMEDIR=$PWD/target/olx/d1ad953f4f20610f2648cce26c9739e7c088409d/share/gamedir cargo nextest run -p rl-import-lxl
```

Expected: output contains `7 passed`. `levels_in_scope` checks each level's name, size and exact dirt-cell count against OLX.

- [ ] **Step 7: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "rl-import-lxl: import image-type levels"
```


---

### Task 10: `rl-import-gfx`: skins and themes, verified on all 409 skins and the Dirt theme

Converts OLX skin sheets into image plus explicit mask, and themes into theme packs.

**Files:**

- Create: `crates/rl-import-gfx/{Cargo.toml,CLAUDE.md}`, `crates/rl-import-gfx/tests/olx_gfx.rs`, `crates/rl-import-gfx/src/{lib,skin,theme}.rs`
- Modify: `xtask/layers.ron` (register `rl-import-gfx`; allow `rl-import-gfx` → `rl-pack`)

**Interfaces:**

- Produces: `import_skin(olx, path: &VPath, origin) -> Result<ImportedSkin, GfxError>` (pack id `olx-skin-<slug(stem)>`) and `import_theme(olx, dir: &VPath, origin) -> Result<ImportedTheme, GfxError>` (pack id `olx-theme-<slug(dir name)>`).
- Produces: `GfxError::{Image, MissingFile, Invalid, Vfs}`.

- [ ] **Step 1: Create `crates/rl-import-gfx/Cargo.toml`**

```toml
[package]
name = "rl-import-gfx"
description = "Imports OpenLieroX skins and level themes into native packs."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
rl-data.workspace = true
rl-vfs.workspace = true
rl-pack.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/rl-import-gfx/CLAUDE.md`**

```markdown
# rl-import-gfx

Imports OLX worm skins and level themes.

- **Layer 2.**
- **Skins** follow `CGameSkin::Colorize_Execute` (OLX `src/client/CGameSkin.cpp:720-790`):
  - the top 18 rows are the image and the next 18 rows the mask;
  - a black mask pixel keeps the pixel, magenta hides it, any other color tints it;
  - magenta or alpha-0 image pixels are always transparent;
  - sheets larger than 672×36 are cropped with a warning.
- **Themes** follow `CMap::LoadTheme` (OLX `src/common/CMap.cpp:308-370`):
  5 holes, plus `NumStones` and `NumMisc` from `theme.txt`, plus the tiles.
  Stamp colors are kept raw, because their colors carry meaning when carving.

Test: `cargo nextest run -p rl-import-gfx` (all 409 skins when `RL_OLX_GAMEDIR` is set).
```

- [ ] **Step 3: Create `crates/rl-import-gfx/tests/olx_gfx.rs`**

```rust
//! Imports every OLX skin and the Dirt theme when `RL_OLX_GAMEDIR` is set.

use rl_data::manifest::Origin;
use rl_vfs::{ContentSource, DirFs, MemFs, VPath};

fn origin(path: &str) -> Origin {
    Origin { source: "OpenLieroX".into(), path: path.into(), revision: "test".into() }
}

#[test]
fn all_skins_and_dirt_theme() {
    let Ok(dir) = std::env::var("RL_OLX_GAMEDIR") else {
        eprintln!("RL_OLX_GAMEDIR not set; skipping real skin/theme import test");
        return;
    };
    let fs = DirFs::new(dir);
    let skins = VPath::new("skins").unwrap();
    let mut imported = 0;
    for name in fs.list(&skins).unwrap() {
        if !name.to_ascii_lowercase().ends_with(".png") {
            continue;
        }
        let s = rl_import_gfx::import_skin(&fs, &skins.join(&name).unwrap(), origin("skins")).unwrap();
        let mut out = MemFs::new();
        rl_pack::write_skin(&mut out, &s.pack).unwrap();
        assert_eq!(rl_pack::read_skin(&out).unwrap(), s.pack, "{name}");
        imported += 1;
    }
    assert_eq!(imported, 409);

    let t =
        rl_import_gfx::import_theme(&fs, &VPath::new("data/themes/Dirt").unwrap(), origin("Dirt")).unwrap();
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
    assert_eq!((t.pack.theme.stones.len(), t.pack.theme.misc.len()), (8, 8));
    let mut out = MemFs::new();
    rl_pack::write_theme(&mut out, &t.pack).unwrap();
    assert_eq!(rl_pack::validate(&out), vec![]);
}
```

- [ ] **Step 4: Replace `xtask/layers.ron`**

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

- [ ] **Step 5: Create `crates/rl-import-gfx/src/lib.rs`**

```rust
//! OpenLieroX skin and theme importers.

mod skin;
mod theme;

pub use skin::{ImportedSkin, import_skin};
pub use theme::{ImportedTheme, import_theme};

#[derive(Debug, thiserror::Error)]
pub enum GfxError {
    #[error("{file}: {message}")]
    Image { file: String, message: String },
    #[error("{0} not found")]
    MissingFile(String),
    #[error("{file}: {message}")]
    Invalid { file: String, message: String },
    #[error(transparent)]
    Vfs(#[from] rl_vfs::VfsError),
}

pub(crate) fn decode(
    src: &dyn rl_vfs::ContentSource,
    path: &rl_vfs::VPath,
) -> Result<rl_data::image::RgbaImage, GfxError> {
    let bytes = src.read(path)?;
    rl_pack::image_io::decode_rgba(&bytes)
        .map_err(|message| GfxError::Image { file: path.to_string(), message })
}

pub(crate) fn manifest(
    id: String,
    name: String,
    kind: rl_data::manifest::PackKind,
    origin: rl_data::manifest::Origin,
) -> rl_data::manifest::PackManifest {
    rl_data::manifest::PackManifest {
        format_version: rl_data::manifest::PACK_FORMAT_VERSION,
        id,
        name,
        version: "1".to_string(),
        kind,
        authors: Vec::new(),
        origin: Some(origin),
        license: None,
    }
}
```

- [ ] **Step 6: Create `crates/rl-import-gfx/src/skin.rs`**

```rust
//! Skin sheets (`CGameSkin`, `src/client/CGameSkin.cpp`): the top half is the
//! image, the bottom half the colorization mask, each `frame_height` rows tall.

use rl_data::image::Rgba;
use rl_data::manifest::{Origin, PackKind};
use rl_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};
use rl_data::slug::slug;
use rl_pack::SkinPack;
use rl_vfs::{ContentSource, VPath};

use crate::{GfxError, decode, manifest};

/// Width of a standard sheet: 21 frames of 32 pixels.
const SHEET_WIDTH: u32 = 672;

#[derive(Debug)]
pub struct ImportedSkin {
    pub pack: SkinPack,
    pub warnings: Vec<String>,
}

/// Imports the skin PNG at `path`. Sheets larger than 672x36 are cropped to it,
/// as OLX only reads that area; smaller sheets are an error.
pub fn import_skin(olx: &dyn ContentSource, path: &VPath, origin: Origin) -> Result<ImportedSkin, GfxError> {
    let sheet = decode(olx, path)?;
    let layout = FrameLayout::OLX;
    let fh = layout.frame_height;
    let (w, h) = (sheet.width(), sheet.height());
    if w < SHEET_WIDTH || h < fh * 2 {
        return Err(GfxError::Invalid {
            file: path.to_string(),
            message: format!("sheet is {w}x{h}, needs at least {SHEET_WIDTH}x{}", fh * 2),
        });
    }
    let mut warnings = Vec::new();
    if (w, h) != (SHEET_WIDTH, fh * 2) {
        warnings.push(format!("{path}: sheet is {w}x{h}, cropped to {SHEET_WIDTH}x{}", fh * 2));
    }
    let mut image = sheet.crop(0, 0, SHEET_WIDTH, fh);
    let mut mask = Vec::with_capacity((SHEET_WIDTH * fh) as usize);
    for y in 0..fh {
        for x in 0..SHEET_WIDTH {
            let px = image.get(x, y);
            let m = sheet.get(x, y + fh);
            // Same order as Colorize_Execute: black keeps, magenta hides, anything else tints.
            let mut kind = if m.same_rgb(Rgba::BLACK) {
                MaskKind::Keep
            } else if m.same_rgb(Rgba::MAGENTA) {
                MaskKind::Transparent
            } else {
                MaskKind::Colorize
            };
            // A magenta or fully transparent image pixel is never drawn (color key).
            if px.same_rgb(Rgba::MAGENTA) || px.a == 0 || m.a == 0 {
                kind = MaskKind::Transparent;
            }
            if kind == MaskKind::Transparent {
                image.set(x, y, Rgba { a: 0, ..px });
            } else {
                image.set(x, y, Rgba { a: 255, ..px });
            }
            mask.push(kind);
        }
    }
    let file = path.file_name();
    let name = file.rsplit_once('.').map_or(file, |(s, _)| s).to_string();
    let pack = SkinPack {
        manifest: manifest(format!("olx-skin-{}", slug(&name)), name.clone(), PackKind::Skin, origin),
        skin: Skin { def: SkinDef { name, layout }, image, mask },
    };
    Ok(ImportedSkin { pack, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::image::RgbaImage;
    use rl_vfs::MemFs;

    fn sheet(w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.set(x, y, Rgba::rgb(100, 100, 100));
            }
        }
        // Mask row 0 (sheet row 18): x=0 black, x=1 magenta, x=2 white.
        img.set(0, 18, Rgba::BLACK);
        img.set(1, 18, Rgba::MAGENTA);
        img.set(2, 18, Rgba::rgb(255, 255, 255));
        // Image pixel x=3 is magenta with a colorize mask: still transparent.
        img.set(3, 0, Rgba::MAGENTA);
        img
    }

    fn import(img: &RgbaImage) -> Result<ImportedSkin, GfxError> {
        let fs = MemFs::new().with("skins/Cool Worm.png", rl_pack::image_io::encode_rgba(img));
        let origin = Origin { source: "t".into(), path: "skins".into(), revision: "0".into() };
        import_skin(&fs, &VPath::new("skins/Cool Worm.png").unwrap(), origin)
    }

    #[test]
    fn mask_follows_olx_rules() {
        let s = import(&sheet(672, 36)).unwrap();
        assert!(s.warnings.is_empty());
        let skin = &s.pack.skin;
        assert_eq!(s.pack.manifest.id, "olx-skin-cool-worm");
        assert_eq!((skin.image.width(), skin.image.height()), (672, 18));
        assert_eq!(skin.mask_at(0, 0), MaskKind::Keep);
        assert_eq!(skin.mask_at(1, 0), MaskKind::Transparent);
        assert_eq!(skin.mask_at(2, 0), MaskKind::Colorize);
        assert_eq!(skin.mask_at(3, 0), MaskKind::Transparent);
        assert_eq!(skin.image.get(1, 0).a, 0);
    }

    #[test]
    fn oversized_sheets_are_cropped_with_a_warning() {
        let s = import(&sheet(673, 51)).unwrap();
        assert_eq!(s.warnings.len(), 1);
        assert_eq!(s.pack.skin.image.width(), 672);
    }

    #[test]
    fn undersized_sheets_are_rejected() {
        assert!(matches!(import(&sheet(600, 36)), Err(GfxError::Invalid { .. })));
    }
}
```

- [ ] **Step 7: Create `crates/rl-import-gfx/src/theme.rs`**

```rust
//! Level themes (`CMap::LoadTheme`, `src/common/CMap.cpp:308-370`):
//! five holes, `NumStones` stones and `NumMisc` misc images from `theme.txt`,
//! plus the front and back tiles.

use rl_data::manifest::{Origin, PackKind};
use rl_data::slug::slug;
use rl_data::theme::{Theme, ThemeDef};
use rl_pack::ThemePack;
use rl_vfs::{ContentSource, VPath, find_ci};

use crate::{GfxError, decode, manifest};

const HOLES: u32 = 5;

#[derive(Debug)]
pub struct ImportedTheme {
    pub pack: ThemePack,
    pub warnings: Vec<String>,
}

/// Reads an integer from the `[General]` section of an OLX ini file.
fn ini_int(text: &str, key: &str) -> Option<u32> {
    let mut in_general = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_general = line.eq_ignore_ascii_case("[General]");
        } else if in_general
            && let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return v.trim().parse().ok();
        }
    }
    None
}

/// Imports the theme in directory `dir` (such as `data/themes/Dirt`).
pub fn import_theme(olx: &dyn ContentSource, dir: &VPath, origin: Origin) -> Result<ImportedTheme, GfxError> {
    let find = |name: &str| -> Result<VPath, GfxError> {
        let p = dir.join(name)?;
        find_ci(olx, &p).ok_or_else(|| GfxError::MissingFile(p.to_string()))
    };
    let ini_path = find("theme.txt")?;
    let ini_bytes = olx.read(&ini_path)?;
    let ini = String::from_utf8_lossy(&ini_bytes);
    let mut warnings = Vec::new();
    let mut count = |key: &str| {
        ini_int(&ini, key).unwrap_or_else(|| {
            warnings.push(format!("{ini_path}: {key} missing, using 0 like OLX"));
            0
        })
    };
    let (stones, misc) = (count("NumStones"), count("NumMisc"));
    let list = |prefix: &str, n: u32| -> Result<Vec<_>, GfxError> {
        (1..=n).map(|i| decode(olx, &find(&format!("{prefix}{i}.png"))?)).collect()
    };
    let name = dir.file_name().to_string();
    let theme = Theme {
        def: ThemeDef { name: name.clone(), holes: HOLES, stones, misc },
        holes: list("Hole", HOLES)?,
        stones: list("Stone", stones)?,
        misc: list("misc", misc)?,
        front_tile: decode(olx, &find("Fronttile.png")?)?,
        back_tile: decode(olx, &find("Backtile.png")?)?,
    };
    let pack = ThemePack {
        manifest: manifest(format!("olx-theme-{}", slug(&name)), name, PackKind::Theme, origin),
        theme,
    };
    Ok(ImportedTheme { pack, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::image::RgbaImage;
    use rl_vfs::MemFs;

    #[test]
    fn ini_parsing() {
        let ini = "[Other]\nNumStones=9\n[General]\nNumStones = 8\r\nNumMisc=2\n";
        assert_eq!(ini_int(ini, "NumStones"), Some(8));
        assert_eq!(ini_int(ini, "nummisc"), Some(2));
        assert_eq!(ini_int(ini, "Missing"), None);
    }

    #[test]
    fn imports_with_case_insensitive_names() {
        let png = rl_pack::image_io::encode_rgba(&RgbaImage::new(2, 2));
        let mut fs =
            MemFs::new().with("data/themes/Dirt/theme.txt", "[General]\nNumStones = 1\nNumMisc = 0\n");
        for f in ["hole1", "hole2", "hole3", "hole4", "hole5", "Stone1", "FrontTile", "BackTile"] {
            fs = fs.with(&format!("data/themes/Dirt/{f}.png"), png.clone());
        }
        let origin = Origin { source: "t".into(), path: "Dirt".into(), revision: "0".into() };
        let t = import_theme(&fs, &VPath::new("data/themes/Dirt").unwrap(), origin).unwrap();
        assert!(t.warnings.is_empty());
        assert_eq!(t.pack.manifest.id, "olx-theme-dirt");
        assert_eq!((t.pack.theme.holes.len(), t.pack.theme.stones.len()), (5, 1));
    }

    #[test]
    fn missing_image_is_an_error() {
        let fs = MemFs::new().with("data/themes/X/theme.txt", "[General]\n");
        let origin = Origin { source: "t".into(), path: "X".into(), revision: "0".into() };
        let err = import_theme(&fs, &VPath::new("data/themes/X").unwrap(), origin).unwrap_err();
        assert!(matches!(err, GfxError::MissingFile(ref f) if f.ends_with("Hole1.png")), "{err}");
    }
}
```

- [ ] **Step 8: Run**

```bash
RL_OLX_GAMEDIR=$PWD/target/olx/d1ad953f4f20610f2648cce26c9739e7c088409d/share/gamedir cargo nextest run -p rl-import-gfx
```

Expected: output contains `7 passed`. `all_skins_and_dirt_theme` imports and round-trips all 409 skins; the Dirt theme has 8 stones and 8 misc.

- [ ] **Step 9: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "rl-import-gfx: import skins and themes"
```


---

### Task 11: `rl-cli`: `import` and `validate` commands

The headless entry point that `xtask fetch-olx-content` uses. Each item is imported independently; one failure does not stop the rest.

**Files:**

- Create: `crates/rl-cli/{Cargo.toml,CLAUDE.md}`, `crates/rl-cli/tests/cli.rs`, `crates/rl-cli/src/{main,import,validate}.rs`
- Modify: `xtask/layers.ron` (register `rl-cli`, layer 6)

**Interfaces:**

- Consumes: `import_mod` (Task 8), `import_lxl` (Task 9), `import_skin` and `import_theme` (Task 10), the `write_*` functions and `validate` (Tasks 5–6).
- Produces: `rl-cli import --olx <gamedir> --out <dir> [--revision R] [--mod D]... [--level F]... [--theme D]... [--skins]`. It writes `<out>/<id>/` per pack plus `<out>/import-report.ron`, deletes an existing pack directory before rewriting it, and exits with status 1 if any item failed.
- Produces: `rl-cli validate <dir>...`, which takes a pack directory or a parent of pack directories and exits with status 1 on any error.

- [ ] **Step 1: Create `crates/rl-cli/Cargo.toml`**

```toml
[package]
name = "rl-cli"
description = "Headless command-line tool: import OLX content, validate packs."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "rl-cli"
path = "src/main.rs"

[dependencies]
rl-data.workspace = true
rl-vfs.workspace = true
rl-pack.workspace = true
rl-import-lgs.workspace = true
rl-import-lxl.workspace = true
rl-import-gfx.workspace = true
anyhow.workspace = true
clap.workspace = true
serde = { workspace = true, features = ["std"] }
ron.workspace = true

[dev-dependencies]
tempfile.workspace = true
insta.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/rl-cli/CLAUDE.md`**

```markdown
# rl-cli

The headless command-line tool. In this milestone it has two commands:

- `rl-cli import --olx <share/gamedir> --out <dir> [--mod D]... [--level F]... [--theme D]... [--skins]`
  writes one pack per item to `<out>/<pack id>/`, plus `import-report.ron`.
  It exits with status 1 if any item failed, and still imports the rest.
- `rl-cli validate <pack-or-parent-dir>...` prints diagnostics and exits with
  status 1 on any error.

**Layer 6.** Later milestones add `match`, `replay`, `scenario`, `dump-state` and `render` (spec §7.4).

Test: `cargo nextest run -p rl-cli`. `tests/corpus.rs` checks every imported
pack when `RL_CONTENT_DIR` is set. To accept a reviewed snapshot change, run
`INSTA_UPDATE=always cargo test -p rl-cli --test corpus`.
```

- [ ] **Step 3: Create `crates/rl-cli/tests/cli.rs`**

```rust
//! End-to-end tests of the `rl-cli` binary on a synthetic OLX game directory.

use std::path::Path;
use std::process::Command;

use rl_data::image::{Rgba, RgbaImage};

fn png(w: u32, h: u32, c: Rgba) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    rl_pack::image_io::encode_rgba(&img)
}

fn gamedir(root: &Path) {
    let theme = root.join("data/themes/Dirt");
    std::fs::create_dir_all(&theme).unwrap();
    std::fs::write(theme.join("theme.txt"), "[General]\nNumStones = 0\nNumMisc = 0\n").unwrap();
    for f in ["Hole1", "Hole2", "Hole3", "Hole4", "Hole5", "FrontTile", "BackTile"] {
        std::fs::write(theme.join(format!("{f}.png")), png(4, 4, Rgba::BLACK)).unwrap();
    }
    let skins = root.join("skins");
    std::fs::create_dir_all(&skins).unwrap();
    std::fs::write(skins.join("default.png"), png(672, 36, Rgba::rgb(200, 200, 200))).unwrap();
    std::fs::write(skins.join("Default.png"), png(672, 36, Rgba::rgb(100, 100, 100))).unwrap();
    std::fs::write(skins.join("broken.png"), b"not a png").unwrap();
    std::fs::write(skins.join("readme.txt"), b"ignored").unwrap();
}

fn rl_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rl-cli"))
}

#[test]
fn import_then_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let status = rl_cli()
        .args(["import", "--olx"])
        .arg(&src)
        .arg("--out")
        .arg(&out)
        .args(["--revision", "abc", "--theme", "data/themes/Dirt", "--skins"])
        .status()
        .unwrap();
    assert!(!status.success(), "the broken skin must make import fail");
    // Both skins slug to the same id; the second gets a suffix.
    assert!(out.join("olx-skin-default/pack.ron").is_file());
    assert!(out.join("olx-skin-default-2/pack.ron").is_file());
    assert!(out.join("olx-theme-dirt/theme.ron").is_file());
    let report = std::fs::read_to_string(out.join("import-report.ron")).unwrap();
    assert!(report.contains("skins/broken.png") && report.contains("error: Some("), "{report}");

    let status = rl_cli().arg("validate").arg(&out).status().unwrap();
    assert!(status.success());
}

#[test]
fn validate_fails_on_broken_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("bad");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("pack.ron"), "(oops").unwrap();
    let out = rl_cli().arg("validate").arg(&pack).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("pack.ron:1:"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn reimport_replaces_stale_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let import = || {
        rl_cli()
            .args(["import", "--olx"])
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .args(["--theme", "data/themes/Dirt"])
            .status()
            .unwrap()
    };
    assert!(import().success());
    let stale = out.join("olx-theme-dirt/stale.png");
    std::fs::write(&stale, b"left over from an older import").unwrap();
    assert!(import().success());
    assert!(!stale.exists(), "re-import must remove files the importer no longer writes");
}
```

- [ ] **Step 4: Replace `xtask/layers.ron`**

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

- [ ] **Step 5: Create `crates/rl-cli/src/main.rs`**

```rust
//! `rl-cli`: headless tools for rust-liero. Run `rl-cli --help`.

mod import;
mod validate;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rl-cli", about = "Headless tools for rust-liero")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import OpenLieroX content into native packs (one directory per pack).
    Import(import::Args),
    /// Validate packs; exits with status 1 if any pack has errors.
    Validate {
        /// Pack directories, or directories containing pack directories.
        paths: Vec<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Import(args) => import::run(&args),
        Command::Validate { paths } => validate::run(&paths),
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
```

- [ ] **Step 6: Create `crates/rl-cli/src/import.rs`**

```rust
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use rl_data::manifest::Origin;
use rl_vfs::{ContentSource, DirFs, VPath};
use serde::Serialize;

#[derive(clap::Args)]
pub struct Args {
    /// OLX game directory (`share/gamedir` of an OpenLieroX checkout).
    #[arg(long)]
    olx: PathBuf,
    /// Output directory; each pack is written to `<out>/<pack id>/`.
    #[arg(long)]
    out: PathBuf,
    /// Source revision recorded in each pack's origin, such as a git commit.
    #[arg(long, default_value = "unknown")]
    revision: String,
    /// Mod directories relative to the game directory, such as "MW 1.0".
    #[arg(long = "mod")]
    mods: Vec<String>,
    /// Level files relative to the game directory, such as "levels/CastleStrike.lxl".
    #[arg(long = "level")]
    levels: Vec<String>,
    /// Theme directories relative to the game directory, such as "data/themes/Dirt".
    #[arg(long = "theme")]
    themes: Vec<String>,
    /// Import every PNG in `skins/`.
    #[arg(long)]
    skins: bool,
}

/// One line of `import-report.ron`.
#[derive(Serialize)]
struct Entry {
    source: String,
    pack: Option<String>,
    error: Option<String>,
    warnings: Vec<String>,
}

enum Imported {
    Mod(rl_import_lgs::ImportedMod),
    Level(rl_import_lxl::ImportedLevel),
    Skin(rl_import_gfx::ImportedSkin),
    Theme(rl_import_gfx::ImportedTheme),
}

impl Imported {
    fn id(&self) -> &str {
        match self {
            Imported::Mod(m) => &m.pack.manifest.id,
            Imported::Level(l) => &l.pack.manifest.id,
            Imported::Skin(s) => &s.pack.manifest.id,
            Imported::Theme(t) => &t.pack.manifest.id,
        }
    }

    fn set_id(&mut self, id: String) {
        match self {
            Imported::Mod(m) => m.pack.manifest.id = id,
            Imported::Level(l) => l.pack.manifest.id = id,
            Imported::Skin(s) => s.pack.manifest.id = id,
            Imported::Theme(t) => t.pack.manifest.id = id,
        }
    }

    fn warnings(&self) -> Vec<String> {
        match self {
            Imported::Mod(m) => m.warnings.clone(),
            Imported::Level(l) => l.warnings.clone(),
            Imported::Skin(s) => s.warnings.clone(),
            Imported::Theme(t) => t.warnings.clone(),
        }
    }

    fn write(&self, dir: &Path) -> anyhow::Result<()> {
        if dir.exists() {
            std::fs::remove_dir_all(dir).with_context(|| format!("removing old {}", dir.display()))?;
        }
        let mut sink = DirFs::new(dir);
        match self {
            Imported::Mod(m) => rl_pack::write_mod(&mut sink, &m.pack)?,
            Imported::Level(l) => rl_pack::write_level(&mut sink, &l.pack)?,
            Imported::Skin(s) => rl_pack::write_skin(&mut sink, &s.pack)?,
            Imported::Theme(t) => rl_pack::write_theme(&mut sink, &t.pack)?,
        }
        Ok(())
    }
}

enum Job {
    Mod(String),
    Level(String),
    Theme(String),
    Skin(VPath),
}

impl Job {
    fn source(&self) -> String {
        match self {
            Job::Mod(s) | Job::Level(s) | Job::Theme(s) => s.clone(),
            Job::Skin(p) => p.to_string(),
        }
    }

    fn run(&self, olx: &DirFs, revision: &str) -> anyhow::Result<Imported> {
        let source = self.source();
        let origin = Origin {
            source: "OpenLieroX".to_string(),
            path: format!("share/gamedir/{source}"),
            revision: revision.to_string(),
        };
        Ok(match self {
            Job::Mod(m) => Imported::Mod(rl_import_lgs::import_mod(olx, &VPath::new(m)?, origin)?),
            Job::Level(l) => Imported::Level(rl_import_lxl::import_lxl(olx, &VPath::new(l)?, origin)?),
            Job::Theme(t) => Imported::Theme(rl_import_gfx::import_theme(olx, &VPath::new(t)?, origin)?),
            Job::Skin(p) => Imported::Skin(rl_import_gfx::import_skin(olx, p, origin)?),
        })
    }
}

/// Returns `Ok(false)` if any item failed; the report lists every item either way.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let olx = DirFs::new(&args.olx);
    let mut jobs: Vec<Job> = Vec::new();
    jobs.extend(args.mods.iter().cloned().map(Job::Mod));
    jobs.extend(args.levels.iter().cloned().map(Job::Level));
    jobs.extend(args.themes.iter().cloned().map(Job::Theme));
    if args.skins {
        let skins = VPath::new("skins")?;
        for name in olx.list(&skins).context("listing skins/")? {
            if name.to_ascii_lowercase().ends_with(".png") {
                jobs.push(Job::Skin(skins.join(&name)?));
            }
        }
    }

    std::fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    let mut report = Vec::new();
    let mut ids = BTreeSet::new();
    for job in &jobs {
        let source = job.source();
        match job.run(&olx, &args.revision) {
            Ok(mut item) => {
                let base = item.id().to_string();
                let id = (1..)
                    .map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") })
                    .find(|c| !ids.contains(c))
                    .expect("unbounded");
                item.set_id(id.clone());
                ids.insert(id.clone());
                item.write(&args.out.join(&id))?;
                report.push(Entry { source, pack: Some(id), error: None, warnings: item.warnings() });
            }
            Err(e) => {
                eprintln!("error: {source}: {e:#}");
                report.push(Entry { source, pack: None, error: Some(format!("{e:#}")), warnings: vec![] });
            }
        }
    }
    let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())? + "\n";
    std::fs::write(args.out.join("import-report.ron"), text)?;
    let failed = report.iter().filter(|e| e.error.is_some()).count();
    let warned: usize = report.iter().map(|e| e.warnings.len()).sum();
    println!(
        "imported {} packs, {failed} failed, {warned} warnings (see import-report.ron)",
        report.len() - failed
    );
    Ok(failed == 0)
}
```

- [ ] **Step 7: Create `crates/rl-cli/src/validate.rs`**

```rust
use std::path::{Path, PathBuf};

use rl_pack::Severity;
use rl_vfs::DirFs;

/// Pack directories under `path`: `path` itself if it has `pack.ron`, else its
/// immediate subdirectories that do.
fn packs(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if path.join(rl_pack::MANIFEST).is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let p = entry?.path();
        if p.join(rl_pack::MANIFEST).is_file() {
            out.push(p);
        }
    }
    out.sort();
    if out.is_empty() {
        anyhow::bail!("{}: no pack.ron here or in any subdirectory", path.display());
    }
    Ok(out)
}

pub fn run(paths: &[PathBuf]) -> anyhow::Result<bool> {
    let (mut checked, mut errors, mut warnings) = (0, 0, 0);
    for path in paths {
        for pack in packs(path)? {
            checked += 1;
            for d in rl_pack::validate(&DirFs::new(&pack)) {
                match d.severity {
                    Severity::Error => errors += 1,
                    Severity::Warning => warnings += 1,
                }
                println!("{}: {d}", pack.display());
            }
        }
    }
    println!("validated {checked} packs: {errors} errors, {warnings} warnings");
    Ok(errors == 0)
}
```

- [ ] **Step 8: Run**

```bash
cargo nextest run -p rl-cli --test cli
```

Expected: output contains `3 passed`. Covers a broken PNG (reported, import continues), case-only duplicate skin names (`-2` suffix), and stale-file removal on re-import.

- [ ] **Step 9: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "rl-cli: add import and validate commands"
```


---

### Task 12: Corpus test, porting status and the full online gate

Wires everything together: `cargo xtask check` now fetches and imports the pinned OLX content, and the corpus test validates, round-trips and snapshots every pack.

**Files:**

- Create: `crates/rl-cli/tests/corpus.rs`, `crates/rl-cli/tests/snapshots/corpus__imported_content.snap`, `docs/porting/STATUS.md`

**Interfaces:**

- Consumes: everything above. `cargo xtask check` sets `RL_OLX_GAMEDIR` and `RL_CONTENT_DIR` for the test step.
- Produces: the M1 done-criterion. All content from spec §1.2 imports, validates and round-trips, and the snapshot pins the importer output.

- [ ] **Step 1: Create `crates/rl-cli/tests/corpus.rs`**

```rust
//! Corpus test (spec §7): every imported pack in `RL_CONTENT_DIR` validates
//! without errors and round-trips through write and read. A snapshot of the
//! mods and levels catches unintended importer changes. `cargo xtask check`
//! sets `RL_CONTENT_DIR` after importing the pinned OLX content.

use std::fmt::Write;

use rl_data::level::Material;
use rl_data::manifest::PackKind;
use rl_pack::Severity;
use rl_vfs::{DirFs, MemFs};

#[test]
fn imported_content() {
    let Ok(dir) = std::env::var("RL_CONTENT_DIR") else {
        eprintln!("RL_CONTENT_DIR not set; skipping corpus test");
        return;
    };
    let mut dirs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join(rl_pack::MANIFEST).is_file())
        .collect();
    dirs.sort();
    let mut summary = String::new();
    let mut skins = 0;
    for path in &dirs {
        let src = DirFs::new(path);
        let errors: Vec<_> =
            rl_pack::validate(&src).into_iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "{}: {errors:?}", path.display());
        let mut copy = MemFs::new();
        match rl_pack::read_manifest(&src).unwrap().kind {
            PackKind::Mod => {
                let p = rl_pack::read_mod(&src).unwrap();
                rl_pack::write_mod(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_mod(&copy).unwrap(), p);
                let d = &p.def;
                writeln!(
                    summary,
                    "mod {}: {:?}, {} weapons, {} projectiles, {} assets, rope {:?}",
                    p.manifest.id,
                    d.name,
                    d.weapons.len(),
                    d.projectiles.len(),
                    p.assets.len(),
                    d.rope
                )
                .unwrap();
            }
            PackKind::Level => {
                let p = rl_pack::read_level(&src).unwrap();
                rl_pack::write_level(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_level(&copy).unwrap(), p);
                let l = &p.level;
                writeln!(
                    summary,
                    "level {}: {:?} {}x{} art x{}, dirt {}, rock {}, theme {}",
                    p.manifest.id,
                    l.def.name,
                    l.def.width,
                    l.def.height,
                    l.def.art_scale,
                    l.material.count(Material::Dirt),
                    l.material.count(Material::Rock),
                    l.def.theme
                )
                .unwrap();
            }
            PackKind::Skin => {
                let p = rl_pack::read_skin(&src).unwrap();
                rl_pack::write_skin(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_skin(&copy).unwrap(), p);
                skins += 1;
            }
            PackKind::Theme => {
                let p = rl_pack::read_theme(&src).unwrap();
                rl_pack::write_theme(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_theme(&copy).unwrap(), p);
                let t = &p.theme.def;
                writeln!(
                    summary,
                    "theme {}: {} holes, {} stones, {} misc",
                    p.manifest.id, t.holes, t.stones, t.misc
                )
                .unwrap();
            }
        }
    }
    writeln!(summary, "skins: {skins}").unwrap();
    insta::assert_snapshot!(summary);
}
```

- [ ] The snapshot below is the exact output for the pinned commit. Write it as-is. If the test later reports a diff, the importer's output changed. Review the change, then accept it with `INSTA_UPDATE=always cargo test -p rl-cli --test corpus` and say why in the commit message.

- [ ] **Step 2: Create `crates/rl-cli/tests/snapshots/corpus__imported_content.snap`**

```text
---
source: crates/rl-cli/tests/corpus.rs
expression: summary
---
mod olx-classic: "Liero v1.0", 38 weapons, 58 projectiles, 39 assets, rope RopeParams { max_length: 300, rest_length: 27, strength: 4.5 }
level olx-level-castlestrike: "Castle Strike" 1350x550 art x1, dirt 5536, rock 147603, theme olx-theme-dirt
level olx-level-dirt-level: "Dirt Level" 504x350 art x1, dirt 159596, rock 4949, theme olx-theme-dirt
level olx-level-fossilfacility: "Fossil Facility" 640x480 art x1, dirt 19261, rock 79490, theme olx-theme-dirt
level olx-level-liero-factory: "Liero Factory" 1200x480 art x1, dirt 24490, rock 102337, theme olx-theme-dirt
mod olx-mw-1-0: "Modern Warfare1.0", 50 weapons, 302 projectiles, 129 assets, rope RopeParams { max_length: 300, rest_length: 30, strength: 3.5 }
theme olx-theme-dirt: 5 holes, 8 stones, 8 misc
skins: 409
```

- [ ] **Step 3: Create `docs/porting/STATUS.md`**

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
| Simulation | `src/common/PhysicsLX56*.cpp` | — | not started (plan 3) | — |

## Deliberate deviations

- Importers reject malformed input that OLX would read as garbage: truncated
  files, unknown weapon or projectile types, and nesting deeper than 128 levels.
- A missing projectile image is an import error. OLX would load the mod and draw nothing.
- Imported level art is stored at 1×, because OLX's 2× surfaces are plain pixel doubling.

## Kept OLX quirks

- `ROF /= 1000` and `Recharge /= 10` are applied after loading (`CGameScript.cpp:776-777`).
- Beam and pixel colors stored as ints are truncated to bytes, as with OLX's `Color(Uint8, …)`.
- Front pixels where the `PX_EMPTY` flag is set become the back pixel, even when other flags are set.
```

- [ ] **Step 4: Run**

```bash
cargo xtask check
```

Expected: output contains `check passed`. Every step prints `ok`, including `olx-content` (first run: about 10 s to clone and import) and `test` (all crates, with real content).

- [ ] **Step 5: Run**

```bash
cargo run -q -p rl-cli -- validate target/content
```

Expected: output contains `validated 416 packs: 0 errors`. Manual check: 416 packs (2 mods, 4 levels, 1 theme, 409 skins).

- [ ] **Step 6: Run**

```bash
cargo xtask api && head -5 target/api/rl_pack.txt
```

Expected: output contains `Public API of rl_pack`. API summaries generate.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Add corpus test and porting status"
```
