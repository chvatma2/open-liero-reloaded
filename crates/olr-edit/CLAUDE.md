# olr-edit

The pack authoring operations `olr-cli` and the editors share (plan 13d):
loading and saving a pack's authoring source.

- **Layer 3, `std`.** Depends on `olr-data`, `olr-vfs`, `olr-format`,
  `olr-pack`, `thiserror`.

Test: `cargo nextest run -p olr-edit`.
