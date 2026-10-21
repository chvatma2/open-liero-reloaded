# olr-mod-editor

The mod editor (issue #17; decisions in `docs/porting/STATUS.md`, "Mod
editor (issue #17)"): a mod's world, weapons and projectiles edited
through `olr_edit::mods`, as pure state a host drives, with egui panels.
`olr-game` hosts it (`olr-game/src/editor/mods.rs`), opened from the
Editor screen's Mod tab (`olr-ui`).

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.** Depends on `olr-data`,
  `olr-format` (the mod format, `ModEditorFile`, canonical RON),
  `olr-edit` (`mods`: get, set, weapons, shared projectiles, check;
  `pack::Pack`), `olr-pack` (`Severity`), `olr-vfs` and egui 0.36.2; not
  on `olr-ui`, `olr-game` or files (the host loads, saves and keeps the
  recovery copies). Not wasm. Tests use `egui_kittest`.
- **The pattern** (the skin editor's): `ModEditorState::update(ModAction)
  -> Vec<ModCommand>` is pure; `ui::show(ui, &state, &mut ModEditorData)`
  draws egui and returns actions (fields' commits first). Commands:
  `Save(pack)` (checked: no errors), `LoadMod(id)` (Add weapon's copies),
  `Play { pack, level, bots }`, `SavePlaySettings`, `Close`; answers
  `Saved`, `ModLoaded(id, pack)`, `PlayEnded`, `Failed(why)` (an error
  dialog), `Note`. Widgets are found by `ui::labels` and the field
  table's labels (`FieldInfo::label`, "Gravity (px/s²)"); a caption
  beside a widget is painted (`ui::caption`), so each name is one node.
- **The document** is the `Pack` itself (`state.rs`). Every change goes
  through `olr_edit::mods` and is one step: a field by `set_lenient` (a
  change that adds an error is kept and listed: the check,
  `mods::check`, runs after every change, undo and redo), a structural
  one by reading the node as its type, changing it and setting it whole
  (`edit`, `edit_spawn`). A refused change leaves the pack as it was and
  says why in the status. Save with errors shows the problems and
  refuses. `editor/mod.ron` (`ModEditorFile { base }`) names the mod it
  was copied from: the host passes that mod's source as `base`, whose
  values show beside the fields that differ, with Reset.
- **History** (`history.rs`): snapshots of the source and the name (and
  the other files when a weapon copied from another mod brought some),
  swapped on undo and redo; `MAX_STEPS` 200; the unsaved mark compares
  `History::at` with the step saved.
- **Tree** (`tree.rs`): `Item::{World, Weapon, Shared}` and the item's
  nodes by field path: the shot, its projectile, and under each
  projectile its event spawns, its trail's spawn and its timer; a spawn
  naming a shared projectile leads into it (a cycle shown once). Events
  (`Event`, `EventView`) are one effect plus whether they spawn (LX56
  runs one effect an event; extra ones are counted).
- **Operations**: weapons added (blank, or a copy from any installed mod
  or this one: `Dialog::AddWeapon`), removed, moved (a note, once, that
  the order is the weapons' ids), duplicated and renamed (a weapon's
  `name` set carries the presets); a fire's kind changed asks first
  (`Dialog::FireKind`); Make a copy for this weapon (`mods::inline`) and
  Make it shared (`mods::share`); presets' states, presets added and
  removed (with their loadout, one step), shipped loadouts.
- **Field table** (`fields.ron`, `fields.rs`): written by `cargo xtask
  format-docs` (keys, types, the format's doc comments as hover text);
  the unit, soft range, place (Common, More, Panel) and asset folder are
  edited by hand and kept. `check` fails when it drifts from the format.
  `ui/field.rs` draws a row from it: a number typed (any value: the
  format validates) with a slider over the soft range, a checkbox, a
  text, a dropdown of the mod's `gfx/` (with thumbnails) or `sfx/` files
  or of an enum's variants. The Look fields are left out (the art issue).
- **Play** (issue #18; STATUS.md "Mod editor Play (issue #18)"): the
  session bar's Play and its ▾ (the level editor's `play_menu`: the
  installed levels by name, a list that scrolls, and 0 to `MAX_BOTS`, 8,
  bots, buttons `labels::play_level`, `play_bots`). The host passes the
  levels and the remembered `PlaySettings { level, bots }` (`set_play`;
  `play_level()` is the one chosen when installed, else the first); a
  change sends `SavePlaySettings`. Play refreshes the check: with errors
  it shows the problems and says "Not played"; else `Play` carries the
  pack as it is, unsaved changes included, and the state is `playing()`
  (nothing but the host's answers acts) until `PlayEnded`. The state and
  the panels' data stay as they were (item, node, history, scroll: egui
  keeps the scroll while the host draws the match instead).
- **Panels** (`ui/`): session bar (Save, Close, Undo, Redo, the unsaved
  mark, the problems' count toggling their list; Play and Play ▾ at the
  right), the left list
  (`list.rs`), the tree, the node's panel (`panels.rs`: World, Weapon,
  Spawn, Projectile, Timer; common fields first, the rest under More ▸),
  the problems (each a link: `GoTo` shows its item and node and marks the
  field) and the dialogs (`dialogs.rs`). Ctrl+Z/Y/S when no field has the
  keyboard.
- **Tests** (`tests/`, one binary, on a copy of Classic from
  `OLR_CONTENT_DIR`; skipped without it): `table.rs`, `state.rs` and
  `flows.rs` (`egui_kittest`: gravity and the bazooka's speed, the shared
  shrapnel and its users, Make a copy, reorder and rename with presets,
  undo, redo, Reset, Save and reopen; an error listed, gone to, refusing
  the save; the fire's kind, an effect, a weapon copied from MW; issue
  #18's `play_the_unsaved_mod_and_come_back`, its test host drawing no
  editor while Play's match runs; `state.rs`'s
  `play_chooses_an_installed_level_and_waits_for_its_end`).

Test: `cargo nextest run -p olr-mod-editor`.
