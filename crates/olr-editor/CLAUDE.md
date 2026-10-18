# olr-editor

The level editor (plan 14b; spec
`docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md`
§4-§5; decisions in `docs/porting/STATUS.md`, "Level editor decisions"):
its document, undo history, tools and egui panels, as pure state a host
drives. `olr-game` hosts it (plan 14c: `olr-game/src/editor/`).

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.** Depends on `olr-data`,
  `olr-vfs`, `olr-format`, `olr-pack`, `olr-edit`, egui 0.36.2, serde and
  thiserror; not on `olr-ui`, `olr-render` or `olr-present` (the host
  applies `olr_ui::theme`). Not wasm. Tests use `egui_kittest` (`wgpu`,
  `snapshot`), `tempfile` and `proptest`.
- **The pattern** (`olr-ui`'s): `EditorState::update(EditorAction) ->
  Vec<EditorCommand>` is pure; `ui::show(ui, &state, &mut EditorData)`
  draws egui and returns actions. The host carries out the commands
  (`Upload` a world rectangle of the bake, `Open`, `Create`, `Save`,
  `Play`, `SaveSettings`, `List`, `ReadImage`, `RecoveryDone`, `Close`)
  and answers (`Opened`, `Recovered`: as `Opened` but unsaved, `OpenFailed`,
  `Saved`, `PlayEnded`, `Listed`, `ImportImage`, `ImportFailed`; and
  `Failed(why)`, an error dialog, `Note(text)`, the status bar's message,
  for what the host could not do: a Play that did not start, a recovery
  copy not written).
  `RecoveryDone` comes with every successful save and before a Close, Open
  or New leaves the open document (the host drops its recovery copy).
  While playing, every action but `PlayEnded` is ignored; while a dialog is open only its answer, the host's answers and
  the view act. Open, New and Close ask first when there are unsaved
  changes. Saves are answered in order (`saves`: each one's follow-up and
  the change count it was sent at), so an edit made meanwhile stays unsaved.
- **Catalog** (`catalog.rs`): the installed themes (with their tiles) and
  mods the host passes in; `EditorSettings` (Play's mod and bots, R9; the
  browser's `browse_dir`, plan 14c).
- **File browser** (`browser.rs`, `ui/browser.rs`, plan 14c R9): `Browse(
  Purpose::{ImportImage, OpenPack})` opens it at `browse_dir`, else
  `Catalog::home` (the host's home directory; else `.`), and asks
  `List(dir)`; `Listed` sorts (folders first,
  then by name case aside, then exactly) and `shown()` filters (no hidden
  entries; folders, and for an import `.png`, `.jpg`, `.jpeg`); a failed
  listing keeps the old one and sets `message()`, and so does one of more
  than `MAX_ENTRIES` (the host cut it after one more, in `sort_entries`'
  order; the first `MAX_ENTRIES` show). `BrowseTo` (a
  typed path, relative to the browser's, `.` and `..` resolved by name)
  lists; `Pick` of a file reads it
  (`ReadImage`), of a pack folder opens it as `Open` does (the dialog
  first), of another folder enters it; a pick remembers its folder
  (`SaveSettings`) and closes the browser. For an import, a click on an
  image asks for its preview (`PreviewImage` → `ReadPreview`, answered by
  `Previewed`: the host's small copy and the image's size; kept for the
  file asked for last, dropped by a new listing), shown beside the list
  (`Browser::preview`, at most 200 points, a small image enlarged with
  sharp pixels, its size under it, or why there is none). The window is modal: the Path
  field (Enter browses), Up, the entries (a click selects, a double click
  picks; rows keyed by directory and name), Choose, Cancel; Escape or the
  backdrop closes it.
- **Document** (`document.rs`, R2): the open level pack, each element
  decoded (`olr_edit::compose::Decoded`) through one `ImageCache` (R3).
  `open` gives a plain level its three drawn layers (`olr_edit::compose::
  split_from`: `background`, `dirt`, `rock`, which bake back to its files
  byte for byte, the playtest's request: a loaded map can be erased and
  painted) and rebakes; `split(id)` turns a raster element into the same
  three layers in its place (`EditorAction::Split`, the properties' "Split
  into layers": one step); `to_pack`
  (or a `snapshot()`'s, which a thread may make: `PackSnapshot`) encodes
  the composition and keeps the bake, refreshing the thumbnail
  only when the bake changed; a layer file whose pixels did not change is
  saved as opened (another program's encoding kept); stray files in
  `editor/` are never overwritten by a new element's (ids and names skip
  them); `diagnostics` are R6's cheap checks.
- **Edits** (`ops.rs`, `history.rs`): every operation checks first, then
  builds an `Edit` and runs it through `Document::apply`, the one path
  undo and redo take (a refusal changes nothing; an edit of no change,
  `Edit::is_noop`, is no step). Painting is recorded per stroke
  (`stroke_edit`, `None` for a stale copy). `History` holds steps within
  `MAX_BYTES` (256 MiB, a running total counting each shared image grid
  once), the oldest dropped first, a larger step kept alone; `undo` and
  `redo` lend the step rather than copy it. Originals
  stay in the document by source name (new names never reuse one), and
  leave the pack when no element names them.
- **Tools** (`tools.rs`, `pointer.rs`, `view.rs`): pointer actions are in
  world pixels (R7); `View` maps screen points (`ZOOMS`, pixel-snapped at
  integer zoom). One stroke or drag is one step; an image move is live, a
  resize previews and applies on release (R8); any action that can change
  the document ends a drag first. Locks refuse content changes, not order
  (R4). `overlays()` is what the canvas draws over the bake; the grid is
  drawn at every zoom, a line every `grid_step` world pixels (1, or the
  smallest power of two at least 8 points apart).
- **Panels** (`ui/`): toolbar in two rows that never move (drawing: tools,
  brush, paint; then history, Open…, Save, Close, the unsaved mark's kept
  space, Play with a fixed-width mod picker and the bots, the view; the
  playtest's controls past the window's edge, `the_toolbar_stays_in_the_window_and_in_place`;
  every control as tall as a button, so each row is one straight line
  under the host's theme, `a_toolbar_row_is_one_straight_line`), element
  list (drag a name to reorder; Import image… opens the browser),
  properties or level settings, status bar, canvas (a frame without fill,
  `egui::Frame::NONE`: the host draws the bake under it; its AccessKit
  name is `labels::CANVAS`), dialogs and the browser; shortcuts when no text field has the
  keyboard or a widget is dragged. Widgets are found by `ui::labels`; a
  `DragValue` or text field commits once, when a drag stops or typing ends
  (`EditorData` drafts; Escape drops a text draft), the properties'
  commits before the frame's other actions. Escape or the backdrop cancels
  a dialog; each wheel event zooms one step; the resize fields follow the
  level's size.
- **Tests** (`tests/`, one binary): `document.rs`, `history.rs` (the
  undo proptest, the cap), `state.rs` (a synchronous test host in
  `common.rs`), `tools.rs` (the ignored `brush_dab_speed`: about 2 ms in
  release for a size-16 dab on Castle Strike) and `flows.rs` (the
  `egui_kittest` flows of spec §8). The content tests skip without
  `OLR_CONTENT_DIR`.

Test: `cargo nextest run -p olr-editor`.
