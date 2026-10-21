# olr-mod-editor

The mod editor (issue #17).

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.**
- **Field table** (`fields.ron`, `fields.rs`): written by `cargo xtask
  format-docs` (keys, types, the format's doc comments as hover text);
  the unit, soft range, place (Common, More, Panel) and asset folder are
  edited by hand and kept. `check` fails when it drifts from the format.

Test: `cargo nextest run -p olr-mod-editor`.
