# olr-skin-editor

The worm skin editor (issue #14; decisions in `docs/porting/STATUS.md`,
"Skin editor (issue #14)"): a skin's 21 frames painted in colour, player
colour and glow, as pure state a host drives, with egui panels.
`olr-game` hosts it (`olr-game/src/skin_editor.rs`), opened from the
Editor screen's Skin tab (`olr-ui`).

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.** Depends on `olr-data`,
  `olr-format` (`SkinSource`, `compile_skin`), `olr-edit` (`skin::blank`,
  `skin::double`, `skin::preview_frame`), `olr-look` (the skin light's
  caps) and egui 0.36.2; not on `olr-ui`, `olr-editor`, `olr-render` or
  files (the host reads and writes packs and applies `olr_ui::theme`).
  Not wasm. Tests use `egui_kittest`.
- **The pattern** (`olr-editor`'s, followed rather than shared: its
  history is of level edits): `SkinEditorState::update(SkinAction) ->
  Vec<SkinCommand>` is pure; `ui::show(ui, &state, &mut SkinEditorData)`
  draws egui and returns actions. Commands: `Save { id, name, source }`
  (into pack `id`, the user's own, or with `id: None` a new pack named
  `name`) and `Close`; answers `Saved(id)` and `Failed(why)` (an error
  dialog). Images are egui textures `ui` makes itself (frame images by
  frame generation, the overlay, the checks, the preview's two cells):
  the host does no GPU work. Widgets are found by `ui::labels` and the
  frame buttons by `state::frame_name` ("Aim 3, walk 2").
- **Document** (`doc.rs`): `SkinDoc::new(SkinSource)` takes OLX's layout
  or double resolution's (7 × 3 frames of 20 × 18 or 40 × 36) with planes
  of its size, else `OpenError` (never a panic); a tint other than 0 is
  the player's colour; the glow plane is always held, saved only when
  something glows (`to_source`). `Frame` (colour, tint, glow) with
  `flipped` and `shifted` (wrapping: nothing is lost); a transparent
  pixel neither tints nor glows. `generation` and `frame_generation(f)`
  follow every change.
- **History** (`history.rs`): `Edit::{Frames, Settings, Whole}` (frames
  before and after, the name and light, the whole source for Make double
  resolution), applied through `Edit::apply`, a no-change edit no step;
  `MAX_STEPS` 400. The unsaved mark compares `History::at` with the step
  saved; a save answered after an edit leaves that edit unsaved.
- **Tools** (`paint.rs`, `state.rs`): `Target::{Colour, Tint, Glow}`
  (one switch, each with its overlay: tint magenta, glow yellow as strong
  as it is); `Tool::{Pencil, Eraser, Fill, Eyedropper, Line, Rect}` (keys
  B, E, G, I, L, R), brush 1 or 2 × 2; a stroke, line or rectangle is one
  step (Press, Drag, Release in art pixels from the frame's top left;
  far pointers clamped); a fill takes the pixels joined to the one
  clicked of its colour; the eyedropper picks a colour (or the glow
  strength) and returns to the pencil. Glow strengths are quarters
  (`GLOW_STRENGTHS`, as the game draws them). Recent colours: the last
  `MAX_RECENT` 12 painted. Frame operations: `CopyFrame`, `Paste(Frame |
  Row | Column)` (one step; another size refused with a status), `Flip`,
  `Shift` (the frame or, `ShiftAll`, all 21). `Onion::{Off, PreviousAim,
  PreviousWalk}` (the walk wraps; the first aim has none).
  `SetLight` clamps to `MAX_SKIN_LIGHT_INTENSITY`/`REACH`; `MakeDouble`
  (classic only) is `olr_edit::skin::double`, one step.
- **Saving and closing**: Save without a pack asks the name
  (`Dialog::Name`, the skin's name too, not a step); Close with unsaved
  changes asks (`Dialog::Unsaved`: Save and close, Don't save, Cancel).
  While a dialog is open only its answers, the host's answers and the
  view act.
- **Preview** (`anim.rs`, `ui/frames.rs`): `Tick(dt)` from the host walks
  the worm (7.5 frames a second) and sweeps its aim (200° a second, −90°
  to 60°), turning round after each sweep; the frame is the game's
  (`OLX: src/common/CWorm.cpp:1225-1244`). `Animate(false)` shows the
  chosen frame. The cells are `olr_edit::skin::preview_frame` of the
  compiled skin (light and dark, the dark showing glow and light), shown
  at game size and enlarged.
- **Panels** (`ui/`): session bar (Save, Close, Undo, Redo, the name, the
  unsaved mark), left the tools (`tools.rs`), middle the large frame
  (`canvas.rs`: checks, onion at 35 %, the frame, the overlay, a grid from
  `GRID_ZOOM`; secondary or middle drag pans, the wheel zooms through
  `ZOOMS`), right the frames grid, the preview and the skin's settings
  (Name, Skin light with intensity and reach committed when a drag stops,
  Make double resolution for a classic skin), dialogs (`dialogs.rs`).
  Shortcuts (Ctrl+Z/Y/S, Ctrl+C/V for a frame, the tools' keys) not while
  a dialog is open or a field has the keyboard.
- **Tests** (`tests/`, one binary): `state.rs` (strokes as steps, every
  tool and target, paste to a row and a column, flip and shift, onion,
  settings within the caps, Make double resolution and its undo, the
  first save's name, Close asking, recent colours, the preview's walk
  covering all 21 frames both ways, other layouts refused) and `flows.rs`
  (`egui_kittest`: paint, paste to a row, tint, glow, light, undo, redo,
  Make double resolution and save through the panels; shift, flip and
  the onion skin; Close asking; the preview), on a synthetic classic skin
  (`common.rs`): no content needed.

Test: `cargo nextest run -p olr-skin-editor`.
