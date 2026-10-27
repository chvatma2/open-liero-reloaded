# olr-script

Asset scripts (the asset-based levels design, call 19; spec
`docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`): an
asset's default script is Rhai text (`script.rhai`). Issue #80 checks its
syntax for the asset tab; nothing runs a script yet.

- **Layer 3, `#![no_std]` + `alloc`, `#![forbid(unsafe_code)]`.** Builds for
  `wasm32-unknown-unknown` (`xtask/layers.ron`'s `no_std` and `wasm`).
  Depends on `rhai` only; its caller in issue #80 is `olr-editor`.
- **Rhai's pin:** `rhai = "=1.27.0"`, default features off, with
  `no_std`, `sync`, `no_float`, `only_i64`, `no_module`,
  `no_custom_syntax`, `no_time`, `no_closure` and `internals` (the
  workspace's `Cargo.toml`): #88's pin and features exactly (#88 needs
  `internals` for the per-run operation count), so the editor never parses
  a script the simulation would read differently. It pulls in `ahash`
  (compile-time-rng: `const-random`), `bitflags`, `hashbrown`,
  `no-std-compat`, `num-traits`, `once_cell`, `rhai_codegen`,
  `smallvec`, `smartstring` and `thin-vec`, all in the offline registry.
- **`engine()`**: `Engine::new_raw` (no packages), no optimisation (a
  compile evaluates nothing), `MAX_EXPR_DEPTH` 32 and
  `MAX_FUNCTION_EXPR_DEPTH` 16 (Rhai's debug-build defaults, so parsing
  never runs out of stack in either build), `MAX_STRING_BYTES` 4096.
- **`check(text) -> Vec<ScriptIssue>`** (`ScriptIssue { line, column,
  message }`, shown `line:column: message`): compiles to an AST and runs
  nothing; at most one issue (Rhai's `compile` stops at the first parse
  error), or one for text over `MAX_SCRIPT_BYTES` (64 KiB,
  `olr_format::assets::MAX_SCRIPT_BYTES`: `olr-editor`'s tests check
  they agree). Syntax only: which functions exist is #88's API.
- **Later:** #88 adds the runtime here (and the dependency on `olr-sim`,
  with the `same_layer` pairs `("olr-script", "olr-sim")` and
  `("olr-edit", "olr-script")`), and #89 the name checks beside this
  syntax-only `check` (`ScriptNames`, `check_level`).

Tests (`tests/check.rs`): `a_good_script_has_no_issues`,
`syntax_errors_name_line_and_column`,
`hostile_scripts_are_cheap_and_never_panic` (deep nesting, a huge string,
`MAX_SCRIPT_BYTES` of `(`: each one issue within 2 seconds).

Test: `cargo nextest run -p olr-script`.
