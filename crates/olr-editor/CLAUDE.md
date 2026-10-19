# olr-editor

The level editor (plan 14b; spec
`docs/superpowers/specs/2026-10-17-editor-foundation-and-level-editor-design.md`
§4-§5; decisions in `docs/porting/STATUS.md`, "Level editor decisions"):
its document, undo history, tools and egui panels, as pure state a host
drives. `olr-game` hosts it (plan 14c: `olr-game/src/editor/`).

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.** Depends on `olr-data`,
  `olr-vfs`, `olr-format`, `olr-pack`, `olr-edit`, `olr-look` (`ViewMode`), egui 0.36.2, serde and
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
- **Plan 14d** (STATUS.md, "Level editor decisions (plan 14d)"): edits
  `Pixels` (a `Patch` of art, cells and back art over a rectangle of the
  layer's box), `Layer` (a whole item swapped, checked to fit its element)
  and `Mask`. `begin_stroke(id, Stroke)` makes the conversion a stroke
  needs (plain to mixed, a picture's first mask) and the pointer records
  it with the stroke as one step, or undoes it when the stroke changed
  nothing; `stroke_before`/`stroke_edit` (`StrokeBefore`); `erase` masks a
  picture (`restore`), `remove_colour`, `merge_down`, `split` (any kind);
  the live check counts masks (`check_extra`). Tools: the Wand (W, Remove
  colour, `tolerance`), "Paint as" (`paint_material`), Restore; Select
  moves a drawn layer only once selected (R9); Ctrl+E merges down. The
  toolbar has three rows (tools and the tool's options in a reserved
  slot, brush and paint, session). `mask_dab_speed` (olr-edit): about 3 ms
  a dab in release; `layer_move_speed`: 7-9 ms a move.
- **Plan 14e** (STATUS.md, "Level editor decisions (plan 14e)"):
  - **Paint**: `PaintChoice::Texture(TextureRef)` (`PaintChoice` is
    `Clone`, `state.paint()` a reference); the state's built-in textures
    (`builtin_textures`) and the user's (`Catalog::textures`,
    `TextureEntry`; `TexturesChanged` replaces them and bumps
    `textures_generation`); a missing texture paints the default colour.
    `Browse(Purpose::ImportTexture)`'s pick sends `AddTexture(path)`;
    `RemoveTexture(name)` asks the host.
  - **Tools**: Fill (G; `Document::fill`, its tolerance `fill_tolerance`,
    `Tolerance` setting the chosen tool's), crater brushes
    (`BrushShape::Crater(i)`, `ThemeEntry::holes`, `state.holes()` for the
    level's theme; joined by dabs about every half crater), Select pixels (M; the
    `pixel_rect`, dropped on a tool change, `DropPixels` (Escape), Open and
    Close).
  - **Palette** (`ui/toolbar.rs`): a Crater picker after Square (hidden
    without holes), "Textures…" showing the current paint's swatch, its
    popup grouped "Theme" (`THEME_FRONT`, `THEME_BACK`), "Built-in" and
    "Yours" (Remove for the chosen one of yours), "Import texture…";
    swatches cached in `EditorData` by key, dropped when no longer listed.
  - **Multi-select**: `Selection::Many` (stack order; `group` normalises,
    `has`, `ids`), `SelectToggle`/`SelectRange` (the list's Ctrl and Shift
    clicks), `Pointer { toggle }` (Ctrl on the canvas), `Drag::MoveMany`,
    `SetMany`; `Document::set_many`, `move_many`, `remove_many` (one step
    each, a locked member refusing, named); the group panel. The
    properties panel keeps 260 points at least.
  - **Clipboard** (`clipboard.rs`): `Clipboard::Elements` (`Document::copy`,
    drawn layers cropped; `paste`: new ids and names, resampled across art
    scales, originals never overwritten) and `Clipboard::Pixels`
    (`copy_pixels`, `paste_pixels`: a new layer "Pasted"), kept across
    documents; `Copy`, `Cut`, `Paste` from Ctrl+C/X/V or egui's copy, cut
    and paste events, each paste (4, 4) further.
- **Plan 15a, the look** (spec `docs/superpowers/specs/2026-10-18-look-system-design.md`
  §The level editor; the canvas is lit by the host, `olr-game`'s
  `editor/canvas.rs`):
  - **Document** (`look_ops.rs`, Task 17): `set_glow` (a glow naming no
    group of the look gets a new one named after the element, made
    unique; `None` removes the glow and its mask), `set_property` (fits
    the glow mask to a new box size, nearest, in the same step),
    `set_look` (elements naming a group gone lose their glow),
    `add_group`, `set_group`, `remove_group` (its elements unglow, one
    step; a group or look used by a locked element is refused,
    `DocError::Locked`), `add_light` (`new_light`), `set_light`,
    `paste_objects`; each checks with `compile_look` first. Edits
    `Edit::Look` (the `LookSource` before and after) and `Edit::GlowMask`
    (an element's whole mask swapped, `Arc`s, counted once by address in
    `MAX_BYTES`; `Edit::Whole` carries `masks` too). The document keeps
    `GlowMasks` (by element id, each its box at art resolution, 0-255;
    `glow_mask(id)`) and bakes with them (`bake_elements_with`); saving
    drops a mask flag whose mask does not fit. A plain level's glow and
    clear flags made outside the editor (`Kept`) stay as they are while no
    element glows or is clear, their groups following the look by id, with
    a diagnostic saying so (`a_plain_level_keeps_its_glow_and_clear`); a
    resize, crop or art-scale change re-lays the masks (`relay_mask`,
    `resample_mask`), and a paste keeps them.
  - **Tools**: Glow (H) paints the selected element's mask
    (`glow_stroke_before`, `paint_glow`, `glow_stroke_edits`: one step
    with any group and glow the stroke made, or none when the mask did not
    change; an element without a glow gets one in a new group, dark but
    for what is painted; a glow without a mask starts whole) at the
    brush's strength (`GlowStrength`, 0 erases); Light (K) drags a light
    under the pointer, else places one. `Tool::DRAWING` is the toolbar's
    first row, `Tool::ALL` (13) every shortcut. Lights are level objects:
    `Overlay::Light` (a small sun; the selected one's radius + reach as a
    circle), listed as "Light i at (x, y)", their properties panel
    (place, colour, intensity, reach, radius, animation).
  - **Glow on selected pixels** (the user's playtest, 2026-10-19): Select
    pixels' "By colour" (`SelectByColour`) picks the connected pixels of
    the selected element alike in colour (`colour_pixels`, the tool's
    tolerance), Ctrl adding to the selection (`join_regions`); the
    selection keeps `pixel_region` (a flag per art pixel of `pixel_rect`,
    `None` for a rectangle). The Glow brush's "By colour"
    (`GlowByColour`) lights such pixels in one click, one step
    (`glow_by_colour`). "Glow selection" (`GlowSelection`) gives the
    selected pixels the brush's glow % (0 clears; `glow_pixels`; a layer
    without a glow gets one, dark elsewhere). Copy, cut and delete refuse
    a colour selection (`BY_COLOUR_TAKES_A_RECTANGLE`).
  - **Clock and view** (`ClockAction::{Play, Pause, Scrub}`,
    `SetViewMode`): `clock_ms`, `clock_running`, `advance_clock` (the
    host's frame time), `view_mode` (Lit, Full bright, Light only); none of
    them a step.
  - **Panels** (`ui/look.rs`, Task 18): the level panel's Look section
    (Add look / Remove look; ambient, max light, steps, softness, soak,
    solid floor, shadows, the grade; its glow groups with colour,
    intensity, reach and the animation editor, Remove asking first,
    `Dialog::RemoveGroup`); an image or drawn element's Glow (strength and
    group only; a locked element's fields hidden) and "Lets light through"
    (`Property::Clear`); a fourth toolbar row (`toolbar_row`: the Glow and
    Light tools, the brush's glow %, Glow selection, the views, Play/Pause
    lights and the clock's ms). Colours are `#rrggbb` text fields
    committed once (no colour picker: each drag change would be a step).
  - **Object clipboard** (the user's playtest, 2026-10-19):
    `Clipboard::Objects` (a selected spawn or light, `copy`; cut copies
    then removes), pasted at their places plus an offset that grows by
    (4, 4) each paste (`paste_objects`, one step, the last pasted
    selected; `objects_copy_cut_and_paste`).
- **Tests** (`tests/`, one binary): `document.rs`, `history.rs` (the
  undo proptest, the cap), `state.rs` (a synchronous test host in
  `common.rs`), `tools.rs` (the ignored `brush_dab_speed`: about 2 ms in
  release for a size-16 dab on Castle Strike), `look.rs` (plan 15a: glow,
  groups, lights, the clock, masks through resizes and pastes, the object
  clipboard, glow by colour) and `flows.rs` (the
  `egui_kittest` flows of spec §8; plan 15a's `glow_an_element_and_save`,
  `paint_glow_with_the_brush`, `glow_pixels_picked_by_colour`,
  `place_a_light_and_edit_it`, `edit_the_look_panel`,
  `toggle_views_and_scrub_the_clock`). The content tests skip without
  `OLR_CONTENT_DIR`.

Test: `cargo nextest run -p olr-editor`.
