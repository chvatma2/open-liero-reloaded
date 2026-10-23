# olr-material-editor

The surface material editor (issue #44, Materials 8/10, and issue #45's
live previews, Materials 9/10; the spec is
`docs/superpowers/specs/2026-10-22-materials-and-heat-design.md`,
decisions in `docs/porting/STATUS.md`, "Material editor (issue #44)"):
the built-in materials and the user's listed, made, copied, renamed,
deleted, and their fields edited, as pure state a host drives, with egui
panels. `olr-game` hosts it (`olr-game/src/editor/material.rs`), opened
from the Editor screen's Material tab (`olr-ui`).

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.** Depends on `olr-data`
  (`Rgba`), `olr-format` (`materials`: `MaterialSource`, its bounds,
  `compile_material`, `valid_material_id`), `olr-edit`
  (`material::MaterialEntry`, the library's listing), `olr-look` (the
  heat's bounds, `LAVA_RAMP`, `ramp_at`) and egui 0.36.2; not on
  `olr-render`, `olr-game`, `olr-ui` or files (the host lists, writes and
  keeps the recovery copies). Not wasm. Tests use `egui_kittest`.
- **The pattern** (the skin and mod editors'):
  `MaterialEditorState::update(MaterialAction) -> Vec<MaterialCommand>` is
  pure; `ui::show(ui, &state, &mut MaterialEditorData)` draws egui and
  returns actions (the fields' commits first). Commands: `Create(m)`
  (New and Copy: a new file, never over one), `Save(m)` (checked),
  `Rename { from, to }` (the saved values under the new id and name, the
  old file removed), `Delete(id)`, `Close`; answers `Created`, `Saved`,
  `Renamed`, `Deleted`, `Failed(why)` (an error dialog), `Note`; and the
  preview's `Preview(PreviewCommand)` (below), never answered. Widgets
  are found by `ui::labels` (the fields' names, "Capacity %", and
  `labels::key_temperature(i)` and the like for the ramp's keys, numbered
  from 1); the list's entries by the materials' names.
- **State** (`state.rs`): the library as the host listed it
  (`MaterialEditorState::new(list_materials(..))`: the built-in ones
  first), the open material (`Key { builtin, id }`, its saved form, the
  document, undo and redo snapshots, `MAX_STEPS` 200), the dialog
  (`Dialog::{Name(Purpose), Delete, Unsaved(Next), Error}`), the status.
  `recovered(library, m)` opens a recovery copy (unsaved; its file may be
  gone: `unsaved_file`, Save writes it). A built-in material is
  read-only (`read_only`: no Set, Save, Rename or Delete; Copy is its
  Edit a copy). `dirty` compares the document with the saved form.
  Every `Edit` is clamped to its bounds; a key's temperature stays
  between its neighbours'; `AddKey` fills the widest gap; `OwnRamp(true)`
  starts from `LAVA_RAMP`. `free_id(name, taken)` is an id's slug, free
  of every listed id (built-in too), bounded by `MAX_ID_TRIES`.
  Selecting another material, New, Copy and Close ask first with unsaved
  changes (`guard`, `Next`): Save changes runs the next step once
  `Saved` is answered (`pending`), Don't save reverts first.
- **Panels** (`ui/`): the session bar (Save, Close, Undo, Redo, the
  material, the unsaved mark), the library on the left (`list`: New
  material, Copy, Rename, Delete; Built in, Yours), the fields in the
  middle (`fields.rs`: name and file, Edit a copy, the problems, Notes,
  issue #57's Light soak (a combo box, `labels::LIGHT_SOAK`: "Level's",
  `LIGHT_SOAK_LEVEL`, none, or `labels::soak_px(n)` 0 to 12 px;
  `Edit::LightSoak`, clamped, undone like any field), Takes heat, the
  four heat numbers and Light as a typed field, a slider and their bounds, Own colour ramp, the ramp's strip, `strip`, and its
  keys, Add key), the live preview on the right (below), the status at
  the bottom, the dialogs
  (`dialogs.rs`: the name with the file it gets, Delete, unsaved changes,
  errors; Escape cancels). Ctrl+Z/Y/S when no field has the keyboard.
- **Live preview** (issue #45, `preview.rs`, `ui/mod.rs`'s `preview`):
  the host (`olr-game`'s `editor/heat_preview.rs`) renders the scene with
  `olr-render`'s heat pass on a renderer of its own and hands the panel
  an egui texture (`MaterialEditorData::set_preview(PreviewImage)`; none:
  the place says it needs a graphics device); this crate stays free of
  `olr-render`. The scene's layout is this crate's (`preview::PATCH` 64
  × 72 world pixels, `DIVIDER` 8, `SCENE` 136 × 72; `patch_x`,
  `patch_at`): the material open on the left patch (the document as
  edited, read by the host each frame: a field edit changes the next
  frame), the one compared on the right. `MaterialAction::Preview(
  PreviewAction)` acts whatever is open, dialogs and built-in materials
  too, and never touches the material: `Blast([x, y])` (a pixel of a
  patch; both are blasted there) asks `PreviewCommand::Blast { at, size
  }`, `Size(BlastSize)` (`Small`, `Bazooka`, `Nuke`: OLX damage 3, 4,
  15, the default Bazooka) sets the next blast's, `Scale(TimeScale)`
  (1×, 4×) asks `TimeScale(factor)` when it changes, `Restart` asks
  `Restart`, `Compare(Option<Key>)` sets the material compared (a key not
  listed is none); `update` asks `SetCompare(material)` whenever the
  library's copy of it changes (picked, saved, renamed, which it
  follows, deleted: none). `PreviewSettings` (`state.preview()`),
  `compare_material()`. Labels: `PREVIEW` (the picture; a click on a
  patch blasts, on the wall nothing), the sizes' and scales' own
  (`BlastSize::label`, `TimeScale::label`), `RESTART`, `COMPARE` (a
  combo box: `COMPARE_NONE`, `labels::compare_entry(name, builtin)`).
  The preview draws lit solids, so it shows a material's own light soak
  (issue #57): its look has no level look, so the level's soak there is
  0 ("Level's" soaks nothing) and the hot rim's glow light seeps into the
  patch's rock and dirt as deep as the material's soak.
- **Tests** (`tests/`, one binary, no content needed: `common.rs`'s
  library of Stone and Wood built in and the user's Lava rock): `state.rs`
  (built-in read-only, every field's bounds, the ramp's keys, New and
  Copy ids, the unsaved changes' question, undo and redo, Rename and
  Delete, a recovered material, `free_id`) and `flows.rs`
  (`egui_kittest`, a test host keeping the user's files in memory:
  create, edit and save; Edit a copy of a built-in one; a field's bounds;
  issue #57's `light_soak_is_set_saved_and_undone`;
  Delete and Rename; a recovered material saved and Close asking);
  `preview.rs` (issue #45: the controls' commands, a dialog open too;
  the material compared following the library; the panel's picture
  clicked and its controls by label). The picture itself is tested by
  `olr-game`'s GPU tests (`tests/preview_gpu.rs`).

Test: `cargo nextest run -p olr-material-editor`.
