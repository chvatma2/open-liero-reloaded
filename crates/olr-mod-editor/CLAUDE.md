# olr-mod-editor

The mod editor (issue #17; decisions in `docs/porting/STATUS.md`, "Mod
editor (issue #17)"): a mod's world, weapons and projectiles edited
through `olr_edit::mods`, as pure state a host drives, with egui panels.
`olr-game` hosts it (`olr-game/src/editor/mods.rs`), opened from the
Editor screen's Mod tab (`olr-ui`).

- **Layer 5, `std`, `#![forbid(unsafe_code)]`.** Depends on `olr-data`,
  `olr-format` (the mod format, `ModEditorFile`, canonical RON),
  `olr-edit` (`mods`: get, set, weapons, shared projectiles, check,
  `mods::art`'s assets; `pack::Pack`), `olr-pack` (`Severity`), `olr-vfs`,
  `olr-look` (`engine::MAX_EXPLOSION_RADIUS`), `olr-skin-editor` (the
  painting: `Frame`, `paint`, `Tool`, the zooms and glow strengths; a
  same-layer pair in `xtask/layers.ron`) and egui 0.36.2; not
  on `olr-ui`, `olr-game` or files (the host loads, saves and keeps the
  recovery copies). Not wasm. Tests use `egui_kittest`.
- **The pattern** (the skin editor's): `ModEditorState::update(ModAction)
  -> Vec<ModCommand>` is pure; `ui::show(ui, &state, &mut ModEditorData)`
  draws egui and returns actions (fields' commits first). Commands:
  `Save(pack)` (checked: no errors), `LoadMod(id)` (Add weapon's copies),
  `Play { pack, level, bots }`, `SavePlaySettings`, `Close`, `List(dir)`
  (the import browser's folder), `ReadFile { import, path }` (a file
  picked), `PlaySound { path, bytes }` (the Sounds section's ▶); answers
  `Saved`, `ModLoaded(id, pack)`, `PlayEnded`, `Listed { dir, entries }`,
  `FileRead { import, name, bytes }`, `Failed(why)` (an error dialog),
  `Note`. `set_browse_dir` is where the browser opens (the host's home).
  Widgets are found by `ui::labels` and the field table's labels
  (`FieldInfo::label`, "Gravity (px/s²)"); a caption beside a widget is painted (`ui::caption`), so each name is one node.
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
- **History** (`history.rs`): snapshots of the source and the name, and
  of the files a step changed (`Snapshot::keep_changed`: a weapon copied
  from another mod's, a stroke's image and mask, an import; created ones
  kept as `None`, so undo removes them), swapped on undo and redo;
  `MAX_STEPS` 200; the unsaved mark compares `History::at` with the step
  saved.
- **Tree** (`tree.rs`): `Item::{World, Weapon, Shared, Art(path),
  Sounds}` and the item's nodes by field path: the shot, its projectile,
  and under each projectile its event spawns, its trail's spawn and its timer; a spawn
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
  or of an enum's variants. The Look fields are the Look group's.
- **Art** (issue #19; STATUS.md "Mod editor art and sounds (issue #19)";
  `art.rs`, `assets.rs`, `ui/art.rs`): the list's Art section, each
  `gfx/` image with its thumbnail and "unused"; an image opens as
  `Item::Art(path)`: `ArtDoc` (the strip and its mask as one
  `olr_skin_editor::Frame`, square frames of the image's height) edited
  with the skin editor's tools (`ArtTools`: pencil, eraser, fill,
  eyedropper, line, rectangle, the targets Colour and Glow at
  `GLOW_STRENGTHS`, recent colours, `ZOOMS`, the pixel grid from
  `GRID_ZOOM`, the onion skin of the frame before) on one frame of the
  frames column (copy, paste, add after, remove: a frame count change
  asks first, `Dialog::FrameCount`, and says Play is the check). A
  stroke is one history step (`commit_art`: `mods::art::set_image` when
  the colour changed, `set_glow` when the mask did; the mask is
  `gfx/<name>.glow.png`, removed when all zero). The header: Rename…,
  Copy…, Replace with PNG…, Remove (refused while used: an error dialog
  listing the users), Used by. The preview (`art::preview_frame`,
  `art::dark`): the frames as the first user plays them (its animation,
  its heading frames, else every frame) at `PREVIEW_FPS`, over the night
  with the glow bright, timed by egui's clock (no repaint asked: the
  game draws every frame). New image (`Dialog::NewImage`: blank of a
  side and frame count, or a copy) and Import PNG… (`browser.rs`'s
  `Browser`, `Dialog::Browse`: the host lists folders and reads the
  picked file; a name taken gets a number, `free_path`).
- **Sounds** (`ui/sounds.rs`): every `sfx/` file with ▶ (`PlaySound`),
  its size, users, Rename… and Remove (refused while used); Import
  sound… (WAV or Ogg, checked as the game loads them).
- **Look** (`ui/look.rs`): the projectile panel's Look group (`glow`,
  `light`, `end_flash`, `explosion`) and the weapon's (`muzzle_light`),
  each set whole as `<path>.look` (a look all default is `None`).
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
  `play_chooses_an_installed_level_and_waits_for_its_end`) and `art.rs`
  (issue #19: a frame painted and saved changes the PNG only there, the
  mask round-trips, undo back to the bytes; new images, frames, renames
  and imports through the browser; removal refused while used; sounds
  imported, played and removed; the Look group's lights); the test host
  answers `List` and `ReadFile` from the disk.

Test: `cargo nextest run -p olr-mod-editor`.
