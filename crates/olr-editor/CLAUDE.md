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
  `Play`, `SaveSettings`, `List`, `ReadImage`, `RecoveryDone`, `Close`;
  the libraries' `AddTexture`, `RemoveTexture`, `ReadAsset`, `SaveAsset`,
  `ImportAsset`, `RemoveAsset`)
  and answers (`Opened`, `Recovered`: as `Opened` but unsaved, `OpenFailed`,
  `Saved`, `PlayEnded`, `Listed`, `ImportImage`, `ImportFailed`,
  `TexturesChanged`, `AssetsChanged`, `AssetSaved`, `AssetRead`; and
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
  browser's `browse_dir`, plan 14c; whether the properties' "More" and the
  lighting's "Fine tuning" are open, issue #8).
- **File browser** (`browser.rs`, `ui/browser.rs`, plan 14c R9): `Browse(
  Purpose::{ImportImage, OpenPack, ImportTexture, ImportAsset})` opens it at `browse_dir`, else
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
  first), for `ImportAsset` of an asset folder (`DirEntry::is_asset`: it
  holds `asset.ron`) sends `ImportAsset(path)`, of another folder enters it; a pick remembers its folder
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
  when the bake changed or the level has a look or a light (drawn lit, the
  user's note 2026-10-19, `a_look_shows_in_the_saved_thumbnail`); a layer file whose pixels did not change is
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
- **Panels** (`ui/`; issue #8's layout, STATUS.md "Level editor layout
  (issue #8)"): on top the session bar (File ▾: Open…, Save, Close; Undo,
  Redo, the unsaved mark's kept space; at the right View ▾: Material
  view, Grid, the light views and the lights' clock; Play and its ▾, the
  mod and bots) and the tool options bar (`toolbar.rs`: the chosen tool's
  options only: the brush for `Tool::uses_brush`, the paint for
  `Tool::lays_paint`, then its own; empty but as tall without any); on
  the left the tool rail (`rail.rs`, icons drawn in `icons.rs`): Import,
  Select, Draw ▸, Pixels ▸ (`ToolGroup`: a click chooses the group's
  last tool, `EditorState::last_tool`; a second click, the corner mark or
  a right click opens the flyout), Glow (named "Glow brush"), Spawn; on
  the right one panel (at least 260 points): the tabs Elements (drag a
  name to reorder, eye and lock, Add layer) | Assets (issue #9, below) |
  Spawns (`PanelTab`, following the selection), then the selection's
  properties (Name, "What is it?", Glow with "Make parts glow…", Place;
  the rest under "More") or the level's (its Lighting's rarer values
  under "Fine tuning"); status bar; canvas (a frame without fill,
  `egui::Frame::NONE`: the host draws the bake under it; its AccessKit
  name is `labels::CANVAS`; a press closes any popup), dialogs and the
  browser. Controls never move as others change width (pickers in fixed
  slots, the brush in one place for every tool,
  `the_toolbar_stays_in_the_window_and_in_place`, which also checks
  1024 points under the game's theme), and every control is as tall as a
  button, so each bar is one straight line under the host's theme
  (`a_toolbar_row_is_one_straight_line`). Shortcuts work when no text
  field has the keyboard or a widget is dragged; Escape with a popup open
  only closes it. Widgets are found by `ui::labels` (`relabel` names a
  widget whatever it shows: "File ▾" is `FILE`); a
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
  moves a drawn layer only once selected (R9); Ctrl+E merges down. `mask_dab_speed` (olr-edit): about 3 ms
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
  - **Palette** (`ui/toolbar.rs`, the options bar): a Crater picker after Square (hidden
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
    `DocError::Locked`), `paste_objects`; each checks with `compile_look` first. Edits
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
    brush's strength (`GlowStrength`, 0 erases). `Tool::ALL` (12) is every
    shortcut; the rail groups them (issue #8). Plan 15c removed
    the Light tool (K) and light objects: glowing pixels are a level's
    light (`the_editor_has_no_light_tool`).
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
  - **Panels** (`ui/look.rs`, Task 18; issue #8): the level panel's
    Lighting section (Add look / Remove look; ambient; its glow groups
    with colour, intensity, reach and the animation editor, Remove asking
    first, `Dialog::RemoveGroup`; max light, steps, softness, soak, solid
    floor and the grade under "Fine tuning"); an image or drawn element's
    Glow (strength and group only, a locked element's fields hidden;
    "Make parts glow…") and "Lets light through" (`Property::Clear`,
    under "More"); the Glow brush's glow % and Select pixels' Glow
    selection in the options bar, the views and the clock in View ▾.
    Colours are `#rrggbb` text fields committed once (no colour picker:
    each drag change would be a step).
  - **Object clipboard** (the user's playtest, 2026-10-19):
    `Clipboard::Objects` (a selected spawn, `copy`; cut copies
    then removes), pasted at their places plus an offset that grows by
    (4, 4) each paste (`paste_objects`, one step, the last pasted
    selected; `objects_copy_cut_and_paste`).
- **Asset library** (issue #9; STATUS.md "Level editor asset library
  (issue #9)"; the format and operations are `olr_format::assets` and
  `olr_edit::asset`):
  - **Catalog**: `Catalog::assets` (`AssetEntry`: `AssetKey { builtin,
    id }` (the folder), name, size, tags, glows, thumbnail; built-in first,
    then the user's); `AssetsChanged` replaces them and bumps
    `assets_generation` (the tab's thumbnail textures, cached in
    `EditorData` by key, follow it).
  - **Save as asset** (`SaveAsAsset`, the button at the bottom of an
    element's or a group's properties): with a pixel rectangle,
    `Document::asset_of_rect` of `pixel_world` (the dragged rectangle
    within the level, not clipped to its layer as `pixel_rect` is: every
    visible layer is cut); else `asset_of_elements` of the selection; a
    colour selection is refused. It waits in `pending_asset` while
    `Dialog::AssetName` asks its name (`NameAsset`: 1 to 64 characters,
    else asked again); a name one of the user's assets has asks
    `Dialog::AssetExists` (Replace: `Choice::Confirm`, over its folder;
    Keep both: `Choice::KeepBoth`, renamed `copy_name`); then
    `SaveAsset { asset, replace }`. The host answers `AssetsChanged` and
    `AssetSaved(key)`, which selects it and shows the Assets tab.
  - **Placing** (`asset_ops.rs`): a thumbnail clicked is picked
    (`PickAsset`, a second click unpicks) and the next press on the canvas
    places it there (`placing()`; Escape, `CancelPlacing`, or a tool
    change stops); one dragged onto the canvas (egui's drag and drop, an
    `AssetKey` payload) lands where let go; both outline its size at the
    pointer. `PlaceAsset { key, at }` asks `ReadAsset`; `AssetRead`
    places it with `Document::place_asset`: one step, its glow groups
    first (`olr_edit::asset::with_groups`: joined by id, the level's
    settings kept; missing ones added with the asset's; a level without a
    look gets `LookSource::default()` and the status bar says
    `ADDED_LIGHTING`), then each piece on top as a paste lands it (`land`,
    shared with `paste_one`: new ids and names, the asset's name for one
    piece, "<asset> <piece>" for several, resampled to the level's art
    scale, masks kept); the new elements are selected. A placed asset is
    ordinary elements.
  - **The tab** (`ui/elements.rs`): thumbnails grouped `BUILT_IN` and
    `YOURS`, each button named by the asset (`relabel`), its name and tag
    under it ("rock, background, glows"); Import asset… (`Browse(
    Purpose::ImportAsset)`); Remove asset for a picked one of yours,
    asking first (`Dialog::RemoveAsset`, then `RemoveAsset(id)`); a
    built-in one is never removed.
- **Tests** (`tests/`, one binary): `asset.rs` (issue #9: save, the name
  clash, the rectangle unclipped and hidden layers left out, placing as one
  step with lighting added, join and add, Remove, Import, and the
  `egui_kittest` flow `save_and_place_through_the_panels`; the test host
  keeps a library, `common::set_library`), `document.rs`, `history.rs` (the
  undo proptest, the cap), `state.rs` (a synchronous test host in
  `common.rs`), `tools.rs` (the ignored `brush_dab_speed`: about 2 ms in
  release for a size-16 dab on Castle Strike), `look.rs` (plan 15a: glow,
  groups, the clock, masks through resizes and pastes, the object
  clipboard, glow by colour) and `flows.rs` (the
  `egui_kittest` flows of spec §8; plan 15a's `glow_an_element_and_save`,
  `paint_glow_with_the_brush`, `glow_pixels_picked_by_colour`,
  `edit_the_look_panel`,
  `toggle_views_and_scrub_the_clock`; issue #8's
  `choose_a_tool_from_a_flyout`, `a_group_button_brings_back_its_last_tool`,
  `the_rail_highlights_the_chosen_tool`, `the_tool_options_follow_the_tool`,
  `more_and_fine_tuning_fold_and_are_remembered`, `make_parts_glow`,
  `the_tab_follows_the_selection`). The content tests skip without
  `OLR_CONTENT_DIR`.

Test: `cargo nextest run -p olr-editor`.
