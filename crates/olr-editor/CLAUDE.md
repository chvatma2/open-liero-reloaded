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
  `snapshot`), `tempfile`, `proptest` and (issue #78) `olr-ui`, a
  dev-dependency only, as the skin editor's: the fit and flow tests run
  under `olr_ui::theme` itself rather than a hand-copied theme. It is
  allowed though `olr-ui` is a higher layer: `cargo xtask layers` checks
  no dev-dependency (tests are not shipped), and `olr-ui` does not depend
  on this crate.
- **The pattern** (`olr-ui`'s): `EditorState::update(EditorAction) ->
  Vec<EditorCommand>` is pure; `ui::show(ui, &state, &mut EditorData)`
  draws egui and returns actions. The host carries out the commands
  (`Upload` a world rectangle of the bake, `Open`, `Create`, `Save`,
  `Play`, `SaveSettings`, `List`, `ReadImage`, `RecoveryDone`, `Close`;
  the libraries' `AddTexture`, `RemoveTexture`, `ReadAsset`, `SaveAsset`,
  `ImportAsset`, `RemoveAsset`; issue #11's `Distant`; issue #77's
  `ReadAssetToEdit`; issue #83's `CompareLibrary`; issue #84's
  `ExportLevel { pack_dir, to }` and `ImportLevel(path)`)
  and answers (`Opened`, `Recovered`: as `Opened` but unsaved, `OpenFailed`,
  `Saved`, `PlayEnded`, `Listed`, `ImportImage`, `ImportFailed`,
  `TexturesChanged`, `AssetsChanged`, `AssetSaved` (the key and, issue
  #83, the saved source), `AssetRead`, `AssetToEdit`, `LibraryCompared`,
  `LevelImported { dir, report }` (issue #84); and
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
  mods the host passes in, and the surface material library (issue #43,
  `materials`: `olr_edit::material::MaterialEntry`, built-in first); `EditorSettings` (Play's mod and bots, R9; the
  browser's `browse_dir`, plan 14c; whether the properties' "More" and the
  lighting's "Fine tuning" are open, issue #8).
- **File browser** (`browser.rs`, `ui/browser.rs`, plan 14c R9): `Browse(
  Purpose::{ImportImage, OpenPack, ImportTexture, ImportAsset,
  ExportLevel, ImportLevel})` opens it at `browse_dir`, else
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
  them); `diagnostics` are R6's cheap checks. Issue #74: the items are
  the ground's layers; issue #82 opens a level placing other assets
  whole (below, "The level tab (issue #82)"); `open` converts an OLR 0.2 level
  (`olr_edit::placed::from_composition`; `converted()`): `opened` then
  leaves it unsaved with the status bar's `CONVERTED` ("Converted from
  OLR 0.2: save to keep it"), and saving writes the placement
  (`placed.ron`, the ground's `asset.ron`) without `composition.ron`.
  The document keeps the placement and the other assets' files as
  opened (`placed`, `verbatim`) for `to_pack`. Tests: `document.rs`'
  `opening_a_02_level_converts_it_unsaved`, `level_tab.rs`'
  `a_placed_level_opens_whole`.
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
  (issue #8)"); #78 and #83 lay both tabs out anew, below): on top the session bar (File ▾: Open…, Save, Close; Undo,
  Redo, the unsaved mark's kept space; at the right View ▾: Collision
  view (rock, dirt, background; "Material view" before issue #43), Grid,
  Preview parallax (issue #66), the light views, the Materials view and
  the lights' clock; Play and its ▾, the mod and bots) and the tool options bar (`toolbar.rs`: the chosen tool's
  options only: the brush for `Tool::uses_brush`, the paint for
  `Tool::lays_paint`, then its own; empty but as tall without any); on
  the left the tool rail (`rail.rs`, icons drawn in `icons.rs`; issue
  #81: the level tab's is Select, the Haze brush and Spawn, the rest the
  asset tab's): Import,
  Select, Draw ▸, Pixels ▸ (`ToolGroup`: a click chooses the group's
  last tool, `EditorState::last_tool`; a second click, the corner mark or
  a right click opens the flyout), Glow (named "Glow brush"), Surface
  (named "Surface brush", issue #43), Spawn; on
  the right one panel (at least 260 points): the tabs Elements (issue
  #81: `COPIES` in the level tab, the ground's row; the asset tab's
  folder tree, issue #61: drag a name to reorder, eye and lock, Add
  layer, New folder) | Assets (issue #9; gone since #83: the library is
  the level tab's left panel) | Spawns (`PanelTab`, following the selection), then the selection's
  properties (Name, "What is it?", Glow with "Make parts glow…", Place;
  the rest under "More") or the level's (its Lighting's rarer values
  under "Fine tuning"); status bar; canvas (a frame without fill,
  `egui::Frame::NONE`: the host draws the bake under it; its AccessKit
  name is `labels::CANVAS`; a press closes any popup), dialogs and the
  browser. Controls never move as others change width (pickers in fixed
  slots, the brush in one place for every tool,
  `the_toolbar_stays_in_the_window_and_in_place`, replaced by #83's
  `the_level_tab_fits` when the options bar went), and every control is as tall as a
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
    `has`, `ids`; folders too, issue #61), `SelectToggle`/`SelectRange` (the list's Ctrl and Shift
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
    drops a mask flag whose mask does not fit. A plain level's glow (with
    its glow beam maps, final review M1) and clear flags made outside the
    editor (`Kept`) stay as they are while no element glows or is clear,
    their groups following the look by id, with a diagnostic saying so
    (`a_plain_level_keeps_its_glow_and_clear`,
    `a_plain_level_keeps_its_glow_beams`); a
    resize, crop or art-scale change re-lays the masks (`relay_mask`,
    `resample_mask`), and a paste keeps them.
  - **Tools**: Glow (H) paints the selected element's mask
    (`glow_stroke_before`, `paint_glow`, `glow_stroke_edits`: one step
    with any group and glow the stroke made, or none when the mask did not
    change; an element without a glow gets one in a new group, dark but
    for what is painted; a glow without a mask starts whole) at the
    brush's strength (`GlowStrength`, 0 erases). `Tool::ALL` (14 with
    issue #43's Surface brush, U, and the Haze brush, F) is every shortcut; the rail groups them (issue #8). Plan 15c removed
    the Light tool (K) and light objects: glowing pixels are a level's
    light (`the_editor_has_no_light_tool`).
  - **Directional glow and haze** (directional-glow design): an element
    glow's "Directional" (rock or background, not distant; a dirt layer
    shows `DIRECTIONAL_DIRT_HINT`; off while its group's reach is
    unlimited) sets `ElementGlow::beam` (0 degrees, spread 35), then
    "Direction °" (0-359, in the beam map's 2° steps: `ui::number_by`, an
    odd degree commits as the even one below it and no change is no step;
    final review M3; the CLI and the format take whole degrees) and
    "Spread °" (1-90), a step each. The level's
    "Haze %" (`LookSource::haze`: 50 the factory's look, 100 twice) sits
    under the occlusion. The Haze brush (F; `haze_stroke_before`,
    `paint_haze`, `haze_stroke_edits`; strength `HazeStrength`, 0-100 maps
    to `round(s * 255 / 100)`) paints `LevelSource::haze`; the first stroke
    makes a map of 255 (ruling T6a). `Edit::Haze` holds the touched
    rectangle's bytes (and `created`, so undo drops the map): one step a
    stroke, none when nothing changed. Past `MAX_GLOW_BEAMS` (64) the
    renderer draws no beams and Save refuses, so `diagnostics` shows the
    Error "N glow beams, above 64: …" (final review I3:
    `olr_format::levels::glow_beam_count` of the bake;
    `diagnostics_count_the_glow_beams`).
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
    Lighting section (Add look / Remove look; ambient; the occlusion's
    strength and, when on, its reach right after the ambient colour (issue
    #10: "Occlusion %", "Occlusion reach"; 0 removes it); its glow groups
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
    `AssetSaved { key, source }`, which picks it in the library.
  - **Placing** (`asset_ops.rs`): a thumbnail clicked is picked
    (`PickAsset`, a second click unpicks) and the next press on the canvas
    places it there (`placing()`; Escape, `CancelPlacing`, or a tool
    change stops); one dragged onto the canvas (egui's drag and drop, an
    `AssetKey` payload) lands where let go; both outline its size at the
    pointer. `PlaceAsset { key, at }` (and the Place button's
    `PlaceAssetCentred`) asks `ReadAsset`; `AssetRead` (the asset's files)
    adds a copy of it (issue #82, `level_tab::place_copy`,
    `Document::add_copy`: one step, the copy selected; a level without a
    look gets one and the status bar says `ADDED_LIGHTING`). The asset tab
    places none (`PLACED_IN_THE_LEVEL`).
  - **The library** (`ui/library.rs`; the right panel's Assets tab
    until #83, below): Search, thumbnails grouped `BUILT_IN` and
    `YOURS`, each button named by the asset (`relabel`), its name and tag
    under it ("rock, background, glows"); Import asset… (`Browse(
    Purpose::ImportAsset)`); Remove asset for a picked one of yours,
    asking first (`Dialog::RemoveAsset`, then `RemoveAsset(id)`); a
    built-in one is never removed.
- **Distant layers** (issue #11; the format is `olr_format::levels`'
  `Element::distant`, the bake `olr_edit::compose`'s): an image or plain
  drawn Background element made distant (`Property::Distant`, through
  `Document::set_distant`, which `set_property` calls: one step, moving
  it to the top of the distant layers or, no longer distant, just above
  them). The order holds everywhere: `move_to` clamps a drag to the
  element's group and refuses a move clamping undoes (`DISTANT_ORDER`), a
  paste lands a distant copy on top of the distant layers (`landed`,
  `insert_landed`), `merge_down` and `split` refuse ("a distant layer does
  not merge|split"); assets never carry one. `Document::rebake`
  rebakes the whole level when the bake turns see-through or back
  (`see_through()`), makes the level's distant layers again when the
  distant items changed (`distant_generation()`), and counts the
  see-through back pixels (`has_holes()`). `EditorState::update` sends
  `EditorCommand::Distant(layers)` whenever the generation moved (and with
  every new document). The canvas's camera (`parallax_camera()`, which
  the host draws with too) is the level's centre; View ▾ Preview parallax
  the view's centre (`View::centre`; issue #66: `PreviewParallax`,
  `preview_parallax()`, a view option like Grid, never a step nor a
  setting, acting while a dialog is open). By default a distant layer
  so shows at its place whatever the pan and zoom; `parallax(e)` is
  where a distant element shows relative to its place, so `hit`, the
  Select handles and resize, the tools on a selected distant layer
  (`to_layer`) and `overlays()` work where it draws. The panels: a
  Distant switch and a Depth slider (`ui::slider`, "near" to "far", committed when the drag
  stops; `ops::DEFAULT_DEPTH` 50) right under "What is it?" for a
  Background element, the hint `labels::DISTANT_HINT` while nothing is
  see-through, and the list's 🌄 mark. Issue #62: a distant element
  shows the Glow controls as any (strength, group, Make parts glow…, the
  Glow brush where it shows, each with undo; the switch's hint
  `labels::DISTANT_GLOW_HINT` says its light falls from its reference
  placement); `rebake` makes the distant layers again when a distant
  element's glow mask changes too (`distant_masks`), so the lit canvas
  shows the sky's baked light and its glowing picture. Tests:
  `tests/distant.rs` (issue #62:
  `a_distant_layer_glows_and_lights_the_level`; issue #66:
  `zooming_leaves_a_distant_layer_in_place`; `flows.rs`'s
  `preview_parallax_in_the_view_menu`).
- **Background glass** (spec `docs/superpowers/specs/2026-10-25-background-glass-design.md`;
  the format is `Element::glass`, the bake `olr_edit::compose`'s): a
  Background image or drawn layer (plain or mixed) that is not distant
  shows a Glass switch under "What is it?" (`Property::Glass`, one step,
  `ops::DEFAULT_GLASS_OPACITY` 35; a raster refused) and, when on, the
  Opacity (1-99 %), Tint (0-100 %) and Frost (0-40 tenths, shown "0.0" to
  "4.0 px", `ui::slider_shown`) sliders, each committed when the drag stops
  (a locked element's hidden, as Glow's); `labels::GLASS_HINT` while the
  level has no distant layer, `GLASS_LOOK_HINT` under Tint and Frost
  while it has no lighting (they show only lit). A glow group's colour is
  one switch, Own, Tint or `labels::FILTERED` (`GlowColourSource::
  Filtered`: the art's hue at full strength times the group's colour, the
  light through tinted glass), a tint's colour carried between the last
  two. Tests: `flows.rs`'s `make_an_element_glass`,
  `a_group_casts_filtered`.
- **Surface materials** (issue #37; the bake is `olr_edit::compose`'s):
  the document keeps the painted surface maps as opened
  (its `surface_maps`, `PaintedSurfaces`) and bakes with them. A resize
  re-lays them with their layers (`Whole::surfaces`, the new area 0) and
  an image's new box size resamples them (`Edit::SurfaceMap`, in the same
  step); split and merge down drop surfaces as they drop glow; saving
  drops a map flag whose map no longer fits. Test: `tests/surface.rs`. Issue #38: Save as asset
  hands `olr_edit::asset` the elements' painted maps (`pieces`), so the
  asset carries the materials they name; a placed copy's materials are
  gathered with the level's (issue #82, `olr_edit::placed`). A paste
  lands no surfaces.
- **The level editor's materials** (issue #43; STATUS.md "Level editor
  materials (issue #43)"; spec
  `2026-10-22-materials-and-heat-design.md` §Editors):
  - **Document** (`surface_ops.rs`): `materials()`, `material_value(id)`
    (1 + its index), `surface_map(e, back)` (flagged and fitting: what
    bakes), `material_users(id)` (elements naming it or painting its
    value, by name), `add_material` (a copy; a level without a look gets
    the default one; its id new, at most `MAX_MATERIALS`) and
    `remove_material` (refused while used, naming the users), both through
    `set_look`. `look_change` keeps the painted maps naming the same
    materials by id (`surface_maps_change`: `Edit::SurfaceMap`s) and
    clears a surface naming a material gone (with an unglow, one
    `Edit::Element`; Remove look takes them all, one step); `Edit::Look`
    rebakes when the materials' ids change. `set` refuses a surface naming
    no material of the level; `with_property` takes `Surface` and
    `BackSurface` (back art only).
  - **The Surface brush** (`Tool::Surface`, U; `Drag::Surface`):
    `surface_stroke_before(id, back)` (refused for a locked or distant
    element, a back map without back art), `paint_surface` (world
    resolution, the brush's shapes through `glow::paint_mask` at scale 1;
    a map not flagged or not fitting starts at 0) and
    `surface_stroke_edits` (one step: the map, then the flag's
    `Edit::Element`; a map all 0 is none; no change, no step). The state's
    `surface_paint` (an id; `None` the element's own: 0), `surface_back`
    (only with back art; else the front).
  - **The Materials view** (`SurfaceView`, `surface_view()`; never with
    the collision view): the host tints the canvas (`surface_tint(v)`,
    `SURFACE_TINTS`, cycling 12); `ui/canvas.rs`'s legend (`labels::
    LEGEND`, `legend_tint(name)` swatches, "None" last).
  - **Panels**: the level panel's Materials after its Lighting
    (`look::level_materials`: each tint, name, "used by …" and Remove,
    disabled while used, `labels::remove_material(name)`; "Add from
    library" with Built-in and Yours, one the level has disabled); an
    element's Surface and, with back art and not distant, Back surface
    pickers under its Glow (`element_surfaces`); the Surface brush's
    options: the brush, its Material (`BRUSH_MATERIAL`, "Element's" or
    the level's) in a fixed slot, then Front and Back when the selected
    element has back art. Save as asset and Play needed no change.
- **Layer folders** (issue #60; STATUS.md "Layer folders"; the format
  and every tree computation are issue #59's `olr_edit::folders`, never
  duplicated here):
  - **Document**: `folders` beside `items`; one emptied by a removal or a
    flat move may stay listed (its undo finds it): `composition()`
    (saved, checked: `check_extra`, `diagnostics`) and `folders()` are
    tidied (`document::tidied`). `shown_at(i)`, `locked_at(i)`: the
    effective flags (`folder_view()`, the folders as a composition
    without elements). `rebake` bakes the folded `visible` when a folder
    is hidden (`baked_elements`) and compares the distant items so;
    `grid_key`, `image_grid` follow the element's own. Every lock check
    (`unlocked`, `none_locked`, `with_property`'s `locked`, `look_change`,
    the pointer) is effective; `hit` and `overlays()` skip or mark by the
    effective flags.
  - **Edits**: `Edit::Folders { before, after }` (the whole list; apply
    rebakes the members whose visibility changed); `Move { from, to,
    before, after }` carries the moved element's folder; an `Element`
    edit of its folder rebakes only when its visibility changes.
  - **Operations** (`folder_ops.rs`, each one step): `group(ids, name)`,
    `ungroup`, `move_into(id, into, position)` (through `restructure`:
    the folder list, the moves, each changed element), `set_folder(id,
    FolderField)` (a locked folder takes only `Locked`, `Collapsed`),
    `remove_folder` (its elements, refused while one is locked),
    `duplicate(id)` (directly above, same folder); `move_to` (the flat
    list) joins the innermost folder holding both new neighbours, or
    keeps one of its own at a folder's edge (`landing_folder`); `split`
    parts take the folder, `merge_down` keeps the lower's, `set_distant`
    leaves folders. `is_folder`, `folder_members`.
  - **Clipboard and assets**: `Clipboard::Elements { folders }`
    (`copy_tree`: a folder all of whose elements are copied comes with
    its subtree; `paste_tree`: new ids, names `copy_name`d when taken,
    the block on top outside the level's folders; `copy` is flat);
    `place_asset` groups two pieces or more into a folder named after
    the asset (`Placed::folder`); the pieces cut by `asset_of_*` carry
    the effective `visible`. Test: `tests/folders.rs`, and the history
    proptest's folder operations.
- **Folders in the editor's UI** (issue #61; STATUS.md "Layer folders",
  part 3):
  - **Selection**: `Selection::Folder(id)`; `Many` holds elements and
    folders (`EditorState::group`: stack order, a folder where its run
    starts, an id a selected folder holds left out); `has` and `ids`
    are the items themselves, `selected_elements()` (`elements_of`)
    their elements. The drag, the arrows, `SetMany` (folder ids
    expanded), Remove (`Document::remove_items`), Copy/Cut, Paste (its
    top-level items selected) and Save as asset act on a folder as one
    step; `AssetRead` selects a placed asset's folder.
  - **Document** (`folder_ops.rs`): `parent_of`, `folders_holding`
    (outermost first), `children`, `rows()` (the list's `Row`s, top
    first, a folded folder's members left out), `drop_row(id,
    RowDrop::{Above, Below, Into})` (through `move_into`), `remove_items`.
  - **Elements list** (`ui/elements.rs`): rows indented `INDENT` per
    level; a folder row's fold arrow, eye and lock (`labels::fold`,
    `folder_visible`, `folder_locked`, by its name; `SetFolder`), an eye
    or lock a parent overrides dimmed; a click selects, Ctrl toggles,
    Shift takes the rows shown (`SelectRange`). A dropped row takes the
    place of the row it lands on, in its folder (below it from above);
    a folder row's middle takes it in on top (`EDGE`: the top 30 %, and
    a folded one's bottom 30 %, drop beside); a line or outline shows
    where; a refusal is the status message. A level without folders
    reorders with `MoveElement` as before. `labels::NEW_FOLDER` beside
    Add layer.
  - **Canvas clicks** (`pointer.rs`, Select): `click_pick` (inside the
    `focus`, its child holding the hit; else the outermost folder, or
    the element; a click outside leaves the focus); a selected item on
    the hit's path stays and drags (a folder only once selected);
    `DoubleClick` goes one level in and focuses the folder; `AltDown`
    (Alt+press; a press for other tools) selects the element itself;
    Ctrl toggles at `click_pick`'s level; `SelectParent` (Escape after
    the popup, `CancelPlacing`, `DropPixels`) selects the parent folder,
    else none. A selected folder's overlay is one `Overlay::Box` around
    its shown members. Tools needing one element add
    `DOUBLE_CLICK_INTO` to their refusal (`one_element`).
  - **Shortcuts**: Ctrl+Shift+G (`Ungroup`) checked before Ctrl+G
    (`GroupSelection`), both before G (Fill); `labels::NEW_FOLDER_KEYS`,
    `UNGROUP_KEYS`. The folder panel: Name, Visible, Locked,
    `labels::UNGROUP`, its elements' shared fields.
  - Tests: `tests/folders.rs` (clicks and double clicks, Alt, Escape,
    folder drags and nudges, copy, delete, save, Ctrl+G, placing, the
    tools' refusal) and `flows.rs` (`the_list_shows_the_folder_tree`,
    `rows_drag_into_out_of_and_between_folders`,
    `shift_selects_the_rows_shown`,
    `canvas_double_click_alt_click_and_escape` (frames 1/60 s apart:
    `harness_at`), `ctrl_g_groups_and_ctrl_shift_g_ungroups`,
    `a_folders_panel_renames_it`).
- **The asset tab (issues #77, #78)** (STATUS.md "Asset tab (issues #77,
  #78, #79)"; the design's "The asset tab", call 18):
  - **Two sessions**: the level tab's and the asset tab's (`DocTab`,
    `state::Session`: document, history, unsaved mark, status,
    selection, saves, tool and its options, view, pixel selection, panel
    tab, focus). The shown tab's are `EditorState`'s own fields; the
    other's wait in `other`, swapped by `place` (`swap_session!`: a new
    per-tab field goes into `Session` and the macro); `on_tab(tab, f)`
    runs `f` on a tab and puts the shown one back; `switch_tab` (the
    session bar's `ShowTab`, an asset opened, `Opened`) bumps
    `shown_generation` (the host uploads the canvas whole) and resends
    `Distant`. Shared: the catalog, settings, clipboard, dialog, browser,
    textures, the views (collision, Materials, grid, lit) and the clock.
    `document()` and `dirty()` are always the level's (crash recovery
    covers the level only); `active_document()`, `active_dirty()` the
    shown tab's (the panels and the host's canvas draw it);
    `asset_document()`, `asset_dirty()`, `asset_key()`.
  - **Opening** (`asset_tab.rs`): `Document::open_asset(asset, theme,
    catalog)` (a level of the asset's size and art scale, the open
    level's theme, else `DEFAULT_THEME`, else the first; no spawns;
    `preview_look` with its groups and materials; `is_asset`,
    `asset_parts`: the states, start, exposed entries, sounds, script and
    health kept as opened) and `to_asset` (the name is the bake's,
    edited by `Meta`; `olr_edit::asset::carried_look`). Edit asset
    (`EditAsset(key)` → `ReadAssetToEdit` → `AssetToEdit`) and New
    asset… (`NewAssetDialog`, `Dialog::NewAsset`, `MakeAsset(NewAsset)`:
    `NewAsset::refusal`, then one empty drawn Background layer,
    `FIRST_LAYER`, unsaved) ask first when the open asset is unsaved
    (`Pending::EditAsset`, `NewAsset`); Close asks for the asset
    (`Pending::CloseAsset`), then the level (`Pending::Close`).
  - **Saving** (`save_asset`): the user's own `SaveAsset { replace:
    Some(folder) }`; a built-in or new one `replace: None`, a name the
    user has asking `Dialog::AssetExists` (`pending_tab` beside
    `pending_asset`; Keep both renames the document too); `tab_save`
    waits for `AssetSaved`, which makes the key the user's
    (`tab_saved`). A built-in asset's key stays `builtin` till then and
    the status bar says `BUILT_IN_NOTE`.
  - **Refusals**: `update` notes the history's `pushes()` before an
    asset tab action; a new step is checked (`Document::asset_refusal`:
    no distant layer, raster or spawn; a state's own picture that no
    longer fits its layer (`misfit_picture`, a safety net: #79 carries
    them along); each exposed entry's and state value's layer still offering it,
    "lamps is used by exposed lamp_colour: unexpose it first"; then
    `olr_format::assets::check_states`) and a refused one reverted and
    forgotten (`guard`, `History::retract`), the unsaved mark, selection
    and focus restored, the status naming the entry. `AssetParts::
    constrains` is false for an asset without states and exposure: never
    refused.
  - **Panels** (issue #77; laid out by #78, below): the
    session bar starts with `labels::LEVEL_TAB` | `ASSET_TAB` (Asset
    disabled with `ASSET_TAB_HINT` until one is open); the asset tab has
    no Play, File ▾ Open…, Spawn, Haze brush (`offers(tool)`, the
    shortcuts too) or Spawns tab, no Distant switch, and nothing
    selected shows the asset's panel (`ui::level::asset`: `NAME`,
    `ASSET_SIZE` with `WIDTH` and `HEIGHT`: a `Resize` from the top-left,
    one step; the art scale shown; Lighting; Materials). The library
    has `NEW_ASSET` and, beside a picked asset, `EDIT_LIBRARY_ASSET`
    ("Edit asset"; #81's "Edit in level" is the level's own copies').
    `EditorData`'s drafts belong to the tab, the directory and the asset
    key.
  - **Layout (issue #78)** (`ui/asset.rs`, the skin editor's, issue
    #55's sizing rules): the session bar wrapped to the window (below
    `ui::NARROW`, 900 points, the `TOOLS_TAB` | `LAYERS_TAB` switch
(its Layers button relabelled "Layers panel" for AccessKit since #80,
whose column tab is `LAYERS`);
    Level | Asset, File ▾ with Save and Close, Undo, Redo, "Asset:
    name", the unsaved mark `UNSAVED`, View ▾ without Preview parallax);
    on the left the tools panel (`olr-editor-asset-tools`, 20 % of the
    width, 200-220 points): the rail's buttons in rows (`rail::grid`),
    then the chosen tool's options (`toolbar::options_panel`: the options
    bar's functions in a column, a group of toggles in wrapped rows, a
    picker under its label at most the panel's width); on the right the
    layers panel (`olr-editor-asset-layers`, 37 %, 240-400 points): issue
    #79's states panel (below), `LAYERS` (the tree, `elements::tree`: no
    scroll area of its own, names cut to the room), the selection's
    properties or the asset's, the previews; the status line; the
    canvas. Narrow, one side panel shows, 55 % of the width (235 points
    at 427). Both scroll vertically only and follow a focus that moved
    otherwise than by the pointer (`see_focus`, `follow_focus`, the skin
    editor's). `EditorData::fit` (the asset tab) makes the shared field
    helpers fit: `number`, `text` and `slider` put the label above the
    field where both do not fit (`beside`), a slider's track shortened;
    "What is it?" and a group's colour kinds wrap their rows in both tabs
    (as before where they fit); an element's Surface pickers, the
    materials' rows and an animation's keys fit in the asset tab. #83
    lays the level tab out the same way (below; `fit` is on in both).
  - **Previews** (`ui/previews.rs`): the asset light (`PREVIEW_LIGHT`,
    full bright) and dark (`PREVIEW_DARK`, under `preview_look`), each
    at game size (3 points a world pixel, shrunk to the panel) and
    enlarged (whole points a pixel fitting the panel, at most 12 and 480
    points high), from `olr_edit::asset::preview_state(&doc.to_asset(),
    shown, dark)` on the CPU (#79: the shown state, fully posed); made again only when the document's
    `bake_generation()` (bumped by every `rebake` and `apply`) moved and
    no drag is in progress (`EditorState::dragging`), kept as textures
    by (`shown_generation`, bake generation) in `EditorData::previews`
    (`preview_generation()` for tests).
  - Tests: `tests/asset_tab.rs`, `flows.rs`'s
    `paint_an_asset_through_the_panels` (also at 1280 × 720 at UI scale
    3, through the narrow switch) and `previews_follow_edits_not_frames`,
    and `tests/fits.rs` (issue #78): `the_asset_tab_fits` (at
    `olr-ui/tests/fits.rs`' sizes and 1400 × 860 under `olr_ui::theme`,
    both switch positions below `NARROW`, the asset, a layer and a glass
    layer: every widget across the window and within its panel, every
    enabled control focused and scrolled whole into view; #79: an asset
    of 32 states) and `the_options_are_under_the_tools`.
- **States in the asset tab (issue #79)** (STATUS.md "Asset tab (issues
  #77, #78, #79)"; `state_ops.rs`, `ui/states.rs`):
  - **The swap form**: `AssetParts::states` (`StateEntry`: name, `own`
    pictures by layer id as `Arc<OwnPicture>`, values, sounds), `start`,
    `shown`. While a state other than the start is shown, the document's
    items and glow masks hold its own pictures and its `own` the start's
    (`swap_shown`, an involution); `show_state(to)` swaps back, sets
    `shown`, swaps again. `to_asset` swaps back on a copy, so saving and
    the previews see the stored asset. Queries: `states()`,
    `start_state()`, `shown_state()`, `in_state()`, `shown_owns(id)`,
    `owners(id)`.
  - **Edits**: `Edit::Show { before, after }` (forward shows `after`,
    backward `before`) and `Edit::States` (`StatesSnap`: the entries,
    start and shown; own pictures counted once by address in
    `MAX_BYTES`). `EditorState::update` brackets every asset-tab step
    with `[Show{k0,k0}] … [Show{k1,k1}]` (`carry_states`, through
    `History::amend_last`), so undo and redo apply in the state the step
    was made in and restore the shown state. `Whole::states` carries them
    through a resize (`Document::resize` re-lays own pictures and masks).
  - **Operations** (`EditorAction::SelectState` (no step, kept per
    asset), `AddState` ("State N", `new_state_name`; selected, its name
    field focused: `state_named()`), `DuplicateState` (`copy_name`),
    `MakeStart`, `RemoveState` (asks `Dialog::RemoveState`; refused on
    the start, `START_STAYS`, and health's state, `HEALTH_ENDS_HERE`:
    `remove_refusal`), `RenameState`, `FollowStart(id)`): each through
    `restate`, which shows the start, runs `olr_edit::asset`'s operation
    on `to_asset()`, diffs it into Layer and GlowMask edits plus one
    States edit, and shows the new state. Add and Duplicate stop at
    `MAX_STATES` (`add_refusal`).
  - **Painting per state**: `begin_stroke` returns the edits to record
    first (`own_for_stroke`: the layer made the shown state's own, then
    `convert_for_stroke`); the pointer drops them when the stroke changes
    nothing. The Glow brush owns through `own_for_glow`; Copy, Cut and
    Paste of pixels act on the shown pictures. Refused in a non-start
    state: an image stroke (`IMAGE_IN_STATE`), its Glow brush
    (`IMAGE_GLOW_IN_STATE`), the Surface brush (`SURFACE_IN_STATE`).
  - **Carried along** (`Document::carry_states`, after each step):
    removed layers' own pictures and the state values naming them are
    forgotten (the status says "removed with it: …"), own pictures mixed
    or unmixed to the layer, own masks dropped with the layer's. Merge
    down and Split refuse an owned layer (`refuse_owned`).
  - **Canvas**: `rebake` poses the shown state in place
    (`olr_edit::asset::posed_in_place`: no X, Y, offsets or Facing) for
    visibility, opacity and the glow groups (`canvas_look()`, which the
    host's lit canvas compiles).
  - **The panel** (`ui/states.rs`, top of the layers panel): each state
    as a button of its picture posed light (at most `PICTURE_HEIGHT`, 48
    points), its name under it (`labels::state(name, start)`, ★ on the
    start); `ADD_STATE`, `DUPLICATE_STATE`, `MAKE_START`, `REMOVE_STATE`
    (disabled with the reason as hover text); `STATE_NAME`. Pictures are
    `StatePictures`, remade only for states whose version moved
    (`AssetParts::versions`, bumped by `Document::touch`; `made()` for
    tests). The layer list's dot `labels::owned(name)` marks layers the
    shown state owns; their properties offer `FOLLOW_START`.
  - Tests: `tests/states.rs`, `flows.rs`'s
    `make_an_off_state_and_paint_it`, `fits.rs`' 32 states, and
    `history.rs`' `every_state_operation_undoes_to_the_same_asset`.
- **Properties, sounds and script (issue #80)** (STATUS.md "Asset tab
  (issues #77, #78, #79, #80)"; `parts_ops.rs`, `ui/parts.rs`):
  - **Edits**: `Edit::Parts { before, after }` (`PartsSnap`: the exposed
    entries, sounds as `Arc<Vec<u8>>` counted once by address, script,
    health). Each operation runs through `Document::reparts`:
    `olr_edit::asset`'s operation on `to_asset()`, then an `Edit::States`
    of the states' values and loop/enter sounds (pictures untouched) and
    an `Edit::Parts`; a refusal (`AssetError`'s message) changes nothing.
    Applying either rebakes the whole canvas when the shown state's
    values, the exposed defaults or health changed.
  - **Operations** (`EditorAction`): `SetValue { property, value }` (the
    shown state's; `None` clears), `Expose { property, default }`
    (`olr_edit::asset::exposed_name`; the start's value as default; a
    position's min and max at it; an `Own` group's glow colour asks
    `Dialog::ExposeColour` first, `Document::expose_needs_default`),
    `Unexpose(name)` (asks `Dialog::Unexpose`: "Copies' values and
    scripts using <name> will be errors"), `RenameExposed`,
    `SetExposed { name, change }` (`ExposedChange`), `Destructible(bool)`
    (`DEFAULT_HEALTH`, At 0 the first state but the start), `SetHealth`,
    `StateSounds { loop_sound, enter_sound }`, `RemoveSound`,
    `PlaySound(name)` (view only: `EditorCommand::PlaySound(bytes)`, the
    host's `HostOut::PlaySound`), `NewScript` (`parts_ops::NEW_SCRIPT`),
    `RemoveScript` (asks `Dialog::RemoveScript`), `SetScript(text)`.
    Import sound… is `Browse(Purpose::ImportSound)` (WAV and Ogg
    listed); the pick sends `EditorCommand::ReadSound(path)`, the host
    reads at most `MAX_ASSET_SOUND_BYTES` and answers
    `SoundRead { name, bytes }` (`parts_ops::sound_name`) or
    `ImportFailed`. Queries: `parts_snap`, `asset_header`, `offers`,
    `shown_value` (`Shown { value, own }`), `exposed_entry`,
    `script_issues`, `sound`.
  - **The column** (`ui::parts::Column`, kept in `EditorData::column`):
    under the states panel the tabs `LAYERS`, `PROPERTIES`, `SOUNDS`,
    `SCRIPT`. Properties: `WHOLE_ASSET`'s rows (with `DESTRUCTIBLE`,
    `HEALTH`, `AT_ZERO`), then the layers top first (folders as
    headings, dirt layers greyed `NOT_OFFERED_DIRT`); each row
    `labels::property(owner, name)`, its value field by kind, ● when the
    state sets it, `clear_value(row)`, `expose(row)` (disabled when the
    group is exposed through another layer), an exposed row's
    `exposed_name`, `exposed_default`, an Int's `exposed_min` and
    `exposed_max`, `UNEXPOSE`, and `ALSO_CHANGES` under a group
    property. Sounds: each sound's name and length, `play(name)`,
    `remove_sound(name)`, `IMPORT_SOUND`, the shown state's
    `LOOP_SOUND` and `ENTER_SOUND` pickers (`NO_SOUND`). Script:
    `NEW_SCRIPT` | `REMOVE_SCRIPT`, the size against `MAX_SCRIPT_BYTES`,
    the errors as buttons `script_error(issue)`
    (`line:column: message`, or `NO_SCRIPT_ERRORS`); the central panel
    becomes `SCRIPT_EDITOR` (a `code_editor` `TextEdit`, unwrapped, a
    line-number gutter marking the error's line with ▶).
  - **The script draft** (`ui::parts::ScriptDraft`, `sync_script`):
    typing changes the draft; it commits as one `SetScript` step after
    `SCRIPT_PAUSE` (1 s of UI time, `request_repaint_after`) without a
    keystroke or when the editor loses the keyboard; a document change
    the draft has not touched resets it. A click on an error sets the
    cursor (`goto`). Broken scripts save; `olr_script::check` is syntax
    only.
  - Tests: `tests/properties.rs`, `flows.rs`'
    `expose_a_claw_and_give_states_values`, `fits.rs`' Properties,
    Sounds and Script setups (the script editor checked by its top-left
    corner only: a text area as large as its script), and `history.rs`'
    proptest (kinds 16-25: values, exposure, health, sounds, script).
- **The ground in the asset tab (issue #81)** (STATUS.md "Level tab
  (issues #81, #82, #83)"; `ground.rs`):
  - **The level tab**: `offers` Select, Spawn and the Haze brush only;
    `update` refuses the layer actions there (`level_refuses`:
    `LAYERS_IN_ASSET_TAB`; a tool `EDIT_IN_LEVEL_HINT`). The right panel's
    first tab is `labels::COPIES` (#82: every copy, below); the ground's
    copy is locked (`GROUND_LOCKED`: it never moves, resizes, goes or is
    duplicated) and drawn as `Overlay::Box { locked: true }` without
    handles. Its panel (`ui::properties`): `ground_name`, the size,
    `labels::EDIT_IN_LEVEL`. The level panel's Lighting has no glow groups
    (`ui::look::LookScope::Level`) and no Materials.
  - **Edit in level** (`EditorAction::EditGround`, `Pending::EditGround`,
    asking first when the asset tab is unsaved): `Document::ground_of(level)`
    (`is_ground`; no spawns, `asset: None`, so no asset refusals: distant
    layers and rasters are allowed) opens in the asset tab
    (`open_ground_tab`; `EditorState::ground`: the asset session's
    `changes` at the last apply), titled `asset_title()` "Ground in
    <level>", drawn under the level's look (`canvas_look()` is the
    level's), its tile paints the level's theme's; no states, column tabs
    or previews; `LookScope::Ground` (glow groups) and Materials; Play in
    its session bar; View ▾ Preview parallax (`ground_open()`). Its size,
    art scale, name and theme are refused there (`GROUND_IS_THE_LEVELS`).
  - **Applying** (`apply_ground`): `ShowTab(Level)`, Play, the asset
    tab's Save and `leave` apply the asset tab's changes since the last
    time as one level step, `Edit::Ground` (`GroundSnap`, `ground_edit`:
    the look is the level's with the ground's groups and materials,
    `merged_look`), the level unsaved; `undo_name()` is `EDIT_GROUND`
    then (the Undo button's hover). Save there applies and saves the
    level (`save_asset`), both marks clear (`ground_saved`).
    `ShowTab(Asset)` follows the level (`sync_ground`: `same_ground` →
    `follow_level`, else `reopen_ground(GROUND_FOLLOWED)`); an undo or
    redo of a step holding `Edit::Ground` reopens it
    (`GROUND_WENT_BACK`), its history cleared, the selection kept where
    it still exists. `Opened`, `Recovered` and closing the asset tab drop
    the link (`drop_ground`).
  - **Tests**: the 0.2 layer tests run in the asset tab, opening the
    ground with `common::ground` / `ground_at` / `edit_ground` (they read
    `active_document()`, `active_dirty()`); `common::rename_level` is a
    level tab's edit; the `Kept` and level-look tests stay in the level
    tab (`editor_at`); `flows.rs`' `opened()` is the ground, `opened_level()`
    the level tab (the look panel, spawns, resize, the layout tests over
    `offered` tools and `BARS`, the level tab's rail);
    `tests/level_tab.rs` (#81's acceptance tests, also
    `every_shipped_level_saves_byte_identical`) and `flows.rs`'
    `paint_the_ground_from_the_level`.
- **The level tab (issue #82)** (STATUS.md "Level tab (issues #81, #82,
  #83)"; the document's side `copies.rs`, the state's `level_tab.rs`):
  - **The document**: `placed` holds the whole placement; the items stay
    the ground's layers and `level.look` the own look (the level's
    settings, the ground's groups and materials). `CopyCache` keeps each
    copy laid (`olr_edit::placed::lay_copy`: posed, laid, folded, ids
    `<copy>.<id>`) by its unlocked `PlacedCopy` and the ground's
    generation (`copy_cache.ground`, bumped by every non-placement edit
    in `apply`), the decoded assets by key. A plain ground (at (0, 0),
    natural, not distant, no state or values) bakes the items directly.
    `rebake_placed(rect)` flattens as `olr_edit::placed::bake` does
    (`gather_copy` in copy order, distant pieces first) under the
    gathered look, the whole level when the gathered ids or see-through
    changed; it sets `canvas_look` and `placed_look`, which `playable()`
    (Play), the bake and `to_pack` use (the ground asset's groups and
    materials are the own look's). An unchanged open and save is byte
    identical (`a_placed_level_opens_whole`,
    `every_shipped_level_saves_byte_identical`).
  - **Operations** run `olr_edit::placed`'s on a light pack
    (`placement_op`: the ground's asset a one-pixel stand-in with the
    own look's groups and materials, a `LevelSource` of sizes and look
    only): `add_copy`, `move_copies`, `resize_copy`, `set_copy`,
    `remove_copies`, `duplicate_copies`, `restack_copy` (nothing under
    the ground: `GROUND_AT_THE_BOTTOM`), `file_change` (a level resize's
    shift), `set_own_asset`; each gives `Edit::Placement { before, after }`
    (`PlacementSnap`: the file, the scripts, each side's assets the other
    lacks or holds otherwise, Arc'ed; `placement_edit`). `set_placement`
    applies a side, rebaking each changed copy's old and new extent (a
    lock none). A drag moves live (`live_move`) and ends in one
    `move_copy` step from the drag's start (`end_move`).
  - **The state** (`level_tab.rs`): `Selection::Copy`/`Copies`
    (`copy_selection`, stack order). Select (`level_press`): a spawn,
    then a handle of the one selected unlocked copy (reach
    `HANDLE_REACH`, at most a quarter of a side: a small copy's middle
    moves it), then `Document::copy_at` (the topmost copy with a covered
    pixel, a transparent one falling through, else the topmost box);
    Ctrl toggles; a press on the selection drags it (`Drag::MoveCopies`),
    on a locked copy `Drag::LockedCopy` ("<id> is locked"). Handles
    (`Drag::ResizeCopy`, from the press point): `copy_resized` (a corner
    from the opposite corner, an edge from the opposite edge, the other
    side centred while the aspect is kept; the leader the side changed
    more, the other `keep_aspect`'s; Shift snaps with `snapped`, never
    below 1×; 1 to `MAX_SIDE`; the natural size `Size::Natural`), the
    outline previewed, the status `size_note` ("58 × 4, 2×"), one
    `resize_copy` step on release. Arrows `nudge_copies`; Delete
    `remove_copies` (the status names an asset that went); Ctrl+C
    `Clipboard::Copies`, Ctrl+V `paste_copies` (`COPY_PASTE_STEP` further
    each paste), Ctrl+D `Duplicate`; cut is refused (`NO_CUT`), elements
    paste in the asset tab only and copies in the level tab only
    (`ELEMENTS_PASTE_IN_ASSET_TAB`, `COPIES_PASTE_IN_LEVEL_TAB`). Place
    (`PlaceAsset`, `PlaceAssetCentred`) reads the asset's files
    (`AssetRead { key, at, asset: AssetSource }`, the hosts'
    `olr_edit::asset::read_dir`) and `place_copy` adds it (refused in the
    asset tab: `PLACED_IN_THE_LEVEL`). A level resize shifts every copy
    but the ground in the same step (`shifted_copies`). `edit_copy` (the
    panel's button, a double click): the ground `EditGround`, a library
    copy `EditAsset`, another own asset `Pending::EditOwn` →
    `open_own_tab` (`decode_own`; `EditorState::own`, `OwnOpen`), whose
    edits reach the level as one placement step on the way back, Play,
    Save or `leave` (`apply_own`), followed on the way there (`sync_own`).
  - **The UI**: the Copies list (`ui::elements::copies`): rows top first,
    a lock toggle, the asset's name, the id, `LIBRARY_MARK` 🔗 or the
    weak `OWN_MARK` "own"; a click selects (Ctrl toggles), a double click
    `EditCopy`, a row dropped on another `RestackCopy`. The copy panel
    (`ui::properties::copy`): name, id, Position X/Y, Width/Height, the
    multiple, `KEEP_ASPECT`, Locked, `EDIT_IN_LEVEL` or
    `EDIT_LIBRARY_COPY`, Delete. The library's `PLACE` button.
  - **Tests**: `tests/level_tab.rs` (`placed_level`: the fixture with a
    lamp, a stretched torch and an own strip light; the issue's
    acceptance tests and the flow `move_and_resize_a_copy_through_the_canvas`),
    `asset.rs`' `placing_is_one_step_that_selects_the_new_copy_and_adds_lighting`.
- **The library panel (issue #83)** (STATUS.md "Level tab (issues #81,
  #82, #83)"; `links.rs`, `ui/library.rs`, `ui/level_tab.rs`):
  - **Layout** (`ui/level_tab.rs`, the asset tab's of #78): the session
    bar wrapped (below `NARROW` the `SHOW_LIBRARY` | `SHOW_COPY` switch,
    `EditorData::side`; Level | Asset, File ▾, Undo, Redo, the unsaved
    mark, View ▾, `toolbar::play`); on the left
    (`olr-editor-level-library`, `asset::side_widths`) `rail::grid`
    (Select, Spawn, Haze), `toolbar::options_panel` and `library::show`
    (`SEARCH_ASSETS`, kept in `EditorData::asset_search`;
    `ui::asset_matches`; the cards in rows of as many as fit); on the
    right (`olr-editor-level-copy`) `elements::show` (Copies | Spawns:
    `PanelTab` has no Assets) and the selection's panel or the level's.
    No options bar or rail column: `toolbar::session`, `options_bar` and
    `rail::show` are gone.
  - **Linking** (`links.rs`, over `olr_edit::library`'s pure `compare`,
    `update`, `relink`, `adopt`): `EditorState::compare_library` sends
    `CompareLibrary { origins }` (the level's linked origins) after
    `AssetsChanged` and `AssetSaved`; the host answers
    `LibraryCompared(Libraries)` (`olr_edit::library::read_origins`; the
    editor reads no files). `take_library`: per origin, an unbreaking
    change is one step (`update_assets`, the copies' keys then merged,
    `merge_same`), a breaking one only said, a missing one nothing.
    `library_links()` keeps the last `Link`s. Placing a newer version
    (`place_copy`'s `added.new_asset`) offers the update (`offer_newer`).
  - **Step names**: `Edit::Name(String)` (applied, nothing; `is_noop`)
    rides in a step (`copy_edit_named`); `undo_name()` reads it: "Update
    Lamp", "Unlink Lamp", "Save Lamp to library", "Use your Lamp".
  - **Actions**: `Unlink(id)` (`adopt`), `SaveToLibrary(id)`
    (`library_refusal` first: distant, raster, an unused glow group,
    `check_asset`; then `Dialog::AssetName` with `pending_link`; its
    `AssetSaved` relinks, `link_saved`; the ground saves a copy and stays
    own), `UseYourCopy` (`use_yours()`, offered by `offer_use_yours`
    when `tab_saved` made a built-in asset the user's; the status line's
    `USE_YOUR_COPY`). The copy panel's link (`ui::properties::link`):
    `labels::origin(builtin, name)` and `UNLINK`, or `SAVE_TO_LIBRARY`
    (the ground's panel too).
  - **Tests**: `tests/library_panel.rs` (the issue's acceptance tests),
    `flows.rs`' `place_two_lamps_and_edit_their_asset`, `fits.rs`'
    `the_level_tab_fits` (the sizes, both switch positions; nothing
    selected, the lighting's fine tuning, a library copy, the own strip
    light, the Spawns tab); the test host answers `CompareLibrary` from
    its library (`common::run`).
- **The copy panel and the level's library (issue #84)** (STATUS.md
  "Level tab (issues #81 to #84)"; `copy_ops.rs`, `notice.rs`,
  `ui/copy_panel.rs`, `ui/level_script.rs`):
  - **The copy panel** (`ui::copy_panel::rows`, under #82's and #83's
    rows; `Document::copy_facts(id) -> CopyFacts`, none for the ground):
    `COPY_ID` (a `text` field: `CopyChange::Id`, refused with
    `set_copy`'s reason; the selection and an open script follow the
    new id), `ID_HINT`; `LIVE` by `LiveOffer` (call 2, `olr_edit::asset::
    live_refusal`: `Hidden` for nothing to drive, `Disabled(why)` shown
    off with the reason as hover text and a line under it, `LIVE_DIRT`,
    the refusal's text for a raster or distant layer, `LIVE_DISTANT` on
    a distant copy; a copy Live already is always `Offered`; no hint under
    it since issue #87 draws live copies in play, its `LIVE_NOT_YET`
    gone; issue #85 leaves Live copies out of the ground; `to_pack`
    (`document::played`) and `playable()` run `olr_edit::placed::bake_pack`
    when a copy is Live, so Save writes and Play reads the played form); `STARTING_STATE` (a picker, hidden for one state:
    `labels::asset_start(start)` sets `state: None`, then each state);
    `VALUES` (`ValueRow`s: each exposed property's `parts::value_field`
    by its `property_type` within the exposed limits, named
    `labels::copy_value(name)`, its value the copy's own else
    `value_in_state` in the copy's starting state; a set row marked ●
    and given `DEFAULT_VALUE`, named `labels::default_value(name)`,
    clearing it); `COPY_SCRIPT` (`NEW_SCRIPT` or `EDIT_SCRIPT` and
    `REMOVE_SCRIPT`; `RUN_DEFAULT_SCRIPT`, shown "Also run <asset>'s
    script", when the asset has a script, enabled while Live);
    `DISTANT` and its `DEPTH` slider (`MIN_DEPTH`-`MAX_DEPTH`, on at
    `ops::DEFAULT_DEPTH`) when `distant_refusal` allows the asset,
    disabled with `DISTANT_LIVE` while Live (`check_placed` refuses
    both). Each change is one `SetCopy` step (`CopyChange::{Id, Live,
    State, Value, DefaultScript, Distant}` → `CopyProperty`).
  - **Scripts** (`ScriptTarget::{Copy(id), Level}`: `file()`
    `editor/scripts/<id>.rhai` or `editor/level.rhai`, `said()`):
    `OpenScript` (level tab, not the ground; a missing script made first
    with `NEW_COPY_SCRIPT` or `NEW_LEVEL_SCRIPT`, one step) sets
    `script_open()` (none once its script or copy is gone);
    `SetTargetScript` commits one step (`set_target_script` through
    `placement_op`; `PlacementSnap` carries `level_script`);
    `RemoveTargetScript` asks (`Dialog::RemoveTargetScript`);
    `CloseScript`. The level tab's middle (`ui::level_script`) shows the
    open script instead of the canvas: `BACK_TO_LEVEL`, its file, Remove
    script, the syntax errors (a click puts the cursor there) and the
    asset tab's editor (`parts::script_text`, `sync_draft`; the draft,
    `EditorData::level_script`, commits to the script it was of on a
    switch, a renamed copy's kept). The level panel (`ui::level`):
    `LEVEL_SCRIPT` New or Edit and Remove (named "Level script <button>").
  - **Problems** (`refresh_problems`, the level in place):
    `link_problems` errors, each break "<copy>: value glow: …" naming
    the asset ("… in Console"), each copy of a missing asset "<copy>:
    <name> is not in your library" ("the built-in library");
    `script_problems`: issue #89's errors for the scripts the level runs
    (`olr_edit::scripts::problems`: syntax, handlers and names; "<id>'s
    script, line 3: …", "the level script, line …", "<id>'s default
    script (<Asset>'s), line …"), which refuse the save (`pack::save`'s
    `SaveError::Scripts`, the host's `Saved` error), then warnings for
    the syntax of scripts that run nowhere (a static copy's, a default no
    live copy runs; `olr_script::check`). Link problems refuse nothing. The status line shows
    the first, cut to the line, and "(+N more)", all on hover.
  - **The notice** (`notice.rs`): `opened_links` (from `opened`) resets
    the kept answer (`libraries()`), the resolutions, a pending restore
    and the open script, and sends `CompareLibrary` with
    `notice_on_compare`; that answer opens `Dialog::LibraryChanged` when
    `library_changed()` (and no dialog is open) and takes nothing. A
    later answer (after `AssetsChanged`, `AssetSaved`, a placing whose
    origin the answer lacks) is merged into the kept one and
    `take_library` runs on the origins whose version changed since, so
    what Later put off stays; the links are recomputed from the kept
    answer on every `refresh_problems` (`relink_links`: origins it lacks
    are left out until the next). `Library(LibraryAction)`: `Update(key)`
    and `UpdateAll` (each unbreaking change, one step each, "Update
    <library name>"), `UpdateAnyway(key)` (`force_update`: one step with
    the resolutions appended to the level's notes, said in the status
    line and kept in `resolutions()`), `Restore(key)` (`restorable`: the
    level's copy, its name numbered on a clash, `SaveAsset { replace:
    None }`; its `AssetSaved` relinks as Save to library does,
    `pending_restore`), `Adopt(key)` ("Unlink <name>"). The dialog
    (`ui::dialogs::library_changed`, scrolled, within the window): the
    changed assets with Update, those with breaks (listed) with Update
    anyway, the missing ones with `RESTORE_ASSET` and `ADOPT`, each
    button named `labels::update(name)` and so on; `UPDATE_ALL`;
    `LATER` (`Choice::Cancel`). It closes when nothing is left. File ▾
    `LIBRARY_CHANGES` (`library_changes`) opens it again and asks the
    library anew.
  - **Export and import**: File ▾ `EXPORT_LEVEL` (`export_level`: the
    asset tab's edits applied; unsaved, `Dialog::Unsaved { then:
    Pending::Export }` offers Save and Cancel only) opens the browser
    in its one save purpose (`Browser::new_save(Purpose::ExportLevel,
    dir, "<pack id>.olrlevel")`: `file_name()`; the UI's `FILE_NAME`
    field, `EXPORT`; a click on a file takes its name). `ExportTo { name,
    replace }` adds `.olrlevel`, refuses what is not a plain file name,
    asks "Replace <name>?" for an entry of the folder (`replacing()`,
    `REPLACE` or `CancelReplace`), else closes the browser, remembers
    the folder and sends `ExportLevel`; the host answers `Note("Exported
    <file> (<size>, <n> assets)")` or `Failed`. File ▾ `IMPORT_LEVEL`
    browses `Purpose::ImportLevel` (folders and `.olrlevel` files); the
    pick sends `ImportLevel(path)`; `LevelImported` opens the level
    (`leave`: asking about unsaved changes) and says `import_said`
    ("Castle 2: reused yours console; added Console 2"; kept for the
    level it brought, `import_note`).
  - **Tests**: `tests/copy_panel.rs`, `tests/library_notice.rs` (the
    issue's acceptance tests; the test host exports and imports with
    `olr_edit::library`, its data folder the user library's parent),
    `flows.rs`' `make_a_live_console_with_values_and_a_script`,
    `fits.rs`' `the_copy_panel_and_the_notice_fit` (a copy of an asset
    exposing 64 values; the notice of ten assets, checked against the
    window only).
- **Tests** (`tests/`, one binary): `asset.rs` (issue #9: save, the name
  clash, the rectangle unclipped and hidden layers left out, placing a
  copy as one step with lighting added, Remove, Import, and the
  `egui_kittest` flow `save_and_place_through_the_panels`; the test host
  keeps a library, `common::set_library`; `common::catalog` lists
  `content/materials`), `materials.rs` (issue #43: the library, users
  and removal, the brush front and back with undo and refusals, the
  views taking turns, a save baking the maps, a level without materials
  saved without them), `document.rs`, `history.rs` (the
  undo proptest, the cap), `surface.rs` (issue #37), `state.rs` (a synchronous test host in
  `common.rs`), `tools.rs` (the ignored `brush_dab_speed`: about 2 ms in
  release for a size-16 dab on Castle Strike), `look.rs` (plan 15a: glow,
  groups, the clock, masks through resizes and pastes, the object
  clipboard, glow by colour) and `flows.rs` (the
  `egui_kittest` flows of spec §8; plan 15a's `glow_an_element_and_save`,
  `paint_glow_with_the_brush`, `glow_pixels_picked_by_colour`,
  `edit_the_look_panel`, `set_the_occlusion` (issue #10),
  `toggle_views_and_scrub_the_clock`; issue #8's
  `choose_a_tool_from_a_flyout`, `a_group_button_brings_back_its_last_tool`,
  `the_rail_highlights_the_chosen_tool`, `the_tool_options_follow_the_tool`,
  `more_and_fine_tuning_fold_and_are_remembered`, `make_parts_glow`,
  `the_tab_follows_the_selection`; issue #43's
  `materials_through_the_panels`). The content tests skip without
  `OLR_CONTENT_DIR`.

Test: `cargo nextest run -p olr-editor`.
