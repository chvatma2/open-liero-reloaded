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
  `ImportAsset`, `RemoveAsset`; issue #11's `Distant`; issue #77's
  `ReadAssetToEdit`)
  and answers (`Opened`, `Recovered`: as `Opened` but unsaved, `OpenFailed`,
  `Saved`, `PlayEnded`, `Listed`, `ImportImage`, `ImportFailed`,
  `TexturesChanged`, `AssetsChanged`, `AssetSaved`, `AssetRead`,
  `AssetToEdit`; and
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
  them); `diagnostics` are R6's cheap checks. Issue #74 (until issue
  #82's level tab): the editor's scope is the ground placed alone, its
  layers the items; `open` refuses a level placing other assets
  (`PLACED_ASSETS`, "This level has placed assets: the level tab opens
  it (issue #82)") and converts an OLR 0.2 level
  (`olr_edit::placed::from_composition`; `converted()`): `opened` then
  leaves it unsaved with the status bar's `CONVERTED` ("Converted from
  OLR 0.2: save to keep it"), and saving writes the placement
  (`placed.ron`, the ground's `asset.ron`) without `composition.ron`.
  The document keeps the placement and the other assets' files as
  opened (`placed`, `verbatim`) for `to_pack`. Tests: `document.rs`'
  `opening_a_02_level_converts_it_unsaved`,
  `a_level_with_placed_assets_is_refused_with_the_reason`.
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
  Redo, the unsaved mark's kept space; at the right View ▾: Collision
  view (rock, dirt, background; "Material view" before issue #43), Grid,
  Preview parallax (issue #66), the light views, the Materials view and
  the lights' clock; Play and its ▾, the mod and bots) and the tool options bar (`toolbar.rs`: the chosen tool's
  options only: the brush for `Tool::uses_brush`, the paint for
  `Tool::lays_paint`, then its own; empty but as tall without any); on
  the left the tool rail (`rail.rs`, icons drawn in `icons.rs`): Import,
  Select, Draw ▸, Pixels ▸ (`ToolGroup`: a click chooses the group's
  last tool, `EditorState::last_tool`; a second click, the corner mark or
  a right click opens the flyout), Glow (named "Glow brush"), Surface
  (named "Surface brush", issue #43), Spawn; on
  the right one panel (at least 260 points): the tabs Elements (the
  folder tree, issue #61: drag a name to reorder, eye and lock, Add
  layer, New folder) | Assets (issue #9, below) |
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
    ordinary elements. Issue #71: the pieces are the asset's starting
    state posed with its defaults (`olr_edit::asset::landed_pieces`); its
    states, values, sounds and script stay behind, and the status bar
    says so (`left_behind`, after `ADDED_LIGHTING` when both).
  - **The tab** (`ui/elements.rs`): thumbnails grouped `BUILT_IN` and
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
  asset carries the materials they name; `place_asset` joins or adds the
  asset's materials with its glow groups (`with_materials`, one look
  edit) and gives each landed element its surfaces and re-numbered maps
  (`Document::land_surfaces`: `Edit::SurfaceMap`s, then an
  `Edit::Element`; the same step). A paste still lands no surfaces.
  Test: `asset.rs`'s
  `placing_joins_or_adds_materials_and_lands_painted_maps`.
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
  #78)"; the design's "The asset tab", call 18):
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
    no distant layer, raster or spawn; a layer a state owns a picture of
    keeps its box and picture shape and the asset its size, "state …
    has its own picture of …: edit it in the states panel (issue #79)";
    each exposed entry's and state value's layer still offering it,
    "lamps is used by exposed lamp_colour: unexpose it first"; then
    `olr_format::assets::check_states`) and a refused one reverted and
    forgotten (`guard`, `History::retract`), the unsaved mark, selection
    and focus restored, the status naming the entry. `AssetParts::
    constrains` is false for an asset without states and exposure: never
    refused.
  - **Panels** (today's layout; #78 moves it to the skin editor's): the
    session bar starts with `labels::LEVEL_TAB` | `ASSET_TAB` (Asset
    disabled with `ASSET_TAB_HINT` until one is open); the asset tab has
    no Play, File ▾ Open…, Spawn, Haze brush (`offers(tool)`, the
    shortcuts too) or Spawns tab, no Distant switch, and nothing
    selected shows the asset's panel (`ui::level::asset`: `NAME`,
    `ASSET_SIZE` with `WIDTH` and `HEIGHT`: a `Resize` from the top-left,
    one step; the art scale shown; Lighting; Materials). The Assets tab
    has `NEW_ASSET` and, beside a picked asset, `EDIT_LIBRARY_ASSET`
    ("Edit asset"; #81's "Edit in level" is the level's own copies').
    `EditorData`'s drafts belong to the tab, the directory and the asset
    key.
  - Tests: `tests/asset_tab.rs` and `flows.rs`'s
    `paint_an_asset_through_the_panels`.
- **Tests** (`tests/`, one binary): `asset.rs` (issue #9: save, the name
  clash, the rectangle unclipped and hidden layers left out, placing as one
  step with lighting added, join and add, Remove, Import, and the
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
