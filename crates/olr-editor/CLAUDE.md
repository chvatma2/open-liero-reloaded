# olr-editor

The level editor (plan 14b; spec
`docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md`
§4-§5): its document, undo history, tools and egui panels, as pure state a
host drives. Plan 14c hosts it in `olr-game`.

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.** Depends on `olr-data`,
  `olr-vfs`, `olr-format`, `olr-pack`, `olr-edit`, egui 0.36.2, serde and
  thiserror; not on `olr-ui`, `olr-render` or `olr-present`. Not wasm.
- **Catalog** (`catalog.rs`): the installed themes (with their tiles) and
  mods the host passes in; `EditorSettings` (Play's mod and bots, ruling
  R9) the host stores.
- **Document** (`document.rs`, ruling R2): the open level pack, each
  element decoded (`olr_edit::compose::Decoded`) through one
  `ImageCache`. `open` composes a plain level (a raster `base`) and
  rebakes; `to_pack` encodes the composition and keeps the bake,
  refreshing the thumbnail only when something was rebaked.
  `diagnostics` are ruling R6's cheap checks.

Test: `cargo nextest run -p olr-editor`.
