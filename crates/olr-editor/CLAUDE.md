# olr-editor

The level editor (plan 14b; spec
`docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md`
§4-§5; decisions in `docs/porting/STATUS.md`, "Level editor decisions"):
its document, undo history, tools and egui panels, as pure state a host
drives. Plan 14c hosts it in `olr-game`.

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.** Depends on `olr-data`,
  `olr-vfs`, `olr-format`, `olr-pack`, `olr-edit`, egui 0.36.2, serde and
  thiserror; not on `olr-ui`, `olr-render` or `olr-present` (the host
  applies `olr_ui::theme`). Not wasm. Tests use `egui_kittest` (`wgpu`,
  `snapshot`), `tempfile` and `proptest`.
- **The pattern** (`olr-ui`'s): `EditorState::update(EditorAction) ->
  Vec<EditorCommand>` is pure; `ui::show(ui, &state, &mut EditorData)`
  draws egui and returns actions. The host carries out the commands
  (`Upload` a world rectangle of the bake, `Open`, `Create`, `Save`,
  `Play`, `SaveSettings`, `Close`) and answers (`Opened`, `OpenFailed`,
  `Saved`, `PlayEnded`). While playing, every action but `PlayEnded` is
  ignored. Open, New and Close ask first when there are unsaved changes.
- **Catalog** (`catalog.rs`): the installed themes (with their tiles) and
  mods the host passes in; `EditorSettings` (Play's mod and bots, R9).
- **Document** (`document.rs`, R2): the open level pack, each element
  decoded (`olr_edit::compose::Decoded`) through one `ImageCache` (R3).
  `open` composes a plain level (a raster `base`) and rebakes; `to_pack`
  encodes the composition and keeps the bake, refreshing the thumbnail
  only when the bake changed; `diagnostics` are R6's cheap checks.
- **Edits** (`ops.rs`, `history.rs`): every operation checks first, then
  builds an `Edit` and runs it through `Document::apply`, the one path
  undo and redo take (a refusal changes nothing). Painting is recorded per
  stroke (`stroke_edit`). `History` holds steps within `MAX_BYTES`
  (256 MiB), the oldest dropped first, a larger step kept alone. Originals
  stay in the document by source name (new names never reuse one), and
  leave the pack when no element names them.
- **Tools** (`tools.rs`, `pointer.rs`, `view.rs`): pointer actions are in
  world pixels (R7); `View` maps screen points (`ZOOMS`, pixel-snapped at
  integer zoom). One stroke or drag is one step; an image move is live, a
  resize previews and applies on release (R8); any action that can change
  the document ends a drag first. Locks refuse content changes, not order
  (R4). `overlays()` is what the canvas draws over the bake.
- **Panels** (`ui/`): toolbar, element list (drag a name to reorder),
  properties or level settings, status bar, canvas (an empty frame in 14b
  under the overlays), dialogs; shortcuts when no text field has the
  keyboard. Widgets are found by `ui::labels`; a `DragValue` or text field
  commits once, when a drag stops or typing ends (`EditorData` drafts).
- **Tests** (`tests/`, one binary): `document.rs`, `history.rs` (the
  undo proptest, the cap), `state.rs` (a synchronous test host in
  `common.rs`), `tools.rs` (the ignored `brush_dab_speed`: about 2 ms in
  release for a size-16 dab on Castle Strike) and `flows.rs` (the
  `egui_kittest` flows of spec §8). The content tests skip without
  `OLR_CONTENT_DIR`.

Test: `cargo nextest run -p olr-editor`.
